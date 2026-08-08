# 39 — Phase 13 E: pictures and tables on a slide

Increment **13-E** of [[34-phase-13-plan]], on top of [[38-phase-13-cascade]]. Two shape kinds
stop being warnings and start being content.

## What changed

- `crates/docsai-office/src/drawingml.rs` (new): `resolve_blip`, `warn_if_unrenderable` and
  `read_crop`, moved out of `docx/drawing.rs` unchanged. `a:blip` and `a:srcRect` are written
  identically in a `w:drawing`, an `xdr:pic` and a `p:pic`.
- `crates/docsai-office/src/pptx/graphics.rs` (new): `read_picture` (`p:pic` → `ImageRef`) and
  `read_table` (`a:tbl` → `Table`).
- `pptx/mod.rs`: `p:pic` and `p:graphicFrame` are read; a new `SlideCtx` carries the package,
  the part name, the relationships, the layout and the cascade so the shape readers keep a
  signature a human can read. `read_package` finally uses its `AssetStore`.

## Non-obvious decisions

1. **No new image or table model.** A slide picture is the `ImageRef` of architecture §3.1 and a
   slide table is the IR `Table`. The plan says so outright ("no new image model"), and it is
   what makes a table convert to Markdown at all: the renderers already exist.
2. **The blip code is shared, not copied.** `docx/drawing.rs` and `pptx/graphics.rs` resolve
   media through the same function, so a picture used by both formats gets the same content
   hash, the same `ExternalImageNotFetched` refusal and the same EMF warning. A second copy
   would have drifted at the first bug fix.
3. **A picture's placement is the shape's, not the image's.** `Shape::geometry` holds the
   position, rotation and flips like every other shape on the slide; `ImageGeometry` holds what
   belongs to the bitmap — displayed size, crop, native pixels. Writing the transform in both
   places is writing it twice to disagree with itself later.
4. **The frame is read by its uri.** `p:graphicFrame` holds a table, a chart, SmartArt or an OLE
   object, and only `a:graphicData@uri` says which. Guessing from the first child is how a chart
   becomes an empty table. What is not a table is reported as `p:graphicFrame (chart)` — by what
   it is, so the increment that models it knows what it is looking for.
5. **A horizontally merged cell leaves the grid; a vertically merged one stays.** DrawingML
   writes the span on the origin cell (`gridSpan`/`rowSpan`) *and* writes out every cell it
   swallowed (`hMerge`/`vMerge`). A `colspan` of 2 already occupies both columns, so keeping the
   `hMerge` cell would make `Table::width()` one wider than the table has; a `vMerge` cell
   becomes a `covered` cell, which is exactly what a `.docx` `vMerge` continuation becomes.
6. **A covered cell's text is dropped on purpose.** PowerPoint repeats the spanning cell's text
   in every cell it swallowed. Reading it would duplicate that text across the grid — the same
   double-emission failure [[11-plan-v2-onramp]] names for properties.
7. **The table style stays a GUID.** `a:tableStyleId` points into `ppt/tableStyles.xml`; it is
   kept as a `StyleId` reference. Resolving it into per-cell formatting would flatten a deck's
   styling into its content, which is the mistake [[38-phase-13-cascade]] exists to avoid.
8. **A picture in a placeholder slot warns.** `ShapeKind::Picture` has nowhere to carry a
   `p:ph`, so the slot is named in a `Warning::Degraded` rather than dropped (`AGENTS.md` §7
   rule 3). No corpus deck has one.

## How it was verified

- `images-anchored.pptx`: the picture is a `ShapeKind::Picture`, placed by its shape at
  1 524 000 × 2 286 000 EMU, drawn at 1 143 000 × 857 250, `descr` read as alt text, native
  pixel size read from the PNG's own header, the media part in the store, `stats.images == 1`,
  no `Warning::Degraded`.
- `tables-simple.pptx`: two columns of 3 733 800 EMU, three rows, `firstRow` making a header
  row, cell text in place, `stats.tables == 1`.
- A unit test on inline XML for the merge model (`gridSpan`, `rowSpan`, `hMerge`, `vMerge`, the
  style GUID and a cell fill), because no corpus deck merges a cell.
- `charts-embedded.pptx` still warns `p:graphicFrame (chart)`; `smartart-fallback.pptx` warns
  about `mc:AlternateContent`, which is where SmartArt actually lives — increment 13-I.
- `cargo test --workspace`, `clippy --all-targets -- -D warnings`, `fmt --check`,
  `corpus/generate.py --check` green.

## Known gaps, written down rather than left implicit

- `a:tr@h` (row height) is not modelled anywhere in the IR — the docx reader drops `w:trHeight`
  the same way, and the value is a minimum PowerPoint recomputes. The skeleton (13-H) is what
  preserves it.
- `a:tblPr@bandRow`/`bandCol`/`firstCol` are banding flags of the table style, not content; they
  travel with the style GUID and are not read separately.
- `a:tcPr` insets, vertical alignment and borders are not read: the IR cell has no field for
  them. They are part of the table style in every corpus deck.
- A picture's `a:effectLst` warns, like the docx one; a picture that is also a placeholder warns.
  Neither case is in the corpus, so both are covered by the code path, not by a fixture.

## Next

13-F: notes — `ppt/notesSlides/*` into `Slide::notes`, reached through the slide's `_rels` and
never by index.
