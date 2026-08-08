---
created_at: 2026-08-07T16:01:55.030381543Z
updated_at: 2026-08-07T16:02:24.177066037Z
tags:
    - plan
    - phase-13
    - pptx
    - presentations
    - reader
---
# 34 — Phase 13 plan: PPTX reading → IR

Implementation plan for **Phase 13** of [`docs/development-plan-v2.md`](../docs/development-plan-v2.md),
opened after Phase 12 closed with three signed spikes ([[31-phase-12-spike-p1]],
[[32-phase-12-spike-p2]], [[33-phase-12-spike-p3]]) and a deck corpus
([[30-phase-12-pptx-corpus]]). Read [[11-plan-v2-onramp]] first: it lists what transfers from
Phases 0–7 and must not be rebuilt.

Objective restated: **`Document::Presentation` exists in the IR, and a reader fills it.** No
DocMark-P serializer, no writer — those are Phases 14 and 15 and `AGENTS.md` §7 rule 1 keeps
them out of here.

## What the spikes already decided, and this phase must obey

- **Custom parser on `zip` + `quick-xml`** ([[31-phase-12-spike-p1]]). `ooxmlsdk` is not linked.
  The pptx reader is a sibling of `docx/` and `xlsx/` inside `docsai-office`, reusing
  `package.rs`, `xml.rs` and the existing `_rels` resolution.
- **Placeholders are implicit in DocMark-P, but explicit in the IR** ([[32-phase-12-spike-p2]]).
  The profile's implicitness is a *serialization* decision that needs the layout catalogue to
  resolve title/body. So the IR must carry, per layout, which placeholder is the title and which
  is the primary body — otherwise Phase 14 cannot write the implicit form. This is the one
  requirement P2 pushes back into the model.
- **Preserve everything, rebuild as the exception** ([[33-phase-12-spike-p3]]). Slide parts are
  found through `[Content_Types].xml`, never by file name. Holding a package in memory costs
  2.7× its size, so parts that are never inspected are not decompressed.

## Increments

Each is independently testable and ends green on `cargo test --workspace`, `clippy -D warnings`
and `fmt --check`.

### 13-A — the third IR root

`docsai-model::presentation`: `Presentation { meta, styles, layouts: LayoutCatalog, slides,
skeleton: Option<SkeletonRef>, addressing }`, `Slide`, `Shape`, `ShapeGeometry`, `PhType`,
`LayoutId`/`LayoutRef`, `ChartRef`. `Document::Presentation` as the third variant, with
`meta()`, `styles()`, `addressing()` extended. `Format::Pptx` + `.pptm` parsed as `Pptx`.
`validate.rs` and `addressing/walk.rs` cover the new nodes. No reader, no I/O.

Follows architecture §9.1, with the P2 correction: `LayoutCatalog` entries name their title and
body placeholder indices.

### 13-B — package layer and slide order

`docsai-office::pptx`: open the OPC package, resolve `[Content_Types].xml`, walk
`ppt/presentation.xml` + its `_rels`, honour `p:sldIdLst` for order and `p14:sectionLst` in
`extLst` for sections. Layout → master → theme parts located and catalogued. Produces a
`Presentation` whose slides are empty but correctly identified, ordered and layout-referenced.

### 13-C — slide text

`a:p`/`a:r` → `Paragraph`/`Inline` with the existing run-property model; `a:pPr@lvl` +
`buChar`/`buAutoNum` → `List`; `a:fld` → `Inline::Field` (the `w:fldSimple` mapping already
exists). Empty placeholders stay as real content — the on-ramp names this as a known trap.

### 13-D — the cascade as reference + delta

Shape → layout placeholder → master → theme (`clrMap`, `schemeClr`, `+mj-lt`/`+mn-lt`) plus
`defaultTextStyle`. Resolved for colour, font and size; **only the delta is stored**, the layout
reference stays. Test that a placeholder inheriting everything emits zero properties.

### 13-E — pictures and tables

`p:pic` → `ImageRef`/`ImageGeometry` + `AssetStore` (no new image model). `p:graphicFrame` →
`a:tbl` → the IR `Table`.

### 13-F — notes

`ppt/notesSlides/*` → `Slide::notes`, reached through the slide's `_rels`, not by index.

### 13-G — deterministic, reversible reading order

Policy: placeholders first by type, then remaining shapes by top-left. The original `spTree`
index travels as data on every shape so the order is reversible (analysis §5.3).

### 13-H — skeleton capture

Non-slide parts stored opaquely through `AssetStore` (content-hash dedupe already exists),
referenced from `Presentation::skeleton`. Streamed, not held: P3 measured the 2.7× cost.

### 13-I — nothing disappears

SmartArt (`dgm:*`), `p:timing`, `p:transition`, OLE, `custGeom`, connectors and groups beyond a
stub → raw block in the Phase 11 sidecar + a visible stub + a typed `Warning`. `Warning::AutofitStale`
for a dropped `a:normAutofit@fontScale` (analysis §5.4).

### 13-J — `.pptm` and corrupt input

`.pptm` read as its macro-free equivalent with a security warning, exactly as `.docm`. Zero
panics on the synthetic corrupt corpus: truncated ZIP, malformed XML, dangling relationships —
always a typed `Err`.

### 13-K — `inspect` slide inventory

`inspect` reports per slide: layout used, shape count, has-notes, has-SmartArt/OLE — what an
agent needs to decide where to edit without loading the deck.

## Acceptance criteria (tracked here)

- [x] Corpus pptx IR goldens pass, including reading order, notes and placeholder identity.
      `<name>.expected.inspect.json` beside every deck, 13-K ([[45-phase-13-inspect-inventory]]).
- [x] Zero panics on the synthetic corrupt corpus: always `Err`. 13-J ([[44-phase-13-pptm-robustness]]).
- [x] A 40-slide deck reads in < 1 s. Synthetic, not real: `forty-slides.pptx`, and the fixture
      says so — no real-world deck can live in the repository.
- [x] Every unmodelled element produces a stub + sidecar raw + typed warning; none disappears.
      13-I ([[43-phase-13-raw]]).
- [x] `.pptm` emits the macro security warning. 13-J.

**One criterion is deferred with a reason.** The plan's *«`--fidelity agent` on that deck is
≤ 15 % of the `full` token count»* cannot be measured in Phase 13: token counts are measured over
DocMark, and the DocMark-P serializer is Phase 14 task 1. Measuring it here would mean building
the serializer here, which is the scope expansion `AGENTS.md` §7 rule 1 forbids. It moves to
Phase 14's acceptance, where the serializer that produces the number exists. Likewise the phase's
goldens are **IR goldens** (`inspect --json`), not DocMark goldens, for the same reason —
[[30-phase-12-pptx-corpus]] already recorded that `corpus/pptx` has no goldens yet.

## Rules that bind this phase

- No Phase 14+ work: no DocMark-P serializer or parser, no pptx writer.
- No new crate, no format crate importing another (`AGENTS.md` §3).
- No panic on corrupt input, ever: typed `Err`.
- Nothing lost silently: stub + sidecar raw + typed warning.
- Every increment recorded in the KB when done (MANDATORY 5), linked back to this plan.