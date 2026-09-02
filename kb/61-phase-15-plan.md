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

### 15-G — the deck with no skeleton

An embedded default template plus **automatic layout selection from content shape** (title +
bullets → «Title and Content», title only → «Title Slide»), so attribute-free Markdown produces a
correct package. Analysis §6.6, and the acceptance criterion that a deck written from plain
Markdown opens with a sensible layout.

### 15-H — `roundtrip` for presentations

The existing `roundtrip` command extended to decks, with the per-category fidelity metric, and
**idempotence**: a second pass equals the first over the whole pptx corpus.

### 15-I — CI gate 1: schema validation

Generated packages validated against the ECMA-376 XSDs (analysis §5.5). A package that is not
schema-valid fails the build; the gate names the part and the element.

### 15-J — CI gate 2: render diff

Headless LibreOffice renders the corpus to PNG before and after the round trip, perceptual diff
over a threshold fails the build. LibreOffice absent is a skip with a clean message, never a
green tick that measured nothing.

### 15-K — the P2 gate and the phase close

The PowerPoint checklist of `docs/spikes/P3-preserved-skeleton.md` §6.2, run by a human on three
decks and recorded with its outcome; risk **P2** closed in
`docs/technical-analysis-presentations.md` §7 if it passes. Docs, CHANGELOG, README, `AGENTS.md`
status and the acceptance criteria checked here.

## Acceptance criteria (tracked here)

- [ ] pptx round-trip idempotent (2nd pass == 1st pass) over the whole pptx corpus.
- [ ] Both CI gates green on the corpus.
- [ ] No repair dialog in PowerPoint or LibreOffice for any corpus output (checklist signed).
- [ ] A deck written from *attribute-free* Markdown opens correctly with a sensible layout.
- [ ] Images rewritten without recompression; geometry compared with the documented tolerance.

## Rules that bind this phase

- No Phase 16+ work: no chart data model, no thumbnails, no cache, no `apply_edits`.
- `rebuilt_parts` is the licence. A part outside it is copied verbatim, whatever the IR seems to
  know about it.
- Nothing lost silently, on the way out as much as on the way in (`AGENTS.md` §7 rule 3).
- No new crate, no format crate importing another (§3). An XSD validator or an image-diff
  dependency is a §7 rule 4 justification, written in the increment that adds it — and CI-only.
- Every increment recorded in the KB when done (MANDATORY 5), linked back to this plan.
