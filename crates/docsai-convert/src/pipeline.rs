//! The conversion pipelines.

use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options as DocMarkOptions};
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
    let file = File::open(input).map_err(|source| ConvertError::Io {
        path: input.display().to_string(),
        source,
    })?;
    let (document, report) = docsai_office::read(format, file, assets)?;
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
/// Supports Office → DocMark, DocMark → Office, and DocMark → DocMark.
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

    let (document, source_format, mut report) = read_path(input, &mut store)?;
    if !crate::can_read(source_format) {
        return Err(ConvertError::Unsupported {
            from: source_format,
            to: target,
        });
    }

    match target {
        Format::DocMark => {
            let docmark_options = DocMarkOptions {
                fidelity: options.fidelity,
                assets_dir: relative_assets_dir(&assets_dir, output),
                source_format,
            };
            let (markdown, write_report) =
                docsai_docmark::serialize(&document, &store, &docmark_options);
            report.merge(write_report);

            if let Some(output) = output {
                ensure_parent(output)?;
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
        Format::Docx => {
            let output = output.ok_or_else(|| {
                ConvertError::Invalid("writing .docx requires an --output path".into())
            })?;
            ensure_parent(output)?;
            let file = File::create(output).map_err(|source| ConvertError::Io {
                path: output.display().to_string(),
                source,
            })?;
            let write_report = docsai_office::write_docx(&document, &store, file)?;
            report.merge(write_report);

            let (markdown, _) = docsai_docmark::serialize(
                &document,
                &store,
                &DocMarkOptions {
                    fidelity: options.fidelity,
                    assets_dir: relative_assets_dir(&assets_dir, Some(output)),
                    source_format,
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
    let (document1, source_format, mut report) = read_path(input, &mut store1)?;
    if source_format != Format::Docx && source_format != Format::DocMark {
        return Err(ConvertError::Unsupported {
            from: source_format,
            to: Format::Docx,
        });
    }

    let (md1, r1) = docsai_docmark::serialize(
        &document1,
        &store1,
        &DocMarkOptions {
            fidelity: Fidelity::Full,
            assets_dir: "assets".into(),
            source_format,
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

    let mut docx_buf = Cursor::new(Vec::new());
    let r3 = docsai_office::write_docx(&document2, &store2, &mut docx_buf)?;
    report.merge(r3);

    if let Some(output) = output {
        ensure_parent(output)?;
        std::fs::write(output, docx_buf.get_ref()).map_err(|source| ConvertError::Io {
            path: output.display().to_string(),
            source,
        })?;
    }

    docx_buf.set_position(0);
    let mut store3 = MemoryAssetStore::new();
    let (document3, r4) = docsai_office::read_docx(&mut docx_buf, &mut store3)?;
    report.merge(r4);
    let (md2, r5) = docsai_docmark::serialize(
        &document3,
        &store3,
        &DocMarkOptions {
            fidelity: Fidelity::Full,
            assets_dir: "assets".into(),
            source_format: Format::Docx,
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

    #[test]
    fn an_unsupported_target_is_rejected() {
        let options = ConvertOptions {
            target: Some(Format::Odt),
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
