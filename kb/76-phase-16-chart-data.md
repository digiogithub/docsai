---
tags:
    - phase-16
    - pptx
    - charts
    - model
    - assets
    - warnings
---
# 76 — Phase 16-A: the chart says what it plots

First increment of [[75-phase-16-plan]]. Until now a chart was a rule-8 stub over a preserved part
([[42-phase-13-skeleton]], [[43-phase-13-raw]]): the deck round-tripped byte for byte and an agent
reading it learned the word `barChart` and nothing else. `ChartRef` now carries the categories and
the series, the title, and the embedded workbook as a real asset — read out of
`ppt/charts/chartN.xml` by `docsai-office::pptx::chart`.

Nothing about writing changed. `raw` still captures `c:chartSpace` verbatim, the skeleton still
preserves `ppt/charts/*`, and every deck of the corpus still round-trips byte for byte
(`corpus/render_gate.py`: 19 decks, 19 identical by bytes). That was the condition of the
increment: if reading a chart as data moved one output byte, 16-C would have been broken before it
started.

## The chart carries its data twice, and the cache wins

`corpus/pptx/charts-embedded.pptx` states the problem exactly. The numbers are in
`c:strCache`/`c:numCache` inside the chart part, and again in `ppt/embeddings/workbook1.xlsx`,
reached through the chart part's **own** `_rels` by a relationship whose type ends in
`/relationships/package` — one level further in than the frame's `r:id`, which resolves against the
slide.

**The cache is authoritative for the IR.** It is present in every valid chart, it is what
PowerPoint draws without opening anything else, and a chart whose workbook is linked, missing or
unreadable still has to read. The workbook is read *as well*, for provenance: its bytes become an
`AssetId` in `ChartRef::workbook`, and its cells are compared against the caches. When the two
disagree the file is telling the truth about itself — somebody edited the sheet and the chart was
never redrawn — so the disagreement is **warned, never averaged**. `charts-stale-workbook.pptx` is
that deck, generated on purpose.

## The type, and what it refuses

```rust
pub struct ChartRef {
    kind: Option<String>,       // barChart, lineChart — the first plot names the chart
    title: Option<String>,
    workbook: Option<AssetId>,
    categories: Vec<String>,    // shared by every series: that is what c:cat is
    series: Vec<ChartSeries>,   // every c:ser of every plot, in document order
    raw: Option<RawId>,
}

pub struct ChartSeries {
    name: Option<String>,
    values: Vec<Option<f64>>,   // positional against `categories`; None is a gap
    number_format: Option<String>,
}
```

Four decisions are load-bearing:

- **Categories live on the chart, not on the series.** `c:cat` repeats the same reference on every
  series because it *is* one axis. A category list per series would invite a writer to give two
  series different axes, which no chart part can express.
- **A value is `Option<f64>`, and a gap is not a zero.** `c:pt` indices are sparse and `c:ptCount`
  is the length of the column, not the number of points in it. A reader that pushed the points it
  found would put every value after a hole on the wrong category, and one that wrote `0.0` would
  draw a line through the floor where PowerPoint leaves a break.
- **A category-less chart is representable**: `categories` is empty and the series are read by
  position. A scatter or a bare `c:val` is not an error.
- **`c:formatCode` is kept.** It is the difference between `0.42` and `42 %`. Without it the
  numbers can be re-rendered, but not as the slide renders them, and a currency or a date axis
  reads as a bare float. It sits on the series because `c:numCache` does.

**Refused: the `c:f` formulas.** A series knows the range it came from (`Hoja1!$B$2:$B$4`), and
that string is *not* in the model. It is used inside the reader to check the workbook and then
dropped, because the only consumer that needs it is 16-C's splice — and 16-C edits inside the
preserved `chart1.xml`, where the formula still is. Storing it would create a second copy that a
DocMark hand-edit could contradict.

**Refused: a multi-level category axis.** `c:multiLvlStrRef` is a tree («2026 / Q1») and
`categories` is a list. Reading only the bottom level would silently change what the axis says, so
the labels are not read and the loss is warned. `charts-multi-series.pptx` carries one.

## The warnings, and exactly when each fires

No new `Warning` variant was added. None of the four cases is something a consumer branches on that
the existing variants cannot express, and `ChartRef::workbook` being `None` already answers the
only structural question a writer asks.

| When | Warning | Severity |
|---|---|---|
| The workbook and a cache disagree, cell by cell | `Degraded { what: "chart `part`", why: "…disagree (series 1 values point 1 is 999 in the workbook (`B3`) and 980 in the cache)…" }` | Minor |
| The chart declares no embedded workbook, or links an external one | `Degraded`, naming the chart and saying the caches are all the provenance there is | Minor |
| The workbook part is missing, unstorable, or unreadable as a package | `AssetIssue { asset, why }` | Severe |
| A series' categories are `c:multiLvlStrRef` | `Degraded`, naming the series | Minor |
| Cached values that are not numbers (`#N/A`), counted per chart | `Degraded`, saying they are kept as gaps | Minor |

Absent is deliberately *not* an `AssetIssue`: nothing was lost — the chart's data is complete
without the workbook — and `AssetIssue` is `Severe` everywhere else in the crate because a part
that exists could not be kept. A chart that simply has no embedded sheet is not that.

**The other half of rule 3 is graded too, and it is a test.** `charts-embedded.pptx` — where the
two copies agree — must produce *no* chart warning at all, and
`a_chart_reads_the_series_its_caches_hold` asserts it. Three things exist only to keep that
promise:

- A **date** cell is not comparable. The xlsx reader turns a date-formatted serial into an ISO
  string and the chart caches the serial; comparing them would report a difference that is not
  there. `numeric()` returns «not comparable» rather than «different», and the same rule covers
  errors and booleans in a numeric column.
- Numbers are compared with a relative tolerance. The two copies are the same number written twice,
  and a reader that warned about the last bit of `0.1 + 0.2` would be crying wolf.
- A reference this reader cannot resolve — a 3-D range, a defined name — is skipped, and nothing is
  claimed about it.

## `.xlsx` in the asset store, and a name that was a lie

`AssetStore` sniffs the content to name what it holds, and it knew only bitmaps: the chart's
workbook would have been stored as `img-48b54984.bin`. Two fixes, both in
`docsai-model::assets`, both defects this increment would otherwise have introduced:

- `sniff` recognises a ZIP and, inside it, an OPC package. The local file headers of a ZIP record
  every entry's name uncompressed, so `xl/workbook.xml`, `word/document.xml` and
  `ppt/presentation.xml` are findable in a bounded scan with no unzip at all — `docsai-model` has
  none and wants none.
- `describe` names a non-image `asset-<hash8>.<ext>`. `img-` for a workbook is false in every
  manifest, every `assets/` directory and every DocMark reference. No existing golden carried a
  non-image asset, so nothing else moved.

`inspect` also counts the chart as a reference to its workbook: a manifest entry reporting zero
references would say the deck holds a file nothing points at.

## What moved that the plan said would not

[[75-phase-16-plan]] says of 16-A: «The `.expected.inspect.json` goldens move; no DocMark golden
does yet.» **One DocMark golden moved**, and it is worth naming rather than hiding:

```diff
-::: {#n2 .chart kind=barChart name="Gráfico 1" …}
+::: {#n2 .chart data="assets/asset-48b54984.xlsx" kind=barChart name="Gráfico 1" …}
```

No writer code was touched. The `data=` branch at `deck_writer.rs:288` has existed since 14-E and
fires on `ChartRef::workbook` being `Some`, which is precisely what this increment made true. The
alternatives were to leave the workbook unread (the increment's own requirement) or to edit the
writer to suppress it (16-B's file). Both are worse than one attribute in one golden. The chart's
**table** — the body of the container — is still 16-B's, and is still empty.

## Corpus

Two fixtures added, both generated (`corpus/generate.py`; `AGENTS.md` §6 rules real documents out):

- **`charts-stale-workbook.pptx`** — the chart of `charts-embedded` with a workbook that says `999`
  where the cache says `980`. Not a corrupt file: what a deck looks like after someone edits the
  embedded sheet. Proves the divergence warning fires and that the cache is what reaches the IR.
- **`charts-multi-series.pptx`** — a title in rich text, two plot elements, three series numbered
  across both, a series that states no `c:cat` of its own, a hole in a column, a `#,##0` format
  code, a multi-level category axis, and **no embedded workbook at all**. Everything a single-series
  bar chart cannot exercise.

Both carry a body placeholder with prose beside the chart. That is not decoration: the residue
budget of risk P4 (`plain_residue.rs`, 18 % over the deck corpus) is a corpus-wide average, and a
deck whose only content is an object is not what a deck looks like. Two more object-only fixtures
would have pushed the average to 19.6 % and the honest fix is a representative fixture, not a
raised budget.

`every_deck_in_the_corpus_gets_through_the_package_layer` used to assert that no deck produces any
`Warning::Degraded`. Two of them now do, on purpose. The assertion was narrowed rather than
deleted: a `Degraded` warning is still a package-layer failure unless it names a chart.

## Known losses

- A multi-level category axis: the labels are not read (warned).
- A cached value that is not a number is a gap (warned, counted).
- The workbook's own read warnings are discarded. They are about the inner workbook — its macros,
  its styles — and attributing them to the slide would be a lie of a different kind. The workbook
  is also read into a **throwaway asset store**, so a picture anchored inside it is not a picture
  of this deck.
- Comparison is skipped for a range wider than 4 096 cells, and for a reference this reader cannot
  resolve. The caches — which are the IR — are unaffected either way.
- The series are not visible in `docsai inspect`, which reports counts and not chart contents. They
  become visible in 16-B, as the DocMark table the specification already describes
  (`docs/docmark-specification.md:801`).

## Next

16-B — the chart in DocMark.
