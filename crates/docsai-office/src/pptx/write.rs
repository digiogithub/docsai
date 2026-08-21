//! The `.pptx` writer (Phase 15, increments 15-A and 15-B).
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
//! What the writer cannot yet put back it **keeps and reports**: a picture, a
//! table, a chart or a preserved object (15-C, 15-D), a notes page (15-E), a
//! hyperlink needing a new relationship, a slide added or removed. Each one is
//! a `Degraded` warning naming what was copied and the increment that lifts it,
//! because a caller who edited the model and got the original back has to be
//! able to learn that from the report rather than from the file (`AGENTS.md` §7
//! rule 3).

use std::collections::BTreeMap;
use std::io::{Cursor, Seek, Write};

use docsai_model::assets::AssetStore;
use docsai_model::presentation::{Placeholder, Presentation, ShapeKind, SkeletonRef, Slide};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::Document;

use crate::package::Package;
use crate::write_error::WriteError;
use crate::xml::Element;

use super::write_text;

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
    let replacements = rebuild_slides(deck, skeleton, &mut report);

    reemit(skeleton, &replacements, writer)?;
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
    report: &mut ConversionReport,
) -> BTreeMap<String, Vec<u8>> {
    let mut replacements = BTreeMap::new();
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
            return replacements;
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
            return replacements;
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
        return replacements;
    }

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
        match rewrite_slide(&root, source, slide) {
            Ok(Rewritten { xml, kept }) => {
                for reason in kept {
                    report.warn(Warning::Degraded {
                        what: format!("slide {number}"),
                        why: format!("{reason}: it was copied from the preserved package, so an edit to it in the model is not in the file"),
                    });
                }
                if xml != source {
                    replacements.insert(part.clone(), xml.into_bytes());
                }
            }
            Err(reason) => report.warn(Warning::Degraded {
                what: format!("slide {number}"),
                why: format!("{reason}, so the whole slide was copied unchanged"),
            }),
        }
    }
    replacements
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
}

/// Rewrites the text bodies of one slide, leaving every other byte alone.
fn rewrite_slide(root: &Element, source: &str, slide: &Slide) -> Result<Rewritten, String> {
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
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
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
        match write_text::write_body(body, bulleted, lang.as_deref()) {
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
    })
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
fn reemit<W: Write + Seek>(
    package: &[u8],
    replacements: &BTreeMap<String, Vec<u8>>,
    writer: W,
) -> Result<(), WriteError> {
    let mut source = zip::ZipArchive::new(Cursor::new(package))?;
    let mut out = zip::ZipWriter::new(writer);
    for index in 0..source.len() {
        let entry = source.by_index_raw(index)?;
        if entry.is_dir() {
            // A directory member carries no content and no relationship; the
            // reader dropped it too.
            continue;
        }
        match replacements.get(entry.name()) {
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
    out.finish()?;
    Ok(())
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

    /// What the writer cannot put back keeps the markup it had, and says so.
    ///
    /// A hyperlink needs a relationship in the slide's own `_rels` part, which
    /// is 15-C. The wrong answer is a run without the link; this is the right
    /// one: the shape stays as it was read and the report names what was left.
    #[test]
    fn a_link_the_writer_cannot_place_keeps_its_shape() {
        let path = deck("basic-slides.pptx");
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
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("example.org"))
            }),
            "the link was dropped without a word: {:?}",
            report.warnings
        );
        let (after, _) = read_bytes(&written);
        let text = format!("{:?}", after.slides[0]);
        assert!(
            !text.contains("un enlace"),
            "the link was written after all; this test is measuring the wrong thing"
        );
    }

    /// A picture, a table, a chart or a preserved object is copied, and the
    /// report says which slide and what.
    #[test]
    fn a_slide_with_a_picture_keeps_it_and_names_it() {
        let path = deck("images-anchored.pptx");
        let (mut document, assets) = read_deck(&path);
        set_first_text(&mut document, "Otro título");

        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");
        let written = out.into_inner();

        assert!(
            report.warnings.iter().any(|warning| {
                matches!(warning, Warning::Degraded { why, .. } if why.contains("picture"))
            }),
            "the copied picture is not in the report: {:?}",
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
}
