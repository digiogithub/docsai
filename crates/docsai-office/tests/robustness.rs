//! Phase 1 acceptance criterion: *"Zero panics on synthetic corrupt corpus
//! (truncated ZIP, malformed XML): always `Err`"*.
//!
//! The R1 spike measured 204 panics out of 903 corrupt inputs for `docx-rs`;
//! this test is the standing guarantee that the own parser does better.

use docsai_model::assets::AssetStore;
use docsai_model::MemoryAssetStore;
use std::io::Cursor;

fn corpus(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../corpus/docx/{name}.docx",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Reads a byte slice, reporting whether it panicked.
fn read_catching(bytes: Vec<u8>) -> Result<bool, ()> {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let outcome = std::panic::catch_unwind(move || {
        let mut assets = MemoryAssetStore::new();
        docsai_office::read_docx(Cursor::new(bytes), &mut assets).is_ok()
    });
    std::panic::set_hook(previous);
    outcome.map_err(|_| ())
}

#[test]
fn truncation_never_panics() {
    let base = corpus("images-floating");
    let mut checked = 0;
    for cut in (1..base.len()).step_by(7) {
        let truncated = base[..cut].to_vec();
        assert!(
            read_catching(truncated).is_ok(),
            "panicked on a {cut}-byte prefix"
        );
        checked += 1;
    }
    assert!(checked > 100, "only {checked} prefixes exercised");
}

#[test]
fn byte_corruption_never_panics() {
    let base = corpus("nested-lists");
    for index in (0..base.len()).step_by(11) {
        let mut corrupted = base.clone();
        corrupted[index] ^= 0xFF;
        assert!(
            read_catching(corrupted).is_ok(),
            "panicked with byte {index} flipped"
        );
    }
}

#[test]
fn malformed_xml_inside_a_valid_zip_is_an_error() {
    // A structurally fine package whose main part is broken XML.
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options: zip::write::FileOptions<'_, ()> = Default::default();
        zip.start_file("word/document.xml", options).unwrap();
        std::io::Write::write_all(&mut zip, b"<w:document><w:body><w:p>").unwrap();
        zip.finish().unwrap();
    }
    let mut assets = MemoryAssetStore::new();
    let result = docsai_office::read_docx(Cursor::new(buffer), &mut assets);
    assert!(result.is_err(), "a truncated part must not read as success");
}

#[test]
fn a_package_without_a_document_part_is_an_error() {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options: zip::write::FileOptions<'_, ()> = Default::default();
        zip.start_file("hello.txt", options).unwrap();
        std::io::Write::write_all(&mut zip, b"not a document").unwrap();
        zip.finish().unwrap();
    }
    let mut assets = MemoryAssetStore::new();
    assert!(docsai_office::read_docx(Cursor::new(buffer), &mut assets).is_err());
}

#[test]
fn random_bytes_are_an_error() {
    for seed in 0u8..32 {
        let noise: Vec<u8> = (0..4096u32)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect();
        let mut assets = MemoryAssetStore::new();
        assert!(docsai_office::read_docx(Cursor::new(noise), &mut assets).is_err());
    }
}

#[test]
fn a_media_part_escaping_the_package_is_ignored() {
    // `word/media/../../evil.png` must never be reachable as a part.
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options: zip::write::FileOptions<'_, ()> = Default::default();
        zip.start_file("word/document.xml", options).unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"<w:document xmlns:w="urn:w"><w:body><w:p/></w:body></w:document>"#,
        )
        .unwrap();
        zip.start_file("word/media/../../evil.png", options)
            .unwrap();
        std::io::Write::write_all(&mut zip, b"\x89PNG\r\n\x1a\n").unwrap();
        zip.finish().unwrap();
    }
    let mut assets = MemoryAssetStore::new();
    let (_, report) = docsai_office::read_docx(Cursor::new(buffer), &mut assets)
        .expect("the document itself is still readable");
    assert_eq!(report.stats.images, 0);
    assert!(assets.is_empty(), "the traversing member was not stored");
}
