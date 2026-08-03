//! The document map and its budget (plan v2 Phase 10, increment E).

use std::path::{Path, PathBuf};

use docsai_convert::{outline_path, token_report_path, ConvertOptions, Fidelity, Outline};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn corpus(relative: &str) -> PathBuf {
    corpus_root().join(relative)
}

fn documents() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (subdir, extension) in [
        ("docx", "docx"),
        ("xlsx", "xlsx"),
        ("odt", "odt"),
        ("ods", "ods"),
    ] {
        let dir = corpus_root().join(subdir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(extension) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn outline_of(path: &Path) -> Outline {
    outline_path(path, &ConvertOptions::default(), None).expect("outline")
}

#[test]
fn the_tree_follows_what_contains_what() {
    let outline = outline_of(&corpus("docx/nested-lists.docx"));
    // Two top-level lists; the numbered one nests two levels deeper.
    let ids: Vec<&str> = outline.nodes.iter().map(|n| n.id.0.as_str()).collect();
    assert_eq!(ids, vec!["n1", "n4"]);
    assert_eq!(outline.nodes[0].children.len(), 1);
    assert_eq!(outline.nodes[0].children[0].children.len(), 1);
    assert_eq!(outline.len(), 5, "five addressed nodes in all");
}

#[test]
fn a_footnote_hangs_from_the_paragraph_that_calls_it() {
    let outline = outline_of(&corpus("docx/footnotes.docx"));
    let first = &outline.nodes[0];
    assert_eq!(first.kind, docsai_model::NodeKind::Paragraph);
    assert_eq!(first.children.len(), 1);
    assert_eq!(first.children[0].kind, docsai_model::NodeKind::Footnote);
}

#[test]
fn previews_carry_the_text_and_not_the_machinery() {
    let outline = outline_of(&corpus("docx/long-report.docx"));
    let first = &outline.nodes[0];
    assert_eq!(first.preview, "# Informe tecnico de seguimiento");
    for node in &outline.nodes {
        assert!(
            !node.preview.contains("{#"),
            "the id is already its own column: {:?}",
            node.preview
        );
        assert!(node.preview.chars().count() <= 60, "{:?}", node.preview);
    }
}

#[test]
fn depth_cuts_the_tree_without_touching_the_top() {
    let path = corpus("docx/nested-lists.docx");
    let full = outline_of(&path);
    let shallow = outline_path(&path, &ConvertOptions::default(), Some(1)).expect("outline");

    assert_eq!(shallow.nodes.len(), full.nodes.len());
    assert!(shallow.nodes.iter().all(|n| n.children.is_empty()));
    assert!(shallow.outline_tokens < full.outline_tokens);
}

#[test]
fn the_lossy_levels_have_nothing_to_map() {
    let options = ConvertOptions {
        fidelity: Fidelity::Plain,
        ..Default::default()
    };
    let outline = outline_path(&corpus("docx/long-report.docx"), &options, None).expect("outline");
    assert!(outline.is_empty(), "ids — and so the map — live at `full`");
}

/// Phase 10 acceptance criterion: the map of the largest corpus document must
/// cost under 5 % of the document itself, or an agent may as well read it all.
#[test]
fn the_outline_of_the_largest_document_stays_under_five_percent() {
    let mut heaviest: Option<(PathBuf, usize)> = None;
    for document in documents() {
        let report =
            token_report_path(&document, &ConvertOptions::default()).expect("token report");
        if heaviest
            .as_ref()
            .is_none_or(|(_, total)| report.total > *total)
        {
            heaviest = Some((document, report.total));
        }
    }
    let (path, total) = heaviest.expect("the corpus is not empty");
    let outline = outline_of(&path);

    let share = outline.outline_tokens as f64 * 100.0 / total as f64;
    assert!(
        share < 5.0,
        "{}: outline {} tokens of {} ({share:.1} %)",
        path.display(),
        outline.outline_tokens,
        total
    );
    assert!(
        total > 5_000,
        "{}: the largest corpus document is too small for this to mean anything ({total} tokens)",
        path.display()
    );
}
