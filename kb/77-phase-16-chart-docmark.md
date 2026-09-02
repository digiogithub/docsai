---
tags:
    - phase-16
    - docmark
    - presentations
    - charts
    - specification
    - goldens
---
# 77 — Phase 16-B: the chart in DocMark

Second increment of [[75-phase-16-plan]], on top of [[76-phase-16-chart-data]]. 16-A put the
series in the IR and nothing could see them: `docsai inspect` counts shapes, and the `.chart`
container's body was still the title and nothing else. This increment gives them a form a person
can read and edit — the GFM table `docs/docmark-specification.md` has reserved since Phase 14 — and
a parser that reads it back.

Nothing about the pptx **writer** changed. A chart's table is not yet written into
`ppt/charts/chart1.xml`; that is 16-C. The 19 decks of the corpus still round-trip byte for byte
(`corpus/render_gate.py`: *19 round trips identical by bytes, 63 pages compared, 0 failures*),
which was the condition of the increment: reading a chart as a table must not move an output byte.

## The table, and the six rules that are the format

```markdown
::: {#n2 .chart data="assets/asset-48b54984.xlsx" kind=barChart}
Sales by region

|       | North {format="#,##0"} | South |
| ----- | ---------------------- | ----- |
| Q1    | 1200                   | 800   |
| Q2    | 980                    |       |
| Q3    | 1440                   | 910   |
:::
```

1. **Categories down the first column, one column per series.** The categories are the chart's, not
   a series' — that is what `c:cat` is, and 16-A made the same decision in the model for the same
   reason. A column of categories per series would let a document state two axes that no chart part
   can express.
2. **The first header cell is empty.** The category axis has no title anywhere in `ChartRef`, so
   there is nothing true to put there. `Category` was refused: it is a word in one language that
   the source file never said, it would differ between an English and a Spanish deck read from the
   same package, and a reader would then have to decide whether the first column of a *hand-typed*
   table is an axis heading or a series name — which is exactly the question the empty cell
   answers. What a person does type there is dropped and **warned** (`Warning::Degraded`, Minor):
   our own writer never writes one, so the warning fires only when something is really lost.
3. **An empty cell is a gap.** `None` is not a zero and not an empty label; a blank cell is what a
   spreadsheet means by the same thing, and it is what a person types. A zero is `0`. This is the
   one rule the whole increment exists to protect: 16-A went to some trouble to keep `c:ptCount`'s
   holes, and writing them as `0` here would have thrown that away one layer up.
4. **Trailing empty category cells are not categories.** One rule, two shapes. A **category-less**
   chart — a scatter — writes a first column that is empty from top to bottom and reads back with
   no categories at all; a series *longer* than the axis writes rows whose category cell is empty
   and does not grow the axis on the way back. The alternative, omitting the first column for a
   category-less chart, was refused: the parser would then have to guess from the header whether
   column one is an axis or a series, and a hand-typed `| Quarter | Sales |` would read `Quarter`
   as a series of unreadable values.
5. **`format=` rides on the series header, at `full` and `agent` only.** It is `c:formatCode`, and
   16-A kept it because it is the difference between `0.42` and `42 %`. It is *not* written at
   `standard` for a reason that is measurable rather than stylistic: it is the one attribute block
   in this design that renders **inside a table cell**, and `plain_residue.rs` forbids `standard`
   any residue that is not the slide marker or a `:::` fence. Dropping it there costs nothing that
   was going to be spent — `standard` does not write back, exactly as with `raw=` and `pos=`.
6. **A value that is not a number is a gap and a warning.** `1.200,00` is a real thing a person
   types; reading it as `1200` guesses a locale and reading it as `0` plots a point nobody wrote.
   `Warning::UnsupportedElement` (Severe), naming the cell.

**The title and the table in one body.** A stub's body has always been «the text the object shows»,
and `plain` still writes exactly that. With a table under it the two have to be told apart, and
they are told apart by the **delimiter row** — not by position, not by a marker. That is what makes
the parser tolerant in the way the profile requires: a table typed unpadded, with no leading pipes,
before or after the title, reads as the same series. Making the title an attribute instead was
refused: it is visible text, `plain` writes it as visible text, and an attribute would have moved
it out of the one level whose whole purpose is what the slide shows.

## What was refused

- **A `Category` heading in the first column.** Rule 2 above.
- **Suppressing `format=General`.** `General` is the default format code, and writing it on nearly
  every series is noise — the goldens carry it three times. But `Some("General")` and `None` are
  different states of the model, and «General means no format» is a normalisation rule with no
  source: a chart part that states `General` said something, and a round trip that turned it into
  nothing would be an edit nobody asked for. Written as it is.
- **Rounding values.** `f64`'s own `Display` is used, which is the shortest text that reads back as
  the same number. `units::number` — the two-decimal helper the geometry attributes use — would
  have rounded `1440.5678` to `1440.57` and changed what the slide plots.
- **A field for the axis title.** Adding one to `ChartRef` to catch the word a person types in the
  corner cell would be 16-A's file and a model change no reader fills in.
- **Reusing `parse_gfm_table`.** It returns a `Table` of rich cells and discards the cell attributes
  it does not know, which is where `format=` lives. The chart's table is parsed by a function of its
  own, twenty lines, which also lets a gap stay a gap instead of becoming an empty paragraph.

## The specification, edit by edit

Three edits to `docs/docmark-specification.md`, all in §11.2, each argued because §7 rule 2 forbids
bending the format to the code:

1. **A new section, *A chart's data table*** (after the stub attribute table). It did not exist and
   had to: the six rules above are decisions a second implementation must make identically, and a
   format whose only statement of them is a Rust function is not a format. It adds no rule to any
   existing construct and weakens none.
2. **The *Not yet implemented* paragraph, rewritten.** It named Phase 16 as what would fill the
   chart in, which is now done, and it carried a sentence that had become false — «the pptx reader
   of Phase 13 keeps [the workbook] inside the skeleton instead, so today [`data=`] is the exception
   rather than the rule» — which 16-A's reader had already made untrue. What replaces it says the
   two things that are still open and why: **SmartArt stays a stub**, because a diagram's meaning is
   its layout algorithm and a list of its node texts would read as a list; and a chart's
   multi-level axis and its `c:f` formulas are not written, with the reason for each.
3. **One paragraph saying this is not a version bump.** 1.2 defined the `.chart` stub with a
   free-text body and named this table as what would fill it. A 1.2 reader that knows nothing of it
   sees a GFM table where it expected a label: it keeps the title, renders the numbers, and loses
   only that they are a series. That is the additive direction the version rule already promises,
   so `docmark: "1.2"` stands and no migration is owed.

## The goldens, and what they cost

Three `.expected.dmk.md` moved, refreshed with `DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert
--test goldens` and reviewed by hand. `charts-multi-series` is the whole design in five lines: three
series across two plot elements, the hole in `Sur`, two format codes and one `General`.
`.expected.inspect.json` did not move — `inspect` reports counts, not chart contents.

Residue, measured by `plain_residue.rs` over the 19 decks at `standard`:

| | before | after | budget |
|---|---|---|---|
| corpus-wide | 17.6 % | **17.3 %** | 18.0 % |
| the decks that are documents | 14.1 % | **13.8 %** | 15.0 % |

It went **down**, which is the honest outcome and worth stating: the table is *content* — a plain
viewer renders it as a table of numbers — so it adds to the denominator and nothing to the residue.
`charts-embedded`, whose whole slide is one chart, falls from about 82 % to 61 %. Nothing here
touched the budget, and nothing needed to.

## Known losses

- **A category axis label** typed by a person: dropped and warned. The model has nowhere for it.
- **A chart whose every category label is genuinely empty** reads back as category-less. That is
  the price of rule 4, and the case is degenerate: 16-A only produces an empty label for a point
  the cache leaves out of an otherwise present axis.
- **A ragged chart is normalised.** A series shorter than the axis reads back padded with gaps to
  the table's row count. Semantically that is the same series — the values are positional and a
  missing one *is* a gap — but the `Vec` lengths differ from the source, so the identity is at the
  chart's meaning, not at its representation.
- **`format=` is not written at `standard` or `plain`**, and the table itself is not written at
  `plain` (the chart is its title there, with the warning that already existed).
- **A multi-level axis and the `c:f` formulas** are not in the document. The first is 16-A's
  documented loss and is warned at read time; the second is deliberate — 16-C splices inside the
  preserved `chart1.xml`, where the formula still is, and a copy in DocMark could contradict it.

## Verification

- `cargo test --workspace`: **686 passed, 0 failed, 1 ignored** — the 679 of the 16-A baseline plus
  the seven tests added in `crates/docsai-docmark/tests/presentation_objects.rs`.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`:
  clean. `python3 corpus/generate.py --check`: exit 0.
- `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer`:
  *19 decks, 19 round trips identical by bytes, 63 pages compared, 0 failures, 0 not measured*.
- `docsai roundtrip` on the three chart decks: second DocMark matches the first, every category at
  100 %.
- By hand: a `.dmk.md` typed with no front matter, no ids, no `kind=`, no padding and no leading
  pipes parses into the series it states and comes back canonicalised — the axis heading dropped
  with its warning, and the short row read as a gap.

## Found, not fixed

`parse_shape` warns with the **slide's** line number, not the container's: a chart table on line 9
reports «line 1». Every warning that function raises has always done this, so it is a defect of the
deck parser and not of this increment, and fixing it means threading a line offset through
`split_one_fence`. Written down here rather than folded into a chart commit (§7 rule 1).

## Next

16-C — the chart written back.
