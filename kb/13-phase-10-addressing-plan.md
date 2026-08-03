# 13 — Phase 10 plan: stable addressing and token budget

Implementation plan for **Phase 10** of [`docs/development-plan-v2.md`](../docs/development-plan-v2.md)
(the first phase of plan v2). Builds on [[11-plan-v2-onramp]] and the DocMark 1.1 sketch in
`docs/docmark-specification.md` §11.1.

Objective restated: every addressable node has a **persistent id** and an **etag**, and document
cost is a **measured** number (`docsai tokens`), not an estimate.

## Increments (each independently testable, in order)

| # | Increment | Scope |
|---|---|---|
| 10-A | Addressing core in `docsai-model` | `addressing.rs`: `NodeId`, `NodeKind`, `IdPolicy`, `IdAllocator` (monotonic `next_id`), `etag()` hash. No IR changes yet. |
| 10-B | Ids on the IR + assignment walk | `id`/`etag` fields on addressable nodes; `assign_ids(&mut Document, IdPolicy)`; proptest for stability / no reuse / no collision (risk P7). |
| 10-C | DocMark 1.1 emission + parse | `docmark: "1.1"`, `next-id:` in the front matter, `{#id etag=…}` on addressable nodes, `--ids never\|preserve\|assign`. Goldens updated by hand. |
| 10-D | `docsai tokens` | Vendored pure-Rust BPE tokenizer (decision recorded in `docs/technical-analysis.md`), per-node and per-document cost, `--json`. |
| 10-E | `docsai outline` | Id tree: node type, ~60-char preview, token cost per node, `--depth`, `--json`. |
| 10-F | Token CI gate | Corpus token report committed as a golden; > 5 % total inflation fails CI. |

## Design decisions taken up front

1. **Id token shape**: opaque `n<counter>` (`n1`, `n42`), allocated from the monotonic
   `next-id` counter in the front matter. The positional forms in the spec examples (`s4`,
   `s4.b2`) are **selectors** (Phase 11 `read --select`), not ids — ids must survive insertion,
   positional labels cannot.
2. **Never renumber, never reuse**: `next-id` only grows. Ids read from a document are preserved
   verbatim; absent ids are assigned on read/write according to `IdPolicy`.
3. **Addressable set** (plan 10.1): section, heading, paragraph *only when it is a list root or a
   container*, list, table, table row, image, footnote, sheet, raw block. Runs/inlines are
   addressed by relative path, never by id.
4. **Etag**: 6 hex chars of a 64-bit FNV-1a over the **normalised content** of the node —
   text and structure only, excluding ids, etags and formatting-only attributes, so a style
   change does not churn the etag.
5. **Compatibility**: additive. A 1.0 document parses unchanged; ids appear on the next write.
   `--ids never` reproduces 1.0 output byte-for-byte (`plain` fidelity always implies `never`).

## Acceptance criteria (from the plan, tracked here)

- [ ] Property test: ids survive N=10 round trips, never reused, never collide.
- [ ] Etag changes iff normalised node content changes (both directions tested).
- [ ] `outline` of the largest corpus document < 5 % of the document's own tokens.
- [ ] Corpus token report generated in CI and diffable.
- [ ] DocMark 1.0 documents still parse; ids added on next write.

## Rules that bind this phase

- No Phase 11+ work (no sidecar raw blocks, no `--fidelity agent`, no `read --select`, no new
  MCP tools) — `AGENTS.md` §7 rule 1.
- Spec change ⇒ version bump documented in `docs/docmark-specification.md` (§2 item 3).
- Golden updates are deliberate and hand-reviewed (`DOCSAI_UPDATE_GOLDENS=1`).
