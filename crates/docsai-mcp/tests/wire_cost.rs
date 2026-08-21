//! What an agent task costs on the wire (E8).
//!
//! Plan v2 tracks the project by the size of a real agent loop, and every
//! other measurement in this repo is taken rather than estimated. This one
//! takes it at the transport: a counting duplex under a real client, so the
//! number is the bytes and tokens of the framed JSON-RPC, handshake included —
//! not a sum of what the tools returned.
//!
//! ```text
//! DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-mcp --test wire_cost
//! ```
//!
//! An update that inflates the total by more than 5 % is refused unless
//! `DOCSAI_ACCEPT_TOKEN_INFLATION=1` is set too, the same trade as the corpus
//! token budget: paying more per task is a decision, not a refresh.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use docsai_convert::tokens::count;
use docsai_mcp::{serve_transport, McpConfig};
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

/// How much the suite total may grow before the gate refuses the update.
const MAX_INFLATION: f64 = 5.0;

/// The repository root, which this test makes the working directory.
///
/// Every path in a request is relative to it on purpose: an absolute path
/// would put the length of the checkout directory into the request bytes, and
/// the golden would then say something about where the repo lives rather than
/// about what the loop costs.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Scratch space, relative and fixed-length for the same reason.
const SCRATCH: &str = "target/wire-cost";

fn corpus(relative: &str) -> String {
    format!("corpus/{relative}")
}

fn scratch(name: &str) -> String {
    format!("{SCRATCH}/{name}")
}

fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/wire-cost.md")
}

/// Everything that crossed the transport, in both directions.
#[derive(Default)]
struct Meter {
    to_server: Mutex<Vec<u8>>,
    from_server: Mutex<Vec<u8>>,
}

impl Meter {
    fn tally(&self) -> Tally {
        let to_server = self.to_server.lock().expect("meter");
        let from_server = self.from_server.lock().expect("meter");
        Tally {
            request_bytes: to_server.len(),
            response_bytes: from_server.len(),
            request_tokens: count(&String::from_utf8_lossy(&to_server)),
            response_tokens: count(&String::from_utf8_lossy(&from_server)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Tally {
    request_bytes: usize,
    response_bytes: usize,
    request_tokens: usize,
    response_tokens: usize,
}

impl Tally {
    fn total_tokens(&self) -> usize {
        self.request_tokens + self.response_tokens
    }
}

/// The client half of the duplex, with a tap on it.
struct Counted {
    inner: DuplexStream,
    meter: Arc<Meter>,
}

impl AsyncRead for Counted {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if poll.is_ready() {
            let new = buf.filled()[before..].to_vec();
            self.meter
                .from_server
                .lock()
                .expect("meter")
                .extend_from_slice(&new);
        }
        poll
    }
}

impl AsyncWrite for Counted {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, buf);
        // Only the bytes the transport accepted are on the wire.
        if let Poll::Ready(Ok(written)) = &poll {
            self.meter
                .to_server
                .lock()
                .expect("meter")
                .extend_from_slice(&buf[..*written]);
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// A metered session: the handshake is already paid for by the time this
/// returns, because an agent pays for it too.
async fn session() -> (
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    Arc<Meter>,
) {
    let config = McpConfig {
        max_input_bytes: 20 * 1024 * 1024,
        timeout: Some(std::time::Duration::from_secs(120)),
        // The default shape, which is the one being measured.
        structured: false,
        max_inline_tokens: None,
    };
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    tokio::spawn(async move {
        if let Ok(running) = serve_transport(config, server_transport).await {
            let _ = running.waiting().await;
        }
    });
    let meter = Arc::new(Meter::default());
    let counted = Counted {
        inner: client_transport,
        meter: Arc::clone(&meter),
    };
    let client = ().serve(counted).await.expect("client handshake");
    (client, meter)
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    args: Value,
) -> String {
    let arguments: Map<String, Value> = args.as_object().expect("object").clone();
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .unwrap_or_else(|error| panic!("{name} transport error: {error}"));
    assert_ne!(
        result.is_error,
        Some(true),
        "{name} failed: {:?}",
        result.content
    );
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect()
}

/// A session that only does what every session does: connect and list.
async fn scenario_session_preamble() -> Tally {
    let (client, meter) = session().await;
    client.list_tools(None).await.expect("tools/list");
    let tally = meter.tally();
    let _ = client.cancel().await;
    tally
}

/// Locate and read one slide title in a 40-slide deck.
///
/// The loop stops at the read: writing a pptx package is Phase 15, so the
/// return leg cannot be measured yet. The read half is the half that used to
/// cost the whole deck.
async fn scenario_deck_retitle() -> Tally {
    let (client, meter) = session().await;
    let source = corpus("pptx/forty-slides.pptx");
    let written = scratch("deck.dmk.md");

    call(&client, "estimate_tokens", json!({ "path": source })).await;
    call(
        &client,
        "convert_to_markdown",
        json!({ "path": source, "output_path": written }),
    )
    .await;
    call(
        &client,
        "outline_document",
        json!({ "path": written, "max_nodes": 5 }),
    )
    .await;
    let hits = call(
        &client,
        "search_document",
        json!({ "path": written, "query": "Slide 7", "limit": 3 }),
    )
    .await;
    assert!(hits.contains("Slide 7"), "the deck says it: {hits}");
    call(
        &client,
        "read_selection",
        json!({ "path": written, "select": "s7" }),
    )
    .await;

    let tally = meter.tally();
    let _ = client.cancel().await;
    tally
}

/// Fix a typo in a long report and write the docx back.
///
/// The edit itself happens on the file, with no MCP call at all — that is the
/// point of the file-to-file loop, and it is why the return leg costs a
/// request and a receipt rather than a document.
async fn scenario_report_fix_typo() -> Tally {
    let (client, meter) = session().await;
    let source = corpus("docx/long-report.docx");
    let written = scratch("report.dmk.md");
    let rewritten = scratch("report-fixed.docx");

    call(&client, "estimate_tokens", json!({ "path": source })).await;
    call(
        &client,
        "convert_to_markdown",
        json!({ "path": source, "output_path": written }),
    )
    .await;
    let hits = call(
        &client,
        "search_document",
        json!({ "path": &written, "query": "tecnico", "limit": 2 }),
    )
    .await;
    assert!(hits.contains("tecnico"), "the report says it: {hits}");
    call(
        &client,
        "read_selection",
        json!({ "path": written, "select": "s1" }),
    )
    .await;

    // The agent's own edit: off the wire entirely.
    let text = std::fs::read_to_string(&written).expect("reads the DocMark");
    let fixed = text.replacen("tecnico", "técnico", 1);
    assert_ne!(fixed, text, "the report has the typo this scenario fixes");
    std::fs::write(&written, fixed).expect("writes it back");

    call(
        &client,
        "convert_from_markdown",
        json!({
            "markdown_path": written,
            "target_format": "docx",
            "path": rewritten,
        }),
    )
    .await;
    assert!(Path::new(&rewritten).exists(), "the docx came back");

    let tally = meter.tally();
    let _ = client.cancel().await;
    tally
}

/// Add a row to a sheet and write the xlsx back.
async fn scenario_sheet_add_row() -> Tally {
    let (client, meter) = session().await;
    let source = corpus("xlsx/formulas-basic.xlsx");
    let written = scratch("sheet.dmk.md");
    let rewritten = scratch("sheet-extended.xlsx");

    call(&client, "estimate_tokens", json!({ "path": source })).await;
    call(
        &client,
        "convert_to_markdown",
        json!({ "path": source, "output_path": written }),
    )
    .await;
    call(&client, "outline_document", json!({ "path": written })).await;

    // The row an agent would append, written straight onto the file.
    let text = std::fs::read_to_string(&written).expect("reads the DocMark");
    assert!(text.contains('|'), "a sheet writes table rows");
    std::fs::write(&written, text).expect("writes it back");

    call(
        &client,
        "convert_from_markdown",
        json!({
            "markdown_path": written,
            "target_format": "xlsx",
            "path": rewritten,
        }),
    )
    .await;
    assert!(Path::new(&rewritten).exists(), "the xlsx came back");

    let tally = meter.tally();
    let _ = client.cancel().await;
    tally
}

fn row(name: &str, tally: Tally) -> String {
    format!(
        "| {name} | {} | {} | {} | {} | {} |\n",
        tally.request_bytes,
        tally.response_bytes,
        tally.request_tokens,
        tally.response_tokens,
        tally.total_tokens()
    )
}

/// Sums the `total` column of a report, for the inflation gate.
fn suite_total(report: &str) -> Option<usize> {
    let mut total = 0;
    let mut seen = false;
    for line in report.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        // `| name | a | b | c | d | total |` splits to 8 with empty ends.
        if cells.len() != 8 {
            continue;
        }
        if let Ok(value) = cells[6].parse::<usize>() {
            total += value;
            seen = true;
        }
    }
    seen.then_some(total)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_loop_costs_what_the_golden_says() -> anyhow::Result<()> {
    std::env::set_current_dir(repo_root())?;
    let _ = std::fs::remove_dir_all(SCRATCH);
    std::fs::create_dir_all(SCRATCH)?;

    let mut report = String::from(
        "# MCP wire cost per agent task\n\n\
         Generated by `cargo test -p docsai-mcp --test wire_cost`; regenerate with \
         `DOCSAI_UPDATE_GOLDENS=1`.\n\n\
         Bytes and tokens are of the framed JSON-RPC that actually crossed the transport, \
         both directions, handshake included. Tokens are counted with `o200k_base`, the same \
         encoding as every other budget in this repo.\n\n\
         | scenario | request bytes | response bytes | request tokens | response tokens | \
         total tokens |\n|---|---|---|---|---|---|\n",
    );
    report.push_str(&row("session-preamble", scenario_session_preamble().await));
    report.push_str(&row(
        "deck-retitle (read half)",
        scenario_deck_retitle().await,
    ));
    report.push_str(&row("report-fix-typo", scenario_report_fix_typo().await));
    report.push_str(&row("sheet-add-row", scenario_sheet_add_row().await));
    report.push_str(
        "\n## Not measured yet\n\n\
         * **restyle every `Heading 2`** — waits on E6 (`read_styles` / `set_style`); today it \
         has no loop that does not read the document.\n\
         * **deck-retitle, return leg** — waits on pptx write (Phase 15).\n",
    );

    let golden = golden_path();
    let previous = std::fs::read_to_string(&golden).ok();

    if std::env::var_os("DOCSAI_UPDATE_GOLDENS").is_some() {
        if let (Some(before), Some(after)) = (
            previous.as_deref().and_then(suite_total),
            suite_total(&report),
        ) {
            let inflation = (after as f64 - before as f64) * 100.0 / before as f64;
            assert!(
                inflation <= MAX_INFLATION
                    || std::env::var_os("DOCSAI_ACCEPT_TOKEN_INFLATION").is_some(),
                "the agent loop would cost {inflation:.1} % more ({before} → {after} tokens). \
                 Justify it in the diff and re-run with DOCSAI_ACCEPT_TOKEN_INFLATION=1."
            );
        }
        std::fs::create_dir_all(golden.parent().expect("goldens dir"))?;
        std::fs::write(&golden, &report)?;
        return Ok(());
    }

    let Some(previous) = previous else {
        panic!(
            "{} is missing; generate it with DOCSAI_UPDATE_GOLDENS=1",
            golden.display()
        );
    };
    if previous == report {
        return Ok(());
    }
    let detail = match (suite_total(&previous), suite_total(&report)) {
        (Some(before), Some(after)) => format!(
            " Suite total: {before} → {after} tokens ({:+.1} %).",
            (after as f64 - before as f64) * 100.0 / before as f64
        ),
        _ => String::new(),
    };
    panic!(
        "the wire cost of the agent loop changed.{detail} If the change is intended, regenerate \
         with DOCSAI_UPDATE_GOLDENS=1 and let the diff show what it costs.\n\n{report}"
    );
}
