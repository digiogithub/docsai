---
tags:
    - phase-14
    - docmark
    - presentations
    - parser
---
# 54 — Phase 14 H: the deck parser

Increment **14-H** of [[46-phase-14-plan]], after the measurement of [[53-phase-14-plain]]. The
serialiser was finished in 14-F and proven in 14-G; nothing read it back. This increment is the
mirror: DocMark-P → `Presentation`.

## What changed

- `docsai-docmark::deck_parser` (new): `looks_like_deck` and `parse_deck`, dispatched from
  `parser::parse_with_base` *before* the workbook check, the same shape `sheet_parser` already had.
- `docsai-docmark::parser`: `BodyParser` gains a `new` constructor and is `pub(crate)`, together
  with the block, table, image, paragraph and chunk helpers the deck parser reuses; `find_asset`
  is extracted from `load_image`; `split_trailing_attrs` gains a tight variant for image lines;
  `paragraph_format_from_attrs` learns the 1.2 structural classes.
- `docsai-docmark/tests/presentation_parse.rs` (new, 21 tests) and
  `docsai-convert/tests/deck_parse_corpus.rs` (new, 4 tests).
- `docs/docmark-specification.md` §11.2: a «Reading a deck back» section — what marks a file a
  deck, what tolerant input means, and `raw=` as the `.shape` discriminator.

## Non-obvious decisions

1. **The deck parser reuses `BodyParser` rather than owning a second one.** A slide's content is
   ordinary Markdown — lists, tables, images, inline styling, the attribute dictionary — and
   `sheet_parser` got away with its own reader only because a cell is not a block. Two block
   parsers for one format is how two readers of the same file start disagreeing. What the deck
   parser adds is the *slide* layer: splitting at headings and dispatching containers.
2. **What marks a file a deck is not `##`.** `source-format: pptx`, a `layouts:` or `skeleton:`
   key, or a `.slide` marker — and nothing else. Guessing from heading levels would turn every
   report into a presentation. The consequence is stated rather than hidden: a `plain` deck writes
   no front matter and no marker, so it reads back as a **text document**. That is what a one-way
   projection is (spec §6), and pretending otherwise would mean inventing slides from `##`.
3. **Tolerance is what the format promises, not a fallback.** A deck typed by hand carries no
   attributes: `#` or `##` opens a slide, its blocks are the implicit body, content before the
   first heading is a slide with no title, and an unknown container class keeps its text as a text
   box **with a warning**. Tolerance without the warning would be the silent degradation §7 rule 3
   forbids, so every guess is typed.
4. **`.shape` is disambiguated by `raw=`, and the corpus is what decided it.** The first version
   read `geom=` as «this is a stub», and the corpus test found `shapes-geometry`: two text boxes
   with a preset outline came back as opaque objects. A stub is a marker over markup only the
   package can reproduce; a rounded box with text in it is content. At `standard`, where `raw=` is
   not written, a stub does read back as a text box — that level does not write back, so it spends
   nothing it was going to use.
5. **The implicit body takes the `idx` its layout gives it.** Not decoration: `implicit_shapes`
   matches the primary body by index, so a placeholder parsed without one would be written back as
   a `::: {.ph}` container and the document would grow a box on every round trip. The catalogue
   answers it, exactly as it does on the writing side.
6. **A heading with no title reads as a slide without a title placeholder.** Rule 1 writes `##`
   both for an empty title placeholder and for a missing one; the two are the same line, so only
   one of them can come back. The empty box is what the skeleton restores in Phase 15, and choosing
   the other way would break byte idempotence in 14-I.
7. **The picture's attribute block needs a *tight* split.** `split_trailing_attrs` requires
   whitespace before the `{`, which is what keeps `[text]{.underline}` from being read as a
   paragraph's attributes — and an image line has none. Rather than a second scanner, the existing
   one took a parameter.
8. **A parse error named a line one short of the truth.** `split_front_matter` skipped the blank
   line after the closing fence without counting it, so every line number in a body error was off
   by one. Found by writing the first test that asserts a line number; fixed there, which moves
   every body error of every document class by one.
9. **The corpus test asks for the shape *kind*, not only the count.** A count alone passes with
   every shape read as a text box. Over the seventeen decks the kinds match exactly at `full`,
   which is a stronger claim than any hand-built deck could make.

## What is deliberately not written

- **No goldens and no byte idempotence over the corpus** (14-I). The round trips here are
  `serialize(parse(md)) == md` on hand-built decks and structural equality on the corpus; the
  `<name>.expected.md` files and the corpus-wide byte check are the next increment.
- **No CLI path** (14-J): `docsai convert deck.pptx -o deck.dmk.md` still refuses, so the tests
  drive `read_pptx`, `serialize` and `parse` directly.
- **No deck-level raw fragments.** The writer references `raw=rN` but writes no sidecar for a deck,
  so `Presentation::raw` comes back empty. That is the writer's state, unchanged here.

## How it was verified

- `presentation_parse.rs`, 21 tests: the slide heading and its attributes, the implicit body's
  index, every container of rules 4 and 8, pictures/tables/groups, notes in both syntaxes, the
  empty notes page, tolerant input, headings inside containers, the unknown-class warning, the
  unclosed-container error with its line, the missing skeleton, and round trips at `full`, `agent`
  and `standard`.
- `deck_parse_corpus.rs`, 4 tests over the seventeen decks: slides, shapes and titles at `full`,
  the kind of every shape, `standard` without addresses, `agent` with unique ones.
- `cargo test --workspace` 41 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## Next

**14-I — goldens and idempotence**: `<name>.expected.md` beside every pptx fixture at the levels
the docx goldens use, and `serialize(parse(md)) == md` byte for byte over all of them.
