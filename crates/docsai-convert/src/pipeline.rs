//! The conversion pipelines.

use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options as DocMarkOptions, RawPolicy};
use docsai_model::addressing::IdPolicy;
use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format, MemoryAssetStore};

use crate::assets::DirAssetStore;
use crate::ConvertError;

/// Settings for one conversion.
#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub fidelity: Fidelity,
    /// Where media go. Defaults to `assets/` next to the output file.
    pub assets_dir: Option<PathBuf>,
    /// Target format. Inferred from the output extension when absent.
    pub target: Option<Format>,
    /// LibreOffice headless policy for legacy formats (Phase 5).
    pub use_loffice: crate::UseLoffice,
    /// Optional publication-mode style map (DocMark spec §5).
    pub style_map: Option<crate::style_map::StyleMap>,
    /// Safety cap on workbook cells; `None` means unlimited.
    pub max_cells: Option<u64>,
    /// Where raw-block bytes go (spec §7). `sidecar` keeps the document
    /// readable: the body says what is there, the bytes wait in a file.
    pub raw: RawPolicy,
    /// What happens to node ids (DocMark 1.1, spec §11.1). `None` takes the
    /// per-fidelity default: `assign` at `full`, `never` otherwise, because
    /// the lossy levels are meant to stay readable.
    pub ids: Option<IdPolicy>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            fidelity: Fidelity::Full,
            assets_dir: None,
            target: None,
            use_loffice: crate::UseLoffice::Auto,
            style_map: None,
            max_cells: None,
            raw: RawPolicy::default(),
            ids: None,
        }
    }
}

impl ConvertOptions {
    /// The id policy this conversion applies.
    pub fn id_policy(&self) -> IdPolicy {
        self.ids.unwrap_or(match self.fidelity {
            Fidelity::Full => IdPolicy::Assign,
            _ => IdPolicy::Never,
        })
    }
}

/// What a conversion produced.
#[derive(Debug)]
pub struct Outcome {
    pub source_format: Format,
    pub target_format: Format,
    /// The DocMark text when the target (or intermediate) is DocMark.
    pub markdown: String,
    pub output_path: Option<PathBuf>,
    pub assets_written: Vec<PathBuf>,
    pub report: ConversionReport,
}

/// Result of a round-trip fidelity check.
#[derive(Debug)]
pub struct RoundtripOutcome {
    pub source_format: Format,
    pub first_markdown: String,
    pub second_markdown: String,
    pub identical: bool,
    pub report: ConversionReport,
    pub output_path: Option<PathBuf>,
}

/// Reads any supported document into the IR, detecting its format by content.
pub fn read_document<R: Read + Seek>(
    reader: R,
    hint: Option<&str>,
    assets: &mut dyn AssetStore,
) -> Result<(Document, Format, ConversionReport), ConvertError> {
    let mut reader = reader;
    let (format, score) = docsai_office::detect(&mut reader, hint);
    if score == docsai_office::DetectScore::No || !crate::can_read(format) {
        return Err(match crate::SUPPORT.iter().find(|s| s.format == format) {
            Some(support) if score != docsai_office::DetectScore::No => ConvertError::Unsupported {
                from: support.format,
                to: Format::DocMark,
            },
            _ => ConvertError::UnknownFormat(hint.unwrap_or("<input>").to_string()),
        });
    }
    let (document, report) = match format {
        Format::DocMark => {
            let mut bytes = Vec::new();
            reader
                .read_to_end(&mut bytes)
                .map_err(|source| ConvertError::Io {
                    path: hint.unwrap_or("<input>").to_string(),
                    source,
                })?;
            let text = String::from_utf8_lossy(&bytes);
            let base = hint.and_then(|h| {
                let p = Path::new(h);
                if p.is_absolute() || p.components().count() > 1 {
                    p.parent()
                } else {
                    None
                }
            });
            let (document, report) = docsai_docmark::parse_with_base(&text, base, assets)
                .map_err(ConvertError::Parse)?;
            (document, report)
        }
        Format::Odt | Format::Ods => docsai_odf::read(format, reader, assets)?,
        other => docsai_office::read(other, reader, assets)?,
    };
    docsai_model::validate::validate(&document).map_err(|errors| {
        ConvertError::Invalid(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    Ok((document, format, report))
}

/// Reads a document from a path on disk.
pub fn read_path(
    input: &Path,
    assets: &mut dyn AssetStore,
) -> Result<(Document, Format, ConversionReport), ConvertError> {
    read_path_with_options(input, assets, &ConvertOptions::default())
}

/// Reads a document from a path, honouring LibreOffice policy in `options`.
pub fn read_path_with_options(
    input: &Path,
    assets: &mut dyn AssetStore,
    options: &ConvertOptions,
) -> Result<(Document, Format, ConversionReport), ConvertError> {
    let hint = input.file_name().and_then(|n| n.to_str());
    let (format, score) = {
        let mut probe = File::open(input).map_err(|source| ConvertError::Io {
            path: input.display().to_string(),
            source,
        })?;
        docsai_office::detect(&mut probe, hint)
    };
    if score == docsai_office::DetectScore::No || !crate::can_read(format) {
        return Err(match crate::SUPPORT.iter().find(|s| s.format == format) {
            Some(support) if score != docsai_office::DetectScore::No => ConvertError::Unsupported {
                from: support.format,
                to: Format::DocMark,
            },
            _ => ConvertError::UnknownFormat(input.display().to_string()),
        });
    }
    if format == Format::DocMark {
        let text = std::fs::read_to_string(input).map_err(|source| ConvertError::Io {
            path: input.display().to_string(),
            source,
        })?;
        let base = input.parent();
        let (document, report) =
            docsai_docmark::parse_with_base(&text, base, assets).map_err(ConvertError::Parse)?;
        docsai_model::validate::validate(&document).map_err(|errors| {
            ConvertError::Invalid(
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })?;
        return Ok((document, format, report));
    }

    // Phase 5: optional LibreOffice pre-conversion for legacy .doc.
    if crate::loffice::benefits_from_loffice(format) {
        if let Some(result) = try_loffice_doc(input, format, assets, options)? {
            return Ok(result);
        }
    }

    let file = File::open(input).map_err(|source| ConvertError::Io {
        path: input.display().to_string(),
        source,
    })?;
    let (document, report) = match format {
        Format::Odt | Format::Ods => docsai_odf::read(format, file, assets)?,
        other => docsai_office::read(other, file, assets)?,
    };
    docsai_model::validate::validate(&document).map_err(|errors| {
        ConvertError::Invalid(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    Ok((document, format, report))
}

/// Attempts LibreOffice `.doc` → `.docx` → IR. Returns `Ok(None)` when the
/// policy says to skip or LO is unavailable under `Auto`.
fn try_loffice_doc(
    input: &Path,
    source_format: Format,
    assets: &mut dyn AssetStore,
    options: &ConvertOptions,
) -> Result<Option<(Document, Format, ConversionReport)>, ConvertError> {
    let soffice = crate::loffice::find_soffice();
    let use_it = crate::loffice::should_use(options.use_loffice, soffice.is_some())?;
    if !use_it {
        if options.use_loffice == crate::UseLoffice::Auto {
            tracing::info!(
                "LibreOffice not found; reading {} with the native degraded .doc path. \
                 Install LibreOffice or set DOCSAI_LIBREOFFICE for full fidelity.",
                input.display()
            );
        }
        return Ok(None);
    }
    let soffice = soffice.ok_or_else(|| ConvertError::Loffice {
        message: "internal: soffice missing after should_use".into(),
    })?;

    let tmp = tempfile::tempdir().map_err(|source| ConvertError::Io {
        path: "tempdir".into(),
        source,
    })?;
    let docx_path = crate::loffice::convert_to_docx(&soffice, input, tmp.path())?;
    let file = File::open(&docx_path).map_err(|source| ConvertError::Io {
        path: docx_path.display().to_string(),
        source,
    })?;
    let (document, mut report) = docsai_office::read_docx(file, assets)?;
    report.warn(docsai_model::Warning::Degraded {
        what: "doc-via-libreoffice".into(),
        why: format!(
            "legacy .doc pre-converted to docx with `{}` before the docx pipeline",
            soffice.display()
        ),
    });
    docsai_model::validate::validate(&document).map_err(|errors| {
        ConvertError::Invalid(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    // Keep the logical source format as Doc so reports/front matter stay honest.
    let _ = source_format;
    Ok(Some((document, Format::Doc, report)))
}

/// Converts a file on disk.
///
/// Supports Office → DocMark, DocMark → Office, and DocMark → DocMark.
/// Pass `input` as `-` to read from stdin (content-based detection).
pub fn convert_file(
    input: &Path,
    output: Option<&Path>,
    options: &ConvertOptions,
) -> Result<Outcome, ConvertError> {
    let target = resolve_target(output, options)?;
    if !crate::can_write(target) {
        return Err(ConvertError::Unsupported {
            from: Format::DocMark,
            to: target,
        });
    }

    let assets_dir = options
        .assets_dir
        .clone()
        .unwrap_or_else(|| default_assets_dir(input, output));
    let mut store = DirAssetStore::new(assets_dir.clone());

    let (mut document, source_format, mut report) = if is_stdin_path(input) {
        read_stdin(&mut store, options)?
    } else {
        read_path_with_options(input, &mut store, options)?
    };
    if !crate::can_read(source_format) {
        return Err(ConvertError::Unsupported {
            from: source_format,
            to: target,
        });
    }

    enforce_max_cells(&document, options)?;
    if let Some(style_map) = options.style_map.as_ref() {
        for warning in crate::style_map::apply_style_map(&mut document, style_map) {
            report.warn(warning);
        }
    }

    write_document(
        document,
        source_format,
        target,
        output,
        options,
        &assets_dir,
        &mut store,
        report,
    )
}

/// Converts bytes already in memory (stdin / MCP path).
pub fn convert_bytes(
    bytes: &[u8],
    hint: Option<&str>,
    output: Option<&Path>,
    options: &ConvertOptions,
) -> Result<Outcome, ConvertError> {
    let target = resolve_target(output, options)?;
    if !crate::can_write(target) {
        return Err(ConvertError::Unsupported {
            from: Format::DocMark,
            to: target,
        });
    }

    let assets_dir = options
        .assets_dir
        .clone()
        .unwrap_or_else(|| default_assets_dir(Path::new(hint.unwrap_or("stdin")), output));
    let mut store = DirAssetStore::new(assets_dir.clone());
    let mut cursor = Cursor::new(bytes);
    let (mut document, source_format, mut report) =
        read_document_with_options(&mut cursor, hint, &mut store, options)?;
    enforce_max_cells(&document, options)?;
    if let Some(style_map) = options.style_map.as_ref() {
        for warning in crate::style_map::apply_style_map(&mut document, style_map) {
            report.warn(warning);
        }
    }
    write_document(
        document,
        source_format,
        target,
        output,
        options,
        &assets_dir,
        &mut store,
        report,
    )
}

#[allow(clippy::too_many_arguments)]
fn write_document(
    document: Document,
    source_format: Format,
    target: Format,
    output: Option<&Path>,
    options: &ConvertOptions,
    assets_dir: &Path,
    store: &mut DirAssetStore,
    mut report: ConversionReport,
) -> Result<Outcome, ConvertError> {
    // `-` as output means stdout (caller prints `markdown`); do not create a file.
    let file_output = output.filter(|p| !is_stdout_path(p));

    match target {
        Format::DocMark => {
            let docmark_options = DocMarkOptions {
                fidelity: options.fidelity,
                ids: options.id_policy(),
                assets_dir: relative_assets_dir(assets_dir, file_output),
                source_format,
                raw: options.raw,
            };
            let (markdown, write_report) =
                docsai_docmark::serialize(&document, store, &docmark_options);
            report.merge(write_report);
            let sidecars = write_raw_sidecars(&document, &docmark_options, assets_dir)?;

            if let Some(output) = file_output {
                ensure_parent(output)?;
                std::fs::write(output, markdown.as_bytes()).map_err(|source| ConvertError::Io {
                    path: output.display().to_string(),
                    source,
                })?;
            }

            let mut assets_written = store.written().to_vec();
            assets_written.extend(sidecars);
            Ok(Outcome {
                source_format,
                target_format: target,
                markdown,
                output_path: file_output.map(Path::to_path_buf),
                assets_written,
                report,
            })
        }
        Format::Docx | Format::Xlsx | Format::Odt | Format::Ods => {
            let ext = target.as_str();
            let output = file_output.ok_or_else(|| {
                ConvertError::Invalid(format!(
                    "writing .{ext} requires a real --output path (stdout is text-only; binary formats cannot go to a terminal)"
                ))
            })?;
            ensure_parent(output)?;
            let file = File::create(output).map_err(|source| ConvertError::Io {
                path: output.display().to_string(),
                source,
            })?;
            let write_report = match target {
                Format::Odt | Format::Ods => docsai_odf::write(target, &document, store, file)?,
                other => docsai_office::write(other, &document, store, file)?,
            };
            report.merge(write_report);

            let (markdown, _) = docsai_docmark::serialize(
                &document,
                store,
                &DocMarkOptions {
                    fidelity: options.fidelity,
                    ids: options.id_policy(),
                    assets_dir: relative_assets_dir(assets_dir, Some(output)),
                    source_format,
                    // This DocMark is the caller's view of what was written, not
                    // a file on disk: a `src=` here would point at nothing.
                    raw: RawPolicy::Inline,
                },
            );

            Ok(Outcome {
                source_format,
                target_format: target,
                markdown,
                output_path: Some(output.to_path_buf()),
                assets_written: store.written().to_vec(),
                report,
            })
        }
        other => Err(ConvertError::Unsupported {
            from: source_format,
            to: other,
        }),
    }
}

/// True when the path is the stdin sentinel `-`.
pub fn is_stdin_path(path: &Path) -> bool {
    path.as_os_str() == "-"
}

/// True when the path is the stdout sentinel `-`.
pub fn is_stdout_path(path: &Path) -> bool {
    path.as_os_str() == "-"
}

fn read_stdin(
    assets: &mut dyn AssetStore,
    options: &ConvertOptions,
) -> Result<(Document, Format, ConversionReport), ConvertError> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|source| ConvertError::Io {
            path: "<stdin>".into(),
            source,
        })?;
    if bytes.is_empty() {
        return Err(ConvertError::Invalid(
            "stdin is empty; pass a file path or pipe a document into `docsai convert -`".into(),
        ));
    }
    let mut cursor = Cursor::new(bytes);
    read_document_with_options(&mut cursor, Some("stdin"), assets, options)
}

/// Like [`read_document`], but honours LibreOffice policy and cell caps prep.
pub fn read_document_with_options<R: Read + Seek>(
    reader: R,
    hint: Option<&str>,
    assets: &mut dyn AssetStore,
    options: &ConvertOptions,
) -> Result<(Document, Format, ConversionReport), ConvertError> {
    let _ = options; // LO needs a path today; bytes path stays native.
    read_document(reader, hint, assets)
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
            "workbook has {total} cells, which exceeds --max-cells {max}; raise the limit or split the sheet"
        )));
    }
    Ok(())
}

/// Office → DocMark → Office → DocMark idempotence check.
///
/// Writes the regenerated Office file to `output` when given, and returns the
/// second DocMark pass in `markdown`.
pub fn roundtrip_file(
    input: &Path,
    output: Option<&Path>,
    options: &ConvertOptions,
) -> Result<RoundtripOutcome, ConvertError> {
    let assets_dir = options
        .assets_dir
        .clone()
        .unwrap_or_else(|| default_assets_dir(input, output));

    let mut store1 = MemoryAssetStore::new();
    let (document1, source_format, mut report) =
        read_path_with_options(input, &mut store1, options)?;
    let office_format = match source_format {
        Format::Docx | Format::Xlsx | Format::Odt | Format::Ods => source_format,
        // Legacy .doc round-trips through docx (write path for .doc is out of scope).
        Format::Doc => Format::Docx,
        Format::DocMark => match &document1 {
            Document::Workbook(_) => Format::Xlsx,
            Document::Text(_) => Format::Docx,
        },
        other => {
            return Err(ConvertError::Unsupported {
                from: other,
                to: Format::DocMark,
            })
        }
    };

    let (md1, r1) = docsai_docmark::serialize(
        &document1,
        &store1,
        &DocMarkOptions {
            fidelity: Fidelity::Full,
            ids: IdPolicy::Assign,
            assets_dir: "assets".into(),
            source_format,
            // The round trip happens in memory, so it has to be self-contained:
            // there is no directory for a sidecar to live in.
            raw: RawPolicy::Inline,
        },
    );
    report.merge(r1);

    let mut store2 = MemoryAssetStore::new();
    for id in store1.ids() {
        if let Some(bytes) = store1.get(&id) {
            let _ = store2.put(bytes);
        }
    }
    let (document2, r2) = docsai_docmark::parse(&md1, &mut store2).map_err(ConvertError::Parse)?;
    report.merge(r2);

    let mut office_buf = Cursor::new(Vec::new());
    let r3 = match office_format {
        Format::Odt | Format::Ods => {
            docsai_odf::write(office_format, &document2, &store2, &mut office_buf)?
        }
        other => docsai_office::write(other, &document2, &store2, &mut office_buf)?,
    };
    report.merge(r3);

    if let Some(output) = output {
        ensure_parent(output)?;
        std::fs::write(output, office_buf.get_ref()).map_err(|source| ConvertError::Io {
            path: output.display().to_string(),
            source,
        })?;
    }

    office_buf.set_position(0);
    let mut store3 = MemoryAssetStore::new();
    let (document3, r4) = match office_format {
        Format::Odt | Format::Ods => docsai_odf::read(office_format, &mut office_buf, &mut store3)?,
        other => docsai_office::read(other, &mut office_buf, &mut store3)?,
    };
    report.merge(r4);
    let (md2, r5) = docsai_docmark::serialize(
        &document3,
        &store3,
        &DocMarkOptions {
            fidelity: Fidelity::Full,
            ids: IdPolicy::Assign,
            assets_dir: "assets".into(),
            source_format: office_format,
            raw: RawPolicy::Inline,
        },
    );
    report.merge(r5);

    let mut written = Vec::new();
    if output.is_some() {
        let dir = assets_dir;
        for id in store3.ids() {
            if let (Some(bytes), Some(info)) = (store3.get(&id), store3.info(&id)) {
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(&info.file_name);
                if std::fs::write(&path, bytes).is_ok() {
                    written.push(path);
                }
            }
        }
    }

    let identical = md1 == md2;
    if !identical {
        report.warn(docsai_model::Warning::Degraded {
            what: "roundtrip".into(),
            why: "second DocMark pass differs from the first".into(),
        });
    }
    Ok(RoundtripOutcome {
        source_format,
        first_markdown: md1,
        second_markdown: md2,
        identical,
        report,
        output_path: output.map(Path::to_path_buf),
    })
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

fn resolve_target(output: Option<&Path>, options: &ConvertOptions) -> Result<Format, ConvertError> {
    if let Some(target) = options.target {
        return Ok(target);
    }
    let Some(output) = output else {
        return Ok(Format::DocMark);
    };
    // `-` means stdout; only text (DocMark) is allowed there.
    if is_stdout_path(output) {
        return Ok(Format::DocMark);
    }
    let name = output.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with(".dmk.md") || name.ends_with(".md") {
        return Ok(Format::DocMark);
    }
    match name.rsplit('.').next().and_then(Format::parse) {
        Some(format) => Ok(format),
        None => Err(ConvertError::UnknownFormat(output.display().to_string())),
    }
}

fn default_assets_dir(input: &Path, output: Option<&Path>) -> PathBuf {
    let base = output
        .and_then(|o| o.parent())
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| input.parent())
        .unwrap_or_else(|| Path::new("."));
    base.join("assets")
}

/// Writes the raw-block sidecars this serialisation refers to, and returns the
/// paths written.
///
/// The body already points at these files by name, so a failure here is a
/// document that references bytes nobody wrote: it is an error, never a
/// warning. Nothing happens unless the options actually put raw-blocks aside.
pub(crate) fn write_raw_sidecars(
    document: &Document,
    options: &DocMarkOptions,
    assets_dir: &Path,
) -> Result<Vec<PathBuf>, ConvertError> {
    let sidecars = docsai_docmark::raw::raw_sidecars(document, options);
    if sidecars.is_empty() {
        return Ok(Vec::new());
    }
    let dir = assets_dir.join("_raw");
    std::fs::create_dir_all(&dir).map_err(|source| ConvertError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    let mut written = Vec::new();
    for sidecar in sidecars {
        // The name is the serializer's, taken from the reference it wrote, so
        // the file and the `src=` cannot drift apart.
        let name = sidecar.path.rsplit('/').next().unwrap_or(&sidecar.path);
        let path = dir.join(name);
        std::fs::write(&path, format!("{}\n", sidecar.content)).map_err(|source| {
            ConvertError::Io {
                path: path.display().to_string(),
                source,
            }
        })?;
        written.push(path);
    }
    Ok(written)
}

fn relative_assets_dir(assets_dir: &Path, output: Option<&Path>) -> String {
    let parent = output.and_then(|o| o.parent()).unwrap_or(Path::new(""));
    let relative = assets_dir.strip_prefix(parent).unwrap_or(assets_dir);
    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/docx")
            .join(name)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("docsai-pipeline-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn converts_a_docx_to_a_file_with_its_assets() {
        let dir = temp_dir("images");
        let output = dir.join("out.dmk.md");
        let outcome = convert_file(
            &corpus("images-inline.docx"),
            Some(&output),
            &ConvertOptions::default(),
        )
        .expect("converts");

        assert_eq!(outcome.source_format, Format::Docx);
        assert_eq!(outcome.target_format, Format::DocMark);
        assert_eq!(outcome.assets_written.len(), 3);
        assert!(output.exists());

        let written = std::fs::read_to_string(&output).unwrap();
        assert_eq!(written, outcome.markdown);
        assert!(written.contains("](assets/img-"), "links point at assets/");
        for path in &outcome.assets_written {
            assert!(path.starts_with(dir.join("assets")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_an_output_path_nothing_is_written() {
        let outcome = convert_file(&corpus("basic-text.docx"), None, &ConvertOptions::default())
            .expect("converts");
        assert!(outcome.output_path.is_none());
        assert!(outcome.markdown.contains("Primer parrafo"));
    }

    #[test]
    fn docx_target_writes_a_package() {
        let dir = temp_dir("to-docx");
        let dmk = dir.join("in.dmk.md");
        let md = convert_file(&corpus("basic-text.docx"), None, &ConvertOptions::default())
            .unwrap()
            .markdown;
        std::fs::write(&dmk, md).unwrap();
        let out = dir.join("out.docx");
        let outcome = convert_file(
            &dmk,
            Some(&out),
            &ConvertOptions {
                target: Some(Format::Docx),
                ..Default::default()
            },
        )
        .expect("docx write");
        assert_eq!(outcome.target_format, Format::Docx);
        assert!(out.exists());
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn roundtrip_basic_text_is_stable() {
        let outcome = roundtrip_file(&corpus("basic-text.docx"), None, &ConvertOptions::default())
            .expect("roundtrip");
        assert!(
            outcome.identical,
            "basic-text should round-trip cleanly\n--- first ---\n{}\n--- second ---\n{}",
            outcome.first_markdown, outcome.second_markdown
        );
    }

    fn corpus_odt(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/odt")
            .join(name)
    }

    fn corpus_ods(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/ods")
            .join(name)
    }

    #[test]
    fn converts_an_odt_to_docmark() {
        let outcome = convert_file(
            &corpus_odt("basic-text.odt"),
            None,
            &ConvertOptions::default(),
        )
        .expect("converts odt");
        assert_eq!(outcome.source_format, Format::Odt);
        assert_eq!(outcome.target_format, Format::DocMark);
        assert!(!outcome.markdown.is_empty());
    }

    #[test]
    fn converts_an_ods_to_docmark() {
        let outcome = convert_file(
            &corpus_ods("values-types.ods"),
            None,
            &ConvertOptions::default(),
        )
        .expect("converts ods");
        assert_eq!(outcome.source_format, Format::Ods);
        assert!(!outcome.markdown.is_empty());
    }

    #[test]
    fn roundtrip_basic_odt_is_stable() {
        let outcome = roundtrip_file(
            &corpus_odt("basic-text.odt"),
            None,
            &ConvertOptions::default(),
        )
        .expect("odt roundtrip");
        assert!(
            outcome.identical,
            "basic-text.odt should round-trip cleanly\n--- first ---\n{}\n--- second ---\n{}",
            outcome.first_markdown, outcome.second_markdown
        );
    }

    #[test]
    fn roundtrip_ods_values_is_stable() {
        let outcome = roundtrip_file(
            &corpus_ods("values-types.ods"),
            None,
            &ConvertOptions::default(),
        )
        .expect("ods roundtrip");
        assert!(
            outcome.identical,
            "values-types.ods should round-trip cleanly\n--- first ---\n{}\n--- second ---\n{}",
            outcome.first_markdown, outcome.second_markdown
        );
    }

    #[test]
    fn odt_target_writes_a_package() {
        let dir = temp_dir("to-odt");
        let dmk = dir.join("in.dmk.md");
        let md = convert_file(
            &corpus_odt("basic-text.odt"),
            None,
            &ConvertOptions::default(),
        )
        .unwrap()
        .markdown;
        std::fs::write(&dmk, md).unwrap();
        let out = dir.join("out.odt");
        let outcome = convert_file(
            &dmk,
            Some(&out),
            &ConvertOptions {
                target: Some(Format::Odt),
                ..Default::default()
            },
        )
        .expect("odt write");
        assert_eq!(outcome.target_format, Format::Odt);
        assert!(out.exists());
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unsupported_target_is_rejected() {
        let options = ConvertOptions {
            target: Some(Format::Doc),
            ..Default::default()
        };
        let error = convert_file(&corpus("basic-text.docx"), None, &options).unwrap_err();
        assert!(matches!(error, ConvertError::Unsupported { .. }));
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let error = convert_file(
            Path::new("no-such-file.docx"),
            None,
            &ConvertOptions::default(),
        )
        .unwrap_err();
        assert!(matches!(error, ConvertError::Io { .. }));
    }

    #[test]
    fn asset_links_stay_relative_to_the_output() {
        assert_eq!(
            relative_assets_dir(Path::new("out/assets"), Some(Path::new("out/doc.md"))),
            "assets"
        );
        assert_eq!(
            relative_assets_dir(Path::new("media"), Some(Path::new("doc.md"))),
            "media"
        );
        assert_eq!(
            relative_assets_dir(Path::new("/tmp/a1"), None),
            "/tmp/a1",
            "an absolute directory keeps exactly one leading slash"
        );
    }
}
