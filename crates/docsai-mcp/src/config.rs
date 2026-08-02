//! Runtime limits for the MCP server (architecture §6).

use std::time::Duration;

/// Environment variable: maximum accepted input size in bytes (path reads and base64).
pub const ENV_MAX_INPUT_BYTES: &str = "DOCSAI_MCP_MAX_INPUT_BYTES";
/// Environment variable: per-tool wall-clock timeout in seconds (`0` disables).
pub const ENV_TIMEOUT_SECS: &str = "DOCSAI_MCP_TIMEOUT_SECS";

/// Default maximum input size: 50 MiB.
pub const DEFAULT_MAX_INPUT_BYTES: u64 = 50 * 1024 * 1024;
/// Default per-tool timeout.
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Tunables loaded once at server start.
#[derive(Debug, Clone)]
pub struct McpConfig {
    pub max_input_bytes: u64,
    pub timeout: Option<Duration>,
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
        McpConfig {
            max_input_bytes,
            timeout,
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
        };
        assert!(cfg.max_input_bytes >= 1024 * 1024);
        assert!(cfg.timeout.unwrap().as_secs() >= 1);
    }
}
