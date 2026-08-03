# 18 — Phase 10 increment F: the corpus token budget, gated

Increment 10-F of [[13-phase-10-addressing-plan]], on top of [[16-phase-10-tokens]] and
[[17-phase-10-outline]]. Phase 10 closes here.

## What changed

- **`corpus/token-budget.md`**: a golden like any other, holding what every corpus document costs
  at `full` / `standard` / `plain`, plus the corpus totals. Regenerated with
  `DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test token_budget`.
- **`crates/docsai-convert/tests/token_budget.rs`**: compares against the golden and, when
  regenerating, **refuses an update that inflates the `full` total by more than 5 %** unless
  `DOCSAI_ACCEPT_TOKEN_INFLATION=1` is set too.
- **CI**: a named `Token budget` step, so a blown budget reads as a blown budget in the checks
  list rather than as "some test failed".
- Phase 10's acceptance criteria ticked in `docs/development-plan-v2.md`, with the evidence for
  each; `AGENTS.md` status updated.

## Non-obvious decisions

1. **Two gates, not one.** Exact comparison keeps the budget *diffable* — any change to any
   document shows up as a reviewable diff. The 5 % rule only bites on the deliberate refresh,
   which is where a real regression would otherwise slip through unnoticed. A single tolerance
   check would have let a hundred 4 % increases through.
2. **The escape hatch is an environment variable, not a config value.** Paying 5 % more tokens
   for every document is a product decision; it should cost someone an explicit command and leave
   a trace in the commit message.
3. **The failure message carries the number** (`before → after (+x.y %)`), because a failing gate
   that makes you re-run the tool to learn what happened is a gate people learn to skip.

## The numbers as of the phase close

Corpus totals (24 documents): **24 851** tokens at `full`, **16 691** at `standard`, **10 413**
at `plain`. The `full` total is dominated by front matter — the style catalogue — on every small
document; that is Phase 11's target, and the budget is now the instrument that will show whether
Phase 11 actually delivers.

## Phase 10, closed

| Increment | Delivered |
|---|---|
| 10-A/B | `docsai-model::addressing`, ids in the IR, derived etags ([[14-phase-10-addressing-core]]) |
| 10-C | DocMark 1.1 in the file, `--ids` ([[15-phase-10-docmark-1-1]]) |
| 10-D | `docsai tokens`, tokenizer decision ([[16-phase-10-tokens]]) |
| 10-E | `docsai outline`, the 5 % map budget ([[17-phase-10-outline]]) |
| 10-F | `corpus/token-budget.md` + CI gate (this document) |

Carried into Phase 11 on purpose: **etags are computed but not written to the file** (the spec
makes emission optional and nothing reads them until `read --select` needs an if-match).

## How it was verified

- `cargo test --workspace` (25 suites), clippy `-D warnings`, `cargo fmt --all -- --check`,
  `python3 corpus/generate.py --check`.
- The gate was checked in both directions: with the golden's total lowered by hand, a
  regeneration fails with `the corpus would cost 24.3 % more to read (20000 → 24851 tokens at
  full)`; restored, it passes.

## Next

Phase 11 — projections, raw-block sidecar and `--fidelity agent`: cut the front matter that this
phase proved is the bill.
