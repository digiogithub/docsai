# 11 — On-ramp for plan v2 (agent-native + presentations)

What [`docs/development-plan-v2.md`](../docs/development-plan-v2.md) will find **already
solved** in the tree, what it must **not** re-do, and the traps already known from Phases 0–7
that will bite again in the new work.

Plan v1 (`docs/development-plan.md`) is delivered and deprecated; its open items live on in
plan v2 Phase 20.

---

## Reusable as-is (do not rebuild)

| Asset | Where | Why it transfers |
|---|---|---|
| OPC / ZIP + `_rels` resolution | `docsai-office` | `.pptx` is the same package model as `.docx`/`.xlsx` |
| `AssetStore` with content-hash dedupe + manifest | `docsai-convert` | Media, and now the preserved package **skeleton**, are just blobs |
| `ImageRef` / `ImageGeometry` / `Anchor` | `docsai-model` | DrawingML in `.pptx` is the model already implemented for `.docx`/`.xlsx` — no new image model |
| `Length` / EMU newtypes | `docsai-model` | Slide geometry is EMU throughout |
| Raw-block hatch + typed `Warning` | model + docmark | Plan v2 moves the payload to a sidecar; the mechanism does not change |
| Style = reference + delta | readers | The placeholder cascade is the same idea applied to layout/master/theme |
| Golden + idempotence test harness | `docsai-convert/tests` | pptx goldens plug into it unchanged |
| `corpus/generate.py` discipline | `corpus/` | pptx corpus is generated with `python-pptx` the same way |
| xlsx reader | `docsai-office` | Chart data in `.pptx` **is** an embedded xlsx workbook |
| LibreOffice detection + `--use-loffice` | `docsai-convert` | Reused verbatim for `.ppt`, and now also for slide thumbnails / render-diff CI |
| In-memory convert helpers | `docsai-convert::service` | New MCP tools must go through these, never import format crates |

## What is genuinely new

1. **Third IR root** `Document::Presentation` (architecture §9.1). Not a `TextDocument` variant:
   a slide is a canvas of positioned shapes with no semantic reading order.
2. **Node addressing** (`NodeId` + etag, monotonic `next-id` in the front matter). Touches
   `docsai-model`, both DocMark directions, and every golden.
3. **Raw-block sidecar** and **`--fidelity agent`** — the token levers.
4. **Preserved package skeleton** — the anti-repair mechanism; conceptually raw-block at package
   level.
5. **Two new CI gates** the current suite lacks: XSD schema validation and headless-LibreOffice
   render diff. Goldens do not catch "PowerPoint offers to repair this file".

## Known traps that will bite again

- **Rounding of lengths.** Phase 1 shipped a bug where a 1417-twip margin serialized as
  `2.499cm`. Plan v2 deliberately serializes geometry in readable units *with a documented
  tolerance* — do not reintroduce exact-equality comparison on geometry, and keep `emu` as the
  exact escape hatch.
- **Empty content silently vanishing.** Empty paragraphs disappeared in Phase 1; empty
  placeholders are the pptx equivalent and are real content (they hold layout position).
- **Double-emitted properties.** A hyperlink's character style was emitted twice in Phase 1. The
  placeholder cascade multiplies the chances: emit deltas only, and test that a placeholder
  inheriting everything emits *zero* attributes.
- **Dialect raw blocks do not cross formats.** `pptx→odp` drops them with a warning, exactly as
  `docx→odt` does today. Same behaviour, same warning type — do not invent a new one.
- **Ids are a correctness feature, not a convenience.** A renumbered or reused id makes an agent
  edit the wrong node silently. That is why plan v2 Phase 10 has a dedicated property test
  (risk P7) instead of a passing round-trip being enough.

## Rules that still bind

- Crate dependency rules of `AGENTS.md` §3 are unchanged: `.pptx`/`.ppt` go in
  `docsai-office::pptx`, `.odp` in `docsai-odf::odp`. **No new crate**, no format crate importing
  another.
- No panic on corrupt input, ever: typed `Err`.
- Nothing is lost silently: stub + sidecar raw + typed warning.
- MCP stays stateless. Ids live in the document; the content-hash cache is a pure optimisation
  that can be turned off without changing any output.
