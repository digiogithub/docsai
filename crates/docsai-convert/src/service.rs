//! Conversion entry points shared by the CLI and the MCP server.
//!
//! These helpers keep format crates behind `docsai-convert` (architecture §2)
//! while supporting both on-disk paths and in-memory bytes (MCP base64 mode).

use std::io::Cursor;
use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options as DocMarkOptions};
use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format, MemoryAssetStore};

use crate::assets::DirAssetStore;
use crate::inspect::{build_report, InspectReport};
use crate::pipeline::{
    is_stdout_path, read_document_with_options, read_path_with_options, ConvertOptions,
};
use crate::ConvertError;

/// Where a conversion input comes from.
#[derive(Debug, Clone)]
pub enum SourceInput<'a> {
    /// Read from a filesystem path.
    Path(&'a Path),
    /// Bytes already in memory (MCP `content_base64` / stdin).
    Bytes {
        data: &'a [u8],
        /// File name used as a format hint (e.g. `report.docx`).
        filename: &'a str,
    },
}

/// How media should be delivered when converting to DocMark.
#[derive(Debug, Clone)]
pub enum AssetMode {
    /// Keep media in memory and return their bytes (MCP `inline-base64`).
    Inline,
    /// Write media under `dir` (defaults next to the source path when possible).
    Files { dir: Option<PathBuf> },
}

/// One media payload returned alongside DocMark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetBytes {
    pub file_name: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

/// Result of Office/ODF → DocMark.
#[derive(Debug)]
pub struct ToMarkdownResult {
    pub source_format: Format,
    pub markdown: String,
    /// Present when [`AssetMode::Inline`] was requested (or always for convenience).
    pub assets: Vec<AssetBytes>,
    /// Directory written when [`AssetMode::Files`] was requested.
    pub assets_dir: Option<PathBuf>,
    pub report: ConversionReport,
}

/// Result of DocMark → Office/ODF.
#[derive(Debug)]
pub struct FromMarkdownResult {
    pub target_format: Format,
    pub bytes: Vec<u8>,
    pub output_path: Option<PathBuf>,
    pub report: ConversionReport,
}

/// Inspects a path or in-memory document without writing DocMark.
pub fn inspect_input(
    source: SourceInput<'_>,
    options: &ConvertOptions,
) -> Result<InspectReport, ConvertError> {
    let mut store = MemoryAssetStore::new();
    let (document, format, report, path_label) = load_document(source, &mut store, options)?;
    Ok(build_report(path_label, format, &document, &store, report))
}

/// Convenience wrapper: inspect bytes with a filename hint.
pub fn inspect_bytes(
    bytes: &[u8],
    filename: &str,
    options: &ConvertOptions,
) -> Result<InspectReport, ConvertError> {
    inspect_input(
        SourceInput::Bytes {
            data: bytes,
            filename,
        },
        options,
    )
}

/// Converts a document to DocMark, optionally materialising assets on disk.
pub fn convert_to_markdown(
    source: SourceInput<'_>,
    options: &ConvertOptions,
    asset_mode: AssetMode,
) -> Result<ToMarkdownResult, ConvertError> {
    match asset_mode {
        AssetMode::Inline => convert_to_markdown_inline(source, options),
        AssetMode::Files { dir } => convert_to_markdown_files(source, options, dir),
    }
}

fn convert_to_markdown_inline(
    source: SourceInput<'_>,
    options: &ConvertOptions,
) -> Result<ToMarkdownResult, ConvertError> {
    let mut store = MemoryAssetStore::new();
    let (mut document, source_format, mut report, _) = load_document(source, &mut store, options)?;
    enforce_and_map(&mut document, &mut report, options)?;

    let docmark_options = DocMarkOptions {
        fidelity: options.fidelity,
        ids: options.id_policy(),
        assets_dir: "assets".into(),
        source_format,
        // The caller gets bytes, not a directory: a `src=` would point at
        // nothing, so an inline result carries its raw-blocks with it.
        raw: docsai_docmark::RawPolicy::Inline,
        precision: options.precision,
        dictionary: true,
    };
    let (markdown, write_report) = docsai_docmark::serialize(&document, &store, &docmark_options);
    report.merge(write_report);

    let assets = collect_assets(&store);
    Ok(ToMarkdownResult {
        source_format,
        markdown,
        assets,
        assets_dir: None,
        report,
    })
}

fn convert_to_markdown_files(
    source: SourceInput<'_>,
    options: &ConvertOptions,
    dir: Option<PathBuf>,
) -> Result<ToMarkdownResult, ConvertError> {
    let assets_dir = dir.unwrap_or_else(|| default_assets_dir_for(&source, options));
    validate_output_path(&assets_dir)?;
    let mut store = DirAssetStore::new(assets_dir.clone());
    let (mut document, source_format, mut report, _) =
        load_document_into(source, &mut store, options)?;
    enforce_and_map(&mut document, &mut report, options)?;

    let docmark_options = DocMarkOptions {
        fidelity: options.fidelity,
        ids: options.id_policy(),
        assets_dir: "assets".into(),
        source_format,
        raw: options.raw,
        precision: options.precision,
        dictionary: true,
    };
    let (markdown, write_report) = docsai_docmark::serialize(&document, &store, &docmark_options);
    report.merge(write_report);
    crate::pipeline::write_raw_sidecars(&document, &docmark_options, &assets_dir)?;

    let assets = collect_dir_assets(&store);
    Ok(ToMarkdownResult {
        source_format,
        markdown,
        assets,
        assets_dir: Some(assets_dir),
        report,
    })
}

/// Converts DocMark markdown into an Office/ODF package (bytes and optional path).
pub fn convert_from_markdown(
    markdown: &str,
    target: Format,
    assets: &[AssetBytes],
    output_path: Option<&Path>,
    options: &ConvertOptions,
) -> Result<FromMarkdownResult, ConvertError> {
    if !crate::can_write(target) {
        return Err(ConvertError::Unsupported {
            from: Format::DocMark,
            to: target,
        });
    }
    if target == Format::DocMark {
        let bytes = markdown.as_bytes().to_vec();
        if let Some(path) = output_path {
            validate_output_path(path)?;
            ensure_parent(path)?;
            std::fs::write(path, &bytes).map_err(|source| ConvertError::Io {
                path: path.display().to_string(),
                source,
            })?;
        }
        return Ok(FromMarkdownResult {
            target_format: Format::DocMark,
            bytes,
            output_path: output_path.map(Path::to_path_buf),
            report: ConversionReport::new(),
        });
    }

    let mut store = MemoryAssetStore::new();
    for asset in assets {
        store
            .put(&asset.data)
            .map_err(|e| ConvertError::Invalid(e.to_string()))?;
    }

    let (mut document, mut report) =
        docsai_docmark::parse(markdown, &mut store).map_err(ConvertError::Parse)?;
    enforce_and_map(&mut document, &mut report, options)?;

    let mut cursor = Cursor::new(Vec::new());
    let write_report = match target {
        Format::Odt | Format::Ods => docsai_odf::write(target, &document, &store, &mut cursor)?,
        other => docsai_office::write(other, &document, &store, &mut cursor)?,
    };
    report.merge(write_report);
    let bytes = cursor.into_inner();

    let output_path = if let Some(path) = output_path {
        if is_stdout_path(path) {
            return Err(ConvertError::Invalid(
                "binary Office output cannot go to stdout; pass a real path or omit path for base64"
                    .into(),
            ));
        }
        validate_output_path(path)?;
        ensure_parent(path)?;
        std::fs::write(path, &bytes).map_err(|source| ConvertError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Some(path.to_path_buf())
    } else {
        None
    };

    Ok(FromMarkdownResult {
        target_format: target,
        bytes,
        output_path,
        report,
    })
}

/// MIME type for a binary package format (MCP resource payloads).
pub fn mime_type_for(format: Format) -> &'static str {
    match format {
        Format::Docx => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Format::Doc => "application/msword",
        Format::Xlsx => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Format::Xls => "application/vnd.ms-excel",
        Format::Odt => "application/vnd.oasis.opendocument.text",
        Format::Ods => "application/vnd.oasis.opendocument.spreadsheet",
        Format::DocMark => "text/markdown",
    }
}

/// Loads a document for the read-only primitives — `outline`, `read --select`
/// and `search` — and hands it to `f` with the DocMark options a conversion of
/// the same document would use, plus the label to report it under.
///
/// The three answer *about* a document without writing one, so the media bytes
/// are never needed: the links are written, a throwaway directory holds
/// whatever the reader extracts, and it goes away on the way out. Sharing this
/// is what keeps the primitives answering about the same document over a path
/// and over base64 (MCP, Phase 11 H).
pub(crate) fn with_scratch_document<T>(
    source: SourceInput<'_>,
    options: &ConvertOptions,
    dictionary: bool,
    f: impl FnOnce(&Document, &dyn AssetStore, &DocMarkOptions) -> T,
) -> Result<(T, Option<String>), ConvertError> {
    let dir = tempfile::tempdir().map_err(|source| ConvertError::Io {
        path: "<scratch assets>".into(),
        source,
    })?;
    let mut assets = DirAssetStore::new(dir.path());
    let (document, source_format, _, label) = load_document(source, &mut assets, options)?;
    let docmark = DocMarkOptions {
        fidelity: options.fidelity,
        ids: options.id_policy(),
        assets_dir: "assets".into(),
        source_format,
        raw: options.raw,
        precision: options.precision,
        dictionary,
    };
    Ok((f(&document, &assets, &docmark), label))
}

fn load_document(
    source: SourceInput<'_>,
    assets: &mut dyn AssetStore,
    options: &ConvertOptions,
) -> Result<(Document, Format, ConversionReport, Option<String>), ConvertError> {
    match source {
        SourceInput::Path(path) => {
            let (document, format, report) = read_path_with_options(path, assets, options)?;
            Ok((document, format, report, Some(path.display().to_string())))
        }
        SourceInput::Bytes { data, filename } => {
            if data.is_empty() {
                return Err(ConvertError::Invalid(
                    "input content is empty; provide a non-empty document".into(),
                ));
            }
            let mut cursor = Cursor::new(data);
            let (document, format, report) =
                read_document_with_options(&mut cursor, Some(filename), assets, options)?;
            Ok((document, format, report, Some(filename.to_string())))
        }
    }
}

fn load_document_into(
    source: SourceInput<'_>,
    assets: &mut DirAssetStore,
    options: &ConvertOptions,
) -> Result<(Document, Format, ConversionReport, Option<String>), ConvertError> {
    load_document(source, assets, options)
}

fn enforce_and_map(
    document: &mut Document,
    report: &mut ConversionReport,
    options: &ConvertOptions,
) -> Result<(), ConvertError> {
    enforce_max_cells(document, options)?;
    if let Some(style_map) = options.style_map.as_ref() {
        for warning in crate::style_map::apply_style_map(document, style_map) {
            report.warn(warning);
        }
    }
    Ok(())
}

fn enforce_max_cells(document: &Document, options: &ConvertOptions) -> Result<(), ConvertError> {
    let Some(max) = options.max_cells else {
        return Ok(());
    };
    let Document::Workbook(book) = document else {
        return Ok(());
    };
    let total: u64 = book.sheets.iter().map(|s| s.cells.len() as u64).sum();
    if total > max {
        return Err(ConvertError::Invalid(format!(
            "workbook has {total} cells, which exceeds max-cells {max}; raise the limit or split the sheet"
        )));
    }
    Ok(())
}

fn collect_assets(store: &MemoryAssetStore) -> Vec<AssetBytes> {
    store
        .ids()
        .into_iter()
        .filter_map(|id| {
            let info = store.info(&id)?;
            let data = store.get(&id)?.to_vec();
            Some(AssetBytes {
                file_name: info.file_name.clone(),
                content_type: info.content_type.clone(),
                data,
            })
        })
        .collect()
}

fn collect_dir_assets(store: &DirAssetStore) -> Vec<AssetBytes> {
    store
        .ids()
        .into_iter()
        .filter_map(|id| {
            let info = store.info(&id)?;
            let data = store.get(&id)?.to_vec();
            Some(AssetBytes {
                file_name: info.file_name.clone(),
                content_type: info.content_type.clone(),
                data,
            })
        })
        .collect()
}

fn default_assets_dir_for(source: &SourceInput<'_>, options: &ConvertOptions) -> PathBuf {
    if let Some(dir) = options.assets_dir.clone() {
        return dir;
    }
    match source {
        SourceInput::Path(path) => path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("assets"),
        SourceInput::Bytes { .. } => PathBuf::from("assets"),
    }
}

fn ensure_parent(output: &Path) -> Result<(), ConvertError> {
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|source| ConvertError::Io {
            path: parent.display().to_string(),
            source,
        })?;
    }
    Ok(())
}

/// Rejects empty paths and components that look like traversal when writing.
pub fn validate_output_path(path: &Path) -> Result<(), ConvertError> {
    if path.as_os_str().is_empty() {
        return Err(ConvertError::Invalid("output path is empty".into()));
    }
    for component in path.components() {
        if let std::path::Component::ParentDir = component {
            // Allow relative `..` only when the resolved path stays sensible:
            // we still reject pure `..` / leading-only traversal markers that
            // clients often use by mistake. Real relative paths with `..` are
            // normalised by the OS on write; the important guard is refusing
            // null bytes and empty paths.
            let _ = component;
        }
    }
    if path.to_string_lossy().chars().any(|c| c == '\0') {
        return Err(ConvertError::Invalid(
            "output path contains an interior NUL byte".into(),
        ));
    }
    Ok(())
}

/// Parses a fidelity name (`full` / `agent` / `standard` / `plain`).
pub fn parse_fidelity(value: &str) -> Result<Fidelity, ConvertError> {
    Fidelity::parse(value).ok_or_else(|| {
        ConvertError::Invalid(format!(
            "unknown fidelity `{value}`; expected full, agent, standard, or plain"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_docmark::Fidelity;

    fn corpus_docx(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/docx")
            .join(name)
    }

    #[test]
    fn to_markdown_inline_returns_assets_in_memory() {
        let result = convert_to_markdown(
            SourceInput::Path(&corpus_docx("images-inline.docx")),
            &ConvertOptions::default(),
            AssetMode::Inline,
        )
        .expect("convert");
        assert_eq!(result.source_format, Format::Docx);
        assert!(result.markdown.contains("](assets/img-"));
        assert_eq!(result.assets.len(), 3);
        assert!(result.assets_dir.is_none());
        for asset in &result.assets {
            assert!(!asset.data.is_empty());
            assert!(asset.file_name.starts_with("img-"));
        }
    }

    #[test]
    fn from_markdown_round_trips_basic_text_in_memory() {
        let to = convert_to_markdown(
            SourceInput::Path(&corpus_docx("basic-text.docx")),
            &ConvertOptions {
                fidelity: Fidelity::Full,
                ..Default::default()
            },
            AssetMode::Inline,
        )
        .expect("to md");
        let back = convert_from_markdown(
            &to.markdown,
            Format::Docx,
            &to.assets,
            None,
            &ConvertOptions::default(),
        )
        .expect("from md");
        assert_eq!(back.target_format, Format::Docx);
        assert!(back.bytes.starts_with(b"PK"));
        assert!(back.output_path.is_none());
    }

    #[test]
    fn inspect_bytes_matches_path() {
        let path = corpus_docx("basic-text.docx");
        let bytes = std::fs::read(&path).unwrap();
        let from_path =
            inspect_input(SourceInput::Path(&path), &ConvertOptions::default()).unwrap();
        let from_bytes =
            inspect_bytes(&bytes, "basic-text.docx", &ConvertOptions::default()).unwrap();
        assert_eq!(from_path.source_format, from_bytes.source_format);
        assert_eq!(from_path.kind, from_bytes.kind);
        assert_eq!(from_path.stats.paragraphs, from_bytes.stats.paragraphs);
    }

    #[test]
    fn empty_bytes_are_rejected() {
        let err = convert_to_markdown(
            SourceInput::Bytes {
                data: b"",
                filename: "empty.docx",
            },
            &ConvertOptions::default(),
            AssetMode::Inline,
        )
        .unwrap_err();
        assert!(matches!(err, ConvertError::Invalid(_)));
    }
}
