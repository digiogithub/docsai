//! Raw-block sidecars (plan v2 Phase 11, increment A).
//!
//! The body keeps the stub, the bytes live in a file, and the pair has to
//! survive a round trip — otherwise the sidecar is a way to lose data quietly,
//! which is the one thing a raw-block exists to prevent.

use std::path::Path;

use docsai_docmark::raw::raw_sidecars;
use docsai_docmark::{parse, parse_with_base, serialize, Fidelity, Options, RawPolicy};
use docsai_model::image::RawId;
use docsai_model::text::{Block, Paragraph, RawFragment, Section, TextDocument};
use docsai_model::{Document, MemoryAssetStore};

/// The blocks of a text document, which is what a raw-block round trip is
/// about; page geometry has its own defaults and its own tests.
fn blocks(doc: &Document) -> Vec<Block> {
    let Document::Text(text) = doc else {
        panic!("a text document")
    };
    text.sections
        .iter()
        .flat_map(|s| s.blocks.clone())
        .collect()
}

const CONTENT: &str = "<m:oMathPara><m:oMath><m:r><m:t>E</m:t></m:r></m:oMath></m:oMathPara>";

fn document() -> Document {
    Document::Text(TextDocument {
        sections: vec![Section {
            blocks: vec![
                Block::Paragraph(Paragraph::text("Antes.")),
                Block::Raw(RawFragment {
                    id: RawId::new("raw-0001"),
                    format: "ooxml".into(),
                    part: "word/document.xml".into(),
                    content: CONTENT.into(),
                }),
            ],
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn options(raw: RawPolicy) -> Options {
    Options {
        fidelity: Fidelity::Full,
        raw,
        ..Options::default()
    }
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-sidecar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("assets/_raw")).expect("creates the sidecar directory");
    dir
}

fn write_sidecars(dir: &Path, doc: &Document, options: &Options) {
    for sidecar in raw_sidecars(doc, options) {
        std::fs::write(dir.join(&sidecar.path), format!("{}\n", sidecar.content))
            .expect("writes the sidecar");
    }
}

#[test]
fn the_body_keeps_the_stub_and_the_bytes_move_out() {
    let doc = document();
    let assets = MemoryAssetStore::new();
    let (markdown, _) = serialize(&doc, &assets, &options(RawPolicy::Sidecar));

    assert!(
        markdown.contains(r#"::: {#raw-0001 .raw format=ooxml part="word/document.xml" src="assets/_raw/raw-0001.xml"}"#),
        "the stub should name its sidecar:\n{markdown}"
    );
    assert!(
        !markdown.contains("oMath"),
        "the payload must not stay in the body:\n{markdown}"
    );

    let sidecars = raw_sidecars(&doc, &options(RawPolicy::Sidecar));
    assert_eq!(sidecars.len(), 1);
    assert_eq!(sidecars[0].path, "assets/_raw/raw-0001.xml");
    assert_eq!(sidecars[0].content, CONTENT);
}

#[test]
fn a_sidecar_round_trip_gives_back_the_same_document() {
    let doc = document();
    let assets = MemoryAssetStore::new();
    let options = options(RawPolicy::Sidecar);
    let (markdown, _) = serialize(&doc, &assets, &options);

    let dir = temp_dir("roundtrip");
    write_sidecars(&dir, &doc, &options);

    let mut back_assets = MemoryAssetStore::new();
    let (parsed, _) =
        parse_with_base(&markdown, Some(&dir), &mut back_assets).expect("parses the stub");
    assert_eq!(blocks(&parsed), blocks(&doc));

    // And the second serialisation is the first one, byte for byte.
    let (again, _) = serialize(&parsed, &back_assets, &options);
    assert_eq!(again, markdown);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_sidecar_is_an_error_not_a_silent_loss() {
    let doc = document();
    let assets = MemoryAssetStore::new();
    let (markdown, _) = serialize(&doc, &assets, &options(RawPolicy::Sidecar));

    // The directory exists, the file does not: exactly the case where a
    // tolerant parser would hand back a document missing its raw-block.
    let dir = temp_dir("missing");
    let mut back_assets = MemoryAssetStore::new();
    let error = parse_with_base(&markdown, Some(&dir), &mut back_assets)
        .expect_err("a missing sidecar must fail");
    let message = error.to_string();
    assert!(
        message.contains("raw-0001.xml"),
        "the error should name the file it could not read: {message}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stub_without_a_base_directory_says_so() {
    let doc = document();
    let assets = MemoryAssetStore::new();
    let (markdown, _) = serialize(&doc, &assets, &options(RawPolicy::Sidecar));

    let mut back_assets = MemoryAssetStore::new();
    let error = parse(&markdown, &mut back_assets).expect_err("nowhere to read the sidecar from");
    assert!(
        error.to_string().contains("base directory"),
        "unexpected error: {error}"
    );
}

#[test]
fn inline_stays_self_contained() {
    let doc = document();
    let assets = MemoryAssetStore::new();
    let options = options(RawPolicy::Inline);
    let (markdown, _) = serialize(&doc, &assets, &options);

    assert!(markdown.contains(CONTENT), "the payload rides along");
    assert!(raw_sidecars(&doc, &options).is_empty());

    let mut back_assets = MemoryAssetStore::new();
    let (parsed, _) = parse(&markdown, &mut back_assets).expect("parses with no base directory");
    assert_eq!(blocks(&parsed), blocks(&doc));
}

#[test]
fn the_sidecar_form_is_cheaper_to_read() {
    let doc = document();
    let assets = MemoryAssetStore::new();
    let (inline, _) = serialize(&doc, &assets, &options(RawPolicy::Inline));
    let (sidecar, _) = serialize(&doc, &assets, &options(RawPolicy::Sidecar));
    assert!(
        sidecar.len() < inline.len(),
        "the whole point is that the stub is smaller"
    );
}
