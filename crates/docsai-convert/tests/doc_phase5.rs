//! Phase 5 convert wiring for legacy `.doc`.

use std::path::{Path, PathBuf};

use docsai_convert::{ConvertOptions, UseLoffice, SUPPORT};
use docsai_model::Format;

fn corpus_doc(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/doc")
        .join(name)
}

#[test]
fn support_matrix_lists_doc_read() {
    let row = SUPPORT.iter().find(|s| s.format == Format::Doc).unwrap();
    assert!(row.read);
    assert!(!row.write);
}

#[test]
fn convert_native_doc_to_docmark() {
    let outcome = docsai_convert::convert_file(
        &corpus_doc("basic-text.doc"),
        None,
        &ConvertOptions {
            use_loffice: UseLoffice::Never,
            ..Default::default()
        },
    )
    .expect("convert doc");
    assert_eq!(outcome.source_format, Format::Doc);
    assert_eq!(outcome.target_format, Format::DocMark);
    assert!(
        outcome.markdown.contains("Hello from Phase 5"),
        "{}",
        outcome.markdown
    );
    assert!(
        outcome.markdown.contains("Second paragraph"),
        "{}",
        outcome.markdown
    );
}

#[test]
fn require_loffice_without_binary_errors() {
    // Force the lookup to miss by pointing at a non-existent binary.
    std::env::set_var("DOCSAI_LIBREOFFICE", "/nonexistent/soffice-docsai-test");
    let err = docsai_convert::convert_file(
        &corpus_doc("basic-text.doc"),
        None,
        &ConvertOptions {
            use_loffice: UseLoffice::Require,
            ..Default::default()
        },
    )
    .unwrap_err();
    std::env::remove_var("DOCSAI_LIBREOFFICE");
    assert!(
        matches!(err, docsai_convert::ConvertError::Loffice { .. }),
        "got {err}"
    );
}

#[test]
fn encrypted_doc_is_an_input_error() {
    let err = docsai_convert::convert_file(
        &corpus_doc("encrypted.doc"),
        None,
        &ConvertOptions {
            use_loffice: UseLoffice::Never,
            ..Default::default()
        },
    )
    .unwrap_err();
    match err {
        docsai_convert::ConvertError::Read(docsai_office::ReadError::Encrypted) => {}
        other => panic!("expected encrypted, got {other}"),
    }
}
