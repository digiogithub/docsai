---
tags:
    - phase-14
    - docmark
    - presentations
    - serializer
---
# 49 — Phase 14 C: the slide, its heading and the implicit body

Increment **14-C** of [[46-phase-14-plan]], on top of the front matter of
[[48-phase-14-deck-frontmatter]]. The first increment where a deck has a body at all.

## What changed

- `docsai-docmark::deck_writer` (new): `write_presentation` renders every slide as
  `## Title {#n1 .slide layout=L1}` plus the primary body placeholder as ordinary blocks.
- `docsai-docmark::writer`: `Writer` gains `render_slide_heading`, `render_slide_blocks`, `ids`,
  `report_mut` and `into_report` — the deck writer owns the *shape* of the output and borrows the
  block renderer.
- `docsai-docmark::attrs`: `Attrs::merge`, so a caller that knows what a node *is* can fold its
  attributes over the paragraph's own.
- `docsai-docmark::lib`: `Document::Presentation` no longer produces an empty body and a
  «not implemented» warning; it calls the deck writer.
- `docs/docmark-specification.md` §11.2: a **slide attributes** table (`#id`, `.slide`, `layout=`,
  `section=`, `hidden=`, `name=`) and the multi-paragraph title rule.

## Non-obvious decisions

1. **The writer asks `implicit_shapes`, it does not re-derive it.** Phase 12 put that function in
   `docsai-model::addressing::walk` with a doc comment saying the Phase 14 serializer must ask it,
   precisely so that «what is implicit» and «what is addressable» cannot disagree. Title and body
   are told apart *inside* the returned indices (`is_title`), not by position, so a slide with only
   a body is still read correctly.
2. **The slide's id goes on the heading, and the two implicit shapes take none.** There is nowhere
   to write theirs, and an id that cannot be written changes on every round trip
   ([[35-phase-13-ir-root]] states it for the IR). `next-id` therefore
   counts the slide and the nodes inside the body, not the placeholders.
3. **The id policy stays the caller's.** `standard` is documented as carrying no ids, but the
   serializer does not force `IdPolicy::Never`: the pipeline derives it from the level
   (`ConvertOptions::id_policy`), and a serializer that overrode it would take that choice away
   from `--ids`. The deck follows the rule text documents already follow.
4. **`section=` is written on every slide of the section**, not once at its start. A section marker
   that only appears on the first slide is state a reader has to carry, and it breaks the one thing
   this format promises about partial reads: a slide selected on its own must still say where it
   belongs.
5. **`name=` is gated on `Fidelity::addresses()`, `section=` is not.** `p:cSld@name` is what the
   writer puts back; a reader gains nothing from it. A section is structure a human wants at
   `standard`, and it costs one attribute.
6. **A multi-paragraph title is joined into the heading with a warning.** A heading is one line.
   Dropping the second paragraph would lose text, and writing it under the heading would move it
   into the body placeholder on the way back — so the text survives, the paragraph break does not,
   and the loss is typed rather than silent (§7 rule 3).
7. **An empty title writes `##` with no trailing space.** The obvious `format!("## {body}")` leaves
   `"## "`, which no parser writes back the same way; the idempotence test of 14-I would have found
   it, and finding it here was cheaper.

## What is deliberately not written

- **Every shape that is not the implicit title or body**: other placeholders, free shapes,
  connectors, pictures, tables, groups and stubs. Each emits `Warning::UnsupportedElement` naming
  the shape and the slide. That is 14-D and 14-F, and the warning is what keeps the gap honest
  until then.
- **Speaker notes** (14-E) and **slide-level raw fragments** (`p:transition`, `p:timing`), the
  latter as `Warning::RawBlockDropped`.
- **No parser.** Nothing reads a `.slide` heading back yet — that is 14-H — so no round-trip
  assertion exists at this increment, by design.

## How it was verified

- `crates/docsai-docmark/tests/presentation_slides.rs`, twelve tests: the exact bytes of a slide at
  `full`, `standard` and `plain`; the id on the heading and nowhere else; the titleless and the
  empty-title slide; `the_layout_decides_which_body_is_implicit` (a layout whose body is index 2,
  the case a «first body wins» heuristic gets wrong); section, `hidden` and `name`; slide
  separation and `stats.slides`; the warnings for what is not written; the joined title;
  determinism.
- Smoke test on a real deck: `docsai tokens corpus/pptx/basic-slides.pptx` reports 233 tokens over
  4 addressed nodes, two of them slides — the first time a pptx fixture has produced DocMark.
- `cargo test --workspace` 35 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## Next

**14-D — the containers**: `::: {.ph idx=…}`, `::: {.shape geom=…}`, `::: {.connector …}`, their
geometry in readable units with `emu` as the exact fallback, and `raw=` pointing at the sidecar.
