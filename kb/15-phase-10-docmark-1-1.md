# 15 — Phase 10 increment C: DocMark 1.1 emission and parse

Increment 10-C of [[13-phase-10-addressing-plan]], on top of [[14-phase-10-addressing-core]].
Node ids now reach the file and come back from it.

## What changed

- **`docsai-docmark::ids::IdSource`** — the serializer, not the IR, hands out ids. It clones the
  document's counter, observes every id already present, then allocates in writing order. The IR
  is never mutated by a write.
- **`serialize` renders the body first**, then the front matter: the body is what allocates ids,
  and the front matter has to declare the resulting `next-id`.
- **`Options.ids: IdPolicy`** in `docsai-docmark`, **`ConvertOptions.ids: Option<IdPolicy>`** in
  `docsai-convert` (`id_policy()` resolves the per-fidelity default), **`docsai convert --ids`**
  in the CLI.
- **Emission points** (writer + sheet writer): heading, container paragraph, list (`list-id=` on
  the first item), table container, complex-table `::: {.row}`, image, footnote *reference*
  (`[^1]{#n9}`), sheet heading, multi-section `::: {.section}`.
- **Parse**: `next-id` into `FrontMatter.addressing`; `{#id}` read at every emission point;
  `observe_ids` after parsing so the counter dominates hand-written ids.
- Front matter declares `docmark: "1.1"` **only when the body carried an id**, so `--ids never`
  reproduces 1.0 byte for byte.
- Spec §11.1 is now normative for ids (table of where each id lives, policy, `next-id` rules);
  README and CHANGELOG updated; 28 corpus goldens regenerated and reviewed by hand.

## Non-obvious decisions

1. **A node is addressable only where DocMark can carry its id.** `for_each_addressable` skips
   sections in a single-section document (no `::: {.section}` container), rows of GFM tables (no
   attribute slot) and lists whose first item does not start with a paragraph. An id that is
   dropped on every write would change on every round trip — worse than not having one. The model
   walkers and the writer must agree on this set; that agreement is the invariant to protect.
2. **Both walkers visit in writing order** (headers/footers before the section body) so an id
   assigned in the IR matches what a conversion would have written.
3. **Inline images are addressable.** Forgetting them in the walker was a real bug: the writer
   allocated an id for an inline image that the observe pass never saw, so `next-id` came back
   one short and the round trip was not idempotent. The golden identity test caught it.
4. **A footnote is addressed on its reference**, not on its definition line: the definition's
   first line already ends in the body paragraph's own attribute block, which would be ambiguous.
5. **Ids only at `full`.** `standard` is the hand-editable level and stays free of `{#n…}` noise;
   `plain` is CommonMark and forces `never` whatever the caller asks.

## How it was verified

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check` green.
- `crates/docsai-docmark/tests/node_ids.rs`: 1.1 front matter and counter, ids unchanged across a
  round trip with a byte-identical second pass, `preserve`/`never` semantics, hand-written id
  never handed out again, a 1.0 document parsing and gaining ids, lossy levels free of ids.
- Golden suite (28 files regenerated) including `serialize(parse(md)) == md`.

## Next

10-D `docsai tokens` (vendored pure-Rust BPE tokenizer, decision to record in
`docs/technical-analysis.md`), then 10-E `outline` and 10-F the token CI gate.
