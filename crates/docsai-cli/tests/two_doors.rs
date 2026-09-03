//! One implementation, two doors — asserted rather than intended (17-E).
//!
//! `docsai edit` and the `apply_edits` MCP tool are supposed to be the same
//! function with different argument parsing. This test does not read the code
//! to check that: it runs the **real `docsai` binary** and a **real MCP server
//! over a transport**, gives them the same operations on two copies of the same
//! corpus document, and compares
//!
//! * the two written files, byte for byte, and
//! * the two reports, field for field — including the new etags, which are the
//!   whole reason the phase claims no confirmation read is needed.
//!
//! A difference in either is a door carrying logic the other lacks.

use std::path::{Path, PathBuf};
use std::process::Command;

use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus(relative: &str) -> PathBuf {
    repo_root().join("corpus").join(relative)
}

/// Runs `docsai edit --json` and returns the report it printed.
///
/// `--json` only spaces the same object out, so the two doors are still being
/// compared on the same value.
fn cli_door(input: &Path, operations: &Value, dry_run: bool) -> (bool, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_docsai"));
    command
        .arg("edit")
        .arg(input)
        .arg("--ops")
        .arg(operations.to_string())
        .arg("--json");
    if dry_run {
        command.arg("--dry-run");
    }
    let output = command.output().expect("the docsai binary runs");
    let ok = output.status.success();
    let text = if ok {
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        String::from_utf8_lossy(&output.stderr).into_owned()
    };
    let value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("the CLI door answered with non-JSON ({error}): {text}"));
    (ok, value)
}

/// Calls `apply_edits` on a live server over a duplex transport.
async fn mcp_door(input: &Path, operations: &Value, dry_run: bool) -> (bool, Value) {
    let config = docsai_mcp::McpConfig {
        max_input_bytes: 64 * 1024 * 1024,
        timeout: None,
        structured: false,
        max_inline_tokens: None,
        cache: None,
    };
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = docsai_mcp::serve_transport(config, server_transport).await {
            let _ = running.waiting().await;
        }
    });
    let client = ().serve(client_transport).await.expect("handshake");

    let mut arguments = Map::new();
    arguments.insert("path".into(), json!(input.display().to_string()));
    arguments.insert("operations".into(), operations.clone());
    if dry_run {
        arguments.insert("dry_run".into(), json!(true));
    }
    let result = client
        .call_tool(CallToolRequestParams::new("apply_edits").with_arguments(arguments))
        .await
        .expect("apply_edits answered");
    let ok = result.is_error != Some(true);
    let text: String = result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|t| t.text.clone()))
        .collect();
    let _ = client.cancel().await;
    let value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("the MCP door answered with non-JSON ({error}): {text}"));
    (ok, value)
}

/// The report without `path`, which is the one field that names a different
/// file on purpose: each door edits its own copy.
fn without_path(report: &Value) -> Value {
    let mut value = report.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("path");
    }
    value
}

fn copy_for(dir: &Path, name: &str, source: &Path) -> PathBuf {
    let destination = dir.join(name);
    std::fs::copy(source, &destination).expect("the corpus document copies");
    destination
}

/// The phase's own loop: retitle a slide, add a bullet under it, bold the
/// bullet. Three operations, one call, both doors.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_edit_through_both_doors_writes_the_same_bytes_and_the_same_report() {
    let source = corpus("pptx/forty-slides.pptx");
    let dir = tempfile::tempdir().expect("scratch");
    let for_cli = copy_for(dir.path(), "cli.pptx", &source);
    let for_mcp = copy_for(dir.path(), "mcp.pptx", &source);

    let operations = json!([
        { "op": "set_notes", "target": "n4", "content": "Ask about Q4." },
        { "op": "duplicate_slide", "target": "n7" },
    ]);

    let (cli_ok, cli_report) = cli_door(&for_cli, &operations, false);
    let (mcp_ok, mcp_report) = mcp_door(&for_mcp, &operations, false).await;
    assert!(cli_ok, "the CLI door refused: {cli_report}");
    assert!(mcp_ok, "the MCP door refused: {mcp_report}");

    assert_eq!(
        without_path(&cli_report),
        without_path(&mcp_report),
        "the two doors reported different things about the same edit"
    );
    assert!(
        !cli_report["created"]
            .as_array()
            .map(Vec::is_empty)
            .unwrap_or(true),
        "duplicate_slide creates a slide and the report says so: {cli_report}"
    );

    let cli_bytes = std::fs::read(&for_cli).expect("the CLI wrote a file");
    let mcp_bytes = std::fs::read(&for_mcp).expect("the MCP tool wrote a file");
    assert_eq!(
        cli_bytes, mcp_bytes,
        "the two doors wrote different bytes for the same operations"
    );
}

/// A dry run through either door leaves the file byte-identical and reports
/// exactly what the real call would.
#[tokio::test(flavor = "multi_thread")]
async fn a_dry_run_agrees_across_the_doors_and_touches_nothing() {
    let source = corpus("docx/long-report.docx");
    let dir = tempfile::tempdir().expect("scratch");
    let for_cli = copy_for(dir.path(), "cli.docx", &source);
    let for_mcp = copy_for(dir.path(), "mcp.docx", &source);
    let before = std::fs::read(&source).expect("the corpus document reads");

    let operations = json!([{ "op": "set_props", "target": "n1", "props": { "bold": true } }]);

    let (cli_ok, cli_report) = cli_door(&for_cli, &operations, true);
    let (mcp_ok, mcp_report) = mcp_door(&for_mcp, &operations, true).await;
    assert!(cli_ok, "{cli_report}");
    assert!(mcp_ok, "{mcp_report}");
    assert_eq!(without_path(&cli_report), without_path(&mcp_report));
    assert_eq!(cli_report["dry_run"], json!(true));

    assert_eq!(std::fs::read(&for_cli).unwrap(), before, "a dry run writes");
    assert_eq!(std::fs::read(&for_mcp).unwrap(), before, "a dry run writes");
}

/// A refusal is typed on both doors, and it is the *same* refusal.
///
/// The three §8 cases the brief names: a stale etag, an unknown target, and an
/// operation of the wrong kind for what it was pointed at.
#[tokio::test(flavor = "multi_thread")]
async fn every_refusal_arrives_typed_and_identical_through_both_doors() {
    let source = corpus("pptx/forty-slides.pptx");
    let dir = tempfile::tempdir().expect("scratch");
    let for_cli = copy_for(dir.path(), "cli.pptx", &source);
    let for_mcp = copy_for(dir.path(), "mcp.pptx", &source);
    let before = std::fs::read(&source).expect("reads");

    let cases: [(&str, Value); 4] = [
        (
            "StaleEtag",
            json!([{
                "op": "set_notes", "target": "n4", "content": "x", "if_match": "000000"
            }]),
        ),
        (
            "UnknownTarget",
            json!([{ "op": "set_notes", "target": "n99999", "content": "x" }]),
        ),
        (
            "NotApplicableToClass",
            json!([{ "op": "set_cell", "target": "n4", "cell": "B7", "value": 1 }]),
        ),
        (
            "UnknownOperation",
            json!([{ "op": "recolour", "target": "n4" }]),
        ),
    ];

    for (expected, operations) in cases {
        let (cli_ok, cli_report) = cli_door(&for_cli, &operations, false);
        let (mcp_ok, mcp_report) = mcp_door(&for_mcp, &operations, false).await;
        assert!(!cli_ok, "{expected} should refuse: {cli_report}");
        assert!(!mcp_ok, "{expected} should refuse: {mcp_report}");
        assert_eq!(
            cli_report["refusal"],
            json!(expected),
            "the CLI door flattened or renamed the refusal: {cli_report}"
        );
        assert_eq!(
            cli_report, mcp_report,
            "the two doors named the refusal differently"
        );
        assert!(
            cli_report["message"].as_str().is_some(),
            "a refusal still carries a sentence for a human: {cli_report}"
        );
        assert_eq!(
            std::fs::read(&for_cli).unwrap(),
            before,
            "refused, untouched"
        );
        assert_eq!(
            std::fs::read(&for_mcp).unwrap(),
            before,
            "refused, untouched"
        );
    }
}

/// A stale etag names both sides, on both doors — the field the phase's
/// economics rest on.
#[tokio::test(flavor = "multi_thread")]
async fn a_stale_etag_carries_the_expected_and_the_actual_value() {
    let source = corpus("pptx/forty-slides.pptx");
    let dir = tempfile::tempdir().expect("scratch");
    let copy = copy_for(dir.path(), "deck.pptx", &source);
    let operations =
        json!([{ "op": "set_notes", "target": "n4", "content": "x", "if_match": "000000" }]);
    let (ok, report) = cli_door(&copy, &operations, false);
    assert!(!ok);
    assert_eq!(report["expected"], json!("000000"));
    assert!(
        report["actual"].as_str().is_some_and(|a| a != "000000"),
        "the document's own etag is in the refusal: {report}"
    );
}

/// `--output` writes elsewhere and leaves the input alone, and the report says
/// which file it wrote.
#[tokio::test(flavor = "multi_thread")]
async fn an_output_destination_leaves_the_input_byte_identical() {
    let source = corpus("pptx/forty-slides.pptx");
    let dir = tempfile::tempdir().expect("scratch");
    let input = copy_for(dir.path(), "in.pptx", &source);
    let elsewhere = dir.path().join("out.pptx");
    let before = std::fs::read(&input).expect("reads");

    let output = Command::new(env!("CARGO_BIN_EXE_docsai"))
        .arg("edit")
        .arg(&input)
        .arg("--ops")
        .arg(json!([{ "op": "set_notes", "target": "n4", "content": "Ask about Q4." }]).to_string())
        .arg("--output")
        .arg(&elsewhere)
        .output()
        .expect("runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(&input).unwrap(),
        before,
        "the input is intact"
    );
    assert!(elsewhere.exists(), "the edit went where it was told");
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(report["path"], json!(elsewhere.display().to_string()));
}

/// An edit is not a conversion: a destination whose name says another format is
/// refused rather than filled with the wrong bytes.
#[test]
fn a_destination_of_another_format_is_refused() {
    let source = corpus("pptx/forty-slides.pptx");
    let dir = tempfile::tempdir().expect("scratch");
    let input = copy_for(dir.path(), "in.pptx", &source);
    let output = Command::new(env!("CARGO_BIN_EXE_docsai"))
        .arg("edit")
        .arg(&input)
        .arg("--ops")
        .arg(json!([{ "op": "set_notes", "target": "n4", "content": "x" }]).to_string())
        .arg("--output")
        .arg(dir.path().join("out.docx"))
        .output()
        .expect("runs");
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("docx"), "{message}");
}
