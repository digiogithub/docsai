---
tags:
    - phase-15
    - pptx
    - writer
    - media
    - relationships
    - content-types
---
# 64 — Phase 15-C: pictures, tables and media

Third increment of [[61-phase-15-plan]], on top of [[63-phase-15-slides]]. The splice that rebuilt
a slide's text now reaches what a text body is not: a picture's bitmap, its alt text, the size it
is drawn at and its crop; the text of a table's cells; and the hyperlinks a run carries.

## The rule did not change, its reach did

15-B's unit of rewriting was a text body. 15-C keeps that rule and applies it one container
further out: **nothing is written unless it differs from what the part already says**. A picture
nobody touched produces no edit at all, a picture whose alt text was retyped produces exactly one
attribute edit, and the byte-identity property of [[62-phase-15-package]] survives untouched — now
including `ppt/slides/_rels/*.rels` and `[Content_Types].xml`, which an edited deck moves and an
unedited one does not.

The comparison is always made **in the model's units, against the model's reader**: the crop is
compared by running `drawingml::read_crop` over the part and comparing `CropRect`s, not by
comparing the strings that state it. A writer that compared rendered text would rewrite
`l="0"` as absent and call it a change.

## The bitmap is compared by bytes, not by name

The asset store keys on the content hash, so an image that came out of this very package *is* the
same file and keeps the relationship it already had. Only genuinely different bytes mint a new
`ppt/media/imageN.<ext>` part, and the bytes that go in are the bytes the store holds — the ones
that came out of the source document. Nothing here decodes, scales or re-encodes an image; the ZIP
deflates the member like any other, which is lossless.

Two details that only show up in a corpus:

- **The extension comes from the store's sniff**, not from any name in the document, because a
  content type has to agree with the content. `AssetInfo` already carries both the sniffed
  extension and the content type.
- **A new part avoids the *stem*, not the name.** `ppt/media/image1.gif` does not collide with
  `ppt/media/image1.png` as a name, and reading a deck with both is a confusion nobody needs.

## The neighbouring parts are spliced, never regenerated

Three parts have to agree with a re-injected picture or a new link, and all three are edited the
same way — the original bytes with something inserted in them:

| Part | What is added | What is preserved |
|---|---|---|
| `ppt/slides/_rels/slideN.xml.rels` | only relationships the deck did not have | every existing id, target, order and byte |
| `[Content_Types].xml` | a `Default` only for an extension never carried | every `Default` and every `Override` |
| `ppt/slides/slideN.xml` | the attributes and elements that changed | everything else, as in 15-B |

Reuse comes first and matters most: `PartRels::id_of_target` answers with the id the deck was
written with whenever the target is unchanged, which is why an unedited slide never touches its
`_rels`. New ids are `rIdN` chosen not to collide with what the part uses — the corpus has decks
whose ids are not numeric at all (`rIdImg`), so «the next number» is a search, not a count.

## A table cell is a text body; a table is not a table writer

The plan said «tables written back through the DrawingML table writer». That is the wrong tool
here and the increment does not use it. Regenerating an `a:tbl` from the IR would throw away the
grid, the row heights, `a:tcPr` (borders, margins, anchoring, fills the IR does not model) and the
band flags — the same loss spike P3 measured on a package, one scale down. So the cells' `a:p`
sequences are spliced and the table around them is the bytes that were read.

The cost is stated rather than hidden: a table whose **grid** the model moved — a row added, a
cell removed, a column width or a style reference changed — is copied whole with a warning saying
so.

> **Amended by [[68-phase-15-template]] (15-G).** This entry originally said the warning named
> **15-G**, «which is where an `a:tbl` written from the model belongs». Half of that came true and
> the half that did not is worth stating plainly. 15-G *does* write an `a:tbl` from the model —
> grid, spans, cell bodies and a style reference — but **only onto a slide it generates**, where
> there is no grid to preserve. A table inside a *preserved* slide is still spliced cell by cell,
> and a preserved grid the model moved is **still copied and named**, for the reason this section
> gives: regenerating it would throw away the row heights, the `a:tcPr` and the band flags the IR
> does not model. No increment owns lifting that; doing it needs those things in `docsai-model`
> first. The warning no longer names an increment.

Two mirror rules from [[39-phase-13-pictures-tables]] had to be inverted, exactly as the bullet rules
were in 15-B:

- A cell swallowed **horizontally** is not in the IR grid at all, so the pairing skips `hMerge`
  cells on the way out too. Pairing them positionally would put row 2's text in row 1's merge.
- A cell **covered** vertically holds no content in the IR, and PowerPoint keeps an empty
  `a:txBody` there. Writing the model's empty body would be indistinguishable — until a covered
  cell someday carries something — so it is skipped rather than emptied.

## The hyperlink, and the one that is still refused

A link is not an element in DrawingML: it is `a:hlinkClick r:id` on the properties of every run it
covers. `write_text` now takes a `Links` trait, one call wide (`hyperlink(target) -> Option<id>`),
so the text writer knows nothing about how relationships are stored or numbered.

What is **still refused**, with the shape kept and the target named, is a target no relationship
can state as it stands — an internal one pointing at another part of the deck. Minting that means
deciding which part the model's target refers to, which is 15-E's bookkeeping. The alternative is
an `r:id` pointing at nothing, and that is precisely the dangling relationship spike P3 measured
LibreOffice accepting in silence: a file that looks fine in the tool that is not the gate.

## Known losses, written down rather than discovered later

- A picture whose `p:spPr/a:xfrm/a:ext` the part does not state inherits its size; the model's
  size cannot be written without *placing* a picture the deck left to its layout, so it is a
  warning rather than an invented `a:xfrm`.
- A picture's own hyperlink (`p:cNvPr/a:hlinkClick`) is not written: it needs an element inserted
  into a self-closing tag, and the increment stops at attributes there.
- A **linked** picture (`r:link`, `external_src`) is left alone: there are no bytes in the package
  to re-inject.

## Verification

`cargo test --workspace` green except the pre-existing `docx_roundtrip_is_idempotent` on
`corpus/docx/footnotes.docx` (from commit `88ff8e11`, unrelated to this phase); clippy and fmt
clean. Sixteen writer tests, including the corpus-wide byte-identity ones from 15-A and 15-B, which
are the real guard here: they fail the moment a rewrite writes something the part already said.

End to end outside the suite: `convert images-anchored.pptx -o deck.dmk.md`, retype the `![alt]`,
overwrite the asset with a different PNG, `convert deck.dmk.md -o out.pptx` — the package holds
`ppt/media/image2.png` byte for byte equal to the new file, a spliced `_rels` with the original two
relationships intact, and the retyped `descr`. Headless LibreOffice renders it and `pdfimages`
reports the 2×2 bitmap that was swapped in. A smoke test, not the P2 gate: that is PowerPoint, in
15-K.

## Next

15-D: raw re-injection — a shape kept only as a stub over a raw fragment returns as its original
markup at its original z-order ([[65-phase-15-raw]]).
