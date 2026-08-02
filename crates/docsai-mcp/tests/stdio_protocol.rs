//! End-to-end MCP protocol tests over an in-memory duplex transport.
//!
//! These replace interactive MCP Inspector checks in CI: a real client
//! handshake, tool listing, convert round-trip, and malformed-input errors.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use docsai_mcp::{serve_transport, McpConfig, TOOLS};
use rmcp::model::CallToolRequestParams;
use rmcp::ServiceExt;
use serde_json::{json, Map, Value};
use tokio::task::JoinHandle;

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/docx")
        .join(name)
}

fn test_config() -> McpConfig {
    McpConfig {
        max_input_bytes: 20 * 1024 * 1024,
        timeout: Some(std::time::Duration::from_secs(60)),
    }
}

fn args_object(value: Value) -> Map<String, Value> {
    value.as_object().expect("object").clone()
}

async fn connect_duplex() -> (
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    JoinHandle<anyhow::Result<()>>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let config = test_config();
    let server = tokio::spawn(async move {
        let running = serve_transport(config, server_transport).await?;
        running.waiting().await?;
        Ok(())
    });
    let client = ().serve(client_transport).await.expect("client handshake");
    (client, server)
}

#[tokio::test]
async fn lists_the_four_tools() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;
    let listed = client.list_tools(None).await?;
    let names: Vec<_> = listed.tools.iter().map(|t| t.name.to_string()).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    for expected in TOOLS {
        assert!(
            names.contains(expected),
            "missing tool {expected} in {names:?}"
        );
    }
    assert_eq!(names.len(), TOOLS.len());
    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

#[tokio::test]
async fn convert_docx_to_markdown_and_back() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;
    let path = corpus("basic-text.docx");

    let to = client
        .call_tool(
            CallToolRequestParams::new("convert_to_markdown").with_arguments(args_object(json!({
                "path": path.display().to_string(),
                "fidelity": "full",
                "assets": "inline-base64"
            }))),
        )
        .await?;
    assert_ne!(
        to.is_error,
        Some(true),
        "to_markdown error: {:?}",
        to.content
    );
    let structured = to.structured_content.expect("structured content");
    assert_eq!(structured["source_format"], "docx");
    let markdown = structured["markdown"].as_str().unwrap().to_string();
    assert!(markdown.contains("docmark"));

    let back = client
        .call_tool(
            CallToolRequestParams::new("convert_from_markdown").with_arguments(args_object(
                json!({
                    "markdown": markdown,
                    "target_format": "docx"
                }),
            )),
        )
        .await?;
    assert_ne!(
        back.is_error,
        Some(true),
        "from_markdown error: {:?}",
        back.content
    );
    let structured = back.structured_content.expect("structured");
    assert_eq!(structured["target_format"], "docx");
    assert!(structured["content_base64"].as_str().unwrap().len() > 16);

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

#[tokio::test]
async fn malformed_input_returns_tool_error_not_hang() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;

    let result = client
        .call_tool(
            CallToolRequestParams::new("inspect_document").with_arguments(args_object(json!({
                "content_base64": "not!!!base64",
                "filename": "x.docx"
            }))),
        )
        .await?;
    assert_eq!(result.is_error, Some(true));
    let text = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("base64") || text.contains("invalid"),
        "unexpected error text: {text}"
    );

    // Server stays alive for a follow-up call.
    let formats = client
        .call_tool(CallToolRequestParams::new("list_supported_formats"))
        .await?;
    assert_ne!(formats.is_error, Some(true));

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

#[tokio::test]
async fn session_of_many_conversions_stays_stable() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;
    let path = corpus("basic-text.docx").display().to_string();
    let ok = Arc::new(AtomicUsize::new(0));

    for _ in 0..25 {
        let result = client
            .call_tool(
                CallToolRequestParams::new("convert_to_markdown").with_arguments(args_object(
                    json!({
                        "path": path,
                        "assets": "inline-base64"
                    }),
                )),
            )
            .await?;
        assert_ne!(result.is_error, Some(true));
        ok.fetch_add(1, Ordering::SeqCst);
    }
    assert_eq!(ok.load(Ordering::SeqCst), 25);

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}
