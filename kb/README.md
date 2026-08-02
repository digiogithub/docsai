# Knowledge base for `docsai`

Working documentation on **what is already built**, how it is organized, and what to keep in
mind when tackling the next phases. It complements — does not replace — the design documentation
in [`docs/`](../docs/), which describes the full project as originally conceived.

Practical difference between the two folders:

| Folder | Answers |
|---|---|
| [`docs/`](../docs/) | What we want to build and why: analysis, DocMark spec, architecture, phased plan |
| `kb/` (this one) | What is built today, how, and what is already known about what comes next |

## Index

| Document | Contents |
|---|---|
| [01 — Summary of phases 0 and 1](01-phases-0-1-summary.md) | What was delivered, acceptance criteria, what was left out and why |
| [02 — Project structure](02-project-structure.md) | Crates, modules, dependency rules, and where each piece enters |
| [03 — Technical decisions](03-technical-decisions.md) | The non-obvious decisions, with their rationale and cost |
| [04 — Considerations for the next phases](04-next-phases.md) | What Phase 2 and later will already find solved, and what awaits them |
| [05 — Phase 3 spreadsheets](05-phase-3-spreadsheets.md) | XLSX/XLS ⇄ DocMark: what landed, decisions, gaps |

## Status in one line

**Phases 0–3 closed for the core path**: docx ⇄ DocMark (Phases 1–2) and xlsx ⇄ DocMark
plus xls read (Phase 3). `docsai convert` / `roundtrip` cover both text and spreadsheet packages.
ODF and MCP remain later phases.

## Quick checks

```bash
cargo test --workspace                                  # workspace tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python3 corpus/generate.py --check                      # corpus is up to date
cargo run -p docsai-cli -- formats                      # real support matrix of the binary
```
