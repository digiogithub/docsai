# 06 — Phase 2 residual: DOCX writer + round-trip closure

What closed the leftover Phase 2 work after the initial DocMark ⇄ DOCX scaffolding
landed, and what an agent should know before touching the writer again.

## Status

**Phase 2 core path is closed**, including the residual fidelity items that were
still open after the first round-trip session:

| Item | Status |
|---|---|
| DocMark parser (hand-written mirror of serializer) | Done |
| DOCX writer (styles, numbering, media, props, raw re-inject) | Done |
| Floating DrawingML (`wp:anchor`) write | Done |
| Image transforms on write (rotation, flip, crop, border, hyperlink) | Done |
| Full footnote body write (runs/styles, not plain text dump) | Done |
| Nested list `w:ilvl` from IR `List.level` | Done |
| GFM table colspan/rowspan → IR `covered` / spans | Done |
| DocMark top-level fence chunking after `:::` | Done |
| Office → DocMark → Office → DocMark on full docx corpus | Done (identity) |
| `serialize(parse(md))` on all docx goldens | Done |
| proptest IR → md → IR | Still open (non-blocking) |
| Word / LibreOffice open checklist | Still open (manual / env) |

See also [`04-next-phases.md`](04-next-phases.md) and
[`docs/development-plan.md`](../docs/development-plan.md) Phase 2.

## What was wrong (residuals)

1. **Floating images degraded to inline** with a `Warning::Degraded`. The writer
   always emitted `wp:inline`.
2. **Footnote parts flattened to plain text**, so bold/links inside notes were lost
   on the second DocMark pass.
3. **Image geometry on write** ignored rotation, flip, crop, border, and image-level
   hyperlinks.
4. **Round-trip identity failed** on several corpus files for reasons outside the
   drawing path:
   - `split_top_level` inspected blank lines *before* closing a `:::` fence, so
     everything after the first header/footer/table container was swallowed.
   - Nested lists were always parsed at `level = 0`, so the writer emitted
     `w:ilvl=0` for every item and nesting collapsed.
   - GFM empty placeholders for `colspan`/`rowspan` were re-parsed as real cells,
     breaking grid width and `vMerge` reconstruction.
   - List levels without an explicit `start` were written as `w:start=1`, inventing
     `start: 1` in DocMark on the way back.

## What landed

### `docsai-office` writer (`docx/write.rs`)

- `wp:anchor` with `positionH` / `positionV` (offset or align), wrap mode/side,
  `behindDoc`, and `relativeHeight` (z-index).
- `pic:spPr` / `a:blipFill` carry rotation (`rot` in 60000ths of a degree), flips,
  `a:srcRect` crop, and simple `a:ln` borders.
- Image hyperlinks as `a:hlinkClick` on `wp:docPr` with an external relationship.
- Footnote bodies are rendered with the same `write_block` path as the main body
  when the reference is collected (stored as pre-rendered XML).
- Numbering levels omit `w:start` when the IR has no explicit start.
- Covered table cells emit `w:vMerge` continuations.

### `docsai-docmark` parser

- Fence depth is updated **before** blank-line chunk splits.
- Nested list content is parsed with `depth + 1`.
- `normalize_gfm_spans` drops colspan pads and marks rowspan pads `covered=true`.

### Tests

- `docsai-convert` goldens: `docx_roundtrip_is_idempotent` over every
  `corpus/docx/*.docx`.
- `docsai-convert` goldens: `serialize_parse_is_identity_on_docx_goldens`
  (seeds `AssetStore` from the companion `.docx` because loose media is not
  checked into `corpus/`; keeps `docsai-docmark` free of an `office` dependency).
- `docsai-docmark`: expanded `serialize(parse)` coverage for non-media goldens
  (headers, tables, footnotes, fields, styles).
- Writer unit test for floating geometry + bold footnote content.

## Commands

```bash
cargo test --workspace
cargo test -p docsai-convert --test goldens
cargo run -p docsai-cli -- roundtrip corpus/docx/images-floating.docx --json
```

## Still out of scope / known gaps

| Item | Note |
|---|---|
| Text boxes as first-class write | Still flattened with a warning; IR + spec exist |
| proptest IR → DocMark → IR | Phase 2 task 5; generator already exists in model tests |
| Word / LibreOffice repair-dialog checklist | Needs desktop apps or headless soffice in CI |
| Anonymized real-world docx corpus | Phase 1 task 10, still pending sources |
| DrawingML effects → `effects_raw` | Detected/warned; dump still incomplete |

## Rules reinforced

1. Never degrade floating geometry silently — write `wp:anchor` or warn with a typed
   reason (sheet anchors in a docx are the remaining degrade path).
2. Do not invent OOXML defaults that change DocMark (e.g. unconditional `w:start=1`).
3. GFM is lossy for spans; the parser must rebuild `covered` / colspan the way the
   docx reader does.
4. Top-level `:::` containers are separate chunks only when fence depth is zero
   *after* processing the closing line.
