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

Tools exposed (v1):

| Tool | Input | Output |
|---|---|---|
| `convert_to_markdown` | `path` (or `content_base64` + `filename`), `fidelity`, `assets` = `inline-base64`\|`files` | DocMark (text), assets, `report` |
| `convert_from_markdown` | `markdown`, `target_format`, optional assets | base64 file or written `path`, `report` |
| `inspect_document` | `path`/`content_base64` | structure JSON (same shape as `inspect --json`) |
| `list_supported_formats` | — | support matrix with status per direction |

Decisions:
- Dual **path/base64** mode: local MCP clients (Claude Desktop/Code) pass paths; remote
  ones can pass embedded content. Size limit configurable via environment variable.
- Large responses: DocMark as `text content`; binaries as base64 resource with correct MIME.
- The server is stateless; each tool call is an independent conversion (no persistent temp
  files unless the client asks for `assets=files`).

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
