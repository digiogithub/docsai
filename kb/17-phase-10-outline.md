# 17 — Phase 10 increment E: `docsai outline`, the map you read first

Increment 10-E of [[13-phase-10-addressing-plan]], on top of [[16-phase-10-tokens]].
An agent can now see what is in a document, and what each part costs, without reading it.

## What changed

- **`NodeFragment.descendants`**: the writers hand `IdSource::record` a `mark` taken when the
  node opened, so each fragment knows how many fragments it contains. A fragment is recorded when
  its node *finishes*, so its descendants are the contiguous block immediately before it — that
  is what turns the flat trace back into a tree, with no second traversal and no guessing.
- **`docsai-convert::outline`**: `outline` / `outline_path` → `Outline { document_tokens,
  outline_tokens, depth, nodes }` with `OutlineNode { id, kind, tokens, preview, children }`,
  plus `render_text()` — the form an agent reads and the one `outline_tokens` measures.
- **`docsai outline <in> [--depth N] [--fidelity …] [--json]`** in the CLI.
- **Previews no longer echo the machinery**: attribute blocks, `:::` fences and `|---|` rules are
  stripped, so a preview is text. Repeating `{#n4 .ListParagraph list=L1}` would have cost tokens
  to say what the id column already says.
- **New corpus fixture `docx/long-report.docx`** (~9 000 tokens, 12 sections, three heading
  levels).

## Non-obvious decisions

1. **The tree is containment, not heading level.** A footnote hangs from the paragraph that calls
   it, a nested list from its parent, an image from its sheet. Headings are siblings, with their
   level visible in the preview. Nesting headings by level would have meant a node's `tokens` no
   longer matched its own fragment — the number would stop being measured and start being
   attributed.
2. **A new corpus document, because the criterion was unmeasurable.** Every existing fixture
   isolates one trait and is 300–700 tokens; on a 700-token file the outline *is* the document
   (measured 8–25 %). The ratio is a property of heading density, so the honest fix was a fixture
   with ordinary prose density rather than a shorter preview. On `long-report.docx` the outline is
   **345 tokens of 9 158 — 3.8 %**, and the test asserts both the 5 % budget *and* that the
   document it measured is over 5 000 tokens, so the criterion cannot silently become vacuous
   again.
3. **`--depth` truncates, it does not summarise.** A cut branch disappears; nothing is folded into
   its parent, because a fabricated aggregate would be a number no one can check.

## How it was verified

- `cargo test --workspace` (24 suites), clippy `-D warnings`, `cargo fmt --all -- --check`,
  `python3 corpus/generate.py --check`.
- `crates/docsai-convert/tests/outline.rs`: containment for lists and footnotes, previews free of
  machinery and within 60 characters, `--depth` cutting only the leaves, lossy levels empty, and
  the phase's own 5 % budget on the largest corpus document.
- The new golden `corpus/docx/long-report.expected.dmk.md` was regenerated with
  `DOCSAI_UPDATE_GOLDENS=1` and read by hand; round-trip identity covers it like every other
  corpus document.

## Next

10-F: the corpus token report as a committed golden plus the CI gate that fails a PR inflating
total corpus tokens by more than 5 %.
