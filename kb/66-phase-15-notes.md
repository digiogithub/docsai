---
tags:
    - phase-15
    - pptx
    - presentations
    - writer
    - notes
    - sections
    - skeleton
---
# 66 — Phase 15 E: the notes page, and the deck's own slide list

Increment **15-E** of [[61-phase-15-plan]], on top of [[65-phase-15-raw]]. The first increment
that writes a part which is not a slide: the notes page of [[51-phase-14-notes]] on the way out,
and the two facts `ppt/presentation.xml` states about the deck rather than about a slide.

## What changed

- `pptx/write.rs`: `rewrite_notes`, and a new front half of `rebuild_slides` — `DeckParts`,
  `SlideEntry`, `Parsed`, `deck_parts`, `Signature`, `align`, `restate_deck`, `slide_list_edit`,
  `section_edits`, `deck_rels_without`. `Rebuild` gains `removals` and `reemit` honours them;
  `content_types` learned to take an `Override` out as well as put a `Default` in.
- `pptx/mod.rs`: `csld_name` and `rel_id` are `pub(super)`. The writer recognises a slide by what
  the reader named it.
- `pptx/skeleton.rs`: what `rebuilt_parts` is **not**, written into the doc comment — see
  decision 2.
- `docsai-convert`: the `SUPPORT` note, and two pipeline tests.

## Non-obvious decisions

1. **A slide has no identity, anywhere in this chain, and the writer needs one.** `p:sldId@id` is
   the deck's own bookkeeping and the IR does not keep it; DocMark's `{#n7}` is assigned on
   serialisation and the package has never heard of it; the reader leaves `Slide::id` empty. So
   when the model's slides are not the package's, *which* package slide the model's second one is
   cannot be looked up — it has to be recovered. `align` does it in three passes, weakest claim
   last: a slide that is where it was and says what it said keeps its part (which is the whole
   corpus, and the reason an unedited deck is untouched); a slide the package still states
   verbatim *somewhere else* has moved and takes its part with it; and what is left — an edited
   slide — is paired in order with the parts that are left. What a slide «says» is its `p:cSld@name`,
   its title text and its shape count, a `Signature` both sides can state. The failure mode is
   bounded: a wrong match writes the model's text into a part with the same name, the same title
   and the same number of shapes as the one it belongs in, which is the case where the two are
   interchangeable anyway. It is a heuristic, and it is written down here as one.
2. **`ppt/presentation.xml` is edited, not rebuilt, and stays out of `rebuilt_parts`.** The list is
   «parts whose content the IR holds», and the IR does not hold that part's content: it holds two
   facts stated inside it, the slide order and each slide's section. Putting the part on the list
   would license regenerating `p:sldSz`, the master and notes-master lists, the presentation text
   styles and the custom shows from a model that does not carry them — the exact loss spike P3
   measured. So the increment made the licence *narrower* in writing rather than wider in code:
   two edits, `p:sldIdLst` and each section's `p14:sldIdLst`, and every other byte copied.
3. **The list is not a licence the writer can read anyway.** `deck_parser` fills `rebuilt_parts`
   with an empty `Vec` ([[65-phase-15-raw]] decision 4 met the same wall), so a deck that came back
   through DocMark — the only way an edit ever reaches this writer through the pipeline — carries
   no list at all. Gating the notes rewrite on it would have made 15-E work in the unit tests and
   do nothing end to end. What actually keeps the two sides agreeing is that they go through the
   **same doors**: the slides `p:sldIdLst` names, and the notes page `notes::part` finds in a
   slide's own relationships. That is the same reasoning 13-H decision 3 used to build the list.
4. **A removed slide leaves the archive.** Unlisting it from `p:sldIdLst` is the smaller edit and
   would have been valid OPC — the part would still be typed and still related, just not shown,
   which is a shape the reader already anticipates. It is the wrong answer: a caller who deleted a
   slide and got a file that still carries its text has been told the slide is gone and been handed
   it anyway. So the part, its `_rels`, its notes page, that page's `_rels`, its `Override` in
   `[Content_Types].xml` and its `Relationship` in `ppt/_rels/presentation.xml.rels` all go. Both
   parts that name it are **spliced**, so the relationships and overrides that stay keep their ids,
   their order and their bytes.
5. **Two refusals guard that removal, because a dangling relationship is the failure mode of the
   phase.** A deck with a `p:custShowLst` keeps every slide it has: a custom show names its slides
   through the relationships this writer would delete, and the IR does not model one. And before
   anything is removed, every other part's relationships are scanned for a target that is about to
   go — a hyperlink from another slide, a thumbnail, a part nobody has looked at yet. Refusing the
   removal loses a deleted slide and says so; performing it can lose the deck (P3 §2.4: LibreOffice
   accepts a dangling relationship in silence, and the gate is PowerPoint).
6. **A section keeps its name and its GUID.** `p14:section@id` is a GUID nothing in the IR records,
   so a section the deck does not already have cannot be created: the model can move a slide
   *between* existing sections and can empty one, and a slide whose `section` names something the
   deck never had **stays where the package puts it**, with a `Degraded` warning naming it. So does
   a slide the model states no section for at all — a model that does not state something is not a
   model asking for it to be removed, which is the same rule the rest of this writer follows, and
   the reason a sectioned deck nobody edited produces no edit *and no warning*. The first version
   reported every such slide as a loss, which would have been a warning about something that did
   not happen. The `p14:` prefix is read off the element rather than assumed; it is a convention,
   not a rule.
7. **The notes page is reached through the slide, never by number.** `notesSlide7.xml` belonging to
   the seventh slide is a convention PowerPoint follows and a converter is free to break —
   `corpus/pptx/notes-crossed.pptx` exists because [[51-phase-14-notes]] said so. The writer calls
   the reader's own `notes::part`, so the page it writes is the page whose text the IR holds.
8. **A page with two note-like shapes is copied and named.** The reader concatenates the text of
   every body-like shape on the notes page, so the model no longer records where one ended and the
   next began. Splitting the note back over two shapes would be a guess; the warning says which
   part and how many shapes.

## Known losses, written down rather than discovered later

- **A slide the model *added* is refused.** There is no part to splice into, and writing a slide
  part from the model rather than into one is the deck-with-no-skeleton work of **15-G**. The
  package is copied whole and the warning names the increment — copying *some* of the edits would
  put the third slide's text under the fourth slide's number.
- **A notes page the deck does not have is not created.** Same reason: a part of its own, hanging
  from a notes master. Named, and pointed at 15-G.
- **Deleting a notes page is not performed.** `notes: None` over a page the package still states is
  a `Degraded` warning and the page stands. Only a *removed slide* takes its notes page with it.
- **A deck that names a slide this writer cannot resolve refuses to restate its list**, rather than
  restating it and dropping the entry. Such a deck is already degraded and the reader warned about
  it on the way in.
- **The alignment is content-based** (decision 1). Two slides with the same name, the same title
  and the same shape count are interchangeable to it, and a swap of the two is invisible: the text
  lands right and the unmodelled markup of each part stays where it was.
- **The whitespace between `p:sldId` entries is not preserved** when the list is restated: the
  entries themselves are the bytes that were read, joined without the indentation a
  pretty-printed deck had between them. Only in a list that actually changed, and it is
  insignificant whitespace.
- **A field is still refused**, and 15-B's table pointed at this increment for it. It was wrong to:
  `a:fld` is identified by a GUID the IR does not record at all, so putting one back is a
  `docsai-model` change and not a matter of which part the writer may touch. The comment in
  `write_text.rs` now says that instead of naming an increment that does not deliver it. The same
  correction was made to the internal-hyperlink refusal: 15-E gives the writer the slide-to-part
  map it was said to be waiting for, and the link is *still* refused, because an internal jump is a
  `slide`-typed relationship plus an `action` attribute the IR does not model. A forward reference
  that does not come true is the same failure as a false warning.

## How it was verified

- Twelve new tests in `pptx::write` (34 in the module, 151 in the crate). The corpus-wide
  byte-identity tests of 15-A and 15-B are unchanged and still green, which is what says the notes
  rewriter reproduces the bytes it was given: `an_unedited_notes_page_is_rewritten_into_the_same_bytes`
  states it directly over every deck that has one. Then `an_edited_note_reaches_the_written_deck`,
  `an_edited_note_lands_in_the_page_the_slide_points_at` — over `notes-crossed.pptx`, whose first
  slide's note lives in `notesSlide2.xml`, so a writer that trusted the numbering would put the
  edit under the other slide —
  `a_note_the_deck_has_no_page_for_is_kept_and_named`,
  `dropping_the_notes_of_a_slide_is_kept_and_named`,
  `removing_a_slide_takes_its_parts_out_of_the_deck`,
  `reordering_the_slides_moves_the_list_and_not_the_parts`, `adding_a_slide_is_refused_by_name`,
  and — because **no corpus deck carries `p14:sectionLst`**, PowerPoint writes it and the generated
  corpus does not — three tests of `section_edits` over a presentation part written by hand, in the
  same shape as the reader's own `sections_map_slides_by_their_deck_id`. No fixture was invented for
  it: the restatement is a pure function of that part, so it is tested as one. The two refusals of
  decision 5 are tested over a `Package` built in memory.
- The 15-B test `removing_a_slide_copies_the_package_and_names_the_increment` was **deleted**: it
  asserted the refusal this increment lifts.
- Two pipeline tests in `docsai-convert`: a note retyped in the Markdown reaches
  `ppt/notesSlides/notesSlide1.xml` while the page's furniture and the other page stand, and a
  slide deleted as text leaves the package with `ppt/slides/slide2.xml` gone.
- End to end outside the suite: `convert notes-speaker.pptx -o deck.dmk.md`, retype a note and a
  title, `convert deck.dmk.md -o out.pptx` — both edits in the package, `ppt/presentation.xml`
  byte for byte the part that was read, no warning. Then the same deck with its second `##` slide
  deleted: `slide2.xml`, `notesSlide2.xml` and both `_rels` are gone from the archive, the deck's
  relationships and content types no longer name them, and `p:sldIdLst` holds one entry. The
  LibreOffice render was not run: the installation on this machine is broken (`soffice.bin: error
  while loading shared libraries: libreglo.so`), which is an environment fault, not a finding
  about the deck.
- `cargo test --workspace --no-fail-fast`: only the pre-existing `docx_roundtrip_is_idempotent`
  fails (`corpus/docx/footnotes.docx`, from `88ff8e11`, untouched by this increment).
  `clippy --workspace --all-targets -- -D warnings` clean, `fmt --all` applied.

## Next

15-F: `Warning::AutofitStale` on the way out — a `fontScale` PowerPoint computed for text that is
no longer there is a lie no reader can detect, so when the shape's text changed the stale scale is
dropped and the warning fires. Done, in [[67-phase-15-autofit]].
