---
tags:
    - phase-17
    - acceptance
    - wire-cost
    - token-budget
    - testing
---
# 92 — Phase 17 H: the loop, measured

Increment **17-H** of [[85-phase-17-plan]]. The four acceptance criteria of
[`docs/development-plan-v2.md:310`](../docs/development-plan-v2.md) answered with evidence, the
`deck-retitle` scenario of `crates/docsai-mcp/tests/wire_cost.rs` completed, and the questions the
earlier increments handed forward settled or carried with a reason.

Nothing was taken on trust. 17-C and 17-D had already built tests for criteria 1, 2 and 3; this
increment checked each of them against what the plan literally asks, found four real defects in
the process, and closed them. The phase close (17-I) is not here.

## The number, which is what the phase exists for

> A scripted realistic agent loop («retitle slide 4, add a bullet, bold it, verify») costs
> **≤ 3 tool calls and ≤ 2k tokens** on a 40-slide deck.

`crates/docsai-mcp/tests/goldens/wire-cost.md`, on `corpus/pptx/forty-slides.pptx`:

| scenario | request tokens | response tokens | total |
|---|---|---|---|
| **deck-retitle** | 242 | 562 | **804** |
| session-preamble | 93 | 2 388 | 2 481 |

**2 tool calls, 804 tokens, against 3 and 2 000.** The preamble is reported beside it at 2 481
and is not part of the budget, exactly as [[85-phase-17-plan]] fixed the reading before anyone was
losing: the 2 000 are the task's own calls — the scenario row — and the preamble is what every
task in a session pays once, tallied as its own row since the harness was written.

The loop is two calls because 17-D's report is the verification:

1. `read_selection` on `s4` — the node ids, the etags, and `next-id: 121`, which is how the
   request below knows `n121` is free to write on the bullet it creates (§5.5).
2. `apply_edits` with three operations: `replace_text` on the title, `insert_after` a bullet
   carrying that id, `set_props` bold on it. The response carries both changed nodes' new etags
   and the created id, so **there is no confirmation read** — plan task 5's claim, spent.

### The gate, and the hatch that was not needed

`wire_cost.rs` refuses a golden update that inflates the suite total by more than `MAX_INFLATION`
(5 %) unless `DOCSAI_ACCEPT_TOKEN_INFLATION=1` is set. The increment was authorised to open that
hatch, because completing a scenario checked in as a read half is new measurement rather than
drift. **It was not opened.** The suite total went **5 632 → 5 186 tokens, −7.9 %**: the old
`deck-retitle (read half)` cost 1 250 tokens over five calls (`estimate_tokens`,
`convert_to_markdown`, `outline_document`, `search_document`, `read_selection`) and the complete
loop costs 804 over two. Measuring the return leg made the row *cheaper*, because the return leg
is what let the read half stop converting the whole deck to a file first. `MAX_INFLATION` is
untouched and nothing about how the harness measures was changed to make a number smaller.

Two things were added to the harness, both of which can only make it stricter:

- `Tally::calls` — the `tools/call` frames counted in the bytes that crossed the transport, so
  the criterion's «tool calls» is measured where everything else in that file is measured and not
  kept on the side by the test.
- The budget asserted **before the golden is consulted**: `LOOP_MAX_CALLS`/`LOOP_MAX_TOKENS` fail
  the test regardless of `DOCSAI_UPDATE_GOLDENS`. A golden is a record of what something costs;
  this is a budget, and regenerating a golden must never be able to make the loop affordable.
  The golden now carries the criterion in prose beneath the table, preamble beside it, so the two
  columns cannot be swapped quietly.

## Criterion 1 — every operation, every class it applies to

§11 of [`docs/edit-operations.md`](../docs/edit-operations.md) is the definition of «all four
document classes it applies to», and it was a table nobody had walked. `edit_ops.rs` proved what
each operation *means* and covered most cells, but not all: `set_props` had no workbook refusal,
`insert_before` was never tried on a deck or a workbook, `duplicate_slide` and `set_notes` had no
workbook `—`, `set_cell` had no presentation `—`, and `replace_image` had no DocMark case at all.

`crates/docsai-convert/tests/edit_matrix.rs` is the answer: §11's matrix as a **constant**, walked
cell by cell, **72 cells** visited by construction — ten operations over four classes, §2's «both
packages» for text (`.docx`, `.odt`) and workbook (`.xlsx`, `.ods`), and one `.dmk.md` per IR root
an operation applies to. A `✔` must apply *and* survive a round trip through the document's own
DocMark; a `—` must be a `NotApplicableToClass` or a `WrongKind` *and* leave the file
byte-identical. A combination the file cannot exercise panics, so the table cannot be covered by
omission, and the cell count is asserted so a row quietly losing a package shows up as a number.

Three assertions in `edit_ops.rs` could also pass by doing nothing — `if let Some(chart)`,
`if let Some(covered)`, and an `if sheets.len() >= 2` on a corpus whose workbooks all have one
sheet, so the workbook `move` case had **never run**. All three are now unconditional.

### Defect 1 — `set_props` was a silent no-op on docx and pptx

Found by requiring the round trip the criterion asks for rather than a non-empty report.

§6.2 specified that a font property is written to `ParaFormat::run_direct` and cleared on
overriding descendants. `run_direct` is the *paragraph mark's* run properties: ODF writes it into
the paragraph's own text properties, where it does reach the text, but docx writes it to
`w:pPr/w:rPr` and pptx to `a:endParaRPr` — neither of which formats anything a reader sees. So
`set_props {"bold": true}` returned `changed` with a new etag and left the document reading
exactly as before, on two of the three families.

Repro, before the fix:

```console
$ docsai edit basic-text.docx --ops '[{"op":"set_props","target":"doc/2","props":{"bold":true}}]'
$ unzip -p basic-text.docx word/document.xml | grep -o '<w:pPr><w:rPr><w:b/>'
<w:pPr><w:rPr><w:b/>            # the paragraph mark, not the text
$ docsai convert basic-text.docx -o out.md && sed -n 44p out.md
Segundo parrafo con un salto    # no bold anywhere
```

Fixed in `crates/docsai-convert/src/edit/ops.rs` (`apply_font`): the property is set on every run
of the paragraph as well, wrapping a bare `Inline::Text` in an `Inline::Styled` that carries it,
and still on `run_direct` because that is what text typed at the end of the paragraph inherits.
This is the edit layer, not the writers: reading and writing a package is untouched, so no
conversion's bytes move. §6.2 was corrected to state all three halves.

### Defect 2 — `highlight` was typed as a colour and validated as anything

§6.2 typed `highlight` as `RRGGBB` by analogy with `color`, and `validate_font_value` accepted any
string at all, writing it verbatim into `w:highlight/@val` — which is the `ST_HighlightColor`
enumeration and not a colour. `highlight: "FFFF00"` produced a `w:highlight w:val="FFFF00"` Word
does not honour: a silent no-op reported as a change, which is precisely what §6.2's own paragraph
about a misspelled `bould` refuses. Now validated against the seventeen names, refused by
`UnknownField` listing them. §6.2's table corrected.

### Defect 3 — the ODS writer dropped a picture's name and description

Found by `replace_image`'s DocMark round trip on `.ods`, which is a cell §11 marks `✔` and nothing
had exercised. `draw::read_frame` takes a picture's description from `svg:desc` and falls back to
`draw:name`; the ODS writer emitted neither, so **every** write-back of a spreadsheet with a
picture lost both, silently, with no warning — a plain `.ods → DocMark → .ods` conversion did it
too, so this is a conversion defect that the edit merely surfaced:

```console
$ docsai convert corpus/ods/images-anchored.ods -o a.md && grep '!\[' a.md
![Pic1](assets/img-40e10599.png){#n2 … name=Pic1 …}
$ docsai convert a.md -o b.ods --to ods && docsai convert b.ods -o c.md && grep '!\[' c.md
![](assets/img-40e10599.png){#n2 …}            # the name and the alt are gone
```

Fixed in `crates/docsai-odf/src/ods/write.rs` (`write_image`), mirroring what the `.odt` writer has
always done: `draw:name`, then `svg:title`/`svg:desc` children. Verified by hand with LibreOffice
Calc, because there is no render gate for spreadsheets — see the carried-forward list.

## Criterion 2 — editing a node never perturbs a sibling's serialization

The existing test, `an_edit_touches_exactly_the_members_it_has_to`, checks that a deck's edit
changes exactly one package member. That is most of the claim for a deck, where a slide is a part
of its own, and almost none of it for a text document or a sheet, where every sibling lives in the
same XML as the target.

`an_edit_leaves_every_sibling_byte_identical_inside_the_part_it_changed` closes it for all five
families: edit the second element of a sequence, find the one member that changed, split it on the
family's own element boundary (`</w:p>`, `</text:p>`, `</a:p>`, `</row>`, `</table:table-row>`)
and assert that **exactly one** element differs.

### What «before» is, and the finding that forced the question

An empty batch is byte-identical only on pptx:

```console
$ for f in long-report.docx basic-text.odt values-types.xlsx values-types.ods slide-order.pptx; do
    docsai edit $f --ops '[]'; done       # hashes before → after
long-report.docx    149b6d8e28a4 → 5246820da5e8
basic-text.odt      77e600cc0a12 → 8e59abd728c1
values-types.xlsx   c7b348236c3f → e16462137746
values-types.ods    98ed3a8c192a → 1d085f040df7
slide-order.pptx    0d5506be766f → 0d5506be766f
```

The preserved skeleton is a **pptx** property (Phase 15). The docx, odt, xlsx and ods writers
build their parts from the model, so an edit's baseline for them cannot be the original package —
it is what the writer would have produced from the same document with **no operations at all**,
which the test takes by running an empty batch first. Against that baseline the criterion means
exactly what it says for all five families, and it is the strongest form these writers support.
The asymmetry is asserted rather than tolerated: the test requires the empty batch to be
byte-identical **if and only if** the family is pptx, so a writer that gains a preserved skeleton
turns this red and gets a stronger assertion.

## Criterion 3 — a failure injected mid-transaction

17-C's two injected-fault tests are sound and prove the mechanism in detail, but only on a deck.
`an_injected_failure_leaves_every_package_family_untouched` runs both fault points —
`AfterApply` and `BeforeRename` — on all five families, asserting for each that the fault is the
typed one requested, that `BeforeRename` leaves a complete temporary file *beside the
destination* while `AfterApply` has serialised nothing, that the original is byte-identical, and
that no `.docsai-edit-*` leftover survives.

## The questions handed to 17-H

- **17-B, `ETAGS_MOVE_ON_REPARSE`** (`crates/docsai-convert/tests/select.rs`). The worry was that
  the subtree etag scope «widens it to the row, the table and the section». **Measured, not
  described**: `the_merged_table_etag_defect_is_measured_and_not_merely_described` re-parses the
  pinned selection and names every id whose etag moved. It is **exactly one**, the table `n1` —
  the rows and the section of that document are not separately addressable, so no widening
  happened. The consequence is stated in the test: an `if_match` on `n1` of `odt/table-merged.odt`
  can be refused as `StaleEtag` against a document nobody changed. That is a **false refusal** and
  therefore a defect, but it fails in the safe direction — no overwrite — and the fix belongs
  where the ODF table is read, not in the edit engine. **Carried forward.**
- **17-C/D, ids renumber on the next read of a package.** Confirmed and settled as a **property**,
  stated where a caller will meet it, not a defect to fix. New §5.4.1 of `docs/edit-operations.md`,
  and `a_package_reassigns_ids_on_the_next_read_and_docmark_does_not` makes the statement true
  rather than hopeful: after `move n1 after n3` on `pptx/slide-order.pptx`, a re-read hands `n1`
  to the slide that used to be `n3`. No Office format stores a node id and this project
  deliberately refuses to annotate a package to make one stick — writing ids into a `.docx` as
  bookmarks was tried and removed (commit `286a51f`) — so a reader assigns ids in document order
  every time. A `.dmk.md` writes its ids down and does not have the property; the same test
  asserts that half too. The honest sentence is *an id is stable for the lifetime of a reading*,
  and the guard is the etag: an id that has come to mean a different node carries a different one.
- **17-C/D, `highlight` typed `RRGGBB`.** Defect 2 above. **Fixed.**
- **17-C/D, the ODF writer drops an all-zero `CropRect`.** **Settled as a normalisation, not a
  loss.** A crop of zero on every side is not a crop; the ODF writer emits no `fo:clip` for it and
  a re-read produces `None` instead of `Some(0,0,0,0)`. The two spellings mean the same placement,
  nothing a person chose is lost, and no warning is owed because there is no degradation to warn
  about. `edit_ops.rs::placement` normalises the two spellings for exactly this reason and says
  so.
- **17-A, selectors as targets.** §5.2's narrowing **stands** for version 1, and the argument is
  now measured: the loop plan task 2 exists to serve costs 2 calls and 804 tokens with ids alone,
  so a selector target saves a caller nothing it does not already hold. Adding a target form is a
  version 2 decision — that is what §1's version number is for — and not a patch. Recorded in §13.
- **17-E, `fullCalcOnLoad` verified against LibreOffice, not Excel.** **Carried forward
  unchanged.** No Excel is reachable from this environment, and asserting against a spreadsheet
  nobody ran would be a false green. The claim in §6.7 stays scoped to what was measured.
- **17-E, no render gate for spreadsheets.** **Carried forward**, and reinforced by defect 3:
  another `.ods` write-back defect went unseen for the same reason. `corpus/schema_gate.py` and
  `corpus/render_gate.py` are both pptx-only by construction. This increment verified the ODS fix
  by opening the edited package in LibreOffice Calc by hand; that is a one-off and not a gate.

## Carried forward

1. **The preserved skeleton is pptx-only.** docx, odt, xlsx and ods rewrite their parts on any
   write-back, so criterion 2 is met against the writer's own output rather than the original
   bytes for four of five families, and an empty edit changes a file's hash. Extending 15-B's
   construction to the other writers is a phase, not a patch.
2. **`odt/table-merged.odt`'s etag moves on re-parse** — the ODF table reader yields rows of three
   cells where the DocMark of that table has four columns. Blast radius measured at one node, the
   table. It costs a false `StaleEtag`, never an overwrite. The fix belongs in the ODF table
   reader.
3. **No schema or render gate for spreadsheets or text documents.** Both gates are pptx-only.
4. **`fullCalcOnLoad` is measured against LibreOffice Calc and not Excel.**
5. **Risk P2 is still unsigned** ([[84-phase-16-close]]).

## Next

17-I — the phase closes: the criteria answered one by one in `docs/`, `CHANGELOG.md`, `AGENTS.md`
§1 and `kb/README.md`, and this carried-forward list folded into the phase's own.
