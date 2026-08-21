//! Pure tool implementations (sync), shared by the MCP handlers and unit tests.
//!
//! Every tool returns a [`ToolOutput`]: the JSON a machine reads and the text a
//! model reads, built once. The server sends **one** of them (the text), and
//! the object only when a client asks for it — see [`crate::config::McpConfig`].

use std::path::{Path, PathBuf};

use docsai_convert::{
    convert_bytes, convert_file, convert_from_markdown, convert_to_markdown, inspect_input,
    mime_type_for, parse_fidelity, token_budget_input, validate_output_path, AssetBytes, AssetMode,
    ConvertError, ConvertOptions, FormatSupport, Outcome, Query, QueryError, Selector,
    SelectorError, SUPPORT,
};
use docsai_convert::{outline::OutlineNode, tokens::count};
use docsai_model::Format;
use rmcp::schemars;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::McpConfig;
use crate::images::{image_payloads, ImagePolicy};
use crate::input::{
    decode_assets, encode_base64, resolve_document_input, IncomingAsset, ResolvedInput,
};

/// What a tool produced, in the two forms a response can take.
///
/// The text is what crosses the wire by default. It is *not* always the JSON:
/// where a result has a compact reading form — an outline, a search, a
/// selection, a budget — that form is the cheaper one and the one an agent was
/// going to read anyway.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub value: Value,
    pub text: String,
}

impl ToolOutput {
    /// A result whose text form is its JSON.
    pub fn json(value: Value) -> Self {
        let text = value.to_string();
        ToolOutput { value, text }
    }

    /// A result with a text rendering of its own.
    pub fn rendered(value: Value, text: impl Into<String>) -> Self {
        ToolOutput {
            value,
            text: text.into(),
        }
    }
}

/// Arguments for `convert_to_markdown`.
///
/// The `path` / `content_base64` / `filename` triple repeats across the
/// read-side tools on purpose: every one of them takes a document, and the two
/// ways a client can supply one are the same everywhere. The prose is kept to
/// one short line each — `tools/list` pays for it once per session per tool
/// (E7), and the loop they belong to is told once, in the server instructions.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ConvertToMarkdownArgs {
    /// Source document path.
    #[serde(default)]
    pub path: Option<String>,
    /// Source bytes, base64; needs `filename`.
    #[serde(default)]
    pub content_base64: Option<String>,
    /// Name hint for `content_base64`, e.g. `report.docx`.
    #[serde(default)]
    pub filename: Option<String>,
    /// Where to write the DocMark. Preferred: the response becomes a receipt
    /// and the document stays out of the context window.
    #[serde(default)]
    pub output_path: Option<String>,
    /// `full` (default), `agent`, `standard`, `plain`.
    #[serde(default)]
    pub fidelity: Option<String>,
    /// `inline-base64` or `files`. Ignored with `output_path`.
    #[serde(default)]
    pub assets: Option<String>,
    /// Media directory. Defaults to `assets/` next to the output.
    #[serde(default)]
    pub assets_dir: Option<String>,
    /// Image payload: `none`, `refs` (default), `thumbnails`, `full`.
    #[serde(default)]
    pub include_images: Option<String>,
}

/// Arguments for `convert_from_markdown`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ConvertFromMarkdownArgs {
    /// DocMark text. Fragments only: it is paid for as input tokens.
    #[serde(default)]
    pub markdown: Option<String>,
    /// `.dmk.md` file to convert. Requires `path`; media resolved next to it.
    #[serde(default)]
    pub markdown_path: Option<String>,
    /// `docx`, `xlsx`, `odt`, `ods`, or `docmark`.
    pub target_format: String,
    /// Output path; without it the package comes back as base64.
    #[serde(default)]
    pub path: Option<String>,
    /// Media as `file_name` + `content_base64` rows.
    #[serde(default)]
    pub assets: Vec<IncomingAsset>,
    /// Directory to take the media from, instead of base64 rows.
    #[serde(default)]
    pub assets_dir: Option<String>,
}

/// Arguments for `outline_document`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct OutlineDocumentArgs {
    /// Source document path.
    #[serde(default)]
    pub path: Option<String>,
    /// Source bytes, base64; needs `filename`.
    #[serde(default)]
    pub content_base64: Option<String>,
    /// Name hint for `content_base64`.
    #[serde(default)]
    pub filename: Option<String>,
    /// Levels returned; 1 by default, `0` for every level.
    #[serde(default)]
    pub depth: Option<usize>,
    /// Node ceiling (default 200); the rest is `omitted` with a `next-cursor`.
    #[serde(default)]
    pub max_nodes: Option<usize>,
    /// Top-level node to start at, from a previous `next-cursor`.
    #[serde(default)]
    pub cursor: Option<usize>,
    /// Preview width in characters (default 60, its maximum).
    #[serde(default)]
    pub preview_chars: Option<usize>,
    /// `full` (default) or `agent`; lossy levels address nothing.
    #[serde(default)]
    pub fidelity: Option<String>,
}

/// Arguments for `read_selection`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct ReadSelectionArgs {
    /// Source document path.
    #[serde(default)]
    pub path: Option<String>,
    /// Source bytes, base64; needs `filename`.
    #[serde(default)]
    pub content_base64: Option<String>,
    /// Name hint for `content_base64`.
    #[serde(default)]
    pub filename: Option<String>,
    /// Comma-separated: `s4`, `s7-s9`, `#n7`, `type:heading`, `text:foo`.
    pub select: String,
    /// `full` (default) or `agent`.
    #[serde(default)]
    pub fidelity: Option<String>,
}

/// Arguments for `search_document`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchDocumentArgs {
    /// Source document path.
    #[serde(default)]
    pub path: Option<String>,
    /// Source bytes, base64; needs `filename`.
    #[serde(default)]
    pub content_base64: Option<String>,
    /// Name hint for `content_base64`.
    #[serde(default)]
    pub filename: Option<String>,
    /// Case-insensitive literal.
    pub query: String,
    /// Characters quoted either side of a match (default 48).
    #[serde(default)]
    pub context: Option<usize>,
    /// Blocks listed before the rest are only counted (default 20).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Any level; text is findable where ids are not.
    #[serde(default)]
    pub fidelity: Option<String>,
}

/// Arguments for `inspect_document` and `estimate_tokens`.
#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
pub struct DocumentArgs {
    /// Source document path.
    #[serde(default)]
    pub path: Option<String>,
    /// Source bytes, base64; needs `filename`.
    #[serde(default)]
    pub content_base64: Option<String>,
    /// Name hint for `content_base64`.
    #[serde(default)]
    pub filename: Option<String>,
}

/// Default nodes an outline response carries before it truncates itself.
const DEFAULT_MAX_NODES: usize = 200;
/// Default levels of the tree an outline response carries (E4): the map, not
/// the territory. `depth: 0` asks for every level.
const DEFAULT_DEPTH: usize = 1;

/// Runs `convert_to_markdown`.
///
/// Two shapes, and the argument decides which: with an `output_path` the
/// DocMark goes to disk and the response is a receipt; without one it comes
/// back inline, and only while it fits under `max_inline_tokens`.
pub fn tool_convert_to_markdown(
    args: &ConvertToMarkdownArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let input = resolve_document_input(
        args.path.as_deref(),
        args.content_base64.as_deref(),
        args.filename.as_deref(),
        config,
    )?;
    let fidelity = match args.fidelity.as_deref() {
        Some(value) => parse_fidelity(value)?,
        None => docsai_convert::Fidelity::Full,
    };
    let options = ConvertOptions {
        fidelity,
        assets_dir: args.assets_dir.as_deref().map(PathBuf::from),
        ..Default::default()
    };

    if let Some(output_path) = args.output_path.as_deref() {
        return convert_to_file(&input, output_path, options);
    }

    let policy = image_policy(args.include_images.as_deref(), args.assets.as_deref())?;
    let mode = asset_mode(args, &input)?;
    let result = convert_to_markdown(input.as_source(), &options, mode)?;
    let document_tokens = count(&result.markdown);
    if let Some(max) = config.max_inline_tokens {
        if document_tokens > max {
            return Err(ConvertError::Invalid(format!(
                "this document is {document_tokens} DocMark tokens, over the inline ceiling of \
                 {max} (DOCSAI_MCP_MAX_INLINE_TOKENS): pass `output_path` to write it to a file \
                 and read it back with outline_document / search_document / read_selection, or \
                 call estimate_tokens first to choose a cheaper fidelity"
            )));
        }
    }
    let image_bytes: usize = result.assets.iter().map(|a| a.data.len()).sum();

    Ok(ToolOutput::json(json!({
        "source_format": result.source_format.as_str(),
        "markdown": result.markdown,
        "document_tokens": document_tokens,
        "include_images": policy.as_str(),
        "assets": image_payloads(&result.assets, policy),
        // Always reported, at every rung: "the response has no images" and
        // "the document has no images" are different facts.
        "image_count": result.assets.len(),
        "image_bytes": image_bytes,
        "assets_dir": result.assets_dir.map(|p| p.display().to_string()),
        "report": result.report,
    })))
}

/// The `output_path` branch: write the DocMark, return what it cost.
fn convert_to_file(
    input: &ResolvedInput,
    output_path: &str,
    mut options: ConvertOptions,
) -> Result<ToolOutput, ConvertError> {
    let output = PathBuf::from(output_path);
    validate_output_path(&output)?;
    // Never inferred from the extension here: this tool writes DocMark, and a
    // caller who named `notes.txt` still gets DocMark rather than an error.
    options.target = Some(Format::DocMark);

    let outcome: Outcome = match input {
        ResolvedInput::Path(path) => convert_file(path, Some(&output), &options)?,
        ResolvedInput::Bytes { data, filename } => {
            convert_bytes(data, Some(filename.as_str()), Some(&output), &options)?
        }
    };
    let document_tokens = count(&outcome.markdown);
    let written = outcome
        .output_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| output.display().to_string());

    let value = json!({
        "source_format": outcome.source_format.as_str(),
        "output_path": written,
        "bytes_written": outcome.markdown.len(),
        "document_tokens": document_tokens,
        "fidelity": options.fidelity.as_str(),
        "assets_written": outcome.assets_written.len(),
        "assets_dir": assets_dir_of(&outcome),
        "report": outcome.report,
        "next": "outline_document / search_document / read_selection accept this path directly",
    });
    let text = format!(
        "wrote {written} ({} bytes, {document_tokens} tokens at {}, {} asset files)\n\
         next: outline_document, search_document or read_selection on that path\n",
        outcome.markdown.len(),
        options.fidelity.as_str(),
        outcome.assets_written.len(),
    );
    Ok(ToolOutput::rendered(value, text))
}

fn assets_dir_of(outcome: &Outcome) -> Value {
    match outcome.assets_written.first().and_then(|p| p.parent()) {
        Some(dir) => Value::String(dir.display().to_string()),
        None => Value::Null,
    }
}

/// The legacy `assets` knob: where media go when nothing is written to disk.
fn asset_mode(
    args: &ConvertToMarkdownArgs,
    input: &ResolvedInput,
) -> Result<AssetMode, ConvertError> {
    match args.assets.as_deref().unwrap_or("inline-base64") {
        "inline-base64" | "inline" => Ok(AssetMode::Inline),
        "files" => {
            let dir = args
                .assets_dir
                .as_deref()
                .map(PathBuf::from)
                .or_else(|| match input {
                    ResolvedInput::Path(p) => {
                        Some(p.parent().unwrap_or_else(|| Path::new(".")).join("assets"))
                    }
                    ResolvedInput::Bytes { .. } => None,
                });
            let Some(dir) = dir else {
                return Err(ConvertError::Invalid(
                    "assets=files requires assets_dir when the source is content_base64".into(),
                ));
            };
            validate_output_path(&dir)?;
            Ok(AssetMode::Files { dir: Some(dir) })
        }
        other => Err(ConvertError::Invalid(format!(
            "unknown assets mode `{other}`; expected inline-base64 or files"
        ))),
    }
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
    args: &OutlineDocumentArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let input = resolve_document_input(
        args.path.as_deref(),
        args.content_base64.as_deref(),
        args.filename.as_deref(),
        config,
    )?;
    let options = read_options(args.fidelity.as_deref())?;
    // `0` is how a caller says "every level"; absent is the cheap default.
    let depth = match args.depth.unwrap_or(DEFAULT_DEPTH) {
        0 => None,
        levels => Some(levels),
    };
    let mut outline = docsai_convert::outline_input(input.as_source(), &options, depth)?;

    if let Some(width) = args.preview_chars {
        truncate_previews(&mut outline.nodes, width);
    }
    let cursor = args.cursor.unwrap_or(0);
    let max_nodes = args.max_nodes.unwrap_or(DEFAULT_MAX_NODES).max(1);
    let (nodes, omitted, next_cursor) =
        paginate(std::mem::take(&mut outline.nodes), cursor, max_nodes);
    outline.nodes = nodes;
    // The number has to describe what was actually sent, not what was built.
    outline.outline_tokens = count(&outline.render_text());

    let ratio = if outline.document_tokens == 0 {
        0.0
    } else {
        outline.outline_tokens as f64 / outline.document_tokens as f64
    };
    let mut value = to_value(&outline)?;
    if let Some(object) = value.as_object_mut() {
        object.insert("omitted".into(), json!(omitted));
        object.insert(
            "outline-ratio".into(),
            json!((ratio * 1000.0).round() / 1000.0),
        );
        if let Some(next) = next_cursor {
            object.insert("next-cursor".into(), json!(next));
        }
    }

    let mut text = format!(
        "{} {} document-tokens={} outline-tokens={} ratio={:.3} nodes={}",
        outline.path.as_deref().unwrap_or("<bytes>"),
        outline.fidelity,
        outline.document_tokens,
        outline.outline_tokens,
        ratio,
        outline.len(),
    );
    if omitted > 0 {
        text.push_str(&format!(
            " omitted={omitted} next-cursor={}",
            next_cursor.unwrap_or(0)
        ));
    }
    text.push('\n');
    text.push_str(&outline.render_text());
    Ok(ToolOutput::rendered(value, text))
}

/// Keeps at most `max_nodes` nodes, counting descendants, from `cursor` on.
///
/// The unit of the window is a top-level node: half a section is not a map of
/// anything, and an agent continues from `next-cursor` rather than from the
/// middle of a subtree. At least one node is always returned — a window that
/// fits nothing would loop forever.
fn paginate(
    nodes: Vec<OutlineNode>,
    cursor: usize,
    max_nodes: usize,
) -> (Vec<OutlineNode>, usize, Option<usize>) {
    let total: usize = nodes.iter().map(OutlineNode::count).sum();
    if cursor == 0 && total <= max_nodes {
        return (nodes, 0, None);
    }
    let mut kept = Vec::new();
    let mut budget = max_nodes;
    let mut index = cursor.min(nodes.len());
    let skipped: usize = nodes[..index].iter().map(OutlineNode::count).sum();
    while index < nodes.len() {
        let size = nodes[index].count();
        if size > budget && !kept.is_empty() {
            break;
        }
        budget = budget.saturating_sub(size);
        index += 1;
        if budget == 0 {
            break;
        }
    }
    kept.extend(nodes[cursor.min(nodes.len())..index].iter().cloned());
    let shown: usize = kept.iter().map(OutlineNode::count).sum();
    let omitted = total - skipped - shown;
    let next = (index < nodes.len()).then_some(index);
    (kept, omitted, next)
}

fn truncate_previews(nodes: &mut [OutlineNode], width: usize) {
    for node in nodes {
        if node.preview.chars().count() > width {
            node.preview = node
                .preview
                .chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
                + "…";
        }
        truncate_previews(&mut node.children, width);
    }
}

/// Runs `read_selection`: part of a document, as self-contained DocMark.
pub fn tool_read_selection(
    args: &ReadSelectionArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let input = resolve_document_input(
        args.path.as_deref(),
        args.content_base64.as_deref(),
        args.filename.as_deref(),
        config,
    )?;
    let options = read_options(args.fidelity.as_deref())?;
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
    let selector: Selector = args
        .select
        .parse()
        .map_err(|e: SelectorError| ConvertError::Invalid(e.to_string()))?;
    let selection = docsai_convert::select_input(input.as_source(), &options, &selector)?;
    let mut text = format!(
        "{} tokens={} document-tokens={}\n",
        selection.selector, selection.tokens, selection.document_tokens
    );
    for node in &selection.nodes {
        text.push_str(&format!(
            "{} {} etag={}\n",
            node.id.0,
            node.kind.as_str(),
            node.etag
        ));
    }
    text.push_str(&selection.docmark);
    Ok(ToolOutput::rendered(to_value(&selection)?, text))
}

/// Runs `search_document`: where a document says something, with context.
pub fn tool_search_document(
    args: &SearchDocumentArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let input = resolve_document_input(
        args.path.as_deref(),
        args.content_base64.as_deref(),
        args.filename.as_deref(),
        config,
    )?;
    // Every level is allowed here, unlike `read_selection`: a level that writes
    // no id still writes text, and saying where the text is beats pretending
    // the document is empty.
    let options = read_options(args.fidelity.as_deref())?;
    let mut query: Query = args
        .query
        .parse()
        .map_err(|e: QueryError| ConvertError::Invalid(e.to_string()))?;
    if let Some(context) = args.context {
        query.context = context;
    }
    if let Some(limit) = args.limit {
        query.limit = (limit > 0).then_some(limit);
    }
    let results = docsai_convert::search_input(input.as_source(), &options, &query)?;
    let text = format!(
        "{} matches={} blocks={} omitted={} tokens={} document-tokens={}\n{}",
        results.query,
        results.matches,
        results.blocks,
        results.omitted,
        results.tokens,
        results.document_tokens,
        results.render_text(),
    );
    Ok(ToolOutput::rendered(to_value(&results)?, text))
}

/// Options for the read-only primitives: fidelity and nothing else.
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
///
/// The source is either `markdown` (a fragment, paid for as input tokens) or
/// `markdown_path` (a file, paid for once when it was written). The second is
/// the one an agent should use for a whole document, and it requires an output
/// `path`: returning a package as base64 would put the document back in the
/// context window by another door.
pub fn tool_convert_from_markdown(
    args: &ConvertFromMarkdownArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let target = Format::parse(&args.target_format).ok_or_else(|| {
        ConvertError::Invalid(format!(
            "unknown target_format `{}`; expected docx, xlsx, odt, ods, or docmark",
            args.target_format
        ))
    })?;
    let output = match args.path.as_deref() {
        Some(p) => {
            let p = PathBuf::from(p);
            validate_output_path(&p)?;
            Some(p)
        }
        None => None,
    };

    match (args.markdown.as_deref(), args.markdown_path.as_deref()) {
        (Some(_), Some(_)) => Err(ConvertError::Invalid(
            "provide either markdown or markdown_path, not both".into(),
        )),
        (None, None) => Err(ConvertError::Invalid(
            "provide markdown or markdown_path".into(),
        )),
        (None, Some(source)) => from_markdown_file(source, target, output.as_deref(), args, config),
        (Some(markdown), None) => {
            from_markdown_text(markdown, target, output.as_deref(), args, config)
        }
    }
}

fn from_markdown_file(
    source: &str,
    target: Format,
    output: Option<&Path>,
    args: &ConvertFromMarkdownArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let Some(output) = output else {
        return Err(ConvertError::Invalid(
            "markdown_path requires path: the point of converting from a file is that the \
             package goes to a file too"
                .into(),
        ));
    };
    let source = PathBuf::from(source);
    let meta = std::fs::metadata(&source).map_err(|e| ConvertError::Io {
        path: source.display().to_string(),
        source: e,
    })?;
    if meta.len() > config.max_input_bytes {
        return Err(ConvertError::Invalid(format!(
            "`{}` is {} bytes, which exceeds DOCSAI_MCP_MAX_INPUT_BYTES ({})",
            source.display(),
            meta.len(),
            config.max_input_bytes
        )));
    }

    let options = ConvertOptions {
        target: Some(target),
        assets_dir: args.assets_dir.as_deref().map(PathBuf::from),
        ..Default::default()
    };
    let outcome = convert_file(&source, Some(output), &options)?;
    let written = outcome
        .output_path
        .clone()
        .unwrap_or_else(|| output.to_path_buf());
    let byte_len = std::fs::metadata(&written).map(|m| m.len()).unwrap_or(0);

    let value = json!({
        "target_format": outcome.target_format.as_str(),
        "path": written.display().to_string(),
        "content_base64": Value::Null,
        "mime_type": mime_type_for(outcome.target_format),
        "byte_len": byte_len,
        "source_path": source.display().to_string(),
        "report": outcome.report,
    });
    let text = format!(
        "wrote {} ({byte_len} bytes, {}) from {}\n",
        written.display(),
        outcome.target_format.as_str(),
        source.display()
    );
    Ok(ToolOutput::rendered(value, text))
}

fn from_markdown_text(
    markdown: &str,
    target: Format,
    output: Option<&Path>,
    args: &ConvertFromMarkdownArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
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
    let mut decoded = decode_assets(&args.assets, config)?;
    if let Some(dir) = args.assets_dir.as_deref() {
        decoded.extend(read_asset_dir(Path::new(dir), config)?);
    }
    let result = convert_from_markdown(
        markdown,
        target,
        &decoded,
        output,
        &ConvertOptions::default(),
    )?;

    let value = json!({
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
    });
    Ok(ToolOutput::json(value))
}

/// Reads every regular file of `dir` as an asset for the reverse conversion.
fn read_asset_dir(dir: &Path, config: &McpConfig) -> Result<Vec<AssetBytes>, ConvertError> {
    let entries = std::fs::read_dir(dir).map_err(|source| ConvertError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let mut out = Vec::new();
    let mut total: u64 = 0;
    for entry in entries {
        let entry = entry.map_err(|source| ConvertError::Io {
            path: dir.display().to_string(),
            source,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let data = std::fs::read(&path).map_err(|source| ConvertError::Io {
            path: path.display().to_string(),
            source,
        })?;
        total += data.len() as u64;
        if total > config.max_input_bytes {
            return Err(ConvertError::Invalid(format!(
                "assets in `{}` exceed DOCSAI_MCP_MAX_INPUT_BYTES ({})",
                dir.display(),
                config.max_input_bytes
            )));
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        out.push(AssetBytes {
            content_type: crate::input::sniff_content_type(&data).to_string(),
            file_name,
            data,
        });
    }
    Ok(out)
}

/// Runs `inspect_document`.
pub fn tool_inspect_document(
    args: &DocumentArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let input = resolve_document_input(
        args.path.as_deref(),
        args.content_base64.as_deref(),
        args.filename.as_deref(),
        config,
    )?;
    let report = inspect_input(input.as_source(), &ConvertOptions::default())?;
    Ok(ToolOutput::json(to_value(&report)?))
}

/// Runs `estimate_tokens`: what the document costs at each fidelity level.
pub fn tool_estimate_tokens(
    args: &DocumentArgs,
    config: &McpConfig,
) -> Result<ToolOutput, ConvertError> {
    let input = resolve_document_input(
        args.path.as_deref(),
        args.content_base64.as_deref(),
        args.filename.as_deref(),
        config,
    )?;
    let budget = token_budget_input(input.as_source(), &ConvertOptions::default())?;
    let text = budget.render_text();
    Ok(ToolOutput::rendered(to_value(&budget)?, text))
}

/// Runs `list_supported_formats`.
pub fn tool_list_supported_formats() -> ToolOutput {
    let formats: Vec<FormatRow> = SUPPORT.iter().map(FormatRow::from).collect();
    ToolOutput::json(json!({ "formats": formats }))
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
            structured: false,
            // The unit tests convert whole corpus documents on purpose; the
            // ceiling is exercised by the test that asks for it.
            max_inline_tokens: None,
        }
    }

    fn corpus(name: &str) -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/docx")
            .join(name)
            .display()
            .to_string()
    }

    fn to_args(path: &str) -> ConvertToMarkdownArgs {
        ConvertToMarkdownArgs {
            path: Some(path.to_string()),
            fidelity: Some("full".into()),
            ..Default::default()
        }
    }

    fn doc_args(path: &str) -> DocumentArgs {
        DocumentArgs {
            path: Some(path.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn convert_to_and_from_markdown_end_to_end() {
        let to = tool_convert_to_markdown(
            &ConvertToMarkdownArgs {
                assets: Some("inline-base64".into()),
                ..to_args(&corpus("basic-text.docx"))
            },
            &cfg(),
        )
        .expect("to markdown")
        .value;
        assert_eq!(to["source_format"], "docx");
        assert!(to["markdown"].as_str().unwrap().contains("docmark"));

        let back = tool_convert_from_markdown(
            &ConvertFromMarkdownArgs {
                markdown: Some(to["markdown"].as_str().unwrap().to_string()),
                target_format: "docx".into(),
                ..Default::default()
            },
            &cfg(),
        )
        .expect("from markdown")
        .value;
        assert_eq!(back["target_format"], "docx");
        assert!(back["content_base64"].as_str().unwrap().len() > 8);
        assert!(back["path"].is_null());
    }

    #[test]
    fn an_output_path_returns_a_receipt_and_not_the_document() {
        let dir = tempfile::tempdir().expect("tempdir");
        let out = dir.path().join("report.dmk.md");
        let value = tool_convert_to_markdown(
            &ConvertToMarkdownArgs {
                output_path: Some(out.display().to_string()),
                ..to_args(&corpus("long-report.docx"))
            },
            &cfg(),
        )
        .expect("to file")
        .value;

        assert!(
            value.get("markdown").is_none(),
            "the document stays on disk"
        );
        assert_eq!(value["output_path"], out.display().to_string());
        assert!(value["document_tokens"].as_u64().unwrap() > 0);
        let written = std::fs::read_to_string(&out).expect("written");
        assert_eq!(
            value["bytes_written"].as_u64().unwrap() as usize,
            written.len()
        );
        assert!(written.contains("docmark"));
    }

    #[test]
    fn a_document_over_the_inline_ceiling_is_refused_with_the_fix() {
        let config = McpConfig {
            max_inline_tokens: Some(10),
            ..cfg()
        };
        let err = tool_convert_to_markdown(&to_args(&corpus("long-report.docx")), &config)
            .unwrap_err()
            .to_string();
        assert!(err.contains("output_path"), "{err}");
    }

    #[test]
    fn a_file_converts_back_to_a_package_without_passing_through_the_response() {
        let dir = tempfile::tempdir().expect("tempdir");
        let markdown = dir.path().join("report.dmk.md");
        tool_convert_to_markdown(
            &ConvertToMarkdownArgs {
                output_path: Some(markdown.display().to_string()),
                ..to_args(&corpus("images-inline.docx"))
            },
            &cfg(),
        )
        .expect("to file");

        let out = dir.path().join("again.docx");
        let value = tool_convert_from_markdown(
            &ConvertFromMarkdownArgs {
                markdown_path: Some(markdown.display().to_string()),
                target_format: "docx".into(),
                path: Some(out.display().to_string()),
                ..Default::default()
            },
            &cfg(),
        )
        .expect("from file")
        .value;
        assert!(value["content_base64"].is_null());
        assert!(value["byte_len"].as_u64().unwrap() > 0);
        assert!(out.exists());
    }

    #[test]
    fn a_file_conversion_without_an_output_path_says_why() {
        let err = tool_convert_from_markdown(
            &ConvertFromMarkdownArgs {
                markdown_path: Some("whatever.dmk.md".into()),
                target_format: "docx".into(),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("requires path"), "{err}");
    }

    #[test]
    fn inspect_and_list_formats() {
        let inspect = tool_inspect_document(&doc_args(&corpus("basic-text.docx")), &cfg())
            .unwrap()
            .value;
        assert_eq!(inspect["source-format"], "docx");
        assert_eq!(inspect["kind"], "text");

        let formats = tool_list_supported_formats().value;
        let arr = formats["formats"].as_array().unwrap();
        assert!(arr
            .iter()
            .any(|f| f["format"] == "docx" && f["read"] == true));
    }

    #[test]
    fn estimate_tokens_prices_every_level_in_one_call() {
        let budget = tool_estimate_tokens(&doc_args(&corpus("long-report.docx")), &cfg()).unwrap();
        let levels = budget.value["levels"].as_array().unwrap();
        assert_eq!(levels.len(), 4);
        let cost = |name: &str| -> u64 {
            levels.iter().find(|l| l["fidelity"] == name).unwrap()["total"]
                .as_u64()
                .unwrap()
        };
        assert!(cost("plain") < cost("full"), "the lossy levels are cheaper");
        assert!(
            budget.text.lines().count() >= 5,
            "one header and one line per level: {}",
            budget.text
        );
        // The point of the tool: it costs a fraction of what it prices.
        assert!(count(&budget.text) * 10 < cost("full") as usize);
    }

    #[test]
    fn malformed_base64_returns_error() {
        let err = tool_inspect_document(
            &DocumentArgs {
                content_base64: Some("%%%".into()),
                filename: Some("x.docx".into()),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("base64") || err.to_string().contains("invalid"));
    }

    #[test]
    fn unknown_target_format_is_rejected() {
        let err = tool_convert_from_markdown(
            &ConvertFromMarkdownArgs {
                markdown: Some("# hi\n".into()),
                target_format: "pdf".into(),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("target_format"));
    }

    fn to_markdown(images: Option<&str>, assets: Option<&str>) -> Value {
        tool_convert_to_markdown(
            &ConvertToMarkdownArgs {
                assets: assets.map(str::to_string),
                include_images: images.map(str::to_string),
                ..to_args(&corpus("images-inline.docx"))
            },
            &cfg(),
        )
        .expect("to markdown")
        .value
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

        let by_path = tool_outline_document(
            &OutlineDocumentArgs {
                path: Some(path.clone()),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap()
        .value;
        let by_bytes = tool_outline_document(
            &OutlineDocumentArgs {
                content_base64: Some(base64),
                filename: Some("long-report.docx".into()),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap()
        .value;
        assert_eq!(by_path["nodes"], by_bytes["nodes"]);
        assert!(
            by_path["outline-tokens"].as_u64().unwrap() * 10
                < by_path["document-tokens"].as_u64().unwrap()
        );

        let hits = tool_search_document(
            &SearchDocumentArgs {
                path: Some(path.clone()),
                query: "conclusion".into(),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap()
        .value;
        assert!(hits["matches"].as_u64().unwrap() > 0);

        let first = by_path["nodes"][0]["id"].as_str().unwrap().to_string();
        let selection = tool_read_selection(
            &ReadSelectionArgs {
                path: Some(path),
                select: format!("#{first}"),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap();
        assert!(selection.value["docmark"]
            .as_str()
            .unwrap()
            .contains(&first));
        assert!(
            selection.value["tokens"].as_u64().unwrap()
                < selection.value["document-tokens"].as_u64().unwrap()
        );
        assert!(selection.text.contains("etag="), "{}", selection.text);
    }

    #[test]
    fn an_outline_is_one_level_deep_until_more_is_asked_for() {
        let path = corpus("long-report.docx");
        let shallow = tool_outline_document(
            &OutlineDocumentArgs {
                path: Some(path.clone()),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap()
        .value;
        let whole = tool_outline_document(
            &OutlineDocumentArgs {
                path: Some(path),
                depth: Some(0),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap()
        .value;
        let tokens = |v: &Value| v["outline-tokens"].as_u64().unwrap();
        assert!(
            tokens(&shallow) <= tokens(&whole),
            "the default must not be the expensive one"
        );
        assert!(shallow["outline-ratio"].as_f64().unwrap() < 1.0);
        assert!(shallow["nodes"][0]["children"].is_null());
    }

    #[test]
    fn an_outline_over_the_node_budget_truncates_and_says_where_to_continue() {
        let value = tool_outline_document(
            &OutlineDocumentArgs {
                path: Some(corpus("long-report.docx")),
                depth: Some(0),
                max_nodes: Some(2),
                preview_chars: Some(12),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap()
        .value;
        assert!(value["omitted"].as_u64().unwrap() > 0);
        assert!(value["next-cursor"].as_u64().is_some());
        let preview = value["nodes"][0]["preview"].as_str().unwrap();
        assert!(preview.chars().count() <= 12, "{preview}");
    }

    #[test]
    fn a_selection_at_a_level_that_addresses_nothing_is_refused_with_the_reason() {
        let err = tool_read_selection(
            &ReadSelectionArgs {
                path: Some(corpus("basic-text.docx")),
                select: "s1".into(),
                fidelity: Some("plain".into()),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("does not address nodes"), "{err}");
    }

    #[test]
    fn a_bad_selector_says_what_it_wanted() {
        let err = tool_read_selection(
            &ReadSelectionArgs {
                path: Some(corpus("basic-text.docx")),
                select: "sausage".into(),
                ..Default::default()
            },
            &cfg(),
        )
        .unwrap_err()
        .to_string();
        assert!(!err.is_empty());
    }
}
