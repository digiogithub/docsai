//! MCP server over stdio for docsai (Phase 7).
//!
//! This crate is a **library**, not a binary: the single distributed
//! `docsai` executable starts it with `docsai mcp` (architecture §2, §6).
//!
//! **Hard rule:** stdout carries the JSON-RPC protocol and nothing else;
//! every log goes to stderr.

#![forbid(unsafe_code)]

mod config;
mod images;
mod input;
mod server;
mod tools;

pub use config::{
    McpConfig, DEFAULT_MAX_INPUT_BYTES, DEFAULT_TIMEOUT_SECS, ENV_MAX_INPUT_BYTES, ENV_TIMEOUT_SECS,
};
pub use images::ImagePolicy;
pub use server::DocsaiServer;

/// Names of the tools the server exposes (architecture §6).
///
/// The last three are the Phase 11 agent-native primitives, and they are what
/// an agent should reach for first: `outline_document` says what is in a
/// document, `search_document` says where it says something, and
/// `read_selection` hands over just that part. `convert_to_markdown` remains
/// the tool that reads a whole document — the expensive one.
pub const TOOLS: &[&str] = &[
    "convert_to_markdown",
    "convert_from_markdown",
    "inspect_document",
    "list_supported_formats",
    "outline_document",
    "search_document",
    "read_selection",
];

/// Runs the MCP server on stdio until the client disconnects.
///
/// Initialises `tracing` to **stderr** when a subscriber is not already set.
/// Intended for the `docsai mcp` CLI entry point.
pub fn run() -> anyhow::Result<()> {
    init_tracing();
    let config = McpConfig::from_env();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(serve_stdio(config))
}

/// Serves MCP over stdin/stdout with the given configuration.
pub async fn serve_stdio(config: McpConfig) -> anyhow::Result<()> {
    use rmcp::transport::stdio;
    use rmcp::ServiceExt;

    tracing::info!(
        max_input_bytes = config.max_input_bytes,
        timeout_secs = ?config.timeout.map(|d| d.as_secs()),
        "starting docsai MCP server on stdio"
    );

    let server = DocsaiServer::new(config);
    let running = server.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

/// Serves on an arbitrary async transport (tests).
pub async fn serve_transport<T, E, A>(
    config: McpConfig,
    transport: T,
) -> Result<
    rmcp::service::RunningService<rmcp::RoleServer, DocsaiServer>,
    rmcp::service::ServerInitializeError,
>
where
    T: rmcp::transport::IntoTransport<rmcp::RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    use rmcp::ServiceExt;
    DocsaiServer::new(config).serve(transport).await
}

fn init_tracing() {
    use tracing_subscriber::prelude::*;

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));

    // Ignore failure when the CLI (or tests) already installed a subscriber.
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(false)
                .with_target(false),
        )
        .with(env_filter)
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_set_is_the_four_converters_plus_the_three_primitives() {
        assert_eq!(TOOLS.len(), 7);
        for expected in [
            "convert_to_markdown",
            "convert_from_markdown",
            "inspect_document",
            "list_supported_formats",
            "outline_document",
            "search_document",
            "read_selection",
        ] {
            assert!(TOOLS.contains(&expected), "missing {expected}");
        }
    }
}
