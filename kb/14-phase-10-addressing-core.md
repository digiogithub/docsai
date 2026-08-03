# 14 — Phase 10 increments A/B: addressing core and ids in the IR

What landed for the first two increments of [[13-phase-10-addressing-plan]] (plan v2 Phase 10).
Nothing of this is visible in DocMark yet — emission is increment 10-C.

## What changed

**New module `docsai-model::addressing`** (`addressing/mod.rs` + `addressing/walk.rs`):

- `NodeId` (`n1`, `n42`), `Etag` (6 lowercase hex chars), `IdPolicy` (`assign` | `preserve` |
  `never`), `Addressing { next_id }` — the monotonic counter serialised as `next-id`.
- `EtagHasher`: FNV-1a 64, truncated to 24 bits. No dependency added.
- `Addressable` trait (`node_kind`, `node_id`, `set_node_id`, `clear_node_id`, `hash_content`,
  `etag`) implemented through a local macro for `Section`, `Heading`, `Paragraph`, `List`,
  `Table`, `TableRow`, `ImageRef`, `Footnote`, `Sheet`.
- `assign_ids(&mut Document, IdPolicy)` — two passes: observe every existing id first so the
  counter dominates it, then fill the gaps. `observe_ids`, `clear_ids`, `node_ids`,
  `for_each_addressable` (read-only twin of the mutable walker, needed by `outline` in 10-E).

**IR changes** (`docsai-model`):

- `id: Option<NodeId>` on `Section`, `Heading`, `Paragraph`, `List`, `Table`, `TableRow`,
  `ImageRef`, `Sheet`; `addressing: Addressing` on `TextDocument` and `Workbook`;
  `Document::addressing()` / `addressing_mut()`.
- `Inline::Footnote(Vec<Block>)` became `Inline::Footnote(Footnote)` with
  `Footnote { id, blocks }` — a footnote is addressable, so it needed a place for its id.
- `Inline::Image(ImageRef)` became `Inline::Image(Box<ImageRef>)`. The extra `id` field pushed
  the variant past clippy's `large_enum_variant` threshold, and boxing is the right answer
  anyway: inlines are stored by value and a text run is an order of magnitude smaller.
  Serde output is unchanged (`Box` is transparent), so no golden moved.
- `style_map` promotion of a paragraph to a heading now carries the paragraph's id across.

## Non-obvious decisions

1. **Ids are opaque, not positional.** `n7`, never `s4.b2`. The positional forms in
   spec §11.1 are Phase 11 *selectors*; an id that encodes a position cannot survive an
   insertion, which is the whole point.
2. **Etags are derived, never stored.** Recomputing from the node is always correct; a stored
   etag can go stale behind an edit. The IR carries ids only.
3. **Etag normalisation**: node kind + text (whitespace-collapsed) + run/link/field structure.
   Formatting properties are excluded, so restyling does not churn the etag, but splitting text
   into differently-shaped runs does.
4. **Paragraphs are addressable only as containers** (they hold a footnote, an inline image or an
   inline raw fragment). Ordinary prose paragraphs are reached by relative path, which keeps id
   noise out of the body — `paragraph_is_container()` is the single place that rule lives.
5. **Raw fragments keep their existing `RawId`** instead of getting a second identity; Phase 11
   names sidecar files after it.

## How it was verified

- `cargo test --workspace` green; `cargo clippy --workspace --all-targets -- -D warnings` clean;
  `cargo fmt --all -- --check` clean.
- New `crates/docsai-model/tests/addressing.rs` (proptest, risk P7): ids survive 10 rounds of
  reassignment unchanged, stay unique, survive insert/delete at both ends without renumbering a
  survivor or reusing a freed id; hand-written ids are preserved and the counter moves past them;
  etag tracks content and ignores formatting and the id itself.
- Existing goldens untouched — nothing reaches DocMark yet.

## Next

10-C: front matter `docmark: "1.1"` + `next-id`, `{#id etag=…}` emission, `--ids` flag, parser
preservation, goldens re-reviewed by hand.
