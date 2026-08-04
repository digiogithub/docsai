//! The corpus token budget (plan v2 Phase 10, increment F).
//!
//! `corpus/token-budget.md` is a golden like any other: it records what every
//! corpus document costs at each fidelity level, so a change that makes
//! documents more expensive to read shows up as a diff instead of as nothing.
//!
//! ```text
//! DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test token_budget
//! ```
//!
//! …and if the update would inflate the corpus total by more than 5 %, it is
//! refused unless `DOCSAI_ACCEPT_TOKEN_INFLATION=1` is set as well. Paying 5 %
//! more tokens for every document is a product decision, not a golden refresh.

use std::path::{Path, PathBuf};

use docsai_convert::{token_report_path, ConvertOptions, Fidelity};

/// How much the corpus total may grow before the gate refuses the update.
const MAX_INFLATION: f64 = 5.0;

const LEVELS: [Fidelity; 4] = [
    Fidelity::Full,
    Fidelity::Agent,
    Fidelity::Standard,
    Fidelity::Plain,
];

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn golden_path() -> PathBuf {
    corpus_root().join("token-budget.md")
}

/// Every convertible corpus document, in a stable order.
fn documents() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for subdir in ["docx", "xlsx", "odt", "ods"] {
        let dir = corpus_root().join(subdir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.ends_with(".expected.dmk.md") {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some(subdir) {
                out.push((format!("{subdir}/{name}"), path));
            }
        }
    }
    out.sort();
    out
}

fn report() -> String {
    let mut rows = String::new();
    let mut totals = [0usize; LEVELS.len()];

    for (name, path) in documents() {
        let mut costs = Vec::new();
        for (index, fidelity) in LEVELS.iter().enumerate() {
            let options = ConvertOptions {
                fidelity: *fidelity,
                ..Default::default()
            };
            let report = token_report_path(&path, &options)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            totals[index] += report.total;
            costs.push(report.total);
        }
        let cells: Vec<String> = costs.iter().map(usize::to_string).collect();
        rows.push_str(&format!("| `{name}` | {} |\n", cells.join(" | ")));
    }

    let header: Vec<&str> = LEVELS.iter().map(|f| f.as_str()).collect();
    let rule = "|---".to_string() + &"|---:".repeat(LEVELS.len()) + "|";
    let total_cells: Vec<String> = totals.iter().map(|t| format!("**{t}**")).collect();

    format!(
        "# Corpus token budget\n\
         \n\
         What every corpus document costs an LLM, measured with `o200k_base` over the DocMark\n\
         `docsai convert` would write. Generated — do not edit by hand:\n\
         \n\
         ```bash\n\
         DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test token_budget\n\
         ```\n\
         \n\
         An update that inflates the total by more than {MAX_INFLATION:.0} % is refused unless\n\
         `DOCSAI_ACCEPT_TOKEN_INFLATION=1` says so on purpose.\n\
         \n\
         | Document | {} |\n\
         {rule}\n\
         {rows}| **total** | {} |\n",
        header.join(" | "),
        total_cells.join(" | ")
    )
}

/// The `full` column total, which is the number the gate watches.
fn full_total(report: &str) -> Option<usize> {
    let line = report.lines().find(|l| l.starts_with("| **total**"))?;
    line.split('|')
        .nth(2)?
        .trim()
        .trim_matches('*')
        .parse()
        .ok()
}

#[test]
fn the_corpus_token_budget_matches_its_golden() {
    let current = report();
    let golden = golden_path();
    let previous = std::fs::read_to_string(&golden).ok();

    if std::env::var_os("DOCSAI_UPDATE_GOLDENS").is_some() {
        if let (Some(previous), Some(new)) = (
            previous.as_deref().and_then(full_total),
            full_total(&current),
        ) {
            let inflation = (new as f64 - previous as f64) * 100.0 / previous as f64;
            assert!(
                inflation <= MAX_INFLATION
                    || std::env::var_os("DOCSAI_ACCEPT_TOKEN_INFLATION").is_some(),
                "the corpus would cost {inflation:.1} % more to read ({previous} → {new} tokens \
                 at `full`). Justify it in the diff and re-run with \
                 DOCSAI_ACCEPT_TOKEN_INFLATION=1."
            );
        }
        std::fs::write(&golden, &current).expect("writes the token budget");
        return;
    }

    let Some(previous) = previous else {
        panic!(
            "{} is missing; generate it with DOCSAI_UPDATE_GOLDENS=1",
            golden.display()
        );
    };
    if previous == current {
        return;
    }

    let detail = match (full_total(&previous), full_total(&current)) {
        (Some(before), Some(after)) => format!(
            " Corpus total at `full`: {before} → {after} ({:+.1} %).",
            (after as f64 - before as f64) * 100.0 / before as f64
        ),
        _ => String::new(),
    };
    panic!(
        "the corpus token budget changed.{detail} If the change is intended, regenerate with \
         DOCSAI_UPDATE_GOLDENS=1 and let the diff show what it costs."
    );
}
