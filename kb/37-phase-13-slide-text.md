---
created_at: 2026-08-07T21:25:23.630430302Z
updated_at: 2026-08-07T21:25:40.09525893Z
tags:
    - phase-13
    - pptx
    - presentations
    - reader
    - drawingml
    - text
---
# 37 — Phase 13 C: slide text

Increment **13-C** of [[34-phase-13-plan]], on top of [[36-phase-13-package-layer]]. Slides stop
being empty: `p:spTree` becomes shapes and `p:txBody` becomes blocks.

## What changed

- `crates/docsai-office/src/pptx/text.rs` (new): `p:txBody` to `Vec<Block>`. `a:p`/`a:r` with
  `a:rPr` into the existing `FontProps`/`RunProps`, `a:br`, `a:fld`, `a:hlinkClick`, and `a:pPr`
  into `ParaProps`.
- `pptx/mod.rs`: `read_shapes` walks the slide's `p:spTree` in source order, producing
  `ShapeKind::Placeholder` for a `p:sp` with `p:ph` and `ShapeKind::TextBox` otherwise, each with
  its `p:cNvPr@name`, its `a:xfrm` geometry and its `z_index`.

## Non-obvious decisions

1. **A body placeholder bullets by inheritance.** The corpus master's `p:bodyStyle` carries
   `a:buChar` for all four levels and the slide states nothing; a reader that only honoured
   explicit bullets would flatten every deck into paragraphs and lose exactly the structure
   [[32-phase-12-spike-p2]] measured DocMark-P against («eight clean bullets»). The rule:
   `a:buNone` wins, then explicit `a:buAutoNum`/`a:buChar`, then `lvl > 0`, then the shape's own
   default — bulleted for a body-like placeholder, plain for a title or a free text box.
2. **An empty paragraph is content and never a bullet.** It holds its place in the box, so it
   stays a `Block::Paragraph`, and a bullet with nothing after it is not what the slide shows.
   Consequence, deliberate: a blank line between bullets splits one list into two, which is what
   a reader sees.
3. **`+mj-lt` is not a font name.** `a:latin@typeface` starting with `+` is a theme reference;
   storing it as a typeface would write a font nobody has installed. Left for the cascade
   (13-D), which is also where `a:schemeClr` is resolved — a fill this reader cannot classify at
   all is reported instead.
4. **A slide number is `FieldKind::Page`**, and the DrawingML `type` travels verbatim as the
   field's `instruction`, so the writer puts back `slidenum` and not a Word instruction that
   means roughly the same thing.
5. **Unread shape kinds warn.** `p:pic`, `p:graphicFrame`, `p:grpSp`, `p:cxnSp` and
   `mc:AlternateContent` are later increments; each emits `Warning::UnsupportedElement` naming
   what was skipped. Silence would make a slide that lost its table look like a slide without
   one. The constant listing them is in the reader so the increment that reads a kind has to
   remove its line.
6. **`a:normAutofit@fontScale` is read now**, into `Placeholder::delta.font_scale`. It is a fact
   about the shape rather than about the cascade, and 13-I needs the number to fire
   `Warning::AutofitStale` on.
7. **Source order, not reading order.** 13-G owns the policy; every shape already carries the
   `z_index` that makes it reversible.

## How it was verified

- Eight unit tests in `text.rs` (bullet resolution, nesting, `buNone`, empty paragraphs, run
  properties, theme fonts, fields, paragraph units) and five new corpus tests in `mod.rs`.
- The corpus sweep now asserts every deck yields shapes and **no `Warning::Degraded`**;
  unsupported-element warnings are expected and are what the later increments retire.
- `cargo test --workspace`, `clippy --all-targets -- -D warnings`, `fmt --check` green.

## Next

13-D: the cascade as reference + delta — shape to layout placeholder to master to theme,
`clrMap`/`schemeClr`/`+mj-lt`, resolved for colour, font and size, with only the delta stored.
