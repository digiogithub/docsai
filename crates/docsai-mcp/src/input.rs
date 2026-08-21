//! Shared input resolution for path / base64 tool arguments.

use std::path::{Path, PathBuf};

use base64::Engine;
use docsai_convert::{AssetBytes, ConvertError, SourceInput};
use rmcp::schemars;
use serde::Deserialize;

use crate::config::McpConfig;

/// Decoded tool input ready for the convert layer.
#[derive(Debug)]
pub enum ResolvedInput {
    Path(PathBuf),
    Bytes { data: Vec<u8>, filename: String },
}

impl ResolvedInput {
    pub fn as_source(&self) -> SourceInput<'_> {
        match self {
            ResolvedInput::Path(path) => SourceInput::Path(path.as_path()),
            ResolvedInput::Bytes { data, filename } => SourceInput::Bytes {
                data: data.as_slice(),
                filename: filename.as_str(),
            },
        }
    }
}

/// Resolves the dual path / base64 input contract (architecture §6).
pub fn resolve_document_input(
    path: Option<&str>,
    content_base64: Option<&str>,
    filename: Option<&str>,
    config: &McpConfig,
) -> Result<ResolvedInput, ConvertError> {
    match (path, content_base64) {
        (Some(path), None) => {
            let path = PathBuf::from(path);
            enforce_path_readable(&path, config)?;
            Ok(ResolvedInput::Path(path))
        }
        (None, Some(b64)) => {
            let filename = filename
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    ConvertError::Invalid(
                        "filename is required when content_base64 is set (used as a format hint)"
                            .into(),
                    )
                })?
                .to_string();
            if filename.contains('\0') || Path::new(&filename).is_absolute() {
                return Err(ConvertError::Invalid(
                    "filename must be a plain file name, not an absolute path".into(),
                ));
            }
            if Path::new(&filename)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(ConvertError::Invalid(
                    "filename must not contain '..' path segments".into(),
                ));
            }
            let data = decode_base64(b64, config)?;
            Ok(ResolvedInput::Bytes { data, filename })
        }
        (Some(_), Some(_)) => Err(ConvertError::Invalid(
            "provide either path or content_base64, not both".into(),
        )),
        (None, None) => Err(ConvertError::Invalid(
            "provide path or content_base64 (+ filename)".into(),
        )),
    }
}

/// Decodes a base64 string, enforcing the configured size limit on the result.
pub fn decode_base64(value: &str, config: &McpConfig) -> Result<Vec<u8>, ConvertError> {
    // Reject obviously oversized base64 before decoding (4/3 expansion).
    let approx = (value.len() as u64).saturating_mul(3) / 4;
    if approx > config.max_input_bytes.saturating_add(4096) {
        return Err(ConvertError::Invalid(format!(
            "content_base64 exceeds DOCSAI_MCP_MAX_INPUT_BYTES ({})",
            config.max_input_bytes
        )));
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|e| ConvertError::Invalid(format!("invalid base64: {e}")))?;
    if data.len() as u64 > config.max_input_bytes {
        return Err(ConvertError::Invalid(format!(
            "decoded content is {} bytes, which exceeds DOCSAI_MCP_MAX_INPUT_BYTES ({})",
            data.len(),
            config.max_input_bytes
        )));
    }
    Ok(data)
}

/// Encodes bytes as standard base64.
pub fn encode_base64(data: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Decodes optional asset payloads for `convert_from_markdown`.
pub fn decode_assets(
    assets: &[IncomingAsset],
    config: &McpConfig,
) -> Result<Vec<AssetBytes>, ConvertError> {
    let mut out = Vec::with_capacity(assets.len());
    for asset in assets {
        let file_name = asset.file_name.trim();
        if file_name.is_empty() {
            return Err(ConvertError::Invalid(
                "asset file_name must not be empty".into(),
            ));
        }
        if Path::new(file_name).is_absolute()
            || Path::new(file_name)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(ConvertError::Invalid(format!(
                "asset file_name `{file_name}` must be a plain name without path traversal"
            )));
        }
        let data = decode_base64(&asset.content_base64, config)?;
        let content_type = asset
            .content_type
            .clone()
            .unwrap_or_else(|| sniff_content_type(&data).to_string());
        out.push(AssetBytes {
            file_name: file_name.to_string(),
            content_type,
            data,
        });
    }
    Ok(out)
}

/// One media file supplied inline.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct IncomingAsset {
    /// Name as referenced from DocMark, e.g. `img-<hash>.png`.
    pub file_name: String,
    /// Standard base64 of the bytes.
    pub content_base64: String,
    /// MIME type; sniffed when omitted.
    #[serde(default)]
    pub content_type: Option<String>,
}

fn enforce_path_readable(path: &Path, config: &McpConfig) -> Result<(), ConvertError> {
    if path.as_os_str().is_empty() {
        return Err(ConvertError::Invalid("path is empty".into()));
    }
    if path.to_string_lossy().chars().any(|c| c == '\0') {
        return Err(ConvertError::Invalid(
            "path contains an interior NUL byte".into(),
        ));
    }
    let meta = std::fs::metadata(path).map_err(|source| ConvertError::Io {
        path: path.display().to_string(),
        source,
    })?;
    if !meta.is_file() {
        return Err(ConvertError::Invalid(format!(
            "`{}` is not a regular file",
            path.display()
        )));
    }
    let len = meta.len();
    if len > config.max_input_bytes {
        return Err(ConvertError::Invalid(format!(
            "file `{}` is {len} bytes, which exceeds DOCSAI_MCP_MAX_INPUT_BYTES ({})",
            path.display(),
            config.max_input_bytes
        )));
    }
    Ok(())
}

pub(crate) fn sniff_content_type(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']) {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpConfig;

    fn cfg() -> McpConfig {
        McpConfig {
            max_input_bytes: 1024,
            timeout: None,
            structured: false,
            max_inline_tokens: None,
        }
    }

    #[test]
    fn rejects_both_path_and_base64() {
        let err = resolve_document_input(Some("a.docx"), Some("YQ=="), None, &cfg()).unwrap_err();
        assert!(err.to_string().contains("either path or content_base64"));
    }

    #[test]
    fn rejects_base64_without_filename() {
        let err = resolve_document_input(None, Some("YQ=="), None, &cfg()).unwrap_err();
        assert!(err.to_string().contains("filename is required"));
    }

    #[test]
    fn decodes_valid_base64() {
        let data = decode_base64("aGVsbG8=", &cfg()).unwrap();
        assert_eq!(data, b"hello");
    }

    #[test]
    fn rejects_oversize_decoded_payload() {
        let big = encode_base64(&vec![0u8; 2048]);
        let err = decode_base64(&big, &cfg()).unwrap_err();
        assert!(err.to_string().contains("exceeds"));
    }
}
