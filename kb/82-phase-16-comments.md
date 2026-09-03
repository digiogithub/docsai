---
tags:
    - phase-16
    - comments
    - docmark
    - docx
    - pptx
    - spec
---
# 82 — Phase 16 E: comments read, one node for two formats

Increment **16-E** of [[75-phase-16-plan]]: `ppt/comments/*` and `ppt/commentAuthors.xml` into the
IR, `word/comments.xml` (plus `word/commentsExtended.xml`) into the *same* IR node, and one DocMark
syntax designed once for both. Writing comments back is 16-F and is not here.

The specification was written before the code, as the plan requires: `docs/docmark-specification.md`
§11.3, plus a row in §3.2's inline table and one in §11.1's id table.

## The design questions, and the answers

### A comment is anchored, and the two formats anchor differently

`docsai_model::text::Comment` is one struct; `CommentAnchor` is where the two formats differ and the
only place they do:

```rust
pub enum CommentAnchor {
    Span(Vec<Inline>),  // w:commentRangeStart … w:commentRangeEnd
    Point(Point),       // p:cm/p:pos
    Unanchored,         // the source anchored it somewhere this model cannot express
}
```

Placement says the rest, and the two cannot disagree because each format only produces one of them:
a range comment is an `Inline::Comment` in the flow, at the start of what it covers; a slide comment
is in `Slide::comments`. An enum rather than two nullable fields: «anchored over text» and «pinned
to a point» are exclusive, and a struct with both `anchor: Vec<Inline>` and `pos: Option<Point>`
would let a caller set both and mean nothing.

`Unanchored` is the honest third case, and it is warned wherever it is produced: a `p:cm` with no
`p:pos` (the schema requires one, so a file without it is already odd), and a DocMark container that
names no `ref=`.

**A commented range that crosses a paragraph is cut at the paragraph end, and warned.** A DocMark
span cannot hold block content, so the alternatives were to lose the comment or to lose the extent
of what it covers. The comment is what a reviewer wrote; the extent is a detail of where they
dragged the cursor. `State::placed_comments` is document-wide precisely so the `w:commentReference`
that follows in the *next* paragraph does not place the same comment a second time.

### Author identity

**DocMark names an author; it does not carry the source's author table.** `author=` and `initials=`
are the display name and the initials, on every comment of a thread. PresentationML's `authorId` is
a number local to one package and meaningless in another, so it is resolved on read
(`pptx::comments::Authors`) and never reaches the IR; 16-F re-establishes it from the name.

An id no author part defines keeps its comment, writes **no** `author=`, and warns naming the id and
the part that failed to explain it. An anonymous comment is a true document; one attributed to
«Unknown» is not. `corpus/pptx/comments-slide.pptx` slide 2 exists to make that path a file rather
than a claim.

### Threads

The IR models a thread (`Comment::replies`), and DocMark nests one: a reply is a `::: {.comment}`
inside the comment it answers, with no `ref=` of its own. One class, and the nesting *is* the thread
— a second class would say a reply is a different kind of thing, and a consumer that flattens
threads would then need two rules.

- **docx** really has threads, and they are not in `comments.xml`: `word/commentsExtended.xml`
  carries `w15:done` and `w15:paraIdParent`, joined to the comments **by paragraph id**
  (`w14:paraId`), never by comment id. The join is built explicitly in `docx::read_comments`, and the
  tree is assembled top-down from a child list so a reply to a reply nests where it belongs. A
  `paraIdParent` cycle terminates on a `seen` set rather than recursing.
- **pptx classic has no threads at all.** `p:cmLst` is a flat list and `CT_Comment` has no parent and
  no done flag. So a deck read here always has `replies: []` and `resolved: false` — and `false` is
  *read from* the absence of the concept, not invented: the code says so where it sets it.

### Modern pptx comments: what is supported, and a warning that is true

Since 2021 PowerPoint writes threaded, text-anchored comments into
`ppt/comments/modernComment_*.xml` under content type `application/vnd.ms-powerpoint.comments+xml`
(`p188:cmLst`), a Microsoft extension ECMA-376 does not define. **It is not read.** The warning names
the content type, says what the schema carries that `p:cmLst` cannot (replies and text anchors), and
says the part is preserved unchanged — which it is, because nothing puts it in `rebuilt_parts`.

The two are told apart by **content type**, not by relationship type: both hang off the slide with a
`…/relationships/comments` type whose last segment is `comments`, so `of_kind("comments")` returns
both and only `[Content_Types].xml` distinguishes them. A reader that guessed from the relationship
would read a `p188:cmLst` as a `p:cmLst` and find no comments in it, silently.

### The false warning at `docx/body.rs:517`

It said «comments are out of scope in v1». It is gone — and finding out *why* it never fired is the
main defect of this increment (below).

### Fidelity levels

Comments are written at **`full` and `agent` only**. Three arguments, each sufficient:

1. A comment is not what the document *shows*, and `plain` is defined as what a reader sees (§6).
2. `standard` does not write back (§11.2 rule 6), so a comment written there is one a hand edit would
   silently discard — and a review thread is exactly the content whose silent loss is expensive.
3. The residue a `standard` deck may leak is **named** in §11.2 — the `{.slide}` marker and the
   containers of rules 4 and 8. A comment container there would widen that normative list and spend a
   budget (`plain_residue.rs`, 18 % / 15 %) that this increment has no licence to raise.

At `standard` and `plain` the commented text survives and the annotation is a warned
`UnsupportedElement`.

## The syntax, as a golden writes it

Text document (`crates/docsai-office/tests/comments.rs`):

```markdown
---
docmark: "1.3"
source-format: docx
next-id: 4
---

Enviar el cierre antes del [viernes]{.comment ref=c1}, antes del consejo. {#n1}

::: {#n2 .comment author="Ada Lovelace" date="2026-05-04T09:12:00Z" initials=AL ref=c1}
El jueves es la fecha real.

::: {#n3 .comment author="Grace Hopper" date="2026-05-04T10:01:00Z" initials=GH}
De acuerdo, lo muevo.
:::
:::
```

Deck (`corpus/pptx/comments-slide.expected.dmk.md`):

```markdown
::: {#n3 .comment author="Ada Lovelace" date="2026-02-03T09:12:00Z" initials=AL pos="1200000emu,2.5cm"}
La cifra de ingresos viene del cierre provisional.
:::
```

`ref=` is a **document-local token** handed out by the writer in writing order — deliberately not the
node id (absent at the levels that write none, so an anchor would point at nothing) and deliberately
not the source's comment id (which is as package-local as `authorId`). It is the same kind of thing
as `raw=r7`.

The `#id` goes on the **container**, not on the anchor. A footnote is addressed at its reference
because its reference is a point; a comment's anchor is a *range*, and a slide comment has no anchor
at all, so the container is the only slot both document classes have. `NodeKind::Comment` is new, and
`paragraph_is_container` now counts a comment, so a paragraph holding one takes an id.

## Why this is a version bump, and 16-B's chart table was not

`DOCMARK_VERSION_COMMENTS = "1.3"`, declared **only by a document that carries a comment** — the same
additive rule 1.1 applies to ids. No existing document changes shape.

The chart data table grew inside a construct 1.2 had already defined, and a 1.2 reader still rendered
the numbers. Comments are different twice over: they add a construct to the **text** profile, where
1.2 added none and where §3 has no «unknown container» tolerance rule; and `ref=` is a *parsing
obligation* — a reader that ignores it does not lose an attribute, it silently drops the fact that a
span of the document was under review. A construct whose absence is indistinguishable from «this
document has no comments» is what the explicit `DOCMARK_VERSIONS` list exists to prevent.

## Defects found

1. **`w:commentReference` inside a run became a raw fragment, and the warning that claimed
   otherwise never fired.** `crates/docsai-office/src/docx/body.rs:517` (before this change) matched
   `commentReference` in `collect_pieces`, which walks the children of `w:p`. Word never puts it
   there — it is always inside a `w:r`, and `read_run` had no arm for it, so it fell through to the
   raw branch. Reproduction before this commit: read any `.docx` with a comment and the DocMark shows
   a `::: {.raw}` block holding `<w:commentReference w:id="1"/>` in the middle of a sentence, with no
   «out of scope in v1» warning anywhere in the report. Both halves are fixed:
   `read_run` handles the three markers, and the false warning is gone.
2. **`corpus/docx/` still has no comment fixture, and that is deliberate.**
   `goldens.rs::docx_roundtrip_is_idempotent` runs over every `corpus/docx/*.docx` and asserts
   `docx → DocMark → docx → DocMark` identity. The docx **writer** does not write comments back until
   16-F, so a corpus fixture would fail that gate for a reason that is not a defect. The docx side is
   covered instead by packages built in memory in `crates/docsai-office/tests/comments.rs`, and the
   fixture lands with the writer. `corpus/pptx/comments-slide.pptx` has no such problem: its comment
   parts are preserved, so the deck comes back byte for byte.
3. **A selection carrying a comment declares 1.2 or 1.1, not 1.3.**
   `docmark::selection_front_matter` computes the version from `options` alone and never sees the
   document. Adding the parameter means changing its call site in `docsai-convert/src/select.rs`,
   which another agent holds this session. Carried to 16-F. Reproduction: `docsai read --select` over
   a document with a comment inside the selection.
4. **`p:cm/p:pos` is read as EMU because `CT_Point2D`/`ST_Coordinate` says so**, and PowerPoint
   writes values (x="1350") that are far too small to be EMU on a 9 144 000-EMU canvas. Nothing here
   converts them: inventing a scale factor the schema does not state would move every comment marker
   in every real deck. The corpus fixture uses honest EMU. If a real file proves PowerPoint's unit,
   that is a 16-F change with a file behind it, not a guess.
5. **A comment marker inside a field is dropped, warned.** `w:commentRangeStart` between `fldChar`
   begin and end would split a node the field owns. The field wins and the report says so.

## Refused

- **Writing comments back**, in either format. That is 16-F.
- **A `comments` counter in `ConversionStats` and a `comments` column in the `inspect` slide
  inventory.** Both live in `docsai-convert`, which another agent holds; and the stats field would
  move every `.expected.inspect.json` in the corpus for a number no gate reads.
- **Reading modern pptx comments.** Named as a limitation with a specific warning instead of guessed
  at: `p188` expresses replies and text anchors the classic model cannot, and reading it through
  `p:cmLst` would flatten both.

## What 16-F inherits

- The comment parts are **not** in `rebuilt_parts`. Putting them there is what licenses regenerating
  them, and `comments.rs::a_deck_whose_comments_nobody_touched_comes_back_byte_for_byte` is the test
  that must keep passing once they are.
- `Comment` carries everything a writer needs except the source's own ids, by design: `authorId` is
  re-established from `author=`/`initials=`, and `w:id` from writing order.
- Defects 2 and 3 above are the two pieces of work 16-F starts with.
