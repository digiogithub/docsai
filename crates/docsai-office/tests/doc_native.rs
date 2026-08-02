//! Phase 5: native degraded `.doc` reading.

use std::io::Cursor;
use std::path::PathBuf;

use docsai_model::text::Block;
use docsai_model::{Document, MemoryAssetStore};

fn corpus_doc(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/doc")
        .join(name)
}

#[test]
fn corpus_basic_text_has_paragraphs() {
    let path = corpus_doc("basic-text.doc");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut assets = MemoryAssetStore::new();
    let (doc, report) = docsai_office::read_doc(Cursor::new(bytes), &mut assets).expect("read");
    let Document::Text(text) = doc else {
        panic!("expected text");
    };
    let plains: Vec<_> = text
        .sections
        .iter()
        .flat_map(|s| s.blocks.iter())
        .filter_map(|b| match b {
            Block::Paragraph(p) => {
                let t = p.plain_text();
                if t.is_empty() {
                    None
                } else {
                    Some(t)
                }
            }
            _ => None,
        })
        .collect();
    assert!(
        plains.iter().any(|p| p.contains("Hello from Phase 5")),
        "{plains:?}"
    );
    assert!(
        plains.iter().any(|p| p.contains("Second paragraph")),
        "{plains:?}"
    );
    assert!(report
        .warnings
        .iter()
        .any(|w| matches!(w, docsai_model::Warning::Degraded { what, .. } if what == "doc")));
}

#[test]
fn corpus_encrypted_is_rejected() {
    let path = corpus_doc("encrypted.doc");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut assets = MemoryAssetStore::new();
    let err = docsai_office::read_doc(Cursor::new(bytes), &mut assets).unwrap_err();
    assert!(
        matches!(err, docsai_office::ReadError::Encrypted),
        "got {err}"
    );
}

#[test]
fn detect_marks_corpus_doc_certain() {
    let path = corpus_doc("basic-text.doc");
    let bytes = std::fs::read(&path).unwrap();
    let (format, score) = docsai_office::detect(Cursor::new(bytes), Some("basic-text.doc"));
    assert_eq!(format, docsai_model::Format::Doc);
    assert_eq!(score, docsai_office::DetectScore::Certain);
}

#[test]
fn truncated_doc_never_panics() {
    let path = corpus_doc("basic-text.doc");
    let base = std::fs::read(&path).unwrap();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    for cut in (1..base.len()).step_by(17) {
        let truncated = base[..cut].to_vec();
        let ok = std::panic::catch_unwind(|| {
            let mut assets = MemoryAssetStore::new();
            let _ = docsai_office::read_doc(Cursor::new(truncated), &mut assets);
        });
        assert!(ok.is_ok(), "panicked on {cut}-byte prefix");
    }
    std::panic::set_hook(previous);
}
