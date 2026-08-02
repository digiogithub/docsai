//! The `docsai` command-line interface.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use docsai_convert::{ConvertOptions, Fidelity, SUPPORT};
use docsai_model::Format;

/// Exit codes (architecture §5).
const EXIT_OK: u8 = 0;
const EXIT_WARNINGS: u8 = 1;
const EXIT_INPUT: u8 = 2;
const EXIT_UNSUPPORTED: u8 = 3;

#[derive(Parser)]
#[command(
    name = "docsai",
    version,
    about = "Bidirectional converter between Office/LibreOffice documents and DocMark",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Print warnings in detail.
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a document.
    Convert {
        /// Input document.
        input: PathBuf,
        /// Output file; without it the result goes to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Force the target format instead of inferring it from `--output`.
        #[arg(long, value_name = "FMT")]
        to: Option<String>,
        /// How much of the source survives: full, standard or plain.
        #[arg(long, default_value = "full")]
        fidelity: String,
        /// Where to extract media. Defaults to `assets/` next to the output.
        #[arg(long, value_name = "DIR")]
        assets_dir: Option<PathBuf>,
        /// Print the conversion report as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// Treat severe warnings as failures.
        #[arg(long)]
        strict: bool,
    },
    /// Show which formats this build can read and write.
    Formats {
        /// Print the matrix as JSON.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    // Logs always go to stderr: stdout may carry the converted document.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("docsai: {error}");
            ExitCode::from(exit_code_for(&error))
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<u8> {
    match &cli.command {
        Command::Formats { json } => {
            print_formats(*json);
            Ok(EXIT_OK)
        }
        Command::Convert {
            input,
            output,
            to,
            fidelity,
            assets_dir,
            json,
            strict,
        } => {
            let fidelity = Fidelity::parse(fidelity).ok_or_else(|| {
                anyhow::anyhow!("unknown --fidelity `{fidelity}`; use full, standard or plain")
            })?;
            let target = match to {
                Some(name) => Some(
                    Format::parse(name)
                        .ok_or_else(|| anyhow::anyhow!("unknown --to format `{name}`"))?,
                ),
                None => None,
            };
            let options = ConvertOptions {
                fidelity,
                assets_dir: assets_dir.clone(),
                target,
            };
            let outcome = docsai_convert::convert_file(input, output.as_deref(), &options)?;

            if output.is_none() && !*json {
                let mut stdout = std::io::stdout().lock();
                stdout.write_all(outcome.markdown.as_bytes())?;
            }
            if *json {
                let report = serde_json::to_string_pretty(&outcome.report)?;
                println!("{report}");
            } else {
                report_to_stderr(&outcome, cli.verbose);
            }

            // Exit code 1 marks a conversion that lost something; `--strict`
            // additionally makes minor warnings count as severe.
            let lost =
                outcome.report.has_severe() || (*strict && !outcome.report.warnings.is_empty());
            Ok(if lost { EXIT_WARNINGS } else { EXIT_OK })
        }
    }
}

fn report_to_stderr(outcome: &docsai_convert::Outcome, verbose: bool) {
    let report = &outcome.report;
    if report.warnings.is_empty() {
        return;
    }
    let severe = report
        .warnings
        .iter()
        .filter(|w| w.severity() == docsai_model::Severity::Severe)
        .count();
    eprintln!(
        "docsai: {} warning(s){}, {} raw-block(s)",
        report.warnings.len(),
        if severe > 0 {
            format!(", {severe} severe")
        } else {
            String::new()
        },
        report.raw_blocks_emitted
    );
    if verbose {
        for warning in &report.warnings {
            eprintln!("  [{:?}] {}", warning.severity(), warning.message());
        }
    } else {
        eprintln!("  run with --verbose (or --json) for the detail");
    }
}

fn print_formats(json: bool) {
    if json {
        let rows: Vec<_> = SUPPORT
            .iter()
            .map(|s| {
                serde_json::json!({
                    "format": s.format.as_str(),
                    "read": s.read,
                    "write": s.write,
                    "note": s.note,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&rows).unwrap_or_default()
        );
        return;
    }
    println!("{:<10} {:<6} {:<6} NOTE", "FORMAT", "READ", "WRITE");
    for support in SUPPORT {
        println!(
            "{:<10} {:<6} {:<6} {}",
            support.format.as_str(),
            if support.read { "yes" } else { "no" },
            if support.write { "yes" } else { "no" },
            support.note
        );
    }
}

fn exit_code_for(error: &anyhow::Error) -> u8 {
    match error.downcast_ref::<docsai_convert::ConvertError>() {
        Some(docsai_convert::ConvertError::Unsupported { .. }) => EXIT_UNSUPPORTED,
        Some(docsai_convert::ConvertError::UnknownFormat(_)) => EXIT_UNSUPPORTED,
        Some(_) => EXIT_INPUT,
        None => EXIT_INPUT,
    }
}
