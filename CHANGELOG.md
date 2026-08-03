# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
