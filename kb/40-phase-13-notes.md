# 40 — Phase 13 F: speaker notes

Increment **13-F** of [[34-phase-13-plan]], on top of [[39-phase-13-pictures-tables]]. The
densest part of a deck in intent, and the cheapest to read — provided it is read through the
right door.

## What changed

- `crates/docsai-office/src/pptx/notes.rs` (new): the slide's `notesSlide` relationship into
  `Slide::notes`.
- `pptx/mod.rs`: `read_slide` takes the `ContentTypes` (a notes part is checked like every other
  part) and fills `notes` instead of leaving it `None`.
- `corpus/generate.py`: new `notes-crossed.pptx`, plus a `notes_for` argument to `build_pptx` so
  a fixture can bind a slide to a notes part that does not share its number.

## Non-obvious decisions

1. **The relationship binds the note, not the number.** `notesSlide7.xml` belonging to slide 7 is
   PowerPoint's habit. The reader follows `slide7.xml.rels`, exactly as slide order comes from
   `p:sldIdLst` and never from part names ([[30-phase-12-pptx-corpus]] makes the same point with
   `slide-order.pptx`). Matching by number is the worst class of bug this project can ship: it
   loses nothing, warns about nothing, and puts real text under the wrong slide.
2. **`notes-crossed.pptx` exists because no other deck could prove it.** `notes-speaker.pptx` has
   agreeing numbers, so it passes under either rule. A fixture that disagrees is the only test
   that distinguishes them.
3. **The notes page's furniture is not a note.** A notes slide carries a `sldImg` placeholder
   (the thumbnail of the slide), and may carry `hdr`, `ftr`, `dt` and `sldNum`. All of it is
   regenerated from the notes master on every render; reading it would put "1" or a header string
   into the speaker's notes. Only body placeholders — and a free text box, which is authored
   content — are read.
4. **Notes are prose, not bullets.** What would bullet them is the notes master's `p:txStyles`,
   which is a separate cascade root this reader does not load. Bulleting them anyway would invent
   a list in the DocMark the deck never had; `bulleted: false` states the absence rather than
   guessing at it.
5. **`None` and `Some([])` are different facts.** No notes part at all versus a notes page that
   is empty. The model already says so (`Slide::notes`); the reader now honours it, so a writer
   neither invents a part nor deletes one the deck declares.
6. **The theme comes from the slide's layout, not the notes master.** The notes master carries
   its own theme relationship — in every deck seen so far, the same `theme1.xml`. Loading a second
   cascade root to resolve `a:schemeClr` in two lines of notes is not worth it *yet*; the layout
   theme is used, and a deck that themes its notes differently would resolve a note's colour
   against the slide's palette. Written down here rather than left to be discovered.
7. **A picture or table on a notes page is reported, not read.** `Slide::notes` is `Vec<Block>`,
   which can hold a table — but nothing in the corpus has one, and reading a shape kind blind is
   how the reader acquires an untested path. It warns `UnsupportedElement`, per `AGENTS.md` §7
   rule 3.

## How it was verified

- `notes-speaker.pptx`: both slides get their notes, two paragraphs for the first and one for the
  second, all `Block::Paragraph` (no list invented), and **zero warnings** — the `sldImg`
  placeholder is furniture, not a loss.
- `notes-crossed.pptx`: slide 1 gets the note from `notesSlide2.xml` and slide 2 the note from
  `notesSlide1.xml`, because that is what the relationships say.
- `basic-slides.pptx`: every slide reads `notes: None`.
- `cargo test --workspace` (34 pptx tests), `clippy --all-targets -- -D warnings`, `fmt --check`,
  `corpus/generate.py --check` green.

## Known gaps, written down rather than left implicit

- The notes master (`ppt/notesMasters/*`) is not read at all: not its placeholder geometry, not
  its `p:txStyles`, not its own theme. The notes page's layout is regenerated, and 13-H's
  skeleton is what preserves the part itself.
- `p:notesSz` (the printed notes page size) is read by nobody; the IR has no field for it.
- A notes slide's `p:sp` with no placeholder is read as content. No corpus deck has one, so that
  branch is covered by the code path, not by a fixture.

## Next

13-G: deterministic, reversible reading order — placeholders first by type, then the remaining
shapes by top-left, with the original `spTree` index travelling as data on every shape.
