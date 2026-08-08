# 41 — Phase 13 G: deterministic, reversible reading order

Increment **13-G** of [[34-phase-13-plan]], on top of [[40-phase-13-notes]]. The first place in
the pptx reader where the IR states something the file does not: the order in which a slide is
read.

## What changed

- `crates/docsai-office/src/pptx/order.rs` (new): the ordering policy and its key function, with
  the unit tests that pin it.
- `pptx/mod.rs`: `read_shapes` sorts before returning, and its doc comment no longer promises
  source order.
- `corpus/generate.py`: new `reading-order.pptx`, a deck whose `p:spTree` is deliberately not its
  reading order.

## Non-obvious decisions

1. **`p:spTree` is z-order, not reading order.** It says what is drawn on top of what. The two
   agree in a deck nobody has edited and stop agreeing the moment a shape is sent to the back or
   a title is added last. Every fixture in the corpus before this one happened to be written in
   reading order, which is exactly why none of them could tell the policy from trusting the file.
2. **`reading-order.pptx` exists for that reason**, the same argument that produced
   `notes-crossed.pptx` in 13-F: a rule no fixture can distinguish from its opposite is untested.
   Its tree puts a footnote box first and the title fourth.
3. **The policy is only admissible because it is reversible.** `Shape::z_index` already carried
   the source index (the model has said so since 13-A); this increment is what makes it
   load-bearing. Reordering without it would permute a deck on every round trip — the class of
   silent damage `AGENTS.md` §7 rule 3 is about, even though nothing is *lost*.
4. **The order is total.** Every comparison ends in `z_index`, which is unique on a slide, so no
   two shapes ever compare equal and the result cannot depend on the sort algorithm. `sort_by_key`
   over a tuple, not a hand-written comparator: a comparator that is not a total order is
   undefined behaviour's well-behaved cousin, an order that changes between runs.
5. **Furniture placeholders are read last of the placeholders, not first.** The plan says
   "placeholders first by type"; the type ranking chosen is title, bodies, other placeholders,
   then `sldNum`/`dt`/`ftr`/`hdr`. They repeat on every slide and say nothing about this one.
   They still precede the free shapes, which is what the plan's wording asks for.
6. **Bodies are ordered by `p:ph@idx`, not by position.** `idx` is what matches a slide
   placeholder to its layout one, so it is also the order the layout intended two content boxes to
   be read in — and it is stable when a user nudges one box above the other.
7. **A row is an eighth of an inch tall (`ROW_BAND_EMU = 114_300`).** Comparing tops exactly would
   read a row of three cards top-down because one of them is 10 000 EMU higher — a hundredth of an
   inch, invisible. The band is a quantisation (`y.div_euclid(BAND)`, not `/`: truncation toward
   zero would make the band across y = 0 twice as tall), so it stays a total order. Its known
   artifact: two shapes a hair apart but on opposite sides of a band boundary land in different
   rows. Deterministic, and preferable to the exact-equality failure it replaces.
8. **A shape with no position keeps source order, at the end.** Its geometry is inherited from a
   layout this function does not resolve. Guessing where it sits would be worse than leaving it
   where the file put it.
9. **Groups are not recursed into.** `p:grpSp` is still an `UnsupportedElement` warning (13-I);
   when it is read, its children need this same sort applied inside the group.

## How it was verified

- `order.rs` unit tests: placeholders first and by type; free shapes row by row, left to right;
  an unpositioned shape last; and the pair that matters — sorting an already-sorted slide changes
  nothing, and sorting by `z_index` restores the source order exactly.
- `reading-order.pptx` through the full reader: shapes come back
  `Title 1, Content Placeholder 2, Marca izquierda, Etiqueta derecha, Pie 1`, with `z_index`
  `[3, 1, 4, 2, 0]` — neither the source order nor a sort of it.
- Existing order assertions still hold (`images-anchored` reads title then picture), so the policy
  did not disturb the decks that were already in reading order.
- `cargo test --workspace` (38 pptx tests), `clippy --all-targets -- -D warnings`, `fmt --check`,
  `corpus/generate.py --check` green. Corpus is 91 files.

## Known gaps, written down rather than left implicit

- Right-to-left decks read left-to-right. Nothing in the IR records slide-level text direction
  yet, and inferring it from the text would be a guess.
- Column layouts are read row by row: two tall boxes side by side, each holding a paragraph, read
  as one row, not as column-then-column. Correcting that needs a layout analysis this increment
  does not attempt.
- The band boundary artifact of decision 7 has no fixture; it is stated, not tested.

## Next

13-H: skeleton capture — non-slide parts stored opaquely through the `AssetStore` and referenced
from `Presentation::skeleton`, streamed rather than held (spike P3 measured the 2.7× cost).
