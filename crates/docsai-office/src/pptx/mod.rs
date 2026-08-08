//! The `.pptx` reader (Phase 13).
//!
//! A sibling of `docx/` and `xlsx/`, on the same `zip` + `quick-xml` foundation
//! spike P1 chose over `ooxmlsdk`. This module is the **package layer** — which
//! parts exist, what they are, in what order the slides come — plus the shapes
//! of a slide's `p:spTree`; the text inside them lives in [`text`] and what they
//! inherit in [`cascade`].
//!
//! Masters are read before layouts and layouts before slides, and that order is
//! load-bearing: a slide's properties are stored as a **delta** over what its
//! layout, master and theme already decided, so the references have to exist
//! before the text that measures itself against them.
//!
//! Shape kinds this reader does not model yet (groups, connectors, charts,
//! SmartArt) are **reported, not skipped in silence**: a slide that quietly
//! loses its chart is the failure this project exists to avoid. Pictures and
//! tables are read, and neither gets a model of its own — they are the same
//! `ImageRef` and `Table` a `.docx` carries, through the same [`AssetStore`].
//!
//! Two rules from spike P3 are enforced here rather than assumed:
//!
//! * **A part is what its content type says it is**, never what its name
//!   suggests. `ppt/slides/slide1.xml` is a convention; the deck a converter
//!   produced may put its slides anywhere, and the reader still has to find
//!   them.
//! * **Order comes from `p:sldIdLst`**, never from the part names. `slide3.xml`
//!   being the first slide of the deck is legal and common after a reorder in
//!   PowerPoint. The same rule binds a slide to its notes: [`notes`] follows the
//!   slide's `notesSlide` relationship, never the number in the part name.
//!
//! What this reader does *not* model is not thereby lost: [`skeleton`] keeps
//! the original package whole and opaque, so the writer re-injects the slides
//! it can rebuild into the deck as it was written rather than regenerating a
//! theme, a master or an embedded workbook it never understood.
//!
//! A third order is not read from the file at all: the order of the shapes on a
//! slide. `p:spTree` is z-order, and [`order`] computes reading order from it —
//! reversibly, because every shape keeps its source index.

mod cascade;
mod graphics;
mod notes;
mod order;
mod raw;
mod skeleton;
mod text;

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::presentation::{
    ChartRef, Layout, LayoutCatalog, LayoutId, LayoutPlaceholder, Master, MasterId, PhType,
    Placeholder, Presentation, RawShapeKind, Shape, ShapeGeometry, ShapeKind, ShapeProps, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::units::{Length, Point, Size};
use docsai_model::Document;

use crate::error::ReadError;
use crate::package::{read_meta, ContentTypes, Package, Relationships};
use crate::xml::Element;

use cascade::{Cascade, Theme};

/// Where PowerPoint puts the presentation part. Used to *detect* a deck and as
/// the last resort when the content types are unreadable; the reader itself
/// resolves the part through the package relationships.
pub(crate) const PRESENTATION_PART: &str = "ppt/presentation.xml";

const PML: &str = "application/vnd.openxmlformats-officedocument.presentationml";
const CT_PRESENTATION: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
const CT_PRESENTATION_MACRO: &str =
    "application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml";

/// Cap on the compressed package the skeleton keeps, matching the cap
/// `package.rs` puts on what it expands to. A deck this reader would refuse to
/// decompress is one it refuses to hold.
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;

/// Default slide size when `p:sldSz` is missing: 4:3 at 10 × 7.5 in.
const DEFAULT_SLIDE_SIZE: (i64, i64) = (9_144_000, 6_858_000);

/// Reads a `.pptx` presentation into the IR.
///
/// The bytes are read once and kept, because they are two things at once: the
/// package to decompress, and the [`skeleton`] the writer will re-inject slides
/// into. Handing `Package::open` a reader and then asking for the original
/// again would mean decompressing or reading the deck twice.
pub fn read<R: Read + Seek>(
    mut reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    // Capped, because reading the stream ahead of `Package::open` moves the
    // first size check to this line: without it, a 5 GiB file that is not even
    // a ZIP would be pulled into memory before anything rejected it.
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
    let mut report = ConversionReport::new();
    let types = ContentTypes::read(package);
    let main = main_part(package, &types)?;

    // A `.pptm` is a `.pptx` with a macro project bolted on, and it is read as
    // its macro-free equivalent — never executed, never carried across — the
    // same rule `.docm` and `.xlsm` already follow (AGENTS.md §7, plan Phase 8).
    // The skeleton still holds the project part byte for byte, so a lossless
    // round trip does not silently disarm the deck either.
    if let Some(part) = package
        .part_names()
        .find(|name| name.ends_with("vbaProject.bin"))
    {
        report.warn(Warning::MacrosIgnored {
            part: part.to_string(),
        });
    }

    let root = parse(package, &main)?;
    let rels = package.relationships(&main);

    let slide_size = match root.child("sldSz") {
        Some(size) => Size::new(
            Length::from_emu(size.attr_i64("cx").unwrap_or(DEFAULT_SLIDE_SIZE.0)),
            Length::from_emu(size.attr_i64("cy").unwrap_or(DEFAULT_SLIDE_SIZE.1)),
        ),
        None => Size::new(
            Length::from_emu(DEFAULT_SLIDE_SIZE.0),
            Length::from_emu(DEFAULT_SLIDE_SIZE.1),
        ),
    };

    let mut layouts = LayoutCatalog::default();
    let mut cascade = Cascade::default();
    let mut wanted_layouts: Vec<String> = Vec::new();

    // Masters first, layouts second, slides last: a slide's runs are read as a
    // delta over what its layout and master already decided, so the references
    // have to exist before the text does.
    for master_part in targets(&root, "sldMasterIdLst", "sldMasterId", &rels) {
        if !types.is(&master_part, &format!("{PML}.slideMaster+xml")) {
            report.warn(Warning::Degraded {
                what: format!("slide master `{master_part}`"),
                why: "the package declares it as something else".into(),
            });
            continue;
        }
        let (id, master, own_layouts) =
            read_master(package, &master_part, &mut cascade, &mut report)?;
        wanted_layouts.extend(own_layouts);
        layouts.masters.insert(id, master);
    }
    cascade.add_default_text(&root, &mut report);

    let sections = read_sections(&root);
    let mut slides = Vec::new();
    // Deck-wide, because the raw ids are: every stub on every slide refers into
    // the one `Presentation::raw` list.
    let mut sink = raw::Sink::default();
    // The parts whose content ends up in the IR, and therefore the only ones a
    // writer is allowed to regenerate instead of copying from the skeleton.
    let mut rebuilt_parts: Vec<String> = Vec::new();

    for entry in slide_entries(&root) {
        let Some(rel) = entry.rel_id.as_deref().and_then(|id| rels.get(id)) else {
            report.warn(Warning::Degraded {
                what: format!("slide {}", entry.id),
                why: "unresolved relationship".into(),
            });
            continue;
        };
        if rel.external {
            report.warn(Warning::Degraded {
                what: format!("slide {}", entry.id),
                why: "the slide part lives outside the package".into(),
            });
            continue;
        }
        if !types.is(&rel.target, &format!("{PML}.slide+xml")) {
            report.warn(Warning::Degraded {
                what: format!("slide {}", entry.id),
                why: format!("`{}` is not declared as a slide", rel.target),
            });
            continue;
        }
        // A slide's own layout is loaded before the slide, even when no master
        // declared it: it is the first reference in the chain and reading the
        // text without it would silently store inherited properties as deltas.
        let slide_rels = package.relationships(&rel.target);
        let layout = slide_rels
            .of_kind("slideLayout")
            .find(|rel| !rel.external)
            .map(|rel| LayoutId::new(rel.target.clone()));
        if let Some(layout) = &layout {
            wanted_layouts.push(layout.as_str().to_string());
            load_layout(
                package,
                &types,
                layout.as_str(),
                &mut layouts,
                &mut cascade,
                &mut report,
            )?;
        }

        let slide = read_slide(
            package,
            &types,
            &rel.target,
            &slide_rels,
            layout,
            &entry,
            &sections,
            &cascade,
            assets,
            &mut sink,
            &mut report,
        )?;
        report.stats.slides = report.stats.slides.saturating_add(1);
        rebuilt_parts.push(rel.target.clone());
        // Only when the notes were read: a notes part the reader refused is a
        // part the IR does not hold, so the writer has to copy it back.
        if slide.notes.is_some() {
            if let Some(part) = notes::part(&slide_rels) {
                rebuilt_parts.push(part.to_string());
            }
        }
        slides.push(slide);
    }

    // The layouts a master declares but no slide uses. They are part of the
    // deck — a slide added later picks one — so they are catalogued too.
    wanted_layouts.sort();
    wanted_layouts.dedup();
    for layout_part in wanted_layouts {
        load_layout(
            package,
            &types,
            &layout_part,
            &mut layouts,
            &mut cascade,
            &mut report,
        )?;
    }

    let skeleton =
        original.and_then(|bytes| skeleton::capture(bytes, &rebuilt_parts, assets, &mut report));

    let presentation = Presentation {
        meta: read_meta(package),
        addressing: Default::default(),
        styles: Default::default(),
        layouts,
        slide_size,
        slides,
        skeleton,
        raw: sink.fragments,
    };
    Ok((Document::Presentation(presentation), report))
}

/// Locates the presentation part.
///
/// The package relationship of type `officeDocument` is the authority; the
/// content types are the cross-check, and the conventional name is only used
/// when a package carries neither — a deck that opens in PowerPoint and would
/// otherwise be refused over a missing `_rels/.rels`.
fn main_part(package: &Package, types: &ContentTypes) -> Result<String, ReadError> {
    let root_rels = package.relationships("_rels/.rels");
    let by_rel = root_rels
        .of_kind("officeDocument")
        .find(|rel| !rel.external && package.has_part(&rel.target))
        .map(|rel| rel.target.clone())
        .filter(|part| is_presentation_part(types, part));
    if let Some(part) = by_rel {
        return Ok(part);
    }
    let by_type = types
        .parts_of(CT_PRESENTATION)
        .chain(types.parts_of(CT_PRESENTATION_MACRO))
        .find(|part| package.has_part(part))
        .map(str::to_string);
    if let Some(part) = by_type {
        return Ok(part);
    }
    if package.has_part(PRESENTATION_PART) {
        return Ok(PRESENTATION_PART.to_string());
    }
    Err(ReadError::MissingPart(PRESENTATION_PART.into()))
}

fn is_presentation_part(types: &ContentTypes, part: &str) -> bool {
    match types.of(part) {
        Some(kind) => kind == CT_PRESENTATION || kind == CT_PRESENTATION_MACRO,
        // No declaration: the relationship already said this is the main part.
        None => true,
    }
}

/// One `p:sldId`: the deck-wide slide id, and the relationship to its part.
struct SlideEntry {
    /// `p:sldId@id`, which is what `p14:sectionLst` refers to.
    id: i64,
    rel_id: Option<String>,
}

fn slide_entries(root: &Element) -> Vec<SlideEntry> {
    root.child("sldIdLst")
        .into_iter()
        .flat_map(|list| list.children_named("sldId"))
        .map(|entry| SlideEntry {
            id: entry.attr_i64("id").unwrap_or_default(),
            rel_id: rel_id(entry).map(str::to_string),
        })
        .collect()
}

/// The relationship id of an element.
///
/// It has to be matched on the **qualified** name: `p:sldId` carries both
/// `r:id` (the relationship) and `id` (a numeric slide id), so a lookup by
/// local name returns the wrong one — silently, and with a number that parses.
fn rel_id(element: &Element) -> Option<&str> {
    element
        .attrs
        .iter()
        .find(|(name, _)| name.ends_with(":id"))
        .map(|(_, value)| value.as_str())
}

/// Part names reached through `<list><item r:id=…/></list>`, in document order.
fn targets(
    root: &Element,
    list: &str,
    item: &str,
    rels: &crate::package::Relationships,
) -> Vec<String> {
    root.child(list)
        .into_iter()
        .flat_map(|list| list.children_named(item))
        .filter_map(|entry| rels.get(rel_id(entry)?))
        .filter(|rel| !rel.external)
        .map(|rel| rel.target.clone())
        .collect()
}

/// `p14:sectionLst`: slide id to section name.
///
/// Sections live in an extension, so every step of the path is optional and a
/// deck without them is the normal case, not a degraded one.
fn read_sections(root: &Element) -> BTreeMap<i64, String> {
    let mut sections = BTreeMap::new();
    let Some(ext_list) = root.child("extLst") else {
        return sections;
    };
    for section in ext_list
        .children_named("ext")
        .filter_map(|ext| ext.child("sectionLst"))
        .flat_map(|list| list.children_named("section"))
    {
        let Some(name) = section.attr("name").filter(|n| !n.is_empty()) else {
            continue;
        };
        for entry in section
            .child("sldIdLst")
            .into_iter()
            .flat_map(|list| list.children_named("sldId"))
        {
            if let Some(id) = entry.attr_i64("id") {
                sections.insert(id, name.to_string());
            }
        }
    }
    sections
}

/// Everything a shape needs from the slide around it: where its media lives,
/// what it inherits, and what to call the part in a warning.
struct SlideCtx<'a> {
    package: &'a Package,
    part: &'a str,
    /// The part's own text, so a raw fragment is a slice of the source rather
    /// than a re-serialisation of the tree.
    source: &'a str,
    rels: &'a Relationships,
    layout: Option<&'a LayoutId>,
    cascade: &'a Cascade,
}

#[allow(clippy::too_many_arguments)]
fn read_slide(
    package: &Package,
    types: &ContentTypes,
    part: &str,
    rels: &Relationships,
    layout: Option<LayoutId>,
    entry: &SlideEntry,
    sections: &BTreeMap<i64, String>,
    cascade: &Cascade,
    assets: &mut dyn AssetStore,
    sink: &mut raw::Sink,
    report: &mut ConversionReport,
) -> Result<Slide, ReadError> {
    let root = parse(package, part)?;

    if layout.is_none() {
        report.warn(Warning::Degraded {
            what: format!("slide `{part}`"),
            why: "no slide layout: its placeholders cannot inherit".into(),
        });
    }

    let ctx = SlideCtx {
        package,
        part,
        // The bytes the raw fragments are sliced out of. Read once per slide:
        // every stub on it points into this string.
        source: package.text(part)?,
        rels,
        layout: layout.as_ref(),
        cascade,
    };
    let shapes = match root.path(&["cSld", "spTree"]) {
        Some(tree) => read_shapes(tree, &ctx, assets, sink, report)?,
        None => {
            report.warn(Warning::Degraded {
                what: format!("slide `{part}`"),
                why: "no shape tree".into(),
            });
            Vec::new()
        }
    };

    let notes = notes::read(
        package,
        types,
        part,
        rels,
        cascade.theme_of(layout.as_ref()),
        report,
    )?;

    // `p:transition` and `p:timing`: subtrees with no Markdown representation
    // and no IR node, and the two that vanished without a word until now.
    let mut slide_raw = Vec::new();
    for child in root.children() {
        if matches!(child.name.as_str(), "transition" | "timing") {
            slide_raw.push(sink.capture(child, part, ctx.source, report));
        }
    }

    Ok(Slide {
        id: None,
        layout,
        name: csld_name(&root),
        shapes,
        notes,
        hidden: root.attr("show") == Some("0"),
        section: sections.get(&entry.id).cloned(),
        raw: slide_raw,
    })
}

/// The `p:spTree` children that become a stub plus a raw fragment rather than
/// a modelled shape, and the kind of stub each one gets.
///
/// A group is stubbed whole, children included. It is a real loss of structure
/// — the model has a `ShapeKind::Group` this reader does not fill — and it is
/// the loss the increment chose: reading a group's children means reading a
/// second, nested cascade, and a stub that carries the group's text and its
/// markup loses nothing a writer cannot put back.
const STUBBED_SHAPES: &[(&str, RawShapeKind)] = &[
    ("grpSp", RawShapeKind::Shape),
    ("cxnSp", RawShapeKind::Connector),
    // SmartArt reaches a slide inside this: a `mc:Choice` holding the diagram
    // and a `mc:Fallback` holding shapes that draw it. Preserved whole, because
    // the choice between the two branches is the consumer's, not ours.
    ("AlternateContent", RawShapeKind::Other),
];

/// What an `mc:AlternateContent` wraps, when it is recognisable.
///
/// Naming the stub is not the same as choosing a branch: the pair is still
/// preserved whole and neither branch is read. But a stub that says «other»
/// about SmartArt under-informs exactly the agent the stub exists for — the
/// `inspect` slide inventory reports has-SmartArt and has-OLE from this
/// (plan v2 Phase 13-K), and both arrive on a slide inside this wrapper.
fn wrapped_kind(element: &Element) -> Option<RawShapeKind> {
    if element.name == "oleObj" {
        return Some(RawShapeKind::Ole);
    }
    if element.name == "graphicData" {
        if let Some(uri) = element.attr("uri") {
            let held = uri.rsplit('/').next().unwrap_or_default();
            match held_kind(held) {
                RawShapeKind::Other => {}
                kind => return Some(kind),
            }
        }
    }
    element.children().find_map(wrapped_kind)
}

/// The stub kind for the last segment of an `a:graphicData@uri`.
fn held_kind(held: &str) -> RawShapeKind {
    match held {
        "diagram" => RawShapeKind::SmartArt,
        "ole" | "oleObject" => RawShapeKind::Ole,
        _ => RawShapeKind::Other,
    }
}

/// Reads the shapes of a slide's `p:spTree`, in **reading order**.
///
/// The tree gives z-order; [`order::sort`] turns it into the order a human
/// reads the slide in, and every shape keeps the `z_index` that makes that
/// reordering reversible.
fn read_shapes(
    tree: &Element,
    ctx: &SlideCtx<'_>,
    assets: &mut dyn AssetStore,
    sink: &mut raw::Sink,
    report: &mut ConversionReport,
) -> Result<Vec<Shape>, ReadError> {
    let mut shapes = Vec::new();
    let mut z_index = 0u32;
    for child in tree.children() {
        match child.name.as_str() {
            // The group's own identity and transform, not a shape on the slide.
            "nvGrpSpPr" | "grpSpPr" => continue,
            "sp" => {
                // A custom geometry is a path list, not a shape any Markdown
                // names. Its text still travels on the stub.
                if child.path(&["spPr", "custGeom"]).is_some() {
                    shapes.push(sink.shape(
                        child,
                        RawShapeKind::Shape,
                        z_index,
                        ctx.part,
                        ctx.source,
                        report,
                    ));
                } else {
                    shapes.push(read_shape(child, z_index, ctx, report));
                }
            }
            "pic" => {
                if let Some(image) =
                    graphics::read_picture(child, ctx.package, ctx.rels, assets, report)?
                {
                    shapes.push(Shape {
                        id: None,
                        name: image.name.clone(),
                        z_index,
                        geometry: read_geometry(child.path(&["spPr", "xfrm"])),
                        kind: ShapeKind::Picture(image),
                    });
                }
            }
            "graphicFrame" => {
                shapes.push(read_graphic_frame(child, z_index, ctx, sink, report));
            }
            name => {
                let kind = STUBBED_SHAPES
                    .iter()
                    .find(|(tag, _)| *tag == name)
                    .map(|(_, kind)| *kind)
                    // An element this reader has never heard of is preserved
                    // too. Guessing what it is would be worse than saying so.
                    .unwrap_or(RawShapeKind::Other);
                let kind = if name == "AlternateContent" {
                    wrapped_kind(child).unwrap_or(kind)
                } else {
                    kind
                };
                shapes.push(sink.shape(child, kind, z_index, ctx.part, ctx.source, report));
            }
        }
        z_index += 1;
    }
    order::sort(&mut shapes);
    Ok(shapes)
}

/// A `p:graphicFrame`: a table, a chart, or something with no IR node at all.
///
/// The frame is a container, and what it holds is named by
/// `a:graphicData@uri` — never by guessing from the first child, which is how a
/// chart ends up read as an empty table. Whatever it holds, a shape comes back:
/// a frame that produced nothing would be a hole in the slide.
fn read_graphic_frame(
    frame: &Element,
    z_index: u32,
    ctx: &SlideCtx<'_>,
    sink: &mut raw::Sink,
    report: &mut ConversionReport,
) -> Shape {
    let data = frame.path(&["graphic", "graphicData"]);
    let uri = data.and_then(|data| data.attr("uri")).unwrap_or_default();
    // The uri's last segment is what the frame holds: `table`, `chart`,
    // `diagram`, `ole`.
    let held = uri
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("unknown content");

    let name = frame
        .path(&["nvGraphicFramePr", "cNvPr"])
        .and_then(|nv| nv.attr("name"))
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    // A graphic frame states its transform on `p:xfrm`, one level up from where
    // a shape does.
    let geometry = read_geometry(frame.child("xfrm"));

    if let Some(tbl) = data
        .filter(|_| uri == graphics::TABLE_URI)
        .and_then(|data| data.child("tbl"))
    {
        let table = graphics::read_table(tbl, ctx.cascade.theme_of(ctx.layout), ctx.rels, report);
        report.stats.tables = report.stats.tables.saturating_add(1);
        return Shape {
            id: None,
            name,
            z_index,
            geometry,
            kind: ShapeKind::Table(table),
        };
    }

    if held == "chart" {
        if let Some(chart) = read_chart(data, ctx, sink, report) {
            return Shape {
                id: None,
                name,
                z_index,
                geometry,
                kind: ShapeKind::Chart(chart),
            };
        }
    }

    let mut shape = sink.shape(
        frame,
        held_kind(held),
        z_index,
        ctx.part,
        ctx.source,
        report,
    );
    shape.name = name;
    shape.geometry = geometry;
    shape
}

/// The chart a frame holds: what kind it is, and its markup.
///
/// Phase 13 records that a chart is there and preserves it; the series are
/// Phase 16's, and the workbook they come from is in the skeleton either way.
/// The part is reached through the frame's `r:id`, like every other reference
/// in the format.
fn read_chart(
    data: Option<&Element>,
    ctx: &SlideCtx<'_>,
    sink: &mut raw::Sink,
    report: &mut ConversionReport,
) -> Option<ChartRef> {
    let rel_id = data?.child("chart")?.attr_qualified("r:id")?;
    let rel = ctx.rels.get(rel_id).filter(|rel| !rel.external)?;
    let part = rel.target.clone();
    let (root, source) = match (parse(ctx.package, &part), ctx.package.text(&part)) {
        (Ok(root), Ok(source)) => (root, source),
        _ => {
            report.warn(Warning::Degraded {
                what: format!("chart `{part}`"),
                why: "the chart part could not be read; the frame is kept as a stub".into(),
            });
            return None;
        }
    };
    // `c:barChart`, `c:lineChart`… the first plot in the plot area names the
    // chart, and a combo chart is named by its first plot rather than by a
    // guess at which one matters.
    let kind = root.path(&["chart", "plotArea"]).and_then(|area| {
        area.children()
            .find(|child| child.name.ends_with("Chart"))
            .map(|child| child.name.clone())
    });
    Some(ChartRef {
        kind,
        title: None,
        workbook: None,
        raw: Some(sink.capture(&root, &part, source, report)),
    })
}

/// One `p:sp`: a placeholder when it fills a slot the layout declares, a free
/// text box when it does not.
fn read_shape(
    sp: &Element,
    z_index: u32,
    ctx: &SlideCtx<'_>,
    report: &mut ConversionReport,
) -> Shape {
    let ph = sp.path(&["nvSpPr", "nvPr", "ph"]);
    let ph_type = ph.map(|ph| PhType::parse(ph.attr("type").unwrap_or_default()));
    let idx = ph
        .and_then(|ph| ph.attr_i64("idx"))
        .and_then(|n| u32::try_from(n).ok());
    let theme = ctx.cascade.theme_of(ctx.layout);

    // What this shape inherits, before it says anything itself.
    let inherited = match &ph_type {
        Some(ph_type) => ctx.cascade.inherited(ctx.layout, ph_type, idx),
        None => ctx.cascade.inherited_text_box(ctx.layout),
    };
    // The shape's own `a:lstStyle` sits between the layout and the runs: it is
    // this shape's delta, and it is also what its runs inherit.
    let own = match sp.path(&["txBody", "lstStyle"]) {
        Some(style) => cascade::read_levels(style, theme, report),
        None => Default::default(),
    };
    let font = own.at(0).minus(&inherited.at(0));
    let runs_inherit = own.over(&inherited);
    let text_ctx = text::TextCtx {
        rels: ctx.rels,
        // A body placeholder bullets by inheritance; a title and a free text
        // box do not.
        bulleted: ph_type.as_ref().is_some_and(PhType::is_body),
        theme,
        inherited: &runs_inherit,
    };
    let body = match sp.child("txBody") {
        Some(tx_body) => text::read_body(tx_body, &text_ctx, report),
        None => Vec::new(),
    };

    let kind = match ph_type {
        Some(ph_type) => ShapeKind::Placeholder(Placeholder {
            ph_type,
            idx,
            body,
            delta: ShapeProps {
                font,
                ..shape_props(sp)
            },
        }),
        None => ShapeKind::TextBox { body },
    };

    let name = sp
        .path(&["nvSpPr", "cNvPr"])
        .and_then(|nv| nv.attr("name"))
        .filter(|name| !name.is_empty())
        .map(str::to_string);

    // The scale PowerPoint computed for the text that was in the box when a
    // human last edited it. It is kept — only PowerPoint can recompute it — and
    // reported, because an agent that adds a line has just made it a lie and
    // nothing in the file says so (analysis §5.4).
    if let Some(scale) = sp
        .path(&["txBody", "bodyPr", "normAutofit"])
        .and_then(|fit| fit.attr_i64("fontScale"))
        .and_then(|scale| u32::try_from(scale).ok())
    {
        report.warn(Warning::AutofitStale {
            what: format!(
                "`{}` on `{}`",
                name.as_deref().unwrap_or("an unnamed shape"),
                ctx.part
            ),
            scale,
        });
    }

    let mut geometry = read_geometry(sp.path(&["spPr", "xfrm"]));
    // `rect`, `roundRect`, `rightArrow`: without it a shape's outline is gone
    // the moment the deck is written from the IR rather than the skeleton.
    geometry.preset = sp
        .path(&["spPr", "prstGeom"])
        .and_then(|geom| geom.attr("prst"))
        .map(str::to_string);

    Shape {
        id: None,
        name,
        z_index,
        geometry,
        kind,
    }
}

/// The shape's own formatting that is a fact about the shape rather than a
/// delta over what it inherits. The delta proper is layered on top of this by
/// the caller, which is the only place that knows the cascade.
fn shape_props(sp: &Element) -> ShapeProps {
    ShapeProps {
        // `a:normAutofit@fontScale` is in thousandths of a percent.
        font_scale: sp
            .path(&["txBody", "bodyPr", "normAutofit"])
            .and_then(|fit| fit.attr_i64("fontScale"))
            .map(|scale| scale as f32 / 1000.0),
        ..Default::default()
    }
}

/// Reads a slide master, returning it with the layout parts it declares.
fn read_master(
    package: &Package,
    part: &str,
    cascade: &mut Cascade,
    report: &mut ConversionReport,
) -> Result<(MasterId, Master, Vec<String>), ReadError> {
    let root = parse(package, part)?;
    let rels = package.relationships(part);
    let layouts = targets(&root, "sldLayoutIdLst", "sldLayoutId", &rels);
    if layouts.is_empty() {
        report.warn(Warning::Degraded {
            what: format!("slide master `{part}`"),
            why: "declares no layouts".into(),
        });
    }

    // The theme is a reference the IR keeps by name, and the values every
    // `a:schemeClr` and `+mj-lt` below resolves through.
    let theme_part = rels
        .of_kind("theme")
        .find(|rel| !rel.external)
        .map(|rel| rel.target.clone());
    let theme = match &theme_part {
        Some(part) => Theme::read(&parse(package, part)?).with_map(&root),
        None => Theme::default().with_map(&root),
    };
    cascade.add_master(part, &root, theme, report);

    let master = Master {
        name: csld_name(&root).unwrap_or_else(|| part_stem(part)),
        theme: theme_part,
        placeholders: read_placeholders(&root, |ph_type, idx| {
            cascade.master_placeholder(part, ph_type, idx)
        }),
    };
    Ok((MasterId::new(part.to_string()), master, layouts))
}

/// Reads a layout into both the catalogue and the cascade, unless it is already
/// there or the package says it is not a layout at all.
fn load_layout(
    package: &Package,
    types: &ContentTypes,
    part: &str,
    layouts: &mut LayoutCatalog,
    cascade: &mut Cascade,
    report: &mut ConversionReport,
) -> Result<(), ReadError> {
    if layouts
        .layouts
        .contains_key(&LayoutId::new(part.to_string()))
    {
        return Ok(());
    }
    if !types.is(part, &format!("{PML}.slideLayout+xml")) {
        report.warn(Warning::Degraded {
            what: format!("slide layout `{part}`"),
            why: "the package declares it as something else".into(),
        });
        return Ok(());
    }
    let (id, layout) = read_layout(package, part, cascade, report)?;
    layouts.layouts.insert(id, layout);
    Ok(())
}

fn read_layout(
    package: &Package,
    part: &str,
    cascade: &mut Cascade,
    report: &mut ConversionReport,
) -> Result<(LayoutId, Layout), ReadError> {
    let root = parse(package, part)?;
    let rels = package.relationships(part);
    let master = rels
        .of_kind("slideMaster")
        .find(|rel| !rel.external)
        .map(|rel| MasterId::new(rel.target.clone()));
    if master.is_none() {
        report.warn(Warning::Degraded {
            what: format!("slide layout `{part}`"),
            why: "no slide master: the cascade stops here".into(),
        });
    }
    cascade.add_layout(part, &root, master.as_ref().map(MasterId::as_str), report);

    let id = LayoutId::new(part.to_string());
    let layout = Layout {
        name: csld_name(&root).unwrap_or_else(|| part_stem(part)),
        master,
        placeholders: read_placeholders(&root, |ph_type, idx| {
            cascade.inherited(Some(&id), ph_type, idx)
        }),
    };
    Ok((id, layout))
}

/// The placeholders a layout or master declares, in `p:spTree` order.
///
/// `resolved` answers what a slide inherits from each of them: unlike a slide
/// placeholder, which stores a delta, a layout or master placeholder stores the
/// resolved values, because it is the reference the delta is measured against.
fn read_placeholders(
    root: &Element,
    resolved: impl Fn(&PhType, Option<u32>) -> cascade::LevelStyles,
) -> Vec<LayoutPlaceholder> {
    let Some(tree) = root.path(&["cSld", "spTree"]) else {
        return Vec::new();
    };
    tree.children_named("sp")
        .filter_map(|shape| {
            let ph = shape.path(&["nvSpPr", "nvPr", "ph"])?;
            let ph_type = PhType::parse(ph.attr("type").unwrap_or_default());
            let idx = ph.attr_i64("idx").and_then(|n| u32::try_from(n).ok());
            let font = cascade::reference_font(&resolved(&ph_type, idx));
            Some(LayoutPlaceholder {
                ph_type,
                idx,
                geometry: read_geometry(shape.path(&["spPr", "xfrm"])),
                props: ShapeProps {
                    font,
                    ..Default::default()
                },
            })
        })
        .collect()
}

/// `a:xfrm`. An absent transform is *inherited*, not zero, so it stays empty.
fn read_geometry(xfrm: Option<&Element>) -> ShapeGeometry {
    let Some(xfrm) = xfrm else {
        return ShapeGeometry::default();
    };
    let pos = xfrm.child("off").and_then(|off| {
        Some(Point::new(
            Length::from_emu(off.attr_i64("x")?),
            Length::from_emu(off.attr_i64("y")?),
        ))
    });
    let size = xfrm.child("ext").and_then(|ext| {
        Some(Size::new(
            Length::from_emu(ext.attr_i64("cx")?),
            Length::from_emu(ext.attr_i64("cy")?),
        ))
    });
    ShapeGeometry {
        pos,
        size,
        // The caller fills this in: the preset lives on `a:prstGeom`, a sibling
        // of the transform rather than a part of it.
        preset: None,
        // DrawingML angles are sixtieth-thousandths of a degree.
        rotation_deg: xfrm.attr_i64("rot").unwrap_or(0) as f32 / 60_000.0,
        flip: docsai_model::image::Flip::from_flags(
            xfrm.attr("flipH").is_some_and(|v| v != "0"),
            xfrm.attr("flipV").is_some_and(|v| v != "0"),
        ),
    }
}

/// `p:cSld@name`, when the part names itself.
fn csld_name(root: &Element) -> Option<String> {
    root.child("cSld")
        .and_then(|c| c.attr("name"))
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// `ppt/slideLayouts/slideLayout1.xml` → `slideLayout1`. The fallback name for
/// a part that does not name itself, which the hand-built corpus does not.
fn part_stem(part: &str) -> String {
    let file = part.rsplit('/').next().unwrap_or(part);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem).into()
}

fn parse(package: &Package, part: &str) -> Result<Element, ReadError> {
    let source = package.text(part)?;
    Element::parse(part, source.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::presentation::RawShape;
    use docsai_model::report::Severity;
    use docsai_model::text::Block;
    use docsai_model::MemoryAssetStore;

    fn fixture_path(name: &str) -> String {
        format!(
            "{}/../../corpus/pptx/{name}.pptx",
            env!("CARGO_MANIFEST_DIR")
        )
    }

    fn read_fixture(name: &str) -> (Presentation, ConversionReport) {
        let mut assets = MemoryAssetStore::new();
        read_fixture_into(name, &mut assets)
    }

    fn read_fixture_into(
        name: &str,
        assets: &mut dyn AssetStore,
    ) -> (Presentation, ConversionReport) {
        let path = fixture_path(name);
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let (document, report) = read(file, assets).unwrap_or_else(|e| panic!("{name}: {e}"));
        match document {
            Document::Presentation(deck) => (deck, report),
            other => panic!("expected a presentation, got {}", other.shape_name()),
        }
    }

    #[test]
    fn a_deck_reads_its_slides_its_size_and_its_layouts() {
        let (deck, report) = read_fixture("basic-slides");
        assert_eq!(deck.slides.len(), 2);
        assert_eq!(report.stats.slides, 2);
        assert_eq!(deck.slide_size.width.emu(), 9_144_000);
        assert_eq!(deck.slide_size.height.emu(), 6_858_000);
        assert_eq!(deck.meta.title.as_deref(), Some("Presentación básica"));
        assert_eq!(deck.layouts.layouts.len(), 1);
        assert_eq!(deck.layouts.masters.len(), 1);
        assert!(
            report.warnings.is_empty(),
            "a well-formed deck warns about nothing: {:?}",
            report.warnings
        );
    }

    #[test]
    fn slide_order_comes_from_the_id_list_not_the_file_names() {
        // `slide-order.pptx` numbers its parts against the presentation order on
        // purpose: a reader that sorts part names gets this fixture backwards.
        let (deck, _) = read_fixture("slide-order");
        let parts = order_of("slide-order");
        assert_eq!(
            parts,
            [
                "ppt/slides/slide2.xml",
                "ppt/slides/slide3.xml",
                "ppt/slides/slide1.xml"
            ],
            "the deck's `p:sldIdLst` is 2, 3, 1 — file order would be 1, 2, 3"
        );
        assert_eq!(deck.slides.len(), 3);
    }

    /// The slide parts, in the order the reader visits them.
    fn order_of(name: &str) -> Vec<String> {
        let path = format!(
            "{}/../../corpus/pptx/{name}.pptx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap();
        let package = Package::open(file).unwrap();
        let types = ContentTypes::read(&package);
        let main = main_part(&package, &types).unwrap();
        let root = parse(&package, &main).unwrap();
        let rels = package.relationships(&main);
        slide_entries(&root)
            .into_iter()
            .filter_map(|entry| Some(rels.get(entry.rel_id.as_deref()?)?.target.clone()))
            .collect()
    }

    #[test]
    fn every_slide_names_the_layout_it_inherits_from() {
        let (deck, _) = read_fixture("placeholders-cascade");
        let layout_id = LayoutId::new("ppt/slideLayouts/slideLayout1.xml");
        for slide in &deck.slides {
            assert_eq!(slide.layout.as_ref(), Some(&layout_id));
        }
        let layout = deck.layouts.layout(&layout_id).expect("catalogued");
        assert_eq!(layout.name, "Titulo y objetos", "`p:cSld@name` wins");
        assert!(layout.title().is_some(), "the layout declares a title");
        assert_eq!(layout.body().and_then(|p| p.idx), Some(1));
        let master = deck.layouts.master_of(&layout_id).expect("reachable");
        assert_eq!(
            master.theme.as_deref(),
            Some("ppt/theme/theme1.xml"),
            "the theme is a reference; the part itself stays in the package"
        );
    }

    #[test]
    fn a_master_placeholder_keeps_the_geometry_its_layout_omits() {
        let (deck, _) = read_fixture("basic-slides");
        let master = deck.layouts.masters.values().next().expect("one master");
        let title = master
            .placeholders
            .iter()
            .find(|p| p.ph_type.is_title())
            .expect("the master positions the title");
        assert_eq!(title.geometry.pos.unwrap().x.emu(), 838_200);
        assert_eq!(title.geometry.size.unwrap().width.emu(), 7_467_600);

        let layout = deck.layouts.layouts.values().next().expect("one layout");
        let layout_title = layout
            .placeholders
            .iter()
            .find(|p| p.ph_type.is_title())
            .expect("the layout redeclares it");
        assert!(
            layout_title.geometry.is_inherited(),
            "a layout placeholder with no `a:xfrm` inherits, it does not sit at zero"
        );
    }

    #[test]
    fn a_slide_carries_its_placeholders_with_their_identity_and_text() {
        let (deck, report) = read_fixture("basic-slides");
        let slide = &deck.slides[0];
        assert_eq!(slide.shapes.len(), 2);
        assert_eq!(slide.title().as_deref(), Some("Informe trimestral"));

        let title = &slide.shapes[0];
        assert_eq!(title.name.as_deref(), Some("Title 1"));
        assert_eq!(title.z_index, 0);
        let ph = title.placeholder().expect("a title placeholder");
        assert_eq!(ph.ph_type, PhType::Title);
        assert!(
            title.geometry.is_inherited() && ph.delta.is_empty(),
            "the slide states nothing of its own: it inherits from the layout"
        );

        let body = deck.slides[0].shapes[1]
            .placeholder()
            .expect("a body placeholder");
        assert_eq!(body.idx, Some(1));
        // The master's `bodyStyle` bullets it; the slide says nothing at all.
        let [Block::List(list)] = body.body.as_slice() else {
            panic!("expected the body to be one list, got {:?}", body.body);
        };
        assert_eq!(list.items.len(), 2);
        assert_eq!(report.stats.lists, 2, "one body list per slide");
    }

    #[test]
    fn a_placeholder_that_inherits_everything_stores_nothing() {
        // The acceptance test of the cascade, and the one the on-ramp asks for
        // by name: `placeholders-cascade` states no geometry, no font, no size
        // and no colour anywhere on its slide. Every one of those is decided by
        // the layout, the master and the theme, so the slide must come back
        // empty of properties — a reader that copied the resolved cascade onto
        // the shape would round-trip a deck no theme change could ever restyle.
        let (deck, report) = read_fixture("placeholders-cascade");
        let slide = &deck.slides[0];
        assert_eq!(slide.shapes.len(), 2);

        for shape in &slide.shapes {
            let ph = shape.placeholder().expect("a placeholder");
            assert!(
                shape.geometry.is_inherited(),
                "{:?} states geometry of its own",
                shape.name
            );
            assert!(
                ph.delta.is_empty(),
                "{:?} states properties of its own: {:?}",
                shape.name,
                ph.delta
            );
            for block in &ph.body {
                assert!(
                    block_is_unstyled(block),
                    "{:?} carries run properties it inherits: {block:?}",
                    shape.name
                );
            }
        }
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| matches!(w, Warning::Degraded { .. })),
            "nothing was lost resolving the cascade: {:?}",
            report.warnings
        );
    }

    /// True when no run in the block carries any direct formatting.
    fn block_is_unstyled(block: &Block) -> bool {
        fn paragraph_is_unstyled(p: &docsai_model::text::Paragraph) -> bool {
            p.format.run_direct.is_empty()
                && p.content
                    .iter()
                    .all(|inline| !matches!(inline, docsai_model::text::Inline::Styled { .. }))
        }
        match block {
            Block::Paragraph(p) => paragraph_is_unstyled(p),
            Block::List(list) => list
                .items
                .iter()
                .flat_map(|item| item.blocks.iter())
                .all(block_is_unstyled),
            _ => true,
        }
    }

    #[test]
    fn the_layout_resolves_the_references_the_slide_leaves_open() {
        // The other half of reference + delta: the slide stores nothing because
        // the layout stores the answer. `+mj-lt` and `tx1` are resolved here,
        // once, instead of on every run of every slide.
        let (deck, _) = read_fixture("placeholders-cascade");
        let id = deck.slides[0].layout.clone().expect("a layout");
        let layout = deck
            .layouts
            .layout(&id)
            .expect("the layout in the catalogue");

        let title = layout.title().expect("a title placeholder");
        assert_eq!(title.props.font.name.as_deref(), Some("Calibri Light"));
        assert_eq!(title.props.font.size, Some(Length::from_pt(44.0)));
        assert_eq!(title.props.font.color.as_deref(), Some("#000000"));

        let body = layout.body().expect("a body placeholder");
        assert_eq!(body.props.font.name.as_deref(), Some("Calibri"));
        assert_eq!(body.props.font.size, Some(Length::from_pt(28.0)));

        // The master says the same thing, because that is where it is written.
        let master = deck
            .layouts
            .master_of(&id)
            .expect("the master behind the layout");
        assert_eq!(
            master
                .placeholders
                .iter()
                .find(|p| p.ph_type.is_title())
                .map(|p| p.props.font.size),
            Some(Some(Length::from_pt(44.0)))
        );
    }

    #[test]
    fn an_empty_placeholder_keeps_its_place_on_the_slide() {
        // The pptx echo of the Phase 1 empty-paragraph bug: an empty box is a
        // box a user is expected to fill, not an absence.
        let (deck, _) = read_fixture("placeholders-empty");
        let empty = deck.slides[0].shapes[1]
            .placeholder()
            .expect("the empty body is still a placeholder");
        assert_eq!(empty.idx, Some(1));
        assert_eq!(empty.body.len(), 1, "one empty paragraph, not zero blocks");
        assert!(matches!(&empty.body[0], Block::Paragraph(p) if p.is_empty()));
    }

    #[test]
    fn a_free_text_box_is_not_a_placeholder() {
        let (deck, _) = read_fixture("shapes-geometry");
        let boxes: Vec<&Shape> = deck.slides[0]
            .shapes
            .iter()
            .filter(|shape| matches!(shape.kind, ShapeKind::TextBox { .. }))
            .collect();
        assert_eq!(boxes.len(), 2, "a rectangle with text and an empty arrow");
        assert_eq!(boxes[0].name.as_deref(), Some("Rectángulo 1"));
        assert_eq!(boxes[0].geometry.pos.unwrap().y.emu(), 2_000_000);
        let ShapeKind::TextBox { body } = &boxes[0].kind else {
            unreachable!()
        };
        // A free text box has no master to bullet it.
        assert!(matches!(&body[0], Block::Paragraph(_)), "{body:?}");
    }

    #[test]
    fn bullet_levels_become_a_nested_list_and_autonum_its_own() {
        let (deck, _) = read_fixture("bullets-levels");
        let slide = &deck.slides[0];
        let body = slide.shapes[1].placeholder().expect("the first body");
        let [Block::List(list)] = body.body.as_slice() else {
            panic!("expected one list, got {:?}", body.body);
        };
        assert_eq!(list.items.len(), 2, "two level-0 items, the rest nested");
        assert!(!list.ordered);

        let numbered = slide.shapes[2].placeholder().expect("the second body");
        let [Block::List(list)] = numbered.body.as_slice() else {
            panic!("expected one list, got {:?}", numbered.body);
        };
        assert!(list.ordered, "`a:buAutoNum` is a numbered list");
    }

    #[test]
    fn a_stale_autofit_scale_is_kept_and_reported() {
        // Risk P5: the scale PowerPoint computed for the text that was in the
        // box when a human last edited it. Only PowerPoint can recompute it, so
        // it is kept — and reported, because an agent that adds a line has just
        // made it a lie and nothing in the file says so.
        let (deck, report) = read_fixture("autofit-stale");
        let body = deck.slides[0].shapes[1].placeholder().expect("the body");
        assert_eq!(body.delta.font_scale, Some(62.5));
        assert!(
            report
                .warnings
                .iter()
                .any(|w| matches!(w, Warning::AutofitStale { scale, .. } if *scale == 62_500)),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn a_group_and_a_custom_geometry_become_stubs_that_keep_their_text() {
        let (deck, report) = read_fixture("raw-preserved");
        let slide = &deck.slides[0];
        let stubs: Vec<(&RawShape, &Shape)> = slide
            .shapes
            .iter()
            .filter_map(|shape| match &shape.kind {
                ShapeKind::Raw(raw) => Some((raw, shape)),
                _ => None,
            })
            .collect();
        assert_eq!(stubs.len(), 2, "the group and the custom geometry");

        let (group, group_shape) = stubs[0];
        assert_eq!(group.kind, RawShapeKind::Shape);
        assert_eq!(group_shape.name.as_deref(), Some("Grupo 1"));
        // A group states its transform on `p:grpSpPr`, not on `p:spPr`, and a
        // stub with no position would be read last on every slide.
        assert_eq!(group_shape.geometry.pos.unwrap().x.emu(), 838_200);
        assert!(
            group.text.contains("Dentro del grupo") && group.text.contains("También dentro"),
            "a stub that swallowed the group's text would hide real content: {:?}",
            group.text
        );

        let (custom, _) = stubs[1];
        assert_eq!(custom.kind, RawShapeKind::Shape);
        assert_eq!(custom.text, "Etiqueta del triángulo");
        let fragment = deck
            .raw
            .iter()
            .find(|f| Some(&f.id) == custom.raw.as_ref())
            .expect("the payload is in the deck");
        assert!(
            fragment.content.contains("<a:custGeom>") && fragment.content.contains("<a:close/>"),
            "the path list is the shape, and it travels verbatim"
        );

        // Every stub's payload is really there, and every fragment is a slice
        // of a part rather than a re-serialisation.
        assert_eq!(report.raw_blocks_emitted as usize, deck.raw.len());
        assert!(deck.raw.iter().all(|f| f.format == "ooxml"));
    }

    #[test]
    fn a_slides_transition_and_timing_no_longer_vanish() {
        // Both are slide-level subtrees with no IR node and no Markdown
        // representation. Until this increment they were dropped without even a
        // warning, which is the one thing this project may not do.
        let (deck, _) = read_fixture("raw-preserved");
        let slide = &deck.slides[0];
        assert_eq!(slide.raw.len(), 2);
        let kept: Vec<&str> = slide
            .raw
            .iter()
            .map(|id| {
                deck.raw
                    .iter()
                    .find(|f| &f.id == id)
                    .expect("the payload is in the deck")
                    .content
                    .as_str()
            })
            .collect();
        assert!(kept[0].starts_with("<p:transition"), "{:?}", kept[0]);
        assert!(kept[1].starts_with("<p:timing"), "{:?}", kept[1]);
        assert!(kept[1].contains("tmRoot"));
    }

    #[test]
    fn a_connector_is_a_stub_and_a_preset_shape_keeps_its_outline() {
        let (deck, _) = read_fixture("shapes-geometry");
        let slide = &deck.slides[0];
        let connector = slide
            .shapes
            .iter()
            .find_map(|shape| match &shape.kind {
                ShapeKind::Raw(raw) => Some(raw),
                _ => None,
            })
            .expect("the connector is preserved, not dropped");
        assert_eq!(connector.kind, RawShapeKind::Connector);

        // A preset shape keeps its text as a box — and its outline, which used
        // to be lost: a `rightArrow` read as a plain box comes back a box.
        let arrow = slide
            .shapes
            .iter()
            .find(|shape| shape.name.as_deref() == Some("Flecha 1"))
            .expect("the arrow");
        assert_eq!(arrow.geometry.preset.as_deref(), Some("rightArrow"));
    }

    #[test]
    fn a_chart_is_recorded_and_its_markup_preserved() {
        // A chart arrives in the same `p:graphicFrame` a table does, and only
        // the `a:graphicData@uri` tells them apart. Phase 13 records that one is
        // there and keeps its XML; the series are Phase 16's.
        let (deck, report) = read_fixture("charts-embedded");
        let chart = deck.slides[0]
            .shapes
            .iter()
            .find_map(|shape| match &shape.kind {
                ShapeKind::Chart(chart) => Some(chart),
                _ => None,
            })
            .expect("the frame held a chart");
        assert_eq!(chart.kind.as_deref(), Some("barChart"));

        let raw = chart.raw.as_ref().expect("its markup travels with it");
        let fragment = deck
            .raw
            .iter()
            .find(|f| &f.id == raw)
            .expect("the payload is in the deck");
        assert_eq!(fragment.part, "ppt/charts/chart1.xml");
        assert!(fragment.content.starts_with("<c:chartSpace"));
        assert!(
            fragment.content.contains("1200"),
            "the values are in the fragment, not summarised out of it"
        );
        assert_eq!(report.raw_blocks_emitted, 1);
    }

    #[test]
    fn smartart_is_stubbed_whole_rather_than_half_read() {
        // SmartArt reaches a slide as `mc:AlternateContent`: a `mc:Choice`
        // holding the diagram and a `mc:Fallback` holding shapes that draw it.
        // Reading either branch would be a decision this reader has no business
        // taking, so the pair is preserved and stubbed.
        let (deck, report) = read_fixture("smartart-fallback");
        let stubs: Vec<&RawShape> = deck.slides[0]
            .shapes
            .iter()
            .filter_map(|shape| match &shape.kind {
                ShapeKind::Raw(raw) => Some(raw),
                _ => None,
            })
            .collect();
        // One stub, not two: the diagram's `p:graphicFrame` is *inside* the
        // `mc:Choice`, and descending into a branch would be picking one.
        assert_eq!(stubs.len(), 1);
        let stub = stubs[0];
        // Named for what it wraps, which is not the same as reading a branch:
        // the `inspect` inventory reports has-SmartArt from this, and a stub
        // that said «other» would leave an agent blind to the one thing on the
        // slide it must not hand-edit.
        assert_eq!(stub.kind, RawShapeKind::SmartArt);
        let fragment = deck
            .raw
            .iter()
            .find(|f| Some(&f.id) == stub.raw.as_ref())
            .expect("the payload is in the deck");
        assert!(fragment.content.contains("mc:Fallback"));
        assert!(
            report.warnings.iter().any(|w| matches!(
                w,
                Warning::UnsupportedElement { kind, action, .. }
                    if kind == "mc:AlternateContent" && action == "raw-block"
            )),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn a_macro_enabled_deck_reads_as_its_macro_free_equivalent() {
        // `.pptm` differs from `.pptx` in its main content type and in carrying
        // a VBA project. Neither changes what a slide says, so the deck reads
        // whole — and the ignored project is stated, not assumed.
        let path = format!(
            "{}/../../corpus/pptx/macro-enabled.pptm",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut assets = MemoryAssetStore::new();
        let (document, report) = read(file, &mut assets).expect("a .pptm is a readable deck");
        let Document::Presentation(deck) = document else {
            panic!("expected a presentation");
        };
        assert_eq!(deck.slides.len(), 1);
        assert!(
            slide_shapes(&deck) >= 2,
            "the slides of a macro-enabled deck are ordinary slides"
        );
        let macros = report
            .warnings
            .iter()
            .find(|w| matches!(w, Warning::MacrosIgnored { .. }))
            .expect("the ignored macro project is reported");
        assert!(
            matches!(macros, Warning::MacrosIgnored { part } if part == "ppt/vbaProject.bin"),
            "{macros:?}"
        );
        // Informational: nothing about the document was degraded. The project
        // was never part of the IR, and the skeleton still holds it verbatim.
        assert_eq!(macros.severity(), Severity::Info);
        assert!(deck.skeleton.is_some(), "the package is preserved whole");
    }

    #[test]
    fn a_macro_enabled_deck_is_detected_by_its_content() {
        // The extension is the one thing detection may not rely on: a `.pptm`
        // renamed `.pptx` is still a deck, and either name reaches the same
        // reader (architecture §4).
        let path = format!(
            "{}/../../corpus/pptx/macro-enabled.pptm",
            env!("CARGO_MANIFEST_DIR")
        );
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let (format, score) =
            crate::detect(std::io::Cursor::new(&bytes), Some("macro-enabled.pptm"));
        assert_eq!(format, docsai_model::Format::Pptx);
        assert_eq!(score, crate::DetectScore::Certain);
    }

    #[test]
    fn a_slide_picture_is_stored_once_and_placed_by_its_shape() {
        let (deck, report) = read_fixture("images-anchored");
        let mut assets = MemoryAssetStore::new();
        let path = format!(
            "{}/../../corpus/pptx/images-anchored.pptx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap();
        read(file, &mut assets).unwrap();

        let picture = &deck.slides[0].shapes[1];
        assert_eq!(picture.name.as_deref(), Some("Imagen 1"));
        assert_eq!(picture.z_index, 1, "it follows the title in the tree");
        // The placement is the shape's, like every other shape on the slide.
        assert_eq!(picture.geometry.pos.unwrap().x.emu(), 1_524_000);
        assert_eq!(picture.geometry.pos.unwrap().y.emu(), 2_286_000);

        let ShapeKind::Picture(image) = &picture.kind else {
            panic!("expected a picture, got {:?}", picture.kind);
        };
        assert_eq!(
            image.alt, "Gráfico de barras azul",
            "`descr` is alt text and travels in the Markdown `![…]` slot"
        );
        assert_eq!(image.geometry.display_size.width.emu(), 1_143_000);
        assert_eq!(image.geometry.display_size.height.emu(), 857_250);
        assert!(
            image.geometry.native_size_px.is_some(),
            "the bitmap's own header says how many pixels it has"
        );
        assert!(
            assets.get(&image.asset).is_some(),
            "the media part is in the store, keyed by content hash"
        );
        assert_eq!(report.stats.images, 1);
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| matches!(w, Warning::Degraded { .. })),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn a_slide_table_is_the_same_table_a_document_carries() {
        let (deck, report) = read_fixture("tables-simple");
        let frame = &deck.slides[0].shapes[1];
        assert_eq!(frame.name.as_deref(), Some("Tabla 1"));
        // `p:graphicFrame` states its transform one level up from a `p:sp`.
        assert_eq!(frame.geometry.pos.unwrap().y.emu(), 1_825_625);

        let ShapeKind::Table(table) = &frame.kind else {
            panic!("expected a table, got {:?}", frame.kind);
        };
        assert_eq!(table.col_widths.len(), 2);
        assert_eq!(table.col_widths[0].emu(), 3_733_800);
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.width(), 2);
        assert!(
            table.header_row && table.rows[0].is_header,
            "`a:tblPr@firstRow` is what makes the first row a header"
        );
        assert!(!table.is_complex(), "one paragraph per cell");
        assert_eq!(cell_text(&table.rows[0].cells[0]), "Región");
        assert_eq!(cell_text(&table.rows[2].cells[1]), "980");
        assert_eq!(report.stats.tables, 1);
    }

    fn cell_text(cell: &docsai_model::text::TableCell) -> String {
        cell.blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(p) => Some(p.plain_text()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn drawingml_states_a_merge_on_both_the_spanning_cell_and_the_covered_one() {
        // No corpus deck merges a cell, and the merge model is where the two
        // OOXML dialects disagree: WordprocessingML continues a `vMerge` and
        // leaves the reader to count, DrawingML writes the span on the origin
        // and marks every cell it swallowed. Reading the covered cells' text
        // would duplicate the spanning cell's content across the grid.
        let xml = r#"<a:tbl xmlns:a="urn:a" xmlns:p="urn:p">
            <a:tblPr firstRow="1"><a:tableStyleId>{5C22544A}</a:tableStyleId></a:tblPr>
            <a:tblGrid><a:gridCol w="100"/><a:gridCol w="200"/></a:tblGrid>
            <a:tr h="10">
              <a:tc gridSpan="2"><p:txBody><a:p><a:r><a:t>Ambas</a:t></a:r></a:p></p:txBody>
                <a:tcPr><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:tcPr></a:tc>
              <a:tc hMerge="1"><p:txBody><a:p><a:r><a:t>Ambas</a:t></a:r></a:p></p:txBody><a:tcPr/></a:tc>
            </a:tr>
            <a:tr h="10">
              <a:tc rowSpan="2"><p:txBody><a:p><a:r><a:t>Alta</a:t></a:r></a:p></p:txBody><a:tcPr/></a:tc>
              <a:tc><p:txBody><a:p><a:r><a:t>x</a:t></a:r></a:p></p:txBody><a:tcPr/></a:tc>
            </a:tr>
            <a:tr h="10">
              <a:tc vMerge="1"><p:txBody><a:p><a:r><a:t>Alta</a:t></a:r></a:p></p:txBody><a:tcPr/></a:tc>
              <a:tc><p:txBody><a:p><a:r><a:t>y</a:t></a:r></a:p></p:txBody><a:tcPr/></a:tc>
            </a:tr>
        </a:tbl>"#;
        let root = Element::parse("t.xml", xml.as_bytes()).unwrap();
        let mut report = ConversionReport::new();
        let table = graphics::read_table(
            &root,
            &Default::default(),
            &Relationships::default(),
            &mut report,
        );

        assert_eq!(table.style.as_ref().map(|s| s.as_str()), Some("{5C22544A}"));
        assert_eq!(
            table.col_widths,
            vec![Length::from_emu(100), Length::from_emu(200)]
        );
        assert_eq!(table.rows[0].cells[0].colspan, 2);
        assert_eq!(
            table.rows[0].cells[0].background.as_deref(),
            Some("#ff0000")
        );
        assert_eq!(
            table.rows[0].cells.len(),
            1,
            "the `hMerge` cell is not in the grid: the colspan already fills it"
        );
        assert_eq!(table.rows[1].cells[0].rowspan, 2);
        assert!(table.rows[2].cells[0].covered, "the row below is swallowed");
        assert!(
            table.rows[2].cells[0].blocks.is_empty(),
            "a covered cell holds no text of its own: PowerPoint repeats the \
             spanning cell's text there and reading it would duplicate it"
        );
        assert_eq!(table.width(), 2, "the spans add up to the grid");
    }

    #[test]
    fn speaker_notes_are_reached_through_the_slide_relationships() {
        let (deck, report) = read_fixture("notes-speaker");
        let first = deck.slides[0].notes.as_ref().expect("the slide has notes");
        assert_eq!(first.len(), 2, "two paragraphs, not one bulleted list");
        assert_eq!(
            first.iter().map(block_text).collect::<Vec<_>>(),
            [
                "Insistir en que el 12 % es interanual.",
                "No entrar en el desglose por región."
            ]
        );
        assert!(
            first.iter().all(|b| matches!(b, Block::Paragraph(_))),
            "notes are prose: the notes master is what would bullet them"
        );
        let second = deck.slides[1]
            .notes
            .as_ref()
            .expect("both slides have notes");
        assert_eq!(
            block_text(&second[0]),
            "Si preguntan por el proveedor, remitir al anexo."
        );
        assert!(
            report.warnings.is_empty(),
            "the notes page's slide-image placeholder is furniture, not a loss: {:?}",
            report.warnings
        );
        // A deck without a notes part gets `None`, which is what tells the
        // writer not to create one.
        let (plain, _) = read_fixture("basic-slides");
        assert!(plain.slides.iter().all(|slide| slide.notes.is_none()));
    }

    #[test]
    fn notes_follow_the_relationship_even_when_the_numbering_disagrees() {
        // `notesSlide7.xml` belonging to slide 7 is PowerPoint's habit, not a
        // rule. This fixture crosses them over, so a reader that matched on the
        // part name would put every note under the wrong slide — silently, and
        // with text that looks plausible where it lands.
        let (deck, _) = read_fixture("notes-crossed");
        assert_eq!(
            deck.slides[0].notes.as_ref().map(|n| block_text(&n[0])),
            Some("Nota de la primera diapositiva.".to_string())
        );
        assert_eq!(
            deck.slides[1].notes.as_ref().map(|n| block_text(&n[0])),
            Some("Nota de la segunda diapositiva.".to_string())
        );
    }

    #[test]
    fn a_slide_comes_back_in_reading_order_and_remembers_the_other_one() {
        // The fixture's `p:spTree` is z-order: a footnote box first, the title
        // fourth. Reading order is computed from the slide — title, body, then
        // the free boxes by top-left — and the two boxes 10 000 EMU apart
        // vertically are one row, read left to right.
        let (deck, _) = read_fixture("reading-order");
        let shapes = &deck.slides[0].shapes;
        let names: Vec<&str> = shapes
            .iter()
            .map(|shape| shape.name.as_deref().unwrap_or(""))
            .collect();
        assert_eq!(
            names,
            vec![
                "Title 1",
                "Content Placeholder 2",
                "Marca izquierda",
                "Etiqueta derecha",
                "Pie 1",
            ]
        );

        // What makes the reordering admissible: the source order is still on
        // the shapes, so a writer can put the tree back exactly as it was.
        assert_eq!(
            shapes.iter().map(|shape| shape.z_index).collect::<Vec<_>>(),
            vec![3, 1, 4, 2, 0]
        );
    }

    #[test]
    fn the_original_package_is_kept_whole_and_opaque() {
        // Spike P3's rule: preserve everything, rebuild as the exception. What
        // the skeleton holds is the file itself — not a re-zip of the parts,
        // which would already have lost the member order and the compression
        // the deck was written with.
        let mut assets = MemoryAssetStore::new();
        let (deck, report) = read_fixture_into("charts-embedded", &mut assets);
        let skeleton = deck.skeleton.expect("the deck keeps its package");

        let stored = assets.get(&skeleton.asset).expect("stored opaquely");
        let original = std::fs::read(fixture_path("charts-embedded")).unwrap();
        assert_eq!(stored, original, "byte for byte, not a rewrite");

        // And it is the whole package, embedded workbook included — the part
        // the naive control of spike P3 lost twelve chart values to.
        let kept = Package::open(std::io::Cursor::new(stored)).unwrap();
        assert!(kept.part_names().any(|p| p.ends_with(".xlsx")));
        assert!(kept.has_part("ppt/theme/theme1.xml"));
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| matches!(w, Warning::Degraded { what, .. } if what.contains("skeleton"))),
            "{:?}",
            report.warnings
        );
    }

    #[test]
    fn the_skeleton_names_only_the_parts_the_ir_holds() {
        // `rebuilt_parts` is a licence to regenerate, and it is granted for
        // exactly the parts this reader turned into IR. The theme, the master,
        // the layouts and `docProps` are not in it: nothing in the IR could
        // reproduce them, so the writer copies them.
        let (deck, _) = read_fixture("notes-speaker");
        let skeleton = deck.skeleton.expect("the deck keeps its package");
        assert_eq!(
            skeleton.rebuilt_parts,
            vec![
                "ppt/notesSlides/notesSlide1.xml",
                "ppt/notesSlides/notesSlide2.xml",
                "ppt/slides/slide1.xml",
                "ppt/slides/slide2.xml",
            ]
        );

        let (plain, _) = read_fixture("basic-slides");
        assert_eq!(
            plain.skeleton.expect("kept too").rebuilt_parts,
            vec!["ppt/slides/slide1.xml", "ppt/slides/slide2.xml"],
            "a deck with no notes rebuilds no notes part"
        );
    }

    #[test]
    fn the_same_deck_read_twice_is_stored_once() {
        // The skeleton is content-hashed like every other asset, so a batch
        // that reads one deck repeatedly does not store it repeatedly.
        let mut assets = MemoryAssetStore::new();
        let (first, _) = read_fixture_into("basic-slides", &mut assets);
        assert_eq!(assets.len(), 1);
        let (second, _) = read_fixture_into("basic-slides", &mut assets);
        assert_eq!(assets.len(), 1);
        assert_eq!(first.skeleton, second.skeleton);
    }

    fn block_text(block: &Block) -> String {
        match block {
            Block::Paragraph(p) => p.plain_text(),
            other => panic!("expected a paragraph, got {other:?}"),
        }
    }

    #[test]
    fn every_deck_in_the_corpus_gets_through_the_package_layer() {
        // The package layer is format-wide: a deck with SmartArt, a chart or an
        // image has the same masters, layouts and slide list as a plain one, and
        // none of them may fail before the shapes are even read.
        let dir = format!("{}/../../corpus/pptx", env!("CARGO_MANIFEST_DIR"));
        let mut decks: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().into_string().ok()?;
                name.strip_suffix(".pptx").map(str::to_string)
            })
            .collect();
        decks.sort();
        assert!(decks.len() >= 12, "the Phase 12 corpus has twelve decks");
        for name in decks {
            let (deck, report) = read_fixture(&name);
            assert!(!deck.slides.is_empty(), "{name}: no slides");
            assert!(
                !deck.layouts.is_empty(),
                "{name}: no layout catalogue to inherit from"
            );
            assert!(
                slide_shapes(&deck) > 0,
                "{name}: slides without a single shape"
            );
            // Unsupported-element warnings are the shape kinds later increments
            // read. A *degraded* warning is the package layer failing, and
            // there must be none.
            assert!(
                !report
                    .warnings
                    .iter()
                    .any(|w| matches!(w, Warning::Degraded { .. })),
                "{name} degrades at the package layer: {:?}",
                report.warnings
            );
        }
    }

    fn slide_shapes(deck: &Presentation) -> usize {
        deck.slides.iter().map(|s| s.shapes.len()).sum()
    }

    #[test]
    fn sections_map_slides_by_their_deck_id() {
        // No corpus fixture carries `p14:sectionLst` yet, and the mapping is a
        // pure function of the presentation part, so it is tested as one.
        let xml = r#"<p:presentation xmlns:p="urn:p" xmlns:p14="urn:p14">
            <p:sldIdLst><p:sldId id="256" r:id="rId2"/><p:sldId id="257" r:id="rId3"/></p:sldIdLst>
            <p:extLst><p:ext uri="{521415D9-36F7-43E2-AB2F-B90AF26B5E84}">
              <p14:sectionLst>
                <p14:section name="Intro" id="{1}">
                  <p14:sldIdLst><p14:sldId id="256"/></p14:sldIdLst>
                </p14:section>
                <p14:section name="Cifras" id="{2}">
                  <p14:sldIdLst><p14:sldId id="257"/></p14:sldIdLst>
                </p14:section>
              </p14:sectionLst>
            </p:ext></p:extLst>
        </p:presentation>"#;
        let root = Element::parse("p.xml", xml.as_bytes()).unwrap();
        let sections = read_sections(&root);
        assert_eq!(sections.get(&256).map(String::as_str), Some("Intro"));
        assert_eq!(sections.get(&257).map(String::as_str), Some("Cifras"));

        let entries = slide_entries(&root);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id, 256);
        assert_eq!(
            entries[0].rel_id.as_deref(),
            Some("rId2"),
            "`r:id` and `id` are different attributes on the same element"
        );
    }

    #[test]
    fn a_package_without_a_presentation_part_is_an_error_not_a_panic() {
        let path = format!(
            "{}/../../corpus/docx/basic-text.docx",
            env!("CARGO_MANIFEST_DIR")
        );
        let file = std::fs::File::open(&path).unwrap();
        let mut assets = MemoryAssetStore::new();
        assert!(matches!(
            read(file, &mut assets),
            Err(ReadError::MissingPart(_))
        ));
    }

    #[test]
    fn part_stems_name_the_parts_that_do_not_name_themselves() {
        assert_eq!(
            part_stem("ppt/slideLayouts/slideLayout1.xml"),
            "slideLayout1"
        );
        assert_eq!(part_stem("noslash"), "noslash");
    }
}
