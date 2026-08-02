# 09 — Phase 6: full CLI and distribution

## Status

**Core path implemented.** The CLI product surface from `architecture.md` §5 is in place
for local use; release automation is configured via `cargo-dist` (tag-driven GitHub
releases with shell/powershell installers).

## Delivered

| Item | Where |
|---|---|
| `docsai inspect` (+ `--json`) | `docsai-convert::inspect`, `docsai-cli` |
| Batch `convert` + `--out-dir` + rayon | `docsai-convert::batch` |
| Stdin/stdout `-` | `pipeline::{is_stdin_path,is_stdout_path,convert_file}` |
| `--style-map` publication mode | `docsai-convert::style_map` |
| `--max-cells` | `ConvertOptions::max_cells` |
| Actionable error hints | `docsai-cli` `hint_for_error` |
| `cargo-dist` workspace config | `dist-workspace.toml`, crate `package.metadata.dist` |
| CHANGELOG (keep-a-changelog) | `CHANGELOG.md` |
| README / `--help` polish | `README.md`, clap `long_about` |

## Style-map format

Flat YAML-ish map (comments allowed):

```text
Heading1: h1
"Heading 1": h1
Title: h1
SourceCode: code-block
Comment: ignore
BodyText: p
```

Targets: `h1`–`h6`, `p`/`paragraph`, `ignore`/`skip`, `code-block`/`pre`.
Matching is case-insensitive on style **id** and **name**. Applying a map emits a
`Degraded` warning and is unidirectional by design (spec §5).

## Inspect JSON shape

Stable fields used by CLI and reserved for MCP `inspect_document`:

- `path`, `source-format`, `kind` (`text` \| `workbook`)
- `meta`, `styles[]`, `sections?`, `sheets?`, `media[]`
- `stats` (`ConversionStats`), `warnings[]`

## Batch behaviour

- Multiple inputs **or** `--out-dir` enter batch mode.
- Outputs are `{stem}.dmk.md` (or `.{fmt}` when `--to` selects an Office target).
- Shared `assets/` directory under `--out-dir` by default.
- Exit code is the worst item code (`0`/`1`/`2`/`3`); `--strict` promotes any warning.

## Distribution

- Binary crate: `docsai-cli` → bin name `docsai`.
- Library crates set `package.metadata.dist.dist = false`.
- Targets: `x86_64`/`aarch64` macOS, `x86_64`/`aarch64` Linux GNU, `x86_64` Windows MSVC.
- Installers: shell + powershell via cargo-dist.
- Publish flow: tag `vX.Y.Z` → cargo-dist GitHub workflow builds and uploads artifacts.

## Acceptance criteria checklist

- [x] CLI surface matches architecture §5 for convert/inspect/roundtrip/formats.
- [x] Batch over many corpus files produces a correct summary.
- [x] User-facing errors include a next step where possible.
- [ ] One-command install verified on a clean machine per OS (needs a tagged release).
- [ ] crates.io publish (deferred until the first release tag).

## Out of scope / follow-ups

- Homebrew tap / Scoop / winget formulas (cargo-dist can grow these later).
- `docsai mcp` subcommand — delivered in Phase 7 (`kb/10-phase-7-mcp.md`).
- Criterion benches and fuzz (Phase 8).
