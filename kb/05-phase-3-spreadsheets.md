# 05 — Phase 3: Spreadsheets (XLSX / XLS ⇄ DocMark)

What landed for Phase 3, how it is wired, and the non-obvious decisions an agent should
know before touching the spreadsheet path again.

## Status

**Phase 3 core path is implemented** in-tree:

| Direction | Status |
|---|---|
| `.xlsx` → IR → DocMark | Yes (`docsai-office::read_xlsx` + `docsai-docmark` sheet writer) |
| DocMark → IR → `.xlsx` | Yes (`sheet_parser` + `docsai-office::write_xlsx`) |
| Office → DocMark → Office → DocMark | Yes (`docsai-convert::roundtrip_file` for xlsx) |
| `.xls` → IR → DocMark | Yes, read-only (`calamine` via `docsai-office::read_xls`) |
| `.xls` write | Out of scope (documented in the support matrix) |

Corpus goldens live next to the fixtures:

```text
corpus/xlsx/*.xlsx
corpus/xlsx/*.expected.dmk.md
```

`cargo test -p docsai-convert --test goldens` covers both the docx and xlsx corpora, plus an
xlsx round-trip idempotence test.

## Spike R3 decision

Documented in [`docs/spikes/R3-xlsx-writer.md`](../docs/spikes/R3-xlsx-writer.md):

- **Custom OPC + SpreadsheetML writer** (zip + `quick-xml` string building), matching the
  docx approach. Not `umya-spreadsheet` / `rust_xlsxwriter`.
- **Custom xlsx reader** as well — calamine does not expose style indexes, shared formula
  masters, or drawing anchors with the fidelity the IR needs.
- **calamine only for legacy `.xls`** read.

## Crate map

```text
docsai-office
  xlsx/mod.rs      read workbook, sheets, shared strings, dates, defined names
  xlsx/styles.rs   styles.xml → numFmt + synthetic font styles (stable ids)
  xlsx/drawing.rs  sheet anchors (two-cell / one-cell / absolute)
  xlsx/write.rs    IR → package (workbook, sheets, sst, styles, drawings, media)
  xls/mod.rs       calamine .xls → Workbook (values + formulas; no styles/drawings)

docsai-docmark
  sheet_writer.rs  Workbook → DocMark §4 (H1 .sheet, GFM table, cell-meta, sheet-images)
  sheet_parser.rs  DocMark §4 → Workbook
  frontmatter*.rs  workbook.active-sheet + defined-names

docsai-convert
  SUPPORT matrix   xlsx read/write = true; xls read = true, write = false
  pipeline         convert_file + roundtrip_file dispatch on Xlsx
```

## Behaviour notes

### Values and types

- Cell display values go in the GFM table; type/format/formula live in `::: {.cell-meta}`.
- Numbers, bools, errors, ISO-8601 dates, and text round-trip.
- Excel serial dates use the 1899-12-30 epoch (`excel_serial_to_iso` / `iso_to_excel_serial`).
- Table-cell escaping does **not** apply CommonMark line-start rules, so values like
  `3.14159` and `#DIV/0!` are not backslash-escaped.

### Formulas

- Stored **without** a leading `=`, dialect defaults to OOXML.
- Shared formulas: reader expands every member with relative A1 translation from the master;
  writer re-emits `t="shared"` with a per-range `si` and master text only on the origin cell.
- Array formulas: `t="array"` + `array-over=` in cell-meta.

### Styles

- Non-default fonts become character styles with **stable ids** derived from a font
  fingerprint (`F{hex}`), not `CellXf{index}`, so DocMark `style=` survives round-trip.
- Unused stylesheet entries (common in the generated corpus shared `styles.xml`) are pruned
  after read so orphan catalogue entries do not appear in DocMark.

### Merges

- `merge=true` is recorded on the range; value and other meta apply only to the **top-left**
  cell. Expanding style to every covered cell broke serialize→parse→serialize stability.

### Images

- All three anchors serialize per spec §4.1; two-cell omits width/height.
- Media bytes are copied without recompression.
- Charts remain out of scope (raw-block + warning when encountered).

### Front matter

```yaml
workbook:
  active-sheet: "Sales"
  defined-names:
    TOTAL: "Sales!$D$10"
```

`source-format: xlsx` (or body headings with `{.sheet}`) selects the workbook parser.

## Support matrix (`docsai formats`)

| Format | Read | Write |
|---|---|---|
| docx | yes | yes |
| xlsx | yes | yes |
| xls | yes | no |
| doc / odt / ods | no | no |
| docmark | yes | yes |

## Tests to keep green

```bash
cargo test --workspace
cargo test -p docsai-convert --test goldens
cargo test -p docsai-office --lib xlsx
cargo run -p docsai-cli -- convert corpus/xlsx/values-types.xlsx -o /tmp/out.dmk.md
cargo run -p docsai-cli -- roundtrip corpus/xlsx/formulas-shared.xlsx
```

Regenerate xlsx goldens only after reviewing the diff:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens
# or per-file:
cargo run -p docsai-cli -- convert corpus/xlsx/NAME.xlsx -o corpus/xlsx/NAME.expected.dmk.md
```

## Known gaps / follow-ups

- **100k-cell performance budget** and **Excel/LibreOffice open checklist** are still open in
  `docs/development-plan.md` Phase 3 acceptance.
- Cell fills/borders are not yet first-class in the IR catalogue (font-focused styles only).
- Cross-sheet formula evaluation is not performed (formulas are preserved as text).
- `.xls` has no style/drawing fidelity; writing `.xls` is explicitly out of scope.
- ODS (Phase 4) should reuse DocMark §4 and the IR; only the ODF package layer is new.

## Files touched (high level)

- `docs/spikes/R3-xlsx-writer.md` — writer strategy
- `crates/docsai-office/src/{xlsx,xls,package,xml,lib}.rs`
- `crates/docsai-docmark/src/{sheet_writer,sheet_parser,parser,frontmatter_parse,escape,lib}.rs`
- `crates/docsai-convert/src/{lib,pipeline}.rs` + `tests/goldens.rs`
- `corpus/xlsx/*.expected.dmk.md`
- Root / office `Cargo.toml` — `calamine = "0.26"`
