# 16 — Phase 10 increment D: `docsai tokens`, the measured budget

Increment 10-D of [[13-phase-10-addressing-plan]], on top of [[15-phase-10-docmark-1-1]].
Document cost stops being an estimate.

## What changed

- **Tokenizer decision** recorded in `docs/technical-analysis.md` §4.4: **`tiktoken-rs` 0.12**
  with the **`o200k_base`** encoding. Vocabulary embedded in the crate — no network, no Python,
  no build script. Alternatives weighed there: `tiktoken` (goliajp — faster, fewer deps, but a
  young single-vendor crate), `bpe-openai` (github/rust-gems — linear-time BPE, but a `build.rs`
  that rebuilds the dictionary on every clean build) and HuggingFace `tokenizers` (rejected: it
  downloads its vocabulary).
- **`docsai_docmark::serialize_traced`** → `(String, ConversionReport, Vec<NodeFragment>)`. Same
  bytes as `serialize`, plus the exact DocMark each addressed node wrote. Collection lives in
  `IdSource`, which already threads through both writers and already knows which node took which
  id, so no signature churn and no second traversal.
- **`docsai-convert::tokens`**: `count`, `token_report` (in-memory) and `token_report_path`
  (reads a file), producing `TokenReport { total, front_matter, body, bytes, nodes }` with
  `NodeTokens { id, kind, tokens, bytes, preview }`.
- **`docsai tokens <in> [--fidelity …] [--ids …] [--top N] [--json]`** in the CLI.
- `NodeKind` now serialises as the spec's own name (`row`, not `TableRow`).

## Non-obvious decisions

1. **Cost is measured over what was written, not over the IR.** A node's fragment is the literal
   DocMark it produced — attribute block included — because that is what a model pays for. An
   estimate from text length would miss exactly the part Phase 11 has to attack.
2. **Fragments nest and are reported innermost first.** A node is recorded when it *finishes*, so
   a footnote arrives before the paragraph containing it, and a section's fragment contains its
   headings'. Node costs therefore add up to more than the document total, on purpose; the CLI
   says so in the listing header.
3. **`total` is counted over the whole file, not as `front_matter + body`.** BPE merges across
   the boundary, so the parts do not have to add up; the test asserts a bound, not an equality.
4. **The encoding is named in the report.** Changing tokenizer changes every number, and the
   token golden of 10-F must show that in the diff instead of absorbing it silently.
5. **A footnote costs its definition**, not its reference: the reference is three characters, the
   text the reader actually pays for is the definition.

## What the first measurement says

Corpus, `full` vs `plain` (tokens):

| Document | full | of which front matter | plain |
|---|---|---|---|
| `docx/nested-lists` | 701 | 565 (81 %) | 77 |
| `docx/basic-text` | 456 | 380 (83 %) | 72 |
| `docx/images-floating` | 642 | 387 (60 %) | 89 |
| `xlsx/formulas-basic` | 351 | 165 (47 %) | 97 |

**The front matter, not the content, is the bill.** On small documents the style catalogue is
60–85 % of the `full` cost. That is the number Phase 11 (`--fidelity agent`, raw sidecar) exists
to cut, and 10-F will freeze it as a golden so any regression shows up in the diff.

## How it was verified

- `cargo test --workspace` (23 suites), `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`.
- `crates/docsai-convert/tests/tokens.rs`: totals and the front-matter/body split, per-node costs
  bounded by the document, lossy levels cost less and address nothing, a workbook is measured
  sheet by sheet, `heaviest()` ordering.
- `crates/docsai-docmark/tests/node_ids.rs::tracing_reports_what_was_written_and_changes_nothing`:
  traced output is byte-identical to untraced, one fragment per addressed node, every fragment is
  literally present in the output, a footnote's fragment is its definition.

## Next

10-E `docsai outline` (id tree with preview and per-node cost, reusing these fragments; must stay
under 5 % of the document's own tokens) and 10-F the corpus token report as a CI golden.
