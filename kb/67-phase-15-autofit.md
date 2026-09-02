---
tags:
    - phase-15
    - pptx
    - presentations
    - writer
    - autofit
    - warnings
---
# 67 — Phase 15 F: the stale autofit scale on the way out

Increment **15-F** of [[61-phase-15-plan]], on top of [[66-phase-15-notes]]. The smallest increment
of the phase and the only one whose whole content is a *deliberate loss*: everything else in this
writer exists to preserve, and this one exists to throw a number away and say so.

Analysis §5.4, restated: `a:normAutofit@fontScale` is a number PowerPoint computed for the text that
was in the box the last time a **human** edited it. PowerPoint does not recompute it when the file
changes underneath, and no reader can tell that it is out of date. So a deck whose body was retyped
through DocMark and written back with its old scale renders the new text at 62.5 % for no reason
anybody can see. Computing the right scale needs text measurement and font metrics this binary does
not carry (analysis §5.4 puts that in the post-2.0 backlog), so the honest answer is to drop the
number, let the application recompute it, and name the shape in a warning.

## What changed

- `pptx/write.rs`: `drop_stale_autofit`, a `stale` field on `Rewritten`, the `source[range] !=
  written` comparison in `rewrite_slide` that decides whether a body changed, the same three lines
  in `rewrite_notes` (whose `Ok` type is now `Vec<(String, u32)>` rather than `()`), and the two
  places in `rebuild_slides` that turn those into `Warning::AutofitStale`.
- `pptx/mod.rs`: the reader's `what` gains one word, `carries` — see decision 3.
- `docsai-model/src/report.rs`: `Warning::AutofitStale`'s message and both doc comments. The variant
  is unchanged in shape.
- `docsai-convert/src/lib.rs`: the `SUPPORT` note. `corpus/pptx/autofit-stale.expected.inspect.json`
  regenerated for the one word.

## Non-obvious decisions

1. **«The shape's text changed» is not a new notion of change; it is the one the writer already
   had.** 15-B's rule is that nothing is written unless it differs from what the part already says,
   and the writer states that as a byte comparison at the moment of the splice: the `a:p` sequence
   it just built against the `a:p` sequence the part carries at that exact range. So «changed» here
   is `source[paragraph_range(tx_body)] != written`, and nothing else. Inventing a second test —
   comparing the model against a re-read of the model, say, or trusting that an edit reached the
   writer at all — would have produced a rule that disagreed with the one deciding whether the part
   is rewritten, and the two disagreeing is exactly how a false warning is born. It also means the
   guard is free: the corpus-wide byte-identity tests already prove that an untouched deck produces
   no such difference anywhere, so they prove no scale is dropped either.

2. **The edit is on `a:bodyPr`, and `splice` drops an overlapping edit without a word.** The
   attribute sits inside the same `p:txBody` whose paragraphs are being replaced, which is the one
   real hazard of the increment. It is safe because `a:bodyPr` precedes the first `a:p` and
   `paragraph_range` starts at that first `a:p` — and when there are no paragraphs the range is the
   empty insertion point *after* the last child, which is at or past the end of the attribute's
   span. Two `set_attr` removals on the same start tag do not overlap each other either: each
   covers its own name, quotes and preceding space. `dropping_the_scale_does_not_overlap_the_
   paragraphs_it_was_computed_for` asserts the outcome rather than the reasoning — both edits in
   the file, not one — because silence is the failure mode.

3. **One variant, two sides, and the message had to stop asserting what the reader does with the
   number.** The old message read «*X* scales its text to 62.5 %, a number PowerPoint computed…»,
   which on the way out would have described a scale this writer had just removed. That is the
   false warning of §7 rule 3 in miniature. Rather than adding a second variant for what is plainly
   the same fact, `what` became a clause the message completes — `… carries` on the way in, `the
   text of X on Y changed, so the writer dropped` on the way out — and the message states only the
   number and where it came from. It costs the reader one word and one regenerated inspection
   golden, and it keeps one warning meaning one thing.

4. **`lnSpcReduction` goes with `fontScale`, and never on its own.** It is the other half of the
   same computation, and leaving the paragraphs squeezed for text that has gone is the same lie in
   a different attribute. But a `lnSpcReduction` stated with **no** `fontScale` is left exactly
   where it is: the IR does not model it, the reader never warned about it, and the shape states no
   scale for this increment to call stale. Leaving something is not a loss, so it needs no warning
   — which is the same rule 15-E used for a slide that states no section.

5. **The notes page is a text body like any other.** `rewrite_notes` runs the same comparison and
   the same drop; a note retyped under a scale takes it with it. The alternative — restricting the
   increment to `p:spTree` shapes because that is where the *reader* fires the warning — would have
   left a page whose text changed carrying a number computed for text that is gone, for no better
   reason than the reader having a blind spot there. The warning names the notes part, since a note
   has no shape name a caller would recognise.

6. **The warning names the shape the way the *part* names it.** `Shape::name` survives a read but
   not a DocMark round trip, and the round trip is the only way an edit actually reaches this
   writer through the pipeline — so the first version of the pipeline test reported «`shape 1`»,
   which is a z-index and not something a caller can go and look at. The fallback reads
   `p:cNvPr@name` off the element being spliced, which is `Content Placeholder 2`, the name
   PowerPoint shows in the selection pane.

## Known losses, written down rather than discovered later

- **The model's `font_scale` is still never written back.** `ShapeProps::font_scale` is read into
  the IR and travels through DocMark, and this writer neither writes it onto a shape that lacks one
  nor updates one that differs: `a:bodyPr` is preserved bytes except for this removal. A caller who
  edits the number in the Markdown does not change the file. That is deliberate — the number is
  PowerPoint's to compute, not a caller's to state — but it is a silent no-op and it is written
  down here rather than found later.
- **A scale on a shape whose text did not change stays, and stays stale in the older sense.** If a
  human shortened the text in PowerPoint and never re-entered the box, the file's scale was already
  wrong when it was read. The reader says so on the way in; the writer has no basis to act, since
  nothing in this conversion changed that body.
- **A `lnSpcReduction` with no `fontScale` is untouched** (decision 4).
- **Nothing recomputes anything.** After the drop, PowerPoint recalculates the scale the first time
  the box is edited by hand; LibreOffice and other consumers lay the text out at 100 % in the
  meantime, which may overflow the box. That is analysis §5.4's accepted v1 outcome — a box that
  overflows visibly beats a box that is silently wrong — and real measurement stays post-2.0.
- **The layout's and master's autofit are not consulted.** A body that inherits `normAutofit` from
  its layout and states none of its own has no attribute on the slide to drop; the inherited one is
  not a number computed for *this* shape's text, so it is not this increment's.

## How it was verified

- Four new tests in `pptx::write` (155 in the crate, up from 151):
  `a_changed_body_drops_the_scale_that_was_computed_for_its_old_text` (the attribute and its
  companion gone, `<a:normAutofit/>` left standing, the new text in the part, the *title* shape
  still the bytes that were read, and the deck read back stating no `font_scale` at all),
  `dropping_the_scale_does_not_overlap_the_paragraphs_it_was_computed_for` (decision 2),
  `a_changed_body_with_no_scale_produces_no_edit_and_no_warning`, and
  `a_scale_whose_own_text_did_not_change_is_left_alone` — the false-warning trap, over the same
  deck, editing the *other* shape: the body's `<a:normAutofit fontScale="62500"
  lnSpcReduction="20000"/>` is still there and the whole `p:sp` is still the bytes that were read.
- The corpus-wide byte-identity tests of 15-A and 15-B are unchanged and still green, which is what
  says no deck loses a scale it should keep.
- **The fixture already existed**: `corpus/pptx/autofit-stale.pptx`, generated by
  `pptx_autofit_stale()` for the reader's own 13-I test, is the only deck in the corpus carrying a
  `normAutofit` with a `fontScale` — verified by scanning every `.xml` of all seventeen decks. No
  fixture was added. `corpus/generate.py --check` is clean.
- One pipeline test in `docsai-convert`, `an_edited_body_leaves_its_stale_autofit_scale_behind`: the
  deck converted to DocMark, one bullet retyped as text, converted back, and the written
  `ppt/slides/slide1.xml` carries the new line and `<a:normAutofit/>`, with
  `Warning::AutofitStale { scale: 62500 }` naming `Content Placeholder 2` in the outcome's report.
  `deck_convert`: 11 passed, 1 ignored.
- `cargo test --workspace --no-fail-fast`: 645 passing; only the pre-existing
  `docx_roundtrip_is_idempotent` fails (`corpus/docx/footnotes.docx`, from `88ff8e11`, untouched by
  this increment). `clippy --workspace --all-targets -- -D warnings` clean, `fmt --all` applied.
- One golden regenerated by hand and reviewed as a one-word diff:
  `corpus/pptx/autofit-stale.expected.inspect.json` (decision 3).
- End to end outside the suite: `convert autofit-stale.pptx -o deck.dmk.md`, one bullet shortened
  by hand, `convert deck.dmk.md -o out.pptx --verbose` — the slide carries `<a:normAutofit/>`, the
  edited line, the seven lines nobody touched and the untouched title, and the single warning reads
  «the text of `Content Placeholder 2` on `ppt/slides/slide1.xml` changed, so the writer dropped an
  autofit scale of 62.5 %, a number PowerPoint computed for the text that was in the box when it
  was last edited by hand». Run in a scratch directory, so no asset directory was left beside the
  corpus.
- The LibreOffice render was **not** run: the installation on this machine is broken (`soffice.bin:
  error while loading shared libraries: libreglo.so`), as it was for 15-D and 15-E. That is an
  environment fault, not a finding about the deck.

## Next

15-G: the deck with **no** skeleton — an embedded default template, automatic layout selection from
the shape of the content (title + bullets → «Title and Content»), and with it the part a slide the
model *added* needs, which is the refusal 15-E wrote down.
