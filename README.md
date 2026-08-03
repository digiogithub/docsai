# docsai

**Bidirectional converter between Office / LibreOffice documents and extended Markdown, written in Rust.**

`docsai` is a single cross-platform binary (Windows, Linux, macOS) that converts office documents to an extended Markdown profile — **DocMark** — designed to keep as much information as possible (styles, images, properties, formulas) and to allow the **reverse conversion with minimal format loss**. It can be used as a CLI tool or as an **MCP (Model Context Protocol) server over stdio**, for integration with AI assistants such as Claude.

> **Project status: Phases 0–7 completed for the core path.**
> `.docx` / `.odt` ⇄ DocMark, `.xlsx` / `.ods` ⇄ DocMark, `.xls` read, and legacy
> `.doc` read (native degraded text, or full fidelity via LibreOffice headless).
> Phase 6 adds `inspect`, batch `--out-dir`, stdin/stdout pipelines, `--style-map`,
> and `cargo-dist` release packaging. Phase 7 adds the MCP stdio server
> (`docsai mcp`) with four tools. That plan
> ([`docs/development-plan.md`](docs/development-plan.md)) is **delivered and superseded**.
>
> **Next: [development plan v2](docs/development-plan-v2.md)** — agent-native docsai
> (stable node ids, `outline`/`read --select`/`search`, `--fidelity agent`, measured token
> budget, patch editing) and presentations (`.pptx` ⇄ DocMark, `.odp` ⇄ DocMark, `.ppt`
> read). Analysis: [`docs/technical-analysis-presentations.md`](docs/technical-analysis-presentations.md).

```bash
cargo run -p docsai-cli -- convert report.docx -o report.dmk.md
cargo run -p docsai-cli -- inspect report.docx
cargo run -p docsai-cli -- formats
cargo run -p docsai-cli -- mcp
```
---

## Supported formats (target)

✅ = works today · 🕓 = planned, with the phase that lands it · ➖ = out of scope

| Format | Extension | Read | Write | Notes |
|---|---|---|---|---|
| Word OOXML | `.docx` | ✅ | ✅ | Styles, images, tables, lists, headers/footers, footnotes, fields, properties |
| Word binary | `.doc` | ✅ | ➖ | Read only: native degraded text, or LibreOffice headless → docx (`--use-loffice`) |
| Excel OOXML | `.xlsx` | ✅ | ✅ | Values **and formulas**, number formats, merged cells, anchored images |
| Excel binary | `.xls` | ✅ | ➖ | Read only (calamine) |
| OpenDocument Text | `.odt` | ✅ | ✅ | Free equivalent of `.docx` |
| OpenDocument Spreadsheet | `.ods` | ✅ | ✅ | Free equivalent of `.xlsx` |
| PowerPoint OOXML | `.pptx` | 🕓 v2 P13 | 🕓 v2 P15 | Slides, placeholders, notes, tables, images, charts; original package skeleton preserved |
| PowerPoint binary | `.ppt` | 🕓 v2 P19 | ➖ | Read only: native degraded text, or LibreOffice headless → pptx |
| OpenDocument Presentation | `.odp` | 🕓 v2 P18 | 🕓 v2 P18 | Free equivalent of `.pptx` |
| Extended Markdown | `.dmk.md` | ✅ | ✅ | Pivot format **DocMark** (superset of CommonMark + GFM) |

`docsai formats` prints this same matrix for what the binary can actually do.

Writing `.doc` and `.xls` (legacy binary formats) is deliberately out of scope: the recommended output path into the Microsoft ecosystem is always OOXML (`.docx` / `.xlsx`).

## What is DocMark?

DocMark is an extended Markdown profile defined in this project (see [`docs/docmark-specification.md`](docs/docmark-specification.md)). It is **human-readable and hand-editable Markdown** that renders reasonably on GitHub or any CommonMark viewer, but adds metadata layers so information is not lost:

- **YAML front matter** with document properties (title, author, language…) and the original **style catalogue**.
- **Inline and block attributes** `{#id .class key="value"}` (Pandoc-compatible syntax) to attach styles, image dimensions, cell properties, and more.
- **Fenced containers** `::: {...}` for sections, text boxes, headers and footers.
- **Extended tables** with per-cell metadata (formulas, types, number formats, merges) for spreadsheets.
- **External assets**: images are extracted to an `assets/` directory (deduplicated by content hash) and referenced with a full geometry attribute model: display and native size, position and anchoring (inline, floating with wrap and z-order, or cell-anchored in spreadsheets), rotation, crop, flip, alternative text and hyperlink.
- **Fidelity hatch** (`raw-block`) for fragments with no Markdown representation, kept opaque and restored on the reverse conversion.

Minimal example:

```markdown
---
docmark: "1.0"
source-format: docx
title: "Annual Report"
styles:
  Heading1: { font: "Calibri Light", size: 16pt, color: "#2E74B5" }
---

# Annual Report {.Heading1}

Text with **bold** and [color]{color="#FF0000"} custom colour.

![Sales chart](assets/img-001.png){width=450px height=300px anchor=inline}
```

## Install

From source:

```bash
cargo install --path crates/docsai-cli
```

Release binaries and installers (shell / PowerShell) are produced by
[`cargo-dist`](https://opensource.axo.dev/cargo-dist/) when a version tag is
pushed. See [`CHANGELOG.md`](CHANGELOG.md) and the GitHub Releases page.

## Usage (CLI)

```bash
docsai convert report.docx -o report.dmk.md      # extracts assets/ next to the .md
docsai convert report.docx                         # DocMark on stdout
docsai convert report.docx -o -                    # same, explicit stdout
docsai convert - --to docmark < report.docx        # stdin → stdout pipeline
docsai convert report.docx --fidelity plain        # clean CommonMark, for LLM/RAG
docsai convert report.docx -o out.md --json        # conversion report as JSON
docsai convert *.docx --out-dir md/                # batch (parallel) into a folder
docsai convert report.docx --ids never             # DocMark 1.0 output, no node ids
docsai convert report.docx --raw inline            # raw-block bytes in the body, not aside
docsai convert report.docx --style-map map.yaml    # publication mode (spec §5)
docsai convert sheet.xlsx --max-cells 100000       # refuse oversized workbooks
docsai convert legacy.doc -o legacy.dmk.md         # .doc: LO if installed, else native text
docsai convert legacy.doc --use-loffice never      # force native degraded path
docsai convert legacy.doc --use-loffice require    # fail if LibreOffice is missing
docsai inspect report.docx                         # metadata, styles, media, stats
docsai inspect report.docx --json                  # same, machine-readable
docsai outline report.docx                         # map of addressable nodes + cost
docsai outline report.docx --depth 1 --json        # top level only, machine-readable
docsai tokens report.docx                          # what the document costs an LLM
docsai tokens report.docx --fidelity plain --json  # per-node costs, machine-readable
docsai formats                                      # support matrix for this binary
docsai roundtrip report.docx
docsai mcp                                          # MCP server over stdio (Phase 7)
```

Fidelity levels (`--fidelity`, spec §6): `full` (default, round-trip grade),
`standard` (rich Markdown without catalogues or raw-blocks) and `plain` (pure
CommonMark+GFM).

Node ids (`--ids`, spec §11.1): at `--fidelity full` the output is **DocMark 1.1**
— addressable nodes carry `{#n7}` and the front matter declares `next-id`, so an
agent can point at a node and keep pointing at it across edits. Ids are never
renumbered on insertion and never reused after deletion. `--ids preserve` writes
back only the ids a document already had, `--ids never` reproduces the DocMark 1.0
shape. The lossy levels default to `never`, and `plain` never carries ids.

Raw-blocks (`--raw`, spec §7): what no DocMark construct can express — SmartArt,
OMML maths, signed content — travels as opaque source bytes. By default those
bytes go to a **sidecar**, `assets/_raw/<id>.xml`, and the body keeps a one-line
stub naming it, so reading the document does not mean paying for markup nobody can
edit. `--raw inline` puts the payload back in a fenced block, which is what a
self-contained single file needs. Either way nothing is lost: a **missing sidecar
is an error**, not a warning.

Document map (`docsai outline`): the tree of addressable nodes — id, kind, a
~60-character preview and the measured cost of each — so an agent can decide what
*not* to read. The tree follows containment (a footnote hangs from its paragraph,
a nested list from its parent); heading level shows in the preview. `--depth N`
keeps the first N levels.

```text
n1 heading 13 # Informe tecnico de seguimiento
n2 heading 17 ## 1. Estado de alcance del proyecto
25 nodes · outline 345 tokens · document 9158 tokens (3.8 %)
```

Token budget (`docsai tokens`): the cost of the document is **measured** with a
real BPE tokenizer (`o200k_base`, embedded — no network, no Python), never
estimated from the file size. The report splits front matter from body and lists
the heaviest addressed nodes, each counted over the exact DocMark it wrote:

```text
report.docx  docx  fidelity=full  encoding=o200k_base
  total             701 tokens (2063 bytes)
  front matter      565
  body              137
```

Nested nodes are counted more than once on purpose (a section's cost includes its
headings'), so the per-node numbers do not sum to the total.

Style maps (`--style-map`, spec §5) are **unidirectional** publication helpers:

```text
Heading1: h1
Title: h1
SourceCode: code-block
Comment: ignore
```

Legacy `.doc` policy (`--use-loffice`): `auto` (default — use LibreOffice when
found), `never` (native piece-table extractor only), `require` (error if
LibreOffice is missing). Override the binary with `DOCSAI_LIBREOFFICE`.

Exit codes: `0` success, `1` conversion with losses (`--strict` also treats minor
warnings as failures), `2` input error, `3` unsupported format. Logs always go to
stderr (`RUST_LOG` / `--verbose`); stdout stays free for DocMark or `--json`.
## MCP server

```bash
docsai mcp        # starts the MCP server over stdio
```

Registration in an MCP client (e.g. Claude Desktop / Claude Code / MCP Inspector):

```json
{
  "mcpServers": {
    "docsai": { "command": "docsai", "args": ["mcp"] }
  }
}
```

Tools: `convert_to_markdown`, `convert_from_markdown`, `inspect_document`,
`list_supported_formats`. Each tool accepts a filesystem `path` **or**
`content_base64` + `filename`. Asset delivery defaults to `inline-base64`
(optional `assets=files` with `assets_dir`). Logs always go to **stderr**;
stdout is the JSON-RPC channel only.

Environment:

| Variable | Default | Meaning |
|---|---|---|
| `DOCSAI_MCP_MAX_INPUT_BYTES` | `52428800` (50 MiB) | Cap on path size and decoded base64 |
| `DOCSAI_MCP_TIMEOUT_SECS` | `120` (`0` = off) | Per-tool wall-clock timeout |

Details in [`docs/architecture.md`](docs/architecture.md) §6 and [`kb/10-phase-7-mcp.md`](kb/10-phase-7-mcp.md).

## Project documentation

| Document | Contents |
|---|---|
| [`docs/technical-analysis.md`](docs/technical-analysis.md) | Format analysis, evaluated Rust libraries, prior open-source projects (Pandoc, MarkItDown, Docling, mammoth…), decisions and risks |
| [`docs/technical-analysis-presentations.md`](docs/technical-analysis-presentations.md) | Presentations (`.pptx`/`.ppt`/`.odp`) and agent context economics: format anatomy, crate evaluation, token-cost analysis, risks |
| [`docs/docmark-specification.md`](docs/docmark-specification.md) | DocMark format specification (extended Markdown) v1.0, plus the committed 1.1 / 1.2 bumps |
| [`docs/architecture.md`](docs/architecture.md) | Software architecture: crate workspace, intermediate document model (IR), CLI, MCP server |
| [`docs/development-plan-v2.md`](docs/development-plan-v2.md) | **Current plan** (Phases 10–20): agent-native primitives and presentations |
| [`docs/development-plan.md`](docs/development-plan.md) | Plan v1 (Phases 0–9), delivered and superseded — historical record |
| [`CHANGELOG.md`](CHANGELOG.md) | Keep-a-changelog release notes |
| [`AGENTS.md`](AGENTS.md) | Operational guide for developers and AI agents working in this repository |
| [`corpus/README.md`](corpus/README.md) | The test corpus: what each document isolates and how it is regenerated |
| [`docs/spikes/`](docs/spikes/) | Risk-spike reports, with the decision that closed each one |
| [`kb/`](kb/) | Knowledge base: what is built, how it is structured, technical decisions and what later phases will face |

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python3 corpus/generate.py --check     # the corpus is generated, not hand-drawn
```

Golden files live next to the corpus (`corpus/docx/*.expected.dmk.md`). To update them:
`DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens`, and **review the diff**.

## Design principles

1. **Single binary, no external runtime**: pure Rust libraries whenever possible; external fallbacks (LibreOffice headless for `.doc`) are optional and detected at runtime, never required.
2. **Pivot IR**: every format converges on an intermediate document model (inspired by Pandoc's AST and DoclingDocument); converters never talk to each other directly.
3. **Measurable fidelity**: format loss is not estimated, it is measured — the `roundtrip` command and the round-trip test suite are part of the product.
4. **Markdown readable first, complete second**: extended metadata degrades gracefully; a normal Markdown viewer shows a useful document even if it ignores the attributes.
5. **User data is never lost silently**: what cannot be represented is kept in raw blocks or reported as an explicit warning.
6. **Context cost is measured, not estimated** (plan v2): tokens per document and tool calls per task are tracked in CI like any other budget, because the primary consumer of this tool is an AI agent with a finite context window.

## License

See [LICENSE](LICENSE).
