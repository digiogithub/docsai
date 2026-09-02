---
tags:
    - docx
    - writer
    - addressing
    - idempotence
    - fidelity
---
# 74 — The docx bookmark that was a node id: `footnotes.docx` goes green

The one red test in the workspace, carried by every increment of Phases 14 and 15 as
«pre-existing and out of scope» ([[72-phase-15-close]], [[73-phase-15-carried]]):
`docx_roundtrip_is_idempotent` on `corpus/docx/footnotes.docx`. It was a real defect in the
`.docx` writer, it is fixed, and the diagnosis is worth more than the fix.

## What the test said

The second pass of `Office → DocMark → Office → DocMark` disagreed with the first on two
characters:

```
--- first ---                                   --- second ---
next-id: 5                                      next-id: 6
Texto con nota[^1]{#n2} y sigue. {#n1}          Texto con nota[^1]{#n4} y sigue. {#n1}
Otro parrafo con segunda nota[^2]{#n4}. {#n3}   Otro parrafo con segunda nota[^2]{#n5}. {#n3}
```

Nothing about the document moved — same text, same footnotes, same styles. The **paragraph** ids
survived the trip through the package; the **footnote** ids did not, and the allocator therefore
numbered the footnotes around the survivors on the second read: `n1` and `n3` observed, the counter
raised to 4, the two footnotes handed `n4` and `n5`.

## What it was

`88ff8e11` («fix: problems in docx TOC and headers-footers») taught the reader to take a
`w:bookmarkStart` name as the paragraph's or heading's `NodeId`, and the writer to write that id
back out as a `w:bookmarkStart`/`w:bookmarkEnd` pair. The reading half is right and load-bearing:
a TOC's hyperlinks and its `PAGEREF` field instructions address a heading by that exact name
(`_Toc234329254`), and dropping the name leaves them pointing nowhere.

The writing half did not ask **where the id came from**. A node id has two provenances:

- a name read off the source package — document content, and the thing those fields address;
- a counter id the allocator handed out (`n7`) — DocMark's own address, derived deterministically
  from document order by `assign_ids`, referred to by nothing inside the package.

Writing the second kind back as a bookmark invents a feature the document never had, and it breaks
idempotence, because **only two of the addressable node kinds own a `w:p` to carry a bookmark**. A
paragraph and a heading keep their id across a round trip; a footnote, a table, an image and a list
have nowhere to put theirs and are numbered afresh on the next read. Persisting some and not the
others is what makes the second pass differ — and the first pass is not even the wrong one: with
nothing persisted, the numbering is a pure function of document order and both passes agree.

`corpus/docx/footnotes.docx` is the only corpus document where an addressable node **with** a
paragraph of its own and one **without** interleave: a paragraph and the footnote inside it. That
is why it is the only file that went red, and why one file went red rather than none or all.

## Why it survived so long

- **The corpus contains no bookmarks at all** — `grep bookmark corpus/generate.py` returns nothing,
  and every `word/document.xml` in `corpus/docx` has zero `w:bookmarkStart`. The only bookmarks the
  suite has ever seen are the ones the writer minted from counter ids, so the fixture that would
  have shown the feature working (a `_Toc…` name read in and written back) does not exist, and the
  only visible effect of the feature was its side effect.
- The failure looks like a **footnote** defect — the ids that move are the footnotes' — and the
  footnote path is deep and was recently touched, so «suspect the numbering/identity path first»
  points at the wrong crate. The cause is one line in `docx/write.rs` and nothing in the footnote
  code is wrong.
- Both goldens stayed green throughout: a golden is the **first** pass, and the first pass was
  always right. Only the round-trip test could see it.

## What was done

`crates/docsai-office/src/docx/write.rs`: `source_bookmark(Option<&NodeId>) -> Option<&NodeId>`
keeps an id whose `NodeId::counter()` is `None` — an id written by hand or read off a bookmark —
and drops one with the allocator's shape. `write_paragraph_with_bookmark` applies it, so both call
sites (a paragraph's id and a heading's) go through the same decision and a future caller cannot
forget it. The doc comment that claimed «spec §9's node ids double as Word bookmark names» is gone:
§9 of the DocMark specification is Pandoc compatibility, and the specification says nothing about
bookmarks. **Nothing in the specification changed**, and no test expectation was weakened.

The reader is untouched. A package that genuinely carries a bookmark named `n5` still reads as the
id `n5`; that is a real address in a real document, and the counter observes it as it always did.

## Is anything lost?

No, and the reason is worth stating: ids are **derived**, not stored. `assign_ids` walks in document
order and hands out `n1, n2, …`, so the same document produces the same numbering every time. An
agent that converts `docx → DocMark`, edits, converts back, and converts to DocMark again sees the
same ids it saw before for everything it did not move — exactly as it did when the ids were minted
into the package, and now for the footnotes too. What the package no longer carries is a bookmark
nobody references.

## The test that proves it

`docx::write::tests::write_keeps_source_bookmarks_and_drops_allocated_ids`, at the level of the
defect rather than only in the corpus-wide round trip: a heading carrying `_Toc234329254` and a
paragraph carrying `n7` with a footnote in it are written, the part is read out of the archive and
asserted to contain `w:name="_Toc234329254"` and **not** `w:name="n7"`, and the package is read back
and asserted to give the heading its bookmark id and the paragraph none. It was watched fail with
the old code in place — `an allocated node id was written as a bookmark` — while the `_Toc` half
passed there too, which is what says the fix removed only the invented half.

## How it was verified

- `cargo test --workspace --no-fail-fast`: **672 passed, 0 failed, 1 ignored** — up from 670/1/1.
  The two new passes are the unit test above and `docx_roundtrip_is_idempotent`, red since
  `88ff8e11`. The ignored one is unchanged: the inherited `--fidelity agent` ≤ 15 % criterion of
  Phase 14, a specification decision.
- `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all` applied.
- `python3 corpus/generate.py --check`: 129 files, exit 0 — **no golden moved**, which is the
  expected blast radius: a golden is a first pass and no first pass changes.
- `python3 corpus/schema_gate.py`: 17 decks, 34 packages, 322 parts, 0 invalid, exit 0.
- `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer`:
  17 decks, 61 pages compared, 0 failures, exit 0.
- pptx byte identity: all 17 corpus decks round-trip through the CLI byte for byte, `.pptm`
  included.

## Found on the way, not fixed here

**An internal hyperlink is written as an external relationship.** `write_inlines` calls
`ctx.add_rel("hyperlink", target, true)` for every `Inline::Link`, so a target the reader produced
as `#_Toc234329254` — a `w:hyperlink w:anchor=…` with no `r:id`, which is what a TOC entry is — comes
back as a relationship with `TargetMode="External"` and a URL of `#_Toc234329254`. The bookmark it
should point at is now written back correctly, and the link still does not use it. The fix is to
split a target starting with `#` into `w:hyperlink w:anchor`, and a `target#anchor` into an `r:id`
plus an `anchor`, mirroring `hyperlink_target` in `docx/body.rs`. It is the docx twin of the refused
internal hyperlink [[72-phase-15-close]] lists for pptx, no corpus document exercises it, and it is
out of scope here.

## Next

[[72-phase-15-close]] and [[73-phase-15-carried]] keep everything else they carry; this closes the
last line of both lists — the red `docx_roundtrip_is_idempotent` — and adds the docx internal
hyperlink above to what is known and unfixed.
