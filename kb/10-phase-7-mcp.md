# 10 — Phase 7: MCP server over stdio

## Status

**Core path implemented.** `docsai mcp` starts an MCP server (official `rmcp` SDK,
stdio transport) exposing the four tools from `architecture.md` §6. Plan v2 Phase 11 added
three more — `outline_document`, `search_document`, `read_selection` — and changed the image
default of `convert_to_markdown`; see [28-phase-11-mcp.md](28-phase-11-mcp.md).

## Delivered

| Item | Where |
|---|---|
| `docsai mcp` subcommand | `docsai-cli` → `docsai_mcp::run()` |
| Four tools with JSON schemas | `docsai-mcp::server` (`#[tool]` / `#[tool_router]`) |
| Path + base64 dual input | `docsai-mcp::input` |
| Inline-base64 vs files assets | `docsai-convert::service::{AssetMode, convert_to_markdown}` |
| In-memory reverse conversion | `docsai-convert::service::convert_from_markdown` |
| Size limit + timeout | `DOCSAI_MCP_MAX_INPUT_BYTES`, `DOCSAI_MCP_TIMEOUT_SECS` |
| stderr-only logging | `docsai_mcp::run` / CLI already used stderr for tracing |
| Duplex protocol tests | `crates/docsai-mcp/tests/stdio_protocol.rs` |

## Tools

| Tool | Notes |
|---|---|
| `convert_to_markdown` | `path` **or** `content_base64`+`filename`; `fidelity`; `assets=inline-base64\|files` |
| `convert_from_markdown` | `markdown`, `target_format`, optional `path`, optional `assets[]` |
| `inspect_document` | Same JSON shape as `docsai inspect --json` (`kebab-case` fields) |
| `list_supported_formats` | Support matrix from `docsai_convert::SUPPORT` |

## Limits

- `DOCSAI_MCP_MAX_INPUT_BYTES` — default 50 MiB (path metadata and decoded base64).
- `DOCSAI_MCP_TIMEOUT_SECS` — default 120; `0` disables the wall-clock timeout.
  Conversion runs on `spawn_blocking` so the async runtime stays responsive.

## Client recipe

```json
{
  "mcpServers": {
    "docsai": { "command": "docsai", "args": ["mcp"] }
  }
}
```

## Acceptance criteria checklist

- [x] Real MCP client (in-process duplex + `rmcp` client) converts docx→markdown and back.
- [x] Malformed inputs return tool-level MCP errors; server stays alive.
- [x] Session of many consecutive conversions succeeds without hang (25 in CI; design is stateless).
- [ ] Interactive verification with MCP Inspector / Claude Desktop (manual; recipe in README).

## Out of scope / follow-ups

- `apply_edits` guided editing tool (plan backlog).
- Streamable HTTP transport (stdio only for v1).
- Phase 8 path-hardening review of MCP write targets.
