//! Parallel batch conversion for `docsai convert --out-dir`.

use std::path::{Path, PathBuf};

use rayon::prelude::*;
use serde::Serialize;

use crate::pipeline::{convert_file, ConvertOptions, Outcome};
use crate::ConvertError;
use docsai_model::Format;

/// One item in a batch run.
#[derive(Debug)]
pub struct BatchItem {
    pub input: PathBuf,
    pub output: PathBuf,
    pub result: Result<ItemSuccess, ItemFailure>,
}

/// Successful conversion summary (serialisable for `--json`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ItemSuccess {
    pub source_format: String,
    pub target_format: String,
    pub warnings: usize,
    pub severe_warnings: usize,
    pub raw_blocks: u32,
    pub output: String,
}

/// Failed conversion summary.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ItemFailure {
    pub error: String,
    pub exit_hint: u8,
}

/// Aggregated batch outcome.
#[derive(Debug)]
pub struct BatchOutcome {
    pub items: Vec<BatchItem>,
    pub ok: usize,
    pub failed: usize,
    pub with_warnings: usize,
}

/// JSON-friendly batch report.
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BatchReport {
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    pub with_warnings: usize,
    pub items: Vec<BatchReportItem>,
}

/// One row of [`BatchReport`].
#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct BatchReportItem {
    pub input: String,
    pub output: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<ItemSuccess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<ItemFailure>,
}

impl BatchOutcome {
    pub fn to_report(&self) -> BatchReport {
        BatchReport {
            total: self.items.len(),
            ok: self.ok,
            failed: self.failed,
            with_warnings: self.with_warnings,
            items: self
                .items
                .iter()
                .map(|item| BatchReportItem {
                    input: item.input.display().to_string(),
                    output: item.output.display().to_string(),
                    success: item.result.as_ref().ok().cloned(),
                    failure: item.result.as_ref().err().cloned(),
                })
                .collect(),
        }
    }

    /// Worst exit code among items (0 / 1 / 2 / 3).
    pub fn exit_code(&self, strict: bool) -> u8 {
        let mut code = 0u8;
        for item in &self.items {
            match &item.result {
                Ok(success) => {
                    if success.severe_warnings > 0 || (strict && success.warnings > 0) {
                        code = code.max(1);
                    }
                }
                Err(failure) => code = code.max(failure.exit_hint),
            }
        }
        code
    }
}

/// Converts many inputs into `out_dir`, in parallel.
///
/// Output names are derived from each input stem plus the target extension
/// (`.dmk.md` for DocMark, otherwise `.<format>`).
pub fn convert_batch(
    inputs: &[PathBuf],
    out_dir: &Path,
    options: &ConvertOptions,
) -> Result<BatchOutcome, ConvertError> {
    if inputs.is_empty() {
        return Err(ConvertError::Invalid(
            "batch convert requires at least one input path".into(),
        ));
    }
    std::fs::create_dir_all(out_dir).map_err(|source| ConvertError::Io {
        path: out_dir.display().to_string(),
        source,
    })?;

    let target = options.target.unwrap_or(Format::DocMark);
    let items: Vec<BatchItem> = inputs
        .par_iter()
        .map(|input| {
            let output = output_path_for(input, out_dir, target);
            let mut item_options = options.clone();
            // Per-file assets default next to that file's output.
            if item_options.assets_dir.is_none() {
                item_options.assets_dir = Some(out_dir.join("assets"));
            }
            let result = match convert_file(input, Some(&output), &item_options) {
                Ok(outcome) => Ok(success_from_outcome(&outcome, &output)),
                Err(error) => Err(ItemFailure {
                    exit_hint: exit_hint_for(&error),
                    error: error.to_string(),
                }),
            };
            BatchItem {
                input: input.clone(),
                output,
                result,
            }
        })
        .collect();

    let mut ok = 0usize;
    let mut failed = 0usize;
    let mut with_warnings = 0usize;
    for item in &items {
        match &item.result {
            Ok(success) => {
                ok += 1;
                if success.warnings > 0 {
                    with_warnings += 1;
                }
            }
            Err(_) => failed += 1,
        }
    }

    Ok(BatchOutcome {
        items,
        ok,
        failed,
        with_warnings,
    })
}

/// Builds the output path for one batch input.
pub fn output_path_for(input: &Path, out_dir: &Path, target: Format) -> PathBuf {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    // `file_stem` of `report.dmk.md` is `report.dmk` on most platforms — strip
    // a trailing `.dmk` so batch re-conversion stays stable.
    let stem = stem.strip_suffix(".dmk").unwrap_or(stem);
    let name = match target {
        Format::DocMark => format!("{stem}.dmk.md"),
        other => format!("{stem}.{}", other.as_str()),
    };
    out_dir.join(name)
}

fn success_from_outcome(outcome: &Outcome, output: &Path) -> ItemSuccess {
    let severe = outcome
        .report
        .warnings
        .iter()
        .filter(|w| w.severity() == docsai_model::Severity::Severe)
        .count();
    ItemSuccess {
        source_format: outcome.source_format.as_str().to_string(),
        target_format: outcome.target_format.as_str().to_string(),
        warnings: outcome.report.warnings.len(),
        severe_warnings: severe,
        raw_blocks: outcome.report.raw_blocks_emitted,
        output: output.display().to_string(),
    }
}

fn exit_hint_for(error: &ConvertError) -> u8 {
    match error {
        ConvertError::Unsupported { .. } | ConvertError::UnknownFormat(_) => 3,
        ConvertError::Loffice { .. } => 2,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_names_use_dmk_md_for_docmark() {
        let path = output_path_for(Path::new("report.docx"), Path::new("md"), Format::DocMark);
        assert_eq!(path, PathBuf::from("md/report.dmk.md"));
    }

    #[test]
    fn output_names_strip_existing_dmk_stem() {
        let path = output_path_for(
            Path::new("report.dmk.md"),
            Path::new("out"),
            Format::DocMark,
        );
        assert_eq!(path, PathBuf::from("out/report.dmk.md"));
    }

    #[test]
    fn batch_converts_corpus_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/docx");
        let inputs = vec![root.join("basic-text.docx"), root.join("basic-styles.docx")];
        let out = std::env::temp_dir().join(format!("docsai-batch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        let outcome = convert_batch(&inputs, &out, &ConvertOptions::default()).expect("batch");
        assert_eq!(outcome.ok, 2);
        assert_eq!(outcome.failed, 0);
        assert!(out.join("basic-text.dmk.md").exists());
        assert!(out.join("basic-styles.dmk.md").exists());
        let _ = std::fs::remove_dir_all(&out);
    }
}
