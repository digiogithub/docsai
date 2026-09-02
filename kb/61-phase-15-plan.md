---
tags:
    - plan
    - phase-15
    - pptx
    - presentations
    - writer
    - skeleton
    - anti-repair
---
# 61 — Phase 15 plan: writing a pptx, and the anti-repair gate

Implementation plan for **Phase 15** of [`docs/development-plan-v2.md`](../docs/development-plan-v2.md),
opened after Phase 14 closed with DocMark-P, its parser, its goldens and the P4 hand-edit gate
([[57-phase-14-p4-gate]]). Read [[11-plan-v2-onramp]] first, and [[42-phase-13-skeleton]] before
touching a single byte of the package: this phase is the other half of that mechanism.

Objective restated: **a deck goes out as a `.pptx` that PowerPoint opens without offering to
repair it.** The cycle `pptx → DocMark → pptx` closes here, and the failure mode that decides the
phase is not a diff — it is a dialog.

## What is already decided, and this phase must obey

- **Preserve everything; rebuilding is the exception.** Spike P3 measured it
  (`docs/spikes/P3-preserved-skeleton.md`): a deck whose non-slide parts are regenerated loses a
  chart's values and four `dgm:` parts of a diagram with no visible change at all. The skeleton is
  the whole original package, byte for byte, and
  [`SkeletonRef::rebuilt_parts`](../crates/docsai-office/src/pptx/skeleton.rs) is the **only**
  licence to regenerate a part. A part not on that list is copied, never rewritten.
- **The ZIP itself is part of the fidelity.** Member order, compression method and the entry's own
  stored bytes are not reproducible from a decompressed `Package` (which is a `BTreeMap` and says
  so). The writer re-emits the original archive entry by entry and substitutes only the parts it
  is licensed to rebuild.
- **LibreOffice is not the oracle.** P3 §2.4 measured LibreOffice accepting a deck with five
  dangling relationships. It stands in for a render diff and for a smoke test; risk **P2** closes
  only with the PowerPoint checklist of P3 §6.2, run by a human.
- **The reader is the input, unchanged.** Phase 13 stores deltas against the cascade
  ([[38-phase-13-cascade]]) and the sidecar raw ([[43-phase-13-raw]]); the writer puts back what
  the IR holds and invents nothing.
- **Tolerant input is a requirement, not a nicety** (analysis §6.6): a deck written from
  attribute-free Markdown, with no skeleton at all, must open with a sensible layout. That is what
  removes the agent's «generate → fail → fix» cycle.

## Increments

Each is independently testable and ends green on `cargo test --workspace`, `clippy -D warnings`
and `fmt --check`.

### 15-A — the package comes back out

`docsai-office::pptx::write`: the preserved skeleton re-emitted **byte for byte** — same members,
same order, same compression method, same stored bytes — through a raw ZIP copy rather than a
decompress/recompress cycle. No slide is regenerated yet, and the writer says so with a typed
warning rather than pretending the IR was consulted. `Format::Pptx` reaches
`docsai_office::write`; a deck with no skeleton is a typed error naming 15-G. The pipeline and
`SUPPORT` stay read-only until 15-B: a `convert` that silently ignored the edits in the IR would
be worse than the refusal it replaced. This increment is the substrate the next five are written
against, and the first honest answer to «is a written deck still the deck we read». Done, in
[[62-phase-15-package]].

### 15-B — the slide part regenerated

Slides rebuilt from the IR into `ppt/slides/slideN.xml` and substituted into the skeleton at the
names `rebuilt_parts` licenses. Placeholders (title, body, the rest), paragraphs, runs and their
deltas — the mirror of [[37-phase-13-slide-text]] and [[38-phase-13-cascade]]: a property the
cascade already implies is not written back as a direct one. Done, in [[63-phase-15-slides]].

### 15-C — pictures, tables and media

Media re-injected from the `AssetStore` **without recompression** (the bytes that came out are
the bytes that go in), the slide's relationship part rewritten to match, and tables written back
through the DrawingML table writer. Content types for a media extension the skeleton did not
already declare. Done, in [[64-phase-15-media]], where the table writer turned out to be the wrong
tool: a cell is a text body, and the grid around it is preserved like everything else.

### 15-D — raw re-injection

A shape the IR kept only as a raw fragment ([[43-phase-13-raw]]) returns as its original XML at
its original z-order: SmartArt, OLE, charts and custom geometry. A fragment whose dialect is not
`ooxml` is dropped with `Warning::RawBlockDropped`, never emitted as nonsense. Done, in
[[65-phase-15-raw]], where the preserved skeleton turned out to have done most of the work
already: the increment's real content is saying so honestly, and covering the payload that
differs from it.

### 15-E — notes pages, sections and the deck's own parts

`ppt/notesSlides/notesSlideN.xml` regenerated for the slides that have notes, `p:sldIdLst` and
`p14:sectionLst` updated when slides were added, removed or reordered — the only edits to
`ppt/presentation.xml`, which is otherwise copied. Done, in [[66-phase-15-notes]], where a slide
removed or reordered is written and a slide *added* is refused by name: there is no part to splice
it into, and writing one is 15-G. The increment's real difficulty was that nothing in the chain
carries an identity for a slide, so which package part a model slide *is* has to be recovered from
its content.

### 15-F — `Warning::AutofitStale` on the way out

Analysis §5.4: a `fontScale`/`lnSpcReduction` PowerPoint computed for text that is no longer there
is a lie no reader can detect. When the shape's text changed, the stale scale is dropped and the
warning is emitted. The variant already exists in `docsai-model`; this is the write side of it.
Done, in [[67-phase-15-autofit]], where «the shape's text changed» turned out to be a fact the
writer already had — 15-B's rule that nothing is written unless it differs from what the part
already says — rather than a second notion of change to be invented, and the one variant had to
stop asserting that the number is kept before it could name either outcome honestly.

### 15-G — the deck with no skeleton

An embedded default template plus **automatic layout selection from content shape** (title +
bullets → «Title and Content», title only → «Title Slide»), so attribute-free Markdown produces a
correct package. Analysis §6.6, and the acceptance criterion that a deck written from plain
Markdown opens with a sensible layout. Done, in [[68-phase-15-template]], where the template turned
out to be best understood as a *preserved package* like any other — generated from source by
`corpus/generate.py` and embedded with `include_bytes!` — so that everything 15-B..15-F built keeps
working unchanged over it, and the one new operation is writing a part the package does not have.
That same operation lifts the two refusals 15-E pointed here for: a slide the model added, and a
notes page for a slide that has none.

### 15-H — `roundtrip` for presentations

The existing `roundtrip` command extended to decks, with the per-category fidelity metric, and
**idempotence**: a second pass equals the first over the whole pptx corpus. Done, in
[[69-phase-15-roundtrip]], where extending the command was four lines and making its answer *true*
was the increment: the measurement found two defects — the external file attributes `zip` rewrites
on every entry, which changed the content hash of the preserved package and so renamed it on every
write, and the writer indexing the shape tree by `Shape::z_index`, which DocMark does not carry, so
a deck whose reading order is not its z-order had every shape's text written into another shape.
Both are fixed in the writer; the metric's categories for a deck are stated there.

### 15-I — CI gate 1: schema validation

Generated packages validated against the ECMA-376 XSDs (analysis §5.5). A package that is not
schema-valid fails the build; the gate names the part and the element. Done, in
[[70-phase-15-xsd]], where the schemas are **vendored** — 1.0 MB, unmodified, from the standard's
own distribution zips, because an oracle that can change under the gate without a commit is not an
oracle — and `xmllint` runs them, which is the §7 rule 4 justification: nothing new reaches a
shipped binary. What the gate validates is not «a round trip's output», which for a preserved deck
is byte for byte the input: it is every part whose bytes the writer **changed or created**, found
by diffing the written package against the one it was written on, over an edited deck and over the
same deck with no skeleton at all. It went red on its first run and the three findings were two
corpus fixtures that are not legal OOXML — a `p:txBody` with no `a:p`, and a table cell holding a
`p:txBody` where an `a:txBody` belongs — plus one false positive that turned out to be a missing
ECMA-376 Part 3 step, not a reason to loosen anything.

### 15-J — CI gate 2: render diff

Headless LibreOffice renders the corpus to PNG before and after the round trip, perceptual diff
over a threshold fails the build. LibreOffice absent is a skip with a clean message, never a
green tick that measured nothing. Done, in [[71-phase-15-render]], where the last clause turned out
to be the whole increment: LibreOffice is broken on the development machine, so the gate was written
by someone who could not watch it render a slide. The renderer is `soffice --convert-to pdf` then
`pdftoppm` — LibreOffice exports a *single* page to PNG, which would have rendered the first slide
of every deck and called the rest measured — and the diff is thirty lines of stdlib arithmetic over
the PGM the rasteriser writes, so **no image dependency is added anywhere**, which is the §7 rule 4
answer in full. The threshold, 2.5 on 100 × RMSE over 8 × 8 block means, is measured: 0.0000 for the
same file twice, 1.72 for anti-aliasing on against off, 3.55 for one text line of twelve gone. Like
gate 1 it refuses to measure a byte-identical round trip and says so; the generated-on-template
package is checked for the same page count and for no page that came out blank. A missing **or
broken** renderer is a skip with a banner and exit 3, and a failure under `--require-renderer`, which
is what CI passes.

### 15-K — the P2 gate and the phase close

The PowerPoint checklist of `docs/spikes/P3-preserved-skeleton.md` §6.2, run by a human on three
decks and recorded with its outcome; risk **P2** closed in
`docs/technical-analysis-presentations.md` §7 if it passes. Docs, CHANGELOG, README, `AGENTS.md`
status and the acceptance criteria checked here. Done, in [[72-phase-15-close]] — with the one
thing this increment could not do left visibly undone: **the checklist has not been run**, because
running it needs PowerPoint and a person, and this machine has neither. What 15-K delivers is
everything around that step: the three decks and the negative control, reproducible in one paste;
the fill-in form `docs/spikes/P2-powerpoint-gate.md`; risk P2 left **open** with §7.1 spelling out
what the two gates do and do not prove and carrying the wording that closes it when the form is
signed; and the acceptance criteria settled — two true, three waiting on an application opening a
file, one of them un-ticked here because its verb is «opens» and nothing has.

## Acceptance criteria (settled in 15-K)

Two of five are true today. The three that are not all wait on the same thing — an application
actually opening a file — and none of them is softened to make the phase look finished.

- [x] **pptx round-trip idempotent (2nd pass == 1st pass) over the whole pptx corpus** — 15-H
      ([[69-phase-15-roundtrip]]). All seventeen decks of `corpus/pptx`, `.pptm` included: the
      second DocMark pass is the first, every category of the per-category metric is 100 %, and
      each written package is byte for byte the package that was read. Re-verified in 15-K:
      `cargo test --workspace --no-fail-fast` → 666 passed, 1 failed, 1 ignored, and neither the
      failure (`docx_roundtrip_is_idempotent` on `corpus/docx/footnotes.docx`, pre-existing since
      `88ff8e11` and unrelated to this phase) nor the ignored test
      (`agent_fidelity_is_at_most_fifteen_percent_of_full`) is a pptx one.

- [ ] **Both CI gates green on the corpus.** Gate 1, schema validation, **is** green — 15-I
      ([[70-phase-15-xsd]]), re-run in 15-K: 17 decks, 34 packages, 322 parts, 0 invalid, 0 not
      measured. Gate 2 is **built and wired** — 15-J ([[71-phase-15-render]]): `corpus/render_gate.py`,
      its own `render-gate` job in `.github/workflows/test.yml`, its comparator measured on fourteen
      pairs and its threshold chosen from those numbers, four negative controls red and named.
      What was missing in 15-K was the only thing that would make this box true: **no corpus slide
      had ever been rendered by LibreOffice on any machine** — re-run then, `corpus/render_gate.py`
      exited 3 with the skip banner (`soffice.bin: libreglo.so`) and `--self-test` exited 0.

      **Amended, and still un-ticked.** The `libreglo.so` failure has since been diagnosed as a
      dynamic-loader fault rather than a broken install, and with
      `LD_LIBRARY_PATH=/usr/lib/libreoffice/program` the gate has now been run for real on this
      machine: `python3 corpus/render_gate.py --require-renderer` → *17 decks, 17 round trips
      identical by bytes, 0 rendered, 61 pages compared, 0 failures, 0 not measured*, exit **0**
      ([[71-phase-15-render]]). That is a real advance — LibreOffice opens and renders every
      generated package without failing, and no page came out blank — and it is still not this
      criterion, for two reasons, both about the words in it.

      *«Green on the corpus.»* Nothing in that run was scored. The seventeen round trips took the
      byte-identity short circuit and were never rendered at all; the 61 pages that were rendered
      are the generated packages, and every one reports `diff n/a (geometry)` because the embedded
      template's canvas is not the corpus deck's, so only the ink check ran. A gate that rendered
      the corpus and produced no perceptual-diff score has not measured how the corpus looks; the
      threshold of 2.5 remains chosen from `--self-test` proxies and unvalidated against real
      renderer output.

      *«CI gates.»* The `render-gate` job of `.github/workflows/test.yml` has still never executed:
      the newest run on the remote is `0aac4eb` (2026-08-21), and that commit's workflow does not
      contain the job. A green local run on one developer's machine is evidence, not a CI result.

      So the box stays open, with the gap now much smaller and precisely named: **not «it has never
      rendered», but «no render has ever been scored, and CI has never run it».** It closes on a
      green `render-gate` job that produced at least one real diff number, and no earlier.

- [ ] **No repair dialog in PowerPoint or LibreOffice for any corpus output (checklist signed).**
      The checklist is now written and the decks it needs are reproducible in one paste:
      `docs/spikes/P2-powerpoint-gate.md` (15-K, [[72-phase-15-close]]). It is **unrun**, because
      running it needs PowerPoint and a human, and neither is available here. Everything that can
      be checked without an application has been: every deck it builds passes `corpus/opc_check.py`
      and its negative control fails it. This box is the last one Phase 15 owns, and it is an
      owner action, not a code action.

- [ ] **A deck written from *attribute-free* Markdown opens correctly with a sensible layout.**
      **Un-ticked in 15-K, deliberately** — 15-G ([[68-phase-15-template]]) ticked it with the
      caveat written beside it, and on re-reading, the caveat *is* the criterion: the verb is
      «opens». What is true is everything short of it — the layout is chosen from the shape of the
      content, the package is well formed (parts, content types, every relationship resolving,
      `opc_check.py` clean), every part the writer created is schema-valid under gate 1, and this
      project's own reader recovers the deck. What is not true is that any application has ever
      opened one. Deck **C** of the P2 gate form is that file; item 5 of the form is this criterion.

- [x] **Images rewritten without recompression; geometry compared with the documented tolerance.**
      Both halves hold, and neither was measured before 15-K said so out loud. *Recompression*:
      media go back as the bytes the `AssetStore` holds, asserted on a swapped image whose
      extension the deck never carried —
      `pptx::write::tests::a_swapped_image_goes_in_as_its_own_bytes_and_is_declared` — and the
      preserved path never inflates an entry at all (15-A copies stored bytes). *Geometry*: the
      documented tolerance is **zero**, fixed in Phase 11-E and normative in
      `docs/docmark-specification.md` §2 (`readable_units.rs::the_tolerance_is_zero_at_every_precision`),
      so the comparison is byte equality — which is exactly what 15-H asserts over the corpus. The
      criterion was written expecting a non-zero tolerance; the project earned a stricter one.

## Rules that bind this phase

- No Phase 16+ work: no chart data model, no thumbnails, no cache, no `apply_edits`.
- `rebuilt_parts` is the licence. A part outside it is copied verbatim, whatever the IR seems to
  know about it.
- Nothing lost silently, on the way out as much as on the way in (`AGENTS.md` §7 rule 3).
- No new crate, no format crate importing another (§3). An XSD validator or an image-diff
  dependency is a §7 rule 4 justification, written in the increment that adds it — and CI-only.
- Every increment recorded in the KB when done (MANDATORY 5), linked back to this plan.
