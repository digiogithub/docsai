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
| [06 — Phase 2 round-trip closure](06-phase-2-roundtrip-closure.md) | Residual DOCX writer fidelity + corpus idempotence |
| [07 — Phase 4 ODF](07-phase-4-odf.md) | ODT/ODS ⇄ DocMark: package, de-automatization, OpenFormula |
| [08 — Phase 5 legacy DOC](08-phase-5-legacy-doc.md) | `.doc` native degraded read + LibreOffice fallback |
| [09 — Phase 6 CLI & distribution](09-phase-6-cli.md) | inspect, batch, style-map, stdin/stdout, cargo-dist |
| [10 — Phase 7 MCP](10-phase-7-mcp.md) | `docsai mcp`, four tools, path/base64, limits |
| [11 — Plan v2 on-ramp](11-plan-v2-onramp.md) | What agent-native + presentations work reuses, what is new, traps that will bite again |
| [13 — Phase 10 plan](13-phase-10-addressing-plan.md) | Increments A–F of stable addressing and the token budget |
| [14 — Phase 10 A/B](14-phase-10-addressing-core.md) | `docsai-model::addressing`, ids in the IR, etags, risk-P7 property tests |
| [15 — Phase 10 C](15-phase-10-docmark-1-1.md) | DocMark 1.1: `{#n7}` emission and parse, `next-id`, `--ids` |
| [16 — Phase 10 D](16-phase-10-tokens.md) | `docsai tokens`: tokenizer decision, traced fragments, first budget numbers |
| [17 — Phase 10 E](17-phase-10-outline.md) | `docsai outline`: the containment tree, previews, the 5 % budget |
| [18 — Phase 10 F](18-phase-10-token-gate.md) | The corpus token budget golden, the CI gate, and the phase close |
| [19 — Phase 11 plan](19-phase-11-plan.md) | Projections, raw sidecar, `--fidelity agent`, `read --select`: the increments |
| [20 — Phase 11 A](20-phase-11-raw-sidecar.md) | Raw-block bytes move to `assets/_raw/`, and a missing sidecar is an error |
| [21 — Phase 11 B](21-phase-11-agent-fidelity.md) | `--fidelity agent`: the projection rule, what it enforces, and what the plan's criterion got wrong |
| [22 — Phase 11 C](22-phase-11-delta-emission.md) | Delta emission against the whole cascade, and the fixture that made it measurable |
| [23 — Phase 11 D](23-phase-11-attr-dictionary.md) | Repeated attribute patterns interned as `{.g1}`, and why expansion happens before interpretation |

## Status in one line

**Phases 0–7 closed for the core path**: docx ⇄ DocMark with full corpus round-trip
identity (Phases 1–2, including floating DrawingML and footnote bodies), xlsx ⇄
DocMark plus xls read (Phase 3), odt/ods ⇄ DocMark (Phase 4), legacy `.doc`
read (Phase 5: native degraded + optional LibreOffice → docx), the full CLI
product surface (Phase 6: `inspect`, batch `--out-dir`, `--style-map`, stdin/stdout,
`cargo-dist`), and the MCP stdio server (Phase 7: four tools, path/base64).

**Phase 10 of plan v2 is closed**: DocMark 1.1 stable node ids and derived etags, `docsai tokens`
and `docsai outline` measured with a vendored BPE tokenizer, and the corpus token budget
(`corpus/token-budget.md`) gated in CI.

**Phase 11 is in progress**: raw-block bytes live in `assets/_raw/` sidecars (11-A) and
`--fidelity agent` projects a document down to what a program can edit (11-B); no attribute
the inheritance chain already implies is written (11-C), and a pattern that repeats anyway is
written once in the front matter and referenced by class (11-D). Next are readable units with
tolerance (11-E).

**Plan v1 is delivered and deprecated.** Active plan:
[`docs/development-plan-v2.md`](../docs/development-plan-v2.md) — agent-native primitives
(Phases 10–11), presentations (12–16, 18–19), patch editing (17), hardening + v2.0 (20).
Start at [`11-plan-v2-onramp.md`](11-plan-v2-onramp.md) before touching any of it.

## Quick checks

```bash
cargo test --workspace                                  # workspace tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python3 corpus/generate.py --check                      # corpus is up to date
cargo run -p docsai-cli -- formats                      # real support matrix of the binary
cargo run -p docsai-cli -- inspect corpus/docx/basic-text.docx
cargo run -p docsai-cli -- tokens corpus/docx/long-report.docx   # measured cost (Phase 10)
cargo run -p docsai-cli -- outline corpus/docx/long-report.docx  # the map an agent reads first
cargo run -p docsai-cli -- mcp   # stdio MCP server (Phase 7)
```