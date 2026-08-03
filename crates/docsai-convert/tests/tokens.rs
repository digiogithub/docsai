//! Measured token cost (plan v2 Phase 10, increment D).

use std::path::{Path, PathBuf};

use docsai_convert::tokens::{count, ENCODING};
use docsai_convert::{token_report_path, ConvertOptions, Fidelity};

fn corpus(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../corpus/{relative}"))
}

fn options(fidelity: Fidelity) -> ConvertOptions {
    ConvertOptions {
        fidelity,
        ..Default::default()
    }
}

#[test]
fn a_document_reports_its_own_cost_split_in_two() {
    let report = token_report_path(&corpus("docx/nested-lists.docx"), &options(Fidelity::Full))
        .expect("token report");

    assert_eq!(report.encoding, ENCODING);
    assert_eq!(report.source_format, "docx");
    assert!(report.total > 0);
    assert!(report.body > 0);
    assert!(
        report.front_matter > 0,
        "full fidelity always writes a front matter"
    );
    // The parts are counted separately from the whole (BPE merges across the
    // boundary), so they bound the total rather than equalling it.
    assert!(report.front_matter + report.body <= report.total + 2);
    assert!(report.total <= report.bytes, "a token is never one byte");
}

#[test]
fn every_node_cost_is_measured_over_what_the_node_actually_wrote() {
    let report = token_report_path(&corpus("docx/nested-lists.docx"), &options(Fidelity::Full))
        .expect("token report");
    assert!(
        !report.nodes.is_empty(),
        "this document has addressed nodes"
    );

    for node in &report.nodes {
        assert!(node.tokens > 0, "{}: an empty fragment is a bug", node.id.0);
        assert!(
            node.tokens >= count(&node.preview),
            "{}: the node costs at least its own preview",
            node.id.0
        );
        assert!(
            node.tokens <= report.total,
            "{}: a node cannot cost more than the document",
            node.id.0
        );
        assert!(!node.preview.is_empty());
    }
}

#[test]
fn the_lossy_levels_cost_less_and_address_nothing() {
    let path = corpus("docx/nested-lists.docx");
    let full = token_report_path(&path, &options(Fidelity::Full)).expect("full");
    let standard = token_report_path(&path, &options(Fidelity::Standard)).expect("standard");
    let plain = token_report_path(&path, &options(Fidelity::Plain)).expect("plain");

    assert!(
        plain.total < standard.total && standard.total < full.total,
        "full {} standard {} plain {}",
        full.total,
        standard.total,
        plain.total
    );
    assert!(standard.nodes.is_empty(), "ids are a `full` feature");
    assert!(plain.nodes.is_empty());
    assert_eq!(
        plain.front_matter, 0,
        "plain is CommonMark, no front matter"
    );
}

#[test]
fn a_workbook_is_measured_sheet_by_sheet() {
    let report = token_report_path(
        &corpus("xlsx/formulas-basic.xlsx"),
        &options(Fidelity::Full),
    )
    .expect("token report");
    assert_eq!(report.source_format, "xlsx");
    let sheets: Vec<_> = report
        .nodes
        .iter()
        .filter(|n| n.kind == docsai_model::NodeKind::Sheet)
        .collect();
    assert_eq!(sheets.len(), 1, "one sheet, one addressed node");
    assert!(sheets[0].tokens > 0);
}

#[test]
fn the_heaviest_nodes_come_out_first() {
    let report = token_report_path(&corpus("docx/nested-lists.docx"), &options(Fidelity::Full))
        .expect("token report");
    let heaviest = report.heaviest(3);
    assert!(heaviest.len() <= 3);
    for pair in heaviest.windows(2) {
        assert!(pair[0].tokens >= pair[1].tokens);
    }
}
