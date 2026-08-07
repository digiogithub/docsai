---
created_at: 2026-08-07T16:14:49.266638887Z
updated_at: 2026-08-07T16:15:10.621722272Z
tags:
    - phase-13
    - pptx
    - presentations
    - model
    - ir
---
# 35 — Phase 13 A: the third IR root

Increment **13-A** of [[34-phase-13-plan]], the first work of the pptx reader track after
Phase 12 closed. It adds the model a reader will fill; there is **no reader yet**, and per
`AGENTS.md` §7 rule 1 none of Phase 14's serializer either.

## What changed

`crates/docsai-model/src/presentation.rs` (new): `Presentation`, `Slide`, `Shape` +
`ShapeKind`, `Placeholder`, `PhType`, `ShapeGeometry`, `ShapeProps`, `LayoutCatalog` /
`Layout` / `Master` / `LayoutPlaceholder`, `ChartRef`, `RawShape` + `RawShapeKind`,
`SkeletonRef`, `LayoutId`, `MasterId`.

- `Document::Presentation` is the third variant; `meta()`, `styles()`, `addressing()` and a new
  `Document::shape_name()` cover it. `shape_name` exists because five writers had to say
  «cannot write X as .docx» and were each spelling it differently.
- `Format::Pptx`, with `.pptm` parsed as its macro-free equivalent exactly as `.docm` is.
- `ConversionStats::slides`, merged by `ConversionReport::merge`.
- `addressing::walk`: `NodeKind::Slide` / `NodeKind::Shape`, `Addressable` for both, and the
  read-only and mutable traversals extended.
- `validate`: `ValidationError::SheetAnchorInPresentation`, and the block walker parameterised
  by a private `Root` so the same traversal names the right error per document root.
- `detect`: a package holding `ppt/presentation.xml` is `Format::Pptx`, **Certain**.
- `docsai formats` lists pptx as `read: no, write: no` — recognised, honestly unsupported.

## Non-obvious decisions

1. **`Shape` is a struct with a `kind`, not the bare enum architecture §9.1 sketched.**
   Identity, name, geometry and the source `p:spTree` index belong to every shape; repeating
   four fields across seven variants is how they drift apart. §9.1 is updated to match, with the
   deviation stated.
2. **A layout names its title and its primary body** (`Layout::title()`, `Layout::body()`). This
   is the one requirement [[32-phase-12-spike-p2]] pushes back into the model: the profile's
   implicit placeholders are only resolvable from the catalogue.
3. **The title and the primary body are not addressable.** They are written as the slide heading
   and as plain blocks under it, so there is nowhere to put an id, and an id that cannot be
   written moves on every round trip — the rule `walk.rs` already applied to sections and rows.
   `addressing::implicit_shapes(slide, layouts)` is the single answer, deliberately public so
   Phase 14's serializer asks the same function instead of reimplementing the rule.
4. **Geometry absent means inherited**, not zero. Same reference-plus-delta rule as the styles;
   `ShapeGeometry::is_inherited()` is what a writer checks before emitting anything.
5. **Serialising a deck to DocMark emits an empty body and a severe
   `Warning::UnsupportedElement`.** DocMark-P is Phase 14; until then the failure is loud. A
   silently empty document is the failure mode this project refuses.
6. **Detection landed here rather than in 13-B.** `docsai formats` listing pptx while detection
   called the same file unknown was a contradiction visible to a user in one command.

## How it was verified

- `cargo test --workspace` green (new: `json_roundtrip::round_trips_a_presentation_using_every_shape_kind`,
  three `addressing.rs` deck tests, two `validate` tests, seven `presentation.rs` unit tests, one
  `detect` test).
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` clean.
- `python3 corpus/generate.py --check` green (89 files, unchanged).
- `docsai convert corpus/pptx/basic-slides.pptx` now exits 3 with
  «unsupported conversion: pptx -> docmark» instead of «could not tell what kind of document».

## Next

13-B: the package layer — `[Content_Types].xml`, `p:sldIdLst` order, sections, and the
layout/master/theme catalogue, producing slides that are correctly identified but still empty.