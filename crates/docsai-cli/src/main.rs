//! The `docsai` command-line interface.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use docsai_convert::{
    convert_batch, convert_file, inspect_path, ConvertOptions, Fidelity, RawPolicy, StyleMap,
    UseLoffice, SUPPORT,
};
use docsai_model::addressing::IdPolicy;
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
    long_about = "docsai converts Office and LibreOffice documents to DocMark (extended Markdown) and back.\n\n\
Examples:\n  \
docsai convert report.docx -o report.dmk.md\n  \
docsai convert *.docx --out-dir md/\n  \
docsai convert - --to docmark < report.docx\n  \
docsai inspect report.docx --json\n  \
docsai roundtrip report.docx\n  \
docsai formats\n  \
docsai mcp"
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
    /// Convert one or more documents.
    Convert {
        /// Input document(s). Use `-` to read a single document from stdin.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Output file; without it the result goes to stdout (single input only).
        /// Use `-` to force stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Write each input into this directory (enables batch mode).
        #[arg(long, value_name = "DIR")]
        out_dir: Option<PathBuf>,
        /// Force the target format instead of inferring it from `--output` / `--out-dir`.
        #[arg(long, value_name = "FMT")]
        to: Option<String>,
        /// How much of the source survives: full, agent, standard or plain.
        #[arg(long, default_value = "full")]
        fidelity: String,
        /// Node ids in the output: `assign`, `preserve` or `never`.
        /// Defaults to `assign` at `--fidelity full` and `never` otherwise.
        #[arg(long, value_name = "MODE")]
        ids: Option<String>,
        /// Raw-block bytes: `sidecar` (a file per fragment) or `inline`.
        #[arg(long, default_value = "sidecar", value_name = "MODE")]
        raw: String,
        /// Decimals a readable unit may use before a length falls back to EMU
        /// (DocMark spec §2). Never rounds: this buys readable units, not
        /// accuracy.
        #[arg(long, default_value_t = docsai_model::units::DEFAULT_PRECISION, value_name = "N")]
        precision: u8,
        /// Where to extract media. Defaults to `assets/` next to the output.
        #[arg(long, value_name = "DIR")]
        assets_dir: Option<PathBuf>,
        /// Publication-mode style map (`StyleName: h1`, DocMark spec §5).
        #[arg(long, value_name = "YAML")]
        style_map: Option<PathBuf>,
        /// Abort when a workbook has more than N non-empty cells.
        #[arg(long, value_name = "N")]
        max_cells: Option<u64>,
        /// Print the conversion report as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// Treat severe warnings as failures; with this flag, any warning fails.
        #[arg(long)]
        strict: bool,
        /// LibreOffice headless for legacy `.doc`: `auto`, `never`, or `require`.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        use_loffice: String,
    },
    /// Show document structure without converting (metadata, styles, sheets, media).
    Inspect {
        /// Input document. Use `-` to read from stdin.
        input: PathBuf,
        /// Print the inspection report as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// LibreOffice headless for legacy `.doc`: `auto`, `never`, or `require`.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        use_loffice: String,
    },
    /// Map a document: the tree of addressable nodes with a preview and a cost.
    ///
    /// The cheap thing to read before deciding what to read: node ids, kinds,
    /// a short preview and the measured token cost of each.
    Outline {
        /// Input document.
        input: PathBuf,
        /// Keep only the first N levels of the tree.
        #[arg(long, value_name = "N")]
        depth: Option<usize>,
        /// How much of the source survives: full, agent, standard or plain.
        #[arg(long, default_value = "full")]
        fidelity: String,
        /// Print the outline as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// LibreOffice headless for legacy `.doc`: `auto`, `never`, or `require`.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        use_loffice: String,
    },
    /// Read part of a document: the selected nodes as self-contained DocMark.
    ///
    /// What `outline` is for: it says where the paragraph is, this returns it
    /// and nothing else. The output declares `partial: true` and carries an
    /// etag per node, so an edit can be written back with a precondition.
    Read {
        /// Input document.
        input: PathBuf,
        /// What to read: `s4`, `s7-s9`, `#n7`, `type:heading`, `text:revenue`.
        /// Comma-separated terms are unioned; the output is in document order.
        #[arg(long, value_name = "SELECTOR")]
        select: String,
        /// How much of the source survives: `full` or `agent`.
        #[arg(long, default_value = "full")]
        fidelity: String,
        /// Output file; without it the selection goes to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Print the selection and what it cost as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// LibreOffice headless for legacy `.doc`: `auto`, `never`, or `require`.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        use_loffice: String,
    },
    /// Find text in a document: node ids plus the words around each match.
    ///
    /// The answer to "where does it say that", without paying for the
    /// document. Every hit carries the selector that reads it, so the next
    /// call is `docsai read --select <that>`.
    Search {
        /// Input document.
        input: PathBuf,
        /// What to look for: a literal, matched case-insensitively.
        query: String,
        /// Characters of context kept either side of a match.
        #[arg(long, default_value_t = 48, value_name = "N")]
        context: usize,
        /// Hits to list; the rest are counted. `0` lists all of them.
        #[arg(long, default_value_t = 20, value_name = "N")]
        limit: usize,
        /// How much of the source survives: full, agent, standard or plain.
        #[arg(long, default_value = "full")]
        fidelity: String,
        /// Print the hits as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// LibreOffice headless for legacy `.doc`: `auto`, `never`, or `require`.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        use_loffice: String,
    },
    /// Measure what a document costs an LLM, per document and per node.
    ///
    /// Counted with a real BPE tokenizer (`o200k_base`) over the DocMark that
    /// would be written, not estimated from the file size.
    Tokens {
        /// Input document.
        input: PathBuf,
        /// How much of the source survives: full, agent, standard or plain.
        #[arg(long, default_value = "full")]
        fidelity: String,
        /// Node ids in the measured output: `assign`, `preserve` or `never`.
        #[arg(long, value_name = "MODE")]
        ids: Option<String>,
        /// How many of the heaviest nodes to list.
        #[arg(long, default_value_t = 10, value_name = "N")]
        top: usize,
        /// Print the token report as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// LibreOffice headless for legacy `.doc`: `auto`, `never`, or `require`.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        use_loffice: String,
    },
    /// Show which formats this build can read and write.
    Formats {
        /// Print the matrix as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Round-trip a document (Office → DocMark → Office → DocMark) and compare.
    Roundtrip {
        /// Input Office document.
        input: PathBuf,
        /// Optional path for the regenerated Office package.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// How much of the source survives: full, agent, standard or plain.
        #[arg(long, default_value = "full")]
        fidelity: String,
        /// Print the fidelity report as JSON on stdout.
        #[arg(long)]
        json: bool,
        /// LibreOffice headless for legacy `.doc`: `auto`, `never`, or `require`.
        #[arg(long, default_value = "auto", value_name = "MODE")]
        use_loffice: String,
    },
    /// Run the MCP server over stdio (for Claude Desktop / Claude Code / MCP Inspector).
    ///
    /// Logs always go to stderr; stdout is reserved for JSON-RPC.
    /// Limits: `DOCSAI_MCP_MAX_INPUT_BYTES` (default 50MiB),
    /// `DOCSAI_MCP_TIMEOUT_SECS` (default 120, `0` disables).
    Mcp,
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
            hint_for_error(&error);
            ExitCode::from(exit_code_for(&error))
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<u8> {
    match &cli.command {
        Command::Mcp => {
            // The MCP crate owns the tokio runtime and stderr-only tracing.
            docsai_mcp::run()?;
            Ok(EXIT_OK)
        }
        Command::Formats { json } => {
            print_formats(*json);
            Ok(EXIT_OK)
        }
        Command::Inspect {
            input,
            json,
            use_loffice,
        } => run_inspect(input, *json, use_loffice, cli.verbose),
        Command::Tokens {
            input,
            fidelity,
            ids,
            top,
            json,
            use_loffice,
        } => run_tokens(input, fidelity, ids.as_deref(), *top, *json, use_loffice),
        Command::Outline {
            input,
            depth,
            fidelity,
            json,
            use_loffice,
        } => run_outline(input, *depth, fidelity, *json, use_loffice),
        Command::Read {
            input,
            select,
            fidelity,
            output,
            json,
            use_loffice,
        } => run_read(
            input,
            select,
            fidelity,
            output.as_deref(),
            *json,
            use_loffice,
        ),
        Command::Search {
            input,
            query,
            context,
            limit,
            fidelity,
            json,
            use_loffice,
        } => run_search(input, query, *context, *limit, fidelity, *json, use_loffice),
        Command::Roundtrip {
            input,
            output,
            fidelity,
            json,
            use_loffice,
        } => {
            let fidelity = parse_fidelity(fidelity)?;
            let use_loffice = parse_use_loffice(use_loffice)?;
            let options = ConvertOptions {
                fidelity,
                use_loffice,
                ..Default::default()
            };
            let outcome = docsai_convert::roundtrip_file(input, output.as_deref(), &options)?;
            if *json {
                let payload = serde_json::json!({
                    "source_format": outcome.source_format.as_str(),
                    "identical": outcome.identical,
                    "report": outcome.report,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else {
                eprintln!(
                    "docsai: roundtrip {} — second DocMark {} first",
                    outcome.source_format.as_str(),
                    if outcome.identical {
                        "matches"
                    } else {
                        "differs from"
                    }
                );
                if !outcome.identical {
                    eprintln!(
                        "  first: {} bytes, second: {} bytes",
                        outcome.first_markdown.len(),
                        outcome.second_markdown.len()
                    );
                }
                if !outcome.report.warnings.is_empty() {
                    eprintln!(
                        "  {} warning(s), {} raw-block(s)",
                        outcome.report.warnings.len(),
                        outcome.report.raw_blocks_emitted
                    );
                    if cli.verbose {
                        for warning in &outcome.report.warnings {
                            eprintln!("  [{:?}] {}", warning.severity(), warning.message());
                        }
                    }
                }
            }
            let lost = outcome.report.has_severe() || !outcome.identical;
            Ok(if lost { EXIT_WARNINGS } else { EXIT_OK })
        }
        Command::Convert {
            inputs,
            output,
            out_dir,
            to,
            fidelity,
            ids,
            raw,
            precision,
            assets_dir,
            style_map,
            max_cells,
            json,
            strict,
            use_loffice,
        } => run_convert(
            inputs,
            output.as_ref(),
            out_dir.as_ref(),
            to.as_deref(),
            fidelity,
            ids.as_deref(),
            raw,
            *precision,
            assets_dir.clone(),
            style_map.as_ref(),
            *max_cells,
            *json,
            *strict,
            use_loffice,
            cli.verbose,
        ),
    }
}

fn run_inspect(input: &Path, json: bool, use_loffice: &str, verbose: bool) -> anyhow::Result<u8> {
    if docsai_convert::is_stdin_path(input) {
        return inspect_stdin(json, use_loffice, verbose);
    }
    let options = ConvertOptions {
        use_loffice: parse_use_loffice(use_loffice)?,
        ..Default::default()
    };
    let report = inspect_path(input, &options)?;
    print_inspect(&report, json, verbose)?;
    Ok(if report_has_severe(&report) {
        EXIT_WARNINGS
    } else {
        EXIT_OK
    })
}

fn run_tokens(
    input: &Path,
    fidelity: &str,
    ids: Option<&str>,
    top: usize,
    json: bool,
    use_loffice: &str,
) -> anyhow::Result<u8> {
    let options = ConvertOptions {
        fidelity: parse_fidelity(fidelity)?,
        ids: parse_ids(ids)?,
        use_loffice: parse_use_loffice(use_loffice)?,
        ..Default::default()
    };
    let report = docsai_convert::token_report_path(input, &options)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(EXIT_OK);
    }
    println!(
        "{}  {}  fidelity={}  encoding={}",
        report.path.as_deref().unwrap_or("<document>"),
        report.source_format,
        report.fidelity,
        report.encoding
    );
    println!(
        "  total         {:>7} tokens ({} bytes)",
        report.total, report.bytes
    );
    println!("  front matter  {:>7}", report.front_matter);
    println!("  body          {:>7}", report.body);
    if report.nodes.is_empty() {
        println!("  nodes             none addressed at this fidelity");
        return Ok(EXIT_OK);
    }
    println!("  nodes         {:>7} addressed", report.nodes.len());
    println!("  heaviest (nested nodes counted more than once):");
    for node in report.heaviest(top) {
        println!(
            "    {:<6} {:<9} {:>6}  {}",
            node.id.0,
            node.kind.as_str(),
            node.tokens,
            node.preview
        );
    }
    Ok(EXIT_OK)
}

fn run_outline(
    input: &Path,
    depth: Option<usize>,
    fidelity: &str,
    json: bool,
    use_loffice: &str,
) -> anyhow::Result<u8> {
    let options = ConvertOptions {
        fidelity: parse_fidelity(fidelity)?,
        use_loffice: parse_use_loffice(use_loffice)?,
        ..Default::default()
    };
    let outline = docsai_convert::outline_path(input, &options, depth)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&outline)?);
        return Ok(EXIT_OK);
    }
    if outline.is_empty() {
        println!(
            "{}: no addressable nodes at --fidelity {} (ids live at `full`)",
            outline.path.as_deref().unwrap_or("<document>"),
            outline.fidelity
        );
        return Ok(EXIT_OK);
    }
    print!("{}", outline.render_text());
    println!(
        "{} nodes · outline {} tokens · document {} tokens ({:.1} %)",
        outline.len(),
        outline.outline_tokens,
        outline.document_tokens,
        share(outline.outline_tokens, outline.document_tokens)
    );
    Ok(EXIT_OK)
}

fn run_read(
    input: &Path,
    selector: &str,
    fidelity: &str,
    output: Option<&Path>,
    json: bool,
    use_loffice: &str,
) -> anyhow::Result<u8> {
    let fidelity = parse_fidelity(fidelity)?;
    if !fidelity.addresses() {
        anyhow::bail!(
            "--fidelity {fidelity} does not address nodes, so there is nothing to select from; \
             node ids live at `full` and `agent`"
        );
    }
    let selector: docsai_convert::Selector =
        selector.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    let options = ConvertOptions {
        fidelity,
        use_loffice: parse_use_loffice(use_loffice)?,
        ..Default::default()
    };
    let selection = docsai_convert::select_path(input, &options, &selector)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&selection)?);
        return Ok(EXIT_OK);
    }
    if selection.is_empty() {
        eprintln!(
            "docsai: `{}` matched no node in {}; run `docsai outline` to see what there is",
            selection.selector,
            input.display()
        );
        return Ok(EXIT_OK);
    }
    match output {
        Some(path) if path != Path::new("-") => {
            std::fs::write(path, &selection.docmark)?;
            eprintln!(
                "docsai: {} node(s) → {} · {} of {} tokens ({:.1} %)",
                selection.nodes.len(),
                path.display(),
                selection.tokens,
                selection.document_tokens,
                share(selection.tokens, selection.document_tokens)
            );
        }
        _ => print!("{}", selection.docmark),
    }
    Ok(EXIT_OK)
}

#[allow(clippy::too_many_arguments)]
fn run_search(
    input: &Path,
    query: &str,
    context: usize,
    limit: usize,
    fidelity: &str,
    json: bool,
    use_loffice: &str,
) -> anyhow::Result<u8> {
    // Every level is allowed, unlike `read --select`: a level that writes no id
    // still writes text, and saying where the text is — relative to nothing,
    // if that is what the level left — beats pretending the document is empty.
    let fidelity = parse_fidelity(fidelity)?;
    let mut query: docsai_convert::Query = query.parse().map_err(|e| anyhow::anyhow!("{e}"))?;
    query.context = context;
    query.limit = (limit > 0).then_some(limit);
    let options = ConvertOptions {
        fidelity,
        use_loffice: parse_use_loffice(use_loffice)?,
        ..Default::default()
    };
    let results = docsai_convert::search_path(input, &options, &query)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(EXIT_OK);
    }
    if results.is_empty() {
        println!("`{}` appears nowhere in {}", results.query, input.display());
        return Ok(EXIT_OK);
    }
    print!("{}", results.render_text());
    if results.omitted > 0 {
        println!("… {} more block(s) not listed (--limit)", results.omitted);
    }
    println!(
        "{} match(es) in {} block(s) · hits {} tokens · document {} tokens ({:.1} %)",
        results.matches,
        results.blocks,
        results.tokens,
        results.document_tokens,
        share(results.tokens, results.document_tokens)
    );
    Ok(EXIT_OK)
}

/// Percentage of `total` that `part` represents, `0.0` when there is no total.
fn share(part: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    part as f64 * 100.0 / total as f64
}

fn inspect_stdin(json: bool, use_loffice: &str, verbose: bool) -> anyhow::Result<u8> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        anyhow::bail!(
            "stdin is empty; pass a file path or pipe a document into `docsai inspect -`"
        );
    }
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("stdin.bin");
    std::fs::write(&path, &bytes)?;
    let options = ConvertOptions {
        use_loffice: parse_use_loffice(use_loffice)?,
        ..Default::default()
    };
    let mut report = inspect_path(&path, &options)?;
    report.path = Some("<stdin>".into());
    print_inspect(&report, json, verbose)?;
    Ok(if report_has_severe(&report) {
        EXIT_WARNINGS
    } else {
        EXIT_OK
    })
}

fn print_inspect(
    report: &docsai_convert::InspectReport,
    json: bool,
    verbose: bool,
) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
        return Ok(());
    }
    if let Some(path) = &report.path {
        println!("path:           {path}");
    }
    println!("format:         {}", report.source_format);
    println!("kind:           {}", report.kind);
    if let Some(title) = &report.meta.title {
        println!("title:          {title}");
    }
    if let Some(author) = &report.meta.author {
        println!("author:         {author}");
    }
    if let Some(language) = &report.meta.language {
        println!("language:       {language}");
    }
    println!("styles:         {}", report.styles.len());
    if verbose {
        for style in &report.styles {
            println!(
                "  - {} ({}){}",
                style.id,
                style.style_type,
                if style.is_default { " [default]" } else { "" }
            );
        }
    }
    if let Some(sections) = &report.sections {
        println!("sections:       {}", sections.len());
        for section in sections {
            println!(
                "  [{}] blocks={} paper={} orientation={}",
                section.index,
                section.blocks,
                section.paper.as_deref().unwrap_or("?"),
                section.orientation
            );
        }
    }
    if let Some(sheets) = &report.sheets {
        println!("sheets:         {}", sheets.len());
        for sheet in sheets {
            println!(
                "  - {} cells={} formulas={} images={}{}",
                sheet.name,
                sheet.cells,
                sheet.formulas,
                sheet.images,
                if sheet.hidden { " (hidden)" } else { "" }
            );
        }
    }
    println!("media:          {}", report.media.len());
    if verbose {
        for asset in &report.media {
            println!(
                "  - {} ({} bytes, {} ref(s))",
                asset.file_name, asset.byte_len, asset.references
            );
        }
    }
    let s = &report.stats;
    println!(
        "stats:          paragraphs={} headings={} lists={} tables={} images={} sheets={} cells={} formulas={}",
        s.paragraphs, s.headings, s.lists, s.tables, s.images, s.sheets, s.cells, s.formulas
    );
    if !report.warnings.is_empty() {
        eprintln!("docsai: {} warning(s) while reading", report.warnings.len());
        if verbose {
            for warning in &report.warnings {
                eprintln!("  [{:?}] {}", warning.severity(), warning.message());
            }
        }
    }
    Ok(())
}

fn report_has_severe(report: &docsai_convert::InspectReport) -> bool {
    report
        .warnings
        .iter()
        .any(|w| w.severity() == docsai_model::Severity::Severe)
}

/// `--ids`; `None` leaves the per-fidelity default to `ConvertOptions`.
fn parse_ids(value: Option<&str>) -> anyhow::Result<Option<IdPolicy>> {
    match value {
        None => Ok(None),
        Some(value) => IdPolicy::parse(value).map(Some).ok_or_else(|| {
            anyhow::anyhow!("unknown --ids `{value}`; use assign, preserve or never")
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_convert(
    inputs: &[PathBuf],
    output: Option<&PathBuf>,
    out_dir: Option<&PathBuf>,
    to: Option<&str>,
    fidelity: &str,
    ids: Option<&str>,
    raw: &str,
    precision: u8,
    assets_dir: Option<PathBuf>,
    style_map: Option<&PathBuf>,
    max_cells: Option<u64>,
    json: bool,
    strict: bool,
    use_loffice: &str,
    verbose: bool,
) -> anyhow::Result<u8> {
    let fidelity = parse_fidelity(fidelity)?;
    let ids = parse_ids(ids)?;
    let raw = RawPolicy::parse(raw)
        .ok_or_else(|| anyhow::anyhow!("unknown --raw `{raw}`; use sidecar or inline"))?;
    let use_loffice = parse_use_loffice(use_loffice)?;
    let target = match to {
        Some(name) => Some(Format::parse(name).ok_or_else(|| {
            anyhow::anyhow!("unknown --to format `{name}`; try `docsai formats`")
        })?),
        None => None,
    };
    let style_map = match style_map {
        Some(path) => Some(StyleMap::load_path(path)?),
        None => None,
    };
    let options = ConvertOptions {
        fidelity,
        ids,
        raw,
        precision,
        assets_dir,
        target,
        use_loffice,
        style_map,
        max_cells,
    };

    let batch = out_dir.is_some() || inputs.len() > 1;
    if batch {
        if output.is_some() {
            anyhow::bail!(
                "use either --output for a single file or --out-dir for batch mode, not both"
            );
        }
        let out_dir = out_dir.ok_or_else(|| {
            anyhow::anyhow!(
                "converting multiple inputs requires --out-dir <dir> \
(example: docsai convert *.docx --out-dir md/)"
            )
        })?;
        if inputs.iter().any(|p| docsai_convert::is_stdin_path(p)) {
            anyhow::bail!(
                "stdin (`-`) cannot be mixed with batch mode; convert one stream at a time"
            );
        }
        let outcome = convert_batch(inputs, out_dir, &options)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&outcome.to_report())?);
        } else {
            eprintln!(
                "docsai: batch {} file(s) — {} ok, {} failed, {} with warnings",
                outcome.items.len(),
                outcome.ok,
                outcome.failed,
                outcome.with_warnings
            );
            for item in &outcome.items {
                match &item.result {
                    Ok(success) => {
                        if verbose || success.warnings > 0 {
                            eprintln!(
                                "  OK  {} → {} ({} warning(s))",
                                item.input.display(),
                                item.output.display(),
                                success.warnings
                            );
                        } else {
                            eprintln!("  OK  {} → {}", item.input.display(), item.output.display());
                        }
                    }
                    Err(failure) => {
                        eprintln!("  ERR {} — {}", item.input.display(), failure.error);
                    }
                }
            }
        }
        return Ok(outcome.exit_code(strict));
    }

    let input = inputs
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing input path"))?;

    let outcome = convert_file(input, output.map(PathBuf::as_path), &options)?;

    let write_stdout = outcome.output_path.is_none() && !json;
    if write_stdout {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(outcome.markdown.as_bytes())?;
    }
    if json {
        let report = serde_json::to_string_pretty(&outcome.report)?;
        println!("{report}");
    } else {
        report_to_stderr(&outcome, verbose);
    }

    let lost = outcome.report.has_severe() || (strict && !outcome.report.warnings.is_empty());
    Ok(if lost { EXIT_WARNINGS } else { EXIT_OK })
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

fn parse_fidelity(value: &str) -> anyhow::Result<Fidelity> {
    Fidelity::parse(value).ok_or_else(|| {
        anyhow::anyhow!("unknown --fidelity `{value}`; use full, agent, standard or plain")
    })
}

fn parse_use_loffice(value: &str) -> anyhow::Result<UseLoffice> {
    UseLoffice::parse(value).ok_or_else(|| {
        anyhow::anyhow!("unknown --use-loffice `{value}`; use auto, never or require")
    })
}

fn exit_code_for(error: &anyhow::Error) -> u8 {
    match error.downcast_ref::<docsai_convert::ConvertError>() {
        Some(docsai_convert::ConvertError::Unsupported { .. }) => EXIT_UNSUPPORTED,
        Some(docsai_convert::ConvertError::UnknownFormat(_)) => EXIT_UNSUPPORTED,
        Some(docsai_convert::ConvertError::Loffice { .. }) => EXIT_INPUT,
        Some(_) => EXIT_INPUT,
        None => EXIT_INPUT,
    }
}

fn hint_for_error(error: &anyhow::Error) {
    if let Some(docsai_convert::ConvertError::UnknownFormat(path)) =
        error.downcast_ref::<docsai_convert::ConvertError>()
    {
        eprintln!(
            "docsai: could not detect the format of `{path}`. \
Pass a known extension or check the file is not corrupt. See `docsai formats`."
        );
    } else if let Some(docsai_convert::ConvertError::Unsupported { from, to }) =
        error.downcast_ref::<docsai_convert::ConvertError>()
    {
        eprintln!(
            "docsai: conversion {} → {} is not supported in this build. See `docsai formats`.",
            from.as_str(),
            to.as_str()
        );
    } else if let Some(docsai_convert::ConvertError::Loffice { .. }) =
        error.downcast_ref::<docsai_convert::ConvertError>()
    {
        eprintln!(
            "docsai: install LibreOffice and ensure `soffice` is on PATH, \
or set DOCSAI_LIBREOFFICE, or pass --use-loffice never for the native degraded path."
        );
    }
}
