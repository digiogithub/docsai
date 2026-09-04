---
tags:
    - phase-18
    - close
    - odp
    - odf
    - presentations
    - gates
    - risk-p2
---
# 104 — Phase 18 F: the phase closed, and the third ODF class

Increment **18-F** of [[94-phase-18-plan]], the sixth and last. It adds no feature. What it owes is
the discipline of [[93-phase-17-close]], [[84-phase-16-close]] and [[72-phase-15-close]]: the two
acceptance criteria answered with evidence **re-run here**, what the phase carries forward written
down rather than discovered later, the state of risk P2 said plainly, and every budget and golden
accounted for with what was paid and what was bought.

Six commits, in the order they landed: `125214c` the plan (shared with Phase 19), `4e1db8b` 18-A,
`bd91e63` 18-B, `20f66ca` 18-C, `958ac1e` 18-D, `359f513` 18-E.

## A note on the history, before anything else

**Phases 18 and 19 ran concurrently in one working copy, and their commits crossed twice.** The tree
was correct at every point and every gate is green today; only the attribution is imperfect, and a
reader running `git log` deserves to be told rather than left to infer.

- **`4e1db8b` (18-A) carries Phase 19-A's work on `corpus/generate.py` and `corpus/ppt/*.ppt`** —
  the thumbnail-stripping of a recorded `.ppt` (`_property_set_without_thumbnail`,
  `_cfb_stream`, the `[MS-OSHARED]` summary-information surgery) and the regenerated decks that
  travel with it. It is in an 18-A commit because `--check` regenerates the whole corpus from one
  script: leaving the generator half-changed would have made `python3 corpus/generate.py --check`
  red on a tree where nothing was wrong. The commit message says so in a closing note.
- **`958ac1e` (18-D) carries Phase 19-B's in-flight `Format::Ppt` wiring** in
  `crates/docsai-convert/src/{lib,pipeline}.rs` — the tenth `SUPPORT` entry and the read dispatch
  that reaches it. It was in the shared working copy when the `.odp` writer's own edits to those two
  files were committed. It left `HEAD` **red** for one commit: `every_format_appears_exactly_once`
  still asserted `total == 9` against ten entries (verifiable:
  `git show 958ac1e:crates/docsai-convert/src/lib.rs` line 242), and `3ffdf35` (19-B) completed the
  wiring and the assertion.

Neither is rewritten here. `jj squash`, `jj split` and `jj rebase` on landed history would trade a
recorded imperfection for an unrecorded one; the close records it instead.

## What the phase set out to do, and what it did

The objective as `docs/development-plan-v2.md:378` states it: **symmetry with `.odt`/`.ods`.** Before
this phase `docsai-odf` read and wrote text documents and spreadsheets and *refused* a presentation.
The third ODF class is now a first-class citizen of the same IR, the same DocMark-P profile and the
same skeleton mechanism the `.pptx` path uses — and it got there without a new crate, without a new
IR root, and without a normative change to DocMark.

- **The corpus came first** (18-A). Seventeen `.odp` fixtures, one trait per file, each named after
  the `corpus/pptx` deck it mirrors so the cross matrix pairs the two sides by name. Both gates
  learned ODF at the same time, deliberately: adding a second ungated format would have made
  [[93-phase-17-close]]'s carried item 3 worse instead of smaller.
- **The reader settled the two mappings** (18-B) — in writing, against files, and one of them by
  correcting the plan's own framing. §«The two mapping decisions» below.
- **DocMark needed no branch on the package** (18-C), and that is a test rather than an intention:
  `the_deck_body_does_not_depend_on_the_package_it_came_from` serialises every `.odp` twice, once
  claiming each format, and asserts the bodies are identical.
- **The writer splices into the preserved archive** (18-D), so an untouched `.odp` comes back byte
  for byte — `mimetype` first and stored, ODF's one rule OPC has no counterpart for, bought for zero
  lines by not rebuilding.
- **The crossing is a matrix the test walks** (18-E), twenty-two cells and thirty-eight crossings,
  with «drops are warned, never silent» asserted in **both** directions: a drop with no warning
  fails, and a warning for a loss that did not happen fails as hard.

## The two acceptance criteria, each with evidence re-run in this increment

Nothing below is quoted from an increment's report. Every number was produced by a command run here,
today, on this tree.

### 1 — «odp round-trip with DocMark identity on the second pass over the odp corpus» ✔

`cargo test -p docsai-convert --test odp_write` — **8 passed, 0 failed**, including
`every_odp_round_trips_with_docmark_identity_on_the_second_pass`, which asserts its own corpus size
(`assert_eq!(decks.len(), 17)`) so the criterion cannot be met by a corpus that shrank.

Beside the test, by hand and outside the suite: every fixture through `docsai convert` three times —
`.odp` → DocMark → `.odp` → DocMark — with both DocMark passes and both packages compared with
`cmp`:

```
basic-slides          docmark=IDENT bytes=IDENT
bullets-levels        docmark=IDENT bytes=IDENT
charts-embedded       docmark=IDENT bytes=IDENT
charts-multi-series   docmark=IDENT bytes=IDENT
comments-slide        docmark=IDENT bytes=IDENT
forty-slides          docmark=IDENT bytes=IDENT
images-anchored       docmark=IDENT bytes=IDENT
notes-speaker         docmark=IDENT bytes=IDENT
ole-embedded          docmark=IDENT bytes=IDENT
placeholders-cascade  docmark=IDENT bytes=IDENT
placeholders-empty    docmark=IDENT bytes=IDENT
presentation-classes  docmark=IDENT bytes=IDENT
raw-preserved         docmark=IDENT bytes=IDENT
reading-order         docmark=IDENT bytes=IDENT
shapes-geometry       docmark=IDENT bytes=IDENT
slide-order           docmark=IDENT bytes=IDENT
tables-simple         docmark=IDENT bytes=IDENT
```

**17 of 17 on both columns.** The criterion asks only for the left one; the right one is stronger
than it and is what makes the left one uninteresting to game — a writer that regenerated the package
could still produce identical DocMark, and this one does not regenerate anything.

`corpus/render_gate.py` measures the same property from the other side, on the pptx arm's own
comparator, and reports the seventeen under «identical by bytes».

### 2 — «pptx→DocMark→odp and back work for common traits; drops are warned, never silent» ✔

`cargo test -p docsai-convert --test cross_matrix` — **2 passed, 0 failed**:
`the_matrix_covers_both_corpora_and_says_how_big_it_is` (the walk asserts `22` cells and that every
fixture of both corpora appears in exactly one of them, so a fixture added tomorrow fails the build
rather than quietly not being measured) and `every_trait_crosses_the_way_the_matrix_says_it_does`.

The property asserted per cell is neither byte identity nor a render diff — both would be measuring
something else — but three things: the source deck's visible text survives **as a multiset**, except
the lines the cell declares lost and those declared **verbatim**; the warnings cover the losses
**exactly, both ways round**; and the written package is real, being readable back by this project's
own reader.

By hand, outside the suite, in both directions:

```console
$ docsai convert corpus/pptx/bullets-levels.pptx -o a.dmk.md
$ docsai convert a.dmk.md -o a.odp --verbose
docsai: 4 warning(s), 0 raw-block(s)
  [Minor] degraded the deck's design: the preserved package is an OOXML package (the `ooxml`
     dialect, which is what a .pptx is), which this writer cannot splice an ODF page into, so the
     deck was written on the embedded default ODF template…
  [Minor] degraded the index of a `body` placeholder on page `page1`: PresentationML numbers a
     placeholder with `p:ph@idx` and ODF distinguishes frames of one presentation:class by document
     order alone, so index 1 was not written
  [Minor] degraded the placement of a shape on page `page1`: it reaches 23.071667cm × 17.222222cm
     on a page of 28cm × 15.75cm, so part of it falls outside the page and a reader will not see it…
  [Minor] degraded the index of a `body` placeholder on page `page1`: … index 2 was not written
```

```console
$ docsai convert corpus/odp/bullets-levels.odp -o b.dmk.md
$ docsai convert b.dmk.md -o b.pptx --verbose
docsai: 3 warning(s), 0 raw-block(s)
  [Minor] degraded the deck's design: the preserved package is an OpenDocument package (the `odf`
     dialect, which is what a .odp is), which this writer cannot splice a PresentationML slide into…
  [Minor] degraded slide 1 ("Niveles de viñeta"): it holds a title and body text and one other
     shape, which matches neither of the rules that choose a layout from content…
  [Minor] degraded a `text` placeholder on slide 1 ("Niveles de viñeta"): `text` is a role the `odf`
     dialect names and PresentationML's closed p:ph@type vocabulary does not, so the frame was
     written as `body`, which is the nearest role this dialect names
```

Both produce a real package (4 025 and 10 157 bytes) and **every degradation is named, in
centimetres where it is a placement and by dialect where it is a vocabulary**. The second run is the
whole criterion in one screen: the text crosses, and the three things that did not survive unchanged
say so before anybody opens the file.

## The gate run, in full

Every command of this increment's definition of done, run here:

| what | command | result |
|---|---|---|
| the suite | `cargo test --workspace` | **60 test suites**, **936 passed, 0 failed, 1 ignored**, exit 0 |
| lints | `cargo clippy --workspace --all-targets -- -D warnings` | clean, exit 0 |
| format | `cargo fmt --all -- --check` | clean, exit 0 |
| the corpus is what its generator says | `python3 corpus/generate.py --check` | **201 generated files**, exit 0 |
| CI gate 1 | `python3 corpus/schema_gate.py` | **21 decks, 42 packages, 691 parts validated (106 of them `.odp`), 0 invalid, 0 not measured**, exit 0 |
| CI gate 2 | `LD_LIBRARY_PATH=… python3 corpus/render_gate.py --require-renderer` | **21 decks + 17 `.odp`: 38 round trips identical by bytes, 55 rendered, 254 pages compared, 0 failures, 0 not measured**, exit 0 |

`890/0/1` at Phase 17's close is **`936/0/1`** here: **46 tests added** across the six commits,
nothing red, and the one ignored test is still the inherited Phase 14 criterion (carried item 33).

**A test must not write into the corpus, and it does not.** `cargo test --workspace` was run twice,
back to back, and `git status --short corpus/` after the second run is **empty**. This was worth
re-running rather than assuming: [[101-phase-18-odp-writer]] recorded a defect where
`crates/docsai-convert/tests/ppt_phase19.rs` left `corpus/ppt/assets/` and
`corpus/pptx/assets/_skeleton/` behind and a second run then failed. That was Phase 19's file, was
uncommitted at the time, and no longer reproduces.

## The two mapping decisions, stated where they will be looked for

The plan named two things that «do not line up» and refused to settle either in advance. 18-B
settled both. They are restated here because an increment entry is not where somebody six months
from now will look for them.

### `presentation:class` versus `p:ph`: the ODF side bends, three times

ODF names a frame's **role** out of a closed vocabulary; PresentationML names a **type and an
index**. The mapping is not a bijection, and the bend is on the ODF side:

1. **Eleven classes map straight across** — `title`, `subtitle`, `outline`, `object`, `chart`,
   `table`, `graphic`, `orgchart`, `header`, `footer`, `date-time`, `page-number`. **Two keep their
   own word** as `PhType::Other`: `text`, because PresentationML has one body type where ODF has
   `outline` and `text` (folding it into `Body` would lose the distinction on every read *and*
   manufacture an index collision on every page carrying both), and `page`, the notes-page slide
   thumbnail — re-read in 18-C as `Other("sldImg")`, which is the PresentationML keyword for the
   same construct, because DocMark-P writes the type into one field and one construct must not reach
   it spelled two ways. A `PhType::Other` placeholder is neither `is_title()` nor `is_body()`, so it
   always writes its own `::: {.ph type=…}` container and a round trip cannot quietly turn it into a
   body.
2. **Two frames of one class on one page get an ordinal, and a warning.** The first frame of a class
   carries no index — which is what the file says. Every later one carries 1, 2, 3 … from document
   order, and is warned once per class per page, because the ordinal is *this reader's* and no
   layout declares it, so it does not resolve against the cascade. Giving both `idx: None` would
   have been the silent collision the plan forbade.
3. **Reading order comes from the canvas, not from the ordinal.** Ordering by an index the reader
   invented would be circular, so within a rank the `.odp` reader orders by row band (the same
   eighth-of-an-inch constant `pptx/order.rs` uses) then left to right — the policy the pptx reader
   already applies to everything that is not a placeholder, applied one rank wider.
   `reading-order.odp` is the measurement, and `Shape::z_index` keeps document order so the page can
   be put back exactly as written.

### Master pages versus layout + master: the plan's framing was wrong

The plan said «OOXML has two levels above a slide; ODF has one». **ODF has two references above a
page; they are simply not nested**, and correcting that is what made 18-B cheap:

| ODF | what it carries | IR |
|---|---|---|
| `draw:master-page-name` → `style:master-page` | background objects, the placeholder **styles** a page inherits, and a `style:page-layout` for the page size | `Master` |
| `presentation:presentation-page-layout-name` → `style:presentation-page-layout` | a naked list of `presentation:placeholder` **rectangles**, with no style of its own | `Layout` |

That is not an approximation: `Layout`'s doc comment already read «a named set of placeholder
positions inherited from a master», written before anybody looked at one. So
`LayoutPlaceholder::props` is empty for every `.odp` layout on purpose (a layout that stated a font
would be stating one the file does not) and `Master::theme` is `None` on purpose (ODF has no theme
part).

What ODF has that the IR does not is that **the pair is chosen per page**: one page layout under two
master pages is two resolved sets in ODF and one `Layout` in the IR, because `Layout::master` is a
single field. The reader catalogues the first pairing and warns by name on a second. 18-D closed the
question by **keeping the layout id as the page-layout name alone**, and the argument is the
writer's own round trip: this writer never rewrites `draw:master-page-name` or
`presentation:presentation-page-layout-name`, so such a deck comes back **byte for byte with both
pairings intact** whatever the catalogue collapsed. The loss is in what an agent reading DocMark is
told, and a compound `LayoutId` would change the `layouts:` catalogue's keys — a normative DocMark-P
change, argued in a specification increment and not smuggled in by a writer. Carried item 5.

## Byte identity for ODF: what it cost, and the four things restored by hand

The plan asked by name whether byte identity was attainable for a format whose first member must be
`mimetype`, stored uncompressed (ODF 1.3 §3.3). **It is, for all seventeen fixtures, and the
`mimetype` rule costs nothing at all.**

`Package::write_to` does honour the rule — and honours it by *rebuilding*, which alphabetises members
through a `BTreeMap`, re-deflates every one and stamps a fixed timestamp. So the writer does what the
pptx one does and works on the ZIP itself: `reemit` copies each entry **raw**, in the original order,
with its compression method, CRC, timestamp and extra fields, substituting bytes only for the parts
that changed. `mimetype` stays first and stays stored **because it was**. There is no rule about it
anywhere in `crates/docsai-odf/src/odp/write.rs`, which is the whole cost: zero lines, bought by not
rebuilding.

Three ZIP fields had to be put back by hand, each a measurement rather than a guess, because the
fixtures are written by Python's `zipfile` and the `zip` crate states them differently:

| field | where | what the `zip` writer does |
|---|---|---|
| external file attributes | central directory | rebuilds from the Unix mode with `S_IFREG` ORed in (`0o600` → `0o100600`) |
| version made by | central directory | derives it from the compression method |
| version needed to extract | central directory **and** local header | 1.0 for a stored entry, 2.0 for a deflated one |

The pptx writer already restored the first. The other two are **ODF's own bill**: `mimetype` is the
one *stored* member in the package, so it is the one entry whose «version needed» a producer writing
2.0 across the board disagrees with — the field moves on exactly the member ODF cares most about.
The repro before the fix was `cmp` reporting *«byte 5»*, which is that local header's version field.

And a fourth difference, which is **not a field but a member**: `corpus/odp/charts-embedded.odp`
carries an `Object 1/` **directory entry** for its chart sub-document. The pptx re-emission drops
directory members on the argument that they carry no content and no relationship — true, and still
wrong here, because an entry the reader ignores is an entry the file has, and dropping it makes an
untouched deck come back one member shorter. They are copied.

`rebuilt_parts` for ODF is **one name, `content.xml`**, where a pptx skeleton names one part per
slide plus one per notes page. `styles.xml`, `meta.xml`, `settings.xml`, `META-INF/manifest.xml`,
`Pictures/*`, the chart sub-documents and any `Basic/` project are copied. 18-B predicted pressure to
widen the list; it never arrived, because the writer splices — and `META-INF/manifest.xml` is
deliberately **not** on the list and is still edited when a rewritten body brings a picture the
package did not carry, which is an edit *inside* a part and not a regeneration of one.

## What the gates now cover, and what they still do not

This is the phase's second deliverable and it closed a real gap. The numbers below are this
increment's own run.

**`schema_gate.py` learned ODF.** OpenDocument 1.3 is a RELAX NG grammar, a different technology
from the ECMA-376 XSDs, and `xmllint` validates it through the same stderr parser the XSD arm uses.
The grammar is **vendored** in `schemas/odf-1.3/` for the reason `schemas/README.md` already argues:
a gate whose oracle can change under it without a commit is not a gate. Two files, because
`META-INF/manifest.xml` is not written in the document grammar. `strip_foreign()` applies ODF's own
extended-document rule before validating — the counterpart of `markup_compatibility()`'s ECMA-376
Part 3 fallback selection — with the allowed namespace set **derived from the vendored grammar's own
declarations** so a schema refresh cannot leave a stale list behind.

**`render_gate.py` gained the `.odp` round trip**, on the pptx arm's comparator and threshold.

Measured here:

| | Phase 17's close | today |
|---|---|---|
| `schema_gate.py` | 363 parts, 42 packages, 0 invalid | **691 parts, 42 packages, 0 invalid — 106 of them `.odp`** |
| `render_gate.py` | 21 round trips by bytes, 67 pages | **38 round trips by bytes (21 pptx + 17 odp), 55 rendered, 254 pages, 0 failures** |

**What they still do not cover.** The honest table is in `corpus/README.md` with the blanks visible,
and it is short:

- **Spreadsheets have no gate at all** — neither `xlsx/` nor `ods/`, neither schema nor render.
  This is the item [[93-phase-17-close]] called «the strongest carried item in this list, because it
  is the one that hides the others», and it is **still open**. Phase 17 found two silent `.ods`
  write-back defects (`Err:510` on every formula this project ever wrote; a picture's name and
  description dropped on every write) that had been in the tree for months for exactly this reason.
  What it would take is not research: `schema_gate.py`'s ODF arm is **one glob away** from covering
  `ods/`, and the same construction the OOXML arm uses — convert, edit, write back, diff the members,
  validate what changed — transfers unchanged. For `xlsx/` the ECMA-376 XSDs are already vendored.
  The blocker for the ODF half is carried item 2 below (`draw:rotation-angle`), one real defect that
  the switch-on would report immediately; the blocker for the OOXML half is nobody's time.
- **`docx/` and `odt/` have neither gate either.** Same construction, same absence.
- **Preserved parts are never schema-validated** for `.pptx`, by construction — the gate validates
  only parts the writer changed or created. For `.odp` the fixtures themselves *are* validated, which
  is a narrower version of the same hole rather than a closing of it.
- **Both gates run only at `full`.** Neither script passes `--fidelity` anywhere, so whether an
  `agent` or `standard` rendering writes a valid package has never been asked.
- **Gate 2's threshold of 2.5 has still never been scored against a real render.** Re-confirmed by
  this increment's own run: every one of the 67 comparable pages reports `diff n/a (geometry)`,
  because every round trip is byte-identical and there is nothing to diff.

## Risk P2: still OPEN, and now on a fourth package family

`docs/spikes/P2-powerpoint-gate.md` **is still unsigned.** It needs PowerPoint and a person, and
this machine has neither. Phase 16 was started on the owner's direction with the form open; kb/84
wrote that Phase 17 «inherits the debt with interest»; kb/93 wrote that Phase 17 had made it
«countable» and that **the form should be signed before Phase 18 extended the writer surface, not
after**. Phase 18 was started the same way, and it did extend it.

What is proven at this close is more than at the last one, and it is proven by the same three
witnesses: a grammar (691 parts, two grammars now, 0 invalid), a renderer (LibreOffice, 254 pages,
0 failures, 0 blank), and this project's own reader. What is **not** proven is unchanged: **no
application but LibreOffice has opened any of it**, and PowerPoint is the loader that decides for
`.pptx`.

Phase 18 changes the shape of the debt in two specific ways.

1. **A fourth package family now leaves this project as written bytes** — `.docx`, `.xlsx`/`.ods`
   and `.odt` from the model, `.pptx` and now `.odp` as splices into a preserved archive. The `.odp`
   half has the better evidence of the two presentation formats, because ODF's RELAX NG grammar
   validates the writer's output and LibreOffice *is* the reference consumer for ODF rather than a
   stand-in for one. The `.pptx` half did not improve.
2. **The cross conversion is a new way to produce a `.pptx` nobody has ever opened in PowerPoint.**
   Every `.odp` in the corpus now generates a full PresentationML package from an embedded template —
   twenty-one more packages through `schema_gate.py`, and it immediately found `p:ph type="text"`,
   markup outside ECMA-376's closed enumeration that a schema refuses and a reader might not. That is
   the class of defect the P2 form exists to catch, found here by a grammar and not by an
   application.

**For Phase 19 and Phase 20 this means one thing.** Phase 19 is legacy `.ppt`, read-only, and adds
no writer — so it does not make the debt larger, and it is the last phase of which that is true.
Phase 20 is hardening: fuzzing, an adversarial suite, benchmarks, an audit. A hardening phase that
runs before a real consumer has ever opened a written deck is hardening against the properties this
repository already knows how to measure, which is precisely the set that has never included
PowerPoint's loader. **The form should be signed before Phase 20 starts.** It has now waited through
three phases.

## Where every budget and golden stands

Named individually, because a moved number that is not argued is a loosened gate. Each was checked
here rather than repeated from an increment's report.

### 1 — The residue budget: untouched, and unchanged

`crates/docsai-convert/tests/plain_residue.rs` measures what a `standard` rendering leaks that a
plain Markdown viewer would have to hide — risk **P4**'s budget. Measured here, by temporarily
lowering the two assertions so the test prints its own numbers, and reverting:

| | measured today | budget |
|---|---|---|
| corpus-wide | **15.1 %** | 18.0 % |
| the decks that are documents | **12.2 %** | 15.0 % |

**Identical to Phase 17's close and to Phase 16's**, and the constants were never touched (`git show
20f66ca -- …/plain_residue.rs` contains no change to `18.0` or `15.0`).

**The seventeen `.odp` decks never entered it, and that is deliberate.**
`plain_residue.rs::decks()` reads `corpus/pptx` only — verified here, it globs `corpus_pptx()` for
`pptx`/`pptm` — so nothing about `.odp` reaches the 18.0 %/15.0 % measurement. 18-C measured the
`.odp` corpus anyway, at **29.2 %** at `standard`, and argued that folding it in would measure the
wrong quantity. The argument is one table:

| | visible | residue | ratio |
|---|---|---|---|
| `pptx/forty-slides` | 2 404 | 320 | 13.3 % |
| `odp/forty-slides` | 1 493 | **320** | 21.4 % |

**The syntax cost is identical — 320 characters, forty `{.slide}` markers — and only the prose
differs.** 18-A's fixtures are minimal one-trait files; 14-G's budget was calibrated on decks
carrying document-length bullets. A ratio over the `.odp` corpus measures how much text 18-A wrote,
not what DocMark-P costs, and a gate on it would be a gate on the wrong quantity. What *is*
comparable — the **kind** of syntax that leaks — is gated instead:
`an_odp_deck_leaks_what_a_pptx_deck_leaks_and_nothing_else` asserts zero residue at `plain` and, at
`standard`, nothing but rule 1's slide marker and rules 4 and 8's containers. It is what found two of
18-C's three defects.

### 2 — `corpus/token-budget.md`: it moved, downward, in 18-C, and was not regenerated after

This is the one budget Phase 18 touched, and it fell:

| column | before `20f66ca` | after | Δ |
|---|---|---|---|
| full | 26 624 | 26 624 | — |
| agent | 16 043 | 16 043 | — |
| **standard** | 17 433 | **17 402** | **−31** |
| **plain** | 10 722 | **10 730** | **+8** |

Both moves are defect fixes, not drift, and neither is a `.odp` measurement — the budget covers
`docx/`, `odt/`, `xlsx/` and `ods/` and no deck is in it at all. The −31 is `list=` no longer leaking
into `standard`, where a numbering-definition name says nothing a reader can use. The +8 is the price
of **no longer losing** an image's title at `plain`: it used to be dropped entirely, silently, and is
now written in CommonMark's own title slot. `cargo test -p docsai-convert --test token_budget` is
green here, and the 5 % inflation refusal was never approached in either direction.

### 3 — `LOOP_MAX_CALLS` / `LOOP_MAX_TOKENS` and the wire-cost suite: not touched at all

`git log 125214c~1..HEAD -- crates/docsai-mcp/tests/wire_cost.rs
crates/docsai-mcp/tests/goldens/wire-cost.md` is **empty**: no commit of this phase touched either
file. The constants are still `LOOP_MAX_CALLS = 3`, `LOOP_MAX_TOKENS = 2_000` and
`MAX_INFLATION = 5.0`, still asserted before the golden is consulted; the `deck-retitle` row is still
**2 calls and 804 tokens**; the session preamble is still **2 481**; and the suite total, summed from
the golden's own table, is still **5 186**.

That is the right outcome and not an accident. Phase 18 added no MCP tool and no resource, so the
preamble — which every session pays whether it wants the feature or not — did not grow by a single
token for a phase that added a whole format. `.odp` reaches an agent through the tools that already
exist.

### 4 — The goldens

The `.odp` corpus is pinned twice, by the same mechanisms the other five corpora use:
`<name>.expected.inspect.json` (18-B, with the **warning list part of the golden**, which is the only
mechanical way to hold «never degrade silently, never warn falsely» to account across an increment
that adds a format) and `<name>.expected.dmk.md` (18-C, satisfying `serialize(parse(md)) == md` byte
for byte). `python3 corpus/generate.py --check` counts **201** generated files, up from 143 at Phase
17's close: the seventeen fixtures, their goldens, the regenerated `.odt` set, `corpus/ppt`'s decks
(Phase 19-A's, riding in 18-A) and `crates/docsai-odf/assets/default-template.odp`, which is
generated from source for the reason the pptx template is — every byte reviewable, and `--check`
fails if the checked-in file stops being what the generator produces.

**No golden was regenerated to make anything pass.** Two `.odp` DocMark goldens moved in 18-E, and
**no `.pptx` golden moved with them**, which is what says the two defects behind the move were
ODF-visible rather than ODF-caused.

## What Phase 18 carries forward

Collected here so Phase 19 and Phase 20 find them in one place instead of in five increments' prose.
**Every item below was reproduced in this increment** before it was written down; the ones that no
longer reproduce are in §«Dropped» with the reason.

### Found or reinforced during Phase 18

1. **`docsai edit` and `apply_edits` cannot reach a `.odp`, and the refusal names the wrong thing.**
   `crates/docsai-convert/src/edit/mod.rs:322` routes `Format::Odt | Format::Ods` to
   `docsai_odf::write` and everything else to `docsai_office::write`, so a `.odp` falls through to a
   writer that does not know the format. Reproduced here on `corpus/odp/basic-slides.odp`: both an
   **empty** batch and a `replace_text` exit 2 with `docsai: unsupported write format: odp`, and the
   file's hash is unchanged in both cases. It fails safe and it fails confusingly: `docsai formats`
   says `odp: write yes`, the convert pipeline writes it, and `docs/edit-operations.md` §2 still says
   the presentation class is `.pptx` — three statements a caller cannot reconcile from the error.
   `edit_matrix.rs`'s `Family` has five variants and no `Odp`, so §11's matrix does not measure it
   either. This is exactly what [[93-phase-17-close]] predicted («`apply_edits` will reach it the
   moment the IR does»); what actually happened is that the IR reached `.odp` and the edit layer did
   not. Fixing it is one match arm plus the matrix cells that would then be owed.
2. **`draw:rotation-angle` is not an ODF attribute, and this project reads *and* writes it.**
   ODF 1.3 has no such attribute on `style:graphic-properties`: a shape is rotated by
   `draw:transform`, and `style:rotation-angle` exists only on chart and table-cell properties. Still
   present in all three places: `crates/docsai-odf/src/styles.rs:260` reads it,
   `crates/docsai-odf/src/odt/write.rs:837` writes it, `corpus/generate.py:2100` generates it. So
   **`crates/docsai-odf` writes invalid ODF**, and it is why `odt/` and `ods/` are excluded from the
   ODF schema arm — switching the glob on today reports exactly this one failure. The repair is
   reader + writer + fixture + golden in one increment, and then a one-line change in
   `schema_gate.py`. Carried unchanged through 18-A, 18-B, 18-C, 18-D and 18-E; recorded a fifth
   time. ([[96-phase-18-corpus]])
3. **A slide or a shape the model added or removed is not written into a preserved `.odp`**, and
   neither is a table's cells, a picture, a chart's data, an ODF annotation or a preserved object's
   payload. Each is a typed `Degraded` warning naming the construct; the writer removes nothing and
   invents nothing. These are 15-E and 15-G's ODF counterparts, unwritten rather than half-written.
   ([[101-phase-18-odp-writer]])
4. **A chart, an ODF annotation and an OLE payload cannot cross into `.odp`.** Three writers this
   project does not have: an ODF chart is a whole sub-document with its own `local-table`, an ODF
   annotation is markup no writer here writes, and an OLE payload is a stream it has never produced.
   Each is a `Degraded` warning naming the construct **and** the dialect, which is what the criterion
   asks for. ([[103-phase-18-cross]])
5. **A page layout used under two master pages collapses to one `Layout` and warns by name.**
   `Layout::master` is a single field. The cost to the **package** is now measured and is **zero** —
   the writer never rewrites either reference, so such a deck comes back byte for byte with both
   pairings intact — and the cost is entirely in what an agent reading DocMark is told. No corpus
   fixture triggers it, so the warning is real and, today, unfired. The fix, if wanted, is a compound
   `LayoutId`, which changes the `layouts:` catalogue's keys and is therefore a normative DocMark-P
   decision. ([[98-phase-18-odp-reader]], [[101-phase-18-odp-writer]])
6. **DocMark-P carries no canvas size.** A `.pptx` states every rectangle in EMU relative to its own
   `p:sldSz` and the front matter has no key for that canvas, so a deck written from its Markdown
   alone lands on the target template's page — 4:3 corpus decks onto a 16:9 template. Seen above in
   the hand demonstration: a shape reaching 17.22 cm on a 15.75 cm page. The honest half is done —
   `write_gen::resize_page` uses `Presentation::slide_size` when the model states one, and
   `warn_outside_the_page` names every shape that falls off with both rectangles in the message. The
   fix is a `slide-size:` front-matter key and a specification increment. ([[103-phase-18-cross]])
7. **`presentation:settings` has no IR node and is carried only by the skeleton.** It is a playback
   setting, deck-level, not document content, with no shape to hang a stub on. It lives in
   `content.xml`, the writer never touches the bytes it lives in, and it comes back untouched — but
   the IR refers to no fragment for it, so a deck that lost its package loses it.
   ([[98-phase-18-odp-reader]], [[101-phase-18-odp-writer]])
8. **`col-widths="0,0"` on a slide table.** The ODF reader does not resolve `table:table-column`
   widths into the IR, so every column reads `0` — on an `.odp` slide table and on a table crossed
   into one. It is **existing ODF behaviour**, not something this phase introduced
   (`corpus/odt/table-simple.expected.dmk.md` says `col-widths="0,0,0"`), and fixing it moves `.odt`
   goldens. ([[100-phase-18-odp-docmark]], [[103-phase-18-cross]])
9. **A `draw:line` reaches the IR with no geometry.** ODF states a connector as `svg:x1/y1/x2/y2`
   and the reader does not translate that into `Geometry`, so `shapes-geometry.odp`'s connector has
   no `pos=`/`size=`. Nothing is lost silently — the whole `draw:line` is preserved as its raw
   fragment and the raw-block warning names it — but the stub is less informative than its `.pptx`
   counterpart. `crates/docsai-odf/src/draw.rs`. ([[100-phase-18-odp-docmark]])
10. **`Presentation::styles` is empty for every `.odp`.** ODF's presentation styles are resolved into
    deltas and its graphic styles into the image model, so nothing reaches `StyleCatalog` — correct
    de-automatization, and it means `inspect` reports `styles: 0` for a deck that plainly has styles.
    If that reads as a defect to a user, the fix is in `inspect`. ([[98-phase-18-odp-reader]])
11. **A `draw:g`'s child paragraphs concatenate into one label** (`raw-preserved.odp` shows
    `Dentro del grupoTambién dentro`). The `.pptx` reader does exactly the same on its own
    `raw-preserved` fixture, so this is shared behaviour of the raw-shape stub, not an ODF defect.
    ([[100-phase-18-odp-docmark]])
12. **`macro-enabled` has no ODF mirror in the corpus.** ODF has no macro-enabled MIME type — a macro
    lives in `Basic/` inside an ordinary `.odp`, which the reader ignores with `MacrosIgnored`. The
    trait exists; the *format distinction* does not, and no fixture carries it yet.
    ([[96-phase-18-corpus]])
13. **`docsai_model::assets::zip_kind` has no ODF branch.** It sniffs an OOXML ZIP by looking for
    `xl/workbook.xml`, `word/document.xml` or `ppt/presentation.xml` in the first 64 KB and answers
    `application/zip` / `zip` for everything else — so an ODF package stored as an asset sniffs as a
    plain ZIP, `mimetype` being the very first member notwithstanding. It matters only for the
    content type an asset is stored under, never for what is read (the `.odp` skeleton's own
    extension comes from `skeleton_path(format)`, fixed in 18-D). Same blast radius as carried item
    14 below, and the same one-branch fix.

### Inherited from `kb/93` and still open

Each re-checked here. Numbering continues; kb/93's own numbers are given so the two lists can be
matched. Where Phase 18 changed what an item says, the change is in the text.

14. *(kb/93 #1, **restated**)* **A preserved skeleton is no longer a `.pptx` property alone — it is a
    `.pptx` **and** `.odp` property.** The docx, odt, xlsx and ods writers still rebuild their parts
    from the model, so an *empty* batch still changes those files' hashes (re-measured here on
    `basic-text.docx`: `8f6c0978b31c → 95713ff26d32`). What has not caught up is the **edit** layer:
    `edit_transaction.rs`'s sibling test still covers five families with pptx as the sole preserved
    one, and `.odp` is in neither it nor `edit_matrix.rs` — see carried item 1. kb/93 wrote that «a
    writer that later gains a preserved skeleton turns the test red and earns a stronger assertion»;
    the writer arrived and the test stayed green, because the edit layer cannot reach it.
15. *(kb/93 #2)* **`odt/table-merged.odt`'s etag moves on a re-parse that changed nothing.** The ODF
    table reader yields rows of three cells where the DocMark of that table has four columns.
    `cargo test -p docsai-convert --test etag_stability` — 5 passed, including the test that measures
    it. Blast radius is exactly one node; the cost is a **false** `StaleEtag`, never an overwrite.
16. *(kb/93 #3, **narrowed**)* **`docx/`, `xlsx/`, `odt/` and `ods/` still have no schema gate and no
    render gate.** Phase 18 closed the presentation half of this — `.odp` now has both, on a vendored
    grammar and a real renderer — and did **not** close the spreadsheet or text half. It remains the
    strongest carried item in the list, because it is the one that hides the others. §«What the gates
    now cover» says what it would take.
17. *(kb/93 #4)* **`fullCalcOnLoad` is verified against LibreOffice Calc, not Excel.** Owner action if
    it matters.
18. *(kb/93 #5)* **The text parser drops an unknown container class *silently*** when the container's
    body flattens to exactly one block. Reproduced here: `docsai validate` on a file holding
    `::: {.sidebar}` / blank / `A` / blank / `:::` reports `container/unknown-class` as a warning,
    and `docsai convert` on the same file says **nothing** and exits 0. The deck parser warns in the
    same situation. A class quietly discarded is AGENTS.md §7 rule 3's silent degradation.
19. *(kb/93 #6, **refined**)* **The deck goldens carry a true skeleton warning** — re-measured here as
    **20 of 21** `.pptx` and **16 of 17** `.odp`. The exception on each side is
    `images-anchored.expected.dmk.md`, which does not warn because it **refuses**: the golden's
    picture bytes are not beside it either, so the conversion fails by name before the skeleton
    warning is reached. The warnings are correct — writing one of those goldens back would lose the
    package it was cut from — and the corpus, not the parser, is where it would be fixed.
20. *(kb/93 #7)* **`set_notes` cannot remove a notes page from a preserved deck, and says so.** The
    rule that matters holds: it is never silent.
21. *(kb/93 #8)* **A selector is not a target, and that is version 1's narrowing.** Adding a target
    form is a version 2 decision.
22. *(kb/93 #9)* **A package stores no node ids, so a structural edit renumbers them on the next
    read.** Settled as a property, written into `docs/edit-operations.md` §5.4.1, guarded by the
    etag.
23. *(kb/93 #10)* **`parse_shape` warns with the *slide's* line number, not the container's.**
    Reproduced here: a deck whose slide heading is on line 8 and whose `.chart` container opens on
    line 12 reports `degraded chart table at line 8`.
24. *(kb/93 #11)* **`count_shape_images` no longer only counts images** — it counts a chart's embedded
    workbook too, correctly, and the name lies (`crates/docsai-convert/src/inspect.rs:424`).
25. *(kb/93 #12)* **A path input is read twice on a cache miss** (`service.rs:416` hashes the bytes,
    `read_source` opens the file again).
26. *(kb/93 #13)* **Legacy `.doc` is never cached**, and that is a decision rather than an oversight
    (`service.rs:369` gates the insert on `benefits_from_loffice`).
27. *(kb/93 #14)* **`sniff` labels a `.pptm` as `.pptx`** (`docsai-model/src/assets.rs:215`, a
    bounded substring search for `ppt/presentation.xml`). It affects only the name an asset is stored
    under. See carried item 13 for its ODF sibling.
28. *(kb/93 #15)* **A stub over `mc:AlternateContent` gets no name and no geometry**, so
    `order::sort` puts it last on its slide. Reproduced on `ole-embedded.pptx`: slide 1's stub is
    `::: {#n3 .ole name="Hoja de cálculo 1" pos=… size=… raw=raw-0001}` and slide 2's is
    `::: {#n6 .ole raw=raw-0002}` and nothing else.
29. *(kb/93 #16)* **A picture inside an opaque object is not extracted as an asset of the deck.**
    Reproduced: `docsai inspect --json` on `ole-embedded.pptx` reports `"images": 0` and
    `"media": []` while the package carries `ppt/media/image1.png`.
30. *(kb/93 #17)* **DocMark carries nothing for a slide transition or a timeline**, so the write-side
    warning cannot fire on the parse-and-write path. Reproduced: `raw-preserved.pptx` through DocMark
    and back emits **zero** raw-block warnings where the direct model path emits two. The `.odp` side
    has the same shape and 18-C at least made the warning name the right dialect.
31. *(kb/93 #18)* **`corpus/schema_gate.py` selects `mc:Fallback` before validating**, which is what
    a consumer does and therefore correct — and means `ole-embedded.pptx`'s slide 2, the branch that
    holds the actual `p:oleObj`, has never been validated.
32. *(kb/93 #19)* **No corpus deck hides a slide** — re-checked, `show="0"` appears nowhere under
    `corpus/` — so 16-G's page-count-mismatch guard is reasoned rather than exercised.
33. *(kb/93 #20)* **`***bold italic***` under `{.underline}` does not survive a docx round trip.**
    Reproduced verbatim: `[***text***]{.underline}` comes back as
    `[**\*text**]{.underline}[\*]{.underline}`.
34. *(kb/93 #21)* **PowerPoint's modern comments are not read.** Reproduced on `comments-slide.pptx`:
    `[Severe] unsupported element <p188:cmLst> at ppt/comments/modernComment_1.xml`, with the part
    preserved unchanged.
35. *(kb/93 #22)* **The `.ole` stub does not carry `progId`** — reproduced, `progId` appears zero
    times in the DocMark of `ole-embedded.pptx`. Carrying it is a normative DocMark change.
36. *(kb/93 #23)* **A preserved chart's *shape* cannot be edited through DocMark — only its
    numbers.**
37. *(kb/93 #24)* **A generated chart has no design and no embedded workbook, and a generated combo
    chart is one plot.** Reinforced this phase: every `.odp` crossing into `.pptx` gets exactly this
    treatment, and the matrix declares it on thirteen cells.
38. *(kb/93 #25)* **A multi-level category axis is not read** (`pptx/chart.rs:273` warns by name),
    **and the `c:f` formulas are not in DocMark** — the second deliberate.
39. *(kb/93 #26)* **The cache charges an entry its *input* length plus its media bytes, not the size
    of the tree.** Bounded and one-directional.
40. *(kb/93 #27)* **Nothing caches a render.** `crates/docsai-convert/src/render.rs` contains no
    cache; a second `render_slides` on the same deck spawns LibreOffice again.
41. *(kb/93 #28)* **A classic pptx comment carries no thread and no resolved flag**, because
    `p:cmLst` is flat. The format, not a defect.
42. *(kb/93 #29)* **There is no `comments` counter in `ConversionStats` and no comments column in
    `inspect`'s slide inventory.** Re-checked: `inspect --json` on `comments-slide.pptx` has
    `stats` keys `paragraphs, headings, lists, tables, images, footnotes, slides, sheets, cells,
    formulas, styles` and no comment count; the four occurrences of the word are a path and two
    warning strings.
43. *(kb/93 #30)* **The human P2 gate itself.** `docs/spikes/P2-powerpoint-gate.md`, unrun. Owner
    action. §«Risk P2» above.
44. *(kb/93 #31)* **Gate 2's threshold of 2.5 has still never been scored against a real render.**
    Re-confirmed by this increment's own gate run: 254 pages compared and **67 of them** reporting
    `diff n/a (geometry)` — every comparable page — because every round trip is byte-identical.
    `THRESHOLD = 2.5` at `corpus/render_gate.py:102`.
45. *(kb/93 #32)* **The LibreOffice loader path is narrowed, not closed.** `corpus/render_gate.py` is
    a Python script and still needs `LD_LIBRARY_PATH=/usr/lib/libreoffice/program` in the command,
    which this increment set. The permanent fix is root and an owner action, and the variable stays
    out of every repository script.
46. *(kb/93 #33)* **The inherited Phase 14 criterion is still open.** `--fidelity agent` was to cost
    ≤ 15 % of `full` and measures ~100 % over the deck corpus. Still `#[ignore]`d rather than
    softened; it is the `1 ignored` in `936/0/1`.
47. *(kb/93 #34)* **A preserved table whose grid the model moved is copied with a warning**
    (`pptx/write.rs:2549`). Nothing owns writing a moved grid back into a preserved slide.
48. *(kb/93 #35)* **The refused internal hyperlink in pptx, and its docx twin.** Reproduced here:
    `[the section](#_Toc12345)` is written as `Target="#_Toc12345" TargetMode="External"` with
    `<w:hyperlink r:id="rId2">`, where WordprocessingML wants `<w:hyperlink w:anchor="_Toc12345">`
    and no relationship at all.
49. *(kb/93 #36, **narrowed**)* **Preserved parts are never schema-validated**, by construction, for
    `.pptx`. For `.odp` the fixtures themselves are validated, which narrows the hole on one format
    without closing it on either.
50. *(kb/93 #37)* **Both gates run only at `full`.** Re-checked: neither script contains the string
    `fidelity`.
51. *(kb/93 #38)* **`align`'s content-based pairing can still guess** when two or more slides are
    both edited *and* reordered (`pptx/write.rs:1145`). The real fix is an identity for a slide,
    which nothing in this chain carries. The `.odp` writer has the same shape of problem and the same
    answer: shapes pair by position when the counts agree, and by a greedy identity-then-kind walk
    when they do not, with a hole reported rather than a neighbour rewritten.
52. *(kb/93 #39)* **No real-world decks in the corpus.** All **201** corpus files are generated by
    `corpus/generate.py`, so no deck any real producer wrote has ever been through this reader. Item
    7 of the P2 form is still the first chance to get PowerPoint's own bytes back through it.

### Dropped from the list, with the reason

Six things earlier entries of this phase recorded as open no longer reproduce.

- **«The `.odp` schema arm validates fixtures, not writer output.»** ([[96-phase-18-corpus]] #4.)
  **Closed in 18-D**: the arm now runs the OOXML arm's own procedure — convert, retype every page
  title, write back on the preserved package, diff the members, validate what changed — *and* keeps
  validating the fixtures. It gained a pass rather than trading one.
- **«`.odp` renders are not compared against anything.»** ([[96-phase-18-corpus]] #6.) **Closed in
  18-D**: the round trip is compared on the pptx arm's comparator and threshold, and all seventeen
  land under «identical by bytes», which is strictly stronger than any render diff.
- **«DocMark-P loses an empty body placeholder, silently.»** ([[101-phase-18-odp-writer]] defect 2.)
  **Fixed in 18-E** in `docsai_model::addressing::walk::implicit_shapes`: rule 2 writes the implicit
  body's blocks at slide level, and an **empty** body has none, so it reached the file as nothing at
  all. An empty body is therefore never implicit, whatever the layout says, and rule 4's ordinary
  container costs one line and survives.
- **«DocMark-P turns an ODF `subtitle` placeholder into a body, silently.»**
  ([[101-phase-18-odp-writer]] defect 3.) **Fixed in 18-E**, in the same function and with the
  specification's own words: §11.2 rule 2 says «**the layout's** primary body placeholder is
  implicit», and the code conflated a layout the catalogue does not hold (which states nothing) with
  a layout it does hold that declares **no** body index (which has said there is no primary body).
  Not a specification change. Two `.odp` goldens moved and no `.pptx` golden did.
- **«The ODF table writer loses every table's header row, silently.»** ([[103-phase-18-cross]] defect
  4.) ODF states «the first row is a header» by *wrapping* it in `table:table-header-rows`, and
  `odt/write.rs::write_table` wrote every row loose, so a `.pptx` table crossing into a `.odp` came
  back with its header demoted and an empty header drawn above it. **Fixed**; the writer is shared
  with `.odt` and `.ods`, whose output no golden pins, and no golden moved. This is the ODT/ODS
  writer defect of this phase, and it is a member of the same family as Phase 17's two `.ods`
  defects — found only because something finally looked at the output.
- **«`crates/docsai-convert/tests/ppt_phase19.rs` writes into the corpus.»**
  ([[101-phase-18-odp-writer]] defect 4.) Phase 19's file, uncommitted at the time. It no longer
  reproduces: two consecutive `cargo test --workspace` runs here leave `git status --short corpus/`
  **empty**.

Two more were fixed *within* their own increment and are recorded so nobody re-opens them from an
increment's prose: **a dropped raw block named the wrong dialect** (`source dialect is pml` for ODF
markup — a false warning, now taken from the fragment itself), and **`p:ph type="text"` — invalid
OOXML, silently** (outside ECMA-376's closed enumeration; now substituted with `body` and warned,
found by the schema gate's new cross arm on four decks).

## What Phase 19 and Phase 20 inherit

A project that reads and writes **six** package families and converts between two presentation
dialects through one IR, with the ODF half of the presentation story better evidenced than the OOXML
half: a vendored grammar validates what the writer produces, and the reference consumer for ODF is
the renderer that is actually installed.

Three things to read before starting.

**Risk P2**, because the form is unsigned after three phases and Phase 20 is the last place before
v2.0 where signing it could still change a decision. §«Risk P2» above.

**Carried item 16**, the spreadsheet and text-document gate gap, because it is now the only format
family in this repository with no oracle of any kind, and because this phase demonstrated on `.odp`
exactly how cheap closing it is once somebody decides to.

**Carried item 1**, because it is a door that says it is open and is not: `docsai formats` says
`odp: write yes` and `docsai edit deck.odp` refuses with a message about an unsupported write
format. Phase 17 built the second door; Phase 18 built a format that door cannot reach.

## Next

Phase 19 (`docs/development-plan-v2.md:393`) — legacy `.ppt` reading, already under way and already
crossed with this phase twice. Then Phase 20 — hardening. **First, though**:
`docs/spikes/P2-powerpoint-gate.md`.
