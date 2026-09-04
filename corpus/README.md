# Test corpus

Versioned test documents. **One trait per file**: when a golden fails, the failing
file points at the reader area that broke.

No document contains real or private data (`AGENTS.md` §6).

## How they are generated

Every file is produced by `corpus/generate.py`, with no dependencies beyond the
Python 3 standard library:

```bash
python3 corpus/generate.py          # regenerate everything
python3 corpus/generate.py --check  # fails if the tree is out of date (run by CI)
```

That the corpus is **generated rather than hand-drawn** is deliberate:

- The XML of each document lives in the generator, where it is reviewed in a
  normal `git diff`; a `.docx` made with Word is an opaque box in review.
- Packages are written with a fixed timestamp and member order, so regenerating
  produces byte-identical files and the repository does not accumulate binary
  noise.
- Media (PNG, GIF, EMF) are synthesised in pure Python, without Pillow, so the
  generator works the same on the three CI platforms.

The trade-off: these are *minimal* documents, not real Word documents. The
anonymised real-world documents required by Phase 1 (task 10) and the
performance and adversarial corpora of Phase 8 will be added separately; the
50-page performance test synthesises its own document at runtime
(`crates/docsai-convert/tests/goldens.rs`).

## Checking a package

Two scripts beside the generator check a package rather than build one. Neither is a dependency of
anything the project ships:

```bash
python3 corpus/opc_check.py deck.pptx   # structural: content types, r:ids, relationship targets
python3 corpus/schema_gate.py           # CI gate 1: markup, against the grammars
python3 corpus/render_gate.py           # CI gate 2: what a renderer makes of it
```

`opc_check.py` asks whether a package hangs together; `schema_gate.py` asks whether the markup is
legal, validating with `xmllint` against the grammars vendored in [`schemas/`](../schemas/). They are
complementary, and a package can pass one and fail the other. See `kb/70-phase-15-xsd.md` and
`kb/96-phase-18-corpus.md`.

### What each gate covers, per format

Read this before assuming a format is checked. The two gates measure different formats to different
depths, and the honest table is shorter than the excuses.

| format | `schema_gate.py` | `render_gate.py` |
|---|---|---|
| `pptx/` | **Writer output**, against the ECMA-376 Transitional XSDs. Every deck is round-tripped twice — edited on its preserved package, and with `skeleton:` removed so the whole package is generated — and every part whose bytes the writer changed or created is validated. `mc:AlternateContent` gets ECMA-376 Part 3 fallback selection first | **Round trip**, against the deck it came from. All 21 decks round-trip byte-identical today, which a render can only agree with, so what is rendered is the *generated* package: same page count, no page that lost its ink |
| `odp/` | **The fixtures and the writer's output.** The fixtures against the OpenDocument 1.3 RELAX NG, with foreign markup (LibreOffice's `officeooo:` slide comment) removed first the way ODF conformance says a consumer ignores it; then, as the OOXML arm does, every deck retitled through DocMark, written back on its preserved package, and every part whose bytes changed validated | **The fixtures and the round trip.** LibreOffice opens every fixture, draws exactly the number of `draw:page` it declares, and leaves ink on each; and the round trip is compared on the pptx arm's comparator, which all 17 pass byte for byte |
| cross (`pptx/` ⇄ `odp/`) | **Every deck of each corpus written in the other dialect** (18-E) and validated against the other grammar, diffed against the embedded template each was generated on — a cross conversion never splices, because a package of the wrong dialect cannot be spliced into | **Every crossed package opened.** Not compared against the deck it came from: the two dialects resolve through different masters and different default fonts, so a pixel diff would be measuring the template. What is asserted is that LibreOffice opens the crossed package, draws the number of pages the package declares, and leaves ink on each |
| `docx/`, `xlsx/` | Nothing. Neither writer's output is schema-validated | Nothing |
| `odt/`, `ods/` | Nothing — see below | Nothing |
| `doc/`, `ppt/` | Nothing, and nothing is owed: these convert *out of* the legacy format only, and there is no markup this project writes | Nothing |

Two of those blanks are carried defects rather than decisions:

- **Spreadsheets have no gate at all**, neither `xlsx/` nor `ods/`. Carried from Phase 17
  (`kb/93-phase-17-close.md`) and not closed here.
- **`odt/` and `ods/` are one glob away from the ODF arm** and are left out of it because switching
  it on today reports exactly one failure: `odt/images-transformed.odt` puts `draw:rotation-angle`
  on `style:graphic-properties`, which ODF 1.3 has nowhere — a shape is rotated by `draw:transform`
  on the shape — and `crates/docsai-odf/src/styles.rs:187` is the code that reads it, so correcting
  the fixture drops the rotation from its golden. That is reader work.

## Golden files

Each corpus package (`docx/`, `xlsx/`, `odt/`, `ods/`, `pptx/`, `odp/`, and the
degraded `doc/` fixtures) has its expected DocMark beside it as
`<name>.expected.dmk.md` when a golden is maintained. The decks of `pptx/` and
`odp/` also keep an `<name>.expected.inspect.json`: that one pins the IR seen
from outside, the DocMark golden pins the text a person edits, and the two
answer different questions. The two deck corpora are pinned by the **same**
writer — a `.odp` and a `.pptx` differ in their front matter and nowhere else
in the body — which is what `goldens.rs` asserts directly. They are compared by the tests in
`crates/docsai-convert/tests/goldens.rs` (OOXML/ODF) and
`crates/docsai-convert/tests/doc_phase5.rs` (legacy `.doc`). To update OOXML/ODF
goldens:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens
```

The resulting diff **is reviewed by hand** before confirming: a golden updated
without looking is a test that has stopped checking anything.

`token-budget.md` is a golden too — what every document here costs an LLM at each
fidelity level (Phase 10). It is regenerated with the same flag on its own test,
and an update that inflates the corpus total by more than 5 % is refused unless
`DOCSAI_ACCEPT_TOKEN_INFLATION=1` says the price is intended:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test token_budget
```

## Text documents (`docx/`)

| File | Trait it isolates |
|---|---|
| `basic-text.docx` | Paragraphs, manual line break, empty paragraph and characters that Markdown escapes |
| `basic-styles.docx` | Bold, italic, strike, underline, colour, highlight, font and size, sub/superscript, hyperlink, alignment and indents |
| `nested-lists.docx` | `numbering.xml` with three numbered levels and two bullet levels; tree rebuild from `(numId, ilvl)` pairs |
| `table-simple.docx` | Regular table with table style and grid |
| `table-merged.docx` | `gridSpan` and `vMerge` → `colspan`/`rowspan` and absorbed cells |
| `images-inline.docx` | `wp:inline` images: PNG with alt/title/name, GIF between text, EMF vector |
| `images-floating.docx` | `wp:anchor`: margin-relative offsets with `wrapSquare`, symbolic page-relative alignment with `wrapTopAndBottom`, and watermark with `behindDoc` |
| `images-transformed.docx` | 45° rotation, `a:srcRect` crop with `a:ln` border, H+V flip and scale ≠ 100 % |
| `images-duplicated.docx` | The same bitmap in three distinct package parts with different geometries: tests `AssetStore` deduplication |
| `images-vml.docx` | `w:pict` with legacy VML (documents converted from `.doc`) |
| `headers-footers.docx` | `sectPr` with default and first-page header, footer with `PAGE`/`NUMPAGES` fields, two columns and `titlePg` |
| `footnotes.docx` | `footnotes.xml` with two notes, one with inner formatting |
| `custom-styles.docx` | Custom style, inherited style with direct delta, character style and custom document properties |
| `redundant-formatting.docx` | Formatting the inheritance chain already implies, at every level Word produces it: runs whose `w:rPr` repeats their paragraph style, `w:pPr` repeating the same style, a derived style repeating its parent, and a heading whose `pStyle` its level already names. None of it may reach the DocMark (spec §3.1). Without this fixture the delta emission of Phase 11 C is measured on documents that have nothing to drop |
| `repeated-formatting.docx` | The other half of the case above: the same direct formatting applied by hand twelve times, which **no** style implies, so the economy rule has to keep every copy. It is what the attribute-set dictionary (spec §3.7) exists to compress — two patterns interned, 725 → 616 tokens at `full`. Without this fixture the dictionary is measured on a corpus where nothing repeats |
| `fields-raw.docx` | `w:sdt` content control, complex `TOC` field, simple `DATE` field, and block-level OMML maths — the one element here with no DocMark representation, so it is the corpus's raw-block. Its bytes live in `docx/assets/_raw/raw-0001.xml` (spec §7 sidecar); that file is part of the corpus, and the golden round trip fails without it |
| `long-report.docx` | Not a trait but a **size**: 12 sections of ordinary prose with three heading levels (~9 000 tokens). Every other fixture is too small for a cost measurement to mean anything — on a 700-token file the outline *is* the document. This is what `docsai tokens` / `docsai outline` are held to (Phase 10: the outline must stay under 5 % of the document) |

## Legacy Word binary (`doc/`)

Phase 5 fixtures for the native degraded MS-DOC path. Produced as CFB packages by
`docsai_office::doc::test_fixture` and embedded (base64) in `generate.py` so the
generator stays dependency-free.

| File | Trait it isolates |
|---|---|
| `basic-text.doc` | Piece-table Unicode text split on paragraph marks |
| `encrypted.doc` | FIB `fEncrypted` → clear `ReadError::Encrypted` |
| `basic-text.expected.dmk.md` | Golden DocMark for the native path (`--use-loffice never`) |

## Legacy PowerPoint binary (`ppt/`)

Phase 19 fixtures, for a format this project **reads and never writes**. Nothing here round-trips,
and nothing here ever will: `.ppt` converts out of the legacy format only.

| File | Trait it isolates |
|---|---|
| `basic-slides.ppt` | Two slides, title and body — the legacy twin of `pptx/basic-slides.pptx`, so «fidelity equivalent to the pptx path» is measurable against a deck the corpus already pins with a golden |
| `mixed-media.ppt` | Three slides, titles, bullets at two levels, and a picture: slide boundaries, text extraction, and a non-empty `Pictures` stream (225 bytes of BLIP) that the text path never touches |
| `mixed-media.source.pptx` | The `.pptx` the deck above was converted from. **Not** a corpus deck: it lives here rather than in `pptx/` so that no gate reads it, and it exists so the recipe below is runnable and the legacy deck's content is reviewable as XML |
| `truncated.ppt` | A `PowerPoint Document` stream cut in half, in a compound file that stays valid — see below |
| `encrypted.ppt` | `CurrentUserAtom.headerToken` = `0xF3D1C4DF`, [MS-PPT] 2.3.2's «this document is encrypted» |

### How they are produced, and what that costs

A `.ppt` is an OLE2 compound file, not a ZIP, and nothing in this repository writes one. So the
corpus's own rule — every byte comes from `generate.py`, and `--check` proves the checked-in tree is
what it produces — is met two different ways here, and both are stated rather than implied.

**The two ordinary decks are recorded, not synthesised.** Their bytes were produced once by headless
LibreOffice (`--convert-to ppt`) and live in `generate.py` zlib-compressed and base64-encoded, the
way the legacy `.doc` fixtures already live there. `--check` **regenerates** them — decompressing a
constant is deterministic everywhere — so the digest is fixed by `generate.py` and not by the
machine. **LibreOffice is a dependency of _refreshing_ that constant, not of running the generator
or any gate.** The recipe is in `generate.py` beside the constants; a refresh under a different
LibreOffice produces a different constant, which is a reviewed change to a source file rather than a
machine-dependent digest.

That LibreOffice's `.ppt` output is byte-deterministic was **measured before it was relied on**, not
assumed: LibreOffice 24.2.7.2 wrote the same bytes twice in a row, again ninety-five seconds later,
and again on a freshly created user profile. It embeds no timestamp and no UUID.

**What is written is not quite what LibreOffice wrote**, and the difference is one derivation the
generator owns. LibreOffice embeds a `PIDSI_THUMBNAIL` — property 17, `VT_CF`, 588 344 bytes of
preview bitmap — in `\5SummaryInformation`, and there is no way to switch it off from the command
line (`Common/Save/Document/GenerateThumbnail` governs the ODF package thumbnail; setting it in the
profile changes nothing here). It is optional in [MS-OSHARED] 2.3.3.2.1 and no part of this phase
reads it, so `_ppt_deck()` removes the property — rebuilding the property set with its count and
offsets restated, not blanking it — and then compacts the container, renumbering the sectors still
in use into a dense run and truncating. **608 KB becomes 23 KB.**

Every other stream is untouched: `PowerPoint Document`, `Pictures`, `Current User`, `\1CompObj`,
`\1Ole` and `\5DocumentSummaryInformation` come out byte for byte as LibreOffice wrote them, and
the generator asserts exactly that. Verified rather than assumed: the `cfb` crate opens the result,
and LibreOffice converts it back to `.pptx` with every slide, every line of text and the picture.

The cost that remains: the bytes are **not reviewable in a diff** the way a generated `.pptx`'s XML
is. Provenance written down is the compensation, and it is weaker. `corpus/ppt/` is **116 KB** in
total.

**The two fixtures that exist to be refused are derived**, by the [MS-CFB] surgery in `generate.py`.
Every byte in which they differ from `basic-slides.ppt` is computed by this repository:
`encrypted.ppt` differs in exactly **four** (asserted), `truncated.ppt` in the region it cuts.
Stripping the thumbnail moves no offset either of them depends on — those are offsets *inside* the
`PowerPoint Document` stream, whose content is unchanged — so the numbers below are the same before
and after the strip, and the generator re-checks them.

### Why the truncation is dangerous rather than decorative

`truncated.ppt` is not a file cut short with `head -c`. The compound file is **intact and
self-consistent** — the `cfb` crate opens it without a word and hands back a 5 632-byte
`PowerPoint Document` stream — and what is gone is the second half of the stream that the format's
own bookkeeping still points into:

- `CurrentUserAtom.offsetToCurrentEdit` still addresses the `UserEditAtom` at 10 946, **5 314 bytes
  past the end** of what exists. A reader that seeks there without a bounds check runs off the end.
- The cut lands **inside** a record whose eight-byte header survives at 5 300 and declares a length
  reaching to 7 164, so a walk that trusts `recLen` overruns on its own, with no seek at all.

Both properties are **asserted in the generator**, so a refreshed constant that happened to make the
cut land tidily between two records fails there rather than producing a fixture that tests nothing.

### The honest caveat on `encrypted.ppt`

The content behind that flag is **not ciphertext**. Nothing in this tree can write RC4-encrypted
binary PowerPoint, and LibreOffice cannot either — `--convert-to ppt` with `EncryptFile` and a
password was tried and produced a byte-identical plaintext file. What the fixture carries is the
*declaration* a conforming reader is required to honour before it parses anything else. It proves
the refusal — which is what it is for, and since 19-E the refusal is real: `docsai convert
corpus/ppt/encrypted.ppt` exits 2 with a message naming the token, on all three `--use-loffice`
values. It proves nothing about decryption, and nothing in Phase 19 claims otherwise.

### What can and cannot gate a `.ppt`

Named here rather than left silent, because Phase 17 carried forward exactly this defect for
spreadsheets:

- `schema_gate.py` — **not applicable**. It validates OOXML parts against the ECMA-376 XSDs. A
  `.ppt` is a binary record stream with no XML in it and no schema to validate against.
- `render_gate.py` — **not applicable**. Its property is a round trip, and `.ppt` is read-only by
  design. There is no written package to render against the one that was read.
- `opc_check.py` — **not applicable**. There is no OPC package.
- Golden DocMark — **not taken**. Both readers exist (19-B, 19-D) and neither has a `.expected.dmk.md`
  beside it, deliberately: the property worth pinning is not «this deck serialises to these bytes»
  but «the legacy deck says what its `.pptx` original says», and `mixed-media.source.pptx` and
  `pptx/basic-slides.pptx` are in the tree so that comparison can be made directly.
  `crates/docsai-convert/tests/ppt_phase19.rs` is where it is asserted, on both routes. A golden
  would additionally pin the *formatting* LibreOffice synthesises on export, which is LibreOffice's
  and would move under it.
- `generate.py` — **this is the gate**, and it is the only one. It asserts each recorded deck's
  `RT_Slide` count, its `Pictures` stream and its plaintext `headerToken`, and it asserts both
  dangerous properties of the truncation. `--check` runs the generators, so CI runs those
  assertions on every push.

## Spreadsheets (`xlsx/`)

Generated in Phase 0 so the corpus is complete; consumed by **Phase 3**, when
the `xlsx` reader exists.

| File | Trait it isolates |
|---|---|
| `values-types.xlsx` | The six cell types: integer, decimal, boolean, error, date (serial + `numFmt`) and inline string |
| `formulas-basic.xlsx` | Formulas with cached value, cell reference and a defined name |
| `formulas-shared.xlsx` | Shared formulas (`t="shared"`) and array formulas (`t="array"`) |
| `number-formats.xlsx` | Currency, date, percentage and thousands via `numFmtId` |
| `merged-cells.xlsx` | Horizontal and vertical `mergeCells`, and custom column width |
| `images-anchored.xlsx` | The three sheet anchors: `twoCellAnchor`, `oneCellAnchor` and `absoluteAnchor` |

## Adding a document

1. Write a `docx_<trait>()` function in `generate.py` and add it to `GENERATORS`.
2. Regenerate (`python3 corpus/generate.py`) and add the row to the table above.
3. Generate its golden and **review the diff**.
4. If the trait is not implemented yet, the golden will document the current
   degradation (a raw-block, for example). That is correct: it makes the gap
   visible.

## Text documents (`odt/`)

OpenDocument Text packages for **Phase 4**. Same traits as the docx set where
applicable; generated as ODF packages (content.xml / styles.xml / meta.xml).

| File | Trait it isolates |
|---|---|
| `basic-text.odt` | Paragraphs and plain text |
| `basic-styles.odt` | Character and paragraph direct formatting via automatic styles |
| `nested-lists.odt` | Nested numbered and bullet lists |
| `table-simple.odt` | Simple table |
| `table-merged.odt` | Column/row spans |
| `images-inline.odt` | `draw:frame` as-char images |
| `images-floating.odt` | Anchored frames with wrap |
| `images-transformed.odt` | Rotation / flip / clip |
| `headers-footers.odt` | Master-page header and footer |
| `footnotes.odt` | Footnote bodies |

## Spreadsheets (`ods/`)

OpenDocument Spreadsheet packages for **Phase 4**.

| File | Trait it isolates |
|---|---|
| `values-types.ods` | String, float, boolean, date cell values |
| `formulas-basic.ods` | OpenFormula with `of:` prefix |
| `merged-cells.ods` | Column/row spans and covered cells |
| `images-anchored.ods` | Frames anchored to cells / sheet |

## Presentations (`odp/`)

OpenDocument Presentation packages for **Phase 18**. Named after the `corpus/pptx` deck each one
mirrors, so that the cross-conversion matrix of 18-E pairs the two sides by name. Both gates cover
them — see *What each gate covers, per format* above for how deeply.

| File | Mirrors | Trait it isolates |
|---|---|---|
| `basic-slides.odp` | `basic-slides.pptx` | Two pages, title + outline. The floor of the format |
| `slide-order.odp` | `slide-order.pptx` | Page order **is** document order: ODF has no `p:sldIdLst` to disagree with the parts. `draw:name` is crossed on purpose, so a reader that sorts by name — as it must for `slideN.xml` — gets the deck backwards |
| `placeholders-cascade.odp` | `placeholders-cascade.pptx` | A title and an outline stating no geometry at all: everything from `style:presentation-page-layout` and the master page. **One** level of inheritance where OOXML has two |
| `placeholders-empty.odp` | `placeholders-empty.pptx` | An empty placeholder is content. Both spellings of emptiness: a `draw:text-box` with no paragraph, and one holding an empty `text:p` |
| `presentation-classes.odp` | — (ODF only) | The `presentation:class` vocabulary: `title`, `subtitle`, `outline`, `text`, `graphic`, `object`, `header`, `footer`, `date-time`, `page-number` — and two `outline` frames on one page, which ODF cannot tell apart and `p:ph`'s `idx` can. The mapping question 18-B has to settle, in a file |
| `bullets-levels.odp` | `bullets-levels.pptx` | Nested `text:list` with both bullet kinds. ODF nests a list *inside the item above it*; OOXML flattens the depth into `a:pPr@lvl` |
| `notes-speaker.odp` | `notes-speaker.pptx` | `presentation:notes` as a child of its page |
| `reading-order.odp` | `reading-order.pptx` | Document order that is not reading order, plus an explicit `draw:z-index` that disagrees with both — ODF states the stacking order where OOXML leaves it implicit in `spTree` order |
| `raw-preserved.odp` | `raw-preserved.pptx` | The four things with no IR node: `draw:g`, a `draw:custom-shape` whose `draw:enhanced-geometry` is the shape, a page transition as drawing-page properties, and an `anim:par` tree |
| `tables-simple.odp` | `tables-simple.pptx` | `table:table` inside a `draw:frame` |
| `images-anchored.odp` | `images-anchored.pptx` | A picture with explicit geometry — in centimetres, where the pptx side is in EMU |
| `shapes-geometry.odp` | `shapes-geometry.pptx` | `draw:rect` with rounded corners, a `draw:custom-shape` naming a preset through `draw:type`, and a `draw:line`. `draw:transform` states a rotation as an affine matrix where `a:xfrm@rot` states an angle |
| `charts-embedded.odp` | `charts-embedded.pptx` | An embedded chart sub-document (`Object 1/`), one series, with its numbers in the chart's own `local-table` |
| `charts-multi-series.odp` | `charts-multi-series.pptx` | Two series and a legend |
| `ole-embedded.odp` | `ole-embedded.pptx` | `draw:object-ole` with its replacement image under `ObjectReplacements/` |
| `comments-slide.odp` | `comments-slide.pptx` | A commented page. Carries `officeooo:annotation` under `draw:page` — LibreOffice's namespace, not ODF's, because that is the only spelling that both survives a renderer and appears in real decks. The generator docstring records all three placements and what each one did |
| `forty-slides.odp` | `forty-slides.pptx` | Not a trait but a **size**: forty pages |

### pptx traits with no `.odp` mirror

Each is absent for a reason in the format, not for lack of effort:

| pptx deck | Why there is no `.odp` |
|---|---|
| `notes-crossed.pptx` | `presentation:notes` is a *child* of its page, not a part bound by a relationship. Nothing can be crossed |
| `autofit-stale.pptx` | ODF has `draw:auto-grow-height` and `draw:fit-to-size`, and no cached `fontScale` — there is no stale number to go stale |
| `charts-stale-workbook.pptx` | An ODF chart holds its numbers in a `table:table` inside itself. There is no second copy, so there is nothing for a cache to disagree with |
| `smartart-fallback.pptx` | `mc:AlternateContent` is an ECMA-376 Part 3 mechanism. ODF's answer to unknown markup is foreign elements a consumer ignores, which is a different shape and is exercised by `comments-slide.odp` |
| `macro-enabled.pptm` | ODF has no macro-enabled variant of the MIME type: a macro lives in `Basic/` inside an ordinary `.odp`. The trait exists, the *format distinction* does not, and no fixture carries it yet |

### The cross-conversion matrix (18-E)

The pairing this table's «Mirrors» column states is not documentation alone: it is a constant in
`crates/docsai-convert/tests/cross_matrix.rs`, twenty-two cells wide — sixteen traits both corpora
carry, the five with no `.odp` above, and `presentation-classes.odp` with no `.pptx`. The test walks
it in both directions and asserts that every fixture of both corpora appears in exactly one cell, so
a fixture added here without a cell fails the build rather than quietly not being measured.

What each cell asserts is stated there and is deliberately not byte identity, which a cross
conversion cannot have: **the visible text survives as a multiset**, minus the lines the cell
declares lost, and **the warnings cover the losses exactly** — every warning declared and every
warning raised, matched both ways.

### Adding an `.odp`

Same recipe as the rest of the corpus, with one addition: `python3 corpus/schema_gate.py <name>` and
`python3 corpus/render_gate.py <name>` both take a fixture name, and a new fixture that neither
validates nor renders is a fixture that isolates nothing.
