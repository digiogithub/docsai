# docsai

**Bidirectional converter between Office / LibreOffice documents and extended Markdown, written in Rust.**

`docsai` is a single cross-platform binary (Windows, Linux, macOS) that converts office documents to an extended Markdown profile — **DocMark** — designed to keep as much information as possible (styles, images, properties, formulas) and to allow the **reverse conversion with minimal format loss**. It can be used as a CLI tool or as an **MCP (Model Context Protocol) server over stdio**, for integration with AI assistants such as Claude.

> **Project status: Phases 0–7 completed for the core path.**
> `.docx` / `.odt` ⇄ DocMark, `.xlsx` / `.ods` ⇄ DocMark, `.xls` read, and legacy
> `.doc` read (native degraded text, or full fidelity via LibreOffice headless).
> Phase 6 adds `inspect`, batch `--out-dir`, stdin/stdout pipelines, `--style-map`,
> and `cargo-dist` release packaging. Phase 7 adds the MCP stdio server
> (`docsai mcp`), now with seven tools. That plan
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
| PowerPoint OOXML | `.pptx` | ✅ `inspect` | 🕓 v2 P15 | Slides, placeholders, notes, tables, images, charts; original package skeleton preserved. Reads into the IR and `docsai inspect` reports the slide inventory; converting to DocMark waits for the DocMark-P profile (v2 P14) |
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
docsai convert report.docx --fidelity agent        # the projection an agent edits from
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
docsai read report.docx --select s7-s9             # just those nodes, as DocMark
docsai read report.docx --select '#n7,text:riesgo' # by id, or by what it says
docsai search report.docx "riesgo"                 # where it says that, with context
docsai search report.docx "riesgo" --json --limit 5 # machine-readable, capped
docsai tokens report.docx                          # what the document costs an LLM
docsai tokens report.docx --fidelity plain --json  # per-node costs, machine-readable
docsai formats                                      # support matrix for this binary
docsai roundtrip report.docx
docsai mcp                                          # MCP server over stdio (Phase 7)
```

Fidelity levels (`--fidelity`, spec §6): `full` (default, round-trip grade),
`agent` (a projection for programs, below), `standard` (rich Markdown without
catalogues or raw-blocks) and `plain` (pure CommonMark+GFM).

`--fidelity agent` (spec §6.1) is what an agent reads before editing. It keeps the
text, the structure, every node id and a stub for everything opaque, and drops what
no program edits: the style and list catalogues, indents and spacing, page geometry,
image EMUs, column widths. On the corpus that is a **62–75 % cut against `full`** for
documents whose cost is their formatting — and almost nothing for a document whose
cost is its prose, which is what `docsai outline` and selectors are for. It says
`fidelity: agent` in its front matter: read it whole, write it back node by node.

Units (`--precision`, spec §2): a length is written in the unit of **what it
measures** — points for layout (indents, margins, column widths), pixels for
bitmaps and drawing offsets — and zero carries no unit. A unit is only used when
it names the length *exactly*: `--precision N` (default 2) sets how many decimals
it may use, and `emu` is the escape hatch when none of them fits. It buys
readable units, never rounding, so the round-trip tolerance for a length is zero.

Repeated formatting (spec §3.7): what a style implies is never written — not the
paragraph's, not the run's, not what a style inherits from its parent — and what no
style implies but repeats anyway is written **once**. A pattern used three times or
more is interned in the front matter under `attribute-sets:` and referenced by class
(`{.g1}`); the reader expands it before anything reads the block, so the document
means exactly what it meant. On a document written without styles that is a 15 %
cut at `full` and 22 % at `standard`.

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

Partial reads (`docsai read --select`, spec §2.1): `outline` says where the
paragraph is; this hands it over and nothing else, as **valid self-contained
DocMark**. Selectors are `s4` and `s7-s9` (positions in the order `outline`
prints them), `#n7`, `type:heading` and `text:foo`; comma-separated terms are
unioned and the output always comes back in document order. The body is what the
whole document wrote for those nodes, byte for byte, and the front matter is the
minimum needed to parse and re-write it — no metadata, no page geometry, no
catalogues. It declares `partial: true` and an etag per node, so an edit can be
written back with a precondition and nobody mistakes a fragment for the document:
writing one back whole is a **severe warning**, every time.

```text
$ docsai read report.docx --select s2-s3
---
docmark: "1.1"
source-format: docx
next-id: 26
partial: true
etags:
  n2: "5e8876"
  n3: "8eb3e5"
---

## \1. Estado de alcance del proyecto {#n2}

### Conclusion de alcance del proyecto {#n3}
```

Finding text (`docsai search <in> <query>`): the answer to *where does it say
that*, without paying for the document. Matching is a case-insensitive literal
over the text a conversion would write, and the unit is the DocMark **block**,
not the addressed node — ordinary prose paragraphs carry no id (spec §11.1:
"reached by relative path"), and a search that only looked at addressed nodes
would find headings and nothing else. A block that carries an id is reported at
it, with the selector that reads it back; one that does not is reported relative
to the last id before it (`n12.b2`). `--context N` sets how many characters
either side of a match are quoted, `--limit N` how many blocks are listed — the
rest are **counted, not dropped**.

```text
$ docsai search report.docx "rendimiento medido" --limit 2
s12 #n12 heading 14 tokens ×1
  …## \6. Estado de «rendimiento medido»…
n12.b1 text 81 tokens ×4
  …El equipo revisa «rendimiento medido» en cada iteracion y deja constancia…
… 9 more block(s) not listed (--limit)
35 match(es) in 11 block(s) · hits 386 tokens · document 9083 tokens (4.2 %)
```

A hit that names a selector composes with the previous command —
`docsai read --select '#n12'` returns exactly what the hit pointed at. A
relative hit names no selector, because `read --select` has no `.bN` term yet:
saying so is the point, rather than handing back an address that would read
something else.

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

Tools: `outline_document`, `search_document`, `read_selection`,
`convert_to_markdown`, `convert_from_markdown`, `inspect_document`,
`list_supported_formats`. Each accepts a filesystem `path` **or**
`content_base64` + `filename`. Logs always go to **stderr**; stdout is the
JSON-RPC channel only.

The first three are the ones to reach for. They are the MCP face of `docsai
outline`, `docsai search` and `docsai read --select`, and together they answer
"change the third heading of this report" without ever sending the report:

```text
outline_document { path }                     → n1 heading, n12 heading, … (3.8 % of the document)
search_document  { path, query: "riesgo" }    → s12 #n12, select "#n12", the words around it
read_selection   { path, select: "#n12" }     → that node as DocMark, with its etag
convert_from_markdown { markdown, target_format: "docx" }
```

`convert_to_markdown` is the whole document, and the expensive path.
Its `include_images` chooses what image payload comes back:

| Value | The client gets |
|---|---|
| `none` | the count and the byte total, nothing else |
| `refs` (**default**) | name, MIME type and size of each image |
| `thumbnails` | the above plus a PNG downscaled to 256 px, to actually look at |
| `full` | the original bytes, base64 |

The markdown is identical at every rung — the body always keeps its
`![](assets/…)` links — so the choice is about cost, never about fidelity. On a
document with one 1200 × 900 screenshot the response goes from 906 709 bytes at
`full` to 2 289 at `refs`. Media can still be written to disk with
`assets=files` + `assets_dir`.

> **Breaking change** (plan v2 Phase 11): `include_images` defaults to `refs`,
> where `convert_to_markdown` used to return every image inline. Pass
> `include_images: "full"` for the old behaviour; clients that already passed
> `assets: "inline-base64"` keep working unchanged.

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
