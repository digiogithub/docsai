# 43 — Phase 13 I: nothing disappears

Increment **13-I** of [[34-phase-13-plan]], on top of [[42-phase-13-skeleton]]. The skeleton keeps
the *package* whole; this increment is the same promise one level down, inside the slide.

## What changed

- `docsai-model`: `Warning::AutofitStale`, `Presentation::raw: Vec<RawFragment>`, and
  `ShapeGeometry::preset` — the preset geometry name the struct's own doc comment had been
  promising since 13-A without a field to hold it. `ShapeGeometry` is no longer `Copy`.
- `crates/docsai-office/src/pptx/raw.rs` (new): the fragment sink and the stub builder.
- `pptx/mod.rs`: groups, connectors, `mc:AlternateContent`, custom geometries, diagrams and OLE
  become stubs; `p:transition` and `p:timing` become slide-level fragments; a chart becomes a
  `ChartRef`; `read_graphic_frame` always returns a shape.
- `corpus/generate.py`: `raw-preserved.pptx`, and `slide(extra=…)` for slide-level markup.

## Non-obvious decisions

1. **Three things, always together.** A stub (so an agent knows the object is there and does not
   delete it by writing the slide back without it), the markup verbatim (so a writer can put it
   back), and a typed warning (so the fidelity loss is in the report, not in the surprise). Two
   out of three is the failure `AGENTS.md` §7 rule 3 names.
2. **A fragment is a slice of the source, never a re-serialisation.** `Element` records its byte
   span, so preservation is `&source[span]`. Re-serialising the tree would already have lost
   namespace prefixes, attribute order and whitespace — and a `mc:AlternateContent` that comes
   back with its prefixes rewritten is not the element PowerPoint wrote.
3. **The fragments live on the deck, the ids on the slides and shapes.** `Presentation::raw` is
   one list, exactly as `Sheet::raw` is for a workbook. A serialiser writes the Phase 11 sidecar
   from that list without walking every shape, and the ids stay unique deck-wide.
4. **`p:transition` and `p:timing` used to vanish without a word.** `read_slide` only ever looked
   at `cSld/spTree`, so every other child of `p:sld` was dropped in silence — the exact bug this
   project exists to prevent, sitting in our own reader. Now captured onto `Slide::raw`.
5. **A group is stubbed whole, children included.** The model has a `ShapeKind::Group` this reader
   still does not fill. Reading a group's children means resolving a second, nested cascade;
   stubbing loses the structure but the stub carries the group's text and the fragment carries
   every child verbatim. Written down as the loss it is, not as a decision that costs nothing.
6. **`mc:AlternateContent` is preserved as a pair, and neither branch is read.** Choosing between
   `mc:Choice` and `mc:Fallback` is the consumer's job, and the corpus proves the point: the
   SmartArt fixture's diagram `p:graphicFrame` is *inside* the `mc:Choice`, so descending would be
   picking a branch. One stub comes back, not two.
7. **An unknown element is preserved, not skipped.** The old code warned "unknown shape element"
   and dropped it. Anything `p:spTree` holds that this reader has never heard of now becomes a
   `RawShapeKind::Other` stub with its bytes.
8. **A chart is recorded rather than stubbed.** `ShapeKind::Chart(ChartRef)` exists and the model
   says Phase 13 records that a chart is there and where its data lives. The kind comes from the
   first plot in `c:plotArea` of the chart part, reached through the frame's `r:id` — a combo
   chart is named by its first plot rather than by a guess at which one matters. The chart XML is
   the fragment. `workbook` stays `None`: the series are Phase 16's, and the embedded `.xlsx` is
   in the skeleton meanwhile.
9. **`a:prstGeom@prst` is now kept.** A `rightArrow` was read as a plain text box and came back a
   plain box — a silent geometry loss the skeleton hid, because nothing had written the deck from
   the IR yet. The field costs `ShapeGeometry` its `Copy`, which turned out to be free: nothing
   outside the model relied on it.
10. **A `custGeom` shape is a stub; a `prstGeom` shape is not.** The plan names `custGeom`. A
    preset shape's outline is now one string, so a text box with a name for its shape is more
    information than a stub would be; a custom geometry is a path list nothing but the raw markup
    can express.
11. **The stale autofit scale is kept *and* reported.** Analysis §5.4 says drop it; that is the
    *serialiser's* decision at the lossy fidelity levels. A reader that dropped it would leave the
    lossless levels unable to write it back. So the number stays in `ShapeProps::font_scale` and
    `Warning::AutofitStale` says what it means. The warning carries the scale in the file's own
    unit (thousandths of a percent, 62 500 = 62.5 %): an integer, because `Warning` derives `Eq`
    and a float in it would not.

## How it was verified

- `raw-preserved.pptx` (new): the group and the custom geometry come back as two stubs; the
  group's stub carries the text of both boxes inside it and the position off `p:grpSpPr` (not
  `p:spPr`, which a group does not have — a stub with no position would sort last on every slide);
  the custom geometry's fragment contains `<a:custGeom>` and its closing path. The slide's
  `p:transition` and `p:timing` come back as two fragments in that order, `tmRoot` included.
- `shapes-geometry.pptx`: the connector is a `Connector` stub, and the arrow's geometry now says
  `rightArrow`.
- `charts-embedded.pptx`: `ChartRef::kind == "barChart"`, the fragment is the chart part and still
  contains the series value `1200`, and exactly one raw block was emitted.
- `smartart-fallback.pptx`: one `Other` stub whose fragment contains `mc:Fallback`, plus the
  `raw-block` warning.
- `autofit-stale.pptx`: `font_scale == 62.5` **and** `Warning::AutofitStale { scale: 62_500 }`.
- Every stub's payload is reachable from `Presentation::raw`, and `raw_blocks_emitted` equals the
  number of fragments.
- `cargo test --workspace` (46 pptx tests), `clippy --all-targets -- -D warnings`, `fmt --check`,
  `corpus/generate.py --check` green; corpus is 92 files.

## Known gaps, written down rather than left implicit

- **`RawShapeKind::SmartArt` and `::Ole` have no fixture.** Both are reached from a
  `p:graphicFrame`'s `a:graphicData@uri`, and the only diagram in the corpus is wrapped in
  `mc:AlternateContent` (decision 6), while no deck has an OLE object at all. Code path, not
  fixture path.
- **A group's structure is lost** (decision 5), and `ShapeKind::Group` stays unused.
- **A free text box's `fontScale` is warned about but not stored**: `ShapeKind::TextBox` has no
  `ShapeProps`, so only placeholders keep the number.
- **Notes pages still warn without preserving.** `notes.rs` reports an unmodelled shape on a notes
  page and moves on; it has no sink. The skeleton holds the part, so nothing is lost from the
  package, but the fragment a DocMark round trip would need is not there.
- **`RawShapeKind` has no `Group` variant**, so a group stubs as `shape`. Adding one changes
  DocMark-P's stub class vocabulary, which is a specification surface and not this increment's to
  widen.

## Next

13-J: `.pptm` read as its macro-free equivalent with the security warning `.docm` already emits,
and zero panics on the synthetic corrupt corpus — truncated ZIP, malformed XML, dangling
relationships — always a typed `Err`.
