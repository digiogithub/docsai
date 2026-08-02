# Spike R3 — XLSX writer strategy

**Date**: 2026-08-02  
**Phase**: 3  
**Status**: decided  
**Timebox**: evaluation against the Phase 3 corpus and the IR contracts in
`docs/architecture.md` / `docs/docmark-specification.md` §4.

## Question

Which crate should write `.xlsx` from the IR with high fidelity for values,
formulas, `numFmt`, cell styles, merges, column widths, defined names, and
sheet drawings (three anchor kinds)?

Candidates from `docs/technical-analysis.md`:

1. **`umya-spreadsheet`** — read+write with styles; whole-workbook in memory.
2. **`rust_xlsxwriter`** — mature pure-write API (formulas, formats); no edit path.
3. **Custom writer** on `zip` + hand-built SpreadsheetML (same stack as the
   Phase 1/2 docx path).

## Evaluation criteria

| Criterion | Weight | Notes |
|---|---|---|
| Styles + `numFmt` fidelity from IR | High | Acceptance: round-trip numFmt intact |
| Sheet drawings geometry | High | calamine/umya do not expose full SpreadsheetDrawingML |
| Shared / array formula metadata | Medium | IR keeps `shared_over` / `array_over` |
| Binary size / deps | Medium | Single reasonably small binary |
| Consistency with docx path | Medium | Agents already know `Package` + XML trees |
| Large sheet budget | Medium | 100k cells &lt; 3 s / &lt; 500 MB |

## Findings

### `umya-spreadsheet`

- Can write styles and many workbook features.
- Loads and rebuilds a heavy in-memory model; drawing geometry is incomplete
  relative to the IR (`SheetTwoCell` / `SheetOneCell` / `SheetAbsolute` with
  offsets and edit-as flags).
- Adds a large dependency surface for features docsai would still reimplement
  (drawings, exact raw re-injection patterns).

### `rust_xlsxwriter`

- Excellent write-only API for values, formulas and formats.
- Does **not** read; fine for regenerate-from-IR.
- Sheet drawings / full anchor model are limited or would still need a custom
  `drawing*.xml` path beside the crate.
- Would force two parallel XML worlds (crate-owned workbook parts + custom
  drawings), which is harder to keep deterministic.

### Custom `zip` + SpreadsheetML (chosen)

- Mirrors the closed R1 decision for docx: readers and writers own every byte
  that reaches the package.
- Reuses `Package`, relationship resolution, asset sniffing, and the existing
  meta/core props patterns from `docsai-office`.
- Sheet drawings map 1:1 onto the IR anchors already validated in
  `docsai-model::validate`.
- Deterministic part order and stable XML emission match golden / round-trip
  requirements.
- No new heavy dependency for the xlsx path. **`calamine` is used only for
  legacy `.xls` reading** (BIFF8), where a custom parser would be unjustified.

## Decision

**Write `.xlsx` with a custom OPC/SpreadsheetML writer** in
`docsai-office::xlsx::write`.

**Read `.xlsx` with a custom package reader** (values, formulas, styles,
merges, panes, drawings). Do **not** route xlsx reading through calamine:
style indexes, shared formula metadata and drawings need the raw parts
anyway, and a second parse would fight the 100k-cell budget.

**Read `.xls` with `calamine`** (read-only). Document write support as out of
scope in `docsai formats`.

## Consequences

- `docsai-office` gains an `xlsx` module (read + write) and a thin `xls`
  module (calamine).
- Spike acceptance for Phase 3 writer choice is closed by this document.
- If a future phase needs chart round-trip or pivot tables, re-evaluate
  before layering another full spreadsheet crate on top of this path.
