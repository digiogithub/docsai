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
python3 corpus/schema_gate.py           # CI gate 1: the .pptx parts we write, against the XSDs
```

`opc_check.py` asks whether a package hangs together; `schema_gate.py` asks whether the markup this
project *writes* is legal ECMA-376, validating with `xmllint` against the schemas vendored in
[`schemas/`](../schemas/). They are complementary, and a package can pass one and fail the other.
`schema_gate.py` round-trips every deck in `pptx/` twice — edited on its preserved package, and with
its `skeleton:` removed so the whole package is generated — and validates every part whose bytes the
writer changed or created. See `kb/70-phase-15-xsd.md`.

## Golden files

Each corpus package (`docx/`, `xlsx/`, `odt/`, `ods/`, `pptx/`, and the
degraded `doc/` fixtures) has its expected DocMark beside it as
`<name>.expected.dmk.md` when a golden is maintained. The decks of `pptx/` also
keep an `<name>.expected.inspect.json`: that one pins the IR seen from outside,
the DocMark golden pins the text a person edits, and the two answer different
questions. They are compared by the tests in
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

The costs, both real:

- The bytes are **not reviewable in a diff** the way a generated `.pptx`'s XML is. Provenance
  written down is the compensation, and it is weaker.
- The files are **large for what they carry**: ~600 KB each, of which about 95 % is a thumbnail
  LibreOffice embeds in `\5SummaryInformation` (`PIDSI_THUMBNAIL`, 588 580 bytes) and which no
  command-line option suppresses. Removing it would mean rewriting the compound file, i.e. writing
  one, which this phase does not do.

**The two fixtures that exist to be refused are derived**, by the [MS-CFB] surgery in `generate.py`.
Every byte in which they differ from `basic-slides.ppt` is computed by this repository:
`encrypted.ppt` differs in exactly **four** (asserted), `truncated.ppt` in the region it cuts.

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
the refusal. It proves nothing about decryption, and nothing in Phase 19 claims otherwise.

### What can and cannot gate a `.ppt`

Named here rather than left silent, because Phase 17 carried forward exactly this defect for
spreadsheets:

- `schema_gate.py` — **not applicable**. It validates OOXML parts against the ECMA-376 XSDs. A
  `.ppt` is a binary record stream with no XML in it and no schema to validate against.
- `render_gate.py` — **not applicable**. Its property is a round trip, and `.ppt` is read-only by
  design. There is no written package to render against the one that was read.
- `opc_check.py` — **not applicable**. There is no OPC package.
- Golden DocMark — **not yet**. There is no `.ppt` reader until 19-B/19-D; the goldens arrive with
  the reader that produces them.
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
