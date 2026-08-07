---
created_at: 2026-08-07T17:02:13.153197893Z
updated_at: 2026-08-07T17:02:34.238698286Z
tags:
    - phase-13
    - pptx
    - presentations
    - reader
    - office
    - opc
---
# 36 — Phase 13 B: the pptx package layer

Increment **13-B** of [[34-phase-13-plan]], on top of [[35-phase-13-ir-root]]. It answers
*which parts exist, what they are, and in what order the slides come*. The slides it produces
are correct and empty; filling them is 13-C.

## What changed

`crates/docsai-office/src/pptx/mod.rs` (new), plus two things lifted into `package.rs` because
a third copy would have been the third place to drift:

- `ContentTypes` — `[Content_Types].xml` as `Default` extensions plus `Override` part names,
  with `of`, `is` and `parts_of`. Shared, not pptx-only.
- `read_meta` — the `docProps/*` reader the docx and xlsx modules each carried verbatim. Both
  now call the shared one.
- `docsai_office::read_pptx` is public but **not** in `READABLE` and not in the `read`
  dispatch; `detect.rs` now uses `pptx::PRESENTATION_PART` instead of repeating the string.

What the reader fills today: `slide_size` from `p:sldSz`, the layout and master catalogue with
placeholder identity and geometry, and per slide its layout, `p:cSld@name`, `show="0"` and its
section.

## Non-obvious decisions

1. **A part is what its content type says it is.** Spike P3's rule ([[33-phase-12-spike-p3]]),
   enforced rather than assumed: the presentation part comes from the `officeDocument`
   relationship cross-checked against `[Content_Types].xml`, and a slide relationship whose
   target is not declared a slide is skipped with a `Warning::Degraded`. An **undeclared** part
   is still accepted — a package with no content types is degraded, and refusing it would be
   worse than reading it.
2. **`PartName` is package-absolute.** `normalise_part_name` rejects a leading `/` because a ZIP
   member with one is an escape attempt, but every `Override PartName` legitimately starts with
   one. Missing that made the reader silently classify every slide as "not a slide" and return a
   deck with zero slides and zero warnings — the exact silent-emptiness failure this project
   refuses. Strip the slash for content types only.
3. **`r:id` must be matched on the qualified name.** `p:sldId` carries `id="256"` (the deck-wide
   slide id, which `p14:sectionLst` refers to) *and* `r:id="rId2"`. A lookup by local name
   returns the wrong one, and it returns a number that parses, so nothing fails loudly. Same
   trap on `p:sldMasterId` and `p:sldLayoutId`; one `rel_id` helper covers all three.
4. **`LayoutId`/`MasterId` are the part names.** Unique and stable within a package, where
   `p:cSld@name` is neither — the hand-built corpus layouts ([[30-phase-12-pptx-corpus]]) do not
   name themselves at all (`part_stem` gives them "slideLayout1"). What DocMark front matter
   writes is Phase 14's question, and it can map.
5. **Layouts are read on demand**, from the masters' `p:sldLayoutIdLst` *and* from every slide's
   own relationship, deduped. A layout reachable only from a slide is legal, and a master with
   forty layouts of which a deck uses two should not cost forty parses.
6. **An absent `a:xfrm` stays absent.** `read_geometry` returns an empty `ShapeGeometry`, not
   zeros: in the cascade, nothing means *inherited*. The corpus proves it — the master positions
   the title, the layout redeclares it without a transform, and the test asserts
   `is_inherited()`.
7. **`read_pptx` is not wired into `read`.** The reader is honest-but-incomplete for several
   more increments, and a deck converting to an empty document is worse than a deck the tool
   refuses. `docsai formats` still reports `pptx: no`.

## How it was verified

- Nine new tests in `pptx::tests`, including
  `every_deck_in_the_corpus_gets_through_the_package_layer`: all twelve Phase 12 decks read with
  slides, a layout catalogue and **zero warnings**.
- `slide_order_comes_from_the_id_list_not_the_file_names` pins `slide-order.pptx` to its real
  `p:sldIdLst` (2, 3, 1) — file order would give 1, 2, 3.
- `a_package_without_a_presentation_part_is_an_error_not_a_panic` feeds it a `.docx`.
- `cargo test --workspace`, `clippy --all-targets -- -D warnings`, `fmt --check` green;
  `python3 corpus/generate.py --check` unchanged at 89 files.

## Known gap

No corpus fixture carries `p14:sectionLst`, so section mapping is tested as the pure function it
is, on inline XML. A fixture belongs with the increment that can show sections end to end.

## Next

13-C: slide text — `a:p`/`a:r` into `Paragraph`/`Inline`, `a:pPr@lvl` + `buChar`/`buAutoNum`
into `List`, and empty placeholders kept as the real content they are.
