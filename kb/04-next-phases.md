# 04 — Considerations for the next phases

What the following phases will already find solved, what awaits them, and the traps that are
already known.

---

## Phase 2 — DocMark → DOCX writing + round-trip

> **Status: closed for the core path.** Details in
> [`06-phase-2-roundtrip-closure.md`](06-phase-2-roundtrip-closure.md). Remaining
> optional polish: proptest IR→md→IR and Word/LibreOffice open checklist.

### Delivered

| | |
|---|---|
| DocMark parser + serializer identity on all docx goldens | `serialize_parse_is_identity_on_docx_goldens` |
| DOCX writer with floating DrawingML, transforms, full footnotes | `docsai-office::write_docx` |
| CLI / pipeline `roundtrip` | Office→DocMark→Office→DocMark identity on full docx corpus |
| List nesting `ilvl`, table spans, fence chunking fixes | See kb/06 |

### Still open (non-blocking)

- proptest: random IR → DocMark → IR identity (generator already in model tests).
- External validation checklist in Word / LibreOffice (no soffice in CI yet).
- First-class text-box write (still flattened with warning; IR + spec ready).

### Traps that bit during residual closure

- Blank-line chunking must update `:::` fence depth **before** testing the blank.
- Nested lists need parse depth > 0 or every item is written as `ilvl=0`.
- GFM empty cells after `colspan`/`rowspan` are pads, not content — rebuild `covered`.
- Do not invent `w:start=1` when the IR left `start` unset.

---

## Phase 4 — ODF (ODT / ODS)

> **Status: closed for the core path.** Details in
> [`07-phase-4-odf.md`](07-phase-4-odf.md). Custom `docsai-odf` with de-automatization,
> OpenFormula preserved, corpus + goldens + convert wiring.

### Delivered (summary)

- ODT/ODS read and write, package rules (`mimetype` first/stored), draw frames.
- Convert `SUPPORT` and round-trip paths for both formats.
- Detection via package mimetype (`Certain`).

### Still open (non-blocking)

- LibreOffice open checklist; richer named-style / column-width fidelity.

---

## Phase 3 — Spreadsheets (XLSX/XLS)

> **Status: implemented** for the core path. Details in
> [`05-phase-3-spreadsheets.md`](05-phase-3-spreadsheets.md). Remaining: 100k-cell perf budget
> and Excel/LibreOffice open checklist.

### What is already done

- **The xlsx corpus exists**: `values-types`, `formulas-basic`, `formulas-shared`,
  `number-formats`, `merged-cells`, and `images-anchored` (all three anchors). Generated in Phase 0
  precisely so it would not have to be invented in a hurry.
- **The `Workbook` side of the IR is defined and tested**: `Sheet`, `Cell`, `CellValue`, `Formula`
  with its dialect, `NumFmt`, `CellRef` with A1 notation both ways, `ColProps`, `RowProps`,
  `Pane`, `DefinedName`. All with verified JSON round-trip.
- **The three sheet anchors** are in the image model and the validator already rejects using them
  outside a `Workbook`.
- **`float_roundtrip`** is enabled: numeric values are not corrupted when going through JSON.

### What was decided and built (summary; see kb/05)

1. **xlsx writer spike**: `umya-spreadsheet` versus `rust_xlsxwriter`. Criterion: which regenerates
   styles + `numFmt` from the IR with higher fidelity. Document in `docs/spikes/`.
2. **Sheet serialization to DocMark** (spec §4): the syntax is specified but **not implemented**.
   `docsai_docmark::serialize` today returns front matter and an explicit `Degraded` warning for
   `Document::Workbook`; that is the entry point.
3. **Reading `xl/drawings/drawing*.xml`** with custom `quick-xml`: neither `calamine` nor `umya`
   expose full geometry. The `docx/drawing.rs` module is the pattern to follow; the target model
   is the same.

### Traps already identified

- **Streaming.** The in-memory XML tree is comfortable for documents but the budget for a 100k-cell
  sheet is < 3 s and < 500 MB. Use `calamine` (lazy per sheet) for values and reserve the custom
  tree for small parts (`styles.xml`, `drawing*.xml`).
- **Dates are serial numbers plus format.** The IR stores `CellValue::DateTime` in ISO-8601 so that
  hand-editing DocMark cannot corrupt them; serial ⇄ ISO conversion both ways is the reader and
  writer responsibility.
- **In `two-cell` anchors, `width`/`height` are not serialized** (spec §4.1): size is defined by the
  grid. The attribute writer already accounts for this.
- **Formulas keep their dialect**; they are not translated (risk R5). `FormulaDialect` is already
  in the IR.

---

## Phase 5 — Legacy DOC

> **Status: closed for the core path.** Details in
> [`08-phase-5-legacy-doc.md`](08-phase-5-legacy-doc.md).

- `detect()` classifies OLE2 via the CFB directory (`WordDocument` vs `Workbook`/`Book`).
- Native degraded reader + `--use-loffice auto|never|require` are wired.
- Remaining polish: richer native structure (lists/tables), real LO-saved samples in CI.

---

## Phase 6 — Full CLI and distribution

> **Status: closed for the core path.** Details in
> [`09-phase-6-cli.md`](09-phase-6-cli.md).

- `inspect`, batch `--out-dir` (rayon), stdin/stdout `-`, `--style-map`, `--max-cells`,
  actionable error hints, CHANGELOG, and `cargo-dist` config are in tree.
- Remaining non-blocking: verify one-command installers on clean machines after the first
  tagged release; optional Homebrew/Scoop formulas.

## Phases 7 to 9 — MCP and hardening

- **MCP (Phase 7)**: `docsai-mcp` declares the four tools. The clean-stdout rule is already
  respected in the CLI (`tracing` always writes to stderr), so the automated test of that guarantee
  already makes sense. `inspect --json` shape is the contract for `inspect_document`.
- **Hardening (Phase 8)**: part of the work is ahead of schedule — decompression caps, XML depth
  limit, path sanitization, 900+ corrupt inputs in CI. Still missing: real `cargo-fuzz`, the full
  adversarial suite, benchmarks with `criterion`, and `cargo audit`/`deny`. The existing corpus
  serves as a seed for fuzzing.

---

## Known technical debt

None of these block anything, but they are worth noting:

| Item | Where | Note |
|---|---|---|
| Anonymized real documents | Phase 1, task 10 | The synthetic corpus covers traits one by one, but not real-world oddities |
| Text boxes as raw-block | `docx/drawing.rs` | The IR type and spec syntax exist; emit and rebuild are missing |
| `w:lvlOverride` not modeled | `docx/numbering.rs` | Emits `Warning::Degraded` |
| Comments ignored | `docx/body.rs` | Out of v1 scope; emits a warning |
| DrawingML effects | `docx/drawing.rs` | Detected and warned, but not yet dumped to `effects_raw` |
| In-memory XML tree | `office/xml.rs` | Revisit for large sheets in Phase 3 |
| FNV hash for assets | `model/assets.rs` | Sufficient today; revisit if the name becomes a trust boundary |

## Rules not to break

1. **Do not advance phases.** The plan order reflects real dependencies.
2. **Do not change the DocMark spec to make a test pass.** It is frozen at v1.0; any change bumps
   the front-matter version and documents the migration.
3. **Nothing is degraded silently.** Every loss is a typed `Warning`.
4. **Parsers never panic.** There is a test that checks this with 900+ corrupt inputs; keep it
   passing.
5. **The serializer is deterministic.** `BTreeMap` always, `HashMap` never, on any path that
   reaches the output.
