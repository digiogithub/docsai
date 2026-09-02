---
tags:
    - phase-16
    - pptx
    - writer
    - charts
    - skeleton
    - warnings
---
# 78 — Phase 16-C: the chart written back

Third increment of [[75-phase-16-plan]], on top of [[76-phase-16-chart-data]] and
[[77-phase-16-chart-docmark]]. 16-A put the series in the IR, 16-B gave a person a table to edit,
and neither touched a byte of a written deck. This increment is where the edit reaches the file —
and it is the riskiest of the phase, because the property it must not break is the one everything
else in this project rests on: **nineteen decks that nobody edited come back byte for byte**.

They still do. `corpus/render_gate.py`: *19 decks, 19 round trips identical by bytes, 0 failures*,
and by hand, outside the suite, every deck of `corpus/pptx/` converted to DocMark and back and
`cmp`ed against the original — **19 identical, 0 differing**.

## The rule, and the mistake it is guarding against

`ppt/charts/chartN.xml` is **not** in `SkeletonRef::rebuilt_parts` and 16-A did not put it there.
Reading a chart as data bought no licence to regenerate it, so the caches are **spliced inside the
part that was read** — the technique of [[62-phase-15-package]] and [[64-phase-15-media]], one part
further out — and every other byte of the chart is the byte that came in: its `c:spPr`, its axes,
its legend, its data labels, its `c:extLst`.

What makes that safe is 15-B's rule: *nothing is written unless it differs from what the part
already says*. [[73-phase-15-carried]] records what happens when that comparison is made in the
wrong units — the writer compared a rewritten body against the part's **bytes** instead of against
what the part **said**, which both rewrote unchanged parts and raised a false `AutofitStale`. So the
question here is asked exactly once, in the model's own units, by the model's own reader:

```rust
let said = chart::read_space(&root, part, &package.relationships(part), &Theme::default(), …);
if said.matches(chart) { return Spliced { part: None, kept } }
```

`read_space` is 16-A's reader factored out of `chart::read` so that the writer runs **the same
code** over the preserved part. There is no second notion of what a chart says, and there is
nowhere for the two to drift apart. A chart nobody touched therefore produces no edit at all — not
to the chart part, not to its `_rels`, not to `ppt/embeddings/`, not to `[Content_Types].xml` — and
**no warning**, which `a_chart_nobody_edited_produces_no_edit_and_no_warning` asserts over all three
chart decks. It could not, before: every chart in every deck used to raise «its chart: it was copied
from the preserved package», on a deck where nothing had been copied instead of anything.

The title is read through the same reader with `Relationships::default()` and `Theme::default()`.
Neither is reached for anything but a `c:rich` title's colours and fonts, none of which survives
into `ChartRef::title` — the title is plain text there — so the defaults change no answer.

## What splices what

| the model changed | the edit |
| --- | --- |
| a value, or a series' `c:formatCode` | the whole `c:numCache` under that series' `c:val` |
| a category label | the `c:strCache` under every `c:cat` that states the chart's axis |
| a series name | the `c:strCache` under its `c:tx`, or its literal `c:v`, or the `c:tx` itself |
| the number of points | the caches above, **and** the `c:f` beside each of them |

Three details are the whole of it:

- **`c:ptCount@val` is the length of the column and `c:pt@idx` are sparse.** A gap is an *absent*
  `c:pt`, never a `c:v` of `0` — PowerPoint draws a break where a zero would draw a line through
  the floor. That is the rule 16-A read by and 16-B carried through the table; writing it back any
  other way would have thrown away two increments of care one layer down.
- **A series' name has three shapes in the format and each gets its own edit.** A reference with a
  cache is renamed in its cache; a literal `c:v` in its `c:v`; a name the model **dropped** takes
  the whole optional `c:tx` with it; and a name the model **added** to a series that never had one
  is written as a literal `<c:tx><c:v>…</c:v></c:tx>`, spliced in right after `c:order`, which is
  the one legal place for it. It is a literal and not a reference on purpose: there is no cell
  behind that name, and minting a `c:f` for it would claim a provenance nobody stated.
- **Escaping.** Categories, series names and format codes are text from the model. They go through
  `write_text::esc_text`, the crate's existing discipline, and `a_hostile_label_reaches_the_chart_part_escaped`
  puts `A & B <x>`, `"Q1" & <b>` and `0.0 & "%"` through a whole round trip.

**The edits cannot overlap, and the code says so rather than trusting it.** Every edit is a distinct
child of a distinct `c:ser`, so they are disjoint by construction — but `splice` drops an
overlapping edit **without a word** ([[62-phase-15-package]]), which makes an overlap a silent
corruption rather than a degradation. `overlapping()` checks the sorted ranges before splicing and
refuses the whole chart with a warning if two ever touch. It has never fired; it exists because the
failure it guards against is invisible.

## `c:f` is restated, not left stale

A cache one point longer than the range it names is a file that says two things about one column.
The decision is to **restate the formula**, and the argument that decides it is not tidiness:
PowerPoint re-reads `c:f` when a user opens «Edit Data», so a stale range silently truncates the
very edit that was just made. Leaving it stale would produce a file that renders the new value and
loses it the moment anyone looks at where it came from.

Restating it needs no more than 16-A's reader already understands. The sheet prefix is kept
**verbatim** — quoting, spaces, apostrophes and all, because the writer never has to parse what it
does not change — and only the end of the range moves, along the axis the range already runs on: a
column grows down, a row grows across. The `$` anchors of the token being replaced are mirrored, so
`$B$2:$B$4` becomes `$B$2:$B$5` and `B2:D2` becomes `B2:C2`. A range down to one point becomes the
single cell it is, rather than a range of one.

**Refused, by name and with the formula quoted**: a rectangle (`$A$1:$B$4` — guessing which way to
grow it is guessing), a 3-D reference, a defined name, and a count of zero. Those keep their cache
edit out of the file too: the part is copied whole.

## What is refused, and why each is a refusal rather than a splice

Each is one `Warning::Degraded` whose `what` names the **chart's own part** — «chart
`ppt/charts/chart1.xml`» — and not the slide's number: a chart's data is in a file of its own, and
«slide 1» is not something a caller can go and look at. Each of the first three leaves the part
exactly as it was.

- **A series added or removed.** That is a `c:ser` element appearing or vanishing, with `c:idx` and
  `c:order` renumbered across every plot of a combo chart, a `c:spPr`, a `c:dPt` and a `c:dLbls`
  invented for a series that was never drawn, and every `c:f` in the chart restated. It is a
  rewrite of the part, not a splice — and an invented `c:ser` is exactly what PowerPoint offers to
  repair, which is risk P2 and the thing this whole phase is measured on. The warning names the
  chart and both counts.
- **The chart's kind.** `barChart` to `lineChart` moves the series into a different plot element,
  with a different content model and different axes.
- **The category axis emptied.** Removing it takes every `c:cat` out of every series.
- **The title.** It is a DrawingML text body whose run properties — size, font, colour — the IR does
  not hold. Writing the model's plain text over it would silently restyle the chart. This one does
  *not* block the data edits: the values are still spliced and the title is named.
- **A `c:cat` that states labels of its own** which the document's single axis never carried. 16-A
  reads `ChartRef::categories` from the *first* `c:cat` that has a cache, so overwriting the others
  would write data the model was never shown. Named per series; multi-level axes fall in here too,
  which is the write-side face of 16-A's documented read loss.
- **Markup the document carries for the chart part that differs from the part.** Compared against
  the preserved bytes rather than assumed, so it says nothing on the normal path — a directly-read
  deck carries the part verbatim, and a DocMark-parsed deck carries no payloads at all
  ([[68-phase-15-template]]). It fires only when someone really hand-edited a chart's XML, which is
  a part outside `rebuilt_parts` and therefore copied.

## The embedded workbook is preserved, and the divergence is named

The choice was between rewriting `ppt/embeddings/workbookN.xlsx` through `xlsx::write` and
preserving it with a warning. **Preserved**, for two reasons, and the second decides it:

1. `xlsx::write` rebuilds a whole package. Nothing of the source workbook that the workbook IR does
   not model — its styles, its defined names, its other sheets, a chart or a picture inside it —
   would survive, and the bytes would differ whether or not anything changed. That is spike P3's
   measurement one scale down, inside a file that is itself an OPC package.
2. **The model does not hold a workbook.** It holds one cached column per series, which is not the
   sheet those numbers came from: `charts-embedded`'s sheet has a header row, a `Región` column and
   cells no series references. Writing the caches back as a sheet would not *restore* the
   provenance; it would invent one and destroy the real one. That is the same argument 16-A used to
   make the cache authoritative, read in the other direction.

And the state this leaves is neither a corruption nor a novelty: it is exactly
`corpus/pptx/charts-stale-workbook.pptx`, the deck 16-A generated on purpose because it is what a
real deck looks like after somebody edits the embedded sheet. Our own reader describes it correctly
on the way back in, which is the test that the file is honest about itself.

The warning fires **only** when a splice actually happened and only when there is a workbook to
disagree — `a_chart_with_no_workbook_says_nothing_about_one` edits `charts-multi-series`, which
embeds none, and asserts the silence.

## The generated deck

`write_gen.rs` turned a chart into a text box carrying its title (15-G decision 6, «markup no model
holds and no template can supply»). Since 16-A that is no longer true, so a chart on a generated
slide is now a chart: `ppt/charts/chartN.xml` written whole, its `p:graphicFrame` on the slide, the
relationship in the slide's `_rels`, and the `Override` in `[Content_Types].xml`. `content_type_of`
learned a third part kind and `Parts` a second method, `chart(xml) -> rel id`, the same one-call
shape as `image` — the generator writes markup and knows nothing about where the package puts it.

The element order is stated in the code rather than discovered in the gate, because `c:chartSpace`
is strictly ordered all the way down: `c:chart` is `c:title?, c:autoTitleDeleted?, …, c:plotArea,
…, c:plotVisOnly?, c:dispBlanksAs?`; a plot is its own leading children, then `c:ser*`, then
`c:axId`; a `c:ser` is `c:idx, c:order, c:tx?, …, c:cat?, c:val?`. Five plots are generated —
`barChart`, `lineChart`, `areaChart`, `pieChart`, `doughnutChart` — and **a pie and a doughnut state
no `c:axId` and their plot area declares no axes**, which is not cosmetic: a `c:catAx` beside a pie
is a part the schema rejects. Anything else (`scatterChart`, whose series are `c:xVal`/`c:yVal` and
not `c:cat`/`c:val` at all, `radarChart`, the 3-D plots) falls back to `barChart` **with a warning
naming the kind it could not draw** — as does a chart whose kind the document does not state.

The caches are written as **literals** (`c:strLit`, `c:numLit`), not references. There is no
workbook and therefore no range, and a `c:f` naming a sheet that does not exist is a dangling
reference of the kind spike P3 §2.4 measured LibreOffice accepting in silence.

**A generated chart carries no embedded workbook at all**, and no `_rels` of its own. Same argument
as above: the model holds columns, not a spreadsheet, and `xlsx::write` would fabricate one and
present it as where the numbers came from. PowerPoint draws a chart from its caches without opening
anything else — that is 16-A's whole case for the cache being authoritative — so what is lost is
«Edit Data», and it is warned per chart, but only for a chart that *had* a workbook.

One more clause is emitted for **every** generated chart, and it is deliberately not conditional:

> its chart's design: what a generated chart carries is the plot, the axes and the model's cached
> series, so its colours, its legend, its data labels and its gridlines are whatever the consumer
> draws by default — and a chart that combined more than one kind of plot comes back as a single
> `{plot}`, because the model records which kind a chart is and not which plot each series was in.

The combo sentence is the reason it cannot be conditional. `ChartRef::kind` is the *first* plot's
name and the series carry no plot of their own (16-A), so a writer looking only at the model has no
way to know a combo chart was one. Saying so always is honest; detecting it is impossible from here
and pretending otherwise would be the silent loss.

## Corpus and gates

**No fixture was added.** The three chart decks 16-A generated already carry every document-level
case this increment writes: an embedded workbook that agrees, one that does not, no workbook at all,
a combo, a gap, two format codes and a `General`, a series with no `c:cat`, and a multi-level axis.
The cases they cannot carry are *element*-level — a series named by a literal `c:v`, a series with
no `c:tx`, a range that is a row rather than a column, a rectangle — and those are unit tests over
synthetic `c:ser` elements in `write_chart.rs`, which is where they belong. Adding two more
object-heavy decks would also have moved the P4 residue average for nothing, which is the argument
16-A already made.

**`corpus/schema_gate.py` was extended**, and this is the one file outside the writer that changed.
Its `edited` variant retitles every slide, which until now meant that the preserved-package path
never produced a chart part and the gate never validated one. `bump_chart_values` now also adds a
digit to every number in a chart's data table, so the splice's output goes through the XSDs on every
run. It went green on its first attempt: **19 decks, 38 packages, 340 parts, 0 invalid** — 334 at
the 16-B baseline plus three generated chart parts and three spliced ones.

## Known losses, written down rather than discovered later

- **The chart's kind, its title, its series count and an emptied axis are not written into a
  preserved chart**, each with its own warning (above).
- **A generated chart has no design and no workbook**, and a generated combo chart is one plot.
- **A `c:extLst` on a rewritten cache survives** (it is copied verbatim from the source), but a
  `c:dPt`, a `c:trendline` or a `c:errBars` that referred to a point the edit removed is left
  pointing at an index that no longer exists. The part stays schema-valid — those elements state an
  `c:idx` and nothing checks it against `c:ptCount` — and rewriting them means modelling them
  first.
- **A `c:f` restated on the way out is not re-read against the workbook.** The workbook is preserved
  and now genuinely disagrees; that is the warned state, and 16-A's reader reports it on the way
  back in.
- **`standard` still does not write a chart back into a preserved part**, for the reason 16-B gave:
  `format=` is not written there, and a deck at `standard` carries no `skeleton:` either, so it is
  generated whole and the question does not arise.

## Verification

- `cargo test --workspace`: **724 passed, 0 failed, 1 ignored**. The 16-B baseline was 686; fifteen
  of the thirty-eight new tests are this increment's (ten in `pptx::write`, five in
  `pptx::write_chart`) and the other twenty-three came in with 16-H, which was written in the same
  working copy at the same time and is now this change's parent commit.
  `cargo test -p docsai-office --lib` alone, which is where every one of this increment's tests
  lives: **195 passed, 0 failed**.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`:
  clean. `python3 corpus/generate.py --check`: exit 0, 135 generated files.
- `python3 corpus/schema_gate.py`: **19 decks, 38 packages, 340 parts, 0 invalid, 0 not measured**.
- `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer`:
  *19 decks, 19 round trips identical by bytes, 63 pages compared, 0 failures, 0 not measured*.
- **Byte identity by hand, outside the suite**: every deck of `corpus/pptx/` (19, `.pptm` included)
  converted to DocMark and back and `cmp`ed against the original — **19 identical, 0 differing**.
- **The edit path end to end, outside the suite**: `charts-embedded.pptx` → `.dmk.md`, `980` retyped
  as `2501` in the table by hand, converted back. Exactly **one** ZIP member differs
  (`ppt/charts/chart1.xml`), and it differs in exactly the one `c:v`; the workbook, the `_rels`, the
  content types and the slide are byte for byte the originals. (a) Our own reader reads `2501` back
  out of the written package. (b) `LD_LIBRARY_PATH=… soffice --headless --convert-to pdf` renders
  it, and the rasterised page shows a bar of 2501 where one of 980 stood. (c) `schema_gate`'s own
  validator over the part the writer changed: `ppt/charts/chart1.xml` ok.
- **The generated path by hand**: the same deck with `skeleton:` removed, written whole on the
  embedded template, rendered by LibreOffice as a real bar chart with the right categories and the
  right numbers — where 15-G drew a text box saying nothing. `charts-multi-series` likewise, as one
  line chart of three series with the gap drawn as a break.

## Found, not fixed

**`docsai convert` writes its sidecar assets beside the *input*, not beside the output.** Converting
`corpus/pptx/basic-slides.pptx` to a `.dmk.md` in another directory leaves
`corpus/pptx/assets/_skeleton/deck-<hash>.pptx` behind — running the byte-identity check over the
corpus by hand littered the corpus with twenty-two files. Nothing to do with charts, and not this
increment's to fix; written down rather than discovered again.

`docsai-mcp` and `docsai-convert` were being edited for 16-H in the same working copy while this
increment was written, and `crates/docsai-mcp/src/input.rs` did not compile for part of that time.
Nothing here touches either crate, and 16-H has since landed as this change's parent; the numbers
above are all from the tree with it in.

## Next

16-D — the objects that stay opaque, confirmed end to end.
