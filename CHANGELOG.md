# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `docsai inspect` — structure report (metadata, styles, sections/sheets, media, stats)
  without converting; `--json` for the machine-readable shape shared with MCP later.
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

## [0.1.0] — 2026-08-01

### Added

- Initial public workspace: Phases 0–5 core path (docx/odt/xlsx/ods ⇄ DocMark, xls read,
  legacy `.doc` read with optional LibreOffice fallback).
- `docsai convert`, `formats`, and `roundtrip` commands.
