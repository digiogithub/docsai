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

## Golden files

Each corpus package (`docx/`, `xlsx/`, `odt/`, `ods/`, and the degraded `doc/`
fixtures) has its expected DocMark beside it as `<name>.expected.dmk.md` when a
golden is maintained. They are compared by the tests in
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
| `fields-raw.docx` | `w:sdt` content control, complex `TOC` field and simple `DATE` field |
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
