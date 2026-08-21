//! The `.pptx` writer (Phase 15, increment 15-A).
//!
//! The reader's counterpart, and the place where spike P3's rule is enforced
//! rather than restated:
//!
//! > **Preserve everything; rebuilding is the exception.**
//!
//! A deck read by [`super::read_pptx`] carries its whole original package in
//! the [`AssetStore`], opaque and byte for byte ([`super::skeleton`]). This
//! writer puts that package back, and — from 15-B on — substitutes only the
//! parts [`SkeletonRef::rebuilt_parts`] licenses it to regenerate. Everything
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
//! What this increment does **not** do is regenerate a slide from the IR. A
//! deck whose text was edited in DocMark and written back through 15-A comes
//! out as it went in, and the writer says so with a warning instead of
//! pretending otherwise: silence there is exactly the loss `AGENTS.md` §7 rule
//! 3 forbids. That is why `SUPPORT` still says `pptx: write no` — the pipeline
//! opens in 15-B, when the slides actually come from the IR.

use std::io::{Cursor, Seek, Write};

use docsai_model::assets::AssetStore;
use docsai_model::presentation::{Presentation, SkeletonRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::Document;

use crate::write_error::WriteError;

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

    reemit(skeleton, writer)?;

    report.warn(Warning::Degraded {
        what: "the slides of the written deck".into(),
        why: format!(
            "the preserved package was re-emitted unchanged: this build does not \
             regenerate a slide from the model yet (plan v2 Phase 15-B), so {} \
             slide(s) come out exactly as they were read, and any edit made to \
             the model since is not in the file",
            deck.slides.len()
        ),
    });
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
fn reemit<W: Write + Seek>(package: &[u8], writer: W) -> Result<(), WriteError> {
    let mut source = zip::ZipArchive::new(Cursor::new(package))?;
    let mut out = zip::ZipWriter::new(writer);
    for index in 0..source.len() {
        let entry = source.by_index_raw(index)?;
        if entry.is_dir() {
            // A directory member carries no content and no relationship; the
            // reader dropped it too.
            continue;
        }
        out.raw_copy_file(entry)?;
    }
    out.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::report::Severity;
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

    /// What 15-A does not do, said out loud. A caller who edited the model and
    /// got the original deck back must be able to find that out from the
    /// report rather than from the file.
    #[test]
    fn the_writer_admits_the_slides_are_not_regenerated() {
        let path = corpus().into_iter().next().expect("a deck");
        let (document, assets) = read_deck(&path);
        let mut out = Cursor::new(Vec::new());
        let report = write_pptx(&document, &assets, &mut out).expect("writes the deck");

        let warning = report
            .warnings
            .iter()
            .find(|w| matches!(w, Warning::Degraded { what, .. } if what.contains("slides")))
            .unwrap_or_else(|| panic!("no warning about the slides: {:?}", report.warnings));
        assert_eq!(warning.severity(), Severity::Minor);
        assert!(
            warning.message().contains("15-B"),
            "the warning does not say where the writer is: {}",
            warning.message()
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
