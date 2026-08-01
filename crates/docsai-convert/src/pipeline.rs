//! The conversion pipelines.

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options as DocMarkOptions};
use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format};

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
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            fidelity: Fidelity::Full,
            assets_dir: None,
            target: None,
        }
    }
}

/// What a conversion produced.
#[derive(Debug)]
pub struct Outcome {
    pub source_format: Format,
    pub target_format: Format,
    /// The DocMark text, also written to `output_path` when one was given.
    pub markdown: String,
    pub output_path: Option<PathBuf>,
    pub assets_written: Vec<PathBuf>,
    pub report: ConversionReport,
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
    let (document, report) = docsai_office::read(format, reader, assets)?;
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

/// Converts a file on disk.
///
/// With `output` set, the DocMark is written there and media go to the assets
/// directory; without it the text is returned for the caller to print.
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

    let file = std::fs::File::open(input).map_err(|source| ConvertError::Io {
        path: input.display().to_string(),
        source,
    })?;
    let hint = input.file_name().and_then(|n| n.to_str());

    let assets_dir = options
        .assets_dir
        .clone()
        .unwrap_or_else(|| default_assets_dir(input, output));
    let mut store = DirAssetStore::new(assets_dir.clone());

    let (document, source_format, mut report) = read_document(file, hint, &mut store)?;

    let docmark_options = DocMarkOptions {
        fidelity: options.fidelity,
        assets_dir: relative_assets_dir(&assets_dir, output),
        source_format,
    };
    let (markdown, write_report) = docsai_docmark::serialize(&document, &store, &docmark_options);
    report.merge(write_report);

    if let Some(output) = output {
        if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| ConvertError::Io {
                path: parent.display().to_string(),
                source,
            })?;
        }
        std::fs::write(output, markdown.as_bytes()).map_err(|source| ConvertError::Io {
            path: output.display().to_string(),
            source,
        })?;
    }

    Ok(Outcome {
        source_format,
        target_format: target,
        markdown,
        output_path: output.map(Path::to_path_buf),
        assets_written: store.written().to_vec(),
        report,
    })
}

fn resolve_target(output: Option<&Path>, options: &ConvertOptions) -> Result<Format, ConvertError> {
    if let Some(target) = options.target {
        return Ok(target);
    }
    let Some(output) = output else {
        // Writing to stdout: DocMark is the only text format there is.
        return Ok(Format::DocMark);
    };
    let name = output.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // `.dmk.md` and `.md` both mean DocMark.
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

/// The path DocMark links should use: relative to the output file when
/// possible, so the `.dmk.md` stays portable.
fn relative_assets_dir(assets_dir: &Path, output: Option<&Path>) -> String {
    let parent = output.and_then(|o| o.parent()).unwrap_or(Path::new(""));
    let relative = assets_dir.strip_prefix(parent).unwrap_or(assets_dir);
    // DocMark links always use `/`, on every platform.
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
    fn an_unsupported_target_is_rejected_before_reading() {
        let options = ConvertOptions {
            target: Some(Format::Docx),
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
