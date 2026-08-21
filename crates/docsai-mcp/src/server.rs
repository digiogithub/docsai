//! MCP server handler exposing the docsai tools over stdio.
//!
//! The handlers are thin on purpose: argument shapes and the work both live in
//! [`crate::tools`], and this module decides only *how a result crosses the
//! wire* — one representation by default (E1), the object as well when
//! `DOCSAI_MCP_STRUCTURED=1` names a client that consumes it.

use std::sync::Arc;

use docsai_convert::ConvertError;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, ContentBlock, Implementation, ListToolsResult, PaginatedRequestParams,
        ResultType, ServerCapabilities, ServerInfo,
    },
    service::RequestContext,
    tool, tool_handler, tool_router, ErrorData as McpError, RoleServer, ServerHandler,
};

use crate::config::McpConfig;
use crate::schema::slim;
use crate::tools::{
    self, ConvertFromMarkdownArgs, ConvertToMarkdownArgs, DocumentArgs, OutlineDocumentArgs,
    ReadSelectionArgs, SearchDocumentArgs, ToolOutput,
};

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

#[tool_router]
impl DocsaiServer {
    #[tool(
        name = "convert_to_markdown",
        description = "Convert a whole document to DocMark. With `output_path` it is written to disk and the answer is a receipt; without one it comes back inline, up to the server's token ceiling. To read only part of a document, use outline_document and read_selection instead."
    )]
    async fn convert_to_markdown(
        &self,
        Parameters(args): Parameters<ConvertToMarkdownArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || tools::tool_convert_to_markdown(&args, &config))
            .await
    }

    #[tool(
        name = "convert_from_markdown",
        description = "Convert DocMark back to docx, xlsx, odt or ods. `markdown_path` + `path` converts file to file; `markdown` is for fragments, and only a call with no `path` answers with base64."
    )]
    async fn convert_from_markdown(
        &self,
        Parameters(args): Parameters<ConvertFromMarkdownArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || tools::tool_convert_from_markdown(&args, &config))
            .await
    }

    #[tool(
        name = "inspect_document",
        description = "Structure without content: metadata, styles, sections or sheets, media, stats, warnings. Same shape as `docsai inspect --json`."
    )]
    async fn inspect_document(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || tools::tool_inspect_document(&args, &config))
            .await
    }

    #[tool(
        name = "estimate_tokens",
        description = "Measured cost of reading this document at all four fidelity levels, in one call. A few dozen tokens whatever its size — call it before deciding to read."
    )]
    async fn estimate_tokens(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || tools::tool_estimate_tokens(&args, &config))
            .await
    }

    #[tool(
        name = "list_supported_formats",
        description = "Formats this build reads and writes, with status notes."
    )]
    async fn list_supported_formats(&self) -> Result<CallToolResult, McpError> {
        Ok(self.result(tools::tool_list_supported_formats()))
    }

    #[tool(
        name = "outline_document",
        description = "Map a document without reading it: addressable nodes with id, kind, preview and cost, plus what the map itself cost (`outline-ratio`). One level and 200 nodes by default; `omitted` and `next-cursor` say what was left out."
    )]
    async fn outline_document(
        &self,
        Parameters(args): Parameters<OutlineDocumentArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || tools::tool_outline_document(&args, &config))
            .await
    }

    #[tool(
        name = "search_document",
        description = "Where a document says something, without returning the document. Each hit gives the words around the match and an address; on an addressed block that address is a selector read_selection takes."
    )]
    async fn search_document(
        &self,
        Parameters(args): Parameters<SearchDocumentArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || tools::tool_search_document(&args, &config))
            .await
    }

    #[tool(
        name = "read_selection",
        description = "Part of a document as self-contained DocMark: the exact bytes it would write for those nodes, plus the front matter to write them back (next-id, partial, per-node etag). Selectors: s4, s7-s9, #n7, type:heading, text:foo."
    )]
    async fn read_selection(
        &self,
        Parameters(args): Parameters<ReadSelectionArgs>,
    ) -> Result<CallToolResult, McpError> {
        let config = Arc::clone(&self.config);
        self.run_tool(move || tools::tool_read_selection(&args, &config))
            .await
    }
}

impl DocsaiServer {
    async fn run_tool<F>(&self, work: F) -> Result<CallToolResult, McpError>
    where
        F: FnOnce() -> Result<ToolOutput, ConvertError> + Send + 'static,
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
            Ok(Ok(output)) => Ok(self.result(output)),
            Ok(Err(error)) => Ok(convert_error_result(error)),
            Err(join_err) => Err(McpError::internal_error(
                format!("tool task failed: {join_err}"),
                None,
            )),
        }
    }

    /// One representation per response, unless a client asked for both.
    ///
    /// `CallToolResult::structured` sends the same facts twice — escaped inside
    /// `content[0].text` and again as an object — and an agent pays for both.
    /// The text block is the one every client reads, so it is the one that is
    /// always sent.
    fn result(&self, output: ToolOutput) -> CallToolResult {
        let mut result = CallToolResult::success(vec![ContentBlock::text(output.text)]);
        if self.config.structured {
            result.structured_content = Some(output.value);
        }
        result
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DocsaiServer {
    /// The tool list, published in its cheapest equivalent form (E7).
    ///
    /// Overriding this is what stops `#[tool_handler]` from generating the
    /// default `list_all()` version: the schemas `schemars` derives are
    /// correct and verbose, and this is the one response a session pays for
    /// before it has done any work. See [`crate::schema::slim`] for what comes
    /// off and why nothing a caller reads does.
    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .tool_router
            .list_all()
            .into_iter()
            .map(|mut tool| {
                tool.input_schema = Arc::new(slim(&tool.input_schema));
                tool
            })
            .collect();
        Ok(ListToolsResult {
            result_type: Some(ResultType::COMPLETE),
            tools,
            meta: None,
            next_cursor: None,
            ttl_ms: None,
            cache_scope: None,
        })
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("docsai", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Bidirectional converter between Office/LibreOffice documents and DocMark. \
                 Documents belong on disk, not in a context window: convert_to_markdown with \
                 an `output_path` writes the DocMark and answers with a receipt, and \
                 convert_from_markdown with `markdown_path` + `path` converts it back — \
                 neither sends the document. What is in it comes from the addressing \
                 primitives, which accept the written `.dmk.md` as readily as the original: \
                 estimate_tokens for what it costs, outline_document for what is in it, \
                 search_document for where it says something, read_selection for just that \
                 part as self-contained DocMark. Images default to include_images=refs \
                 (names and sizes, no bytes). Also: inspect_document, \
                 list_supported_formats. Prefer path mode on local machines; use \
                 content_base64 + filename when the client cannot share a filesystem path. \
                 Logs always go to stderr; stdout is reserved for MCP JSON-RPC.",
            )
    }
}

fn convert_error_result(error: ConvertError) -> CallToolResult {
    // Tool-level errors so the client sees the message (rmcp CallToolResult::error contract).
    CallToolResult::error(vec![ContentBlock::text(error.to_string())])
}
