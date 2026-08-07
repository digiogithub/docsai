---
created_at: 2026-08-07T22:13:02.868415974Z
updated_at: 2026-08-07T22:13:24.784105268Z
tags:
    - phase-13
    - pptx
    - presentations
    - reader
    - drawingml
    - cascade
    - theme
---
# 38 — Phase 13 D: the cascade as reference + delta

Increment **13-D** of [[34-phase-13-plan]], on top of [[37-phase-13-slide-text]]. A slide stops
storing what it inherits.

## What changed

- `crates/docsai-office/src/pptx/cascade.rs` (new): `Theme` (colour scheme, font scheme and the
  master's `p:clrMap`), `LevelStyles` (run properties per outline level, which is how every
  PresentationML list style is written), and `Cascade`, which holds every master's `p:txStyles`
  and every layout's placeholder list styles, keyed by part name.
- `pptx/text.rs`: `font_props` resolves theme references and every run subtracts what its
  outline level already inherits.
- `pptx/mod.rs`: masters, then layouts, then slides — the read order is now load-bearing. A
  layout and master placeholder stores the **resolved** properties; a slide placeholder stores
  the **delta**.

## Non-obvious decisions

1. **Resolve, then subtract — in that order.** `#000000` and `a:schemeClr val="tx1"` are the
   same colour and only one of them is a fact. Comparing before resolving would make every
   fully-inheriting slide look like it overrode everything, which is precisely the
   double-emitted-properties failure [[11-plan-v2-onramp]] names.
2. **The `p:clrMap` is read, not assumed.** A master names colours by slot (`tx1`) and the theme
   defines them by scheme name (`dk1`). They coincide in the default mapping and stop coinciding
   the moment a deck inverts a master — the case where assuming the identity map silently
   inverts every colour in the deck.
3. **`+mj-lt` resolves to a font name, and the reference does not survive on the run.** Storing
   `+mj-lt` as a typeface would ask for a font nobody has installed. The reference is not lost:
   it is on the master, in the skeleton (13-H), and on the layout placeholder as the resolved
   value the writer compares against.
4. **A layout placeholder stores the resolved cascade; a slide placeholder stores the delta.**
   Two different jobs for the same `ShapeProps`: the model's own doc comment says a
   `LayoutPlaceholder` carries «the properties slides inherit from it», and a delta measured
   against nothing is just a copy.
5. **The shape's own `a:lstStyle` is both a delta and a reference.** It sits between the layout
   and the runs, so it is stored as the placeholder's delta *and* layered onto what its runs
   inherit. Reading it only as one of the two would double-count it.
6. **A colour transform is a loss and says so.** `a:lumMod`/`a:alpha`/`a:tint` shift the slot's
   colour; the base colour travels, the shade does not, and that emits `Warning::Degraded`
   rather than passing for the real thing (`AGENTS.md` §7 rule 3).
7. **Only colour, font and size are what the cascade decides**, and they travel through the
   shared `FontProps`, so every property an `a:defRPr` states is carried with them. Paragraph
   properties are still read as direct formatting: no corpus deck states one twice, and
   inventing a second subtraction path without a case to test it would be guesswork.
8. **A slide's layout is loaded before the slide, even when no master declares it.** Reading the
   text without its reference would store inherited properties as deltas — silently, and with
   values that look right.

## How it was verified

- Four unit tests in `cascade.rs` (the `clrMap` indirection, font references, layering a layout
  over a master's text style, and a slot with no layout inheriting nothing) and two in `text.rs`
  (references resolve; what a run inherits is not stored, what it changes is).
- Two corpus tests on `placeholders-cascade.pptx` ([[30-phase-12-pptx-corpus]]): **every
  placeholder stores zero properties and zero geometry**, with no `Warning::Degraded`; and the
  layout carries the answers — `Calibri Light` at 44 pt in `#000000` for the title, `Calibri` at
  28 pt for the body — with the master saying the same thing, because that is where it is
  written.
- `cargo test --workspace`, `clippy --all-targets -- -D warnings`, `fmt --check`,
  `corpus/generate.py --check` green.

## Known gaps, written down rather than left implicit

- No corpus deck states a property the cascade also states, so the subtraction itself is only
  covered by unit tests on inline XML. A fixture whose slide overrides one level's size would
  make it a corpus test; that means extending `build_pptx`, which is Phase 12 territory.
- `p:defaultTextStyle` is read but no corpus deck carries one.
- A free text box inherits `p:defaultTextStyle` over the master's `p:otherStyle`. PowerPoint's
  own rule is narrower; nothing in the corpus distinguishes them.

## Next

13-E: pictures and tables — `p:pic` to `ImageRef` through the `AssetStore`, `p:graphicFrame`
with `a:tbl` to the IR `Table`. Both currently emit the unsupported-element warning that
increment removes.
