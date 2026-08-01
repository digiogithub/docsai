//! MCP server over stdio.
//!
//! Scheduled for **Fase 7** of `docs/plan-desarrollo.md`. It is a library, not
//! a binary, so that `docsai mcp` runs inside the single distributed
//! executable (arquitectura §2).
//!
//! One rule already applies to anything added here: **stdout carries the
//! JSON-RPC protocol and nothing else**; every log goes to stderr.

#![forbid(unsafe_code)]

/// Names of the tools the server will expose (arquitectura §6).
pub const TOOLS: &[&str] = &[
    "convert_to_markdown",
    "convert_from_markdown",
    "inspect_document",
    "list_supported_formats",
];

#[cfg(test)]
mod tests {
    #[test]
    fn the_v1_tool_set_is_four_tools() {
        assert_eq!(super::TOOLS.len(), 4);
    }
}
