# docsai architecture

Describes the software structure: crate workspace, intermediate document model (IR),
conversion pipelines, CLI and MCP server. Complements `technical-analysis.md` (the why)
and `development-plan.md` (the when).

## 1. Overview

```
                 ┌────────────────────────── docsai-convert ──────────────────────────┐
  .docx ─┐       │                                                                    │
  .doc  ─┤  readers (docsai-office / docsai-odf)          writers                     │
  .odt  ─┼──────────────►  IR (docsai-model)  ◄──────────────────────┐                │
  .xlsx ─┤                    ▲        │                             │                │
  .xls  ─┤                    │        ▼                             │                │
  .ods  ─┘             DocMark parser  DocMark serializer     .docx/.xlsx/.odt/.ods   │
                        (docsai-docmark)                             ▲                │
                              ▲        │                             │                │
                              │        ▼                             │                │
                          .dmk.md file + assets/ ────────────────────┘                │
                 └─────────────────────────────────────────────────────────────────────┘
                                   ▲                          ▲
                            docsai-cli (clap)          docsai-mcp (rmcp, stdio)
```

Central rule: **no converter talks to another converter**. Everything goes through the IR.

## 2. Crate workspace

| Crate | Type | Responsibility | Internal dependencies |
|---|---|---|---|
| `docsai-model` | lib | IR + common types (`Style`, `ConversionReport`, units) | none |
| `docsai-docmark` | lib | DocMark serializer and parser (IR ⇄ .dmk.md + assets) | model |
| `docsai-office` | lib | docx/xlsx/xls/doc readers and docx/xlsx writers | model |
| `docsai-odf` | lib | odt/ods readers/writers | model |
| `docsai-convert` | lib | Orchestration: format detection, pipelines, asset management, LibreOffice fallback, reports | all of the above |
| `docsai-cli` | bin | `docsai` binary (convert/inspect/roundtrip/mcp subcommands) | convert |
| `docsai-mcp` | lib | MCP server implementation (CLI starts it with `docsai mcp`) | convert |

Notes:
- A single distributed binary (`docsai`); `docsai-mcp` is a lib so the `mcp` subcommand
  lives inside the same executable.
- Cargo features per format (`office-doc`, `odf`, …) to compile minimal variants.

## 3. The intermediate model (IR) — `docsai-model`

> Plan v2 adds a **third root** (`Document::Presentation`) and a node addressing layer
> (`NodeId` + etag). See §9.

Two roots:

```rust
pub enum Document {
    Text(TextDocument),
    Workbook(Workbook),
}

pub struct TextDocument {
    pub meta: DocumentMeta,          // title, author, dates, custom props, language
    pub styles: StyleCatalog,        // named styles with inheritance (based_on)
    pub list_defs: ListCatalog,
    pub sections: Vec<Section>,      // page geometry + headers/footers + blocks
}

pub enum Block {
    Paragraph(Paragraph),            // Vec<Inline> + ParaProps (style_id + direct deltas)
    Heading(Heading),                // outline level + Paragraph
    List(List),                      // already reconstructed tree (not numId/ilvl pairs)
    Table(Table),                    // grid with spans, col-widths, style
    Image(ImageRef),                 // AssetStore reference + geometry/anchor
    TextBox(TextBox),
    Raw(RawFragment),                // fidelity hatch (source format + XML bytes)
    // …
}

pub enum Inline {
    Text(String),
    Styled(Vec<Inline>, RunProps),   // RunProps = optional style_id + deltas (bold, color…)
    Link { target: String, content: Vec<Inline>, props: RunProps },
    Footnote(Vec<Block>),
    Field { kind: FieldKind, cached: String },
    Break(BreakKind),
    ImageInline(ImageRef),
}

pub struct Workbook {
    pub meta: DocumentMeta,
    pub styles: StyleCatalog,
    pub defined_names: Vec<DefinedName>,
    pub sheets: Vec<Sheet>,          // Sheet = sparse Cell grid + col/row props + merges
}                                    //         + panes + images: Vec<ImageRef> (sheet-anchored)

pub struct Cell {
    pub value: CellValue,            // Number | Text | Bool | DateTime | Error | Empty
    pub formula: Option<Formula>,    // text + dialect (Ooxml | OpenFormula) + shared/array info
    pub num_fmt: Option<NumFmt>,
    pub style_id: Option<StyleId>,
}
```

### 3.1 Normalized image model (`ImageRef` + `ImageGeometry`)

Each input format has its own graphics model (DrawingML, VML, SpreadsheetDrawingML,
ODF `draw:frame`, Escher — see analysis §1.4). The IR normalizes them to a single model,
which DocMark spec §3.5/§4.1 serializes attribute by attribute:

```rust
pub struct ImageRef {
    pub asset: AssetId,                  // key in the AssetStore (content hash)
    pub geometry: ImageGeometry,
    pub alt: String,                     // alternative text (accessibility)
    pub title: Option<String>,
    pub name: Option<String>,            // internal object name (docPr @name)
    pub link: Option<String>,            // hyperlink on the image
    pub external_src: Option<String>,    // linked image, not embedded
    pub effects_raw: Option<RawId>,      // unmodeled DrawingML effects → associated raw-block
}

pub struct ImageGeometry {
    pub display_size: Size,              // EMU; displayed size (≠ native size)
    pub native_size_px: Option<(u32, u32)>,
    pub dpi: Option<u32>,
    pub anchor: Anchor,
    pub rotation_deg: f32,
    pub flip: Flip,                      // None | H | V | HV
    pub crop: Option<CropRect>,          // % of original per side
    pub border: Option<SimpleBorder>,
    pub z_index: Option<i32>,
}

pub enum Anchor {
    Inline,                                              // flows with text
    Floating { relative_to: RelBase,                     // Page | Margin | Paragraph | Character
               position: HVPos,                          // EMU offsets or symbolic alignment
               wrap: WrapMode, wrap_side: WrapSide,
               behind_text: bool },
    // Spreadsheets only:
    SheetTwoCell { from: CellAnchor, to: CellAnchor,     // CellAnchor = (col,row) + EMU offsets
                   move_with_cells: bool, size_with_cells: bool },
    SheetOneCell { from: CellAnchor },                   // + fixed display_size
    SheetAbsolute { pos: Point },
}
```

Model rules:
- Readers **translate into this model, they do not bypass it**: an image attribute a reader
  cannot map is degraded with a typed warning (`Warning::ImageGeometryDegraded`), and
  unrepresentable effects go to raw-block via `effects_raw` — never silently discarded.
- Writers do the full reverse translation (e.g. `Anchor::Floating` → `wp:anchor`
  with `positionH/V`, wrap and z-order; `SheetTwoCell` → `xdr:twoCellAnchor`).
- Cross conversion of anchors (text document ⇄ sheet) does not exist: each sheet `Anchor`
  is only valid inside a `Workbook` (invariant checked by the IR validator).

### 3.2 `AssetStore`

Trait that abstracts media storage; `docsai-convert` provides the implementations
(`assets/` directory in CLI, memory/base64 in MCP):

- **Content-hash deduplication**: `put(bytes) -> AssetId` returns the same id for the same
  content; N appearances of a bitmap share a file, each `ImageRef` keeps its own geometry.
- **Deterministic names** `img-<hash8>.<ext>` (extension derived from content sniffing,
  not the source) → round-trips do not duplicate media and `assets/` diffs are stable.
- **Manifest**: the store records content-type, byte size and detected native dimensions
  (`image` crate); `inspect --json` exposes it and the writer uses it to fill required
  destination fields without re-reading files.
- **Security**: asset names are sanitized (never derived from internal document paths →
  no path traversal); external linked images are never downloaded.

IR principles:
- **Style = reference + delta**, never flattened format (see analysis §5.2).
- **No I/O dependency**: `docsai-model` knows nothing about ZIP or XML; `ImageRef`s point to an
  abstract `AssetStore` (trait) materialized by `docsai-convert`.
- All types are `serde`-serializable → free `inspect --json` and simple debugging.
- Units normalized to EMU/twips internally with newtypes (`Length`), converting to
  `pt/cm/px` only when serializing DocMark.

## 4. Conversion contracts

```rust
pub trait DocumentReader {
    fn detect(path: &Path, sniff: &[u8]) -> DetectScore;      // by content, not only extension
    fn read(&self, input: &mut dyn ReadSeek, assets: &mut dyn AssetStore)
        -> Result<(Document, ConversionReport), ReadError>;
}

pub trait DocumentWriter {
    fn write(&self, doc: &Document, assets: &dyn AssetStore, out: &mut dyn Write)
        -> Result<ConversionReport, WriteError>;
}

pub struct ConversionReport {
    pub warnings: Vec<Warning>,       // typed: UnsupportedElement { kind, location, action }
    pub raw_blocks_emitted: u32,      //          Degraded { what, why } · AssetIssue { … }
    pub stats: ConversionStats,       // paragraphs, cells, images, formulas processed
}
```

- Readers **never panic** on corrupt input: always typed `Err`.
- `ConversionReport` flows to CLI (readable stderr / `--json`) and to the MCP response.
- The `roundtrip` command compares original IR vs IR after round-trip with a **structural
  diff of normalized IR** and produces a fidelity metric (% nodes preserved per category).

## 5. CLI (`docsai-cli`)

> Plan v2 adds `outline`, `read --select`, `search`, `tokens` and `edit`, plus the
> `--fidelity agent` level. See §9.

```
docsai convert <in> [-o <out>] [--to <fmt>] [--fidelity full|standard|plain]
               [--assets-dir <dir>] [--style-map <yaml>] [--max-cells N]
               [--use-loffice auto|never|require] [--json]
docsai inspect <in> [--json]        # metadata, styles, sheets, media, without converting
docsai roundtrip <in> [--report <path>] [--json]
docsai formats                       # current support matrix
docsai mcp                           # MCP server over stdio
```

- Destination format inferred from `-o` extension, forceable with `--to`.
- stdin/stdout supported (`-` as name) for pipelines, except binary output formats on a
  terminal (standard protection).
- Exit codes: 0 OK, 1 conversion with severe warnings (`--strict` makes them fatal),
  2 input error, 3 unsupported format.

## 6. MCP server (`docsai-mcp`)

Based on `rmcp` (official SDK), **stdio** transport. Logs to stderr exclusively.

Tools exposed:

| Tool | Input | Output |
|---|---|---|
| `convert_to_markdown` | `path` (or `content_base64` + `filename`), `fidelity`, `assets` = `inline-base64`\|`files`, `include_images` | DocMark (text), image payloads, `report` |
| `convert_from_markdown` | `markdown`, `target_format`, optional assets | base64 file or written `path`, `report` |
| `inspect_document` | `path`/`content_base64` | structure JSON (same shape as `inspect --json`) |
| `list_supported_formats` | — | support matrix with status per direction |
| `outline_document` | `path`/`content_base64`, `depth`, `fidelity` | tree of addressable nodes with id, kind, preview, token cost (same shape as `outline --json`) |
| `search_document` | `path`/`content_base64`, `query`, `context`, `limit`, `fidelity` | hits: an address, a selector when there is one, and the words around each match |
| `read_selection` | `path`/`content_base64`, `select`, `fidelity` | self-contained DocMark for those nodes, with etags (same shape as `read --select --json`) |

Decisions:
- Dual **path/base64** mode: local MCP clients (Claude Desktop/Code) pass paths; remote
  ones can pass embedded content. Size limit configurable via environment variable.
- Large responses: DocMark as `text content`; binaries as base64 resource with correct MIME.
- The server is stateless; each tool call is an independent conversion (no persistent temp
  files unless the client asks for `assets=files`).
- The **last three tools are the intended path** (plan v2 Phase 11): map, find, read the part.
  `convert_to_markdown` returns the whole document and is the expensive one; the server says
  so in its `instructions`, which is the only place an agent reads before choosing a tool.
- `include_images` = `none|refs|thumbnails|full`, default **`refs`**. It changes the
  *payload*, never the markdown: the body keeps its `assets/…` links at every rung, so no
  rung is a lossy conversion and a client can come back for `full` later. Every rung reports
  `image_count` and `image_bytes`, because "no images in the response" and "no images in the
  document" must not look alike.

## 7. Cross-platform and distribution

- No system dependencies on the main path (pure Rust). `soffice` is searched at runtime
  in standard locations per OS only for `.doc`.
- CI: GitHub Actions, ubuntu/windows/macos matrix; release artifacts with `cargo-dist`
  (tar.gz/zip + shell/powershell installers; optional: Homebrew tap, Scoop/winget, cargo-binstall).
- Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`,
  `aarch64-apple-darwin`, `x86_64-apple-darwin`. LTO + `strip` on release.

## 8. Performance (indicative budgets)

- 100-page docx → DocMark: < 1 s on ordinary hardware.
- 100k-cell xlsx: < 3 s and < 500 MB RAM (avoid cloning sharedStrings; use `Cow`).
- Readers stream where the crate allows (calamine is lazy per sheet).
- Benchmarks with `criterion` on the corpus from Phase 8; regressions > 20% block PRs.
- Plan v2 budget: 40-slide pptx → DocMark < 1 s (plan v2 Phase 13).

---

## 9. Planned in development plan v2

Everything in §1–§8 describes what is built (Phases 0–7 of plan v1). This section records the
architectural deltas committed by [`development-plan-v2.md`](development-plan-v2.md); it is not
implemented yet. Rationale in
[`technical-analysis-presentations.md`](technical-analysis-presentations.md).

### 9.1 Third IR root: presentations

```rust
pub enum Document {
    Text(TextDocument),
    Workbook(Workbook),
    Presentation(Presentation),      // v2
}

pub struct Presentation {
    pub meta: DocumentMeta,
    pub addressing: Addressing,
    pub styles: StyleCatalog,
    pub layouts: LayoutCatalog,      // slide layouts + masters, as reference targets
    pub slide_size: Size,            // p:sldSz, the canvas geometry is relative to
    pub slides: Vec<Slide>,
    pub skeleton: Option<SkeletonRef>, // the opaque original package (§9.4)
}

pub struct Slide {
    pub id: Option<NodeId>,
    pub layout: Option<LayoutId>,
    pub name: Option<String>,
    pub shapes: Vec<Shape>,          // reading order; source spTree index on each shape
    pub notes: Option<Vec<Block>>,
    pub hidden: bool,
    pub section: Option<String>,     // p14:sectionLst
    pub raw: Vec<RawId>,             // transition/timing subtrees, sidecar-stored
}

pub struct Shape {                   // identity and position belong to every shape
    pub id: Option<NodeId>,
    pub name: Option<String>,        // p:cNvPr@name
    pub z_index: u32,                // index in the source p:spTree
    pub geometry: ShapeGeometry,
    pub kind: ShapeKind,
}

pub enum ShapeKind {
    Placeholder(Placeholder),        // ph_type + idx + body + delta
    TextBox { body: Vec<Block> },
    Picture(ImageRef),               // reuses the existing image model unchanged
    Table(Table),
    Chart(ChartRef),                 // embedded workbook + raw chart XML (Phase 16 fills it)
    Group(Vec<Shape>),
    Raw(RawShape),                   // kind + sidecar payload + the text it shows
}
```

Rules, consistent with §3:
- **The placeholder cascade is stored as reference + delta**, never flattened: a placeholder
  keeps the slide's `layout` and only the properties that differ from the resolved layout/master/
  theme chain.
- `ShapeGeometry` reuses `Length`/EMU and the existing `ImageGeometry` primitives; DrawingML in
  `.pptx` is the same model already implemented for `.docx`/`.xlsx`. Absent position and size
  mean *inherited*, which is the same reference-plus-delta rule the styles follow.
- Reading order is a **policy** (placeholders by type, then top-left), and the original z-order
  index travels as data so the round trip is reversible.
- **A layout names its title and its primary body** (`Layout::title()`, `Layout::body()`).
  Spike P2 made placeholders implicit in DocMark-P — the heading *is* the title — and that is
  resolvable only because the catalogue says which placeholder is which.
- **Addressability follows what DocMark-P can write.** A slide always carries an id; the title
  and the primary body do not, because they are written as a heading and as plain blocks with
  nowhere to put one. `addressing::implicit_shapes` is the single answer to that question, shared
  by the id walker and the serializer so the two cannot disagree.
- No new crate: `.pptx`/`.ppt` live in `docsai-office::pptx`, `.odp` in `docsai-odf::odp`. The
  crate dependency rules of `AGENTS.md` §3 are unchanged.

> `Shape` is a struct with a `kind`, where the sketch above originally had a bare enum:
> identity, name, geometry and source z-order belong to *every* shape, and repeating those four
> fields across seven variants is how they drift apart.

### 9.2 Node addressing (`NodeId` + etag)

Addressable nodes (slide, section, heading, list, table, row, image, sheet, footnote, raw block)
carry a persistent `NodeId`, allocated from a monotonic counter stored in the DocMark front
matter (`next-id`) and **never renumbered or reused**. Each carries a 6-character `etag` over
normalised content, enabling optimistic concurrency for editing. Runs and other fine-grained
nodes are addressed by relative path (`s4.b2:3`), not by id — an id per run is noise nobody pays
for willingly.

### 9.3 Fourth fidelity level: `agent`

`full` / `standard` / `plain` are axes of information loss. `agent` is an axis of *editable
surface*: everything an agent may safely modify is text; everything else is a one-line stub with
id and etag, with the payload in the raw sidecar (`assets/_raw/`) or the skeleton. Round-trip
fidelity is unchanged, because the non-editable truth lives in the preserved package.

### 9.4 Preserved package skeleton

For presentations, the reader stores the non-slide parts of the original package opaquely
through the existing `AssetStore` (content-hash dedupe applies), and the writer re-injects
regenerated slides into that skeleton rather than rebuilding masters, layouts and theme. This is
the raw-block hatch lifted to package level, and it is the mitigation for the "PowerPoint offers
to repair this file" failure mode.

Part names are conventions, so the reader never trusts them: a part is what `[Content_Types].xml`
declares it to be, reached through the relationship graph, and slide order comes from
`p:sldIdLst` alone. `slide3.xml` first in the deck is what PowerPoint writes after a reorder.

The same rule applies inside a slide. A placeholder's font, size and colour are decided by the
layout, the master's `p:txStyles` and the theme; the reader resolves those references —
`+mj-lt` through `a:fontScheme`, `a:schemeClr` through the master's `p:clrMap` — and stores only
what the slide changes over them. The resolved values live on the layout and master
placeholders, which keeps a restyle a restyle: change the theme and every slide follows, because
no slide copied the answer.

### 9.5 CLI surface v2

```
docsai outline <in> [--depth N] [--json]
docsai read <in> --select <selector>          # s4 | s7-s9 | #id | type:notes | text:foo
docsai search <in> <query> [--json]
docsai tokens <in> [--fidelity …] [--json]
docsai edit <in> --ops <ops.json> [--dry-run]
docsai convert … --fidelity full|standard|plain|agent [--raw inline|sidecar]
                 [--ids assign|preserve|never] [--thumbnails]
```

### 9.6 MCP surface v2

| Tool | Purpose |
|---|---|
| `outline_document` | id tree with type, preview and **token cost per node** — lets an agent plan before reading |
| `read_selection` | valid self-contained DocMark for a selector, not the whole document |
| `search_document` | ids + context for a query |
| `apply_edits` | transactional patch operations against ids, with `dry_run`, etag preconditions, and the applied diff + new etags in the response |
| `validate_docmark` | typed errors with node id and suggested fix |

Plus: `include_images=none|refs|thumbnails|full` (default moves from `inline-base64` to `refs`),
a contextual DocMark cheat-sheet exposed as an MCP resource, and a content-hash LRU cache in
`docsai-convert`.

The protocol stays **stateless**: ids live inside the document, not in server-side state, and
the cache is a pure optimisation that can be disabled without changing any output. No sessions,
no locks, no server-held document handles — that boundary is a deliberate decision, not an
omission.
