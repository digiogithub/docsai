---
tags:
    - phase-14
    - docmark
    - presentations
    - serializer
    - images
    - tables
---
# 52 — Phase 14 F: pictures, tables, groups and stubs

Increment **14-F** of [[46-phase-14-plan]], after the notes of [[51-phase-14-notes]]. Rule 4 is
about shapes Markdown has no form for; this increment is about the ones it *does* have, and about
the ones it will never have. With it, every `ShapeKind` is written — nothing on a slide is
«skipped: not written yet» any more.

## What changed

- `docsai-docmark::deck_writer`: `write_picture`, `write_table`, `write_group` (new) and a `.chart`
  arm; the `RawShapeKind` guard is gone, so SmartArt, OLE, media and unclassified objects are stubs
  of their own class instead of a warning.
- `docsai-docmark::deck_writer::placement` (new): the `name=`/`pos=`/`size=`/`rotation=`/`flip=`
  block, shared by every shape that has geometry, with `size` optional.
- `docsai-docmark::writer`: `image_line` split out of `render_image_body` so a caller can seed the
  attributes; `render_slide_table`; `render_table_body`/`render_complex_table` take an `Attrs` seed
  instead of an id; `asset_path`; `for_deck` and the `image_geometry` predicate.
- `docs/docmark-specification.md` §11.2: a **pictures, tables, groups and stubs** subsection.

## Non-obvious decisions

1. **A picture is an image line, not a container.** Rule 4 exists because Markdown has no shape;
   Markdown *has* an image, so a `:::` around it would be a box a reader cannot use and a plain
   viewer would print. The shape's id and position ride on the image's own attribute block, which
   is also what the addressing walk already assumes: `each_shape` gives a picture shape one id and
   deliberately does not descend into the `ImageRef`.
2. **The picture's size stays the image's, its position stays the shape's.** `read_picture` copies
   `a:ext` into `ImageGeometry::display_size` and `read_geometry` copies the same `a:xfrm` into the
   shape, so writing both would be one measurement in two places, free to disagree after an edit.
   `placement(shape, writer, with_size = false)` is that decision, in one argument.
3. **`image_geometry()` is a property of the document class, not of the level.** Spike P2 §3.3
   measured that `{width=… height=…}` is residue on a slide — a viewer draws the picture at its own
   size — but §3.5's *«`width`/`height` are always required»* is a round-trip rule that still holds
   for a docx at the same level. Hence `Writer::for_deck` and a predicate next to `formatting()`
   rather than a new fidelity level, and a test that asserts a text document keeps what a slide
   drops.
4. **A table shape is one addressable node, not two.** `render_table` takes an id for the `Table`;
   on a slide the walk addresses the `Shape` and never the table inside it. So `render_table_body`
   now takes an `Attrs` seed and the deck writer hands it the shape's id and placement — which also
   gives the container a reason to exist where a bare GFM table would have had none.
5. **A group is a container, not a flattening.** Every shape inside a group is addressable in its
   own right (`each_shape` says so explicitly), so writing the children and dropping the box would
   lose the grouping silently. `::: {.group}` nests with the same three colons the complex-table
   containers already nest with, and the group takes its id *before* its children, which is the
   order the walk visits them in. At `plain` the children survive and the grouping is the loss.
6. **A chart's title is its body, not an attribute.** A stub with `title="Revenue by region"` says
   nothing to a plain reader, who sees no attributes at all; the title as body text survives every
   level, which is the same reason a preset shape's label is written inside its container.
7. **`data=` is not a raw payload.** The embedded workbook is a real file beside the document, so
   rule 6 does not take it away at `standard` — unlike `raw=`, which points into the sidecar. Note
   that the Phase 13 reader keeps the workbook in the skeleton and leaves `ChartRef::workbook`
   `None`, so today the attribute only appears for an IR built by hand; the spec says so in place
   rather than promising something the reader does not produce.
8. **An empty table is a warning, not an empty table.** GFM needs a row to be a table at all, and
   `render_table_body` would have written a header rule over nothing. The document path already
   returned early for this; the slide path says why out loud.

## What is deliberately not written

- **A chart's series.** Phase 16 turns them into a table; until then `kind=` and the stub are what
  an agent gets, which is enough to know not to hand-edit it.
- **No parser** (14-H). The image line, the seeded table container, `.group` and the stub classes
  are all pinned by exact-byte tests so the parser mirrors a specification and not an intention.

## How it was verified

- `crates/docsai-docmark/tests/presentation_objects.rs`, thirteen tests: the picture at `full`,
  `standard` and `agent`; the docx counter-case for decision 3; the table with the shape's id and
  the bare table at `standard`; the empty table; the group and the four ids in walk order; the
  group at `plain`; the chart with `kind=`, `data=` and its title, at `standard` and at `plain`;
  determinism over a slide holding all three.
- One test of [[50-phase-14-containers]] changed meaning: what asserted a SmartArt was *warned and
  not written* now asserts the stub, at `full` and at `standard`. That is the increment.
- Real fixtures at `full`: `images-anchored` writes the image line with `pos=` and `width=`,
  `tables-simple` the GFM table inside `::: {#n2 .table col-widths=… pos=… size=…}`,
  `charts-embedded` `::: {#n2 .chart kind=barChart … raw=raw-0001}`, and `smartart-fallback` the
  stub with the diagram's own text — `Planificar · Ejecutar · Revisar` — inside it.
- `cargo test --workspace` 38 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## Next

**14-G — `plain`, and the degradation rule as a test**: a slide at `plain` is heading, bullets and
images and nothing else, verified as clean CommonMark with `comrak`, and the P2 residue probe
becomes a repository test rather than a scratchpad binary.
