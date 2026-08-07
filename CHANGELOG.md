# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`Document::Presentation`, the third IR root** (plan v2 Phase 13, first increment): a
  `Presentation` of layouts, masters and slides, with `Shape` carrying identity, name, geometry
  and its index in the source `p:spTree` so the reading-order policy is reversible. The
  placeholder cascade is stored the way styles already are — **reference plus delta** — and a
  layout names its title and its primary body, which is what lets DocMark-P write the title as
  the slide heading (spike P2). Slides carry a stable id; the title and primary body do not,
  because there is nowhere in the profile to write one, and `addressing::implicit_shapes` is the
  single rule that says so for both the id walker and the future serializer.
  `pptx`/`pptm` is now a recognised format, honestly reported by `docsai formats` as **not yet
  readable** — the reader is the next increment. Converting a presentation to DocMark produces
  an empty body and a severe warning until DocMark-P lands in Phase 14; it never fails silently.
  `ConversionStats` gains `slides`, and `inspect` reports `kind: "presentation"`.

- **The three addressing primitives over MCP** (plan v2 Phase 11, closing the phase):
  `outline_document`, `search_document` and `read_selection`, the same answers as
  `docsai outline`, `docsai search` and `docsai read --select`, over a filesystem `path` or
  `content_base64` + `filename` like every other tool. They are the intended path for a
  document of any size — map it, find where it says the thing, read back that part as
  self-contained DocMark with an etag per node — and the server's `instructions` now say so,
  since that is the only text an agent reads before choosing a tool. `convert_to_markdown`
  stays what it was: the whole document, the expensive one. On the 9 000-token report the
  three cost 3.8 %, 4.2 % and 0.3 % of reading it.
- **`include_images=none|refs|thumbnails|full`** on MCP `convert_to_markdown`, default
  **`refs`** (see *Changed*). `none` sends no image payload, `refs` sends each image's name,
  MIME type and size, `thumbnails` adds a PNG downscaled to 256 px on the long side, and
  `full` sends the original bytes. The **markdown never changes**: the body keeps its
  `![](assets/…)` links at every rung, so no rung is a lossy conversion and a client that
  started cheap can come back for `full`. Every rung reports `image_count` and `image_bytes`
  — an empty payload list must not read as a document without images. A thumbnail never
  costs more than the image it stands for: for the icons and logos already smaller than the
  box, re-encoding would *add* bytes, so the original is sent instead. Images that no
  pure-Rust decoder in the dependency budget reads (EMF, WMF, SVG) come back as a ref with
  the reason on the row, never dropped in silence.
- `docsai_convert::outline_input`, `select_input` and `search_input`: the three primitives
  over a `SourceInput`, so they answer about the same document from a path or from bytes.
  The `*_path` functions remain, as wrappers.
- **`docsai search <in> <query>`** (plan v2 Phase 11): where a document says something, with an
  address and the words around each match — never the document. The query is a case-insensitive
  literal; `--context N` (default 48) sets the characters quoted either side of a match,
  `--limit N` (default 20) how many blocks are listed, and the rest are counted rather than
  dropped. The unit is the DocMark **block**, not the addressed node, because ordinary prose
  carries no id (spec §11.1): a block with an id is reported at it together with the selector
  that reads it back, and a block without one is reported relative to the last id before it
  (`n12.b2`). A footnote is reported at the block that refers to it, since it cannot be selected
  on its own. Searching a 9 000-token report for a phrase that appears 35 times costs 386 tokens
  (4.2 %). A relative hit deliberately names **no** selector: `read --select` has no `.bN` term
  yet, and an address that would read something else is worse than none.
- **`docsai read <in> --select <selector>`** (plan v2 Phase 11, DocMark spec §2.1): part of a
  document as valid, self-contained DocMark. Selector terms are `s4` and `s7-s9` (positions in
  the order `docsai outline` prints, 1-based, inclusive), `#n7`, `type:heading` and `text:foo`;
  comma-separated terms are unioned and the output is always in document order. The body is the
  bytes the whole document wrote for those nodes, so nothing is re-derived; the front matter is
  the minimum needed to parse and re-write it — version, source format, the *source document's*
  `next-id`, `partial: true` and an `etags:` map — and deliberately carries no metadata, page
  geometry, catalogue or attribute-set dictionary. A footnote is not selectable on its own, and a
  selected block that refers to one always carries its definition. Two headings of a 9 000-token
  report cost 26 tokens.
- **Etags in the output**, at last (spec §2.1, §11.1): Phase 10 computed them and wrote none. A
  selection now carries one per addressed node, recomputed on every write rather than stored, so
  an edited node's etag moves with it and an if-match write-back has something to check.
- `Warning::PartialDocument`: raised on every serialisation of a document with `partial: true`.
  Severe on purpose — the loss is not in that document, it is in the one a careless whole-file
  write would replace with it.
- `docsai_docmark::Options::dictionary`, and `selection_front_matter`. A selection turns the
  attribute-set dictionary off so it depends on nothing outside itself.
- **Readable units** (plan v2 Phase 11, DocMark spec §2): a length is now written in the unit of
  **what it measures** rather than in whichever unit happened to divide it exactly — points for
  layout and typography (indents, margins, column widths, list levels), pixels for drawings and
  bitmaps. An indent of 720 twips reads `36pt` instead of `48px`, and zero carries no unit at all
  (`0`, not `0px`).
- `docsai convert --precision N` (default 2): how many decimals a readable unit may use before a
  length falls back to `emu`. It buys readable units, never rounding — `1.251cm` is written
  `450360emu` at precision 2 and `1.251cm` at precision 3, never rounded to `1.25cm`. The
  round-trip tolerance for a length therefore stays **zero** at every precision, which
  `crates/docsai-docmark/tests/readable_units.rs` checks over every Word twip value.
- `docsai_model::units::LengthStyle` and `Length::render(style, precision)`, the one place the
  rule lives. `Display` keeps the geometric rendering for logs and messages.
- **Attribute-set dictionary** (plan v2 Phase 11, DocMark spec §3.7): a pattern of attributes
  that repeats at least three times and is at least twelve characters long is written once in the
  front matter under `attribute-sets:` and referenced by class in the body (`{.g1}`). The reader
  expands the class into its pairs before anything interprets the block, so the IR is identical
  either way and no consumer can tell a dictionary was used; a pair written on the node wins over
  the entry's. Names follow first appearance and skip anything the document already uses — a
  style id, a list name, a structural class — so the dictionary is a function of the document and
  stays part of serializer determinism. Active at `full` and `standard`. Measured on
  `docx/repeated-formatting.docx`: 725 → 616 tokens at `full` (−15 %) and 499 → 390 at
  `standard` (−22 %).
- Corpus fixture `docx/repeated-formatting.docx`: the same direct formatting applied by hand over
  and over, which no style implies and the economy rule therefore cannot remove. Without it the
  dictionary would be measured on a corpus that repeats nothing.
- **Delta emission against the whole inheritance chain** (plan v2 Phase 11): a run is now
  resolved against its **paragraph's** style before its own, which is the middle of the OOXML
  cascade and the level the serializer used to skip — text typed after applying a style carries
  that style's run properties again in `w:rPr`, and all of it was being written out. A style in
  the catalogue emits only what its `based-on` chain does not already say; the default paragraph
  style is no longer named on a paragraph; a heading omits the class its `#` level already names
  (and the parser puts it back). Every omission is reversible: a value equal to the inherited one
  resolves to the inherited one.
- Corpus fixture `docx/redundant-formatting.docx`, which carries that redundancy on purpose —
  600 → 528 tokens with the change, and the existing corpus 24 883 → 24 667 (−0.9 %), small only
  because generated fixtures were already clean.
- **`--fidelity agent`** (plan v2 Phase 11), a fourth level and the first that is a *projection*
  rather than a conversion: text, structure, node ids and raw stubs, without the style and list
  catalogues, indents, spacing, page geometry, image geometry or column widths. A sheet keeps its
  formulas and merges — those are what a sheet is. The output declares `fidelity: agent` in its
  front matter and is meant to be read whole and written back node by node, so ids are assigned
  as at `full`. Measured on the corpus: 62–75 % fewer tokens than `full` on 24 of the 25 text
  documents; the exception is the one whose cost is prose rather than formatting.
- `corpus/token-budget.md` now has an `agent` column, so the projection's cost is gated in CI
  like every other level.
- **Raw-block sidecars** (plan v2 Phase 11): at `--fidelity full` the bytes of a raw-block go
  to `assets/_raw/<id>.xml` and the body keeps a stub, `::: {.raw … src="assets/_raw/r7.xml"}`.
  `docsai convert --raw inline|sidecar` (default `sidecar`) chooses; `--raw inline` restores the
  1.0 form. A missing or unreachable sidecar is a typed error, never a silent loss.
  `docsai_docmark::raw::raw_sidecars` gives a caller the files a serialisation refers to.
- Corpus fixture `docx/fields-raw.docx` now carries block-level OMML maths, so the raw-block
  path — and its sidecar — is exercised end to end by the corpus instead of only by unit tests.
- **DocMark 1.1 — stable node addressing** (plan v2 Phase 10): addressable nodes carry
  `{#n7}` and the front matter declares `next-id`, a monotonic counter that is never
  renumbered on insertion and never reused after deletion. Ids ride in the attribute
  block the node already has (headings, container paragraphs, images, tables, complex
  table rows, sheets, multi-section containers), on the first item for lists
  (`list-id=`) and on the reference for footnotes (`[^1]{#n9}`).
- `docsai convert --ids assign|preserve|never`; the default is `assign` at
  `--fidelity full` and `never` at the lossy levels. `plain` never carries ids.
- `docsai-model::addressing`: `NodeId`, `Etag`, `IdPolicy`, the `Addressable` trait and
  the document walkers (`assign_ids`, `for_each_addressable`, `node_ids`). Etags are a
  6-character hash of the node's normalised content, derived on demand and never stored.
- **`docsai tokens <in> [--fidelity …] [--ids …] [--top N] [--json]`** — the cost of a
  document measured with a real BPE tokenizer (`tiktoken-rs`, `o200k_base`, vocabulary
  embedded: no network, no Python), split into front matter and body, plus the cost of
  every addressed node over the exact DocMark it wrote. Rationale for the tokenizer
  choice in `docs/technical-analysis.md` §4.4.
- **`docsai outline <in> [--depth N] [--fidelity …] [--json]`** — the tree of addressable
  nodes with id, kind, a ~60-character preview and the measured token cost of each, plus what
  the whole document would cost. On the largest corpus document the outline is 3.8 % of the
  document's own tokens (Phase 10 budget: under 5 %).
- **Corpus token budget** `corpus/token-budget.md`: what every corpus document costs at each
  fidelity level, committed as a golden and checked in CI. An update that inflates the corpus
  total by more than 5 % is refused unless `DOCSAI_ACCEPT_TOKEN_INFLATION=1` says so.
- Corpus fixture `docx/long-report.docx` (~9 000 tokens): the first corpus document sized for
  measuring rather than for isolating a trait.
- `docsai_docmark::serialize_traced` and `NodeFragment`: the same output as `serialize`,
  byte for byte, plus what each addressed node contributed, with `descendants` recording how
  many fragments it contains so the flat list rebuilds into a tree. `docsai-convert::tokens`
  (`token_report`, `token_report_path`) and `docsai-convert::outline` (`outline`,
  `outline_path`) build on it.

- **Phase 7 MCP server**: `docsai mcp` over stdio (`rmcp`) with tools
  `convert_to_markdown`, `convert_from_markdown`, `inspect_document`, and
  `list_supported_formats`. Path and base64 input modes, inline or on-disk assets,
  `DOCSAI_MCP_MAX_INPUT_BYTES` / `DOCSAI_MCP_TIMEOUT_SECS` limits.
- `docsai-convert::service` helpers for in-memory / bytes conversion shared by MCP.
- `docsai inspect` — structure report (metadata, styles, sections/sheets, media, stats)
  without converting; `--json` for the machine-readable shape shared with MCP.
- Batch conversion: `docsai convert *.docx --out-dir md/` with rayon parallelism and an
  aggregated summary (`--json` supported).
- Stdin/stdout pipelines: pass `-` as the input and/or `--output -` for DocMark text.
- `--style-map <file>` publication mode (DocMark spec §5): map source styles to `h1`–`h6`,
  `p`, `ignore`, or `code-block` (unidirectional).
- `--max-cells N` safety cap for workbook conversions.
- Actionable CLI error hints for unknown formats, unsupported conversions, and LibreOffice.
- `cargo-dist` configuration and release installers (shell/powershell) for the five
  primary targets.

### Changed

- **BREAKING (MCP): `convert_to_markdown` no longer returns image bytes by default.** The
  default is now `include_images=refs` — name, MIME type and size per image — where it used
  to be the whole base64 payload. On a document holding one 1200 × 900 screenshot the
  response drops from **906 709 bytes to 2 289** (0.25 %); at `thumbnails` it is 83 956
  (9.3 %), still a picture the client can look at.

  *Migration*: a client that wants the old behaviour passes `include_images: "full"`. A
  client already passing `assets: "inline-base64"` **needs no change at all** — an explicit
  request is still honoured, only the default moved. The `markdown` field is byte for byte
  identical at every rung, so a client that only reads the text is unaffected.

  Why break it: an image outweighs every word around it once it is base64, which made the
  tool unusable on exactly the documents an agent most wants to read. The old default
  charged for content the caller had not asked for and usually could not use; the new one
  makes wanting the pixels an explicit choice.
- MCP `list_tools` now returns **seven** tools rather than four.
- `--fidelity full` output is now **DocMark 1.1** and declares `next-id`; documents
  without ids (and `--ids never`) still declare `1.0`. A 1.0 document parses unchanged
  and gains ids on its next write. MCP `convert_to_markdown` inherits the new default.
- `Inline::Footnote` now carries a `Footnote { id, blocks }` and `Inline::Image` boxes
  its `ImageRef` (library API change; serialised JSON is unchanged).
- CLI `--help` documents the full Phase 6 command surface with examples.
- README usage section covers inspect, batch, style maps, and install paths.
- Workspace MSRV raised to **1.88** (required by `rmcp` 3.x).

## [0.1.0] — 2026-08-01

### Added

- Initial public workspace: Phases 0–5 core path (docx/odt/xlsx/ods ⇄ DocMark, xls read,
  legacy `.doc` read with optional LibreOffice fallback).
- `docsai convert`, `formats`, and `roundtrip` commands.
