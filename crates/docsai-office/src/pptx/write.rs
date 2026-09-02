//! The `.pptx` writer (Phase 15, increments 15-A to 15-C).
//!
//! The reader's counterpart, and the place where spike P3's rule is enforced
//! rather than restated:
//!
//! > **Preserve everything; rebuilding is the exception.**
//!
//! A deck read by [`super::read_pptx`] carries its whole original package in
//! the [`AssetStore`], opaque and byte for byte ([`super::skeleton`]). This
//! writer puts that package back and substitutes only the parts
//! [`SkeletonRef::rebuilt_parts`] licenses it to regenerate. Everything
//! else is copied: the theme, the masters, `tableStyles.xml`, the embedded
//! workbook of a chart, the four `dgm:` parts of a diagram, and the parts
//! nobody has looked at yet.
//!
//! **The archive is part of the fidelity.** A [`Package`](crate::package::Package)
//! is a `BTreeMap`, so a package rebuilt through it comes out alphabetised,
//! recompressed and re-timestamped even when every byte of every part is
//! identical. That is a different file, and a different file is a file
//! PowerPoint gets to have an opinion about. So the re-emission here works on
//! the ZIP itself: entry by entry, in the original order, copying the *stored*
//! bytes without a decompress/recompress cycle, keeping each entry's own
//! compression method, CRC and timestamp.
//!
//! Since 15-B the **text bodies** of a slide are rebuilt from the IR and spliced
//! back into the part they came from, which is spike P3's `rewrite` mode taken
//! one level further in: a placeholder's `a:p` sequence comes from the model,
//! and its `p:spPr`, its `a:bodyPr`, the shapes around it and the rest of the
//! part are the bytes that were read. A deck nobody edited therefore comes out
//! byte for byte as it went in — the rewriter reproduces what was already
//! there — while a retitled slide differs in exactly the run that was retyped.
//!
//! Since 15-C the same splice reaches **what a text body is not**: a picture's
//! image, alt text, drawn size and crop; the text of a table's cells; and the
//! hyperlinks a run carries. Those three need something the part alone cannot
//! give — a relationship id, a media part, a content type — so this module also
//! writes the *neighbouring* parts, and only the ones an edit actually moved:
//! a slide's `_rels` gains the relationships that were not in it, spliced in
//! before its closing tag rather than regenerated, and `[Content_Types].xml`
//! gains a `Default` for an extension the deck had never carried. Media bytes
//! go in **as they came out of the store**: the asset is the file that was
//! read, and nothing here re-encodes a bitmap.
//!
//! Since 15-D a **preserved object** — SmartArt, OLE, a group, a custom
//! geometry, an animation timeline — is not a loss at all: the splice leaves
//! its markup where the reader found it, so it goes out as the original XML at
//! its original z-order, and a payload the model carries that differs from the
//! package's is written back over it. A fragment in another dialect is dropped
//! with `Warning::RawBlockDropped` rather than emitted as nonsense.
//!
//! What the writer cannot yet put back it **keeps and reports**: a chart's
//! data (Phase 16), a notes page (15-E), a field, a table whose grid
//! changed, a slide added or removed. Each one is a `Degraded` warning naming
//! what was copied and the increment that lifts it, because a caller who edited
//! the model and got the original back has to be able to learn that from the
//! report rather than from the file (`AGENTS.md` §7 rule 3).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Seek, Write};
use std::ops::Range;

use docsai_model::assets::AssetStore;
use docsai_model::image::{ImageGeometry, ImageRef};
use docsai_model::presentation::{
    Placeholder, Presentation, RawShape, ShapeKind, SkeletonRef, Slide,
};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::{RawFragment, Table};
use docsai_model::units::{Length, Size};
use docsai_model::Document;

use crate::package::{rels_part_name, Package, Relationships};
use crate::write_error::WriteError;
use crate::xml::Element;

use super::write_text::{self, Links};

/// The relationship type of an embedded image, as the `_rels` part spells it.
const IMAGE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
/// The relationship type of a hyperlink.
const HYPERLINK_REL: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";

/// Writes a presentation as a `.pptx` package.
pub fn write_pptx<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    let Document::Presentation(deck) = document else {
        return Err(WriteError::Invalid(format!(
            "cannot write {} as .pptx",
            document.shape_name()
        )));
    };
    let mut report = ConversionReport::new();
    let skeleton = skeleton_bytes(deck, assets)?;
    let rebuild = rebuild_slides(deck, skeleton, assets, &mut report);

    reemit(skeleton, &rebuild, writer)?;
    Ok(report)
}

/// The bytes of the preserved package, or the reason there are none.
fn skeleton_bytes<'a>(
    deck: &Presentation,
    assets: &'a dyn AssetStore,
) -> Result<&'a [u8], WriteError> {
    let Some(SkeletonRef { asset, .. }) = deck.skeleton.as_ref() else {
        return Err(WriteError::Invalid(
            "this deck carries no preserved package, and generating one from an \
             embedded template is plan v2 Phase 15-G; a deck read from a .pptx \
             carries it, one parsed from DocMark carries it when the \
             `skeleton:` file is beside the document"
                .into(),
        ));
    };
    assets.get(asset).ok_or_else(|| {
        WriteError::Invalid(format!(
            "the deck names its preserved package as `{}` and the asset store \
             does not hold it",
            asset.as_str()
        ))
    })
}

/// The slide parts this deck's model can regenerate, as new part bytes.
///
/// The unit of rewriting is a **text body**, not a slide and not a package. The
/// part is otherwise the one that was read: its shape tree keeps its `p:spPr`,
/// its `a:bodyPr`, its transition and its timing, and a picture, a table or a
/// stub keeps its markup byte for byte. What changes is the `a:p` sequence
/// inside a placeholder or a text box, which is the content DocMark can hold
/// and therefore the content that can have been edited.
///
/// That is spike P3's `rewrite` mode rather than its `naive` one, applied one
/// level further in: preserve everything, and rebuild only what the IR models.
fn rebuild_slides(
    deck: &Presentation,
    skeleton: &[u8],
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Rebuild {
    let mut rebuild = Rebuild::default();
    let package = match Package::open(Cursor::new(skeleton)) {
        Ok(package) => package,
        Err(error) => {
            report.warn(Warning::Degraded {
                what: "every slide of the written deck".into(),
                why: format!(
                    "the preserved package could not be read back ({error}), so it \
                     was copied unchanged and no edit made to the model is in it"
                ),
            });
            return rebuild;
        }
    };

    let parts = match slide_parts(&package) {
        Some(parts) => parts,
        None => {
            report.warn(Warning::Degraded {
                what: "every slide of the written deck".into(),
                why: "the preserved package does not name its slides in a way this \
                      writer can follow, so it was copied unchanged"
                    .into(),
            });
            return rebuild;
        }
    };

    // A slide added or removed in the model moves `p:sldIdLst`, the package
    // relationships and the content types, which is Phase 15-E. Rewriting the
    // parts that happen to line up would put the edits of slide 3 into slide 4.
    if parts.len() != deck.slides.len() {
        report.warn(Warning::Degraded {
            what: "every slide of the written deck".into(),
            why: format!(
                "the model holds {} slide(s) and the preserved package {}; adding or \
                 removing a slide rewrites the deck's own parts, which is plan v2 \
                 Phase 15-E, so the package was copied unchanged",
                deck.slides.len(),
                parts.len()
            ),
        });
        return rebuild;
    }

    let mut media = Media::new(&package, assets);
    // The payloads are one deck-wide list and the stubs point into it by id
    // ([[43-phase-13-raw]] decision 3), so the writer indexes it once.
    let fragments: BTreeMap<&str, &RawFragment> = deck
        .raw
        .iter()
        .map(|fragment| (fragment.id.as_str(), fragment))
        .collect();

    for (index, (part, slide)) in parts.iter().zip(&deck.slides).enumerate() {
        let number = index + 1;
        let Ok(source) = package.text(part) else {
            report.warn(Warning::Degraded {
                what: format!("slide {number}"),
                why: format!("`{part}` is not readable as XML, so it was copied unchanged"),
            });
            continue;
        };
        let Ok(root) = Element::parse(part, source.as_bytes()) else {
            report.warn(Warning::Degraded {
                what: format!("slide {number}"),
                why: format!("`{part}` did not parse, so it was copied unchanged"),
            });
            continue;
        };
        // The notes page is its own part, and rewriting it moves the notes
        // master and the deck's relationships with it: 15-E. Copying it is
        // right; copying it in silence would not be.
        if slide.notes.is_some() {
            report.warn(Warning::Degraded {
                what: format!("the speaker notes of slide {number}"),
                why: "the notes page is a part of its own and is copied from the \
                      preserved package (plan v2 Phase 15-E), so an edit to the \
                      notes in the model is not in the file"
                    .into(),
            });
        }
        let mut rels = PartRels::new(&package, part);
        let mut outside = Outside {
            part,
            package: &package,
            rels: &mut rels,
            media: &mut media,
            fragments: &fragments,
        };
        match rewrite_slide(&root, source, slide, &mut outside) {
            Ok(Rewritten { xml, kept, dropped }) => {
                for (id, format) in dropped {
                    report.warn(Warning::RawBlockDropped { id, format });
                }
                for reason in kept {
                    report.warn(Warning::Degraded {
                        what: format!("slide {number}"),
                        why: format!("{reason}: it was copied from the preserved package, so an edit to it in the model is not in the file"),
                    });
                }
                if xml != source {
                    rebuild.replacements.insert(part.clone(), xml.into_bytes());
                }
                // The relationship part moves only when the slide gained a
                // target it did not have; an unedited slide reuses the ids it
                // was read with and leaves its `_rels` untouched.
                if let Some((name, xml)) = rels.rewritten() {
                    rebuild.put(&package, name, xml.into_bytes());
                }
            }
            Err(reason) => report.warn(Warning::Degraded {
                what: format!("slide {number}"),
                why: format!("{reason}, so the whole slide was copied unchanged"),
            }),
        }
    }

    for (name, bytes) in std::mem::take(&mut media.added) {
        rebuild.additions.insert(name, bytes);
    }
    if let Some((name, xml)) = content_types(&package, &media) {
        rebuild.put(&package, name, xml.into_bytes());
    }
    rebuild
}

/// What goes into the written archive that is not a raw copy.
///
/// Two maps rather than one, because the ZIP treats them differently: a
/// replacement keeps its member's position, compression and timestamp, and an
/// addition is a member the original package never had.
#[derive(Default)]
struct Rebuild {
    replacements: BTreeMap<String, Vec<u8>>,
    additions: BTreeMap<String, Vec<u8>>,
}

impl Rebuild {
    /// Files a part as whichever of the two it is.
    fn put(&mut self, package: &Package, name: String, bytes: Vec<u8>) {
        if package.has_part(&name) {
            self.replacements.insert(name, bytes);
        } else {
            self.additions.insert(name, bytes);
        }
    }
}

/// The slide parts of a package, in `p:sldIdLst` order — the reader's rule, so
/// the *n*th slide of the model is the *n*th part here whatever they are named.
fn slide_parts(package: &Package) -> Option<Vec<String>> {
    let types = crate::package::ContentTypes::read(package);
    let main = super::main_part(package, &types).ok()?;
    let root = super::parse(package, &main).ok()?;
    let rels = package.relationships(&main);
    Some(
        super::slide_entries(&root)
            .into_iter()
            .filter_map(|entry| rels.get(entry.rel_id.as_deref()?).cloned())
            .filter(|rel| !rel.external)
            .map(|rel| rel.target)
            .collect(),
    )
}

/// One slide part rewritten, and what inside it was left as it was.
struct Rewritten {
    xml: String,
    kept: Vec<String>,
    /// Fragments the model carried in a dialect this package cannot hold:
    /// their id and the format they were written in.
    dropped: Vec<(String, String)>,
}

/// Rewrites the text bodies of one slide, leaving every other byte alone.
/// Everything a slide's rewrite reaches outside the slide itself: the package
/// it was read from, the three neighbours an edit can move, and the payloads
/// the stubs point into.
struct Outside<'a, 'm> {
    /// Name of the part being rewritten, which is what says whether a
    /// fragment's markup belongs to it.
    part: &'a str,
    package: &'a Package,
    rels: &'a mut PartRels,
    media: &'a mut Media<'m>,
    fragments: &'a BTreeMap<&'a str, &'a RawFragment>,
}

fn rewrite_slide(
    root: &Element,
    source: &str,
    slide: &Slide,
    outside: &mut Outside<'_, '_>,
) -> Result<Rewritten, String> {
    let tree = root
        .path(&["cSld", "spTree"])
        .ok_or_else(|| "the slide has no shape tree".to_string())?;
    // The z-order the reader counted: `p:nvGrpSpPr` and `p:grpSpPr` are the
    // tree's own identity and transform, not shapes on it.
    let children: Vec<&Element> = tree
        .children()
        .filter(|child| !matches!(child.name.as_str(), "nvGrpSpPr" | "grpSpPr"))
        .collect();
    if children.len() != slide.shapes.len() {
        return Err(format!(
            "the model holds {} shape(s) and the slide {}; adding or removing a shape \
             is not this increment's",
            slide.shapes.len(),
            children.len()
        ));
    }

    let lang = editing_language(tree);
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    let mut dropped: Vec<(String, String)> = Vec::new();
    // What is not a shape: `p:transition`, `p:timing` and anything else the
    // reader took off `p:sld` itself, put back at the level it came from.
    for id in &slide.raw {
        // `p:cSld` is not a candidate whatever a fragment calls itself: an edit
        // spanning it would swallow every text edit inside the shape tree, and
        // `splice` drops an overlapping edit without a word.
        let candidates = root.children().filter(|child| child.name != "cSld");
        match reinject(candidates, source, id.as_str(), outside, &mut edits) {
            Reinjected::Standing => {}
            Reinjected::Dropped(format) => dropped.push((id.as_str().to_string(), format)),
            Reinjected::Kept(reason) => kept.push(reason),
        }
    }
    for shape in &slide.shapes {
        let Some(element) = children.get(shape.z_index as usize) else {
            return Err(format!(
                "shape z-index {} is outside the slide's shape tree",
                shape.z_index
            ));
        };
        let (body, bulleted) = match &shape.kind {
            ShapeKind::Placeholder(Placeholder { body, ph_type, .. }) => (body, ph_type.is_body()),
            ShapeKind::TextBox { body } => (body, false),
            // A picture is not a text body and never was: what the model can
            // have changed is the bitmap itself, its alt text, the size it is
            // drawn at and its crop, each written only where it differs from
            // what the part already says.
            ShapeKind::Picture(image) => {
                if let Err(reason) = rewrite_picture(
                    element,
                    source,
                    image,
                    outside.package,
                    outside.rels,
                    outside.media,
                    &mut edits,
                ) {
                    kept.push(reason);
                }
                continue;
            }
            ShapeKind::Table(table) => {
                if let Err(reason) =
                    rewrite_table(element, table, lang.as_deref(), outside.rels, &mut edits)
                {
                    kept.push(reason);
                }
                continue;
            }
            // A preserved object goes back **as the bytes that came out**:
            // SmartArt, OLE, a group, a custom geometry, an animation the IR
            // never modelled. The splice already leaves the element alone, so
            // the work here is to say so honestly and to put the fragment back
            // when the model carries one that differs from the package's.
            ShapeKind::Raw(RawShape { raw, .. }) => {
                let Some(id) = raw else {
                    // No payload at this fidelity level: the stub is all the
                    // model has, and the object standing in the package is the
                    // original XML at its original z-order. Nothing is lost.
                    continue;
                };
                match reinject(
                    std::iter::once(*element),
                    source,
                    id.as_str(),
                    outside,
                    &mut edits,
                ) {
                    Reinjected::Standing => {}
                    Reinjected::Dropped(format) => {
                        dropped.push((id.as_str().to_string(), format));
                    }
                    Reinjected::Kept(reason) => kept.push(reason),
                }
                continue;
            }
            other => {
                kept.push(format!("its {}", shape_noun(other)));
                continue;
            }
        };
        // A `p:sp` the reader stubbed — a custom geometry, a group — reaches
        // here as a raw shape and is filtered out above; anything else that is
        // not a `p:sp` never carried a modelled text body either.
        if element.name != "sp" {
            kept.push(format!("its `{}`", qualified(element)));
            continue;
        }
        let Some(tx_body) = element.child("txBody") else {
            kept.push("a text shape with no text body".into());
            continue;
        };
        match write_text::write_body(body, bulleted, lang.as_deref(), outside.rels) {
            Ok(paragraphs) => {
                let range = paragraph_range(tx_body);
                if paragraphs.is_empty() && !source[range.clone()].trim().is_empty() {
                    // `CT_TextBody` needs a paragraph; emptying the box means
                    // writing the empty one rather than none at all.
                    edits.push((range, "<a:p/>".to_string()));
                } else {
                    edits.push((range, paragraphs));
                }
            }
            Err(reason) => kept.push(format!("{reason} in `{}`", shape_name(shape))),
        }
    }

    Ok(Rewritten {
        xml: splice(source, edits),
        kept,
        dropped,
    })
}

/// What became of one raw fragment on the way out.
enum Reinjected {
    /// The original markup is in the written part — either because the
    /// preserved package already holds it byte for byte, or because the
    /// fragment the model carried was spliced back over it.
    Standing,
    /// The fragment is in a dialect this package cannot hold, so it was not
    /// emitted: `Warning::RawBlockDropped`, never nonsense in the file.
    Dropped(String),
    /// The fragment could not be placed, and the package's own bytes stand.
    Kept(String),
}

/// Puts one raw fragment back where it came from.
///
/// The preserved package is the first answer: a fragment nobody edited is
/// already in the part, at its original z-order, byte for byte — so the
/// comparison is with the source, and an identical fragment produces no edit
/// and no warning. What this adds is the case the skeleton cannot cover: a
/// payload that differs from the package's bytes, which is written back over
/// the element it belongs to.
///
/// `candidates` are the elements the fragment could be: one shape for a shape
/// fragment, the children of `p:sld` for a slide-level one, where the match is
/// by element name because the id is not in the file.
fn reinject<'a>(
    candidates: impl Iterator<Item = &'a Element>,
    source: &str,
    id: &str,
    outside: &Outside<'_, '_>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Reinjected {
    let Some(fragment) = outside.fragments.get(id) else {
        // The stub points at a payload this document does not carry — a deck
        // read back from DocMark, whose sidecar the deck parser does not load.
        // The object is in the package all the same, which is what the caller
        // needs; inventing markup for it would be worse than saying nothing.
        return Reinjected::Standing;
    };
    if fragment.format != "ooxml" {
        return Reinjected::Dropped(fragment.format.clone());
    }
    if fragment.part != outside.part {
        // A chart's fragment is the chart part, not this slide's markup, and
        // that part is outside `rebuilt_parts`: it is copied, not rewritten.
        return Reinjected::Kept(format!(
            "the preserved markup `{id}`, which belongs to `{}`",
            fragment.part
        ));
    }
    // A fragment whose own root element cannot be read matches **nothing**:
    // falling back to the first candidate would put a slide-level payload over
    // `p:spTree`, which is a corrupt slide rather than a degraded one.
    let Some(name) = fragment_name(&fragment.content) else {
        return Reinjected::Kept(format!(
            "the preserved markup `{id}`, which does not open with an element this writer can name"
        ));
    };
    for element in candidates {
        if name != qualified(element) {
            continue;
        }
        if element.raw(source) == fragment.content {
            return Reinjected::Standing;
        }
        edits.push((element.span.clone(), fragment.content.clone()));
        return Reinjected::Standing;
    }
    Reinjected::Kept(format!(
        "the preserved markup `{id}`, which no element of the part answers to"
    ))
}

/// The qualified name of a fragment's own root element, read off its first tag.
///
/// The fragment is a slice of a part, so its first `<` opens the element it
/// preserves; `None` for markup this cannot read rather than a guess that would
/// match the wrong element.
fn fragment_name(content: &str) -> Option<&str> {
    let rest = content.strip_prefix('<')?;
    let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
    let name = &rest[..end];
    (!name.is_empty() && !name.starts_with('?') && !name.starts_with('!')).then_some(name)
}

/// The editing language the slide's runs already state, if they agree.
///
/// `a:rPr@lang` is spell-check metadata with no IR field, and it is on every
/// run PowerPoint writes. Reading it off the slide and putting it back is what
/// keeps a rewritten body the same file rather than the same text: the
/// alternative is dropping an attribute from every run in the deck.
fn editing_language(tree: &Element) -> Option<String> {
    fn walk(element: &Element, out: &mut Vec<String>) {
        if element.name == "rPr" || element.name == "endParaRPr" {
            if let Some(lang) = element.attr("lang") {
                out.push(lang.to_string());
            }
        }
        for child in element.children() {
            walk(child, out);
        }
    }
    let mut found = Vec::new();
    walk(tree, &mut found);
    let first = found.first()?.clone();
    // Disagreement is a slide with runs in two languages, and picking one would
    // retag the other. Better to write none than to write the wrong one.
    found.iter().all(|lang| *lang == first).then_some(first)
}

/// The byte range of a text body's `a:p` children.
///
/// Empty and pointing just after `a:lstStyle` when there are none, so an
/// insertion lands where a paragraph belongs rather than before the properties
/// that have to precede it.
fn paragraph_range(tx_body: &Element) -> std::ops::Range<usize> {
    let paragraphs: Vec<&Element> = tx_body.children_named("p").collect();
    match (paragraphs.first(), paragraphs.last()) {
        (Some(first), Some(last)) => first.span.start..last.span.end,
        _ => {
            let after = tx_body
                .children()
                .map(|child| child.span.end)
                .max()
                .unwrap_or(tx_body.span.end);
            after..after
        }
    }
}

/// Applies non-overlapping edits to the source, left to right.
fn splice(source: &str, mut edits: Vec<(std::ops::Range<usize>, String)>) -> String {
    edits.sort_by_key(|(range, _)| range.start);
    let mut out = String::with_capacity(source.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        if range.start < cursor {
            continue;
        }
        out.push_str(&source[cursor..range.start]);
        out.push_str(&replacement);
        cursor = range.end;
    }
    out.push_str(&source[cursor..]);
    out
}

fn qualified(element: &Element) -> String {
    if element.prefix.is_empty() {
        element.name.clone()
    } else {
        format!("{}:{}", element.prefix, element.name)
    }
}

fn shape_name(shape: &docsai_model::presentation::Shape) -> String {
    shape
        .name
        .clone()
        .unwrap_or_else(|| format!("shape {}", shape.z_index))
}

/// What a shape is, for a warning that has to name it.
fn shape_noun(kind: &ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Picture(_) => "picture",
        ShapeKind::Table(_) => "table",
        ShapeKind::Chart(_) => "chart",
        ShapeKind::Raw(_) => "preserved object",
        ShapeKind::Group(_) => "group",
        _ => "shape this writer does not rebuild",
    }
}

/// Rewrites what a picture states about itself, attribute by attribute.
///
/// Nothing here is written unless it differs from what the part already says,
/// which is what keeps an unedited deck byte for byte: a picture nobody touched
/// produces no edit at all, and a picture whose alt text was retyped produces
/// exactly one.
///
/// The bitmap is compared by **bytes**, not by name: the asset store keys on
/// the content hash, so an image that came out of this very package is the same
/// file and keeps the relationship it already had. A different one becomes a
/// new `ppt/media/` part, a new relationship, and — if the deck had never
/// carried that extension — a new content type.
fn rewrite_picture(
    pic: &Element,
    source: &str,
    image: &ImageRef,
    package: &Package,
    rels: &mut PartRels,
    media: &mut Media<'_>,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    if pic.name != "pic" {
        return Err(format!(
            "its picture, which the part states as `{}`",
            qualified(pic)
        ));
    }
    let Some(blip) = pic.path(&["blipFill", "blip"]) else {
        return Err("its picture, which has no `a:blip` to point an image at".into());
    };

    // A linked picture states `r:link` and holds no bytes in the package; the
    // model keeps the URL and this writer has nothing to re-inject.
    if image.external_src.is_none() {
        let embed = blip
            .attrs
            .iter()
            .find(|(name, _)| local_of(name) == "embed")
            .map(|(name, value)| (name.clone(), value.clone()));
        let current = embed
            .as_ref()
            .and_then(|(_, id)| rels.target_of(id))
            .and_then(|target| package.part(&target).map(<[u8]>::to_vec));
        let assets = media.assets;
        let Some(wanted) = assets.get(&image.asset) else {
            return Err(format!(
                "its picture, whose bytes the asset store does not hold under `{}`",
                image.asset.as_str()
            ));
        };
        if current.as_deref() != Some(wanted) {
            let part = media.part_for(image)?;
            let id = rels.relate(IMAGE_REL, &part, false);
            match &embed {
                Some((name, value)) if *value != id => {
                    set_attr(source, blip, name, Some(&id), edits);
                }
                Some(_) => {}
                None => set_attr(source, blip, "r:embed", Some(&id), edits),
            }
        }
    }

    if let Some(nv) = pic.path(&["nvPicPr", "cNvPr"]) {
        if nv.attr("descr").unwrap_or_default() != image.alt {
            let alt = image.alt.as_str();
            set_attr(source, nv, "descr", (!alt.is_empty()).then_some(alt), edits);
        }
        // `name` is required on `CT_NonVisualDrawingProps`, so a model that
        // does not carry one leaves the part's alone rather than emptying it.
        if let Some(name) = &image.name {
            if nv.attr("name") != Some(name.as_str()) {
                set_attr(source, nv, "name", Some(name), edits);
            }
        }
    }

    // The crop is the bitmap's own, `a:srcRect` beside the blip; the reader's
    // own function decides what the part currently says, so the comparison is
    // made in the units the model uses rather than in the ones the file does.
    let src_rect = pic.path(&["blipFill", "srcRect"]);
    let mut probe = ImageGeometry::inline(Size::new(Length::from_emu(0), Length::from_emu(0)));
    crate::drawingml::read_crop(src_rect, &mut probe);
    if probe.crop != image.geometry.crop {
        match (src_rect, &image.geometry.crop) {
            (Some(rect), Some(crop)) => edits.push((rect.span.clone(), src_rect_xml(crop))),
            (Some(rect), None) => edits.push((rect.span.clone(), String::new())),
            (None, Some(crop)) => {
                edits.push((blip.span.end..blip.span.end, src_rect_xml(crop)));
            }
            (None, None) => {}
        }
    }

    // Last, because it is the one that can end the function: a picture with no
    // `a:ext` inherits its size, and writing one would place a picture the deck
    // had left to its layout.
    let size = image.geometry.display_size;
    match pic.path(&["spPr", "xfrm", "ext"]) {
        Some(ext) => {
            for (name, emu) in [("cx", size.width.emu()), ("cy", size.height.emu())] {
                if ext.attr_i64(name) != Some(emu) {
                    set_attr(source, ext, name, Some(&emu.to_string()), edits);
                }
            }
        }
        None if size.width.emu() != 0 || size.height.emu() != 0 => {
            return Err("its picture's size, which the part does not state at all".into());
        }
        None => {}
    }
    Ok(())
}

/// `a:srcRect` in the thousandths of a percent the format counts in.
fn src_rect_xml(crop: &docsai_model::image::CropRect) -> String {
    let mut out = String::from("<a:srcRect");
    for (name, value) in [
        ("l", crop.left),
        ("t", crop.top),
        ("r", crop.right),
        ("b", crop.bottom),
    ] {
        let units = (value * 1_000.0).round() as i64;
        if units != 0 {
            out.push_str(&format!(" {name}=\"{units}\""));
        }
    }
    out.push_str("/>");
    out
}

/// Rewrites the text of a table's cells, and nothing else about the table.
///
/// The same rule as a shape, one container further in: the grid, the merges,
/// the row heights, the cell properties and the table style are the bytes that
/// were read, and what comes from the model is the `a:p` sequence of each
/// cell's body. A grid the model changed is a table that has to be regenerated
/// rather than spliced, and regenerating an `a:tbl` from the IR belongs with
/// the deck that has no skeleton to preserve.
fn rewrite_table(
    frame: &Element,
    table: &Table,
    lang: Option<&str>,
    rels: &mut PartRels,
    edits: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    let Some(tbl) = frame.path(&["graphic", "graphicData", "tbl"]) else {
        return Err("its table, which the part does not state as an `a:tbl`".into());
    };
    let rows: Vec<&Element> = tbl.children_named("tr").collect();
    if rows.len() != table.rows.len() {
        return Err(format!(
            "its table, whose grid the model moved from {} row(s) to {} — an `a:tbl` \
             written from the model rather than spliced is plan v2 Phase 15-G",
            rows.len(),
            table.rows.len()
        ));
    }

    for (tr, row) in rows.iter().zip(&table.rows) {
        // A cell swallowed horizontally is not in the IR grid at all: a
        // `gridSpan` of 2 already occupies both columns, so the reader dropped
        // it and the pairing has to drop it here too.
        let cells: Vec<&Element> = tr
            .children_named("tc")
            .filter(|tc| !is_true(tc.attr("hMerge")))
            .collect();
        if cells.len() != row.cells.len() {
            return Err(format!(
                "its table, whose row the model moved from {} cell(s) to {} — an \
                 `a:tbl` written from the model rather than spliced is plan v2 \
                 Phase 15-G",
                cells.len(),
                row.cells.len()
            ));
        }
        for (tc, cell) in cells.iter().zip(&row.cells) {
            // A cell covered by the one above holds no content of its own, and
            // the reader read none: writing the empty body it has in the model
            // would empty the `a:txBody` PowerPoint keeps there.
            if cell.covered {
                continue;
            }
            let Some(body) = tc.child("txBody") else {
                continue;
            };
            let paragraphs = write_text::write_body(&cell.blocks, false, lang, rels)
                .map_err(|reason| format!("its table, which holds {reason}"))?;
            let range = paragraph_range(body);
            if paragraphs.is_empty() {
                edits.push((range, "<a:p/>".to_string()));
            } else {
                edits.push((range, paragraphs));
            }
        }
    }

    let grid = tbl
        .child("tblGrid")
        .map(|grid| grid.children_named("gridCol").count())
        .unwrap_or(0);
    let style = tbl
        .path(&["tblPr", "tableStyleId"])
        .map(|id| id.text().trim().to_string());
    if grid != table.col_widths.len()
        || style.as_deref() != table.style.as_ref().map(|id| id.as_str())
    {
        return Err("its table's column grid and style reference".into());
    }
    Ok(())
}

/// An OOXML boolean attribute, where absent means false.
fn is_true(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("on"))
}

/// The relationships of one part: what it already has, and what an edit added.
///
/// The reuse comes first and matters most. A picture that did not change, a
/// link that still points where it pointed, a slide reference that was already
/// there — all resolve to the id the deck was written with, so the `_rels` part
/// is not touched at all and the slide keeps the ids it was read with. Only a
/// genuinely new target mints an id, and even then the part is **spliced**: the
/// relationships the deck had keep their bytes, their order and their ids.
struct PartRels {
    /// The `_rels` part's own name.
    part: String,
    /// The directory a relative target is written against.
    base: String,
    existing: Relationships,
    /// The part as it stands, when the package has one.
    source: Option<String>,
    /// Id, type, package-absolute target, external.
    added: Vec<(String, &'static str, String, bool)>,
    used: BTreeSet<String>,
}

impl PartRels {
    fn new(package: &Package, owner: &str) -> Self {
        let existing = package.relationships(owner);
        let part = rels_part_name(owner).unwrap_or_default();
        let source = package.text(&part).ok().map(str::to_string);
        let used = existing.iter().map(|(id, _)| id.to_string()).collect();
        PartRels {
            part,
            base: parent_dir(owner),
            existing,
            source,
            added: Vec::new(),
            used,
        }
    }

    /// The part a relationship id points at, when it points inside the package.
    fn target_of(&self, id: &str) -> Option<String> {
        self.existing
            .get(id)
            .filter(|rel| !rel.external)
            .map(|rel| rel.target.clone())
    }

    /// The id already pointing at a target, whatever kind of relationship it is.
    fn id_of_target(&self, target: &str) -> Option<String> {
        self.existing
            .iter()
            .find(|(_, rel)| rel.target == target)
            .map(|(id, _)| id.to_string())
            .or_else(|| {
                self.added
                    .iter()
                    .find(|(_, _, added, _)| added == target)
                    .map(|(id, _, _, _)| id.clone())
            })
    }

    /// The id for a target, reusing the relationship the part already has.
    fn relate(&mut self, kind: &'static str, target: &str, external: bool) -> String {
        if let Some(id) = self.id_of_target(target) {
            return id;
        }
        let id = self.fresh_id();
        self.added
            .push((id.clone(), kind, target.to_string(), external));
        id
    }

    fn fresh_id(&mut self) -> String {
        let mut n = self.used.len() + 1;
        loop {
            let id = format!("rId{n}");
            if self.used.insert(id.clone()) {
                return id;
            }
            n += 1;
        }
    }

    /// The relationship part with the new relationships in it, or `None` when
    /// nothing was added — an unedited slide must not move its `_rels` at all.
    fn rewritten(&self) -> Option<(String, String)> {
        if self.added.is_empty() || self.part.is_empty() {
            return None;
        }
        let mut entries = String::new();
        for (id, kind, target, external) in &self.added {
            let (target, mode) = if *external {
                (target.clone(), " TargetMode=\"External\"")
            } else {
                (relative_target(&self.base, target), "")
            };
            entries.push_str(&format!(
                "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{mode}/>",
                esc_attr(id),
                esc_attr(kind),
                esc_attr(&target)
            ));
        }
        match &self.source {
            Some(source) => {
                let at = source.rfind("</Relationships>")?;
                Some((
                    self.part.clone(),
                    format!("{}{entries}{}", &source[..at], &source[at..]),
                ))
            }
            None => Some((
                self.part.clone(),
                format!(
                    "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
                     <Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                     {entries}</Relationships>"
                ),
            )),
        }
    }
}

impl Links for PartRels {
    fn hyperlink(&mut self, target: &str) -> Option<String> {
        if let Some(id) = self.id_of_target(target) {
            return Some(id);
        }
        // An internal target — another slide of this deck — is a relationship
        // whose target is a part name, and minting one means knowing which part
        // the model's target refers to. Until 15-E settles that, a link the
        // deck did not already have is refused rather than pointed at nothing.
        is_external(target).then(|| self.relate(HYPERLINK_REL, target, true))
    }
}

/// A target a hyperlink relationship can carry as it stands.
fn is_external(target: &str) -> bool {
    target.contains("://") || target.starts_with("mailto:") || target.starts_with("tel:")
}

/// Media parts the written package needs and the original did not have.
struct Media<'a> {
    assets: &'a dyn AssetStore,
    /// Asset id to the part it was given, so one bitmap on four slides is one
    /// part — the store already deduplicated it, and so does this.
    parts: BTreeMap<String, String>,
    added: BTreeMap<String, Vec<u8>>,
    /// Every part name the package uses, without its extension, so a new one
    /// collides with nothing — not even with the `image1.png` a new
    /// `image1.gif` would sit beside and be mistaken for.
    taken: BTreeSet<String>,
    /// Extensions the new parts need, with the content type each declares.
    extensions: BTreeMap<String, String>,
}

impl<'a> Media<'a> {
    fn new(package: &Package, assets: &'a dyn AssetStore) -> Self {
        Media {
            assets,
            parts: BTreeMap::new(),
            added: BTreeMap::new(),
            taken: package.part_names().map(stem).collect(),
            extensions: BTreeMap::new(),
        }
    }

    /// The package part holding this image's bytes, minting one if needed.
    ///
    /// The bytes go in as the store holds them. They are the bytes that came
    /// out of the source document — the store keys on their hash — so nothing
    /// here decodes, scales or re-encodes a bitmap; the ZIP deflates the member
    /// like any other, which is lossless and reversible.
    fn part_for(&mut self, image: &ImageRef) -> Result<String, String> {
        let key = image.asset.as_str().to_string();
        if let Some(part) = self.parts.get(&key) {
            return Ok(part.clone());
        }
        let Some(bytes) = self.assets.get(&image.asset) else {
            return Err(format!(
                "its picture, whose bytes the asset store does not hold under `{key}`"
            ));
        };
        let info = self.assets.info(&image.asset);
        // The store sniffs the extension from the content rather than trusting
        // the source document's naming, which is exactly what a content type
        // has to agree with.
        let extension = info
            .and_then(|info| info.file_name.rsplit('.').next())
            .unwrap_or("bin")
            .to_ascii_lowercase();
        let content_type = info
            .map(|info| info.content_type.clone())
            .unwrap_or_else(|| "application/octet-stream".into());

        let mut n = 1;
        let part = loop {
            let candidate = format!("ppt/media/image{n}.{extension}");
            if !self.taken.contains(&stem(&candidate)) {
                break candidate;
            }
            n += 1;
        };
        self.taken.insert(stem(&part));
        self.parts.insert(key, part.clone());
        self.added.insert(part.clone(), bytes.to_vec());
        self.extensions.insert(extension, content_type);
        Ok(part)
    }
}

/// `[Content_Types].xml` with a `Default` for every extension the new media
/// needs and the package does not declare, or `None` when it declares them all.
///
/// A part with no content type is the one thing in an OPC package PowerPoint
/// refuses outright, and it is spliced rather than rebuilt for the same reason
/// everything else here is: the overrides a deck carries are its own.
fn content_types(package: &Package, media: &Media<'_>) -> Option<(String, String)> {
    const PART: &str = "[Content_Types].xml";
    if media.extensions.is_empty() {
        return None;
    }
    let source = package.text(PART).ok()?;
    let root = Element::parse(PART, source.as_bytes()).ok()?;
    let declared: BTreeSet<String> = root
        .children_named("Default")
        .filter_map(|default| default.attr("Extension"))
        .map(str::to_ascii_lowercase)
        .collect();

    let mut entries = String::new();
    for (extension, content_type) in &media.extensions {
        if declared.contains(extension) {
            continue;
        }
        entries.push_str(&format!(
            "<Default Extension=\"{}\" ContentType=\"{}\"/>",
            esc_attr(extension),
            esc_attr(content_type)
        ));
    }
    if entries.is_empty() {
        return None;
    }
    let at = source.rfind("</Types>")?;
    Some((
        PART.to_string(),
        format!("{}{entries}{}", &source[..at], &source[at..]),
    ))
}

/// Where an attribute lives inside an element's start tag.
enum Slot {
    /// The whole attribute, and the byte range of its value inside it.
    Value(Range<usize>, Range<usize>),
    /// Where a new attribute goes, just before the tag closes.
    Absent(usize),
}

/// Scans an element's start tag in the source it was parsed from.
///
/// The parser keeps an element's span but not its attributes', and this writer
/// needs the second: changing one attribute has to leave the order, the
/// spacing and the quoting of every other one exactly as it was.
fn attr_slot(source: &str, element: &Element, name: &str) -> Slot {
    let bytes = source.as_bytes();
    let end = element.span.end.min(bytes.len());
    let mut i = element.span.start + 1;
    while i < end && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' && bytes[i] != b'/' {
        i += 1;
    }
    loop {
        let gap = i;
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end || bytes[i] == b'>' || bytes[i] == b'/' {
            return Slot::Absent(i.min(end));
        }
        let name_start = i;
        while i < end && bytes[i] != b'=' && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
            i += 1;
        }
        let found = &source[name_start..i];
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end || bytes[i] != b'=' {
            return Slot::Absent(i.min(end));
        }
        i += 1;
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end || (bytes[i] != b'"' && bytes[i] != b'\'') {
            return Slot::Absent(i.min(end));
        }
        let quote = bytes[i];
        i += 1;
        let value_start = i;
        while i < end && bytes[i] != quote {
            i += 1;
        }
        let value = value_start..i;
        i = (i + 1).min(end);
        let hit = if name.contains(':') {
            found == name
        } else {
            local_of(found) == name
        };
        if hit {
            return Slot::Value(gap..i, value);
        }
    }
}

/// Sets, replaces or removes one attribute of a start tag, as an edit.
fn set_attr(
    source: &str,
    element: &Element,
    name: &str,
    value: Option<&str>,
    edits: &mut Vec<(Range<usize>, String)>,
) {
    match (attr_slot(source, element, name), value) {
        (Slot::Value(_, range), Some(value)) => edits.push((range, esc_attr(value))),
        // Removing it takes its name, its quotes and the space before it.
        (Slot::Value(whole, _), None) => edits.push((whole, String::new())),
        (Slot::Absent(at), Some(value)) => {
            edits.push((at..at, format!(" {name}=\"{}\"", esc_attr(value))))
        }
        (Slot::Absent(_), None) => {}
    }
}

/// A package-absolute part name as a relationship writes it: relative to the
/// directory of the part that owns the relationship.
fn relative_target(base: &str, target: &str) -> String {
    let base: Vec<&str> = base.split('/').filter(|s| !s.is_empty()).collect();
    let target: Vec<&str> = target.split('/').filter(|s| !s.is_empty()).collect();
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    let mut out = String::new();
    for _ in common..base.len() {
        out.push_str("../");
    }
    out.push_str(&target[common..].join("/"));
    out
}

/// A part name without its extension.
fn stem(part: &str) -> String {
    match part.rsplit_once('.') {
        Some((stem, extension)) if !extension.contains('/') => stem.to_string(),
        _ => part.to_string(),
    }
}

fn parent_dir(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => part[..i].to_string(),
        None => String::new(),
    }
}

fn local_of(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn esc_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Copies an OPC package entry by entry, preserving what a rebuilt one loses.
///
/// Member order, compression method, CRC and timestamp all come from the source
/// entry, and the compressed bytes are copied as they stand: no entry is
/// inflated on the way through, so no entry can come out re-deflated by a
/// different encoder into different bytes.
///
/// One field does change, and it is worth naming so nobody spends an afternoon
/// on it: the **external file attributes** of each central-directory entry.
/// `zip` builds them from the source entry's Unix mode but ORs `S_IFREG` in
/// (`ZipFile::options`), so a corpus deck written by Python's `zipfile` with
/// mode `0o600` comes back as `0o100600`. The archive is therefore the same
/// length and the same content with a different permission bit — nothing an
/// OPC consumer reads, and the reason the test for this asserts equality
/// member by member rather than over the whole file.
fn reemit<W: Write + Seek>(package: &[u8], rebuild: &Rebuild, writer: W) -> Result<(), WriteError> {
    let mut source = zip::ZipArchive::new(Cursor::new(package))?;
    let mut out = zip::ZipWriter::new(writer);
    for index in 0..source.len() {
        let entry = source.by_index_raw(index)?;
        if entry.is_dir() {
            // A directory member carries no content and no relationship; the
            // reader dropped it too.
            continue;
        }
        match rebuild.replacements.get(entry.name()) {
            // A rewritten part cannot be copied raw — its bytes are new — but
            // it keeps its neighbours' treatment: same position in the archive,
            // same compression method, same timestamp.
            Some(bytes) => {
                let options = entry.options();
                let name = entry.name().to_string();
                drop(entry);
                out.start_file(name, options)?;
                out.write_all(bytes)?;
            }
            None => out.raw_copy_file(entry)?,
        }
    }
    // A part the package never had — a picture swapped for one the deck did not
    // carry — goes on the end. OPC names its parts in `[Content_Types].xml` and
    // in the relationship graph, never by position, and a fixed timestamp keeps
    // two runs of the writer producing the same file.
    for (name, bytes) in &rebuild.additions {
        out.start_file(name.as_str(), added_options())?;
        out.write_all(bytes)?;
    }
    out.finish()?;
    Ok(())
}

/// How a member the original package did not have is stored.
fn added_options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(
            zip::DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0)
                .unwrap_or_else(|_| zip::DateTime::default_for_write()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    /// Every deck of the corpus, so a trait nobody thought about — a macro
    /// part, an embedded workbook, a diagram's four parts — is in the sample.
    fn corpus() -> Vec<std::path::PathBuf> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx");
        let mut decks: Vec<_> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|p| {
                matches!(
                    p.extension().and_then(|e| e.to_str()),
                    Some("pptx") | Some("pptm")
                )
            })
            .collect();
        decks.sort();
        assert!(decks.len() >= 17, "the pptx corpus lost fixtures");
        decks
    }

    fn deck(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/pptx")
            .join(name)
    }

    fn presentation(document: &Document) -> &Presentation {
        match document {
            Document::Presentation(deck) => deck,
            _ => panic!("not a deck"),
        }
    }

    fn presentation_mut(document: &mut Document) -> &mut Presentation {
        match document {
            Document::Presentation(deck) => deck,
            _ => panic!("not a deck"),
        }
    }

    /// The written bytes read back the way a consumer would.
    fn read_bytes(package: &[u8]) -> (Presentation, ConversionReport) {
        let mut assets = MemoryAssetStore::new();
        let (document, report) =
            super::super::read(Cursor::new(package.to_vec()), &mut assets).expect("reads it back");
        match document {
            Document::Presentation(deck) => (deck, report),
            _ => panic!("it came back as something that is not a deck"),
        }
    }

    /// The first paragraph of the first shape that has one.
    fn first_paragraph(slide: &mut Slide) -> Option<&mut docsai_model::text::Paragraph> {
        slide.shapes.iter_mut().find_map(|shape| {
            let body = match &mut shape.kind {
                ShapeKind::Placeholder(ph) => &mut ph.body,
                ShapeKind::TextBox { body } => body,
                _ => return None,
            };
            body.iter_mut().find_map(|block| match block {
                docsai_model::text::Block::Paragraph(paragraph) => Some(paragraph),
                _ => None,
            })
        })
    }

    fn set_first_text(document: &mut Document, text: &str) {
        let deck = presentation_mut(document);
        let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph to retype");
        paragraph.content = vec![docsai_model::text::Inline::Text(text.into())];
    }

    /// The first picture of the first slide.
    fn picture_mut(document: &mut Document) -> &mut docsai_model::image::ImageRef {
        presentation_mut(document).slides[0]
            .shapes
            .iter_mut()
            .find_map(|shape| match &mut shape.kind {
                ShapeKind::Picture(image) => Some(image),
                _ => None,
            })
            .expect("the fixture has a picture")
    }

    /// The first table of the first slide.
    fn table_mut(document: &mut Document) -> &mut Table {
        presentation_mut(document).slides[0]
            .shapes
            .iter_mut()
            .find_map(|shape| match &mut shape.kind {
                ShapeKind::Table(table) => Some(table),
                _ => None,
            })
            .expect("the fixture has a table")
    }

    fn count_items(slide: &Slide) -> usize {
        fn count(blocks: &[docsai_model::text::Block]) -> usize {
            blocks
                .iter()
                .map(|block| match block {
                    docsai_model::text::Block::List(list) => {
                        list.items.len()
                            + list
                                .items
                                .iter()
                                .map(|item| count(&item.blocks))
                                .sum::<usize>()
                    }
                    _ => 0,
                })
                .sum()
        }
        slide
            .shapes
            .iter()
            .map(|shape| match &shape.kind {
                ShapeKind::Placeholder(ph) => count(&ph.body),
                ShapeKind::TextBox { body } => count(body),
                _ => 0,
            })
            .sum()
    }

    /// One part of a package, decompressed.
    fn part_of(package: &[u8], part: &str) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).ok()?;
        let mut bytes = Vec::new();
        zip.by_name(part).ok()?.read_to_end(&mut bytes).ok()?;
        Some(bytes)
    }

    /// Every `ppt/slides/*.xml` of a package, with its bytes.
    fn slide_parts_of(package: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).expect("a readable package");
        let names: Vec<String> = (0..zip.len())
            .filter_map(|i| {
                let entry = zip.by_index_raw(i).ok()?;
                let name = entry.name().to_string();
                (name.starts_with("ppt/slides/") && name.ends_with(".xml")).then_some(name)
            })
            .collect();
        names
            .into_iter()
            .map(|name| {
                let bytes = part_of(package, &name).expect("the part is readable");
                (name, bytes)
            })
            .collect()
    }

    fn read_deck(path: &std::path::Path) -> (Document, MemoryAssetStore) {
        let mut assets = MemoryAssetStore::new();
        let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let (document, _) = super::super::read(file, &mut assets)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        (document, assets)
    }

    fn write_deck(document: &Document, assets: &dyn AssetStore) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        write_pptx(document, assets, &mut out).expect("writes the deck");
        out.into_inner()
    }

    /// Every member, in order, with what the archive says about it.
    fn members(package: &[u8]) -> Vec<(String, zip::CompressionMethod, u32, u64)> {
        let mut zip = zip::ZipArchive::new(Cursor::new(package)).expect("a readable package");
        (0..zip.len())
            .filter_map(|i| {
                let entry = zip.by_index_raw(i).expect("a readable member");
                (!entry.is_dir()).then(|| {
                    (
                        entry.name().to_string(),
                        entry.compression(),
                        entry.crc32(),
                        entry.size(),
                    )
                })
            })
            .collect()
    }

    /// The claim of the increment, over the whole corpus: what goes out is what
    /// came in — not «the same parts», the same archive.
    #[test]
    fn a_written_deck_is_the_package_that_was_read() {
        for path in corpus() {
            let original = std::fs::read(&path).expect("reads the deck");
            let (document, assets) = read_deck(&path);
            let written = write_deck(&document, &assets);

            let name = path.file_name().unwrap().to_string_lossy();
            assert_eq!(
                members(&written),
                members(&original),
                "{name}: the archive is not the one that was read"
            );
            // Same length, because the one field that does move — the external
            // attributes of a central-directory entry, see `reemit` — is fixed
            // width. A size that drifts means something was recompressed.
            assert_eq!(
                written.len(),
                original.len(),
                "{name}: the package changed size"
            );
        }
    }

    /// The parts themselves, decompressed, over a deck that carries the traits
    /// a rebuilt package silently loses: a chart's embedded workbook, a
    /// diagram's `dgm:` parts.
    #[test]
    fn every_part_survives_byte_for_byte() {
        for name in ["charts-embedded", "smartart-fallback", "macro-enabled"] {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx");
            let path = if name == "macro-enabled" {
                dir.join(format!("{name}.pptm"))
            } else {
                dir.join(format!("{name}.pptx"))
            };
            let original = std::fs::read(&path).expect("reads the deck");
            let (document, assets) = read_deck(&path);
            let written = write_deck(&document, &assets);

            let mut before = zip::ZipArchive::new(Cursor::new(&original)).expect("original");
            let mut after = zip::ZipArchive::new(Cursor::new(&written)).expect("written");
            assert_eq!(before.len(), after.len(), "{name}: the member count moved");
            for i in 0..before.len() {
                use std::io::Read;
                let mut a = Vec::new();
                let mut b = Vec::new();
                let part = {
                    let mut entry = before.by_index(i).expect("a member");
                    entry.read_to_end(&mut a).expect("reads it");
                    entry.name().to_string()
                };
                after
                    .by_name(&part)
                    .unwrap_or_else(|e| panic!("{name}: `{part}` is not in the written deck: {e}"))
                    .read_to_end(&mut b)
                    .expect("reads it back");
                assert!(a == b, "{name}: `{part}` came out different");
            }
        }
    }

    /// The rewriter is a no-op on a deck nobody edited.
    ///
    /// The strongest thing 15-B can claim, and the one that keeps a conversion
    /// from being a diff: rewriting a text body from the model produces the
    /// bytes that were already in the part, so a slide nobody edited is not
    /// merely equivalent — it is the same XML. The two tests above cover the
    /// rest of the package; this one is about the parts the writer *does* touch.
    #[test]
    fn an_unedited_slide_is_rewritten_into_the_same_bytes() {
        for path in corpus() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let original = std::fs::read(&path).expect("reads the deck");
            let (document, assets) = read_deck(&path);
            let written = write_deck(&document, &assets);
            for (part, bytes) in slide_parts_of(&original) {
                assert!(
                    part_of(&written, &part) == Some(bytes),
                    "{name}: `{part}` came out different from an untouched model"
                );
            }
        }
    }

    /// The claim of 15-B: an edit to the model is in the file.
    #[test]
    fn a_retitled_slide_reaches_the_written_deck() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        let before = presentation(&document).clone();
        set_first_text(&mut document, "Título reescrito");

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);

        assert_eq!(
            after.slides[0].title().as_deref(),
            Some("Título reescrito"),
            "the new title is not in the written deck"
        );
        assert_ne!(before.slides[0].title(), after.slides[0].title());
        // The slide nobody touched is the slide that was read, shape for shape.
        assert_eq!(
            after.slides[1..],
            before.slides[1..],
            "a slide nobody edited changed"
        );
    }

    /// A bullet added to a body placeholder comes back as a bullet, at the
    /// level it was given — the bullet the master supplies is inherited, so
    /// what is written is a paragraph with no bullet element at all.
    #[test]
    fn an_added_bullet_comes_back_as_a_bullet() {
        let path = deck("bullets-levels.pptx");
        let (mut document, assets) = read_deck(&path);
        let before = count_items(&presentation(&document).slides[0]);

        {
            let deck = presentation_mut(&mut document);
            let list = deck.slides[0]
                .shapes
                .iter_mut()
                .find_map(|shape| match &mut shape.kind {
                    ShapeKind::Placeholder(ph) if ph.ph_type.is_body() => {
                        ph.body.iter_mut().find_map(|block| match block {
                            docsai_model::text::Block::List(list) => Some(list),
                            _ => None,
                        })
                    }
                    _ => None,
                })
                .expect("the fixture has a bulleted body");
            list.items.push(docsai_model::text::ListItem {
                blocks: vec![docsai_model::text::Block::Paragraph(
                    docsai_model::text::Paragraph::text("Punto añadido"),
                )],
            });
        }

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);
        assert_eq!(count_items(&after.slides[0]), before + 1);
        let text = format!("{:?}", after.slides[0]);
        assert!(text.contains("Punto añadido"), "the bullet is not there");
    }

    /// A run's formatting is a delta over the cascade on the way in and a
    /// direct property on the way out, and the two have to agree: what is read
    /// back is what was set, not what the layout would have supplied.
    #[test]
    fn run_formatting_survives_the_round_trip() {
        use docsai_model::style::FontProps;
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck = presentation_mut(&mut document);
            let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph");
            paragraph.content = vec![docsai_model::text::Inline::Styled {
                content: vec![docsai_model::text::Inline::Text("Con formato".into())],
                props: docsai_model::text::RunProps::direct(FontProps {
                    bold: Some(true),
                    italic: Some(true),
                    size: Some(docsai_model::units::Length::from_pt(28.0)),
                    color: Some("#c00000".into()),
                    ..FontProps::default()
                }),
            }];
        }

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);
        let paragraph = format!("{:?}", after.slides[0]);
        assert!(paragraph.contains("Con formato"));
        assert!(paragraph.contains("bold: Some(true)"), "{paragraph}");
        assert!(paragraph.contains("italic: Some(true)"), "{paragraph}");
        assert!(paragraph.contains("#c00000"), "{paragraph}");
    }

    /// 15-C: the hyperlink reaches the file, and so does the relationship.
    ///
    /// The link is not an element of its own in DrawingML — it is an `r:id` on
    /// the run — so «the link was written» means two parts agreeing: the slide
    /// names an id, and the slide's `_rels` says what that id points at. The
    /// relationships the deck already had keep their bytes: the part is
    /// spliced, not regenerated.
    #[test]
    fn a_link_reaches_the_written_deck_with_its_relationship() {
        let path = deck("basic-slides.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        {
            let deck = presentation_mut(&mut document);
            let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph");
            paragraph.content = vec![docsai_model::text::Inline::Link {
                target: "https://example.org".into(),
                content: vec![docsai_model::text::Inline::Text("un enlace".into())],
                props: Default::default(),
            }];
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            !report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("example.org"))
            }),
            "the link is reported as lost and it was written: {:?}",
            report.warnings
        );
        let (after, _) = read_bytes(&written);
        let text = format!("{:?}", after.slides[0]);
        assert!(
            text.contains("un enlace") && text.contains("https://example.org"),
            "the link did not come back: {text}"
        );

        let part = "ppt/slides/_rels/slide1.xml.rels";
        let before = String::from_utf8(part_of(&original, part).expect("the rels part")).unwrap();
        let now = String::from_utf8(part_of(&written, part).expect("the rels part")).unwrap();
        assert!(
            now.contains("TargetMode=\"External\"") && now.contains("https://example.org"),
            "the relationship is not in the part: {now}"
        );
        let head = before.trim_end().trim_end_matches("</Relationships>");
        assert!(
            now.starts_with(head),
            "the relationships the deck already had were rewritten: {now}"
        );
    }

    /// A target no relationship can state as it stands is still refused.
    ///
    /// An internal link points at a *part*, and deciding which part a model's
    /// target names is the deck-level bookkeeping of 15-E. The wrong answer is
    /// an `r:id` pointing at nothing, which is precisely the dangling
    /// relationship spike P3 measured LibreOffice accepting in silence.
    #[test]
    fn a_link_with_no_relationship_to_make_is_still_refused() {
        let path = deck("basic-slides.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck = presentation_mut(&mut document);
            let paragraph = first_paragraph(&mut deck.slides[0]).expect("a paragraph");
            paragraph.content = vec![docsai_model::text::Inline::Link {
                target: "otra-presentacion.pptx".into(),
                content: vec![docsai_model::text::Inline::Text("un enlace".into())],
                props: Default::default(),
            }];
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("otra-presentacion"))
            }),
            "the link was dropped without a word: {:?}",
            report.warnings
        );
    }

    /// 15-C, the claim that decides the increment: the bytes that came out of
    /// the store are the bytes that go into the package.
    ///
    /// A new extension brings a new `Default` with it, because a part with no
    /// content type is the one thing an OPC consumer refuses outright — and the
    /// media part is compared byte for byte, so nothing here re-encoded a
    /// bitmap on the way through.
    #[test]
    fn a_swapped_image_goes_in_as_its_own_bytes_and_is_declared() {
        // A 1×1 transparent GIF: a real file, and an extension this deck has
        // never carried.
        const TINY_GIF: &[u8] = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\
                                  !\xf9\x04\x01\x00\x00\x00\x00,\x00\x00\x00\x00\x01\x00\x01\
                                  \x00\x00\x02\x02D\x01\x00;";
        let path = deck("images-anchored.pptx");
        let (mut document, mut assets) = read_deck(&path);
        let swapped = assets.put(TINY_GIF).expect("stores the image");
        picture_mut(&mut document).asset = swapped;

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let part = "ppt/media/image2.gif";
        assert_eq!(
            part_of(&written, part).as_deref(),
            Some(TINY_GIF),
            "the image is not in the package as the store holds it"
        );
        let types = String::from_utf8(part_of(&written, "[Content_Types].xml").expect("types"))
            .expect("utf-8");
        assert!(
            types.contains("Extension=\"gif\"") && types.contains("image/gif"),
            "the new extension is not declared: {types}"
        );
        // The old image is still there: nothing in this writer deletes a part.
        assert!(part_of(&written, "ppt/media/image1.png").is_some());

        let (after, _) = read_bytes(&written);
        let ShapeKind::Picture(image) = &after.slides[0].shapes[1].kind else {
            panic!("the picture did not survive the swap");
        };
        let mut store = MemoryAssetStore::new();
        let expected = store.put(TINY_GIF).expect("stores it");
        assert_eq!(image.asset, expected, "the deck points at the other image");
    }

    /// The alt text is what a reader of the Markdown edits, and it is an
    /// attribute of the picture rather than anything textual.
    #[test]
    fn a_retyped_alt_text_reaches_the_picture() {
        let path = deck("images-anchored.pptx");
        let (mut document, assets) = read_deck(&path);
        picture_mut(&mut document).alt = "Otro texto alternativo".into();

        let written = write_deck(&document, &assets);
        let (after, _) = read_bytes(&written);
        let ShapeKind::Picture(image) = &after.slides[0].shapes[1].kind else {
            panic!("the picture is gone");
        };
        assert_eq!(image.alt, "Otro texto alternativo");
        // One attribute moved, and nothing else about the picture did.
        assert_eq!(
            image.geometry.display_size,
            docsai_model::units::Size::new(Length::from_emu(1_143_000), Length::from_emu(857_250))
        );
    }

    /// A picture nobody edited is not a loss, and must not be reported as one.
    #[test]
    fn a_picture_nobody_edited_is_not_reported() {
        let path = deck("images-anchored.pptx");
        let (mut document, assets) = read_deck(&path);
        set_first_text(&mut document, "Otro título");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.is_empty(),
            "an untouched picture was reported: {:?}",
            report.warnings
        );
        let (after, _) = read_bytes(&written);
        assert_eq!(after.slides[0].title().as_deref(), Some("Otro título"));
        assert!(
            after.slides[0]
                .shapes
                .iter()
                .any(|shape| matches!(shape.kind, ShapeKind::Picture(_))),
            "the picture did not survive the rewrite"
        );
    }

    /// A table's cells are text bodies like any other, and the grid around them
    /// is the bytes that were read.
    #[test]
    fn a_retyped_table_cell_reaches_the_written_deck() {
        let path = deck("tables-simple.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let table = table_mut(&mut document);
            table.rows[1].cells[0].blocks = vec![docsai_model::text::Block::Paragraph(
                docsai_model::text::Paragraph::text("Suroeste"),
            )];
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);

        let (mut after, _) = read_bytes(&written);
        let ShapeKind::Table(table) = &mut after.slides[0].shapes[1].kind else {
            panic!("the table is gone");
        };
        let text = format!("{:?}", table.rows[1].cells[0]);
        assert!(
            text.contains("Suroeste"),
            "the cell was not rewritten: {text}"
        );
        // The row nobody touched came back as it was.
        let other = format!("{:?}", table.rows[1].cells[1]);
        assert!(other.contains("1 200"), "{other}");
    }

    /// A grid the model moved is a table this writer cannot splice, so it keeps
    /// the one that was read and names the increment that writes `a:tbl`.
    #[test]
    fn a_table_whose_grid_moved_is_kept_and_named() {
        let path = deck("tables-simple.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        table_mut(&mut document).rows.pop();

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("15-G"))
            }),
            "the report does not name the increment: {:?}",
            report.warnings
        );
        for (part, bytes) in slide_parts_of(&original) {
            assert_eq!(
                part_of(&written, &part),
                Some(bytes),
                "`{part}` was rewritten around the table anyway"
            );
        }
    }

    /// A slide added or removed moves the deck's own parts, which is 15-E. The
    /// package is copied whole rather than rewritten shape by shape into the
    /// wrong slides.
    #[test]
    fn removing_a_slide_copies_the_package_and_names_the_increment() {
        let path = deck("basic-slides.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        presentation_mut(&mut document).slides.pop();

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        for (part, bytes) in slide_parts_of(&original) {
            assert_eq!(
                part_of(&written, &part),
                Some(bytes),
                "`{part}` was rewritten anyway"
            );
        }
        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("15-E"))
            }),
            "the report does not name the increment: {:?}",
            report.warnings
        );
    }

    /// A deck with no preserved package is a typed refusal, not an empty file.
    #[test]
    fn a_deck_without_a_skeleton_is_refused_by_name() {
        let path = corpus().into_iter().next().expect("a deck");
        let (mut document, assets) = read_deck(&path);
        let Document::Presentation(deck) = &mut document else {
            panic!("the corpus deck is not a deck");
        };
        deck.skeleton = None;

        let mut out = Cursor::new(Vec::new());
        let error = write_pptx(&document, &assets, &mut out).expect_err("refuses");
        assert!(
            error.to_string().contains("15-G"),
            "the refusal does not name the increment that lifts it: {error}"
        );
        assert!(out.into_inner().is_empty(), "a refused write wrote bytes");
    }

    /// The other shapes of a document are not decks.
    #[test]
    fn a_text_document_is_not_written_as_a_deck() {
        let document = Document::Text(docsai_model::TextDocument::default());
        let assets = MemoryAssetStore::new();
        let mut out = Cursor::new(Vec::new());
        let error = write_pptx(&document, &assets, &mut out).expect_err("refuses");
        assert!(error.to_string().contains(".pptx"), "{error}");
    }

    /// 15-D: a preserved object is not a loss, and must not be reported as one.
    ///
    /// The group, the custom geometry, the transition and the timing of
    /// `raw-preserved.pptx` are markup the IR only stubs. The splice never
    /// touched them, so they leave the writer as the bytes that came in, at
    /// the z-order they came in at — which is what 15-D promises, and what a
    /// `Degraded` warning would have been lying about.
    #[test]
    fn a_preserved_object_goes_out_as_the_bytes_that_came_in() {
        let path = deck("raw-preserved.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (document, assets) = read_deck(&path);
        let deck_ir = presentation(&document);
        assert!(
            !deck_ir.raw.is_empty() && !deck_ir.slides[0].raw.is_empty(),
            "the fixture stopped carrying raw fragments"
        );

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            !report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("preserved"))
            }),
            "a preserved object was reported as copied: {:?}",
            report.warnings
        );
        let part = "ppt/slides/slide1.xml";
        assert_eq!(
            part_of(&original, part),
            part_of(&written, part),
            "the slide holding the preserved objects moved"
        );
    }

    /// A payload the model changed is written back over the shape it belongs to.
    ///
    /// This is the case the preserved package cannot cover on its own: the
    /// bytes in the skeleton are the old ones, and the fragment is what the
    /// caller edited. The match is by element name, because the fragment is a
    /// slice of the part and carries no id of its own.
    #[test]
    fn an_edited_fragment_is_written_back_over_its_shape() {
        let path = deck("raw-preserved.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.contains("custGeom"))
                .expect("the custom geometry fragment");
            fragment.content = fragment
                .content
                .replace("Triángulo libre", "Triángulo reescrito");
        }

        let written = write_deck(&document, &assets);
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            slide.contains("Triángulo reescrito") && !slide.contains("Triángulo libre"),
            "the edited fragment is not in the part: {slide}"
        );
        // The neighbouring shapes were spliced around, not regenerated.
        assert!(slide.contains("Caja agrupada 2") && slide.contains("<a:close/>"));
    }

    /// A slide-level fragment goes back at the level it came from.
    ///
    /// `p:transition` and `p:timing` are children of `p:sld`, not shapes on the
    /// tree, and the reader captures them onto `Slide::raw`
    /// ([[43-phase-13-raw]] decision 4). Writing them back at shape level would
    /// put an animation inside `p:spTree`.
    #[test]
    fn an_edited_slide_level_fragment_reaches_the_deck() {
        let path = deck("raw-preserved.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.starts_with("<p:transition"))
                .expect("the transition fragment");
            fragment.content = fragment.content.replace("spd=\"slow\"", "spd=\"fast\"");
        }

        let written = write_deck(&document, &assets);
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            slide.contains("<p:transition spd=\"fast\"") && !slide.contains("spd=\"slow\""),
            "the slide-level fragment did not move: {slide}"
        );
        // The edit landed on `p:transition` and on nothing else: its siblings
        // are the bytes that were read.
        let original = std::fs::read(&path).expect("reads the deck");
        let before =
            String::from_utf8(part_of(&original, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert_eq!(
            slide.replace("spd=\"fast\"", "spd=\"slow\""),
            before,
            "something other than the transition moved"
        );
    }

    /// A fragment this writer cannot name does not land on the first element
    /// that happens to be there.
    ///
    /// The candidates for a slide-level fragment are every child of `p:sld`,
    /// and the first of them is `p:cSld` — the whole shape tree. Matching it
    /// by default would not degrade the slide, it would corrupt it.
    #[test]
    fn a_fragment_that_opens_with_nothing_is_kept_and_named() {
        let path = deck("raw-preserved.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.starts_with("<p:transition"))
                .expect("the transition fragment");
            fragment.content = "<!-- lo que quedó del fragmento -->".into();
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("does not open with an element"))
            }),
            "the fragment that could not be placed is not named: {:?}",
            report.warnings
        );
        assert_eq!(
            part_of(&original, "ppt/slides/slide1.xml"),
            part_of(&written, "ppt/slides/slide1.xml"),
            "an unplaceable fragment moved the slide"
        );
    }

    /// A fragment in another dialect is dropped, never emitted as nonsense.
    ///
    /// `odf` markup inside a `p:spTree` is not a degraded deck, it is a broken
    /// one. The package's own bytes stand and `Warning::RawBlockDropped` names
    /// the fragment and the dialect it was in.
    #[test]
    fn a_fragment_in_another_dialect_is_dropped_and_named() {
        let path = deck("raw-preserved.pptx");
        let original = std::fs::read(&path).expect("reads the deck");
        let (mut document, assets) = read_deck(&path);
        let id = {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.contains("custGeom"))
                .expect("the custom geometry fragment");
            fragment.format = "odf".into();
            fragment.content = "<draw:custom-shape/>".into();
            fragment.id.as_str().to_string()
        };

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::RawBlockDropped { id: reported, format }
                    if *reported == id && format == "odf")
            }),
            "the dropped fragment is not in the report: {:?}",
            report.warnings
        );
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            !slide.contains("draw:custom-shape"),
            "foreign markup reached the package: {slide}"
        );
        assert_eq!(
            part_of(&original, "ppt/slides/slide1.xml"),
            part_of(&written, "ppt/slides/slide1.xml"),
            "the slide moved for a fragment that was not written"
        );
    }

    /// A fragment whose markup belongs to another part is kept and named.
    ///
    /// `rebuilt_parts` is the licence, and a part outside it is copied whatever
    /// the IR seems to know about it — a chart's XML is the case that exists in
    /// the corpus, and it is Phase 16's, not this writer's.
    #[test]
    fn a_fragment_from_another_part_is_kept_and_named() {
        let path = deck("raw-preserved.pptx");
        let (mut document, assets) = read_deck(&path);
        {
            let deck_ir = presentation_mut(&mut document);
            let fragment = deck_ir
                .raw
                .iter_mut()
                .find(|fragment| fragment.content.contains("custGeom"))
                .expect("the custom geometry fragment");
            fragment.part = "ppt/charts/chart1.xml".into();
            fragment.content = fragment.content.replace("Triángulo libre", "Otro sitio");
        }

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("ppt/charts/chart1.xml"))
            }),
            "the part that was not rewritten is not named: {:?}",
            report.warnings
        );
        let slide =
            String::from_utf8(part_of(&written, "ppt/slides/slide1.xml").expect("the slide"))
                .expect("utf-8");
        assert!(
            !slide.contains("Otro sitio"),
            "the fragment was written into the wrong part: {slide}"
        );
    }
}
