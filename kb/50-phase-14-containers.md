---
tags:
    - phase-14
    - docmark
    - presentations
    - serializer
    - geometry
---
# 50 — Phase 14 D: the containers a slide needs

Increment **14-D** of [[46-phase-14-plan]], on top of the slide body of [[49-phase-14-slide]].
Spec §11.2 rule 4: what the layout does not make implicit gets a container, because Markdown has
no shape.

## What changed

- `docsai-docmark::deck_writer::write_shape` (new): every non-implicit shape becomes
  `::: {.ph idx=…}`, `::: {.shape geom=…}` or `::: {.connector …}`, with its content inside.
- `docsai-docmark::writer`: `Writer::dict` and `Writer::drawing_length` — the two things the deck
  writer needs to render an attribute block and a length, and nothing more.
- `docsai-docmark::frontmatter`: a **selection** of a deck now declares `docmark: "1.2"`. Its body
  carries `.slide` and containers; the version names the profile, not the ids (spec §11.2 version
  rule). Found by reading a real fixture through `docsai read --select`, not by a test.
- `docs/docmark-specification.md` §11.2: a **shape container attributes** table, plus the
  furniture rule and what `plain` does.

## Non-obvious decisions

1. **`geom=` is identity, not measurement, so it survives `standard`.** Rule 6 takes geometry away
   from `standard`, but `a:prstGeom@prst` is the only thing that says a box is an arrow rather than
   a rectangle. A reader who is told «a shape» and not «an arrow» has been told less than the
   document knows, and it costs one word. `pos=`, `size=`, `rotation=` and `flip=` do go.
2. **`idx=` is for the writer, `type=` is for the reader.** `p:ph@idx` matches a shape to its
   layout placeholder — useless to a level that never writes back — so it follows
   `Fidelity::addresses()`. `type=` stays at `standard`: a footer and a chart slot are different
   boxes. It is omitted for `body`, which is the PresentationML default, so the common container
   stays `::: {.ph idx=2}`.
3. **Slide furniture is dropped where the document does not write back.** `PhType::is_furniture`
   already says why in the model: slide numbers, dates, footers and headers are inherited from the
   layout and hold nothing an author wrote. At `standard` they would be a container on every single
   slide — exactly the noise the P4 hand-edit gate is about. The drop is a typed warning per shape.
4. **The id is taken before the body is rendered.** `for_each_addressable` visits a shape and
   *then* its blocks; a writer that allocated them the other way round would hand out ids the walk
   cannot find again. `a_shape_takes_its_id_before_what_it_holds` is that assertion.
5. **`plain` writes no containers, and says when a shape leaves nothing behind.** A `:::` fence is
   literal text to a CommonMark viewer, which is the whole point of 14-G. The text a shape holds
   still reaches the reader in slide order; a shape with no text at all is warned, because that one
   *is* a loss.
6. **A `standard` stub keeps its container and loses its `raw=`.** Rule 8 says the stub is visible
   at every level; rule 6 says `standard` carries no raw payload. Both hold: the box stays, the
   pointer goes, and a `RawBlockDropped` warning says so — the same shape `render_raw` already has
   for text documents.
7. **An empty shape is an empty container**, `::: {…}\n:::` on two lines. A box the author placed
   and left empty is still a box, and the two-line form is what 14-I's idempotence test can write
   back unchanged.

## What is deliberately not written

- **Pictures, tables, charts and groups** inside a slide: 14-F wires the existing image and table
  writers in. SmartArt, OLE and media stay `RawShapeKind` stubs until then. All five warn.
- **Speaker notes** (14-E) and **slide-level raw fragments**, unchanged from 14-C.
- **No parser.** 14-H is the mirror; nothing reads a container back yet.

## How it was verified

- `crates/docsai-docmark/tests/presentation_shapes.rs`, fourteen tests: the exact bytes of the
  spec's own arrow at `full`; `standard` keeping the stub and dropping the measurements; `agent`
  keeping the geometry because it writes back; the connector; the text box; rotation and flip; the
  placeholder index and type at both levels; furniture kept at `full` and warned at `standard`;
  `plain` with no fences; the id order; the still-warned SmartArt; determinism.
- Two tests of [[49-phase-14-slide]] changed meaning and were rewritten: what they asserted was
  *not written* is now a container. That is the increment, not a regression.
- Real fixture: `docsai read --select '#n1' corpus/pptx/shapes-geometry.pptx` writes the round
  rectangle, the arrow and the connector with `pos="88px,2000000emu" size="2000000emu,2.5cm"` —
  readable units where they are exact, `emu` where they are not, and `0` with no unit at all.
- `cargo test --workspace` 36 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## Next

**14-E — notes**: `::: {.notes}` at `full`/`agent` and a **blockquote** at `standard`, the one node
whose syntax differs between levels, which the parser of 14-H must read both ways.
