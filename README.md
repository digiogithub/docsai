# docsai

**Bidirectional converter between Office / LibreOffice documents and extended Markdown, written in Rust.**

`docsai` is a single cross-platform binary (Windows, Linux, macOS) that converts office documents to an extended Markdown profile — **DocMark** — designed to keep as much information as possible (styles, images, properties, formulas) and to allow the **reverse conversion with minimal format loss**. It can be used as a CLI tool or as an **MCP (Model Context Protocol) server over stdio**, for integration with AI assistants such as Claude.

> **Project status: Phases 0–2 completed.**
> `.docx` ⇄ DocMark works with styles, lists, tables, images, headers/footers,
> footnotes, fields and properties. `docsai roundtrip` checks DocMark idempotence
> after a write/read cycle. Spreadsheets arrive in Phase 3. See
> [`docs/development-plan.md`](docs/development-plan.md).

```bash
cargo run -p docsai-cli -- convert report.docx -o report.dmk.md
cargo run -p docsai-cli -- formats
```

---

## Supported formats (target)

✅ = works today · 🕓 = planned, with the phase that lands it · ➖ = out of scope

| Format | Extension | Read | Write | Notes |
|---|---|---|---|---|
| Word OOXML | `.docx` | ✅ | ✅ | Styles, images, tables, lists, headers/footers, footnotes, fields, properties |
| Word binary | `.doc` | 🕓 Phase 5 | ➖ | Read only (native parser or LibreOffice headless fallback) |
| Excel OOXML | `.xlsx` | 🕓 Phase 3 | 🕓 Phase 3 | Values **and formulas**, number formats, merged cells, anchored images |
| Excel binary | `.xls` | 🕓 Phase 3 | ➖ | Read only (calamine) |
| OpenDocument Text | `.odt` | 🕓 Phase 4 | 🕓 Phase 4 | Free equivalent of `.docx` |
| OpenDocument Spreadsheet | `.ods` | 🕓 Phase 4 | 🕓 Phase 4 | Free equivalent of `.xlsx` |
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

## Usage (CLI)

What works today:

```bash
docsai convert report.docx -o report.dmk.md      # extracts assets/ next to the .md
docsai convert report.docx                         # to stdout, without writing media beside it
docsai convert report.docx --fidelity plain        # clean CommonMark, for LLM/RAG
docsai convert report.docx -o out.md --json        # conversion report as JSON
docsai formats                                      # support matrix for this binary
```

Fidelity levels (`--fidelity`, spec §6): `full` (default, round-trip grade),
`standard` (rich Markdown without catalogues or raw-blocks) and `plain` (pure
CommonMark+GFM).

Exit codes: `0` success, `1` conversion with losses (`--strict` also treats minor
warnings as failures), `2` input error, `3` unsupported format.

Planned:

```bash
docsai convert report.dmk.md -o report.docx
docsai convert sales.xlsx -o sales.dmk.md      # Phase 3
docsai inspect report.docx                      # Phase 6
docsai roundtrip report.docx
```

## Intended usage (MCP server, Phase 7)

```bash
docsai mcp        # starts the MCP server over stdio
```

Registration in an MCP client (e.g. Claude Desktop / Claude Code):

```json
{
  "mcpServers": {
    "docsai": { "command": "docsai", "args": ["mcp"] }
  }
}
```

Planned MCP tools: `convert_to_markdown`, `convert_from_markdown`, `inspect_document`, `list_supported_formats`. Details in [`docs/architecture.md`](docs/architecture.md).

## Project documentation

| Document | Contents |
|---|---|
| [`docs/technical-analysis.md`](docs/technical-analysis.md) | Format analysis, evaluated Rust libraries, prior open-source projects (Pandoc, MarkItDown, Docling, mammoth…), decisions and risks |
| [`docs/docmark-specification.md`](docs/docmark-specification.md) | DocMark format specification (extended Markdown) v1.0-draft |
| [`docs/architecture.md`](docs/architecture.md) | Software architecture: crate workspace, intermediate document model (IR), CLI, MCP server |
| [`docs/development-plan.md`](docs/development-plan.md) | Detailed development plan in 9 phases, with deliverables, acceptance criteria, estimates and testing strategy |
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

## License

See [LICENSE](LICENSE).
