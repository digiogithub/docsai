---
tags:
    - phase-16
    - close
    - charts
    - comments
    - thumbnails
    - cache
    - risk-p2
---
# 84 — Phase 16 I: the phase closed, with the same box still open

Increment **16-I** of [[75-phase-16-plan]], the ninth and last. It adds no feature. What it owes is
the discipline of [[72-phase-15-close]]: the three acceptance criteria answered one by one with
evidence **re-run here**, what the phase carries forward written down rather than discovered later,
and the state of risk P2 said plainly rather than implied.

Eight increments, in the order they landed: `6291ecb` 16-A, `15422e5` 16-B, `6f0f4ef` 16-H,
`78b792c` 16-C, `e3b87b9` 16-D, `58c6ba3` 16-G, `0267619` 16-E, `7517f9d` 16-F.

## What the phase set out to do, and what it did

The objective as `docs/development-plan-v2.md:289` states it: **the content classes that survive as
data rather than as opaque XML.** Three of them did, one was confirmed as staying opaque, and one
increment was a pure optimisation:

- **A chart is data.** `ChartRef` carries categories, series, format codes, a title and the embedded
  workbook as an asset (16-A); DocMark writes them as a GFM table a person can edit (16-B); and an
  edited table is **spliced inside the preserved `ppt/charts/chartN.xml`**, with `c:ptCount`,
  `c:pt@idx` and the `c:f` ranges kept consistent, or refused by name when the edit is a rewrite
  rather than a splice (16-C).
- **A comment is data**, in both formats, through **one IR node and one DocMark syntax** designed
  once — `docs/docmark-specification.md` §11.3, DocMark **1.3** (16-E) — and written back by both
  writers (16-F), the pptx one by splicing the part that was read and the docx one by regenerating
  the package it always regenerated.
- **The objects that stay opaque were confirmed end to end** rather than assumed: a `p:oleObj`
  fixture the corpus never had, and eleven assertions over read, write and the loop through DocMark
  and back (16-D).
- **A slide has a picture.** `--thumbnails` on the CLI and a `render_slides` tool on the MCP server,
  through headless LibreOffice and `pdftoppm`, with the renderer's availability decided by *running*
  it (16-G).
- **The conversion cache** removes the re-parsing four MCP calls on one document used to pay
  (16-H).

Underneath all of it the property Phase 15 established never moved: **a deck nobody edited comes
back byte for byte.** It was 17 decks at Phase 15's close and it is **21** now, and each of the
eight increments held it before it was allowed to land.

## The three acceptance criteria, each with evidence re-run in this increment

Nothing below is quoted from an increment's report. Every number was produced by a command run
here, today, on this tree.

### 1 — «Chart data readable as a table and editable through DocMark; chart survives round-trip» ✔

**Readable.** `docsai convert corpus/pptx/charts-embedded.pptx -o deck.dmk.md` writes the chart as
the container the specification reserved:

```markdown
::: {#n2 .chart data="assets/asset-48b54984.xlsx" kind=barChart name="Gráfico 1" pos="88px,143.75pt" raw=raw-0001 size="784px,10cm"}
|       | Ventas {format=General} |
| ----- | ----------------------- |
| Norte | 1200                    |
| Sur   | 980                     |
| Este  | 1440                    |
:::
```

**Editable.** `980` retyped as `2501` in that table by hand and converted back to `.pptx`. The
written package has the **same sixteen members in the same order**, and exactly **one** of them
differs: `ppt/charts/chart1.xml`. The embedded workbook, the chart's `_rels`,
`[Content_Types].xml` and every slide part are byte for byte the originals. Reading the written
deck back gives `2501` in the same cell of the same table.

**Survives the round trip.** Two measurements, because one of them is the gate and the other is the
gate's own claim checked by hand:

| what | result |
|---|---|
| `LD_LIBRARY_PATH=… python3 corpus/render_gate.py --require-renderer` | **21 deck(s): 21 round trip(s) identical by bytes, 0 rendered, 67 page(s) compared, 0 failure(s), 0 not measured** |
| every deck of `corpus/pptx/` converted to DocMark and back and `cmp`ed, outside the suite | **21 identical, 0 differing** (20 `.pptx` + `macro-enabled.pptm`) |

The second is the honest form of the first: the gate short-circuits a byte-identical round trip
rather than rendering it, so the byte claim is the gate's and the render claim is not.

### 2 — «Thumbnails generated for the corpus when LibreOffice is present; clean warning when not» ✔

**Present.** `docsai convert corpus/pptx/*.pptx --out-dir <tmp> --thumbnails`, on this machine with
**no environment variable set at all**, exits 0 and writes **67 PNGs, 2.0 MB**, one directory per
deck at 96 dpi, naming the renderer it used (`LibreOffice 24.2.7.2 420(Build:2)`). 67 is exactly the
page count `corpus/render_gate.py` reports for the same decks in the run above — an independent
confirmation that no deck lost a slide on the way through.

**Absent.** Three ways of being absent, three warnings that are each **true**, each naming what is
missing and how to supply it, and each exiting **1**, because the user asked for pictures and did
not get them:

| invocation | message | exit |
|---|---|---|
| `DOCSAI_LIBREOFFICE=/nope/soffice` | `slide thumbnails skipped — LibreOffice was not found: install it, or set DOCSAI_LIBREOFFICE to the soffice binary` | 1 |
| a stub that prints the loader error and exits **0** | `slide thumbnails skipped — \`…/fake-soffice\` is installed but does not run: soffice.bin: error while loading shared libraries: libreglo.so…` | 1 |
| `DOCSAI_PDFTOPPM=/nope/pdftoppm` | `slide thumbnails skipped — pdftoppm was not found: install poppler-utils… LibreOffice alone exports one page of a deck, so the rasteriser is not optional` | 1 |

The middle row is the one the criterion's word *clean* is about. This machine's LibreOffice is
installed, complete and able to render, and plain `soffice` prints a loader error and **exits 0**
([[71-phase-15-render]]). A file-existence check would have called it present and failed mid-corpus;
a `which`-shaped check would have said «LibreOffice is not installed» on a machine where it is. Both
are false statements, and 16-G replaced the check with a measurement.

### 3 — «Cache demonstrably removes re-parsing in a repeated-call benchmark, and disabling it changes no output» ✔

`cargo test -p docsai-convert --test cache -- --nocapture`, all ten tests green, printing:

```
hit cost on forty-slides.pptx: one read 19.914571ms, one clone of the tree 79.786µs
repeated-call benchmark on forty-slides.pptx × 20 outline calls
  without cache: 706.863281ms (35.343164ms per call)
  with cache:    351.203227ms (17.560161ms per call)
  reads: 20 uncached vs 1 cached
```

**Re-parsing removed**, counted rather than timed: 20 uncached reads against **1** cached, which is
the assertion; the times are printed because a wall-clock assertion on a shared machine is a coin
toss. A clone of the tree is **250×** cheaper than the read it replaces. The end-to-end win on
`outline_document` is 2.01×, not 250×, and the reason is worth restating at the close: an outline
also serialises the document and counts its tokens on every call, and the cache removes none of
that. The cache is worth what the parse was worth and no more.

**Disabling it changes no output** is `disabling_the_cache_changes_no_output`, green above: four
corpus documents × four fidelity levels × three conversions each (uncached, cold, warm), asserting
the DocMark, the assets *and* the `ConversionReport` are identical. The report is in that comparison
on purpose — a cached read that dropped the warnings would report a degradation on the first call
and stay silent on the next three, which is §7 rule 3's failure made intermittent, and intermittent
is worse than permanent.

## The gate run, in full

Every command of the increment's definition of done, run here:

| what | command | result |
|---|---|---|
| the suite | `cargo test --workspace` | **776 passed, 0 failed, 1 ignored**, exit 0 |
| lints | `cargo clippy --workspace --all-targets -- -D warnings` | clean, exit 0 |
| format | `cargo fmt --all -- --check` | clean, exit 0 |
| the corpus is what its generator says | `python3 corpus/generate.py --check` | **143 generated files**, exit 0 |
| CI gate 1 | `python3 corpus/schema_gate.py` | **21 decks, 42 packages, 363 parts validated, 0 invalid, 0 not measured**, exit 0 |
| CI gate 2 | `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer` | **21 decks, 21 round trips identical by bytes, 0 rendered, 67 pages compared, 0 failures, 0 not measured**, exit 0 |

The one ignored test is the inherited Phase 14 criterion (`--fidelity agent` ≤ 15 % of `full`),
carried below as item 24. `666/1/1` at Phase 15's close is `776/0/1` here: the red
`docx_roundtrip_is_idempotent` was fixed in [[74-docx-bookmark-node-ids]] before this phase opened,
and 110 tests were added across the eight increments.

## Risk P2: still OPEN, and what that means for Phase 17

`kb/72` wrote that Phase 16 «should not start before the P2 form is signed: a chart data model built
on top of a package PowerPoint repairs would be built on sand.» **The form is still unsigned.**
`docs/spikes/P2-powerpoint-gate.md` needs PowerPoint and a person, and this machine has neither. The
owner directed the phase to start anyway, and [[75-phase-16-plan]] recorded that in writing rather
than in a commit message.

What the phase owed in exchange, it paid: **every increment kept byte-identity**, so the form is
still re-runnable at any time, on any commit of this phase, with the same three decks and the same
negative control that `kb/72` built. The `--thumbnails` of 16-G does not change this either way —
LibreOffice is not PowerPoint, and a thumbnail shows what LibreOffice drew.

So what is proven at this close is what was proven at the last one, over a larger corpus: 21
byte-identical round trips, 363 parts of 42 packages schema-valid, every relationship resolving, 67
pages rendered by a real consumer without a failure or a blank page — and **nothing at all** about
PowerPoint's own loader, which is stricter than every one of them.

**For Phase 17 this is not a formality.** `apply_edits` is the increment where an agent, not a
person, changes a document and hands the bytes back. Everything Phase 16 built is now edit surface —
a chart's numbers, a comment's text, an author's name — and each of those edits reaches the package
as a *splice into a preserved part*, which is exactly the construction P2 asks about. Phase 17 will
multiply the number of ways bytes leave this project without adding a single new piece of evidence
that any of them opens. The form should be signed before, not after; and if it is not, Phase 17
inherits the debt with interest, because a repair dialog found then will indict a much larger
surface than the one it would indict today.

## The three places a budget or a golden moved, and what each bought

Named individually because a moved number that is not argued is a loosened gate.

### 1 — 16-A's residue: paid, and it came back

`plain_residue.rs` measures what a `standard` rendering leaks that a plain Markdown viewer would
have to hide — risk **P4**'s budget, **18.0 %** corpus-wide and **15.0 %** over the decks that are
documents. 16-A added two chart fixtures and the corpus-wide figure rose — reported as 17.0 % →
17.9 % when the increment was directed, and as **17.6 %** by [[77-phase-16-chart-docmark]] measuring
the same tree one increment later; the two differ and neither is re-derivable now, so what is
recorded as evidence here is the figure this increment measured, below. What matters is the same
either way: the **ceiling was not raised**. The increment argued the alternative down in writing:
two *object-only* decks would have taken it to 19.6 %, and the honest fix is a representative
fixture — each chart deck carries a body placeholder with real prose beside the chart — not a raised
budget.

Measured here, over the corpus as it now stands (21 decks):

| | measured today | budget |
|---|---|---|
| corpus-wide | **15.1 %** | 18.0 % |
| the decks that are documents | **12.2 %** | 15.0 % |

It bought back more than it spent, and the reason is 16-B: a chart's data table is *content*. A
plain viewer renders it as a table of numbers, so it adds to the denominator and nothing to the
residue — `charts-embedded`, whose whole slide is one chart, fell from about 82 % to 61.2 %. The
later fixtures (`ole-embedded`, `comments-slide`, `comments-review`) are prose-heavy for the same
stated reason, and the corpus figure is now 2.9 points under its ceiling. **Nothing was loosened at
any point in the phase.**

### 2 — 16-G's wire-cost golden: the one place a gate's escape hatch was used

`crates/docsai-mcp/tests/wire_cost.rs` budgets how much of an agent's context the MCP surface costs
and **refuses an update that inflates the suite total by more than 5 %**. A ninth tool inflated it by
**6.6 % — 4 915 → 5 239 tokens** — and the golden was regenerated with
`DOCSAI_ACCEPT_TOKEN_INFLATION=1`, which is the hatch the gate itself documents *on condition the
increase is argued*. The budget was not lowered and the gate was not weakened; this is the only
place in the phase where the hatch was opened.

What was paid: ~320 tokens a session, of which ~260 is the `tools/list` schema and the rest is the
handshake repeated in each scenario. It was squeezed first — the tool description and all six
argument descriptions rewritten shorter, taking the figure from 7.4 % to 6.6 %.

What it bought: every other tool on this server answers in **text**. `outline_document` says a slide
has three shapes; `read_selection` says what they say. None of them can say what the slide *looks*
like — whether the chart overlaps the title, whether the text overflowed its placeholder. The images
themselves are paid only when asked for, and by default not through the context at all, because
`output_dir` puts them on disk and returns a receipt.

### 3 — 16-F's `corpus/token-budget.md`: a new fixture, not a loosening — checked, not repeated

16-F states that the corpus token budget moved «+3.3 % corpus-wide, and the golden was regenerated
deliberately». The claim was verified here rather than taken on trust, by reading the diff the
commit actually made to that file (`git show 7517f9d -- corpus/token-budget.md`). It is **three
lines**:

```diff
+| `docx/comments-review.docx` | 848 | 494 | 346 | 145 |
-| **total** | **25776** | **15549** | **17087** | **10577** |
+| **total** | **26624** | **16043** | **17433** | **10722** |
```

One new row and the total recomputed. **Not one existing document's numbers moved**, so no document
became cheaper or dearer to read: the corpus simply has one more file in it. 26 624 / 25 776 is
+3.29 %, which is **inside** that gate's own 5 % ceiling, so unlike the wire-cost golden this
regeneration needed no escape hatch at all. The claim stands as written.

There is a fourth number worth recording beside these three, because it was *not* moved when it
easily could have been. `agent_projection.rs::the_projection_never_pays_for_what_it_cannot_edit`
went to **52 %** against a ceiling of **50** when `comments-review.docx` landed, and the ceiling was
not raised: the metric is `(agent − plain) / (full − plain)`, body text cancels out of it, and
comments sit on the numerator because `plain` drops them — so the fixture was given more *`full`-only*
weight (direct character formatting, real paragraph properties), which is also what a reviewed
document actually looks like. The test is green here.

## A warning for Phase 17: the preamble, and the question it raises

Phase 17's acceptance criterion, from the plan, is **«≤ 3 tool calls and ≤ 2 000 tokens»** for a
scripted agent loop on a 40-slide deck. `crates/docsai-mcp/tests/wire_cost.rs` is the instrument
that will measure it, and its `session-preamble` row moved this phase:

| scenario | at Phase 15's close | today | Δ |
|---|---|---|---|
| **session-preamble** | 1 863 | **2 124** | **+14.0 %** |
| deck-retitle (read half) | 1 217 | 1 238 | +1.7 % |
| report-fix-typo | 998 | 1 019 | +2.1 % |
| sheet-add-row | 837 | 858 | +2.5 % |
| **suite total** | 4 915 | **5 239** | +6.6 % |

The preamble is `initialize` plus `tools/list` — the handshake a session pays **once**, before it has
asked a single question — and 16-G's `render_slides` is why it grew.

**2 124 > 2 000.** So the question Phase 17 has to answer before it can score itself is not «is the
loop cheap enough» but **«does the 2 000-token budget include the preamble a session pays once, or
only the calls the task itself makes?»** Read one way the criterion is already failed by the
handshake alone, with no work done; read the other way the three task rows above are 858–1 238
tokens each and there is room for a loop. The two readings differ by more than the entire budget of
one call, and nothing in `docs/development-plan-v2.md` §Phase 17 says which is meant.

**This close does not decide it.** Deciding it changes what the criterion measures, which is a
specification decision and Phase 17's own — the same reason the inherited `agent` ≤ 15 % criterion
has sat as an `#[ignore]`d test since Phase 14 instead of being softened. It is recorded here, with
the number, so that Phase 17 answers it on purpose and in writing rather than by picking whichever
reading its first measurement happens to satisfy.

## What Phase 16 carries forward

Collected here so the next phase finds them in one place instead of in eight increments' prose.
Every item was **reproduced in this increment** before it was written down.

### Found during Phase 16, deliberately not fixed

1. **`parse_shape` warns with the *slide's* line number, not the container's.** Every warning that
   function raises has always done this, so it is a defect of the deck parser rather than of any one
   increment. Reproduce: a `.dmk.md` whose slide heading is on line 6 and whose `.chart` container
   opens on line 10 reports `chart table at line 6`. Fixing it means threading a line offset through
   `split_one_fence`. ([[77-phase-16-chart-docmark]])
2. **`count_shape_images` in `crates/docsai-convert/src/inspect.rs` no longer only counts images.**
   It counts a chart's embedded workbook too — correctly, because the manifest would otherwise report
   a file nothing points at — and the name now lies about what the function does.
   ([[76-phase-16-chart-data]])
3. **A path input is read twice on a cache miss.** `service.rs::cache_key` does
   `std::fs::read(&canonical)` to hash the bytes, and `read_source` then opens the file again. The
   cost is one extra read of a file already in the page cache; the fix is a key built from the bytes
   the read already has, which means restructuring `read_source`'s ownership.
   ([[79-phase-16-cache]])
4. **Legacy `.doc` is never cached**, and that is a decision rather than an oversight
   (`benefits_from_loffice` gates the insert): whether a `.doc` read went through LibreOffice depends
   on what was installed *at that moment*, which is a property of the machine and not of the
   document, and no key can carry it honestly. Written down because a future reader will otherwise
   report it as a missing hit. ([[79-phase-16-cache]])
5. **`sniff` labels a `.pptm` as `.pptx`.** `crates/docsai-model/src/assets.rs`, `zip_kind`: the ZIP
   branch is a bounded **substring search** for `ppt/presentation.xml` in the first 64 KiB, not a
   local-file-header parse, and a macro-enabled deck contains that string too. It affects only the
   name and content type an asset is *stored under*, never what is read; a real fix parses the local
   file headers. ([[76-phase-16-chart-data]])
6. **A stub over `mc:AlternateContent` gets no name and no geometry, so `order::sort` puts it last
   on its slide.** `raw::name_of` and `raw::geometry_of`
   (`crates/docsai-office/src/pptx/raw.rs:96` and `:112`) look at the element's own children, and an
   `mc:AlternateContent` has neither a `cNvPr` nor an `xfrm` — both are one level down, inside each
   branch. Reproduce: `docsai convert corpus/pptx/ole-embedded.pptx -o deck.dmk.md` and compare the
   two stubs — slide 1's is `::: {#n3 .ole name="Hoja de cálculo 1" pos="4400000emu,2000000emu" …}`,
   slide 2's is `::: {#n6 .ole raw=raw-0002}` and nothing else. Harmless on both current fixtures
   (the object *is* last); wrong on a deck where it is not. Descending into a branch is what
   [[43-phase-13-raw]] decision 6 refuses on purpose. ([[80-phase-16-opaque]])
7. **A picture inside an opaque object is not extracted as an asset of the deck.**
   `corpus/pptx/ole-embedded.pptx` carries `ppt/media/image1.png` — the snapshot every renderer draws
   in place of an OLE object — and `docsai inspect --json` reports `"images": 0` and `"media": []`.
   The bytes are safe (the part is in the skeleton and comes back byte for byte); what is lost is
   that an agent extracting a deck's images gets everything except the one image an OLE object is
   *seen* as. ([[80-phase-16-opaque]])
8. **DocMark carries nothing for a slide transition or a timeline, so the write-side warning cannot
   fire on that path.** Reading `raw-preserved.pptx` and serialising warns
   `raw-block-dropped {id: raw-0003, format: pml}` twice, honestly; parsing that DocMark back leaves
   `Slide::raw` empty, so writing the parsed model emits **0 warnings** where the direct model emits
   two. This is the sidecar gap [[65-phase-15-raw]] decision 4 names, and it is what 16-D's own fix
   (a generated deck now *reports* a dropped `p:transition`) cannot reach.
   ([[80-phase-16-opaque]])
9. **`corpus/schema_gate.py:63` selects `mc:Fallback` before validating, so an `mc:Choice` branch is
   never checked against the XSD.** That is the correct behaviour for a Transitional-only schema set —
   it is exactly the compatibility processing a consumer does — but it means `ole-embedded.pptx`'s
   slide 2, the branch that holds the actual `p:oleObj`, has never been validated. Slide 1's plain
   `p:graphicFrame` form is, and validates, which is the only evidence this repository has that the
   fixture's OLE markup is schema-valid. ([[80-phase-16-opaque]])
10. **No corpus deck hides a slide, so 16-G's page-count-mismatch guard is reasoned rather than
    exercised.** Checked here: no `ppt/slides/slide*.xml` in any of the 21 decks carries `show="0"`.
    The guard drops the node ids for every slide and says why in the response, because an id on the
    wrong picture is a *false* claim; the fixture that would exercise it belongs to whoever adds a
    hidden-slide deck. ([[81-phase-16-thumbnails]])
11. **`***bold italic***` under `{.underline}` does not survive a docx round trip.** A `w:r` whose
    `w:rPr` is `<w:b/><w:i/><w:u w:val="single"/>` reads as `[***text***]{.underline}`, and the next
    pass produces `[**\*text**]{.underline}[\*]{.underline}` — the emphasis markers escaped and the
    span split. Unrelated to comments; found while building `comments-review.docx`, which uses
    `<w:b/><w:u/>` instead so the defect is recorded rather than carried into a gate that would fail
    for an unrelated reason. Open and belonging to nobody. ([[83-phase-16-comment-write]])
12. **PowerPoint's modern comments are not read.** `ppt/comments/modernComment_*.xml`, content type
    `application/vnd.ms-powerpoint.comments+xml` (`p188:cmLst`), a Microsoft extension ECMA-376 does
    not define, carrying replies and text anchors `p:cmLst` cannot. The part is **preserved
    unchanged** and the refusal is a specific `UnsupportedElement` naming the content type and what
    it carries — reproduced here on `comments-slide.pptx`. Reading it through `p:cmLst` would flatten
    both. ([[82-phase-16-comments]])
13. **The `.ole` stub does not carry `progId`.** An agent reading `::: {.ole}` cannot tell an
    embedded worksheet from an embedded equation, and `progId` is precisely the string that would
    tell it. Carrying it means a new attribute on a stub container, which is normative DocMark §11.2
    rule 8 — a specification change made to improve a reading, argued in its own increment or not at
    all. Refused in 16-D, recorded as a gap. ([[80-phase-16-opaque]])
14. **A preserved chart's *shape* cannot be edited through DocMark — only its numbers.** A series
    added or removed, the chart's kind, its title, and an emptied category axis are each refused with
    a `Warning::Degraded` naming the chart's own part, and the part is left as it was. Each refusal
    is argued: an invented `c:ser` is exactly what PowerPoint offers to repair, and writing the
    model's plain text over a `c:rich` title would silently restyle the chart.
    ([[78-phase-16-chart-write]])
15. **A generated chart has no design and no embedded workbook, and a generated combo chart is one
    plot.** Warned unconditionally per generated chart, because `ChartRef::kind` is the *first*
    plot's name and the series carry no plot of their own, so a writer looking only at the model
    cannot know a combo chart was one. `scatterChart`, `radarChart` and the 3-D plots fall back to
    `barChart` with a warning naming the kind. ([[78-phase-16-chart-write]])
16. **A multi-level category axis is not read, and the `c:f` formulas are not in DocMark.** The first
    is warned on read (`c:multiLvlStrRef` is a tree and `categories` is a list) and is the read-side
    face of item 14's per-series refusal. The second is deliberate: 16-C splices inside the preserved
    `chart1.xml` where the formula still is, and a copy in DocMark could contradict it.
    ([[76-phase-16-chart-data]], [[77-phase-16-chart-docmark]])
17. **The cache charges an entry its *input* length plus its media bytes, not the size of the tree.**
    The IR has no size hook and adding one is another increment's work. The consequence is bounded
    and one-directional — the cache can hold more than 64 MiB of *tree*, never more than 64 MiB of
    *input plus media* — and is worst for a small package that expands into a large tree.
    ([[79-phase-16-cache]])
18. **Nothing caches a render.** A second `render_slides` on the same deck spawns LibreOffice again.
    16-H's cache covers parsed documents; a render cache needs a key over the renderer and its fonts
    as well as the bytes. ([[81-phase-16-thumbnails]])
19. **A classic pptx comment carries no thread and no resolved flag**, because `p:cmLst` is flat and
    `CT_Comment` states no parent and no done flag. A deck therefore always reads with `replies: []`
    and `resolved: false`, and on the way out a reply, a `resolved=true`, a non-paragraph block, a
    `date=` that is not an `xsd:dateTime`, a missing position and a missing author are each refused
    by name. This is the format, not a defect — recorded so nobody reads the asymmetry with docx as
    a reader bug. ([[82-phase-16-comments]], [[83-phase-16-comment-write]])
20. **There is no `comments` counter in `ConversionStats` and no comments column in `inspect`'s slide
    inventory.** Refused twice, most recently because it would move every `.expected.inspect.json`
    in the corpus for a number no gate reads. ([[82-phase-16-comments]], [[83-phase-16-comment-write]])

**Dropped from the list, with the reason.** [[78-phase-16-chart-write]] and
[[83-phase-16-comment-write]] both record that *«`docsai convert` writes its sidecar assets beside
the **input**, not the output»*, the repro being a by-hand byte-identity run that littered
`corpus/pptx/assets/_skeleton/` with 22 files. **It does not reproduce on this tree.** Measured three
ways — `-o` into another directory, `--out-dir`, and `roundtrip` — the CLI puts the skeleton beside
the *output* every time and `corpus/pptx/` stays clean (`git status` on `corpus/` is empty after all
three). What remains is a latent asymmetry rather than the defect as stated:
`crates/docsai-convert/src/service.rs:504`, `default_assets_dir_for`, still defaults `assets/` next
to the **source** for the `SourceInput::Path` case, and that function serves the MCP
`convert_to_markdown` path rather than the CLI's `pipeline::convert_file`. No caller reached in this
increment writes files through it, so it is recorded here as a shape to watch and not as a
reproducible defect.

### Inherited from `kb/72` and still open

21. **The human P2 gate itself.** `docs/spikes/P2-powerpoint-gate.md`, unrun. Owner action, not a
    code action. §«Risk P2» above.
22. **Gate 2's threshold of 2.5 has still never been scored against a real render.** Re-confirmed by
    this increment's own gate run: *21 round trips identical by bytes, **0 rendered**, 67 pages
    compared* — the round trips take the byte-identity short circuit, and every generated page
    reports `diff n/a (geometry)` because the embedded template's canvas is not the corpus deck's.
    What the run establishes is that LibreOffice opens and renders every generated package and that
    no page came out blank. The threshold may still have to move when the first genuinely scored
    pages appear.
23. **The LibreOffice loader path is *narrowed*, not closed.** 16-G derives the installation's own
    `program` directory from the resolved `soffice` and puts it on the child's `LD_LIBRARY_PATH`, so
    `--thumbnails`, `render_slides` and the legacy `.doc` fallback all work here with no environment
    variable — no machine-specific path is written down anywhere in the repository. But
    `corpus/render_gate.py` is a Python script and still needs the variable in the command (this
    increment set it), and the permanent fix — `/etc/ld.so.conf.d/libreoffice.conf` plus `ldconfig` —
    is root and an environment action still open for the owner.
24. **The inherited Phase 14 criterion is still open.** `--fidelity agent` was to cost ≤ 15 % of
    `full` and measures ~100 % over the deck corpus. Still in the suite as
    `#[ignore = "analysis §6.5 target not met: \`agent\` is ~100 % of \`full\`, not 15 %"]` rather
    than softened: closing it changes what a fidelity level *means*, which is a specification
    decision for the owner. It is the `1 ignored` in `776/0/1`.
25. **A preserved table whose grid the model moved is copied with a warning.** The `a:tbl` writer
    exists only for slides the writer *generates*; nothing owns writing a moved grid back into a
    preserved slide, and it needs row heights and `a:tcPr` in `docsai-model` first. The warning is
    still there (`crates/docsai-office/src/pptx/write.rs:2501`).
26. **The refused internal hyperlink in pptx, and its docx twin.** pptx: a link pointing at another
    slide is refused rather than written pointing at nothing, because the `action` attribute the
    relationship needs is not in the model
    (`a_link_with_no_relationship_to_make_is_still_refused`). The docx twin is **worse and
    reproduces**: `[the section](#_Toc12345)` is written as
    `<Relationship … Type="…/hyperlink" Target="#_Toc12345" TargetMode="External"/>` with
    `<w:hyperlink r:id="rId2">`, where WordprocessingML wants `<w:hyperlink w:anchor="_Toc12345">`
    and no relationship at all. Word follows an external URL of `#_Toc12345`, which resolves to
    nothing.
27. **Preserved parts are never schema-validated**, by construction: `corpus/schema_gate.py`
    validates only the parts whose bytes the writer changed or created, found by diffing the written
    package against the one it was written on. A real deck with an invalid layout is preserved
    faithfully and nothing says so — which is the correct behaviour and worth knowing.
28. **Both gates run only at `full`.** Neither `corpus/schema_gate.py` nor `corpus/render_gate.py`
    passes `--fidelity` anywhere, so whether an `agent` or `standard` rendering writes a valid
    package has never been asked.
29. **`align`'s content-based pairing can still guess.** When two or more slides are both edited
    *and* reordered there is nothing left to match on; the writer pairs them with the parts left
    over, in order, and reports it with `Warning::Degraded` naming each slide and the part it went
    into (`crates/docsai-office/src/pptx/write.rs:693`). A slide can still take over another's
    layout, relationships and notes page. The real fix is an identity for a slide, which nothing in
    this chain carries.
30. **No real-world decks in the corpus**, and `AGENTS.md` §6 rules them out. The `p:oleObj` half of
    `kb/72`'s item 13 is **closed** — 16-D added `corpus/pptx/ole-embedded.pptx`, with both
    spellings, and it is the fixture that finally executed `wrapped_kind`'s `p:oleObj` line. What
    remains is that every one of the 143 corpus files is generated by `corpus/generate.py`, so no
    deck any real producer wrote has ever been through this reader. Item 7 of the P2 form is still
    the first chance to get PowerPoint's own bytes back through it.

## What Phase 17 inherits

A converter whose presentation path is complete in both directions and whose content is now largely
*data*: a chart's numbers, a comment's thread, a slide's picture, all addressable by node id and all
reachable over MCP. Phase 17 is `apply_edits` — the patch operations that turn every one of those
into something an agent changes without rewriting the document around it.

Two things it should read before it starts. The **preamble question** of §«A warning for Phase 17»,
because Phase 17's own acceptance criterion cannot be scored until it is answered. And **risk P2**,
because `apply_edits` is where the number of ways bytes leave this project stops being countable by
hand.

## Next

Phase 17 (`docs/development-plan-v2.md:310`) — `apply_edits`, patch editing. **First, though**:
`docs/spikes/P2-powerpoint-gate.md`. It has now waited through a whole phase.
