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
        // These tests assert on fields, so they ask for the object; the
        // default — text only — is what `one_representation_per_response`
        // checks.
        structured: true,
        max_inline_tokens: None,
        // The read cache a real server runs with, so the protocol suite runs
        // over it too and would catch an answer the cache changed.
        cache: Some(docsai_convert::ConvertCache::shared()),
    }
}

fn args_object(value: Value) -> Map<String, Value> {
    value.as_object().expect("object").clone()
}

/// Calls a tool and returns its structured content, failing on a tool error.
async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    args: Value,
) -> anyhow::Result<Value> {
    let result = client
        .call_tool(CallToolRequestParams::new(name).with_arguments(args_object(args)))
        .await?;
    assert_ne!(
        result.is_error,
        Some(true),
        "{name} failed: {:?}",
        result.content
    );
    Ok(result.structured_content.expect("structured content"))
}

async fn connect_duplex() -> (
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    JoinHandle<anyhow::Result<()>>,
) {
    connect_with(test_config()).await
}

async fn connect_with(
    config: McpConfig,
) -> (
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    JoinHandle<anyhow::Result<()>>,
) {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        let running = serve_transport(config, server_transport).await?;
        running.waiting().await?;
        Ok(())
    });
    let client = ().serve(client_transport).await.expect("client handshake");
    (client, server)
}

#[tokio::test]
async fn lists_every_tool_it_declares() -> anyhow::Result<()> {
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

/// The workflow Phase 11 exists for, over the real protocol: map the document,
/// find the words in it, read back only that part — never the document.
#[tokio::test]
async fn outline_then_search_then_read_costs_a_fraction_of_the_document() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;
    let path = corpus("long-report.docx").display().to_string();

    let outline = call(&client, "outline_document", json!({ "path": path })).await?;
    let document_tokens = outline["document-tokens"].as_u64().unwrap();
    assert!(outline["nodes"].as_array().unwrap().len() > 10);
    assert!(
        outline["outline-tokens"].as_u64().unwrap() * 10 < document_tokens,
        "the map has to be cheaper than the territory"
    );

    let hits = call(
        &client,
        "search_document",
        json!({ "path": path, "query": "rendimiento", "limit": 5 }),
    )
    .await?;
    assert!(hits["matches"].as_u64().unwrap() > 0);
    assert!(hits["tokens"].as_u64().unwrap() * 5 < document_tokens);

    // A hit that names a selector is a hit `read_selection` can act on: this is
    // the join between the three tools, and it is checked, not assumed.
    let select = hits["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|hit| hit["select"].as_str())
        .expect("at least one addressed hit")
        .to_string();
    let matched = hits["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["select"].as_str() == Some(select.as_str()))
        .and_then(|hit| hit["snippets"][0]["matched"].as_str())
        .expect("the snippet it matched")
        .to_lowercase();

    let selection = call(
        &client,
        "read_selection",
        json!({ "path": path, "select": select }),
    )
    .await?;
    let docmark = selection["docmark"].as_str().unwrap();
    assert!(
        docmark.to_lowercase().contains(&matched),
        "read_selection has to return what search found there:\n{docmark}"
    );
    assert!(docmark.contains("partial: true"), "{docmark}");
    assert!(selection["tokens"].as_u64().unwrap() * 10 < document_tokens);

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

#[tokio::test]
async fn images_are_not_sent_unless_they_are_asked_for() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;
    let path = corpus("images-inline.docx").display().to_string();

    let default = call(&client, "convert_to_markdown", json!({ "path": path })).await?;
    assert_eq!(default["include_images"], "refs");
    assert!(default["image_count"].as_u64().unwrap() > 0);
    for row in default["assets"].as_array().unwrap() {
        assert!(row.get("content_base64").is_none());
    }

    let full = call(
        &client,
        "convert_to_markdown",
        json!({ "path": path, "include_images": "full" }),
    )
    .await?;
    assert_eq!(default["markdown"], full["markdown"], "same document");
    assert!(!full["assets"][0]["content_base64"]
        .as_str()
        .unwrap()
        .is_empty());

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

/// E1: a response says everything once.
///
/// `CallToolResult::structured` sent the same object twice — escaped in the
/// text block and again as `structuredContent` — and the client paid for both.
/// The default is now the text block alone, and for the primitives that text is
/// the compact reading form, not the JSON.
#[tokio::test]
async fn one_representation_per_response_by_default() -> anyhow::Result<()> {
    let config = McpConfig {
        structured: false,
        ..test_config()
    };
    let (client, server) = connect_with(config).await;
    let path = corpus("long-report.docx").display().to_string();

    let result = client
        .call_tool(
            CallToolRequestParams::new("outline_document")
                .with_arguments(args_object(json!({ "path": path }))),
        )
        .await?;
    assert_ne!(result.is_error, Some(true), "{:?}", result.content);
    assert!(
        result.structured_content.is_none(),
        "the object is opt-in since E1"
    );
    let text = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("document-tokens="), "{text}");
    assert!(
        serde_json::from_str::<Value>(&text).is_err(),
        "the text form of an outline is the outline, not its JSON"
    );

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

/// E2 + E3: the document moves between files, and the response is a receipt.
#[tokio::test]
async fn a_document_moves_between_files_not_through_the_response() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;
    let dir = tempfile::tempdir()?;
    let markdown = dir.path().join("report.dmk.md");
    let package = dir.path().join("again.docx");

    let receipt = call(
        &client,
        "convert_to_markdown",
        json!({
            "path": corpus("long-report.docx").display().to_string(),
            "output_path": markdown.display().to_string()
        }),
    )
    .await?;
    assert!(receipt.get("markdown").is_none(), "{receipt}");
    assert_eq!(receipt["output_path"], markdown.display().to_string());
    assert!(markdown.exists());

    // The written file is a first-class input to the primitives.
    let outline = call(
        &client,
        "outline_document",
        json!({ "path": markdown.display().to_string() }),
    )
    .await?;
    assert!(!outline["nodes"].as_array().unwrap().is_empty());

    let back = call(
        &client,
        "convert_from_markdown",
        json!({
            "markdown_path": markdown.display().to_string(),
            "target_format": "docx",
            "path": package.display().to_string()
        }),
    )
    .await?;
    assert!(back["content_base64"].is_null(), "{back}");
    assert!(package.exists());

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

/// E5: the one call that prices the document before anything reads it.
#[tokio::test]
async fn estimate_tokens_prices_the_document_for_almost_nothing() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;
    let budget = call(
        &client,
        "estimate_tokens",
        json!({ "path": corpus("long-report.docx").display().to_string() }),
    )
    .await?;
    let levels = budget["levels"].as_array().unwrap();
    assert_eq!(levels.len(), 4);
    assert!(levels.iter().all(|l| l["total"].as_u64().unwrap() > 0));

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

#[tokio::test]
async fn the_cheat_sheet_is_a_resource_and_it_tracks_the_document() -> anyhow::Result<()> {
    let (client, server) = connect_duplex().await;

    // Discovery: one template, no static resources. A cheat-sheet without a
    // document does not exist, so there is nothing to list.
    let templates = client.list_resource_templates(None).await?;
    assert_eq!(templates.resource_templates.len(), 1);
    assert_eq!(
        templates.resource_templates[0].uri_template,
        "docmark://cheatsheet/{path}"
    );
    assert!(client.list_resources(None).await?.resources.is_empty());

    let read = |uri: String| {
        let client = &client;
        async move {
            let result = client
                .read_resource(rmcp::model::ReadResourceRequestParams::new(uri))
                .await?;
            let rmcp::model::ResourceContents::TextResourceContents { text, .. } =
                &result.contents[0]
            else {
                panic!("a cheat-sheet is text");
            };
            anyhow::Ok(text.clone())
        }
    };

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let uri = |relative: &str| {
        format!(
            "docmark://cheatsheet/{}",
            root.join(relative).to_string_lossy()
        )
    };
    let plain = read(uri("docx/basic-text.docx")).await?;
    let deck = read(uri("pptx/charts-multi-series.pptx")).await?;

    // Contextual, and that is the whole feature: a plain report is not told
    // about charts or slides, and it is materially cheaper for it.
    assert!(!plain.contains(".slide"), "{plain}");
    assert!(!plain.contains(".chart"), "{plain}");
    assert!(deck.contains(".slide") && deck.contains(".chart"), "{deck}");
    assert!(
        docsai_convert::tokens::count(&deck) > docsai_convert::tokens::count(&plain),
        "plain {} tokens, deck {}",
        docsai_convert::tokens::count(&plain),
        docsai_convert::tokens::count(&deck)
    );

    // A URI this server does not serve is a refusal that names what it does.
    let refused = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            uri("docx/basic-text.docx").replace("cheatsheet", "outline"),
        ))
        .await;
    let error = refused.expect_err("an unknown resource is refused");
    assert!(
        error.to_string().contains("docmark://cheatsheet/{path}"),
        "{error}"
    );

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}
