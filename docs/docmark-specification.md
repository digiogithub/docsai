# DocMark Specification v1.0

**DocMark** is the extended Markdown profile used by `docsai` as its textual pivot format.
Goal: represent text documents (docx/odt/doc) and spreadsheets (xlsx/xls/ods) in a way that is
**readable, hand-editable, and rich enough to regenerate the original document with minimal
loss**.

Status: **frozen (v1.0)** at the close of Phase 0. Any later change bumps the version declared
in the front matter and requires documenting the migration (`AGENTS.md` §2).

Two version bumps are already committed by
[`development-plan-v2.md`](development-plan-v2.md) and sketched in §11 of this document:
**1.1** (node ids and etags, raw-block sidecar, `agent` fidelity level — plan v2 Phases 10–11)
and **1.2** (the presentation profile, "DocMark-P" — plan v2 Phase 14). Both are additive: a
document written against 1.0 stays valid.

Annex A (complex tables) is resolved in §3.4. Draft TODOs are closed; additions made explicit by
the Phase 1 implementation are marked in the text and collected in §10.

## 0. Principles

1. **Downward compatibility**: a DocMark file is valid CommonMark+GFM. A viewer that ignores
   attributes and containers still shows a useful document.
2. **Pandoc attribute syntax**: `{#id .class key="value"}` — no new syntax is invented where
   Pandoc already has one.
3. **Everything non-representable is preserved**, never discarded: the `raw-block` mechanism
   (§7).
4. **Determinism**: two serializations of the same IR produce identical bytes (required for
   golden tests and round-trip idempotence). Line endings are always `\n`, UTF-8 without BOM.

## 1. File conventions

- Recommended extension: `.dmk.md` (double extension: editors treat it as Markdown).
- One text document ⇒ one file. One workbook ⇒ one file with one H1 section per sheet.
- Media are extracted to a sibling `assets/` directory (configurable with `--assets-dir`).

## 2. Front matter (YAML)

Initial YAML block delimited by `---`. Defined fields:

```yaml
---
docmark: "1.0"                  # version of this specification (required)
source-format: docx             # docx | doc | odt | xlsx | xls | ods (required when converting)
fidelity: agent                 # only on a projection (§6.1); absent means the whole document
title: "Annual Report"          # docProps / meta.xml
author: "Ana Perez"
created: 2026-03-01T10:00:00Z
modified: 2026-07-15T09:30:00Z
language: en-US
custom-properties:              # document custom properties, as-is
  Department: "Sales"

page:                           # default page / section geometry (text documents)
  size: A4                      # or explicit width/height
  margins: { top: 2.5cm, bottom: 2.5cm, left: 3cm, right: 3cm }
  orientation: portrait

style-defaults:                 # OOXML `w:docDefaults`: the base of the cascade
  font: { name: "Calibri", size: 11pt }
  paragraph: { space-after: 8pt }

styles:                         # style catalog from the original document (§3)
  Heading1:
    type: paragraph
    based-on: Normal
    font: { name: "Calibri Light", size: 16pt, color: "#2E74B5", bold: true }
    paragraph: { space-before: 12pt, space-after: 4pt, keep-with-next: true }
  Emphatic:
    type: character
    font: { italic: true, color: "#C00000" }

list-definitions:               # numbering.xml / normalized ODF list styles
  L1:
    levels:
      - { format: decimal, text: "%1.", indent: 0.63cm }
      - { format: lowerLetter, text: "%2)", indent: 1.27cm }
---
```

Rules:
- Keys in kebab-case. Explicit units (`px`, `cm`, `pt`, `emu`, `%`); colors `#RRGGBB`.
- **Unit rule (normative)**: the serializer chooses the **first unit that represents the length
  exactly**, in the order `px` (at 96 dpi) → `cm` → `pt` → `emu`. Readability yields to exactness:
  a Word margin of 1417 twips is written `70.85pt`, never an approximate `2.499cm`, because
  rounding would shift the margin on every round-trip. `emu` is the final escape hatch and always
  exists.
- The `styles` block is a **catalog**: the body references styles by name via classes
  (`{.Heading1}`); the inverse writer uses it to regenerate `styles.xml` / ODF `styles.xml`.
- Unknown fields are preserved (parsers must not reject them): forward-compatible.

## 3. Text blocks

### 3.1 Headings and paragraphs

```markdown
# Chapter title {.Heading1}

A normal paragraph (default style, no attributes).

Paragraph with style and direct formatting. {.Quote align=center space-after=12pt}
```

- The `#` level reflects the outline level; the class indicates the actual style.
- Paragraph attributes go in `{...}` **at the end of the block**: `align`,
  `indent-left`, `indent-right`, `indent-first-line`, `indent-hanging`,
  `space-before`, `space-after`, `line-height`, `background`, `keep-with-next`,
  `page-break-before`, `outline-level`.
- An **empty** paragraph is real content (documents are spaced that way) and a blank line cannot
  represent it, because Markdown absorbs it: in `full` mode it is written `[]{.empty}`.
  `standard` and `plain` modes discard it, which is exactly what they promise (§6).
- Economy rule: if formatting matches exactly what the style defines, redundant attributes are
  **not** emitted (keeps Markdown clean and diffs stable).

### 3.2 Inline formatting

| Original | DocMark |
|---|---|
| bold / italic / strikethrough | `**x**`, `*x*`, `~~x~~` (native GFM) |
| underline | `[text]{.underline}` |
| color, highlight, font, size | `[text]{color="#FF0000" highlight="yellow" font="Arial" size=14pt}` |
| character style | `[text]{.Emphatic}` |
| sub/superscript | `[x]{.sub}` / `[x]{.sup}` |
| small caps / all caps | `[x]{.small-caps}` / `[x]{.caps}` |
| non-simple underline | `[x]{.underline underline=double}` (values: `double`, `thick`, `dotted`, `dashed`, `wave`) |
| formatting turned off over a style | `[x]{bold=false}` / `[x]{italic=false}` |
| manual line break | trailing double space (hard break) |
| page or column break | `[]{.break kind=page}` / `[]{.break kind=column}` |
| hyperlink | `[text](https://...)` — with attributes if styled: `[text](url){.Hyperlink}` |
| footnote | `[^1]` + definition at the end (Pandoc/GFM footnotes syntax) |
| dynamic field | `[value]{.field field=PAGE}`, with `instr="…"` in `full` mode |

### 3.3 Lists

Standard Markdown lists; typographic definition lives in `list-definitions` and is referenced on
the first item when it is not the default list:

```markdown
1. First point {.ListParagraph list=L1}
2. Second point {.ListParagraph}
   1. Sub-point (the real marker is defined by L1 level 2) {.ListParagraph list=L1}
```

- `list=` travels **inside the first item's attribute block**, not in a separate block: an item
  never carries two `{...}`.
- Nested lists are written *tight* (no blank line between the item and its sublist) so a
  CommonMark viewer does not wrap each item in a `<p>`.
- The ordinal marker written is `1.`, `2.`… The real presentation marker is defined by the
  corresponding level of `list-definitions`.

### 3.4 Tables (text documents)

GFM tables when they are regular. A table with merged cells, fixed widths, or a table style is
wrapped in a container that supplies the metadata:

```markdown
::: {.table style=TableGrid col-widths="3cm,5cm,5cm"}
| Concept | T1 | T2 |
|---|---|---|
| Sales {rowspan=2} | 100 | 200 |
| | 300 | 400 |
:::
```

- `rowspan`/`colspan` on the **first cell** of the merged area; absorbed cells are left empty.
- The container accepts `style`, `col-widths`, and `header-row=false`. GFM requires a header row:
  when the original had none, an empty row is emitted and `header-row=false` records that, so the
  round-trip does not invent a header.
- If the structure exceeds what GFM can represent (nested tables, multi-paragraph cells), the
  full table is emitted as a `::: {.table complex=true}` container with rows and cells as
  sub-blocks:

````markdown
::: {.table complex=true style=TableGrid}
::: {.row}
::: {.cell rowspan=2}
First paragraph of the cell.

Second paragraph of the same cell.
:::
::: {.cell}
Another cell.
:::
:::
:::
````

  Complex format rules: one `::: {.row}` per row, one `::: {.cell}` per cell **including
  absorbed ones** (which are empty), `colspan`/`rowspan` on the cell that opens the area, and
  inside each cell any valid DocMark block. This closes draft annex A.

### 3.5 Images and objects

Every image is extracted to `assets/` and referenced with standard Markdown image syntax plus a
normalized set of attributes that capture the original's full geometry (size, position, anchor,
crop, rotation…). The same attribute model applies in text documents and spreadsheets (§4.1).

```markdown
Inline image (flows with the text):

![Sales diagram](assets/img-3f2a91.png){width=450px height=300px title="Figure 1"}

Floating image with position and text wrap:

![Logo](assets/img-9c04b7.png){#img-logo width=3.5cm height=3.5cm
  anchor=floating relative-to=margin x=1.2cm y=0.5cm
  wrap=square wrap-side=right z-index=2
  rotation=0 crop="0,0,10%,0" native-size=800x800 dpi=300
  name="Corporate logo" link="https://example.com"}
```

**Normalized image attributes** (inapplicable ones are omitted; units explicit):

| Attribute | Meaning | Typical origin |
|---|---|---|
| `width`, `height` | **Displayed** size (always required) | `wp:extent`, `svg:width/height` |
| `native-size` | Pixel dimensions of the original bitmap (`AxB`) | file header |
| `dpi` | Declared resolution, if other than 96 | bitmap metadata |
| `anchor` | `inline` (default) \| `floating` \| `behind` \| `front` | `wp:inline`/`wp:anchor`, `text:anchor-type` |
| `relative-to` | Horizontal position reference: `page` \| `margin` \| `paragraph` \| `character` \| `line` \| `column` | `wp:positionH @relativeFrom` |
| `relative-to-v` | Vertical reference, **only if it differs** from `relative-to` | `wp:positionV @relativeFrom` |
| `x`, `y` | Offsets from the reference (floating only) | `wp:posOffset`, `svg:x/y` |
| `align-h`, `align-v` | Symbolic alignment (`left/center/right`, `top/middle/bottom`) when the original uses alignment instead of offset | `wp:align` |
| `wrap` | `square` \| `tight` \| `through` \| `top-bottom` \| `none` | `wp:wrapSquare…`, `style:wrap` |
| `wrap-side` | `both` \| `left` \| `right` \| `largest` | `@wrapText` |
| `z-index` | Stacking order among floating objects | `@relativeHeight` |
| `rotation` | Degrees clockwise | `a:xfrm @rot` |
| `flip` | `h` \| `v` \| `hv` | `a:xfrm @flipH/V` |
| `crop` | Crop `"left,top,right,bottom"`, all four sides with `%` suffix | `a:srcRect`, `fo:clip` |
| `border` | Simple border `"1pt solid #000000"` (complex borders → raw) | `pic:spPr` |
| `name` | Internal object name in the document | `wp:docPr @name` |
| `title` | Title/caption | `wp:docPr @title` |
| `link` | Hyperlink on the image | `a:hlinkClick` |
| `external-src` | Original URL if the image was **linked**, not embedded | `r:link`, `xlink:href` |
| `render` | `unsupported` for formats a Markdown viewer cannot draw (WMF/EMF) | content sniffing |
| `effects-raw` | Id of the raw-block holding non-representable effects | `a:effectLst`, `a:scene3d` |

Rules:
- Alternative text (accessibility, `wp:docPr @descr` / `svg:desc`) goes in the standard Markdown
  alt field `![…]`, not in an attribute — so every viewer shows it.
- File name: `img-<hash8>.<ext>` (content hash → stable across conversions and with
  deduplication: N appearances of the same bitmap share a file, each with its own geometry
  attributes).
- `width`/`height` are **always** required in serialization even when they match native size:
  the round-trip must not depend on re-reading the bitmap.
- WMF/EMF are extracted with their original extension and full geometry; the serializer adds
  `render=unsupported` as a hint for viewers (warning in the report).
- Image effects with no representation (shadows, bevels, DrawingML 3D styles) are preserved as a
  raw-block associated via `effects-raw=<id>` referencing a contiguous `::: {.raw}`.
- Non-image objects (OLE, SmartArt, embedded charts): extracted to `assets/` and referenced as
  `![...](assets/obj-xxx.bin){.embedded-object content-type="..."}`, plus a warning in the
  report.

### 3.6 Sections, headers and footers, text boxes

```markdown
::: {.header scope=default}
Header text — page [n.]{.field field=PAGE}
:::

::: {.section columns=2 page-size=A4 orientation=landscape}
… section content …
:::

::: {.textbox x=5cm y=2cm width=6cm}
Text box content.
:::
```

Dynamic fields (page number, date, TOC) are represented as `{.field field=...}` spans with their
last known value as visible text.

## 4. Spreadsheets

Each sheet is a `#` section with a metadata container; data go in a GFM table.
**Golden rule: the cell shows the value; the formula and format travel in metadata.**

```markdown
---
docmark: "1.0"
source-format: xlsx
workbook:
  active-sheet: Sales
  defined-names:
    TOTAL_ANNUAL: "Sales!$D$10"
---

# Sales {.sheet cols="A:D" col-widths="12,9,9,11" frozen="A2"}

| | A | B | C | D |
|---|---|---|---|---|
| **1** | Product | T1 | T2 | Total |
| **2** | Widgets | 100 | 200 | 300 |
| **3** | Gadgets | 150 | 250 | 400 |

::: {.cell-meta}
- D2: formula="SUM(B2:C2)" num-fmt="#,##0"
- D3: formula="SUM(B3:C3)" num-fmt="#,##0"
- A1:D1: style=HeaderRow
- B2:D3: type=number num-fmt="#,##0"
:::
```

Rules:
- First column/row of the table = coordinates (generated, bold); they let the `cell-meta` block
  use readable A1 references.
- `cell-meta` accepts ranges (`B2:D3`) to compact repeated metadata; the parser expands them.
- `formula` is stored **without** a leading `=` and in the original dialect; `formula-dialect:
  openformula` is added when the source is ODS.
- Cell types: `number | text | bool | date | error` (+ `num-fmt` with the format code).
  Dates are shown in the table as ISO-8601 and restored as serial+format on write.
- Merged cells: `A5:C5: merge=true` (the value lives in the top-left cell).
- Cell styles (font, border, fill) are catalogued in front-matter `styles:` and referenced with
  `style=`.
- Huge sheets: by default the full used range is dumped; `--max-cells` allows truncating
  **only in unidirectional mode** (never when preparing a round-trip; truncating invalidates the
  return trip).

### 4.1 Images in spreadsheets

Sheets also carry images (logos, screenshots, diagrams) anchored to the grid. They are declared
in a `sheet-images` block at the end of each sheet, using the same syntax and image attributes
from §3.5 plus spreadsheet-specific anchor attributes:

```markdown
::: {.sheet-images}
![Company logo](assets/img-9c04b7.png){anchor=two-cell
  from="B2" from-offset="12px,3px" to="D8" to-offset="0,0" move-with-cells=true size-with-cells=false}

![Signature](assets/img-11ab42.png){anchor=one-cell from="F20" from-offset="0,0" width=180px height=60px}

![Watermark](assets/img-77cd01.png){anchor=absolute x=5cm y=8cm width=10cm height=10cm}
:::
```

Rules:
- `anchor` on sheets: `two-cell` (cell to cell; the image moves/stretches with the grid,
  according to `move-with-cells`/`size-with-cells`) | `one-cell` (origin cell + fixed size) |
  `absolute` (absolute position). They correspond to OOXML `xdr:twoCellAnchor`/`oneCellAnchor`/
  `absoluteAnchor` and to ODF cell/sheet anchors.
- In `two-cell`, `width`/`height` are **not serialized** (size is defined by the grid); in
  `one-cell` and `absolute` they are required, as in §3.5.
- `from`/`to` use A1 references; offsets within the cell go in `from-offset`/`to-offset`.
- Remaining properties (rotation, crop, alt, hyperlink, `native-size`…) work the same as in
  §3.5.
- Native sheet charts are not images: in v1 they are preserved as a raw-block with a warning
  (backlog: also export them as a courtesy image in unidirectional mode).

## 5. Configurable style mapping ("publication" mode)

Inspired by mammoth: an optional `style-map.yaml` file lets you project custom styles onto pure
Markdown elements (`MyTitle ⇒ h2`, `SourceCode ⇒ code-block`) for those who want clean output
without metadata. This mode is **unidirectional by definition** and the CLI marks it as such.

## 6. Fidelity levels (`--fidelity`)

| Mode | Content | Use |
|---|---|---|
| `full` (default) | Everything: attributes, catalogs, raw-blocks | Round-trip |
| `agent` | Text, structure, node ids, raw stubs; no catalogs and no measurements | An agent reading a document it means to edit |
| `standard` | Main attributes, no raw-blocks or full catalog | Readable rich Markdown |
| `plain` | Pure CommonMark+GFM, no attributes | LLM/RAG consumption MarkItDown-style |

### 6.1 `agent` is a projection, not a conversion

`agent` is not "a bit less than `standard`" — it is a different question. `standard` asks *what
survives if a human reads and edits this in a text editor*; `agent` asks *what a program needs in
order to change one node and leave the rest alone*. The rule that decides every case is:

> keep what a node **is** and what it **says**; drop how it **looks**.

So the style *name* stays (`{.Quote}`) and its indents do not; `.sup` stays because superscript
changes what the text says; a cell's formula and its merge stay because they are the sheet; the
column widths, the page geometry, the image EMUs, the style and list catalogues do not. A
raw-block becomes a stub with its id (§7) — an agent has to see that something is there, and the
bytes are the definition of what it cannot edit.

Two invariants make it a projection rather than a lossy level:

- **It addresses exactly what `full` addresses.** A node with no id could not be written back.
- **It says everything the document says.** Only appearance is dropped, and the level declares
  itself in the front matter with `fidelity: agent` so nothing writes it back as a whole
  document: the way back is node by node, with the etag proving nothing else moved.

## 7. Fidelity escape hatch: raw-blocks

Fragments with no DocMark representation (complex fields, SmartArt, DrawingML drawings, signed
content…) are preserved opaquely:

````markdown
::: {.raw format=ooxml part="word/document.xml" id=raw-0007}
```xml
<w:sdt>…original content…</w:sdt>
```
:::
````

Since 1.1 the payload may live in a **sidecar** instead, which is the default (`--raw
sidecar`); `--raw inline` keeps the form above:

```markdown
::: {#raw-0007 .raw format=ooxml part="word/document.xml" src="assets/_raw/raw-0007.xml"}
:::
```

- `src` is relative to the `.dmk.md` file. The name is derived from the id, sanitised: an id
  comes from the source package, and a package is not a trustworthy source of file names.
- A **missing sidecar is an error**, never a warning: the raw-block exists to hold what nothing
  else can hold, so a parser that shrugged it off would be a way to lose data quietly. Parsing a
  sidecar reference with no base directory is an error for the same reason.
- The inverse writer re-injects the fragment as-is if the destination matches `format`;
  otherwise it omits it with a warning.
- A human editor can delete a raw-block knowing they only lose that element.
- The `docsai convert` command reports how many raw-blocks it emitted (coverage metric: each
  phase aims to reduce them).

## 8. Escape rules and serializer determinism

- Only characters that would change meaning in CommonMark are escaped (`*_#|[]<>` depending on
  context), with a fixed decision table documented in the code.
- Attributes: canonical order (id, alphabetically sorted classes, sorted keys); values always
  double-quoted except simple numbers/identifiers.
- GFM tables: columns aligned with fixed padding if the table has < 120 columns; no padding if
  it exceeds that.
- These rules are **normative**: the idempotence test (`parse(serialize(ir)) == ir` and
  `serialize(parse(md)) == md`) verifies them in CI.

## 9. Pandoc compatibility

DocMark in `full` mode is parseable by `pandoc -f markdown` with these documented caveats:
`cell-meta` and `raw` blocks appear as generic divs, and non-standard attributes are kept as
div/span attributes. This is intentional: it gives free PDF/HTML/EPUB output via Pandoc without
docsai having to implement it.

---

## 10. v1.0 change log

Additions relative to the draft, decided during Phase 1 while implementing the serializer. All
are additions: no document written against the draft becomes invalid.

| Change | Reason |
|---|---|
| Exact unit rule (§2) with `emu` as escape hatch | The draft left the unit up to the serializer; rounding to `cm` shifted margins and sizes on every round-trip |
| `style-defaults` in the front matter | `w:docDefaults` is the base of the 4-level cascade and had no home |
| `indent-left/right/first-line/hanging` instead of a single `indent` | OOXML and ODF distinguish all four; a single one lost information |
| `[]{.empty}` for empty paragraphs | A blank line does not survive Markdown, and empty paragraphs are real content |
| `[]{.break kind=page\|column}` | The draft only covered line breaks |
| `.small-caps`, `.caps`, `underline=<style>`, `bold=false`, `italic=false` | Direct formatting that table §3.2 did not cover |
| `{.field field=X instr="…"}` | The draft described fields in prose but did not fix the syntax |
| `list=` inside the first item's attribute block | Avoids an item carrying two consecutive `{...}` |
| `header-row=false` on the table container | GFM requires a header row the original may not have had |
| Full `::: {.table complex=true}` format | Closes draft annex A |
| `relative-to-v`, `render=unsupported`, `effects-raw` on images | Axes with different references, non-drawable media, and effects without a model |

**Extensions not yet implemented** (the spec defines them; Phase 1 does not emit them): paragraph
`border`, `::: {.textbox}` (text boxes travel as raw-block), and everything in §4
(spreadsheets), which arrives in Phase 3.

---

## 11. Committed future versions (1.1 and 1.2)

Normative sketch of the two bumps scheduled by
[`development-plan-v2.md`](development-plan-v2.md). The exact syntax is finalised in the phase
that implements it (1.1 in Phases 10–11, 1.2 in Phase 14); what is fixed here is the intent and
the compatibility contract. Rationale:
[`technical-analysis-presentations.md`](technical-analysis-presentations.md) §6.

**Compatibility contract for both bumps**: additive only. A 1.0 document parses under 1.1/1.2
unchanged; ids are assigned on the next write. A 1.1/1.2 document read by a 1.0 parser loses
addressing metadata but stays valid CommonMark — principle §0.1 holds.

### 11.1 DocMark 1.1 — addressing, sidecar, `agent` fidelity

**Node ids and `next-id` are implemented** (plan v2 Phase 10) and normative as described below;
the remaining bullets stay a sketch until Phase 11 implements them.

#### Node ids (normative)

- An id is an **opaque token**, written with the existing attribute syntax: `{#n7}`. Ids handed
  out by docsai are `n` followed by the counter value; ids written by hand are preserved verbatim
  as long as they match the attribute token rules (ASCII alphanumerics, `-`, `_`, `.`).
  The positional forms used elsewhere in this document (`s4`, `s4.b2`) are **selectors**, not
  ids: an id that encodes a position cannot survive an insertion.
- Ids come from a **monotonic counter in the front matter** (`next-id: 128`), are **never
  renumbered on insertion and never reused after deletion**. A reader raises the counter above
  every id it finds, including hand-written ones, before allocating anything.
- A document that carries at least one id declares `docmark: "1.1"` and a `next-id`; one that
  carries none stays `docmark: "1.0"`, so the version always describes what was actually written.
- Ids are written **where the format has an attribute block to hold them**, and only there:

  | Node | Where the id lives |
  |---|---|
  | heading | its own attribute block — `## Q3 {#n4 .Heading2}` |
  | paragraph | its trailing attribute block, and **only when it is a container** (it holds a footnote, an image or a raw fragment); ordinary prose is reached by relative path |
  | list | `list-id=` in the first item's attribute block, beside `list=` (§3.3) |
  | table | the `::: {.table}` container, which an id forces even when nothing else needs it |
  | table row | the `::: {.row}` of a complex table (§3.4); rows of a GFM table have no attribute slot and are reached by relative path |
  | image | its attribute block — `![alt](p){#n6 width=…}` |
  | footnote | the **reference**, `[^1]{#n9}`, which is where the note sits in the flow |
  | sheet | the sheet heading's attribute block (§4) |
  | section | the `::: {.section}` container, so only in a multi-section document |
  | raw block | its existing `{#r7}` (§7), unchanged |

  A node that cannot be written keeps no id at all: an id that is dropped on every write would
  change on every round trip, which is worse than not having one.
- Runs and other inline nodes are addressed by relative path (`s4.b2:3`), never by id.
- **Policy**: `--ids assign|preserve|never`. `assign` fills the gaps, `preserve` writes back only
  what the document already had, `never` reproduces the 1.0 shape byte for byte. The default is
  `assign` at `--fidelity full` and `never` at the lossy levels, which stay readable; `plain` is
  CommonMark and never carries ids whatever is asked for.

#### Implemented in Phase 11

- **Raw-block sidecar** (§7): `src=` in the stub, payload in `assets/_raw/<id>.xml`, `--raw
  inline|sidecar` with `sidecar` as the default. Inline raw-blocks remain valid.
- **Fidelity level `agent`** (§6.1) and the `fidelity:` front-matter key that declares a
  projection. Ids are assigned at `agent` as they are at `full`, since a node with no address
  cannot be written back.

#### The rest of 1.1 (not yet implemented)

- **Etags**: optional 6-character content hash, `{#s4.b2 etag=a3f9c1}`, over the *normalised*
  node content, so formatting-only changes do not churn it. Used as an edit precondition. The
  hash is **derived from the node, never stored in it**, so it cannot go stale behind an edit.
- **Attribute-set dictionary**: repeated attribute patterns interned in the front matter and
  referenced as a class (`{.g1}`), with deterministic naming so §8 idempotence still holds.
- **Etags in the output**: `agent` addresses every node `full` addresses, but the etag is still
  computed and never written; the stub carries the id alone until `read --select` needs an
  if-match precondition.

### 11.2 DocMark 1.2 — the presentation profile (DocMark-P)

Third document class, alongside text documents (§3) and spreadsheets (§4). Design rule,
enforced by test: **`--fidelity standard` must remain hand-editable by a human**, and a plain
Markdown viewer must show, per slide, title + bullets + images and nothing else.

````markdown
---
docmark: "1.2"
source-format: pptx
next-id: 128
layouts:
  L1: { name: "Title and Content", master: M1 }
skeleton: assets/_skeleton/deck-9f3a21c8.pptx
---

## Q3 results {#s4 .slide layout=L1}

::: {.ph type=title idx=0 #s4.title}
Q3 results
:::

::: {.ph type=body idx=1 #s4.body}
- Revenue up 12 % {#s4.b1}
- Churn flat {#s4.b2}
:::

::: {.shape geom=rect #s4.sh3 raw=r7 pos="3in,2in" size="2in,1in"}
:::

::: {.notes #s4.notes}
Open with the churn number, it is the one they ask about.
:::
````

- `::: {.slide}` is the primary structural unit; `layout=` references the front-matter layout
  catalogue, and **only deltas against the resolved layout/master/theme are serialized**.
- `::: {.ph type=… idx=…}` for semantic placeholders; `::: {.shape …}` with explicit geometry
  for free shapes. A shape with no Markdown representation is a **visible stub** with a `raw=`
  reference — present so an agent knows not to delete it, cheap because the payload is in the
  sidecar.
- `::: {.notes}` for speaker notes.
- `skeleton:` references the preserved non-slide package parts (architecture §9.4). Absent for
  documents authored from scratch, in which case the writer uses the embedded default template
  and selects a layout from the content shape.
- Geometry is serialized in readable units (`pos`, `size` in in/cm/pt) and compared in
  round-trip with a documented tolerance; `emu` remains the exact escape hatch (§2).
