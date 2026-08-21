---
tags:
    - phase-15
    - pptx
    - writer
    - drawingml
    - skeleton
---
# 63 — Phase 15-B: the slide comes from the model

Second increment of [[61-phase-15-plan]], on top of [[62-phase-15-package]]. A deck edited as
DocMark goes back into the package it came from, and `docsai convert deck.dmk.md -o deck.pptx`
works end to end.

## The unit of rewriting is a text body

Not a package (spike P3's `naive`), not a slide, not a shape: the `a:p` sequence inside a
placeholder or a text box. Everything around it is the bytes that were read — `p:spPr`,
`a:bodyPr` with its `a:normAutofit`, `a:lstStyle`, `p:nvSpPr`, the shapes that are not text, the
`p:transition` and `p:timing` subtrees, `p:clrMapOvr`. That is spike P3's `rewrite` mode taken one
level further in, and the reason to prefer it over generating a slide from scratch is the same
reason the skeleton exists: what the IR does not model is not thereby unimportant.

The consequence is the property worth naming: **a deck nobody edited comes out byte for byte as it
went in**, over the whole corpus, and a retitled slide differs in exactly the run that was
retyped. A conversion is not a diff.

## `crates/docsai-office/src/pptx/write_text.rs`

The mirror of [[37-phase-13-slide-text]]. Two of the reader's judgement calls are *inverted* here
rather than copied:

- **The bullet.** A body placeholder bullets by inheritance, so a list item inside one writes no
  `a:buChar` at all and it is a *plain* paragraph there that needs the explicit `a:buNone`.
  Outside a body placeholder it is the other way round. A nested item needs neither: `lvl` greater
  than zero already means «item» on the way in.
- **The delta.** A run stores what it adds to the cascade ([[38-phase-13-cascade]]), and the
  cascade lives in the layout and the master, which this phase never rewrites. So the delta is
  written back as a direct property and subtracted again on the next read, arriving where it
  started.

`a:rPr@lang` is the one attribute written from outside the IR. It is spell-check metadata with no
IR field, it is on every run PowerPoint has ever written, and dropping it from every run of a
rewritten deck would be a change to what the file says about itself. So the slide's own language
is read off its runs and put back — and only when they all agree, because picking one of two
languages would retag the other.

## What the writer keeps, and says it kept

Every one of these is a `Degraded` warning naming what was copied and the increment that lifts it,
because a caller who edited the model and got the original back has to learn it from the report
rather than from the file (`AGENTS.md` §7 rule 3):

| Left alone | Increment |
| --- | --- |
| a picture, a table, a chart, a preserved object | 15-C, 15-D |
| a speaker notes page (a part of its own) | 15-E |
| a hyperlink (needs a relationship in the slide's `_rels`) | 15-C |
| a field (`a:fld` is identified by a GUID the IR does not record) | 15-E |
| a slide added or removed (moves `p:sldIdLst`, the rels and the content types) | 15-E |

A shape whose text cannot be written keeps *its own* markup; the rest of the slide is still
rewritten. Only a structural mismatch — the model and the part disagreeing about how many shapes
or slides there are — copies the whole package.

## The pipeline opens

`SUPPORT` now answers `pptx: write yes`, `docsai_office::WRITABLE` includes it, and the convert
pipeline routes `Format::Pptx` like the other binary targets. Measured end to end: convert a
corpus deck to DocMark, retype the `##` heading, convert back, and the new title is in
`ppt/slides/slide1.xml`; LibreOffice renders the result with the new title and no complaint. That
is a smoke test, not the gate — the gate is PowerPoint and lives in 15-K.

## Tests

Eleven in `pptx::write` (`cargo test -p docsai-office pptx::write`), of which 15-B adds:
`an_unedited_slide_is_rewritten_into_the_same_bytes` over the corpus,
`a_retitled_slide_reaches_the_written_deck` (and the slides nobody touched compare equal),
`an_added_bullet_comes_back_as_a_bullet`, `run_formatting_survives_the_round_trip`,
`a_link_the_writer_cannot_place_keeps_its_shape`, `a_slide_with_a_picture_keeps_it_and_names_it`,
`removing_a_slide_copies_the_package_and_names_the_increment`. Plus
`a_deck_edited_as_docmark_goes_back_into_its_package` in `docsai-convert`, which is the only place
the `skeleton:` reference is seen being written and resolved again.

## Known losses, written down rather than discovered later

Regenerating a text body drops the editor metadata the IR does not model on the elements it
rewrites: `dirty`, `err`, `smtClean` and friends on `a:rPr`. `lang` is put back, the others are
not, and none of them changes what the slide shows. A run whose font came from a theme reference
(`+mj-lt`) is written with the resolved name, so it follows the theme by value rather than by
reference — which only happens where the run already overrode what it inherited.

Done, in [[61-phase-15-plan]] · next: 15-C, pictures, tables and media.
