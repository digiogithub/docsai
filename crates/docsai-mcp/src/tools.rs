//! Pure tool implementations (sync), shared by the MCP handlers and unit tests.

use std::path::PathBuf;

use docsai_convert::{
    convert_from_markdown, convert_to_markdown, inspect_input, mime_type_for, parse_fidelity,
    validate_output_path, AssetMode, ConvertError, ConvertOptions, FormatSupport, Query,
    QueryError, Selector, SelectorError, SUPPORT,
};
use docsai_model::Format;
use serde::Serialize;
use serde_json::{json, Value};

use crate::config::McpConfig;
use crate::images::{image_payloads, ImagePolicy};
use crate::input::{
    decode_assets, encode_base64, resolve_document_input, IncomingAsset, ResolvedInput,
};

/// Runs `convert_to_markdown` and returns a JSON-shaped payload.
#[allow(clippy::too_many_arguments)]
pub fn tool_convert_to_markdown(
    path: Option<&str>,
    content_base64: Option<&str>,
    filename: Option<&str>,
    fidelity: Option<&str>,
    assets: Option<&str>,
    assets_dir: Option<&str>,
    include_images: Option<&str>,
    config: &McpConfig,
) -> Result<Value, ConvertError> {
    let input = resolve_document_input(path, content_base64, filename, config)?;
    let policy = image_policy(include_images, assets)?;
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
    let image_bytes: usize = result.assets.iter().map(|a| a.data.len()).sum();

    Ok(json!({
        "source_format": result.source_format.as_str(),
        "markdown": result.markdown,
        "include_images": policy.as_str(),
        "assets": image_payloads(&result.assets, policy),
        // Always reported, at every rung: "the response has no images" and
        // "the document has no images" are different facts.
        "image_count": result.assets.len(),
        "image_bytes": image_bytes,
        "assets_dir": result.assets_dir.map(|p| p.display().to_string()),
        "report": result.report,
    }))
}

/// Resolves the image payload rung from the new argument and the old one.
///
/// `include_images` (plan v2 Phase 11) replaces `assets` as the payload knob,
/// and its default is `refs` where the old default was the whole bytes — the
/// documented breaking change of this phase. An old client that *asked* for
/// `inline-base64` still gets exactly what it asked for: only the default
/// moved, so silence now means cheap instead of expensive.
fn image_policy(
    include_images: Option<&str>,
    assets: Option<&str>,
) -> Result<ImagePolicy, ConvertError> {
    match (include_images, assets) {
        (Some(value), _) => ImagePolicy::parse(value),
        (None, Some("inline-base64" | "inline")) => Ok(ImagePolicy::Full),
        (None, _) => Ok(ImagePolicy::default()),
    }
}

/// Runs `outline_document`: the map of a document, not the document.
pub fn tool_outline_document(
    path: Option<&str>,
    content_base64: Option<&str>,
    filename: Option<&str>,
    fidelity: Option<&str>,
    depth: Option<usize>,
    config: &McpConfig,
) -> Result<Value, ConvertError> {
    let input = resolve_document_input(path, content_base64, filename, config)?;
    let options = read_options(fidelity)?;
    let outline = docsai_convert::outline_input(input.as_source(), &options, depth)?;
    to_value(&outline)
}

/// Runs `read_selection`: part of a document, as self-contained DocMark.
pub fn tool_read_selection(
    path: Option<&str>,
    content_base64: Option<&str>,
    filename: Option<&str>,
    select: &str,
    fidelity: Option<&str>,
    config: &McpConfig,
) -> Result<Value, ConvertError> {
    let input = resolve_document_input(path, content_base64, filename, config)?;
    let options = read_options(fidelity)?;
    // The CLI refuses the levels that address nothing, and so does this: a
    // selection whose nodes have no ids cannot be written back, and handing one
    // over would be handing over a dead end.
    if !options.fidelity.addresses() {
        return Err(ConvertError::Invalid(format!(
            "fidelity `{}` does not address nodes, so there is nothing to select from; \
             node ids live at `full` and `agent`",
            options.fidelity
        )));
    }
    let selector: Selector = select
        .parse()
        .map_err(|e: SelectorError| ConvertError::Invalid(e.to_string()))?;
    let selection = docsai_convert::select_input(input.as_source(), &options, &selector)?;
    to_value(&selection)
}

/// Runs `search_document`: where a document says something, with context.
#[allow(clippy::too_many_arguments)]
pub fn tool_search_document(
    path: Option<&str>,
    content_base64: Option<&str>,
    filename: Option<&str>,
    query: &str,
    context: Option<usize>,
    limit: Option<usize>,
    fidelity: Option<&str>,
    config: &McpConfig,
) -> Result<Value, ConvertError> {
    let input = resolve_document_input(path, content_base64, filename, config)?;
    // Every level is allowed here, unlike `read_selection`: a level that writes
    // no id still writes text, and saying where the text is beats pretending
    // the document is empty.
    let options = read_options(fidelity)?;
    let mut query: Query = query
        .parse()
        .map_err(|e: QueryError| ConvertError::Invalid(e.to_string()))?;
    if let Some(context) = context {
        query.context = context;
    }
    if let Some(limit) = limit {
        query.limit = (limit > 0).then_some(limit);
    }
    let results = docsai_convert::search_input(input.as_source(), &options, &query)?;
    to_value(&results)
}

/// Options for the three read-only primitives: fidelity and nothing else.
fn read_options(fidelity: Option<&str>) -> Result<ConvertOptions, ConvertError> {
    Ok(ConvertOptions {
        fidelity: match fidelity {
            Some(value) => parse_fidelity(value)?,
            None => docsai_convert::Fidelity::Full,
        },
        ..Default::default()
    })
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, ConvertError> {
    serde_json::to_value(value).map_err(|e| ConvertError::Invalid(e.to_string()))
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

    fn to_markdown(images: Option<&str>, assets: Option<&str>) -> Value {
        tool_convert_to_markdown(
            Some(&corpus("images-inline.docx")),
            None,
            None,
            Some("full"),
            assets,
            None,
            images,
            &cfg(),
        )
        .expect("to markdown")
    }

    #[test]
    fn images_default_to_refs_and_the_markdown_never_changes() {
        let refs = to_markdown(None, None);
        assert_eq!(refs["include_images"], "refs");
        assert!(refs["image_count"].as_u64().unwrap() > 0);
        assert!(refs["image_bytes"].as_u64().unwrap() > 0);
        for row in refs["assets"].as_array().unwrap() {
            assert!(row.get("content_base64").is_none(), "refs carries no bytes");
            assert!(row["byte_len"].as_u64().unwrap() > 0);
        }

        // The rung is a payload choice, never a conversion: the same document
        // comes back whichever one is asked for.
        let full = to_markdown(Some("full"), None);
        assert_eq!(refs["markdown"], full["markdown"]);
        assert_eq!(refs["image_count"], full["image_count"]);
        assert!(!full["assets"][0]["content_base64"]
            .as_str()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn none_still_says_how_many_images_it_did_not_send() {
        let none = to_markdown(Some("none"), None);
        assert!(none["assets"].as_array().unwrap().is_empty());
        assert!(
            none["image_count"].as_u64().unwrap() > 0,
            "an empty list must not read as a document without images"
        );
        assert!(none["markdown"].as_str().unwrap().contains("assets/img-"));
    }

    #[test]
    fn a_client_that_asks_for_the_bytes_still_gets_the_bytes() {
        // Only the default moved (plan v2 Phase 11): an old client that named
        // `inline-base64` keeps working unchanged.
        let legacy = to_markdown(None, Some("inline-base64"));
        assert_eq!(legacy["include_images"], "full");
        assert!(!legacy["assets"][0]["content_base64"]
            .as_str()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_thumbnail_never_costs_more_than_the_image_it_stands_for() {
        // The corpus images are already smaller than a thumbnail box, which is
        // exactly the case where re-encoding would *cost* bytes: the invariant
        // has to hold there too, so it is stated as `<=` and the strict
        // reduction is measured on a big image in `images::tests`.
        let thumbs = to_markdown(Some("thumbnails"), None);
        let full = to_markdown(Some("full"), None);
        let weigh = |value: &Value| -> usize {
            value["assets"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|row| {
                    row.get("thumbnail_base64")
                        .or_else(|| row.get("content_base64"))
                        .and_then(Value::as_str)
                })
                .map(str::len)
                .sum()
        };
        assert!(weigh(&thumbs) > 0, "something has to be decodable");
        assert!(
            weigh(&thumbs) <= weigh(&full),
            "thumbnails {} bytes, originals {} bytes",
            weigh(&thumbs),
            weigh(&full)
        );
    }

    #[test]
    fn the_three_primitives_answer_about_the_same_document_over_bytes_and_over_a_path() {
        let path = corpus("long-report.docx");
        let bytes = std::fs::read(&path).expect("read");
        let base64 = crate::input::encode_base64(&bytes);

        let by_path = tool_outline_document(Some(&path), None, None, None, None, &cfg()).unwrap();
        let by_bytes = tool_outline_document(
            None,
            Some(&base64),
            Some("long-report.docx"),
            None,
            None,
            &cfg(),
        )
        .unwrap();
        assert_eq!(by_path["nodes"], by_bytes["nodes"]);
        assert!(
            by_path["outline-tokens"].as_u64().unwrap() * 10
                < by_path["document-tokens"].as_u64().unwrap()
        );

        let hits = tool_search_document(
            Some(&path),
            None,
            None,
            "conclusion",
            None,
            None,
            None,
            &cfg(),
        )
        .unwrap();
        assert!(hits["matches"].as_u64().unwrap() > 0);

        let first = by_path["nodes"][0]["id"].as_str().unwrap().to_string();
        let selection =
            tool_read_selection(Some(&path), None, None, &format!("#{first}"), None, &cfg())
                .unwrap();
        assert!(selection["docmark"].as_str().unwrap().contains(&first));
        assert!(
            selection["tokens"].as_u64().unwrap() < selection["document-tokens"].as_u64().unwrap()
        );
    }

    #[test]
    fn a_selection_at_a_level_that_addresses_nothing_is_refused_with_the_reason() {
        let err = tool_read_selection(
            Some(&corpus("basic-text.docx")),
            None,
            None,
            "s1",
            Some("plain"),
            &cfg(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("does not address nodes"), "{err}");
    }

    #[test]
    fn a_bad_selector_says_what_it_wanted() {
        let err = tool_read_selection(
            Some(&corpus("basic-text.docx")),
            None,
            None,
            "sausage",
            None,
            &cfg(),
        )
        .unwrap_err()
        .to_string();
        assert!(!err.is_empty());
    }
}
