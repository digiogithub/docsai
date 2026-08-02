//! Pure tool implementations (sync), shared by the MCP handlers and unit tests.

use std::path::PathBuf;

use docsai_convert::{
    convert_from_markdown, convert_to_markdown, inspect_input, mime_type_for, parse_fidelity,
    validate_output_path, AssetMode, ConvertError, ConvertOptions, FormatSupport, SUPPORT,
};
use docsai_model::Format;
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::McpConfig;
use crate::input::{
    decode_assets, encode_base64, resolve_document_input, IncomingAsset, ResolvedInput,
};

/// Runs `convert_to_markdown` and returns a JSON-shaped payload.
pub fn tool_convert_to_markdown(
    path: Option<&str>,
    content_base64: Option<&str>,
    filename: Option<&str>,
    fidelity: Option<&str>,
    assets: Option<&str>,
    assets_dir: Option<&str>,
    config: &McpConfig,
) -> Result<Value, ConvertError> {
    let input = resolve_document_input(path, content_base64, filename, config)?;
    let fidelity = match fidelity {
        Some(value) => parse_fidelity(value)?,
        None => docsai_convert::Fidelity::Full,
    };
    let options = ConvertOptions {
        fidelity,
        assets_dir: assets_dir.map(PathBuf::from),
        ..Default::default()
    };
    let mode = match assets.unwrap_or("inline-base64") {
        "inline-base64" | "inline" => AssetMode::Inline,
        "files" => {
            let dir = assets_dir.map(PathBuf::from).or_else(|| match &input {
                ResolvedInput::Path(p) => Some(
                    p.parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .join("assets"),
                ),
                ResolvedInput::Bytes { .. } => None,
            });
            if dir.is_none() {
                return Err(ConvertError::Invalid(
                    "assets=files requires assets_dir when the source is content_base64".into(),
                ));
            }
            if let Some(ref d) = dir {
                validate_output_path(d)?;
            }
            AssetMode::Files { dir }
        }
        other => {
            return Err(ConvertError::Invalid(format!(
                "unknown assets mode `{other}`; expected inline-base64 or files"
            )))
        }
    };

    let result = convert_to_markdown(input.as_source(), &options, mode)?;
    let assets_json: Vec<Value> = result
        .assets
        .iter()
        .map(|a| {
            json!({
                "file_name": a.file_name,
                "content_type": a.content_type,
                "content_base64": encode_base64(&a.data),
                "byte_len": a.data.len(),
            })
        })
        .collect();

    Ok(json!({
        "source_format": result.source_format.as_str(),
        "markdown": result.markdown,
        "assets": assets_json,
        "assets_dir": result.assets_dir.map(|p| p.display().to_string()),
        "report": result.report,
    }))
}

/// Runs `convert_from_markdown`.
pub fn tool_convert_from_markdown(
    markdown: &str,
    target_format: &str,
    path: Option<&str>,
    assets: &[IncomingAsset],
    config: &McpConfig,
) -> Result<Value, ConvertError> {
    if markdown.is_empty() {
        return Err(ConvertError::Invalid("markdown must not be empty".into()));
    }
    if markdown.len() as u64 > config.max_input_bytes {
        return Err(ConvertError::Invalid(format!(
            "markdown is {} bytes, which exceeds DOCSAI_MCP_MAX_INPUT_BYTES ({})",
            markdown.len(),
            config.max_input_bytes
        )));
    }
    let target = Format::parse(target_format).ok_or_else(|| {
        ConvertError::Invalid(format!(
            "unknown target_format `{target_format}`; expected docx, xlsx, odt, ods, or docmark"
        ))
    })?;
    let decoded = decode_assets(assets, config)?;
    let output = match path {
        Some(p) => {
            let p = PathBuf::from(p);
            validate_output_path(&p)?;
            Some(p)
        }
        None => None,
    };
    let result = convert_from_markdown(
        markdown,
        target,
        &decoded,
        output.as_deref(),
        &ConvertOptions::default(),
    )?;

    Ok(json!({
        "target_format": result.target_format.as_str(),
        "path": result.output_path.as_ref().map(|p| p.display().to_string()),
        "content_base64": if result.output_path.is_some() {
            Value::Null
        } else {
            Value::String(encode_base64(&result.bytes))
        },
        "mime_type": mime_type_for(result.target_format),
        "byte_len": result.bytes.len(),
        "report": result.report,
    }))
}

/// Runs `inspect_document`.
pub fn tool_inspect_document(
    path: Option<&str>,
    content_base64: Option<&str>,
    filename: Option<&str>,
    config: &McpConfig,
) -> Result<Value, ConvertError> {
    let input = resolve_document_input(path, content_base64, filename, config)?;
    let report = inspect_input(input.as_source(), &ConvertOptions::default())?;
    serde_json::to_value(&report).map_err(|e| ConvertError::Invalid(e.to_string()))
}

/// Runs `list_supported_formats`.
pub fn tool_list_supported_formats() -> Value {
    let formats: Vec<FormatRow> = SUPPORT.iter().map(FormatRow::from).collect();
    json!({ "formats": formats })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
struct FormatRow {
    format: String,
    read: bool,
    write: bool,
    note: String,
}

impl From<&FormatSupport> for FormatRow {
    fn from(value: &FormatSupport) -> Self {
        FormatRow {
            format: value.format.as_str().to_string(),
            read: value.read,
            write: value.write,
            note: value.note.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpConfig;
    use std::path::Path;

    fn cfg() -> McpConfig {
        McpConfig {
            max_input_bytes: 20 * 1024 * 1024,
            timeout: None,
        }
    }

    fn corpus(name: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/docx")
            .join(name)
            .display()
            .to_string()
    }

    #[test]
    fn convert_to_and_from_markdown_end_to_end() {
        let to = tool_convert_to_markdown(
            Some(&corpus("basic-text.docx")),
            None,
            None,
            Some("full"),
            Some("inline-base64"),
            None,
            &cfg(),
        )
        .expect("to markdown");
        assert_eq!(to["source_format"], "docx");
        assert!(to["markdown"].as_str().unwrap().contains("docmark"));

        let markdown = to["markdown"].as_str().unwrap();
        let back =
            tool_convert_from_markdown(markdown, "docx", None, &[], &cfg()).expect("from markdown");
        assert_eq!(back["target_format"], "docx");
        assert!(back["content_base64"].as_str().unwrap().len() > 8);
        assert!(back["path"].is_null());
    }

    #[test]
    fn inspect_and_list_formats() {
        let inspect =
            tool_inspect_document(Some(&corpus("basic-text.docx")), None, None, &cfg()).unwrap();
        assert_eq!(inspect["source-format"], "docx");
        assert_eq!(inspect["kind"], "text");

        let formats = tool_list_supported_formats();
        let arr = formats["formats"].as_array().unwrap();
        assert!(arr
            .iter()
            .any(|f| f["format"] == "docx" && f["read"] == true));
    }

    #[test]
    fn malformed_base64_returns_error() {
        let err = tool_inspect_document(None, Some("%%%"), Some("x.docx"), &cfg()).unwrap_err();
        assert!(err.to_string().contains("base64") || err.to_string().contains("invalid"));
    }

    #[test]
    fn unknown_target_format_is_rejected() {
        let err = tool_convert_from_markdown("# hi\n", "pdf", None, &[], &cfg()).unwrap_err();
        assert!(err.to_string().contains("target_format"));
    }
}
