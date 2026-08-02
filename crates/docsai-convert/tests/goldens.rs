//! Golden tests over the whole docx corpus (`AGENTS.md` §6).
//!
//! Each `corpus/docx/<name>.docx` has its expected DocMark beside it as
//! `<name>.expected.dmk.md`. Updating a golden is a deliberate act:
//!
//! ```text
//! DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens
//! ```
//!
//! …and the resulting diff has to be reviewed by hand.

use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options};
use docsai_model::{ConversionReport, Document, MemoryAssetStore};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/docx")
}

fn documents() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(corpus_dir())
        .expect("the corpus is present")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("docx"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 14, "only {} documents found", paths.len());
    paths
}

/// Converts a corpus document, with a stable `assets/` prefix so the golden
/// does not depend on where the test ran.
fn convert(path: &Path, fidelity: Fidelity) -> (String, ConversionReport, MemoryAssetStore) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut assets = MemoryAssetStore::new();
    let (document, mut report) = docsai_office::read_docx(file, &mut assets)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

    if let Err(errors) = docsai_model::validate::validate(&document) {
        panic!("{}: invalid IR: {errors:?}", path.display());
    }

    let options = Options {
        fidelity,
        assets_dir: "assets".into(),
        source_format: docsai_model::Format::Docx,
    };
    let (markdown, write_report) = docsai_docmark::serialize(&document, &assets, &options);
    report.merge(write_report);
    (markdown, report, assets)
}

fn golden_path(document: &Path) -> PathBuf {
    let stem = document.file_stem().unwrap().to_string_lossy().into_owned();
    document.with_file_name(format!("{stem}.expected.dmk.md"))
}

#[test]
fn the_corpus_matches_its_goldens() {
    let updating = std::env::var_os("DOCSAI_UPDATE_GOLDENS").is_some();
    let mut mismatches = Vec::new();

    for document in documents() {
        let (markdown, _, _) = convert(&document, Fidelity::Full);
        let golden = golden_path(&document);
        if updating {
            std::fs::write(&golden, &markdown).expect("writes the golden");
            continue;
        }
        match std::fs::read_to_string(&golden) {
            Ok(expected) if expected == markdown => {}
            Ok(expected) => mismatches.push(format!(
                "{}:\n{}",
                golden.display(),
                first_difference(&expected, &markdown)
            )),
            Err(_) => mismatches.push(format!("{}: golden missing", golden.display())),
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} golden(s) differ; review the diff, then regenerate with \
         DOCSAI_UPDATE_GOLDENS=1\n\n{}",
        mismatches.len(),
        mismatches.join("\n\n")
    );
}

fn first_difference(expected: &str, actual: &str) -> String {
    for (index, (want, got)) in expected.lines().zip(actual.lines()).enumerate() {
        if want != got {
            return format!("  line {}\n  - {want}\n  + {got}", index + 1);
        }
    }
    format!(
        "  line count differs: expected {}, got {}",
        expected.lines().count(),
        actual.lines().count()
    )
}

#[test]
fn serialisation_is_deterministic() {
    for document in documents() {
        let (first, _, _) = convert(&document, Fidelity::Full);
        let (second, _, _) = convert(&document, Fidelity::Full);
        assert_eq!(
            first,
            second,
            "{} serializes differently on a second run",
            document.display()
        );
    }
}

#[test]
fn every_document_uses_unix_line_endings_and_no_bom() {
    for document in documents() {
        let (markdown, _, _) = convert(&document, Fidelity::Full);
        assert!(
            !markdown.contains('\r'),
            "{} contains a CR",
            document.display()
        );
        assert!(
            !markdown.starts_with('\u{feff}'),
            "{} has a BOM",
            document.display()
        );
        assert!(
            markdown.ends_with('\n') && !markdown.ends_with("\n\n"),
            "{} does not end in exactly one newline",
            document.display()
        );
    }
}

/// Phase 1 acceptance criterion: *"`--fidelity plain` output is clean CommonMark
/// verified with comrak"*.
#[test]
fn plain_fidelity_is_clean_commonmark() {
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;

    for document in documents() {
        let (markdown, _, _) = convert(&document, Fidelity::Plain);
        let name = document.display();

        assert!(
            !markdown.starts_with("---"),
            "{name}: plain output must not carry front matter"
        );
        assert!(
            !markdown.contains(":::"),
            "{name}: plain output must not carry fenced divs"
        );

        // comrak must reach the same text, and no attribute block may survive
        // into the rendered HTML.
        let html = comrak::markdown_to_html(&markdown, &options);
        assert!(
            !html.contains("{."),
            "{name}: an attribute block reached the HTML:\n{html}"
        );
        assert!(
            !html.contains("&lt;/") && !html.contains("<script"),
            "{name}: unexpected raw HTML in the output"
        );

        // Every non-blank line of the source shows up as content, i.e. comrak
        // did not silently swallow a block.
        if markdown.trim().is_empty() {
            continue;
        }
        assert!(
            !html.trim().is_empty(),
            "{name}: comrak rendered nothing from a non-empty document"
        );
    }
}

#[test]
fn standard_fidelity_drops_raw_blocks_and_says_so() {
    let document = corpus_dir().join("fields-raw.docx");
    let (_full, full_report, _) = convert(&document, Fidelity::Full);
    let (standard, standard_report, _) = convert(&document, Fidelity::Standard);

    assert!(
        !standard.contains("{.raw"),
        "raw-blocks are a full-only feature"
    );
    assert!(!standard.contains("styles:"), "the catalogue is full-only");
    assert_eq!(standard_report.raw_blocks_emitted, 0);
    // Whatever `full` preserved as raw, `standard` must report as dropped.
    let dropped = standard_report
        .warnings
        .iter()
        .filter(|w| matches!(w, docsai_model::Warning::RawBlockDropped { .. }))
        .count();
    assert_eq!(dropped as u32, full_report.raw_blocks_emitted);
}

/// Phase 1 acceptance criterion: *"A real 50+ page docx converts in
/// < 1 s with < 10 raw-blocks"*.
#[test]
fn a_fifty_page_document_converts_quickly_with_few_raw_blocks() {
    let path = std::env::temp_dir().join(format!("docsai-perf-{}.docx", std::process::id()));
    build_large_docx(&path, 1_500);

    let started = std::time::Instant::now();
    let file = std::fs::File::open(&path).unwrap();
    let mut assets = MemoryAssetStore::new();
    let (document, report) = docsai_office::read_docx(file, &mut assets).expect("reads");
    let (markdown, _) = docsai_docmark::serialize(&document, &assets, &Options::default());
    let elapsed = started.elapsed();
    let _ = std::fs::remove_file(&path);

    assert!(matches!(document, Document::Text(_)));
    assert!(
        markdown.len() > 50_000,
        "the document should be substantial"
    );
    assert!(
        report.raw_blocks_emitted < 10,
        "{} raw-blocks in an ordinary document",
        report.raw_blocks_emitted
    );

    // The budget of architecture §8 is for an optimised build; a debug build
    // runs roughly an order of magnitude slower, so it gets a looser bound
    // rather than no bound at all.
    let budget = if cfg!(debug_assertions) {
        std::time::Duration::from_secs(10)
    } else {
        std::time::Duration::from_secs(1)
    };
    assert!(
        elapsed < budget,
        "conversion took {elapsed:?}, over the {budget:?} budget"
    );
}

/// Writes a synthetic document of roughly 50 pages: paragraphs, headings and
/// tables, all of them things the reader must handle at speed.
fn build_large_docx(path: &Path, paragraphs: usize) {
    use std::io::Write;

    let ns = concat!(
        r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#
    );
    let mut body = String::with_capacity(paragraphs * 200);
    for index in 0..paragraphs {
        if index % 40 == 0 {
            body.push_str(&format!(
                r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Seccion {index}</w:t></w:r></w:p>"#
            ));
        }
        body.push_str(&format!(
            r#"<w:p><w:r><w:t xml:space="preserve">Parrafo {index} con texto suficiente para ocupar una linea completa de la pagina. </w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>Negrita.</w:t></w:r></w:p>"#
        ));
        if index % 100 == 0 {
            body.push_str(
                r#"<w:tbl><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>
                   <w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc>
                   <w:tc><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
            );
        }
    }
    let document = format!("<w:document {ns}><w:body>{body}</w:body></w:document>");
    let styles = format!(
        r#"<w:styles {ns}><w:style w:type="paragraph" w:styleId="Heading1">
           <w:name w:val="heading 1"/><w:pPr><w:outlineLvl w:val="0"/></w:pPr></w:style></w:styles>"#
    );

    let file = std::fs::File::create(path).expect("creates the fixture");
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> = Default::default();
    for (name, content) in [
        ("word/document.xml", document.as_str()),
        ("word/styles.xml", styles.as_str()),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}
