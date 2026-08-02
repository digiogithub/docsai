//! MCP server over stdio for docsai (Phase 7).
//!
//! This crate is a **library**, not a binary: the single distributed
//! `docsai` executable starts it with `docsai mcp` (architecture §2, §6).
//!
//! **Hard rule:** stdout carries the JSON-RPC protocol and nothing else;
//! every log goes to stderr.

#![forbid(unsafe_code)]

mod config;
mod input;
mod server;
mod tools;

pub use config::{
    McpConfig, DEFAULT_MAX_INPUT_BYTES, DEFAULT_TIMEOUT_SECS, ENV_MAX_INPUT_BYTES, ENV_TIMEOUT_SECS,
};
pub use server::DocsaiServer;

/// Names of the tools the server exposes (architecture §6).
pub const TOOLS: &[&str] = &[
    "convert_to_markdown",
    "convert_from_markdown",
    "inspect_document",
    "list_supported_formats",
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
    fn the_v1_tool_set_is_four_tools() {
        assert_eq!(TOOLS.len(), 4);
        assert!(TOOLS.contains(&"convert_to_markdown"));
        assert!(TOOLS.contains(&"convert_from_markdown"));
        assert!(TOOLS.contains(&"inspect_document"));
        assert!(TOOLS.contains(&"list_supported_formats"));
    }
}
