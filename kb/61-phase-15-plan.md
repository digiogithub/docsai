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
status and the acceptance criteria checked here.

## Acceptance criteria (tracked here)

- [x] pptx round-trip idempotent (2nd pass == 1st pass) over the whole pptx corpus — 15-H
      ([[69-phase-15-roundtrip]]). All seventeen decks of `corpus/pptx`, `.pptm` included: the
      second DocMark pass is the first, every category of the per-category metric is 100 %, and
      each written package is byte for byte the package that was read.
- [ ] Both CI gates green on the corpus. **Still not tickable, and for a different reason now.**
      Gate 1, schema validation, is green — 15-I ([[70-phase-15-xsd]]): 34 packages written from the
      seventeen corpus decks, 322 parts validated, 0 invalid, 0 unmeasured. Gate 2 is **built and
      wired** — 15-J ([[71-phase-15-render]]): `corpus/render_gate.py`, its own `render-gate` job in
      `.github/workflows/test.yml`, its comparator measured on fourteen pairs and its threshold
      chosen from those numbers, four negative controls red and named, and the skip path exercised on
      this machine's real broken LibreOffice. What is missing is the only thing that would make this
      box true: **no corpus slide has ever been rendered by LibreOffice here**, so gate 2 has never
      been *green on the corpus* — it has been green on its comparator and on a stand-in renderer.
      The first real numbers come from the first CI run, and the threshold may have to move when they
      do. The box closes when that run is seen, and no earlier.
- [ ] No repair dialog in PowerPoint or LibreOffice for any corpus output (checklist signed).
- [x] A deck written from *attribute-free* Markdown opens correctly with a sensible layout —
      15-G ([[68-phase-15-template]]). **With one caveat, stated rather than hidden**: the layout is
      chosen and the package is well formed (parts, content types, every relationship resolving,
      and this project's reader recovering the deck), but no application has *opened* it — the
      LibreOffice install on this machine is broken, and «opens without a repair dialog» is the P2
      checklist of 15-K.
- [ ] Images rewritten without recompression; geometry compared with the documented tolerance.

## Rules that bind this phase

- No Phase 16+ work: no chart data model, no thumbnails, no cache, no `apply_edits`.
- `rebuilt_parts` is the licence. A part outside it is copied verbatim, whatever the IR seems to
  know about it.
- Nothing lost silently, on the way out as much as on the way in (`AGENTS.md` §7 rule 3).
- No new crate, no format crate importing another (§3). An XSD validator or an image-diff
  dependency is a §7 rule 4 justification, written in the increment that adds it — and CI-only.
- Every increment recorded in the KB when done (MANDATORY 5), linked back to this plan.
