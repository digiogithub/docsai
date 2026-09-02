//! Runtime limits for the MCP server (architecture §6).

use std::sync::Arc;
use std::time::Duration;

use docsai_convert::ConvertCache;

/// Environment variable: maximum accepted input size in bytes (path reads and base64).
pub const ENV_MAX_INPUT_BYTES: &str = "DOCSAI_MCP_MAX_INPUT_BYTES";
/// Environment variable: per-tool wall-clock timeout in seconds (`0` disables).
pub const ENV_TIMEOUT_SECS: &str = "DOCSAI_MCP_TIMEOUT_SECS";
/// Environment variable: also send `structuredContent` next to the text block.
pub const ENV_STRUCTURED: &str = "DOCSAI_MCP_STRUCTURED";
/// Environment variable: largest DocMark a response will carry inline, in
/// tokens (`0` disables the cap).
pub const ENV_MAX_INLINE_TOKENS: &str = "DOCSAI_MCP_MAX_INLINE_TOKENS";
/// Environment variable: how many bytes of parsed documents the read cache may
/// hold (`0` turns the cache off).
pub const ENV_CACHE_BYTES: &str = "DOCSAI_MCP_CACHE_BYTES";

/// Default maximum input size: 50 MiB.
pub const DEFAULT_MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;
/// Default per-tool timeout.
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// Default inline ceiling for `convert_to_markdown`: past this the caller is
/// told to pass an `output_path` instead of being handed a document it did not
/// budget for.
pub const DEFAULT_MAX_INLINE_TOKENS: usize = 2_000;

/// Tunables loaded once at server start.
#[derive(Debug, Clone)]
pub struct McpConfig {
    pub max_input_bytes: u64,
    pub timeout: Option<Duration>,
    /// Whether a response repeats itself as `structuredContent`.
    ///
    /// Off by default since the E1 pass: `CallToolResult::structured` sends the
    /// same facts twice — once escaped inside `content[0].text`, once as an
    /// object — and an agent pays for both. Clients that consume the object
    /// set `DOCSAI_MCP_STRUCTURED=1` and get the old shape back.
    pub structured: bool,
    /// Ceiling on inline DocMark, in tokens; `None` means no ceiling.
    pub max_inline_tokens: Option<usize>,
    /// The read cache shared by every tool call this server serves
    /// (Phase 16-H), or `None` when the operator turned it off with
    /// `DOCSAI_MCP_CACHE_BYTES=0`.
    ///
    /// The server is the process the cache exists for: an agent outlines a
    /// document, searches it and then reads a selection from it, and without
    /// this those are three reads of the same bytes. It changes no answer —
    /// only how long the second and third take.
    pub cache: Option<Arc<ConvertCache>>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl McpConfig {
    /// Reads limits from the environment, falling back to defaults.
    pub fn from_env() -> Self {
        let max_input_bytes = std::env::var(ENV_MAX_INPUT_BYTES)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_INPUT_BYTES);
        let timeout = match std::env::var(ENV_TIMEOUT_SECS)
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            Some(0) => None,
            Some(secs) => Some(Duration::from_secs(secs)),
            None => Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        };
        let structured = std::env::var(ENV_STRUCTURED)
            .ok()
            .map(|v| matches!(v.trim(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let max_inline_tokens = match std::env::var(ENV_MAX_INLINE_TOKENS)
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
        {
            Some(0) => None,
            Some(tokens) => Some(tokens),
            None => Some(DEFAULT_MAX_INLINE_TOKENS),
        };
        let cache = match std::env::var(ENV_CACHE_BYTES)
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
        {
            Some(0) => None,
            Some(bytes) => Some(Arc::new(ConvertCache::with_capacity(
                bytes,
                docsai_convert::DEFAULT_MAX_ENTRIES,
            ))),
            None => Some(ConvertCache::shared()),
        };
        McpConfig {
            max_input_bytes,
            timeout,
            structured,
            max_inline_tokens,
            cache,
        }
    }

    /// Base conversion options for this server: the read cache and nothing
    /// else. Every tool that reads a document starts from this.
    pub fn convert_options(&self) -> docsai_convert::ConvertOptions {
        docsai_convert::ConvertOptions {
            cache: self.cache.clone(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let cfg = McpConfig {
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            timeout: Some(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
            structured: false,
            max_inline_tokens: Some(DEFAULT_MAX_INLINE_TOKENS),
            cache: Some(ConvertCache::shared()),
        };
        assert!(cfg.max_input_bytes >= 1024 * 1024);
        assert!(cfg.timeout.unwrap().as_secs() >= 1);
        assert!(
            !cfg.structured,
            "one representation per response by default"
        );
        assert!(cfg.max_inline_tokens.unwrap() > 0);
        assert!(cfg.convert_options().cache.is_some());
    }

    #[test]
    fn the_cache_is_on_by_default() {
        let cfg = McpConfig::from_env();
        // Only meaningful when the environment has not turned it off.
        if std::env::var(ENV_CACHE_BYTES).is_err() {
            assert!(cfg.cache.is_some());
            assert_eq!(
                cfg.cache.as_ref().unwrap().capacity_bytes(),
                docsai_convert::DEFAULT_CAPACITY_BYTES
            );
        }
    }
}
