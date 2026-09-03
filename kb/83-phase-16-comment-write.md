---
tags:
    - phase-16
    - comments
    - docx
    - pptx
    - writer
    - skeleton
    - units
---
# 83 — Phase 16 F: comments written back

Increment **16-F** of [[75-phase-16-plan]], the other half of [[82-phase-16-comments]]. 16-E read
`ppt/comments/*`, `ppt/commentAuthors.xml`, `word/comments.xml` and `word/commentsExtended.xml` into
one IR node and specified the DocMark syntax; this increment writes all of it back and settles the
four defects 16-E carried by name.

Nothing here redesigns 16-E. The one place the specification changed is a **correction** argued
below: `p:cm/p:pos` is not in EMU, and now there is a file that proves it.

## The property, first

**Twenty-one decks that nobody edited still come back byte for byte.** `render_gate.py`:
*21 deck(s): 21 round trip(s) identical by bytes, 0 failure(s)*. The comment parts are still **not**
in `SkeletonRef::rebuilt_parts` — reading a comment bought no licence to regenerate the part that
holds it, and writing one buys none either. An edited comment is **spliced into the part that was
read**, which is [[78-phase-16-chart-write]]'s technique one part further out.

And the question «has anything changed?» is asked exactly once, in the model's own units, by the
model's own reader:

```rust
let said = comments::read_one_part(package, part, &self.authors, &mut quiet)?;
let wanted = slide.comments.iter().map(comment_without_ids).collect();
if said == wanted { return Written::default(); }
```

`read_one_part` is 16-E's `read_part`, unchanged, so there is no second notion of what a comment
part says and nowhere for the two to drift apart. A deck whose comments nobody touched produces no
edit at all — not to the comment part, not to its `_rels`, not to `ppt/commentAuthors.xml`, not to
`[Content_Types].xml` — and **no warning**, which
`a_comment_nobody_touched_produces_no_edit_and_no_warning` asserts.

`comment_without_ids` is new, and it is the one thing the naive comparison got wrong: a node id is
DocMark's, handed out by `assign_ids` after the document was read, and a `p:cm` never carried one.
So the comparison strips them — through the very walk that put them there
(`addressing::walk::visit_comment`), so the two cannot disagree about what an id is.

## The four defects 16-E carried

### 1 — the missing `corpus/docx/` fixture

**Added: `corpus/docx/comments-review.docx`.** It could not exist before because
`goldens.rs:400 docx_roundtrip_is_idempotent` runs over every `corpus/docx/*.docx` and the docx
writer dropped comments. It does not any more, and the fixture is green in that gate.

Four shapes, one per thing a reader or a writer can get wrong: a commented **range**; a **thread**
joined through `w14:paraId`/`w15:paraIdParent`; a **resolved** thread (`w15:done="1"`); and a bare
`w:commentReference` with **no range**, which reads back with an empty anchor. Its prose is real,
for the reason every other fixture's is — the residue and token budgets are measured over exactly
these files.

Two consequences the fixture had, both recorded rather than papered over:

- `agent_projection.rs::the_projection_never_pays_for_what_it_cannot_edit` failed at **52 %**
  against a ceiling of 50. The ceiling was **not** raised. The metric is
  `(agent − plain) / (full − plain)`, and body text cancels out of it, so the fix was not «more
  prose»: comments are content that `plain` drops, so they sit on the numerator, and the fixture
  needed more *`full`-only* weight to balance them. It got direct character formatting and two
  paragraphs with real paragraph properties — which is also what a reviewed document looks like.
- `token_budget.md` moved, +3.3 % corpus-wide, and the golden was regenerated deliberately.

### 2 — a selection carrying a comment declared 1.2 or 1.1

**Fixed, and it was hiding a bigger one.** `selection_front_matter` now takes `has_comment` and
declares 1.3 when the selection carries a comment, whichever profile it is in. Its only caller is
`docsai-convert/src/select.rs`, which 16-E could not touch.

Writing that fix exposed the real defect: **a selection of a commented paragraph carried the anchor
and not the container at all.** §11.3 writes the container after the body, with the footnote
definitions — outside every fragment — so `select()` produced `[viernes]{.comment ref=c1}` with
nothing declaring `c1`, which is the spec's own «anchor with no definition» case: the annotation
silently gone. `select.rs` already had exactly this repair for footnotes, and now has it for
comments, bound by `ref=` rather than by `#id` because the anchor carries no id.

### 3 — the unit of `p:cm/p:pos`

**Settled: it is PowerPoint's master unit, 1/576 in — which is an eighth of a point — and not EMU.**
The reader now uses `Length::from_eighth_points` and the writer `Length::eighth_points`.

The evidence, and exactly what it is:

1. **ECMA-376's own example is impossible as EMU.** The standard's `pos` element gives
   `<p:pos x="1426" y="660"/>`. As EMU that is 1/640 of an inch from the corner of a ten-inch
   slide — every comment in every deck stacked in the top-left pixel. As master units it is
   2.48 in × 1.15 in, which is where a comment marker sits.
2. **A file, constructed rather than assumed.** PowerPoint was not available, so the next best
   producer was used: an `.odp` carrying one `office:annotation` at `svg:x="5cm" svg:y="3cm"`,
   converted by LibreOffice to `.pptx`, comes out as `<p:pos x="1134" y="680"/>`. 5 cm is exactly
   1134 × 1/576 in, and 3 cm is 680.3.
3. **LibreOffice's exporter says so in source** —
   `sd/source/filter/eppt/pptx-epptooxml.cxx`, `WriteComments`, `convertMm100ToMasterUnit`, where
   `o3tl::Length::master` is defined as 1/576 in — and its OOXML interop is reverse-engineered from
   what PowerPoint writes.

What the evidence is **not**: LibreOffice's *importer* is no confirmation in the other direction.
Feeding `x="1134" y="680"` back to it produces `svg:x="4.761cm" svg:y="3.174cm"`, and a second
comment at `y="1517"` produces exactly `6.348cm` — twice the first. Impress ignores `p:pos` and
lays comment markers out on a grid of its own. That is stated here so nobody later reads the
asymmetry as a contradiction.

The round trip is exact for the numbers that matter and safe for the rest. `x = 1134` reads as
`div_round(1134 × 12700, 8)` EMU and writes back as `div_round(emu × 8, 12700) = 1134`; an odd
master value costs half an EMU on the way in and rounds back to itself on the way out
(`x = 2899` in the fixture is exactly that case, and the deck is byte-identical). One EMU is 1/914400
in, so the residue is below anything a renderer can express.

The corpus fixture moved from «honest EMU» (`x="1200000"`) to the numbers a real deck states
(`1134`, `2899`, `1260`), and the pptx golden with it. Because one pixel is exactly six master units —
914400/96 = 9525 EMU = 6 × 1587.5 — a master value divisible by six writes as a clean `px` in
DocMark, so the fixture deliberately carries both: `pos="189px,113px"` and
`pos="4602163emu,2408238emu"`.

### 4 — a comment marker inside a field

**Consistent, and now asserted.** 16-E drops such a marker on read with a named warning, so the
model never holds the comment: what it holds is an `Inline::Field` whose `cached` is a string.
There is therefore nothing for the writer to claim, and
`a_docx_comment_marker_inside_a_field_is_dropped_on_read_and_cannot_be_written` asserts both
halves — the read warning fires, and the writer emits **no** comment warning and **no**
`word/comments.xml`. A writer that emitted an empty comments part would be claiming a review the
document does not have.

## The pptx writer

### What splices what

| the model did | the edit |
| --- | --- |
| nothing | nothing at all |
| changed a comment in place | that `p:cm` regenerated, every other one copied byte for byte |
| added a comment to a slide that had a part | appended into the same `p:cmLst` |
| added a comment to a slide that had none | a new `ppt/comments/commentN.xml`, its `Override`, its relationship |
| deleted the last comment of a slide | the part removed, and its relationship and its `Override` with it |
| named an author the deck does not know | one `p:cmAuthor` appended to the existing table |
| named an author on a deck with no table | `ppt/commentAuthors.xml`, its `Override`, its relationship on `ppt/presentation.xml` |

A comment that is unchanged **and in place** is written as its own source bytes, so its `p:extLst`
survives; one whose slot moved is regenerated and the loss of its extension list is named. `idx` is
reused from the slot when there is one, so editing a comment's text renumbers nothing, and a new
comment takes an `idx` above every one the part states.

**Removal is a real removal.** `PartRels::without` is new: it splices the `<Relationship>` element
out of the `_rels` part and leaves every other byte, including the ids of the relationships that
stay. A part the package does not carry must not be named by a relationship any more than by an
`Override` — the same OPC inconsistency, one file over. Writing an empty `<p:cmLst/>` instead was
considered and refused: it is valid, and it leaves a package saying it has a comment part when the
document has no comments.

### The author table: matched, never renumbered

16-E decided DocMark names an author and never carries `authorId`, so the writer has to
re-establish the number. **It does so by looking the name up in the table the package already
has**, and mints an id only for a name no `p:cmAuthor` states, appending it after the last. That is
the whole answer to «how do you keep ids stable across a round trip that changed nothing»: they are
not re-established at all on that path, they are *recognised*, and the preserved `p:cm` bytes stand
because every `authorId` in them still resolves to the same person.
`a_new_author_is_appended_and_the_ids_the_deck_already_gave_do_not_move` adds a third author to a
two-author deck and asserts that 1 and 2 do not move.

The match is on the display name, with the initials as a tie-breaker only when **both** sides state
them — a level that wrote `author=` and no `initials=` is still the same person, and refusing to
match there would mint a duplicate author on every write.

`p:cmAuthor` requires `initials`, `lastIdx` and `clrIdx`, none of which DocMark carries. Initials
for a name that stated none are the first letter of each word, which is what every producer does and
what a reader shows; `lastIdx` is the largest `idx` that author actually used in this write; `clrIdx`
is the author's own id. All three are stated in the code with the schema line that requires them.

### What a classic deck cannot hold, said out loud

`CT_Comment` is an author, a date, a point and a string. So on a **slide** comment:

- a **reply** is refused, naming the comment and the count, because `p:cmLst` is flat and
  `CT_Comment` states no parent — the write-side face of 16-E's `replies: []`;
- **`resolved=true`** is refused, because there is no done flag;
- a **block that is not a paragraph** inside the body is refused, because `p:text` is one string;
- a **`date=` that is not an `xsd:dateTime`** is dropped rather than written, because a `dt` the
  schema rejects is a file that does not open — DocMark's `date=` is whatever the source said and a
  hand-typed document can say «ayer»;
- a comment with **no position** is pinned to the corner and said, because `p:pos` is required by
  the schema and there is no way to spell «nowhere»;
- a comment with **no author** is written as author 0 and said, because `authorId` is required.

`what_a_classic_deck_cannot_hold_is_named_and_never_silently_dropped` puts all of those through one
deck at once and asserts each warning by name.

## The docx writer

The docx writer has no skeleton: it **regenerates the whole package**, and always has. So its
contract is the model's, not the bytes' — `goldens.rs` compares docx round trips at the DocMark
level and pptx at the byte level, and this increment holds to exactly that.

- The body gets `w:commentRangeStart` … `w:commentRangeEnd` and a `w:commentReference` run **for
  every comment of the thread**, bracketing the same range, which is what Word writes. This crate's
  own reader puts it back together: the first `commentRangeEnd` places the root over the covered
  inlines, and the reply's is recognised through `CommentIndex::replies` and folds back into it.
- `word/comments.xml` carries the thread **flat**, because WordprocessingML has no nesting there,
  and `word/commentsExtended.xml` rebuilds it. That part is written **only when the document says
  something `comments.xml` cannot** — a thread or a resolved mark; flat, open comments are what
  every producer that is not Word writes, and they read back as exactly that.
- **`w14:paraId` is allocated**, sequentially from `0x10000001`. `0x00000000` is reserved and Word
  treats a value at or above `0x80000000` as invalid, so the range is the safe middle; sequential
  rather than hashed so that the same document written twice is the same document.
- **A reply the model added** is written exactly like one that was read: its own `w:id`, its own
  `w14:paraId`, and a `w15:paraIdParent` naming the paragraph of the comment it answers. Word joins
  on the parent's **last** paragraph id and so does this.
- `commentsExtended` is a Microsoft extension whose relationship type is **not** under the ECMA-376
  namespace, so `WriterCtx::add_rel_kind` states the whole URI. A writer that had reused the
  shorthand would have produced `…/officeDocument/2006/relationships/commentsExtended`, which Word
  does not recognise, and nothing in this repository would have failed.

## Defects found

1. **`w:annotationRef` became a raw fragment.** `crates/docsai-office/src/docx/body.rs:767` (before
   this change) ignored `w:footnoteRef` and not `w:annotationRef`, so the comment's own mark — which
   Word writes inside every `w:comment` and which says nothing the model does not already know from
   *being* a comment — fell through to the raw branch. Reproduction before this commit: read any
   `.docx` Word wrote with a comment in it and every comment's body opens with a
   `::: {.raw} <w:annotationRef/> :::` block and a `RawBlockDropped`-class warning. **Fixed**, in
   the same arm as `w:footnoteRef`.
2. **A selection lost the comment it was under review by.** `crates/docsai-convert/src/select.rs`,
   `select()` — the container is written outside every fragment, so a selection carried the anchor
   and nothing else. Reproduction before this commit:
   `docsai read corpus/docx/comments-review.docx --select n2`. **Fixed** (defect 2 above).
3. **`***bold italic***` inside `{.underline}` does not survive a docx round trip.** Found while
   building the fixture, unrelated to comments, **not fixed**. Reproduction: a `w:r` whose `w:rPr`
   is `<w:b/><w:i/><w:u w:val="single"/>` reads as `[***text***]{.underline}` and the second DocMark
   pass produces `[**\*text**]{.underline}[\*]{.underline}` — the emphasis markers are escaped and
   the span is split. The fixture uses `<w:b/><w:u/>` instead, so the defect is recorded rather than
   carried into a gate that would fail for a reason nothing to do with this increment.
4. **`docsai convert` writes its sidecar assets beside the *input*, not beside the output.** Already
   recorded by [[78-phase-16-chart-write]]; still true, and it littered `corpus/pptx/` again during
   the hand demonstration. Not this increment's to fix.

## Refused

- **Reading PowerPoint's modern comments** (`modernComment_*.xml`), which stays 16-E's named
  limitation. The parts are preserved and the refusal is specific; writing a `p188:cmLst` would mean
  modelling a schema ECMA-376 does not define.
- **Writing `w:annotationRef` into the comments this writer produces.** Word writes one, styled
  `CommentReference`. Emitting a run that names a character style the document may not define is
  worse than omitting a mark whose only job is to be the comment's own bullet; LibreOffice and our
  own reader show the comments correctly without it.
- **Rewriting `p:cmAuthor@lastIdx` for authors this write did not touch.** The value is a hint
  PowerPoint keeps for allocating the next `idx`; touching an author's row to correct it would move
  bytes in a part that has no reason to move.
- **A `comments` counter in `ConversionStats`.** 16-E refused it because another agent held
  `docsai-convert`; the reason now is the one it gave second, and it still stands — it would move
  every `.expected.inspect.json` in the corpus for a number no gate reads.

## Verification

- `cargo test --workspace`: **776 passed, 0 failed, 1 ignored**.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`:
  clean.
- `python3 corpus/generate.py --check`: exit 0, **143** generated files.
- `python3 corpus/schema_gate.py`: **21 decks, 42 packages, 363 parts validated, 0 invalid, 0 not
  measured**. `edit_comments` was added to the gate for the same reason 16-C added
  `bump_chart_values`: the comment parts are preserved, so nothing else the gate does makes the
  writer touch them, and a part the gate never sees is a part whose schema nobody checks. It now
  validates `ppt/comments/comment1.xml`, `comment2.xml` and `ppt/commentAuthors.xml` on both
  variants.
- `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer`:
  **21 decks, 21 round trips identical by bytes, 67 pages compared, 0 failures, 0 not measured**.
- **The edit path end to end, outside the suite.** `comments-slide.pptx` → `.dmk.md`, one comment's
  text retyped by hand, converted back. The member set is identical (21 members), exactly **one**
  member differs — `ppt/comments/comment1.xml` — and inside it exactly one `p:text`: the second
  comment, both `p:pos`, both `authorId`, both `idx`, the namespace declarations and the XML
  declaration are byte for byte the originals. Our own reader reads the new text back out, and
  `xmllint` against the vendored transitional schema says `validates`.
- **The docx edit path, outside the suite.** `comments-review.docx` → `.dmk.md`, one comment's text
  retyped, back to `.docx`, and to DocMark again: the two bodies are identical. LibreOffice opens
  the written `.docx` and shows four annotations with the right authors, initials, dates and text,
  the reply among them.

## What this leaves for 16-I

- The residue and token budgets moved with the new fixture (above); neither ceiling did.
- Defect 3 above (`***text***` under `{.underline}`) is open and belongs to nobody yet.
- Risk P2 is still OPEN. Nothing here needs PowerPoint to be re-run: every deck of the corpus is
  still byte-identical through a round trip, so the P2 form is re-runnable at any time.

## Next

16-I — the phase closes.
