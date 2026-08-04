# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Delta emission against the whole inheritance chain** (plan v2 Phase 11): a run is now
  resolved against its **paragraph's** style before its own, which is the middle of the OOXML
  cascade and the level the serializer used to skip — text typed after applying a style carries
  that style's run properties again in `w:rPr`, and all of it was being written out. A style in
  the catalogue emits only what its `based-on` chain does not already say; the default paragraph
  style is no longer named on a paragraph; a heading omits the class its `#` level already names
  (and the parser puts it back). Every omission is reversible: a value equal to the inherited one
  resolves to the inherited one.
- Corpus fixture `docx/redundant-formatting.docx`, which carries that redundancy on purpose —
  600 → 528 tokens with the change, and the existing corpus 24 883 → 24 667 (−0.9 %), small only
  because generated fixtures were already clean.
- **`--fidelity agent`** (plan v2 Phase 11), a fourth level and the first that is a *projection*
  rather than a conversion: text, structure, node ids and raw stubs, without the style and list
  catalogues, indents, spacing, page geometry, image geometry or column widths. A sheet keeps its
  formulas and merges — those are what a sheet is. The output declares `fidelity: agent` in its
  front matter and is meant to be read whole and written back node by node, so ids are assigned
  as at `full`. Measured on the corpus: 62–75 % fewer tokens than `full` on 24 of the 25 text
  documents; the exception is the one whose cost is prose rather than formatting.
- `corpus/token-budget.md` now has an `agent` column, so the projection's cost is gated in CI
  like every other level.
- **Raw-block sidecars** (plan v2 Phase 11): at `--fidelity full` the bytes of a raw-block go
  to `assets/_raw/<id>.xml` and the body keeps a stub, `::: {.raw … src="assets/_raw/r7.xml"}`.
  `docsai convert --raw inline|sidecar` (default `sidecar`) chooses; `--raw inline` restores the
  1.0 form. A missing or unreachable sidecar is a typed error, never a silent loss.
  `docsai_docmark::raw::raw_sidecars` gives a caller the files a serialisation refers to.
- Corpus fixture `docx/fields-raw.docx` now carries block-level OMML maths, so the raw-block
  path — and its sidecar — is exercised end to end by the corpus instead of only by unit tests.
- **DocMark 1.1 — stable node addressing** (plan v2 Phase 10): addressable nodes carry
  `{#n7}` and the front matter declares `next-id`, a monotonic counter that is never
  renumbered on insertion and never reused after deletion. Ids ride in the attribute
  block the node already has (headings, container paragraphs, images, tables, complex
  table rows, sheets, multi-section containers), on the first item for lists
  (`list-id=`) and on the reference for footnotes (`[^1]{#n9}`).
- `docsai convert --ids assign|preserve|never`; the default is `assign` at
  `--fidelity full` and `never` at the lossy levels. `plain` never carries ids.
- `docsai-model::addressing`: `NodeId`, `Etag`, `IdPolicy`, the `Addressable` trait and
  the document walkers (`assign_ids`, `for_each_addressable`, `node_ids`). Etags are a
  6-character hash of the node's normalised content, derived on demand and never stored.
- **`docsai tokens <in> [--fidelity …] [--ids …] [--top N] [--json]`** — the cost of a
  document measured with a real BPE tokenizer (`tiktoken-rs`, `o200k_base`, vocabulary
  embedded: no network, no Python), split into front matter and body, plus the cost of
  every addressed node over the exact DocMark it wrote. Rationale for the tokenizer
  choice in `docs/technical-analysis.md` §4.4.
- **`docsai outline <in> [--depth N] [--fidelity …] [--json]`** — the tree of addressable
  nodes with id, kind, a ~60-character preview and the measured token cost of each, plus what
  the whole document would cost. On the largest corpus document the outline is 3.8 % of the
  document's own tokens (Phase 10 budget: under 5 %).
- **Corpus token budget** `corpus/token-budget.md`: what every corpus document costs at each
  fidelity level, committed as a golden and checked in CI. An update that inflates the corpus
  total by more than 5 % is refused unless `DOCSAI_ACCEPT_TOKEN_INFLATION=1` says so.
- Corpus fixture `docx/long-report.docx` (~9 000 tokens): the first corpus document sized for
  measuring rather than for isolating a trait.
- `docsai_docmark::serialize_traced` and `NodeFragment`: the same output as `serialize`,
  byte for byte, plus what each addressed node contributed, with `descendants` recording how
  many fragments it contains so the flat list rebuilds into a tree. `docsai-convert::tokens`
  (`token_report`, `token_report_path`) and `docsai-convert::outline` (`outline`,
  `outline_path`) build on it.

- **Phase 7 MCP server**: `docsai mcp` over stdio (`rmcp`) with tools
  `convert_to_markdown`, `convert_from_markdown`, `inspect_document`, and
  `list_supported_formats`. Path and base64 input modes, inline or on-disk assets,
  `DOCSAI_MCP_MAX_INPUT_BYTES` / `DOCSAI_MCP_TIMEOUT_SECS` limits.
- `docsai-convert::service` helpers for in-memory / bytes conversion shared by MCP.
- `docsai inspect` — structure report (metadata, styles, sections/sheets, media, stats)
  without converting; `--json` for the machine-readable shape shared with MCP.
- Batch conversion: `docsai convert *.docx --out-dir md/` with rayon parallelism and an
  aggregated summary (`--json` supported).
- Stdin/stdout pipelines: pass `-` as the input and/or `--output -` for DocMark text.
- `--style-map <file>` publication mode (DocMark spec §5): map source styles to `h1`–`h6`,
  `p`, `ignore`, or `code-block` (unidirectional).
- `--max-cells N` safety cap for workbook conversions.
- Actionable CLI error hints for unknown formats, unsupported conversions, and LibreOffice.
- `cargo-dist` configuration and release installers (shell/powershell) for the five
  primary targets.

### Changed

- `--fidelity full` output is now **DocMark 1.1** and declares `next-id`; documents
  without ids (and `--ids never`) still declare `1.0`. A 1.0 document parses unchanged
  and gains ids on its next write. MCP `convert_to_markdown` inherits the new default.
- `Inline::Footnote` now carries a `Footnote { id, blocks }` and `Inline::Image` boxes
  its `ImageRef` (library API change; serialised JSON is unchanged).
- CLI `--help` documents the full Phase 6 command surface with examples.
- README usage section covers inspect, batch, style maps, and install paths.
- Workspace MSRV raised to **1.88** (required by `rmcp` 3.x).

## [0.1.0] — 2026-08-01

### Added

- Initial public workspace: Phases 0–5 core path (docx/odt/xlsx/ods ⇄ DocMark, xls read,
  legacy `.doc` read with optional LibreOffice fallback).
- `docsai convert`, `formats`, and `roundtrip` commands.
