# 04 — Considerations for the next phases

What the following phases will already find solved, what awaits them, and the traps that are
already known.

---

## Phase 2 — DocMark → DOCX writing + round-trip

> **Status: implemented** in-tree (`parse` / `parse_with_base`, `write_docx`,
> `convert` DocMark↔DOCX, CLI `roundtrip`). Remaining polish is fidelity on
> complex drawings, full footnote body rewrite, and floating anchors.

## Phase 2 (original plan) — DocMark → DOCX writing + round-trip

This is the immediate phase. It closes the cycle and builds the fidelity infrastructure.

### What is already done for it

| | |
|---|---|
| **The spec is frozen** | v1.0, with the changelog in §10. The parser has a fixed contract to work against, not a moving draft |
| **The serializer is deterministic** | The idempotence test `serialize(parse(md)) == md` already has half the equation guaranteed |
| **Lengths do not lose precision** | Every written length re-reads exactly. Without this, the fidelity metric would have an artificial floor |
| **The IR is complete** | `Block::TextBox`, `Workbook`, sheet anchors… already exist, even if the docx reader does not produce all of them |
| **The invariant validator** | Already invoked on every conversion; the parser inherits that safety net |
| **Raw-blocks are exact** | They store the original bytes, so `format=ooxml` re-injection is copy and paste |
| **Goldens are the reference** | 14 documents with their expected DocMark: the parser has 14 known inputs and their expected IR |

### What needs to be built

1. **DocMark parser** (`docsai-docmark`): comrak + custom layer for `{...}` attributes and fenced
   divs `:::`. Comrak is already in the tree as a test dependency, so evaluation is done.
2. **docx writer** (`docsai-office`): IR → `document.xml`, `styles.xml`, `numbering.xml`, media and
   properties.
3. **`roundtrip` command** with structural diff of the normalized IR and per-category metric.
4. **Serializer idempotence test** over all goldens.
5. **Property testing**: random IR → md → IR must be the identity. The arbitrary IR generator
   **already exists** in `crates/docsai-model/tests/json_roundtrip.rs` and can be reused almost as-is.

### Traps already identified

- **Escaping must be reversible, not merely correct.** `escape()` escapes `\` first, so
  unescaping is deterministic. The `escaping_is_idempotent_in_shape` test fixes that property; the
  parser must be its exact inverse.
- **The three fidelity modes are not symmetric.** Only `full` is reversible. `standard` and
  `plain` lose information on purpose, and `roundtrip` only makes sense over `full`.
- **`[]{.empty}`, `[]{.break kind=page}`, and `{.field ...}` are custom syntax** on top of
  CommonMark: the parser needs them explicitly. They are in the spec §3.1, §3.2, and §10.
- **A list item never carries two `{...}` blocks.** `list=` goes inside the attribute block of the
  first item.
- **The docx writer must put absorbed cells back.** The IR marks `covered=true` and
  `colspan`/`rowspan` on the cell that opens the area; OOXML expects `w:gridSpan` and `w:vMerge`.
- **Text boxes are still raw-block.** If Phase 2 wants real `::: {.textbox}`, the reader must be
  extended first — it is noted in the plan.
- **Decide whether `docx-rs` works as a writer.** Still open. Spike R1 only closed reading.
- **Validating in Word and LibreOffice** is an acceptance criterion and there is no LibreOffice in
  the current CI environment: it must be planned for (manual checklist per release, or headless
  `soffice` on the Linux runner).

---

## Phase 3 — Spreadsheets (XLSX/XLS)

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

### What needs to be decided and built

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

## Phase 4 — ODF (ODT and ODS)

- `docsai-odf` is a skeleton with the `FORMATS` constant; the dependency rule is already enforced
  by the compiler.
- The known hard point is **de-automatizing styles**: ODF `office:automatic-styles` represent
  direct formatting and must be mapped to IR deltas. The “reference + delta” model is ready to
  receive them: `FontProps::minus()` is exactly the inverse operation.
- `detect()` already recognizes ODF by `content.xml` + `mimetype`; it only disambiguates `.odt`
  from `.ods` by extension, which will need refining by reading the `mimetype`.
- The image model already has the concepts ODF needs (`as-char`/`char`/`paragraph`/
  `page` → `Inline`/`Floating`, `fo:clip` → `CropRect`).

---

## Phase 5 — Legacy DOC

- `detect()` already recognizes the OLE2 container and disambiguates `.doc` from `.xls` by name;
  that remains to be replaced by reading the CFB directory.
- The two-level strategy from analysis §1.3 still stands: fallback to headless LibreOffice and a
  degraded native extractor.
- The `Warning::ImageGeometryDegraded` that the native extractor needs already exists and is
  already used (for VML), so the pattern is established.

---

## Phases 6 to 9 — Product, MCP, and hardening

- **CLI (Phase 6)**: `convert` and `formats` exist with `--fidelity`, `--assets-dir`, `--json`,
  `--strict`, `--verbose`, and the exit codes from architecture §5. Still missing: `inspect`,
  `roundtrip`, `--style-map`, stdin/stdout with `-`, batch processing, and `cargo-dist`.
- **MCP (Phase 7)**: `docsai-mcp` declares the four tools. The clean-stdout rule is already
  respected in the CLI (`tracing` always writes to stderr), so the automated test of that guarantee
  already makes sense.
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
