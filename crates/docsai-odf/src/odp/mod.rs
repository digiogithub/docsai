//! The `.odp` reader (Phase 18, increment 18-B).
//!
//! A `draw:page` and a `p:sld` are the same idea in two dialects, and the IR is
//! the OOXML dialect made general. This module is where the two are reconciled,
//! and the reconciliation turns on two questions the phase plan
//! (`kb/94-phase-18-plan.md`) posed and this increment answers. Both answers are
//! argued at length in `kb/98-phase-18-odp-reader.md`; what follows is what a
//! reader of this file needs to know to read it.
//!
//! **`presentation:class` versus `p:ph`.** ODF names a frame's *role* out of a
//! closed vocabulary; PresentationML names a *type* **and an index**. So the
//! mapping bends on the ODF side: every class becomes a [`PhType`], with
//! [`PhType::Other`] — the IR's verbatim-keyword hatch — carrying the two
//! classes PresentationML has no distinct word for (`text`, `page`). What ODF
//! cannot state is the index, and two frames of one class on one page are
//! distinguishable in ODF by document order alone. The reader gives the first
//! no index (which is what the file says) and every later one an ordinal, and
//! **says so with a warning**: the number is document order made explicit, it
//! is not in the file, and a placeholder whose `idx` no layout declares cannot
//! resolve against the cascade. A silent collision was the one outcome the plan
//! ruled out.
//!
//! **Master pages versus layout + master.** ODF is usually described as having
//! one level above a page where OOXML has two. It has two references, they are
//! simply not nested: `draw:master-page-name` names a `style:master-page`,
//! which carries the placeholder *styles*, and
//! `presentation:presentation-page-layout-name` names a
//! `style:presentation-page-layout`, which carries the placeholder
//! *rectangles* and nothing else. Those are precisely the IR's [`Master`] and
//! [`Layout`]. The one thing ODF has that the IR does not is that the pair is
//! chosen per page rather than fixed: a page layout used under two different
//! master pages is legal ODF and is one [`Layout`] in the IR, so the second
//! pairing is warned rather than silently overwritten.
//!
//! **Reading order comes from the canvas.** The pptx reader orders body
//! placeholders by `p:ph@idx`, because that is the order the layout intended.
//! ODF has no index to intend anything with, so within a rank this reader
//! orders by position — row band, then left to right — which is the same policy
//! the pptx reader applies to everything that is not a placeholder. Every shape
//! still keeps its document-order index in [`Shape::z_index`], so the ordering
//! is reversible, which is the only reason it is allowed to exist.
//!
//! **De-automatization is unchanged.** A frame's `presentation:style-name` is
//! resolved through [`OdfStyles::resolve_presentation`] and stored as a delta
//! over the master placeholder of the same class — never carried as a style
//! name. That is `styles.rs`'s existing discipline applied to the one style
//! family `.odt` and `.ods` never needed.

mod chart;
pub(crate) mod skeleton;
pub(crate) mod write;

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::image::{ImageGeometry, ImageRef, RawId};
use docsai_model::presentation::{
    Layout, LayoutCatalog, LayoutId, LayoutPlaceholder, Master, MasterId, PhType, Placeholder,
    Presentation, RawShape, RawShapeKind, Shape, ShapeGeometry, ShapeKind, ShapeProps, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::FontProps;
use docsai_model::text::{Block, Comment, CommentAnchor, RawFragment};
use docsai_model::units::{Length, Point, Size};
use docsai_model::Document;

use crate::error::ReadError;
use crate::length::parse_length;
use crate::odt::body::{read_block, read_blocks, read_table, Ctx, State};
use crate::package::Package;
use crate::styles::{read_automatic_styles, read_named_styles, OdfStyles};
use crate::xml::Element;

const CONTENT_PART: &str = "content.xml";
const STYLES_PART: &str = "styles.xml";
pub(crate) const MIME: &str = "application/vnd.oasis.opendocument.presentation";

/// Default page size when no `style:page-layout` says otherwise: ODF's own
/// default presentation page, 28 × 21 cm.
const DEFAULT_PAGE: (f64, f64) = (28.0, 21.0);

/// Two shapes whose tops differ by less than this are on the same row and are
/// read left to right. An eighth of an inch, the same band the pptx reader
/// uses, and for the same reason: boxes meant to line up rarely agree exactly.
const ROW_BAND_EMU: i64 = 114_300;

/// Cap on the bytes pulled into memory ahead of the ZIP directory, which is
/// where the skeleton's copy of the package comes from.
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

/// Reads a `.odp` presentation into the IR.
///
/// The stream is read once, into memory: the same bytes are both the package to
/// decompress and the [`skeleton`](crate::odp::skeleton) the writer splices
/// into. Handing [`Package::open`] a reader and then asking for the original
/// again would mean reading the deck twice.
pub fn read<R: Read + Seek>(
    mut reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    // Capped here, because reading ahead of `Package::open` moves the first
    // size check to this line: without it a file that is not even a ZIP would
    // be pulled into memory before anything rejected it.
    let mut original = Vec::new();
    let read = (&mut reader)
        .take(MAX_PACKAGE_BYTES.saturating_add(1))
        .read_to_end(&mut original)?;
    if read as u64 > MAX_PACKAGE_BYTES {
        return Err(ReadError::TooLarge(format!(
            "the package is larger than {MAX_PACKAGE_BYTES} bytes"
        )));
    }
    let package = Package::open(std::io::Cursor::new(&original))?;
    read_package(&package, Some(&original), assets)
}

/// `original` is the undecompressed package, when the caller has it: it is what
/// the skeleton preserves. `None` — a package assembled in memory, as the tests
/// do — reads into a deck with no skeleton, which is honest rather than a
/// skeleton reconstructed from parts that have already lost their ZIP order.
pub(crate) fn read_package(
    package: &Package,
    original: Option<&[u8]>,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    if !package.has_part(CONTENT_PART) {
        return Err(ReadError::MissingPart(CONTENT_PART.into()));
    }
    if let Some(mime) = package.part("mimetype") {
        let mime = std::str::from_utf8(mime).unwrap_or("").trim();
        if !mime.is_empty() && !mime.contains("opendocument.presentation") {
            return Err(ReadError::WrongShape {
                part: "mimetype".into(),
                expected: MIME.into(),
            });
        }
    }

    let mut report = ConversionReport::new();

    // A `.odp` has no macro-enabled MIME type: a Basic project lives inside an
    // ordinary package. It is never executed and never carried across, exactly
    // as `.docm` and `.xlsm` already are (AGENTS.md §7).
    if let Some(part) = package
        .part_names()
        .find(|name| name.starts_with("Basic/") && name.ends_with(".xml"))
    {
        report.warn(Warning::MacrosIgnored {
            part: part.to_string(),
        });
    }

    let mut styles = OdfStyles::default();
    let styles_source = package.part(STYLES_PART).map(|_| package.text(STYLES_PART));
    let styles_source = match styles_source {
        Some(Ok(text)) => Some(text),
        Some(Err(e)) => return Err(e),
        None => None,
    };
    let styles_root = match styles_source {
        Some(text) => Some(Element::parse(STYLES_PART, text.as_bytes())?),
        None => None,
    };
    if let Some(root) = &styles_root {
        read_named_styles(root, &mut styles);
        if let Some(auto) = root.child("automatic-styles") {
            read_automatic_styles(auto, &mut styles);
        }
    }

    let content_source = package.text(CONTENT_PART)?;
    let content_root = Element::parse(CONTENT_PART, content_source.as_bytes())?;
    if let Some(auto) = content_root.child("automatic-styles") {
        read_automatic_styles(auto, &mut styles);
    }

    // Where a `style:style` of family `drawing-page` is written, so that a page
    // transition can be preserved as the bytes it was written as rather than as
    // a re-serialisation of the tree.
    let mut drawing_pages: BTreeMap<&str, StyleSource<'_>> = BTreeMap::new();
    if let (Some(root), Some(text)) = (&styles_root, styles_source) {
        index_drawing_pages(root, STYLES_PART, text, &mut drawing_pages);
    }
    index_drawing_pages(
        &content_root,
        CONTENT_PART,
        content_source,
        &mut drawing_pages,
    );

    // Masters before page layouts, page layouts before pages: a page's frames
    // are read as a delta over what the master placeholder of the same class
    // already decided, so the reference must exist before the delta.
    let masters = read_masters(styles_root.as_ref(), &styles, &mut report);
    let page_layouts = read_page_layouts(styles_root.as_ref(), &content_root);
    let slide_size = read_page_size(styles_root.as_ref(), &masters);

    let mut catalog = LayoutCatalog::default();
    for master in &masters {
        catalog
            .masters
            .insert(master.id.clone(), master.model.clone());
    }

    let presentation_body = content_root
        .path(&["body", "presentation"])
        .ok_or_else(|| ReadError::WrongShape {
            part: CONTENT_PART.into(),
            expected: "OpenDocument presentation".into(),
        })?;

    let mut sink = Sink::default();
    let mut slides = Vec::new();
    for page in presentation_body.children_named("page") {
        let slide = read_page(
            page,
            &PageCtx {
                package,
                source: content_source,
                styles: &styles,
                masters: &masters,
                drawing_pages: &drawing_pages,
            },
            &page_layouts,
            &mut catalog,
            assets,
            &mut sink,
            &mut report,
        )?;
        report.stats.slides = report.stats.slides.saturating_add(1);
        slides.push(slide);
    }

    report.stats.styles = styles.catalog.styles.len() as u32;

    let presentation = Presentation {
        meta: crate::odt::read_meta(package),
        addressing: Default::default(),
        styles: styles.catalog,
        layouts: catalog,
        slide_size,
        slides,
        // The whole package, byte for byte, so the writer splices into it
        // rather than regenerating masters, page layouts and everything else
        // this reader never looked at (18-D).
        skeleton: original.and_then(|bytes| skeleton::capture(bytes, assets, &mut report)),
        raw: sink.fragments,
    };
    Ok((Document::Presentation(presentation), report))
}

// ---------------------------------------------------------------------------
// The class ↔ placeholder-type mapping
// ---------------------------------------------------------------------------

/// `presentation:class` → [`PhType`].
///
/// Eleven of ODF's classes name something PresentationML also names, and those
/// map straight across. Two do not, and they keep their ODF keyword in
/// [`PhType::Other`] rather than being folded into the nearest neighbour:
///
/// * **`text`.** PresentationML has one body type where ODF has `outline` and
///   `text`. Collapsing `text` into [`PhType::Body`] would lose the distinction
///   on every read *and* manufacture an index collision on every page that
///   carries both — the very thing the reader is trying not to do. `Other`
///   keeps the keyword, which is what `Other` is for.
/// * **`page`.** The slide thumbnail on a notes page. PresentationML spells it
///   `sldImg`, which is not in [`PhType`] either — but it *is* a
///   PresentationML keyword, and [`PhType::Other`] is documented as holding
///   one. 18-C settled it there: DocMark-P writes the placeholder type into
///   `type=`, and a construct both dialects name would otherwise reach the
///   same field spelled two ways, which is precisely what normalising
///   `chart:class` to the PresentationML plot name avoids. So `page` is read
///   as `Other("sldImg")` and `text`, which PresentationML does not name at
///   all, keeps its own word.
fn ph_type_of(class: &str) -> PhType {
    match class {
        "title" => PhType::Title,
        "subtitle" => PhType::Subtitle,
        "outline" => PhType::Body,
        "object" => PhType::Object,
        "chart" => PhType::Chart,
        "table" => PhType::Table,
        "graphic" => PhType::Picture,
        "orgchart" => PhType::Diagram,
        "header" => PhType::Header,
        "footer" => PhType::Footer,
        "date-time" => PhType::DateTime,
        "page-number" => PhType::SlideNumber,
        "page" => PhType::Other("sldImg".to_string()),
        other => PhType::Other(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Masters and page layouts
// ---------------------------------------------------------------------------

/// A `style:master-page` as both the IR sees it and the cascade needs it.
struct MasterInfo {
    id: MasterId,
    model: Master,
    /// The resolved font of each class the master declares — the reference a
    /// page's frame measures its delta against.
    fonts: BTreeMap<String, FontProps>,
    /// `style:page-layout-name`, which is where the page size lives.
    page_layout: Option<String>,
}

fn read_masters(
    styles_root: Option<&Element>,
    styles: &OdfStyles,
    report: &mut ConversionReport,
) -> Vec<MasterInfo> {
    let mut out = Vec::new();
    let Some(root) = styles_root else {
        report.warn(Warning::Degraded {
            what: "the deck".into(),
            why: "no styles.xml: its pages have no master to inherit from".into(),
        });
        return out;
    };
    let Some(master_styles) = root.child("master-styles") else {
        return out;
    };
    for master in master_styles.children_named("master-page") {
        let Some(name) = master.attr("name") else {
            continue;
        };
        let mut fonts = BTreeMap::new();
        let mut placeholders = Vec::new();
        for frame in master.children_named("frame") {
            let Some(class) = frame.attr_qualified("presentation:class") else {
                continue;
            };
            let style = frame
                .attr_qualified("presentation:style-name")
                .or_else(|| frame.attr_qualified("draw:style-name"));
            let (font, _) = styles.resolve_presentation(style);
            fonts.insert(class.to_string(), font.clone());
            placeholders.push(LayoutPlaceholder {
                ph_type: ph_type_of(class),
                idx: None,
                geometry: frame_geometry(frame),
                props: ShapeProps {
                    font,
                    ..Default::default()
                },
            });
        }
        out.push(MasterInfo {
            id: MasterId::new(name.to_string()),
            model: Master {
                name: name.to_string(),
                // ODF has no theme part: a master page carries its own styles,
                // so there is no reference to keep and nothing is lost.
                theme: None,
                placeholders,
            },
            fonts,
            page_layout: master.attr("page-layout-name").map(str::to_string),
        });
    }
    out
}

/// The `style:presentation-page-layout` definitions, by name.
///
/// A page layout is a naked list of placeholder rectangles with no style of its
/// own, which is exactly the IR's [`Layout`]: positions, inherited from a
/// master. So [`LayoutPlaceholder::props`] is empty here on purpose — a layout
/// that stated a font would be stating one the file does not.
fn read_page_layouts(
    styles_root: Option<&Element>,
    content_root: &Element,
) -> BTreeMap<String, Vec<LayoutPlaceholder>> {
    let mut out = BTreeMap::new();
    let mut collect = |container: Option<&Element>| {
        for layout in container
            .into_iter()
            .flat_map(|c| c.children_named("presentation-page-layout"))
        {
            let Some(name) = layout.attr("name") else {
                continue;
            };
            let placeholders = layout
                .children_named("placeholder")
                .map(|slot| LayoutPlaceholder {
                    ph_type: ph_type_of(slot.attr("object").unwrap_or_default()),
                    idx: None,
                    geometry: frame_geometry(slot),
                    props: ShapeProps::default(),
                })
                .collect();
            out.insert(name.to_string(), placeholders);
        }
    };
    if let Some(root) = styles_root {
        collect(root.child("styles"));
        collect(root.child("automatic-styles"));
    }
    collect(content_root.child("styles"));
    collect(content_root.child("automatic-styles"));
    out
}

/// The canvas every page's geometry is relative to: `fo:page-width` and
/// `fo:page-height` of the `style:page-layout` the first master page names.
fn read_page_size(styles_root: Option<&Element>, masters: &[MasterInfo]) -> Size {
    let default = Size::new(
        Length::from_cm(DEFAULT_PAGE.0),
        Length::from_cm(DEFAULT_PAGE.1),
    );
    let Some(root) = styles_root else {
        return default;
    };
    let wanted = masters.iter().find_map(|m| m.page_layout.as_deref());
    let layouts = root.child("automatic-styles");
    let props = layouts
        .into_iter()
        .flat_map(|auto| auto.children_named("page-layout"))
        .find(|layout| match wanted {
            Some(name) => layout.attr("name") == Some(name),
            None => true,
        })
        .and_then(|layout| layout.child("page-layout-properties"));
    match props {
        Some(props) => {
            let width = props.attr("page-width").and_then(parse_length);
            let height = props.attr("page-height").and_then(parse_length);
            match (width, height) {
                (Some(w), Some(h)) => Size::new(w, h),
                _ => default,
            }
        }
        None => default,
    }
}

/// Where a `style:style` of family `drawing-page` is written.
struct StyleSource<'a> {
    part: &'a str,
    source: &'a str,
    element: &'a Element,
}

fn index_drawing_pages<'a>(
    root: &'a Element,
    part: &'a str,
    source: &'a str,
    into: &mut BTreeMap<&'a str, StyleSource<'a>>,
) {
    for container in ["automatic-styles", "styles"] {
        for style in root
            .child(container)
            .into_iter()
            .flat_map(|c| c.children_named("style"))
        {
            if style.attr("family") != Some("drawing-page") {
                continue;
            }
            if let Some(name) = style.attr("name") {
                into.insert(
                    name,
                    StyleSource {
                        part,
                        source,
                        element: style,
                    },
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw preservation
// ---------------------------------------------------------------------------

/// Collects the deck's raw fragments and hands out their ids.
///
/// The same three rules the pptx reader enforces (`kb/43-phase-13-raw.md`): a
/// visible stub, the markup verbatim as a slice of the source part, and a typed
/// warning. `seq` is shared with the block reader's own counter so that a
/// fragment inside a paragraph and a fragment behind a shape stub never claim
/// the same id.
#[derive(Debug, Default)]
struct Sink {
    seq: u32,
    fragments: Vec<RawFragment>,
}

impl Sink {
    fn capture(
        &mut self,
        element: &Element,
        part: &str,
        source: &str,
        report: &mut ConversionReport,
    ) -> RawId {
        self.seq += 1;
        let id = RawId::new(format!("raw-{:04}", self.seq));
        report.raw_block(qualified(element), format!("{part}:{}", element.span.start));
        self.fragments.push(RawFragment {
            id: id.clone(),
            format: "odf".into(),
            part: part.to_string(),
            content: element.raw(source).to_string(),
        });
        id
    }

    fn shape(
        &mut self,
        element: &Element,
        kind: RawShapeKind,
        z_index: u32,
        part: &str,
        source: &str,
        report: &mut ConversionReport,
    ) -> Shape {
        let raw = self.capture(element, part, source, report);
        Shape {
            id: None,
            name: element.attr_qualified("draw:name").map(str::to_string),
            z_index,
            geometry: frame_geometry(element),
            kind: ShapeKind::Raw(RawShape {
                kind,
                raw: Some(raw),
                // The label of an arrow travels on the stub as well as inside
                // the payload: a stub that swallowed it would hide real text
                // behind bytes no agent is meant to read.
                text: element.deep_text().trim().to_string(),
            }),
        }
    }
}

fn qualified(element: &Element) -> String {
    if element.prefix.is_empty() {
        element.name.clone()
    } else {
        format!("{}:{}", element.prefix, element.name)
    }
}

// ---------------------------------------------------------------------------
// One page
// ---------------------------------------------------------------------------

struct PageCtx<'a> {
    package: &'a Package,
    source: &'a str,
    styles: &'a OdfStyles,
    masters: &'a [MasterInfo],
    drawing_pages: &'a BTreeMap<&'a str, StyleSource<'a>>,
}

impl PageCtx<'_> {
    fn master(&self, name: Option<&str>) -> Option<&MasterInfo> {
        let name = name?;
        self.masters.iter().find(|m| m.id.as_str() == name)
    }
}

#[allow(clippy::too_many_arguments)]
fn read_page(
    page: &Element,
    ctx: &PageCtx<'_>,
    page_layouts: &BTreeMap<String, Vec<LayoutPlaceholder>>,
    catalog: &mut LayoutCatalog,
    assets: &mut dyn AssetStore,
    sink: &mut Sink,
    report: &mut ConversionReport,
) -> Result<Slide, ReadError> {
    let page_name = page.attr_qualified("draw:name").map(str::to_string);
    let master_name = page.attr_qualified("draw:master-page-name");
    let master = ctx.master(master_name);
    if master.is_none() {
        report.warn(Warning::Degraded {
            what: format!("page `{}`", page_name.clone().unwrap_or_default()),
            why: "names no master page: its frames have nothing to inherit from".into(),
        });
    }

    // The two references above a page, and the pair is what decides its
    // placeholders. The IR nests them, so a page layout reached under a second
    // master is a shape ODF has and the IR does not.
    let layout_name = page.attr_qualified("presentation:presentation-page-layout-name");
    let layout = match layout_name {
        Some(name) => {
            let id = LayoutId::new(name.to_string());
            let master_id = master.map(|m| m.id.clone());
            match catalog.layouts.get(&id) {
                Some(existing) if existing.master != master_id => {
                    report.warn(Warning::Degraded {
                        what: format!("presentation page layout `{name}`"),
                        why: format!(
                            "it is used under master page `{}` and also under `{}`; \
                             the IR nests a layout under one master, so the first pairing wins",
                            existing
                                .master
                                .as_ref()
                                .map(MasterId::as_str)
                                .unwrap_or("none"),
                            master_id.as_ref().map(MasterId::as_str).unwrap_or("none"),
                        ),
                    });
                }
                Some(_) => {}
                None => {
                    catalog.layouts.insert(
                        id.clone(),
                        Layout {
                            name: name.to_string(),
                            master: master_id,
                            placeholders: page_layouts.get(name).cloned().unwrap_or_default(),
                        },
                    );
                }
            }
            Some(id)
        }
        None => {
            report.warn(Warning::Degraded {
                what: format!("page `{}`", page_name.clone().unwrap_or_default()),
                why: "no presentation page layout: its placeholders have no rectangles to \
                      resolve against"
                    .into(),
            });
            None
        }
    };

    // `presentation:transition-*` sits on the page's drawing-page style, not on
    // the page: it is a property of a style the page names. There is no IR node
    // for it, so it is preserved as the markup it was written as.
    let page_style = page.attr_qualified("draw:style-name");
    let mut slide_raw: Vec<RawId> = Vec::new();
    let mut hidden = false;
    if let Some(style) = page_style.and_then(|name| ctx.drawing_pages.get(name)) {
        if let Some(props) = style.element.child("drawing-page-properties") {
            hidden = props.attr("visibility") == Some("hidden");
            if props
                .attrs
                .iter()
                .any(|(name, _)| local_of(name).starts_with("transition-"))
            {
                slide_raw.push(sink.capture(style.element, style.part, style.source, report));
            }
        }
    }

    let body_ctx = Ctx {
        package: ctx.package,
        part: CONTENT_PART,
        source: ctx.source,
        styles: ctx.styles,
    };

    let mut shapes = Vec::new();
    let mut z_index = 0u32;
    let mut comments = Vec::new();
    let mut notes: Option<Vec<Block>> = None;
    // How many frames of each placeholder type this page has seen: the only
    // thing ODF gives the reader to tell two of a kind apart.
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    let mut stated_z: Vec<(i64, u32)> = Vec::new();

    for child in page.children() {
        match child.name.as_str() {
            // `officeooo:annotation` under `draw:page` is where every `.odp`
            // with comments in it actually puts them: ODF 1.3's own
            // `office:annotation` has no placement that is both grammar-valid
            // and readable by LibreOffice (18-A, defect 4). The reader matches
            // on the local name, so both spellings arrive here.
            "annotation" => {
                comments.push(read_comment(child, &body_ctx, assets, sink, report));
            }
            "notes" => {
                notes = Some(read_notes(child, &body_ctx, assets, sink, report));
            }
            "forms" | "layer-set" => {}
            name if is_animation(name, child) => {
                slide_raw.push(sink.capture(child, CONTENT_PART, ctx.source, report));
            }
            _ => {
                if let Some(stated) = child
                    .attr_qualified("draw:z-index")
                    .and_then(|v| v.parse::<i64>().ok())
                {
                    stated_z.push((stated, z_index));
                }
                shapes.push(read_shape(
                    child, z_index, ctx, &body_ctx, master, &mut seen, assets, sink, report,
                )?);
                z_index += 1;
            }
        }
    }

    // ODF states the stacking order explicitly where OOXML leaves it implicit
    // in `p:spTree` order, so the two can disagree — and the IR has one field
    // for both. Document order is what it keeps, because that is what makes the
    // reading-order policy reversible; when `draw:z-index` says something
    // different, that difference is the loss and it is named.
    let mut by_stated = stated_z.clone();
    by_stated.sort_by_key(|(stated, index)| (*stated, *index));
    if by_stated
        .iter()
        .map(|(_, index)| *index)
        .ne(stated_z.iter().map(|(_, i)| *i))
    {
        report.warn(Warning::Degraded {
            what: format!(
                "draw:z-index on page `{}`",
                page_name.clone().unwrap_or_default()
            ),
            why: "the stacking order it states differs from document order, and a shape has one \
                  index: document order is kept, because it is what makes the reading order \
                  reversible"
                .into(),
        });
    }

    shapes.sort_by_key(reading_key);

    for warning in collisions(&seen, page_name.as_deref()) {
        report.warn(warning);
    }

    Ok(Slide {
        id: None,
        layout,
        name: page_name,
        shapes,
        notes,
        comments,
        hidden,
        // ODF has no counterpart of `p14:sectionLst`: a deck is a flat list of
        // pages, so there is no section to name and nothing is dropped.
        section: None,
        raw: slide_raw,
    })
}

fn local_of(qualified: &str) -> &str {
    qualified.split_once(':').map_or(qualified, |(_, n)| n)
}

/// `anim:par`, `anim:seq` and the rest of the animation tree. Matched on the
/// prefix as well as the name because `par` and `seq` are generic words.
fn is_animation(name: &str, element: &Element) -> bool {
    element.prefix == "anim"
        || matches!(
            name,
            "par" | "seq" | "iterate" | "animate" | "set" | "transitionFilter"
        )
}

/// The warnings the class-to-index mapping owes the reader.
fn collisions(seen: &BTreeMap<String, u32>, page: Option<&str>) -> Vec<Warning> {
    seen.iter()
        .filter(|(_, count)| **count > 1)
        .map(|(class, count)| Warning::Degraded {
            what: format!(
                "{count} `{class}` placeholders on page `{}`",
                page.unwrap_or("")
            ),
            why: "ODF distinguishes frames of one presentation:class by document order alone, \
                  so every one after the first carries an index this reader assigned and the \
                  file does not state; no layout declares it, so it does not resolve against \
                  the cascade"
                .into(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// One shape
// ---------------------------------------------------------------------------

/// Drawing elements the IR has no node for, and the stub each one gets.
///
/// `draw:g` is stubbed whole, children included, exactly as `p:grpSp` is: the
/// model has a group variant this reader does not fill, and reading a group's
/// children means reading a second nested cascade for no gain a stub carrying
/// the group's text and its markup does not already give.
fn stub_kind(name: &str) -> Option<RawShapeKind> {
    Some(match name {
        "g" => RawShapeKind::Shape,
        "line" | "connector" | "measure" => RawShapeKind::Connector,
        "rect" | "ellipse" | "circle" | "polygon" | "polyline" | "path" | "regular-polygon"
        | "caption" | "custom-shape" => RawShapeKind::Shape,
        "plugin" | "applet" | "floating-frame" => RawShapeKind::Media,
        "control" | "page-thumbnail" => RawShapeKind::Other,
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn read_shape(
    element: &Element,
    z_index: u32,
    ctx: &PageCtx<'_>,
    body_ctx: &Ctx<'_>,
    master: Option<&MasterInfo>,
    seen: &mut BTreeMap<String, u32>,
    assets: &mut dyn AssetStore,
    sink: &mut Sink,
    report: &mut ConversionReport,
) -> Result<Shape, ReadError> {
    if element.name == "frame" {
        return read_frame(
            element, z_index, ctx, body_ctx, master, seen, assets, sink, report,
        );
    }
    let kind = stub_kind(&element.name).unwrap_or(RawShapeKind::Other);
    let mut shape = sink.shape(element, kind, z_index, CONTENT_PART, ctx.source, report);
    // `draw:type` names an ODF preset the way `a:prstGeom@prst` names an OOXML
    // one; without it a stub says «a shape», with it, «a right arrow».
    let named_preset = match element.name.as_str() {
        "rect" | "ellipse" | "circle" | "line" | "polygon" | "polyline" => {
            Some(element.name.as_str())
        }
        _ => None,
    };
    if let Some(preset) = element
        .child("enhanced-geometry")
        .and_then(|geom| geom.attr_qualified("draw:type"))
        .or(named_preset)
    {
        shape.geometry.preset = Some(preset.to_string());
    }
    Ok(shape)
}

#[allow(clippy::too_many_arguments)]
fn read_frame(
    frame: &Element,
    z_index: u32,
    ctx: &PageCtx<'_>,
    body_ctx: &Ctx<'_>,
    master: Option<&MasterInfo>,
    seen: &mut BTreeMap<String, u32>,
    assets: &mut dyn AssetStore,
    sink: &mut Sink,
    report: &mut ConversionReport,
) -> Result<Shape, ReadError> {
    let geometry = frame_geometry(frame);
    let name = frame.attr_qualified("draw:name").map(str::to_string);
    let class = frame.attr_qualified("presentation:class");

    // An OLE object is looked for first, because the frame that holds one also
    // holds the replacement bitmap the application drew for it. Reading that
    // bitmap as the frame's picture would replace an object with a thumbnail of
    // it, quietly, and the object would be gone.
    if frame.child("object-ole").is_some() {
        let mut shape = sink.shape(
            frame,
            RawShapeKind::Ole,
            z_index,
            CONTENT_PART,
            ctx.source,
            report,
        );
        shape.name = name;
        shape.geometry = geometry;
        return Ok(shape);
    }

    if let Some(object) = frame.child("object") {
        if let Some(chart) = read_chart(object, ctx, sink, report)? {
            return Ok(Shape {
                id: None,
                name,
                z_index,
                geometry,
                kind: ShapeKind::Chart(chart),
            });
        }
        let mut shape = sink.shape(
            frame,
            RawShapeKind::Ole,
            z_index,
            CONTENT_PART,
            ctx.source,
            report,
        );
        shape.name = name;
        shape.geometry = geometry;
        return Ok(shape);
    }

    if let Some(table) = frame.child("table") {
        let mut state = State {
            assets,
            report,
            raw_seq: sink.seq,
        };
        let table = read_table(table, body_ctx, &mut state);
        sink.seq = state.raw_seq;
        return Ok(Shape {
            id: None,
            name,
            z_index,
            geometry,
            kind: ShapeKind::Table(table),
        });
    }

    if let Some(image) = frame.child("image") {
        if let Some(picture) = read_picture(frame, image, ctx, assets, report)? {
            if let Some(class) = class {
                report.warn(Warning::Degraded {
                    what: name.clone().unwrap_or_else(|| "a picture".into()),
                    why: format!(
                        "it fills the `{class}` presentation slot, which a picture cannot carry"
                    ),
                });
            }
            return Ok(Shape {
                id: None,
                name: picture.name.clone().or(name),
                z_index,
                geometry,
                kind: ShapeKind::Picture(picture),
            });
        }
        let mut shape = sink.shape(
            frame,
            RawShapeKind::Other,
            z_index,
            CONTENT_PART,
            ctx.source,
            report,
        );
        shape.name = name;
        shape.geometry = geometry;
        return Ok(shape);
    }

    let Some(text_box) = frame.child("text-box") else {
        let mut shape = sink.shape(
            frame,
            stub_kind(&frame.name).unwrap_or(RawShapeKind::Other),
            z_index,
            CONTENT_PART,
            ctx.source,
            report,
        );
        shape.name = name;
        shape.geometry = geometry;
        return Ok(shape);
    };

    let mut state = State {
        assets,
        report,
        raw_seq: sink.seq,
    };
    let body = read_blocks(text_box, body_ctx, &mut state);
    sink.seq = state.raw_seq;

    let style = frame
        .attr_qualified("presentation:style-name")
        .or_else(|| frame.attr_qualified("draw:style-name"));

    let kind = match class {
        Some(class) => {
            let ph_type = ph_type_of(class);
            let count = seen.entry(class.to_string()).or_default();
            *count += 1;
            // The first frame of a class carries no index, which is what the
            // file says. Every later one carries an ordinal, which is what
            // makes it addressable — and what the collision warning names.
            let idx = (*count > 1).then(|| *count - 1);
            let (font, _) = ctx.styles.resolve_presentation(style);
            let inherited = master
                .and_then(|m| m.fonts.get(class))
                .cloned()
                .unwrap_or_default();
            Placeholder {
                ph_type,
                idx,
                body,
                delta: ShapeProps {
                    font: font.minus(&inherited),
                    fill: ctx.styles.presentation_fill(style),
                    ..Default::default()
                },
            }
            .into_kind()
        }
        None => ShapeKind::TextBox { body },
    };

    Ok(Shape {
        id: None,
        name,
        z_index,
        geometry,
        kind,
    })
}

trait IntoKind {
    fn into_kind(self) -> ShapeKind;
}

impl IntoKind for Placeholder {
    fn into_kind(self) -> ShapeKind {
        ShapeKind::Placeholder(self)
    }
}

/// A `draw:image` inside a frame, as the normalised image model.
///
/// The picture's *placement* is the shape's geometry, read once from the frame;
/// what travels on the [`ImageRef`] is what belongs to the bitmap. The same
/// division the pptx reader makes, and for the same reason: a position written
/// twice is a position that disagrees with itself later.
fn read_picture(
    frame: &Element,
    image: &Element,
    ctx: &PageCtx<'_>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Option<ImageRef>, ReadError> {
    let Some(href) = image
        .attr_qualified("xlink:href")
        .or_else(|| image.attr("href"))
    else {
        return Ok(None);
    };

    let display = Size::new(
        frame
            .attr_qualified("svg:width")
            .and_then(parse_length)
            .unwrap_or(Length::ZERO),
        frame
            .attr_qualified("svg:height")
            .and_then(parse_length)
            .unwrap_or(Length::ZERO),
    );
    let mut geometry = ImageGeometry::inline(display);

    let title = frame
        .child("title")
        .map(|t| t.deep_text())
        .filter(|t| !t.is_empty());
    let desc = frame
        .child("desc")
        .map(|d| d.deep_text())
        .filter(|d| !d.is_empty());
    let name = frame.attr_qualified("draw:name").map(str::to_string);

    if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("file:") {
        report.warn(Warning::ExternalImageNotFetched {
            url: href.to_string(),
        });
        return Ok(None);
    }

    let Some(part) = ctx.package.resolve_href(CONTENT_PART, href) else {
        return Ok(None);
    };
    let Some(bytes) = ctx.package.part(&part) else {
        report.warn(Warning::AssetIssue {
            asset: href.to_string(),
            why: "the package has no such part".into(),
        });
        return Ok(None);
    };
    let asset = match assets.put(bytes) {
        Ok(id) => id,
        Err(e) => {
            report.warn(Warning::AssetIssue {
                asset: href.to_string(),
                why: e.to_string(),
            });
            return Ok(None);
        }
    };
    if let Some(info) = assets.info(&asset) {
        geometry.native_size_px = info.native_size_px;
    }
    report.stats.images += 1;

    let mut picture = ImageRef::new(asset, geometry);
    picture.alt = desc.or_else(|| title.clone()).unwrap_or_default();
    picture.title = title;
    picture.name = name;
    Ok(Some(picture))
}

/// A `draw:object` pointing at an embedded chart sub-document.
///
/// `None` when the target is not a chart — an embedded spreadsheet or text
/// document is an object with no IR node, and the caller stubs it.
fn read_chart(
    object: &Element,
    ctx: &PageCtx<'_>,
    sink: &mut Sink,
    report: &mut ConversionReport,
) -> Result<Option<docsai_model::presentation::ChartRef>, ReadError> {
    let Some(href) = object
        .attr_qualified("xlink:href")
        .or_else(|| object.attr("href"))
    else {
        return Ok(None);
    };
    let Some(dir) = ctx.package.resolve_href(CONTENT_PART, href) else {
        return Ok(None);
    };
    let part = format!("{dir}/content.xml");
    if !ctx.package.has_part(&part) {
        return Ok(None);
    }
    let source = ctx.package.text(&part)?;
    let root = Element::parse(&part, source.as_bytes())?;
    let Some(element) = root.path(&["body", "chart", "chart"]) else {
        return Ok(None);
    };
    let Some(mut chart) = chart::read(element, report) else {
        return Ok(None);
    };
    // The chart part travels verbatim beside the data read out of it, exactly
    // as `c:chartSpace` does: reading a chart as numbers must not be the reason
    // its markup stops coming back.
    chart.raw = Some(sink.capture(element, &part, source, report));
    Ok(Some(chart))
}

// ---------------------------------------------------------------------------
// Notes and comments
// ---------------------------------------------------------------------------

/// `presentation:notes`: a child of its page, never a part bound by a
/// relationship — which is why there is no `.odp` counterpart of
/// `pptx/notes-crossed.pptx`, and why nothing here can be crossed.
///
/// `draw:page-thumbnail` is skipped without a word, and that is the one
/// deliberate silence in this reader: it is the slide preview the application
/// draws on the printed notes page, it holds no authored content, and the pptx
/// reader skips its `sldImg` counterpart on the same grounds.
fn read_notes(
    notes: &Element,
    body_ctx: &Ctx<'_>,
    assets: &mut dyn AssetStore,
    sink: &mut Sink,
    report: &mut ConversionReport,
) -> Vec<Block> {
    let mut blocks = Vec::new();
    for frame in notes.children_named("frame") {
        let Some(text_box) = frame.child("text-box") else {
            continue;
        };
        let mut state = State {
            assets,
            report,
            raw_seq: sink.seq,
        };
        blocks.extend(read_blocks(text_box, body_ctx, &mut state));
        sink.seq = state.raw_seq;
    }
    blocks
}

/// One annotation on a page.
///
/// ODF writes the author into the annotation, so — unlike the OOXML side, whose
/// author table lives in a part of its own — there is nothing here that can
/// fall out of step with a comment.
fn read_comment(
    annotation: &Element,
    body_ctx: &Ctx<'_>,
    assets: &mut dyn AssetStore,
    sink: &mut Sink,
    report: &mut ConversionReport,
) -> Comment {
    let mut blocks = Vec::new();
    let mut state = State {
        assets,
        report,
        raw_seq: sink.seq,
    };
    for child in annotation.children() {
        if matches!(child.name.as_str(), "p" | "h" | "list" | "table") {
            if let Some(block) = read_block(child, body_ctx, &mut state) {
                blocks.push(block);
            }
        }
    }
    sink.seq = state.raw_seq;

    let anchor = match (
        annotation.attr_qualified("svg:x").and_then(parse_length),
        annotation.attr_qualified("svg:y").and_then(parse_length),
    ) {
        (Some(x), Some(y)) => CommentAnchor::Point(Point::new(x, y)),
        _ => CommentAnchor::Unanchored,
    };

    Comment {
        id: None,
        author: annotation
            .child("creator")
            .map(|c| c.deep_text())
            .filter(|a| !a.is_empty()),
        initials: None,
        date: annotation
            .child("date")
            .map(|d| d.deep_text())
            .filter(|d| !d.is_empty()),
        resolved: false,
        anchor,
        blocks,
        replies: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Geometry and reading order
// ---------------------------------------------------------------------------

/// `svg:x`/`svg:y`/`svg:width`/`svg:height`, plus `draw:transform`.
///
/// An absent position is *inherited*, not zero, so it stays empty: a
/// placeholder that sits exactly where its page layout puts it stores nothing,
/// which is the same reference-plus-delta rule the styles follow.
fn frame_geometry(frame: &Element) -> ShapeGeometry {
    let x = frame.attr_qualified("svg:x").and_then(parse_length);
    let y = frame.attr_qualified("svg:y").and_then(parse_length);
    let width = frame.attr_qualified("svg:width").and_then(parse_length);
    let height = frame.attr_qualified("svg:height").and_then(parse_length);

    let mut geometry = ShapeGeometry {
        pos: match (x, y) {
            (Some(x), Some(y)) => Some(Point::new(x, y)),
            _ => None,
        },
        size: match (width, height) {
            (Some(w), Some(h)) => Some(Size::new(w, h)),
            _ => None,
        },
        ..Default::default()
    };

    if let Some(transform) = frame.attr_qualified("draw:transform") {
        apply_transform(transform, &mut geometry);
    }
    geometry
}

/// `draw:transform="rotate (…) translate (…)"`.
///
/// ODF states a rotation as an angle in **radians, counter-clockwise**, where
/// DrawingML states sixty-thousandths of a degree clockwise; the IR holds
/// degrees clockwise, so the sign flips. A transform function this reader does
/// not know is left alone rather than approximated — the shape it belongs to is
/// already a stub over its own markup.
fn apply_transform(transform: &str, geometry: &mut ShapeGeometry) {
    let mut rest = transform;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().trim_start_matches(')').trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args: Vec<&str> = rest[open + 1..open + close].split_whitespace().collect();
        match name {
            "rotate" => {
                if let Some(radians) = args.first().and_then(|a| a.parse::<f64>().ok()) {
                    geometry.rotation_deg = -(radians.to_degrees()) as f32;
                }
            }
            "translate" => {
                let x = args.first().copied().and_then(parse_length);
                let y = args.get(1).copied().and_then(parse_length);
                if let (Some(x), Some(y)) = (x, y) {
                    geometry.pos = Some(Point::new(x, y));
                }
            }
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
}

/// The reading-order key, as `(rank, row band, x, document order)`.
///
/// The pptx reader orders body placeholders by `p:ph@idx`, because that is the
/// order the layout intended. ODF has no index to intend anything with, so the
/// canvas decides inside a rank — which is the policy the pptx reader already
/// applies to everything that is not a placeholder. The last component is
/// document order, which is unique on a page and makes the order total.
fn reading_key(shape: &Shape) -> (u8, i64, i64, u32) {
    let rank = match &shape.kind {
        ShapeKind::Placeholder(ph) if ph.ph_type.is_title() => 0,
        ShapeKind::Placeholder(ph) if ph.ph_type.is_body() => 1,
        ShapeKind::Placeholder(ph) if ph.ph_type.is_furniture() => 3,
        ShapeKind::Placeholder(_) => 2,
        _ => 4,
    };
    match shape.geometry.pos {
        Some(pos) => (
            rank,
            pos.y.emu().div_euclid(ROW_BAND_EMU),
            pos.x.emu(),
            shape.z_index,
        ),
        // A frame with no position of its own takes it from the page layout,
        // which this key cannot resolve. Guessing would be worse than leaving
        // it where the file put it, so it keeps document order inside its rank.
        None => (rank, i64::MIN, i64::MIN, shape.z_index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    fn read_fixture(name: &str) -> (Presentation, ConversionReport) {
        let path = format!("{}/../../corpus/odp/{name}.odp", env!("CARGO_MANIFEST_DIR"));
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut assets = MemoryAssetStore::new();
        let (document, report) = read(file, &mut assets).unwrap_or_else(|e| panic!("{name}: {e}"));
        let Document::Presentation(deck) = document else {
            panic!("{name} is not a presentation");
        };
        (deck, report)
    }

    #[test]
    fn a_page_reads_into_a_slide_with_its_placeholders() {
        let (deck, report) = read_fixture("basic-slides");
        assert_eq!(deck.slides.len(), 2);
        assert_eq!(
            deck.slides[0].title().as_deref(),
            Some("Informe trimestral")
        );
        assert_eq!(deck.slide_size.width, Length::from_cm(28.0));
        assert!(
            report.warnings.is_empty(),
            "nothing is lost here: {:?}",
            report.warnings
        );
    }

    #[test]
    fn page_order_is_document_order_and_not_the_names() {
        // `slide-order.odp` calls its first page `page3` on purpose.
        let (deck, _) = read_fixture("slide-order");
        let names: Vec<&str> = deck
            .slides
            .iter()
            .filter_map(|s| s.name.as_deref())
            .collect();
        assert_eq!(names, vec!["page3", "page1", "page2"]);
    }

    #[test]
    fn an_inherited_placeholder_carries_no_delta_and_no_geometry() {
        let (deck, _) = read_fixture("placeholders-cascade");
        for shape in &deck.slides[0].shapes {
            let ph = shape.placeholder().expect("both frames are placeholders");
            assert!(
                ph.delta.is_empty(),
                "{:?} states what it inherits",
                ph.ph_type
            );
            assert!(shape.geometry.is_inherited(), "{:?}", shape.geometry);
        }
    }

    #[test]
    fn two_outlines_on_one_page_are_told_apart_and_warned_about() {
        let (deck, report) = read_fixture("presentation-classes");
        let bodies: Vec<Option<u32>> = deck.slides[0]
            .shapes
            .iter()
            .filter_map(|s| s.placeholder())
            .filter(|ph| ph.ph_type == PhType::Body)
            .map(|ph| ph.idx)
            .collect();
        assert_eq!(bodies, vec![None, Some(1)]);
        assert_eq!(
            report
                .warnings
                .iter()
                .filter(|w| matches!(w, Warning::Degraded { what, .. } if what.contains("outline")))
                .count(),
            1,
            "the collision is named once: {:?}",
            report.warnings
        );
        // `text` keeps its ODF keyword rather than becoming a second body.
        assert!(deck.slides[0]
            .shapes
            .iter()
            .filter_map(|s| s.placeholder())
            .any(|ph| ph.ph_type == PhType::Other("text".into())));
    }

    /// The two classes PresentationML does not model, and the reason they are
    /// spelled differently (18-C).
    #[test]
    fn the_two_unmodelled_classes_keep_the_word_the_dialects_share() {
        // `text` has no PresentationML counterpart, so the ODF word is the
        // only word there is.
        assert_eq!(ph_type_of("text"), PhType::Other("text".into()));
        // `page` does have one. DocMark-P writes `type=` for both dialects, so
        // it is written the way both dialects can read it.
        assert_eq!(ph_type_of("page"), PhType::Other("sldImg".into()));
        // And what neither models is still kept verbatim.
        assert_eq!(ph_type_of("handout"), PhType::Other("handout".into()));
    }

    #[test]
    fn reading_order_is_the_canvas_and_z_order_is_kept() {
        let (deck, report) = read_fixture("reading-order");
        let texts: Vec<String> = deck.slides[0]
            .shapes
            .iter()
            .map(|shape| match &shape.kind {
                ShapeKind::Placeholder(ph) => blocks_text(&ph.body),
                ShapeKind::TextBox { body } => blocks_text(body),
                _ => String::new(),
            })
            .collect();
        assert_eq!(
            texts,
            vec![
                "El título va el cuarto",
                "Cuerpo heredado",
                "A la izquierda",
                "A la derecha",
                "Al pie de la página",
            ]
        );
        // Reversible: document order is still there.
        let mut z: Vec<u32> = deck.slides[0].shapes.iter().map(|s| s.z_index).collect();
        z.sort_unstable();
        assert_eq!(z, vec![0, 1, 2, 3, 4]);
        assert!(
            report.warnings.iter().any(
                |w| matches!(w, Warning::Degraded { what, .. } if what.contains("draw:z-index"))
            ),
            "the stated stacking order disagrees and is named: {:?}",
            report.warnings
        );
    }

    fn blocks_text(blocks: &[Block]) -> String {
        blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph(p) => p.plain_text(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_string()
    }

    #[test]
    fn every_unmodelled_construct_is_a_stub_over_its_own_markup() {
        let (deck, report) = read_fixture("raw-preserved");
        // A group, a custom shape, the animation tree and the transition.
        assert_eq!(deck.raw.len(), 4, "{:?}", deck.raw);
        assert_eq!(report.raw_blocks_emitted, 4);
        assert!(deck.raw.iter().any(|f| f.content.starts_with("<draw:g")));
        assert!(deck
            .raw
            .iter()
            .any(|f| f.content.contains("draw:enhanced-geometry")));
        assert!(deck.raw.iter().any(|f| f.content.starts_with("<anim:par")));
        assert!(deck
            .raw
            .iter()
            .any(|f| f.content.contains("presentation:transition-style")));
        assert_eq!(deck.slides[0].raw.len(), 2, "the page-level pair");
    }

    #[test]
    fn an_ole_object_is_not_read_as_its_replacement_bitmap() {
        let (deck, _) = read_fixture("ole-embedded");
        let ole = deck.slides[0]
            .shapes
            .iter()
            .find_map(|s| match &s.kind {
                ShapeKind::Raw(raw) => Some(raw),
                _ => None,
            })
            .expect("the OLE frame is a stub");
        assert_eq!(ole.kind, RawShapeKind::Ole);
        assert!(ole.raw.is_some());
    }

    #[test]
    fn a_chart_reads_its_series_out_of_its_own_local_table() {
        let (deck, _) = read_fixture("charts-multi-series");
        let chart = deck.slides[0]
            .shapes
            .iter()
            .find_map(|s| match &s.kind {
                ShapeKind::Chart(chart) => Some(chart),
                _ => None,
            })
            .expect("a chart");
        assert_eq!(chart.kind.as_deref(), Some("barChart"));
        assert_eq!(chart.categories, vec!["Norte", "Sur", "Este"]);
        assert_eq!(chart.series.len(), 2);
        assert_eq!(chart.series[0].name.as_deref(), Some("2025"));
        assert_eq!(
            chart.series[1].values,
            vec![Some(1310.0), Some(1015.0), Some(1502.0)]
        );
        // An ODF chart has one copy of its numbers, so there is no workbook.
        assert!(chart.workbook.is_none());
    }

    #[test]
    fn notes_and_comments_land_on_the_page_they_belong_to() {
        let (deck, report) = read_fixture("notes-speaker");
        assert!(deck.slides[0].notes.is_some());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let (deck, _) = read_fixture("comments-slide");
        assert_eq!(deck.slides[0].comments.len(), 2);
        assert_eq!(
            deck.slides[0].comments[0].author.as_deref(),
            Some("Ana Revisora")
        );
        assert!(matches!(
            deck.slides[0].comments[0].anchor,
            CommentAnchor::Point(_)
        ));
    }

    #[test]
    fn forty_pages_read_well_inside_a_second() {
        let start = std::time::Instant::now();
        let (deck, _) = read_fixture("forty-slides");
        let elapsed = start.elapsed();
        assert_eq!(deck.slides.len(), 40);
        assert!(elapsed < std::time::Duration::from_secs(1), "{elapsed:?}");
    }

    #[test]
    fn a_rotation_is_read_as_clockwise_degrees() {
        let mut geometry = ShapeGeometry::default();
        apply_transform(
            "rotate (-0.5235987755982988) translate (9cm 6cm)",
            &mut geometry,
        );
        assert!((geometry.rotation_deg - 30.0).abs() < 0.001, "{geometry:?}");
        assert_eq!(geometry.pos.map(|p| p.x), Some(Length::from_cm(9.0)));
    }
}
