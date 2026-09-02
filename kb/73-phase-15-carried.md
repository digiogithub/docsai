---
tags:
    - phase-15
    - pptx
    - presentations
    - writer
    - byte-identity
    - fidelity
---
# 73 — Phase 15 carried forward: the added shape, and `<a:p/>`

Two of the items [[72-phase-15-close]] listed as carried forward, fixed after the phase closed
because a phase close must not change the writer. Neither is a new capability: both are the same
rule stated better — **write everything the model says and nothing it does not**, judged against
what the part already *says* rather than against its bytes or its shape count.

## Defect 1 — a shape added to a preserved slide discarded that slide's other edits

### What it was

`rewrite_slide` opened with a count check: if the model held a different number of shapes than the
part's `p:spTree` did, the whole slide was refused and the package's own copy of it stood, with a
`Degraded` warning saying so. The warning was true and the outcome was worse than it sounded — a
caller who **added a shape and retyped the title lost the retyped title too**, and nothing said
that, because the warning was about the shape. The 15-K close found it with an accidental edit
(`kb/72` §6): a stray blank paragraph became a third shape on a two-shape slide and every other
edit on the slide went with it.

### What was done, and why that and not the other thing

Two honest answers existed. **Writing the added shape** is the bigger one: 15-G already generates
`p:sp` from the model ([[68-phase-15-template]] — `write_gen::text_box_xml`, `picture_xml`,
`table_xml`), so the markup is not the problem. What is missing is everything that makes a shape
belong to *this* slide: a `p:cNvPr@id` unique in the part, a `p:spTree` position (z-order, which
DocMark does not carry — 15-H), and above all a **position and a size**, which a shape typed into
Markdown does not have. On a slide this writer generates whole there is a layout it chose and a
frame it can take from it; on a *preserved* slide there is neither, and putting the box somewhere
would be inventing a fact about the document. §7 rule 3 allows that only as a loss that is warned
about, and a shape landing at a guessed position in a design nobody can see from here is a poor
trade for the caller.

So the fix is the second answer: **splice everything the writer can express, and name precisely the
one thing it could not.** The count check is gone. In its place `pair_shapes` decides which tree
child each model shape is written into:

- **Same number of shapes on both sides** — the pairing is the reading order and nothing else,
  exactly as before. This is every deck in the corpus and every path 15-A…15-J measured; the
  by-bytes behaviour of that case is untouched on purpose.
- **Different numbers** — the pairing is *recovered*, the way [[66-phase-15-notes]]'s `align`
  recovers which package slide a model slide is. The surviving shapes are in the same relative
  order on both sides (both lists are reading order over the same slide), so the only question is
  where the odd one out sits, and `unique_alignment` answers it by **counting** the order-preserving
  alignments in which every shape is matched to one of its own kind. One alignment is an answer;
  none or several is a refusal, and the caller then copies the slide whole exactly as it did before.

Counting rather than searching is the whole point: a search finds *an* answer, a count knows whether
it was the only one. A wrong pairing would write one shape's text into another shape's box — the
silent corruption 15-H found by measuring idempotence — and that is worse than not writing the edit.

The kind a shape is matched by (`ShapeKey`) is deliberately coarse: placeholder type and `idx`, text
box, picture, and one class for everything else. It has to survive an *edit* (a retyped placeholder
is the same placeholder), and it must never disagree with the reader — telling a table from a chart
from a diagram inside a `p:graphicFrame` is the reader's judgement, and repeating it here would be a
second opinion that could contradict the first. A key too coarse costs a refusal; a key that
disagreed with the reader would cost a wrong pairing.

Both directions are now written and named:

- a model shape with no counterpart — «the text box `Caja nueva` the model holds is not in the
  preserved slide, and this writer does not add a shape to a slide it did not generate, so it is not
  in the file; **every other edit to the slide was written**»;
- a part shape no model shape claims — «the shape `p:sp` "Content Placeholder 2" stands in the
  preserved slide and the model holds none that is it; removing a shape from a preserved slide is
  not written, so it is still in the file».

The second is the symmetric case and it was fixed by the same change rather than left half done:
before, a *deleted* shape discarded the slide's other edits too.

### What remains

- **Adding a shape is still not performed.** What it needs is written above: an id, a z-order
  position and a frame. The frame is the real cost — either the model starts carrying one for a
  shape typed by hand, or the writer invents one and warns, which is a decision about what
  `--fidelity` means and not a defect fix.
- **A slide that both gained and lost a shape** has the same number of shapes on both sides and is
  paired positionally, as it always was. Detecting it needs the key comparison to run in the equal
  case too, which changes the behaviour of every deck that round-trips today; it is a separate
  measurement and it was deliberately not opened here.
- **The reader can drop a shape of its own accord** — an unreadable `p:pic` produces no shape — and
  such a slide now reaches the recovery path and reports the picture as one the model does not hold.
  The sentence is true (the model does not hold it, and it is still in the file); the reader has
  already warned about it separately on the way in. No corpus deck does this.

## Defect 2 — `<a:p/>` did not survive a round trip

### What it was

[[70-phase-15-xsd]] recorded it as a known limit: the writer normalises an empty paragraph to
`<a:p><a:endParaRPr/></a:p>` — deliberately, because that is where the properties of the text typed
there live and it is what every producer writes — so a package spelling it `<a:p/>` came back
schema-valid, visually identical and **not byte-identical**. `corpus/generate.py` worked around it
by never emitting the short spelling, which meant nothing in the suite could see it.

The recorded diagnosis was that fixing it needed the IR to record which spelling the source used, a
`docsai-model` change. **That diagnosis was wrong**, and the cheaper possibility the writer's own
rule already implied is the fix: 15-B says *nothing is written unless it differs from what the part
already says*, and «differs» is a question about what the markup **means**. `super::text` reads
`<a:p/>` and `<a:p><a:endParaRPr/></a:p>` as the same paragraph — no runs, no direct properties —
so the two are one paragraph in the IR and rewriting one into the other is an edit nobody asked for.
The writer was answering the question by bytes, and that is where the defect was. No IR field is
needed, and none was added.

### What was done

`body_changed(current, written)` replaces the three `!=` comparisons at the three places a text body
is spliced — a slide's shape, a notes page, a table cell — and expands the short spelling before
comparing. When the two say the same thing **no edit is produced at all**, so the source bytes
stand, whichever spelling they are in. It is a comparison and not a normalisation of the output: a
body that really was edited is still written in this writer's own spelling.

It also unifies 15-F: «the shape's text changed», which decides whether a stale
`a:normAutofit@fontScale` is dropped, is now the same notion of change and not a second one. A deck
spelling an empty paragraph the short way used to have its autofit reported stale on a shape nobody
had touched — a warning that was **false**, which §7 rule 3 forbids as firmly as a silent loss.

### The fixture, because a fix nobody can see fail is not a fix

`corpus/pptx/placeholders-empty.pptx` now spells one of its two empty paragraphs the short way
(`corpus/generate.py`, `A_P_SHORT`) and the other the long way. That deck is the right home for it:
it exists to say that an empty placeholder is content. With the fixture and without the fix, the
two corpus-wide byte-identity tests go red —
`an_unedited_slide_is_rewritten_into_the_same_bytes` and `a_written_deck_is_the_package_that_was_read`
— naming `ppt/slides/slide1.xml`. `the_short_spelling_of_an_empty_paragraph_is_not_an_edit` states
it directly and also asserts the other half: a body that *was* retyped is still written, and the
untouched empty paragraph beside it is not.

Blast radius, measured rather than assumed: **one golden line**, the `skeleton:` content hash of
that deck. The token budget is unchanged, no other corpus format moved, and both CI gates are green.

### What remains

- The expansion is spelt `a:`, as everything this writer emits is. A package binding the DrawingML
  namespace to a different prefix is already rewritten into this one wherever a body is touched, so
  the comparison does not make that worse — but it does not make it better either.
- `<a:p></a:p>` and `<a:p />` are expanded too; `<a:p>\n</a:p>`, with insignificant whitespace
  inside, is not. It reads as the same paragraph and would be a wider claim about whitespace than
  this comparison wants to make.

## Files and symbols

- `crates/docsai-office/src/pptx/write.rs`: `Pairing`, `pair_shapes`, `unique_alignment`,
  `ShapeKey`, `model_key`, `part_key`, `element_label`, `Rewritten::unpaired`; `body_changed`,
  `expand_empty_paragraphs`; `rewrite_slide` (the count refusal removed, the pairing and the two
  new warnings in), `rewrite_notes` and `rewrite_table` (which now takes the part's `source`).
- `corpus/generate.py`: `A_P_SHORT` and `pptx_placeholders_empty`.
- `corpus/pptx/placeholders-empty.pptx` and its `.expected.dmk.md` (the `skeleton:` line).

## How it was verified

- `cargo test --workspace --no-fail-fast`: **670 passed, 1 failed, 1 ignored** — the failure the
  pre-existing `docx_roundtrip_is_idempotent` on `corpus/docx/footnotes.docx` (`88ff8e11`,
  untouched here). Four new tests, each of which was **watched fail** with the old code in place:
  `the_short_spelling_of_an_empty_paragraph_is_not_an_edit`,
  `a_shape_added_to_a_preserved_slide_keeps_the_slide_s_other_edits`,
  `a_shape_removed_from_a_preserved_slide_is_named_and_stands`,
  `an_ambiguous_shape_pairing_copies_the_slide`.
- `clippy --workspace --all-targets -- -D warnings` clean, `fmt --all` applied.
- `python3 corpus/generate.py --check` clean at 129 files.
- **Byte identity, the property this must not weaken**: all 17 corpus decks round-trip through the
  CLI byte for byte, `.pptm` included — and the corpus now covers one spelling more than it did.
- `python3 corpus/schema_gate.py`: 17 decks, 34 packages, **322 parts, 0 invalid**, exit 0.
- `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer`:
  17 decks, 61 pages compared, **0 failures**, exit 0.
- Every corpus deck round-tripped through the CLI with the new warnings grepped for: **none fires**,
  which is what says they are not false ones.

## Next

Still carried forward from [[72-phase-15-close]] and untouched here: the unrun P2 gate, the render
gate that has never rendered a real slide, the preserved table whose grid moved, the unvalidated
preserved parts, `align`'s content-based pairing, the refused internal hyperlink, the inherited
`agent` ≤ 15 % criterion, and the red `docx_roundtrip_is_idempotent`.
