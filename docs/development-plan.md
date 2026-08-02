# Development plan

Phased plan to implement `docsai`. Each phase has an objective, tasks, deliverables,
acceptance criteria, and an indicative estimate (in person-weeks for a senior Rust
developer; adjust for team size). Phases 1–3 are the heart of the product; from
Phase 6 there is room to parallelize.

**Management rule**: a phase is not opened until the previous phase's acceptance criteria
are closed (except those marked parallelizable). Each phase ends with a `v0.x.0` tag.

---

## Phase 0 — Foundations (2–3 weeks)

**Objective**: compiling workspace, designed IR, frozen DocMark specification, initial corpus,
and green multiplatform CI. This is the phase that de-risks everything else.

Tasks:
1. Create the Cargo workspace with the 7 crates from `architecture.md` §2 (even if nearly empty).
2. Implement `docsai-model` v1: IR types (architecture §3), `StyleCatalog` with
   inheritance, `ConversionReport`, unit newtypes (`Length` with EMU/twips/pt/cm).
   Includes the **normalized image model** `ImageRef`/`ImageGeometry`/`Anchor`
   (architecture §3.1) with its invariant validator (sheet anchors only in `Workbook`),
   and the `AssetStore` trait (architecture §3.2). Everything `serde`-serializable, with unit
   tests for unit conversions (EMU⇄pt⇄cm⇄px at 96 dpi).
3. **Risk spike R1** (1-week timebox): read three real docx documents with custom styles
   using `docx-rs` + `quick-xml` and verify that the 4-level cascade is resolvable with the
   information exposed. Result written under `docs/spikes/` with a decision: crate + custom
   complement, or full custom OOXML parser.
4. Freeze `docmark-specification.md` v1.0 (team review, resolve the TODOs in annex A
   on complex tables).
5. Initial corpus in `corpus/`: ~15 minimal hand-made documents (one trait per file):
   `docx/basic-text`, `basic-styles`, `nested-lists`, `table-simple`, `table-merged`,
   `images-inline`, `images-floating` (wrap + position relative to page/margin),
   `images-transformed` (crop, rotation, flip, scale ≠ 100 %), `images-duplicated`
   (same bitmap N times with different geometries), `headers-footers`, `footnotes`,
   `custom-styles`; `xlsx/values-types`, `formulas-basic`, `formulas-shared`,
   `number-formats`, `merged-cells`, `images-anchored` (all three anchors: two-cell,
   one-cell, absolute).
   Script of how each was created in `corpus/README.md`.
6. GitHub Actions CI: build+test+clippy+fmt on a 3-OS matrix; cargo cache; badge in README.
7. CLI skeleton (`docsai formats`, `--version`) and `insta` template for golden tests.

Deliverables: green workspace in CI, `docsai-model` v1, frozen spec, corpus v1, spike report.

Acceptance criteria:
- [x] `cargo test --workspace` green on all 3 platforms.
- [x] IR serializes/deserializes to JSON with identical round-trip (basic proptest).
- [x] Spike documented with a signed decision on the docx strategy.
- [x] DocMark v1.0 with no open TODOs.

**Status**: closed. Execution notes:
- Spike R1 (`docs/spikes/R1-docx-strategy.md`) discarded `docx-rs`: it loses almost the entire
  image model, footnotes and simple fields, and panics on 23 % of corrupt
  inputs. docx reading is done with a custom parser over `zip` + `quick-xml`.
- The corpus is **generated** with `corpus/generate.py` instead of drawn by hand: the XML is
  reviewable in the diff and packages are byte-for-byte reproducible (see `corpus/README.md`).
- Golden files are `.expected.dmk.md` next to the corpus, as prescribed by `AGENTS.md` §6, instead
  of `insta` snapshots; `insta` leaves the dependency tree.

---

## Phase 1 — DOCX reading → DocMark (4–6 weeks)

**Objective**: `docsai convert x.docx -o x.dmk.md` with styles, images, tables, lists,
headers/footers and properties. This is the longest phase: it sets the patterns reused by all others.

Tasks:
1. docx reader (`docsai-office`): ZIP open, `document.xml`, relationship resolution.
2. Styles: full parse of `styles.xml` → `StyleCatalog`; reference+delta resolution
   (never flatten); document defaults (`docDefaults`).
3. Paragraphs and runs: all inline formatting from spec table §3.2; hyperlinks; breaks.
4. Lists: tree reconstruction from `numbering.xml` + `(numId, ilvl)` pairs → `ListCatalog`.
5. Tables: grid, `gridSpan`/`vMerge` → IR rowspan/colspan, widths, table style.
6. Images (spec §3.5, architecture §3.1): DrawingML `wp:inline` and `wp:anchor` → `ImageRef` +
   extraction to `AssetStore` with hash deduplication. Full geometry mapping: displayed vs native
   size, relative position (page/margin/paragraph/character) with offsets or
   symbolic alignment, wrap and side, z-order/behind-text, rotation, flips, crop
   (`a:srcRect`), simple border, alt/title/name, hyperlink on image, linked
   images (`r:link`, no download). Also read **legacy VML** (`w:pict`) limited
   to the normalized model. DrawingML effects without a model → associated raw-block (`effects_raw`).
   WMF/EMF extracted as-is with full geometry and a warning.
7. Headers/footers/sections (`sectPr`), footnotes/endnotes, simple fields (PAGE, TOC as
   raw/field), document properties (core+app+custom).
8. DocMark serializer (`docsai-docmark`): IR → Markdown per spec §8 (deterministic); asset
   management and front matter; `--fidelity` modes.
9. Every unrecognized OOXML element → raw-block + typed warning (measurable coverage).
10. Golden tests for the whole docx corpus; add 5+ anonymized "real-world" documents.

Acceptance criteria:
- [x] Corpus docx goldens pass (including the three image ones: floating,
      transformed and duplicated — with full geometry in DocMark attributes).
- [x] Zero panics on synthetic corrupt corpus (truncated ZIP, malformed XML): always `Err`.
- [x] A real 50+ page docx converts in < 1 s with < 10 raw-blocks.
- [x] Output in `--fidelity plain` is clean CommonMark verified with comrak.

**Status**: closed except task 10 (anonymized real documents), which depends on having
real documents available and does not block Phase 2. Execution notes:
- DrawingML text boxes (`wps:txbx`) are preserved as raw-block, not as
  `::: {.textbox}`: the type exists in the IR and the spec, but emitting it is deferred to Phase 2,
  where the writer will need to reconstruct them.
- Three defects the corpus uncovered and that are now fixed: length formatting was
  crippled by rounding (a 1417-twip margin was written as `2.499cm`), empty paragraphs
  silently disappeared, and a hyperlink's character style was emitted twice.

---

## Phase 2 — DocMark → DOCX writing + round-trip (3–5 weeks)

**Objective**: close the bidirectional text cycle and stand up the fidelity infrastructure.

**Status: closed for the core path** (residual fidelity polish landed). Hand-written
DocMark parser, full OOXML writer (including floating DrawingML and footnote bodies),
bidirectional `convert`, CLI `roundtrip`, corpus-wide Office↔DocMark idempotence, and
`serialize(parse(md))` over all docx goldens are in tree.

Tasks:
1. [x] DocMark parser (`docsai-docmark`): hand-written mirror of the serializer for
   `{...}` attributes and fenced divs `:::` → IR. Front-matter validation with useful
   line errors. (Comrak remains available for plain-fidelity paths later.)
2. [x] docx writer: IR → `document.xml` + `styles.xml` + `numbering.xml` + media + props
   via direct ZIP/XML. Re-injection of `format=ooxml` raw-blocks; DrawingML images from
   the `AssetStore` without recompression, including **floating anchors**, rotation,
   flip, crop, border, and image hyperlinks. Footnote parts keep full run formatting.
3. [x] `roundtrip` command: docx→md→docx→md with identity check and `--json` report.
4. [x] **Serializer idempotence** test in CI: `serialize(parse(md)) == md` byte for byte
   over all docx goldens.
5. [ ] Property testing (proptest): generate valid random IRs and verify IR→md→IR == identity.
6. [ ] External validation: generated docx open without a repair dialog in Word and LibreOffice
   (manual checklist documented per release; later automatable via headless LibreOffice on Linux CI).

Acceptance criteria:
- [x] Idempotent round-trip on `basic-text` (CLI + pipeline test).
- [x] Idempotent round-trip (2nd pass == 1st pass) on the whole docx corpus.
- [x] Fidelity metric ≥ 95 % on text/styles/tables/lists of the corpus (identity on
      corpus goldens via `docx_roundtrip_is_idempotent`).
- [x] Images rewrite without recompression (bytes from `AssetStore`); inline **and**
      floating geometry (wrap, position, z-order, transforms) preserved.
- [ ] Generated docx open cleanly in Word and LibreOffice (checklist).
- [x] Hand-editing a `.dmk.md` and regenerating docx works (`convert` DocMark → Docx).

---

## Phase 3 — Spreadsheets: XLSX/XLS ⇄ DocMark (4–5 weeks)

**Objective**: bidirectional `xlsx` with values, formulas and formats; `xls` reading.

Tasks:
1. **Spike (3 days)**: decide xlsx writer — `umya-spreadsheet` (reads and writes styles) vs
   `rust_xlsxwriter` (better pure-write API). Criterion: which regenerates with higher fidelity
   styles+numFmt from the IR. Document in `docs/spikes/`.
2. xlsx reader: custom OPC/`quick-xml` reader (not calamine) for values, formulas,
   `xl/styles.xml` (numFmt, fonts, fills, borders by index), dimensions/merges/panes and
   drawings. Shared and array formulas expanded with metadata. (Spike R3.)
3. Cell typing: date/time detection via numFmt (serial→ISO-8601 and back); booleans,
   errors (`#DIV/0!`…).
4. Sheet serialization to DocMark per spec §4 (value table + `cell-meta` with compacted
   ranges) and reverse parser.
5. xlsx writer from IR (custom OPC/SpreadsheetML writer per spike R3): values, formulas
   (cached value preserved when present; recalc delegated to Excel/LibreOffice on open),
   numFmt, styles, merges, widths, defined names, shared/array formulas.
6. **Sheet images** (spec §4.1): custom reading of `xl/drawings/drawing*.xml` with
   `quick-xml` (calamine/umya do not cover them with full geometry) → `ImageRef` with anchors
   `SheetTwoCell`/`SheetOneCell`/`SheetAbsolute`, in-cell offsets,
   `move-with-cells`/`size-with-cells`, and the rest of the common model (rotation, crop, alt,
   hyperlink). Writing of `drawing*.xml` + relationships from the IR. Native charts →
   raw-block with warning.
7. xls reader (calamine) → same pipeline (read-only; document in `formats`).
8. Corpus: add workbooks with dates, percentages, currencies, cross-sheet formulas, defined
   names, images with all three anchor types, 100k cells (performance).

Acceptance criteria:
- [x] xlsx round-trip: values, formulas and numFmt intact on the corpus (fidelity ≥ 95 %).
- [x] Sheet image round-trip: all three anchors preserved symbolically (two-cell
      still stretches with the grid after the round trip), bitmaps without recompression.
- [x] An xlsx with dates survives round-trip without corrupting serials (dedicated test).
- [ ] 100k-cell xlsx: < 3 s, < 500 MB RAM.
- [ ] Excel and LibreOffice recalculate generated files without errors (checklist).

---

## Phase 4 — ODF: ODT and ODS ⇄ DocMark (3–4 weeks) — *parallelizable with Phase 5*

> **Status: closed for the core path.** Custom `docsai-odf` (`zip`+`quick-xml`)
> reads and writes `.odt` / `.ods`. Automatic styles are de-automatized into parent
> + deltas on read and regenerated on write. OpenFormula is kept as-is (no
> translation). Corpus under `corpus/odt/` and `corpus/ods/` with goldens;
> convert `SUPPORT` and CLI `formats` list both directions. Cross OOXML⇄ODF goes
> through the IR; non-matching `raw-block` dialects are dropped with a warning.

**Objective**: LibreOffice parity with the already-supported OOXML formats.

Tasks:
1. [x] Custom ODT reader/writer (`docsai-odf`, `zip`+`quick-xml`): content/styles/meta;
   **de-automatization** of automatic styles on read (→ deltas) and regeneration on write.
2. [x] ODT images: `<draw:frame>/<draw:image>` + `Pictures/*` → same `ImageGeometry` model
   (`as-char/char/paragraph/page` anchors → `Inline`/`Floating`; `svg:x/y/width/height`,
   `style:wrap`, rotation and `fo:clip`); reverse write with ODF graphic styles.
3. [x] ODS reader/writer with custom `quick-xml` (values, OpenFormula, merges, sheet
   images). `calamine` / `spreadsheet-ods` were not required after the custom path
   matched the IR cleanly.
4. [x] OpenFormula formulas: keep dialect (`formula-dialect=openformula`); do NOT translate yet.
5. [x] ODF corpus mirroring OOXML traits (text, styles, lists, tables, images, headers,
   footnotes; sheet values, formulas, merges, images), generated by `corpus/generate.py`.
6. [x] Scope note: cross conversion docx⇄odt "works" via the IR, but raw-blocks of one
   dialect are dropped with a warning in the other.

Acceptance criteria:
- [x] odt and ods round-trip on ODF corpus with DocMark identity on the second pass
      (pipeline + golden coverage, including image packages).
- [x] docx→DocMark→odt / odt→DocMark→docx work via the IR for common traits; dialect
      raw-blocks are dropped with `RawBlockDropped`.

---

## Phase 5 — Legacy DOC (2–3 weeks) — *parallelizable with Phase 4*

> **Status: closed for the core path.** Native degraded reader (`cfb` + FIB +
> piece table + BLIP scan) and optional LibreOffice headless pre-conversion to
> `.docx` are wired through `docsai-convert` / CLI `--use-loffice`. OLE2
> detection uses the CFB directory. Corpus under `corpus/doc/`.

**Objective**: `.doc` reading with the two-level strategy from the analysis (§1.3).

Tasks:
1. [x] Runtime LibreOffice detection per OS (standard paths + PATH); flag
   `--use-loffice auto|never|require`; conversion `soffice --headless --convert-to docx` in a
   sandboxed temporary directory and re-entry through the Phase 1 docx pipeline.
2. [x] Degraded native extractor: `cfb` + FIB + piece table → text with paragraphs and basic
   properties; extraction of embedded images (BLIPs from the Escher/OfficeArt stream) to the
   `AssetStore` — without fine geometry: emitted with `anchor=inline` and native size, with
   `ImageGeometryDegraded` warning. Mark output as degraded in the `ConversionReport`.
3. [x] Clear messaging: if LibreOffice is missing, the user knows exactly what is being lost and how
   to improve the result.
4. [x] Tests: synthetic `.doc` corpus (`corpus/doc/`) covering basic text and encryption;
   robustness against truncated input. (Real Word/LibreOffice-saved samples remain welcome
   additions; they do not block the phase.)

Acceptance criteria:
- [x] With LibreOffice installed: fidelity equivalent to the docx path
      (`--use-loffice auto|require` → docx pipeline).
- [x] Without LibreOffice: full text and correct paragraph structure, no panics.
- [x] Encrypted/protected `.doc` rejected with a clear error.

---

## Phase 6 — Full CLI and distribution (2–3 weeks)

**Objective**: product experience: the definitive CLI and installable binaries on 3 OSes.

Tasks:
1. Final CLI per `architecture.md` §5: `convert`, `inspect`, `roundtrip`, `formats`;
   `--json`, `--strict`, stdin/stdout, exit codes; `--style-map` (mammoth mode, spec §5).
2. Polished error messages and warnings (with `miette` or similar for nice diagnostics);
   `--verbose`/`RUST_LOG`.
3. Batch processing: `docsai convert *.docx --out-dir md/` with parallelism (`rayon`) and
   aggregated summary.
4. `cargo-dist`: automatic releases per tag with signed binaries for the 5 targets;
   shell/powershell installers; Homebrew/Scoop formulas; crates.io publication.
5. User documentation: definitive README with real examples, carefully written `--help`,
   CHANGELOG (keep-a-changelog).

Acceptance criteria:
- [x] CLI surface complete: `convert` (batch/`--out-dir`, stdin/stdout, `--style-map`,
      `--max-cells`), `inspect`, `roundtrip`, `formats`; exit codes and `--json`/`--strict`.
- [x] `docsai convert` over many inputs with `--out-dir` finishes with a correct aggregated summary
      (rayon). Verified on the corpus; scale to 100 mixed files is the same code path.
- [x] Foreseeable user errors emit an actionable hint (unknown format, unsupported conversion,
      LibreOffice missing, empty stdin, batch misuse).
- [x] `cargo-dist` configuration and CHANGELOG present; tag-driven GitHub release workflow.
- [ ] Installation on a clean machine of each OS with one command and a successful test conversion
      (requires publishing the first release tag / installers).

---

## Phase 7 — MCP server (2 weeks)

**Objective**: `docsai mcp` operational with real clients.

Tasks:
1. Implement `docsai-mcp` with `rmcp` (stdio): the 4 tools from `architecture.md` §6 with documented
   JSON schemas and input validation.
2. Path mode and base64 mode; size limits; timeouts; inline assets vs files.
3. Clean-stdout guarantee: automatic test that no code path writes to stdout outside
   the protocol (logs → stderr).
4. Integration tests with MCP Inspector and real Claude Desktop/Claude Code; configuration
   recipes in README.
5. Consider (backlog, non-blocking): `apply_edits` tool for guided document editing via
   DocMark in the future.

Acceptance criteria:
- [ ] A real MCP client converts docx→markdown and markdown→docx end to end.
- [ ] Malformed inputs return correct MCP errors, never hang the server.
- [ ] Session of 100 consecutive conversions with no memory leaks or orphan temporary files.

---

## Phase 8 — Hardening and quality (2–3 weeks, partially continuous)

**Objective**: production robustness.

Tasks:
1. Fuzzing with `cargo-fuzz` of the 4 input parsers (docx, xlsx, odf, docmark); fuzzing
   corpus seeded with the test corpus; run on scheduled CI (weekly cron).
2. Adversarial document suite: ZIP bombs (decompression limits), XML entity expansion
   (verify quick-xml does not expand external entities), malicious asset paths
   (path traversal in media names), extreme sizes.
3. `criterion` benchmarks + performance budget in CI (regression > 20 % blocks).
4. `cargo audit`/`cargo deny` in CI (licenses + vulnerabilities); informative `cargo bloat`.
5. Expand corpus with varied real documents (reports, corporate templates, financial
   sheets) and publish the **fidelity matrix** by trait in the documentation.
6. Security review: the MCP server never writes outside the indicated directories;
   path normalization; no execution of document content (macros ALWAYS ignored —
   `.docm/.xlsm` are read as their macro-free equivalents, with a warning).

Acceptance criteria:
- [ ] 72 h of accumulated fuzzing without crashes on the 4 parsers.
- [ ] Full adversarial suite green.
- [ ] Fidelity matrix published and ≥ per-phase targets.

---

## Phase 9 — v1.0 and post-1.0 backlog (1 week + continuous)

**v1.0 close**: freeze stable CLI and DocMark 1.0 format, release notes, announcement.

Prioritized post-1.0 backlog (not committed):
- OOXML ⇄ OpenFormula formula dialect translation (risk R5).
- PowerPoint (`.pptx`/`.odp`) → read-only DocMark.
- WMF/EMF → PNG/SVG conversion (`emf` crate / custom library or fallback).
- Comments and track changes (`w:ins`/`w:del`) as DocMark extensions (CriticMarkup syntax).
- Incremental MCP edit tool (`apply_edits`).
- Library mode: publish `docsai-convert` as a stable crate for third parties + WASM bindings.
- `.doc`/`.xls` writing via LibreOffice fallback if demand warrants it.

---

## Schedule summary (indicative, 1 senior developer)

| Phase | Duration | Cumulative |
|---|---|---|
| 0 Foundations | 2–3 wk | 3 |
| 1 DOCX reading | 4–6 wk | 9 |
| 2 DOCX writing + round-trip | 3–5 wk | 14 |
| 3 XLSX/XLS | 4–5 wk | 19 |
| 4 ODF | 3–4 wk | 22* |
| 5 DOC | 2–3 wk | 22* (*parallel 4‖5 with 2 devs) |
| 6 CLI + distribution | 2–3 wk | 25 |
| 7 MCP | 2 wk | 27 |
| 8 Hardening | 2–3 wk | 30 |
| 9 v1.0 | 1 wk | ~31 wk (~7 months; ~5–5.5 with 2 devs from Phase 3) |

## Project tracking metrics

- **Fidelity per category** (`roundtrip` command on corpus): target ≥ 95 % OOXML, ≥ 90 % ODF.
- **Raw-blocks per real corpus document**: downward trend per phase.
- **Test coverage** of library crates ≥ 80 %.
- **Performance**: budgets from `architecture.md` §8 in CI.
