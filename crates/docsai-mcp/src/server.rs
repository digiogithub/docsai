//! MCP server handler exposing the four docsai tools over stdio.

use std::sync::Arc;

use docsai_convert::ConvertError;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use serde::Deserialize;
use serde_json::Value;

use crate::config::McpConfig;
use crate::input::IncomingAsset;
use crate::tools;

/// The docsai MCP server (architecture §6).
#[derive(Clone)]
pub struct DocsaiServer {
    config: Arc<McpConfig>,
    tool_router: ToolRouter<Self>,
}

impl DocsaiServer {
    /// Builds a server with the given configuration.
    pub fn new(config: McpConfig) -> Self {
        DocsaiServer {
            config: Arc::new(config),
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for DocsaiServer {
    fn default() -> Self {
        Self::new(McpConfig::default())
    }
}

/// Arguments for `convert_to_markdown`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConvertToMarkdownArgs {
    /// Filesystem path to the source document (local MCP clients).
    #[serde(default)]
    pub path: Option<String>,
    /// Base64-encoded document bytes (remote / embedded content).
    #[serde(default)]
    pub content_base64: Option<String>,
    /// File name hint required with `content_base64` (e.g. `report.docx`).
    #[serde(default)]
    pub filename: Option<String>,
    /// Fidelity: `full` (default), `agent`, `standard`, or `plain`.
    #[serde(default)]
    pub fidelity: Option<String>,
    /// Asset delivery: `inline-base64` (default) or `files`.
    #[serde(default)]
    pub assets: Option<String>,
    /// Directory for `assets=files` (required with base64 input).
    #[serde(default)]
    pub assets_dir: Option<String>,
}

/// Arguments for `convert_from_markdown`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConvertFromMarkdownArgs {
    /// DocMark source text.
    pub markdown: String,
    /// Target format: `docx`, `xlsx`, `odt`, `ods`, or `docmark`.
    pub target_format: String,
    /// Optional output path; when omitted the package is returned as base64.
    #[serde(default)]
    pub path: Option<String>,
    /// Media referenced by the markdown (`file_name` + `content_base64`).
    #[serde(default)]
    pub assets: Vec<IncomingAsset>,
}

/// Arguments for `inspect_document`.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InspectDocumentArgs {
    /// Filesystem path to the source document.
    #[serde(default)]
    pub path: Option<String>,
    /// Base64-encoded document bytes.
    #[serde(default)]
    pub content_base64: Option<String>,
    /// File name hint required with `content_base64`.
    #[serde(default)]
    pub filename: Option<String>,
}

#[tool_router]
impl DocsaiServer {
    #[tool(
        name = "convert_to_markdown",
        description = "Convert an Office/LibreOffice document to DocMark (extended Markdown). Accepts a filesystem path or base64 content. Returns markdown, assets, and a conversion report."
    )]
    async fn convert_to_markdown(
        &self,
        Parameters(args): Parameters<ConvertToMarkdownArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || {
            tools::tool_convert_to_markdown(
                args.path.as_deref(),
                args.content_base64.as_deref(),
                args.filename.as_deref(),
                args.fidelity.as_deref(),
                args.assets.as_deref(),
                args.assets_dir.as_deref(),
                &config,
            )
        })
        .await
    }

    #[tool(
        name = "convert_from_markdown",
        description = "Convert DocMark markdown back to an Office/LibreOffice package (docx, xlsx, odt, ods). Returns base64 bytes or writes to path."
    )]
    async fn convert_from_markdown(
        &self,
        Parameters(args): Parameters<ConvertFromMarkdownArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || {
            tools::tool_convert_from_markdown(
                &args.markdown,
                &args.target_format,
                args.path.as_deref(),
                &args.assets,
                &config,
            )
        })
        .await
    }

    #[tool(
        name = "inspect_document",
        description = "Inspect document structure without converting: metadata, styles, sections/sheets, media, stats, and warnings. Same JSON shape as `docsai inspect --json`."
    )]
    async fn inspect_document(
        &self,
        Parameters(args): Parameters<InspectDocumentArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || {
            tools::tool_inspect_document(
                args.path.as_deref(),
                args.content_base64.as_deref(),
                args.filename.as_deref(),
                &config,
            )
        })
        .await
    }

    #[tool(
        name = "list_supported_formats",
        description = "List formats this docsai build can read and write, with status notes."
    )]
    async fn list_supported_formats(&self) -> Result<CallToolResult, McpError> {
        Ok(json_result(tools::tool_list_supported_formats()))
    }
}

impl DocsaiServer {
    async fn run_tool<F>(&self, work: F) -> Result<CallToolResult, McpError>
    where
        F: FnOnce() -> Result<Value, ConvertError> + Send + 'static,
    {
        let timeout = self.config.timeout;
        let join = tokio::task::spawn_blocking(work);
        let result = if let Some(duration) = timeout {
            match tokio::time::timeout(duration, join).await {
                Ok(joined) => joined,
                Err(_) => {
                    return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                        "tool timed out after {}s (DOCSAI_MCP_TIMEOUT_SECS)",
                        duration.as_secs()
                    ))]));
                }
            }
        } else {
            join.await
        };

        match result {
            Ok(Ok(value)) => Ok(json_result(value)),
            Ok(Err(error)) => Ok(convert_error_result(error)),
            Err(join_err) => Err(McpError::internal_error(
                format!("tool task failed: {join_err}"),
                None,
            )),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DocsaiServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("docsai", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Bidirectional converter between Office/LibreOffice documents and DocMark. \
                 Tools: convert_to_markdown, convert_from_markdown, inspect_document, \
                 list_supported_formats. Prefer path mode on local machines; use \
                 content_base64 + filename when the client cannot share a filesystem path. \
                 Logs always go to stderr; stdout is reserved for MCP JSON-RPC.",
            )
    }
}

fn json_result(value: Value) -> CallToolResult {
    CallToolResult::structured(value)
}

fn convert_error_result(error: ConvertError) -> CallToolResult {
    // Tool-level errors so the client sees the message (rmcp CallToolResult::error contract).
    CallToolResult::error(vec![ContentBlock::text(error.to_string())])
}
