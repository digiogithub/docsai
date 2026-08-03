# Technical analysis

Pre-development analysis document for `docsai`. Covers: (1) input/output formats and their real
complexity, (2) the state of the art in comparable open-source projects, (3) candidate extended
Markdown variants as pivot format, (4) evaluation of available Rust libraries, and (5) decisions
taken with their risks.

Analysis date: August 2026.

Scope note: this document analyses **flow documents and grids**, which is what plan v1
(Phases 0–9) built. Presentations (`.pptx`/`.ppt`/`.odp`) and the agent context-cost axis are
analysed in [`technical-analysis-presentations.md`](technical-analysis-presentations.md), which
backs [`development-plan-v2.md`](development-plan-v2.md). Nothing here is retracted by it; the
decisions below (IR pivot, custom readers over `zip` + `quick-xml`, raw-block hatch, `AssetStore`
with content hashing, optional LibreOffice fallback) are exactly what makes the presentation
work affordable.

---

## 1. Input/output formats

### 1.1 OOXML: `.docx` and `.xlsx` (ECMA-376 / ISO 29500)

Both are ZIP containers with XML inside. They are the formats with the best public documentation
and library support, and therefore anchor phases 1–3 of the plan.

**`.docx` (WordprocessingML)** — relevant parts inside the ZIP:

| Part | Content | Relevance for docsai |
|---|---|---|
| `word/document.xml` | Body: paragraphs (`w:p`), runs (`w:r`), tables (`w:tbl`) | Core of the conversion |
| `word/styles.xml` | Style catalog (paragraph, character, table) + `basedOn` inheritance | Must be dumped into DocMark front matter |
| `word/numbering.xml` | List definitions (numbering, bullets, levels) | Critical: lists in OOXML are not syntactically nested; they are rebuilt from `numId`/`ilvl` |
| `word/media/*` | Embedded images (png, jpeg, gif, **wmf/emf**) | Extract to `assets/` |
| `word/_rels/*.rels` | Relationships (images, hyperlinks) | Reference resolution |
| `word/header*.xml`, `word/footer*.xml` | Headers and footers | Dedicated DocMark containers |
| `docProps/core.xml`, `docProps/app.xml` | Properties (title, author, dates…) | Front matter |
| `word/settings.xml`, `w:sectPr` sections | Page size, margins, columns | Section metadata |

Known hard points of `.docx`:
- **4-level formatting inheritance**: document defaults → paragraph style → character style →
  direct formatting (`rPr`/`pPr`). For a faithful conversion you must *resolve* the cascade but
  *store* only the style reference + direct deltas (if everything is flattened, the inverse
  conversion produces monstrous documents with no reusable styles).
- **Lists**: rebuild the tree from flat `(numId, ilvl)` pairs.
- **Fields** (`w:fldSimple`, `w:instrText`): TOC, cross-references, page numbers. Preserved as
  raw-blocks in v1.
- **WMF/EMF images**: legacy Windows vector formats without simple multiplatform rendering
  support. Strategy: extract as-is + warning; optional conversion in later phases.
- **Revisions/comments** (`w:ins`, `w:del`, `w:comment*`): out of scope for v1; documents with
  revisions are accepted by taking the "accepted" version and emitting a warning.

**`.xlsx` (SpreadsheetML)** — relevant parts: `xl/workbook.xml`, `xl/worksheets/sheet*.xml`,
`xl/sharedStrings.xml`, `xl/styles.xml` (number formats, fonts, fills, borders — all via cross
indexes), `xl/calcChain.xml`.

Hard points of `.xlsx`:
- **Cells store cached value + formula** (`<c><f>SUM(A1:A3)</f><v>42</v></c>`). DocMark must keep
  both: the formula for bidirectionality, the value for readability.
- **Shared formulas** (`t="shared"`) and array formulas (`t="array"`): expand them or preserve
  their range metadata.
- **Number formats** (`numFmt`): the difference between `45123` and `15/07/2023` — Excel dates
  are serial numbers + format. Keeping `numFmtId`/format code per cell is mandatory to avoid
  corrupting data on round-trip.
- Merged cells (`mergeCells`), column widths/row heights, frozen panes, validations, conditional
  formatting (the last three: metadata in v1, no semantics).

### 1.2 ODF: `.odt` and `.ods` (ISO 26300, OASIS OpenDocument)

Also ZIP+XML (`content.xml`, `styles.xml`, `meta.xml`, `settings.xml`, `Pictures/`). Conceptual
model very similar to OOXML but with important differences:

- **Automatic styles** (`office:automatic-styles`) represent direct formatting: every fragment
  with manual formatting generates an anonymous style. They must be "de-automated" on read
  (map automatic styles to direct-formatting deltas in the IR).
- `.ods` formulas use **OpenFormula** with a namespace prefix (`of:=SUM([.A1:.A3])`) and a
  different reference syntax (`[.A1]` vs `A1`). For OOXML⇄ODF bidirectionality, formula syntax
  would need translating; in v1 the formula is kept in its original dialect, annotating
  `formula-dialect` on the cell.
- ODF is better specified and more regular than OOXML; the effort of a custom parser with
  `quick-xml` is manageable and is in fact the plan for `.odt` (see §4.3).

### 1.3 Legacy binary formats: `.doc` (MS-DOC) and `.xls` (BIFF8)

- **`.xls`**: solved — `calamine` reads it natively (values and formulas). Read-only.
- **`.doc`**: the project's largest technical risk. It is a binary format on an OLE2/CFB
  container with complex internal structures (piece table, FIB, FKPs…). **No mature Rust crate
  reads it with styles and images.** Options evaluated:

| Option | Effort | Fidelity | Dependencies |
|---|---|---|---|
| Custom parser on the `cfb` crate (MS-DOC spec) | Very high (months) | High | No external deps |
| Fallback to LibreOffice headless (`soffice --headless --convert-to docx`) and reuse the docx pipeline | Low | Very high | LibreOffice installed (optional, detected at runtime) |
| `antiword`/`wvWare` as external process | Low | Low (loses styles) | External binary |
| Custom plain-text extraction (piece table only) | Medium | Text only | None |

  **Decision**: two-level strategy. (a) LibreOffice headless fallback if installed — maximum
  fidelity with minimum effort; (b) native text+basic-structure extractor on `cfb` as a degraded
  mode without dependencies. A full MS-DOC parser is not pursued unless real demand justifies it.
  This keeps the principle "single binary with no mandatory external runtime": LibreOffice
  improves `.doc` but is never a requirement.

### 1.4 Images and graphic objects: cross-cutting analysis

Images are a first-class project requirement: **all** input formats carry them, with different
geometry models, and conversion must extract them and preserve size, position, anchor, and other
properties for round-trip. Model by format:

| Format | Where they live | Geometry/anchor model |
|---|---|---|
| `.docx` | `word/media/*` + `<w:drawing>` (DrawingML) or `<w:pict>` (legacy VML) | `wp:inline` (inline, only `wp:extent`) or `wp:anchor` (floating: position relative to page/margin/paragraph/character, EMU offsets, wrap `square/tight/through/topAndBottom/none`, z-order, `behindDoc`) |
| `.xlsx` | `xl/media/*` + `xl/drawings/drawing*.xml` (SpreadsheetDrawingML) | Three anchors: `xdr:twoCellAnchor` (cell to cell + offsets, stretches with the grid), `xdr:oneCellAnchor` (origin cell + fixed size), `xdr:absoluteAnchor` (absolute position in EMU) |
| `.odt` | `Pictures/*` + `<draw:frame><draw:image>` | `text:anchor-type` = `as-char/char/paragraph/page/frame`; `svg:width/height/x/y`; wrap via graphic style (`style:wrap`) |
| `.ods` | `Pictures/*` + `<draw:frame>` inside `<table:shapes>` or cell-anchored | Cell anchor (`table:end-cell-address` + offsets) or sheet anchor |
| `.doc` | `Data`/Escher stream (OfficeArt) | Binary Escher; on the LibreOffice-fallback path conversion is free; on the degraded native path BLIPs (images) are extracted without fine geometry |

Properties to preserve in all cases (they define the IR's normalized model; see
`architecture.md` §3.1): displayed dimensions vs native bitmap dimensions (and thus the scale
factor), DPI, crop (`srcRect` in OOXML, `fo:clip` in ODF), rotation and flips, anchor and
position with their offsets, text wrap mode, Z-order, alternative text and title (accessibility,
`descr`/`svg:desc`), internal object name and hyperlink on the image.

Specific hard points:
- **Heterogeneous coordinates**: EMU in OOXML (914,400/inch), cm/in in ODF, twips in `.doc`.
  The IR normalizes everything to EMU with newtypes; DocMark exposes readable `px`/`pt`/`cm`.
- **Legacy VML in docx**: old documents (or converted from `.doc`) use `<w:pict>` with VML
  instead of DrawingML. Both must be readable; writing always emits DrawingML.
- **twoCell anchors in xlsx**: real size depends on column widths/row heights; to preserve
  geometry the symbolic anchor (cells + offsets) must be stored, not the resolved size — and the
  inverse on write.
- **WMF/EMF**: extracted as-is (perfect round-trip) but not viewable in Markdown viewers;
  conversion to PNG/SVG stays in the post-1.0 backlog. The image is still referenced with its
  full geometry.
- **Duplicates**: the same bitmap may appear N times with different geometries; the
  `AssetStore` deduplicates by content hash and each appearance keeps its own geometry.
- **Linked (non-embedded) images**: OOXML/ODF allow external `r:link`/`xlink:href`; the URL is
  preserved with a warning (remote content is not downloaded — security implication).

---

## 2. Open-source state of the art (what to learn and reuse)

| Project | Language | What it does | Lessons for docsai |
|---|---|---|---|
| **Pandoc** | Haskell | Universal conversion via pivot AST; its extended Markdown (attributes `{...}`, divs `:::`, front matter) is the most expressive on the market | The full architectural pattern: readers → AST → writers. Its attribute syntax is the base of DocMark. Known limitation: medium-low fidelity on complex docx (custom styles, text boxes) and no xlsx support |
| **MarkItDown** (Microsoft) | Python | Office/PDF/HTML → "LLM-ready" Markdown, unidirectional | Validates demand for the MCP/LLM use case; its total loss of styles is exactly the gap docsai fills |
| **Docling** (IBM) | Python | Documents → Markdown/JSON with its own `DoclingDocument` model | Confirms the need for a rich document model as pivot and for exporting both readable MD + structured metadata |
| **mammoth** (.js/Python) | JS/Python | docx → semantic HTML via a **configurable style map** (`Heading1 ⇒ h1`) | The user-configurable style-map concept is adopted in docsai (`--style-map`) |
| **html2md / turndown, marker, unoconv/unoserver** | various | Partial converters | unoserver documents the LibreOffice headless fallback pattern |
| **rdocx** | Rust | docx read/write + render to PDF/HTML/MD (recent crate, 2026) | Watch as an alternative; too young to anchor the project today |

**State-of-the-art conclusion**: nobody today combines (1) native binary with no runtime,
(2) bidirectionality with styles, (3) spreadsheets with formulas, and (4) an MCP server.
Pandoc is the reference ceiling for text; MarkItDown/Docling for LLM integration. docsai
positions itself in the empty intersection.

---

## 3. The pivot format: evaluated extended Markdown variants

Requirements: readable by humans and standard viewers, arbitrary attributes on inline and block,
document metadata, extensible without breaking parsers, and with an ecosystem.

| Candidate | Attributes | Ecosystem | Verdict |
|---|---|---|---|
| **Pure CommonMark + GFM** | ❌ None | Huge | Insufficient: without attributes there are no styles |
| **Pandoc Markdown** (attributes + fenced divs + spans + YAML) | ✅ Complete | Large (pandoc consumes it) | **Chosen base.** Syntax proven for a decade for exactly this problem |
| **MyST Markdown** | ✅ (directives/roles) | Scientific/Sphinx | More verbose directives; oriented to publishing, not round-trip |
| **Djot** (Jyrki/MacFarlane) | ✅ native | Small | Technically superior but breaks "looks fine on GitHub" compatibility |
| **MDX** | JSX | Web/React | Discarded: not readable Markdown for non-programmers |

**Decision**: **DocMark = CommonMark + GFM (tables, strikethrough, task lists) + a subset of
Pandoc extensions** (attributes `{...}` on headings/images/spans/code, fenced divs `:::`, YAML
front matter) **+ custom extensions for spreadsheets** (cell metadata) documented in
`docmark-specification.md`. Additional benefit: a DocMark file is processable by Pandoc
directly with acceptable degradation, giving free interoperability with the whole Pandoc
ecosystem (PDF via LaTeX, HTML, EPUB…).

---

## 4. Rust library evaluation

### 4.1 Document read/write

| Crate | Proposed role | Status (2026) | Evaluation notes |
|---|---|---|---|
| **`calamine`** | Read `.xls`, `.xlsx`, `.xlsb`, `.ods` (values **and formulas**) | Mature, maintained, widely used | Lazy and fast reading; reads formulas via `worksheet_formula()`. **Does not read styles/number formats in enough detail** → complemented with custom reading of `xl/styles.xml` |
| **`umya-spreadsheet`** | Read+write `.xlsx` with styles | Maintained | Evaluated in spike R3; **not adopted** for writing (opaque API / fidelity vs custom OPC) |
| **`rust_xlsxwriter`** | Write `.xlsx` (alternative) | Very maintained (port of XlsxWriter) | Evaluated in spike R3; **not adopted** — write-only and weaker style round-trip than a custom writer |
| **Custom OPC + SpreadsheetML** (chosen) | Read+write `.xlsx` | In-tree (`docsai-office::xlsx`) | Same approach as docx: full control of styles, shared formulas, drawings. **calamine only for `.xls`**. See `docs/spikes/R3-xlsx-writer.md` |
| **`docx-rs` (bokuweb)** | ~~Read~~ + possible write `.docx` | Most used (1M+ downloads) | **Discarded for reading** after spike R1 (`docs/spikes/R1-docx-strategy.md`, August 2026): handles styles and numbering well, but loses almost the entire image model (wrap, `behindDoc`, crop, flip, rotation, alt, title, hyperlink), footnotes and `w:instr` on simple fields, does not preserve unknown elements for raw-blocks, and *panics* on 23% of 903 measured corrupt inputs. docx reading uses a custom parser on `zip` + `quick-xml`. Still a candidate for the Phase 2 **writer**, an independent decision |
| **`docx-rust`** | Alternative `.docx` reading | Less activity | More direct XML mapping; keep as reference |
| **`spreadsheet-ods`** | Read+write `.ods` | Maintained | Covers ODS styles and formulas; avoids writing a custom ODF-spreadsheet writer |
| **(none)** | `.odt` | — | No mature crate for ODT with styles: **custom parser/writer** on `zip` + `quick-xml` (ODF is regular; bounded effort) |
| **`cfb`** | OLE2 container for legacy `.doc`/`.xls` | Stable | Base of the degraded `.doc` extractor |

### 4.2 Markdown (inverse path)

| Crate | Role | Notes |
|---|---|---|
| **`comrak`** | Markdown parser for the DocMark→IR pipeline | Full CommonMark+GFM, keeps positions, supports front matter, has limited attribute extension; full `{...}` attributes and fenced divs `:::` are processed in a custom pass over its AST (or pre-lexer) |
| `markdown-rs` / `pulldown-cmark` | Alternatives | pulldown-cmark is faster but event-oriented (awkward for transformation); markdown-rs has a pleasant mdast AST but fewer native extensions |

**Decision**: `comrak` + custom attributes/divs layer. The DocMark **serializer** (IR→MD) is
hand-written (not delegated to comrak) to control output byte-by-byte and guarantee round-trip
idempotence.

### 4.3 Infrastructure

| Crate | Role |
|---|---|
| `zip` | OOXML/ODF containers |
| `quick-xml` (+ `serde`) | High-performance XML parsing where format crates fall short |
| `serde` / `serde_yaml` / `serde_json` | Front matter, `inspect --json`, config |
| `clap` (derive) | CLI |
| `rmcp` (official MCP SDK, stdio transport) | MCP server; `#[tool]` macros; implements the 2026-07-28 spec with backward compatibility |
| `rmcp` MSRV | **1.88+** (`rmcp` 3.x). Workspace `rust-version` tracks this floor; CI uses stable |
| ~~`image`~~ | **Not used in Phase 1.** docsai never re-encodes a bitmap; it only needs to name and measure it, which is done by reading the format header (PNG, JPEG, GIF, BMP, TIFF, WebP, EMF, WMF) in `docsai-model::assets`, without a heavy dependency. Will be re-evaluated if some phase truly needs re-encoding |
| ~~`serde_yaml`~~ | **Not used.** Front matter has a small known schema and the spec requires byte-for-byte determinism, so it is written by hand; the crate is also without active maintenance |
| `thiserror` / `anyhow` | Errors |
| `tracing` + `tracing-subscriber` | Logs (always to stderr) |
| `tokio` | Only in `docsai-mcp` (rmcp requires it); the conversion core is synchronous |
| ~~`insta`~~ | **Not used.** Golden files are `.expected.dmk.md` beside the corpus, as prescribed by `AGENTS.md` §6: they are reviewed as normal text in the diff and do not depend on a snapshot format |
| `cargo-fuzz` | Parser fuzzing (Phase 8) |
| `cargo-dist` | Multiplatform release packaging |

---

## 5. Derived architecture decisions (summary)

1. **Mandatory pivot IR** (`docsai-model`): document tree with two possible roots
   (`TextDocument`, `Workbook`) — detail in `architecture.md`. N formats → 2N converters
   instead of N².
2. **Styles by reference + delta**: the IR stores `style_id` + direct properties, and the full
   style catalog travels in the front matter. Round-trip thus rebuilds a real `styles.xml` and
   Markdown stays clean.
3. **External assets with manifest**: images to `assets/` with deterministic names
   (content hash) so round-trip does not duplicate media.
4. **Structured `ConversionReport`**: every conversion returns document + list of typed
   warnings (unsupported element, degradation, raw-block emitted). The CLI shows it; MCP returns
   it in the tool response.
5. **Optional external fallbacks**: LibreOffice headless only for `.doc`, detected at runtime
   (`--use-loffice=auto|never|require`).

## 6. Main risks and mitigations

| # | Risk | Prob. | Impact | Mitigation |
|---|---|---|---|---|
| R1 | OOXML style cascade turns out more expensive than expected and delays Phase 1 | High | High | 1-week spike in Phase 0 with real documents; limit v1 to 4-level resolution without conditional `tblStyle` |
| R2 | No xlsx crate covers style reading in enough detail | Medium | Medium | Already assumed: complementary custom reading of `xl/styles.xml` with quick-xml (bounded effort, documented format) |
| R3 | Non-idempotent round-trip due to Markdown ambiguities (escapes, spaces) | Medium | High | Custom serializer with deterministic rules + idempotence test in CI from Phase 2 |
| R4 | `.doc` without LibreOffice disappoints users | Medium | Low | Clear degraded-mode messages; document in README |
| R5 | OOXML/OpenFormula formula dialect divergence | High | Medium | v1 keeps original dialect + `formula-dialect` field; automatic translation postponed (Phase 9/backlog) |
| R6 | Third-party crates abandoned mid-project | Low | Medium | The four critical crates are among the most used in the ecosystem; IR design lets a reader be replaced without touching the rest |
| R7 | Binary size grows out of control | Low | Low | `cargo bloat` in CI, optional features, LTO in release |
| R8 | Diversity of image models (DrawingML vs VML vs Escher vs draw:frame) fragments effort | Medium | Medium | Normalized `ImageGeometry` model in the IR defined in Phase 0 (architecture §3.1); VML reading limited to model attributes; Escher only BLIPs on the native `.doc` path |

## 7. Sources consulted

- [Pandoc User's Guide](https://pandoc.org/MANUAL.html) — attribute, div, and front-matter syntax
- [calamine (GitHub)](https://github.com/tafia/calamine) and [docs.rs/calamine](https://docs.rs/calamine)
- [umya-spreadsheet (GitHub)](https://github.com/mathnya/umya-spreadsheet) and comparison [calamine vs umya-spreadsheet](https://umaranis.com/2026/05/04/reading-excel-files-in-rust-calamine-vs-umya-spreadsheet/)
- [docx-rs (crates.io)](https://crates.io/crates/docx-rs) · [docx-rust (crates.io)](https://crates.io/crates/docx-rust) · [rdocx (lib.rs)](https://lib.rs/crates/rdocx)
- [Official Rust MCP SDK — rmcp](https://github.com/modelcontextprotocol/rust-sdk) and [docs.rs/rmcp](https://docs.rs/rmcp)
- Converter comparisons: [MarkItDown vs Pandoc](https://www.file2markdown.ai/blog/markitdown-vs-pandoc), [Docling vs MarkItDown](https://www.file2markdown.ai/blog/docling-vs-markitdown), [Real Python on MarkItDown](https://realpython.com/python-markitdown/)
- ECMA-376 (OOXML), ISO/IEC 26300 (ODF), [MS-DOC]/[MS-XLS] Microsoft Open Specifications
