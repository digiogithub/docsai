---
tags:
    - phase-14
    - docmark
    - presentations
    - serializer
    - fidelity
---
# 51 — Phase 14 E: notes, and the one node with two syntaxes

Increment **14-E** of [[46-phase-14-plan]], after the containers of [[50-phase-14-containers]].
Spec §11.2 rule 5, which spike P2 measured: a container where the document writes back, a
blockquote where a human has to read it.

## What changed

- `docsai-docmark::deck_writer::write_notes` (new): `::: {.notes}` at `full` and `agent`, a
  CommonMark blockquote at `standard`, nothing plus a warning at `plain`.
- `docsai-docmark::deck_writer::blockquote` (new): `> ` per line, a bare `>` for the blank lines
  inside.
- `docs/docmark-specification.md` §11.2: a **speaker notes** subsection — the table of forms and
  the empty-notes rule.

## Non-obvious decisions

1. **The syntax depends on the level, and that is deliberate.** Every other node in DocMark writes
   the same way at every level and only sheds attributes. Notes do not, because rule 5 is a
   readability decision: at `standard` a deck must survive being hand-edited by someone who has not
   read this spec ([[46-phase-14-plan]], risk P4), and `::: {.notes}` is syntax they would have to
   learn. A blockquote is the one Markdown construct PresentationML cannot collide with — there is
   no blockquote a placeholder could occupy — so the mapping stays unambiguous in both directions.
   The cost lands on the parser of 14-H, which must read both, and that cost is written down here
   rather than discovered there.
2. **The `.notes` container takes no id.** What is addressable inside notes is the blocks they
   hold, exactly as on a slide; `for_each_addressable` already walks them that way, and a container
   that took an id the walk does not hand out would break `serialize(parse(md)) == md` at the first
   round trip.
3. **`Some(vec![])` and `None` stay different.** The model says so in a doc comment and now the
   output says so too: a deck with an empty notes page writes an empty `::: {.notes}`, a deck with
   no notes page writes nothing. The pptx writer of Phase 15 is the reader of that difference —
   one deck has a `notesSlide` part and the other does not.
4. **An empty notes page writes nothing at `standard`.** A lone `>` is noise, and `standard` never
   goes back into a package, so the distinction of decision 3 buys nothing there.
5. **`plain` drops the notes and warns.** A notes page is not what the slide shows, which is what
   `plain` means; the drop is typed, so §7 rule 3 holds. It is the only level where notes are a
   loss.
6. **Notes are written after the shapes**, which is both the spec's example order and the order
   `for_each_addressable` visits them in — so the ids inside the notes are the ones the walk
   expects.

## What is deliberately not written

- **Pictures, tables, charts and groups** on a slide: 14-F. Notes holding an image go through the
  same block renderer, so they inherit whatever 14-F wires in.
- **No parser** yet (14-H). Both syntaxes are pinned by exact-byte tests here so that the parser
  has a specification to mirror rather than an intention.

## How it was verified

- `crates/docsai-docmark/tests/presentation_notes.rs`, nine tests: the exact bytes at `full`,
  `standard`, `agent` and `plain`; the empty notes page at both syntaxes; the slide without one;
  multi-block notes, where the blank line between a paragraph and a list stays inside the quote as
  a bare `>`; the ids the walk expects; determinism at both syntaxes.
- One test of [[49-phase-14-slide]] lost its subject — notes are written now — and was re-pointed
  at the slide-level raw fragments (`p:transition`, `p:timing`), which are still a warning.
- Real fixtures: `docsai read --select '#n3' corpus/pptx/notes-crossed.pptx` writes each slide's
  own notes under it, which is the bug class that fixture exists for.
- `cargo test --workspace` 37 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## Next

**14-F — pictures, tables and the shape stub**: wiring the existing image and table writers into a
slide, with no image geometry at `standard`, and SmartArt, OLE, charts and custom geometry as
visible stubs over the Phase 13 sidecar ([[43-phase-13-raw]]). Done, in [[52-phase-14-objects]].
