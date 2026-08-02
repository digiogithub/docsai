# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

- CLI `--help` documents the full Phase 6 command surface with examples.
- README usage section covers inspect, batch, style maps, and install paths.
- Workspace MSRV raised to **1.88** (required by `rmcp` 3.x).

## [0.1.0] — 2026-08-01

### Added

- Initial public workspace: Phases 0–5 core path (docx/odt/xlsx/ods ⇄ DocMark, xls read,
  legacy `.doc` read with optional LibreOffice fallback).
- `docsai convert`, `formats`, and `roundtrip` commands.
