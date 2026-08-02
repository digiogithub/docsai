# 02 — Project structure

## Tree

```
docsai/
├── Cargo.toml                  # workspace: 7 members + shared dependencies
├── .github/workflows/ci.yml    # build/test on 3 OSes + fmt/clippy/rustdoc
├── crates/
│   ├── docsai-model/           # the IR. No I/O, no heavy dependencies
│   ├── docsai-docmark/         # DocMark serializer + parser (Phase 2)
│   ├── docsai-office/          # .docx reader (xlsx Phase 3, doc Phase 5)
│   ├── docsai-odf/             # ODT/ODS readers and writers (Phase 4)
│   ├── docsai-convert/         # orchestration: detection, pipelines, assets, MCP I/O
│   ├── docsai-cli/             # `docsai` binary (incl. `mcp` subcommand)
│   └── docsai-mcp/             # MCP stdio server (Phase 7, rmcp)
├── corpus/
│   ├── generate.py             # generates ALL corpus files
│   ├── README.md               # what each document isolates
│   ├── docx/*.docx             # 14 documents + 14 *.expected.dmk.md (goldens)
│   └── xlsx/*.xlsx             # 6 workbooks, for Phase 3
├── docs/                       # design documentation (English)
│   └── spikes/                 # spike reports with their decision
└── kb/                         # this knowledge base
```

## The seven crates

| Crate | Type | Lines | Responsibility | Depends on |
|---|---|---|---|---|
| `docsai-model` | lib | ~3,500 | The IR and common types | *(nothing from the workspace)* |
| `docsai-docmark` | lib | ~1,850 | IR ⇄ DocMark | model |
| `docsai-office` | lib | ~3,500 | OOXML and legacy readers/writers | model |
| `docsai-odf` | lib | — | ODF readers/writers | model |
| `docsai-convert` | lib | — | Detection, pipelines, assets, MCP bytes helpers | the four above |
| `docsai-cli` | bin | — | The `docsai` binary (`convert`/`inspect`/`mcp`/…) | convert, mcp, model |
| `docsai-mcp` | lib | — | MCP server over stdio (`rmcp`) | convert, model |

**Dependency rule (`AGENTS.md` §3)**: no format crate imports another format crate.
`docsai-mcp` depends only on `docsai-convert` (and `docsai-model` for types); it must
not import `docsai-office` / `docsai-odf` / `docsai-docmark` directly.

## `docsai-model` — the IR

| Module | Contents |
|---|---|
| `units` | `Length` (newtype over EMU), `Size`, `Point`, exact conversions and output formatting |
| `style` | `StyleCatalog`, `Style`, `FontProps`, `ParaProps`, `DocDefaults` and cascade resolution |
| `list` | `ListCatalog`, `ListDef`, `ListLevel`, `NumFormat` |
| `text` | `TextDocument`, `Section`, `PageGeometry`, `Block`, `Inline`, `Paragraph`, `Table`, `RawFragment` |
| `sheet` | `Workbook`, `Sheet`, `Cell`, `CellRef` (A1), `Formula`, `NumFmt` — populated in Phase 3 |
| `image` | `ImageRef`, `ImageGeometry`, `Anchor` and related types (wrap, crop, flip, borders) |
| `assets` | `AssetStore` (trait), `MemoryAssetStore`, content hash, sniffing and dimensions |
| `report` | `ConversionReport`, typed `Warning`, `Severity`, `ConversionStats` |
| `validate` | Invariants: sheet anchors only in `Workbook`, rows wider than the grid, inverted `two-cell` anchors |

Three principles encoded in the types, which should not be broken:

1. **Style = reference + delta.** Every field of `FontProps`/`ParaProps` is `Option`: `None`
   means *inherit*, not *disabled*. `over()` merges the cascade; `minus()` computes the delta
   that must be emitted (the “economy rule” of spec §3.1).
2. **All lengths in EMU.** A single integer, lossless, for OOXML (EMU/twips),
   ODF (cm/in), and `.doc` (twips).
3. **The IR does not know about I/O.** Images are an `AssetId` behind a trait.

## `docsai-office` — the docx reader

| Module | Responsibility |
|---|---|
| `xml` | XML tree over `quick-xml` with **byte spans** per node |
| `package` | ZIP, OPC relationships, anti-bomb limits, name sanitization |
| `detect` | Format detection **by content**, not by extension |
| `docx/format` | `w:rPr` and `w:pPr` → the IR delta types |
| `docx/styles` | `styles.xml` → `StyleCatalog`; heading level |
| `docx/numbering` | `numbering.xml` → `ListCatalog` (flattens the double indirection) |
| `docx/drawing` | DrawingML and VML → `ImageRef`/`ImageGeometry` |
| `docx/body` | Body traversal: blocks, inlines, fields, tables, lists |
| `docx/mod` | Assembly: properties, sections, headers/footers, footnotes |

The detail that holds everything together: **every node in the XML tree remembers its byte
range in the source**. An unrecognized element is preserved by citing the original bytes, not a
re-serialization, and that is why the raw-block is exact.

## `docsai-docmark` — the serializer

| Module | Responsibility |
|---|---|
| `escape` | Fixed escape table by context (block, table cell, link label) |
| `attrs` | `{#id .class key="value"}` blocks with the canonical order from spec §8 |
| `units` | How a length, a percentage, or a number is written |
| `frontmatter` | The YAML, handwritten to guarantee byte-for-byte determinism |
| `writer` | IR traversal: blocks, inlines, images, tables, containers |

## Conversion flow

```
file.docx
    │  docsai-convert::read_document
    ▼
detect (by content) ──► docsai-office::read_docx
                                │  Package::open  → parts in memory, sanitized names
                                │  Element::parse → XML tree with spans
                                │  styles + numbering + footnotes
                                │  read_sections → blocks, with DirAssetStore receiving media
                                ▼
                          (Document, ConversionReport)
                                │  docsai_model::validate
                                ▼
                       docsai-docmark::serialize
                                │  front matter + body, according to --fidelity
                                ▼
                    file.dmk.md  +  assets/img-<hash8>.<ext>
```

## External dependencies

The entire core rests on five crates:

| Crate | Purpose | Where |
|---|---|---|
| `serde` | IR serialization | model and derivatives |
| `thiserror` | Typed library errors | model, docmark, office, convert |
| `quick-xml` | XML parsing | office |
| `zip` | OOXML containers | office |
| `tracing` | Logs, always to stderr | office, convert, cli |

And in the binaries: `clap` (CLI), `anyhow` (binary errors), `serde_json` (`--json`),
`tracing-subscriber`. In tests: `proptest` and `comrak`.

Deliberately **absent**: `docx-rs` (rejected by spike R1), `image` (reading the format
header is enough), `serde_yaml` (front matter is handwritten and the crate is unmaintained),
and `insta` (goldens are reviewable text files). Each absence is justified in
[`docs/technical-analysis.md`](../docs/technical-analysis.md) §4, as required by
`AGENTS.md` §2.

## Where the tests live

| Location | What it covers |
|---|---|
| `#[cfg(test)]` modules in each file | Units: conversions, escaping, style cascade, XML fragment parsing |
| `crates/docsai-model/tests/json_roundtrip.rs` | IR JSON round-trip: hand-built documents with all nodes + proptest |
| `crates/docsai-office/tests/docx_images.rs` | Image geometry over the real corpus |
| `crates/docsai-office/tests/robustness.rs` | The 900+ corrupt inputs, path traversal, ZIP without document |
| `crates/docsai-convert/tests/goldens.rs` | Goldens, determinism, line endings, `plain` with comrak, performance |

## Conventions worth respecting

- Code names, comments, commit messages, and **all documentation in `docs/` and `kb/`** must be
  in **English** (`AGENTS.md` §5).
- `thiserror` in libraries, `anyhow` only in binaries.
- Parsers **never panic**: no `unwrap`, `expect`, or unchecked indexing on the
  read path.
- The serializer is **deterministic**: same IR ⇒ same bytes. Any iteration over a
  map uses `BTreeMap`, never `HashMap`.
- Paths with `std::path`, never concatenating `/`.
