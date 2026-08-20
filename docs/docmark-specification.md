# DocMark Specification v1.0

**DocMark** is the extended Markdown profile used by `docsai` as its textual pivot format.
Goal: represent text documents (docx/odt/doc) and spreadsheets (xlsx/xls/ods) in a way that is
**readable, hand-editable, and rich enough to regenerate the original document with minimal
loss**.

Status: **frozen (v1.0)** at the close of Phase 0. Any later change bumps the version declared
in the front matter and requires documenting the migration (`AGENTS.md` §2).

Two version bumps are committed by
[`development-plan-v2.md`](development-plan-v2.md) and specified in §11 of this document:
**1.1** (node ids and etags, raw-block sidecar, `agent` fidelity level — plan v2 Phases 10–11)
and **1.2** (the presentation profile, "DocMark-P" — plan v2 Phase 14). Both are **normative**;
both are additive, so a document written against 1.0 stays valid.

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

attribute-sets:                 # repeated attribute patterns, interned (§3.7)
  g1: "color=#1F4E79 font=Consolas size=12pt"

list-definitions:               # numbering.xml / normalized ODF list styles
  L1:
    levels:
      - { format: decimal, text: "%1.", indent: 0.63cm }
      - { format: lowerLetter, text: "%2)", indent: 1.27cm }
---
```

Rules:
- Keys in kebab-case. Explicit units (`px`, `cm`, `pt`, `emu`, `%`); colors `#RRGGBB`.
- **Unit rule (normative)**: a length is written in the unit of **what it measures**, and only in
  a unit that represents it **exactly**.
  - *Layout and typography* — indents, spacing, margins, page size, column widths, list levels:
    `pt` → `cm` → `emu`. Points, because a text document is authored in them: Word stores layout
    in twips and a twip is exactly `0.05pt`, so two decimals name every value one can hold.
  - *Drawings and bitmaps* — image sizes, anchor offsets: `px` (at 96 dpi) → `cm` → `pt` → `emu`.
    Pixels, because a bitmap has a natural size in pixels.
  - **Zero names no unit**: `0`, not `0px`.
  - **Precision** is configurable (`--precision N`, default 2 decimals) and buys *readable units*,
    never rounding. A unit is used only when `N` decimals express the length exactly; otherwise
    the next unit is tried and `emu` is the final escape hatch, which always exists. So
    `1.251cm` is written `450360emu` at precision 2 and `1.251cm` at precision 3 — never rounded
    to `1.25cm`, which would move the length by 3600 EMU on every round-trip.

  The consequence, which is normative too: **the round-trip tolerance for a length is zero**. A
  serialised length re-parses to the identical EMU at every precision.
- The `styles` block is a **catalog**: the body references styles by name via classes
  (`{.Heading1}`); the inverse writer uses it to regenerate `styles.xml` / ODF `styles.xml`.
- Unknown fields are preserved (parsers must not reject them): forward-compatible.

### 2.1 Partial documents (selections)

A **selection** is a document made of some of another document's addressed nodes — what
`docsai read --select` writes. It is a normal DocMark document in every respect: it parses, it
re-writes to itself, and nothing in the body says it is partial. Two front-matter keys do:

```yaml
---
docmark: "1.1"
source-format: docx
next-id: 26                     # the *source* document's counter, not the selection's
partial: true                   # this is part of a larger document
etags:                          # content hash per addressed node, in id order
  n3: "8eb3e5"
  n4: "36576c"
---
```

Rules:
- `partial: true` means **writing this document back whole deletes everything it does not
  contain**. A writer must say so; `docsai` raises a `partial-document` warning on every
  serialisation of one. It is the only warning a well-formed selection produces.
- `next-id` is the **source document's** counter, deliberately ahead of the ids present. That is
  what keeps a node inserted into a selection from colliding with a node left behind.
- `etags` is **derived, never stored**: a writer recomputes the map from the nodes it is writing,
  so an edited node's etag changes with it. It is written only for a partial document — a whole
  one carries its content already, and a hash of it would be a second copy to keep in step.
- The front matter of a selection is the **minimum**: no metadata, no page geometry, no style or
  list catalogue, no `attribute-sets`. Those describe the document the selection came from, which
  remains the authority; a caller that declined to read the document did not ask for them either.
  A class the selection does not define is inert (§3.7), so the body still parses.
- A selection therefore carries **no attribute-set dictionary**: every attribute is written where
  it is used, so the file depends on nothing outside itself.
- A footnote is not selectable on its own: it is addressed at its reference, inside a block, and
  its text is written at the foot of the document. A selection containing a reference **must**
  carry the matching definition.

## 3. Text blocks

### 3.1 Headings and paragraphs

```markdown
# Chapter title {.Heading1}

A normal paragraph (default style, no attributes).

Paragraph with style and direct formatting. {.Quote align=center space-after=12pt}
```

- The `#` level reflects the outline level; the class indicates the actual style — and is
  **omitted when the level already names it**: when exactly one paragraph style in the catalogue
  declares that outline level, a heading of that level and that style writes no class, and the
  parser puts the style back. Two styles claiming the same level make it ambiguous, and an
  ambiguous chain implies nothing: the class stays.
- Paragraph attributes go in `{...}` **at the end of the block**: `align`,
  `indent-left`, `indent-right`, `indent-first-line`, `indent-hanging`,
  `space-before`, `space-after`, `line-height`, `background`, `keep-with-next`,
  `page-break-before`, `outline-level`.
- An **empty** paragraph is real content (documents are spaced that way) and a blank line cannot
  represent it, because Markdown absorbs it: in `full` mode it is written `[]{.empty}`.
  `standard` and `plain` modes discard it, which is exactly what they promise (§6).
- Economy rule: if formatting matches exactly what the style defines, redundant attributes are
  **not** emitted (keeps Markdown clean and diffs stable). "The style" means the whole cascade,
  in this order:
  1. document defaults (`style-defaults`),
  2. the **paragraph's** style chain — a run inherits from its paragraph before it inherits from
     the document, so a run repeating what its paragraph's style says carries no span at all,
  3. the run's own character style chain,
  4. direct formatting, which is the only thing left to write.

  The same rule applies to the catalogue itself: a style writes only what its `based-on` chain
  does not already say. And the **default paragraph style is never named** on a paragraph — it
  is what applies where nothing else does.

  Every omission here is reversible by construction: a value equal to the inherited one resolves
  to the inherited one. Nothing is dropped that re-reading would not give back.

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

### 3.7 The attribute-set dictionary

The economy rule of §3.1 removes what a style implies. What it cannot remove is formatting no
style implies — an author who colours each term and indents each note by hand — and that
repetition is paid for once per node. A pattern of key/value pairs that repeats is therefore
**named once in the front matter and referenced by class**:

```yaml
attribute-sets:
  g1: "color=#1F4E79 font=Consolas size=12pt"
  g2: "indent-left=1.27cm space-after=6pt space-before=6pt"
```

```markdown
[convert]{.g1} transforms a document into the pivot format.

A note about convert, with no style and a manual indent. {.g2}
```

Normative rules:

- The value of an entry is an **attribute-block payload**: exactly what would have been written
  inside the `{…}`, so a reader parses it with the same parser it uses for the body.
- A reader **expands every dictionary class into its pairs before interpreting the block**. A
  pair written on the node itself wins over the entry's: the dictionary is a default, never an
  override. After expansion no consumer can tell whether a document used a dictionary, which is
  what makes it a compression and not a feature of the format's meaning.
- Only key/value pairs are interned. The id and the other classes stay on the node — they are
  what it *is*, not how it is written. A raw-block (§7) is never interned: its `src=` is scanned
  out of the serialised text.
- A pattern earns an entry when it appears **at least 3 times** and is **at least 12 characters**
  long. Below either threshold the entry plus the references cost more than the repetition, so
  interning would inflate the document it exists to shrink.
- Names are `g1`, `g2`, … assigned in **order of first appearance**, skipping any name the
  document already uses — a style id, a list name, or one of the structural classes (`.section`,
  `.table`, `.raw`, `.underline`, …). A name that collided would change what a node *is*.
- The dictionary is a function of the document, so it is part of serializer determinism (§8):
  serialising a document twice, or re-serialising after a round trip, yields the same names in
  the same order.
- Levels: `full` and `standard`. `agent` (§6.1) has already dropped the formatting a dictionary
  would compress, and is meant to be read directly; `plain` has no attributes at all.

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
  double-quoted except simple numbers/identifiers. A dictionary class (§3.7) sorts with the
  others, so interning a pattern never depends on where the writer put the class.
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

The two bumps scheduled by [`development-plan-v2.md`](development-plan-v2.md), each finalised by
the phase that implements it (1.1 in Phases 10–11, 1.2 in Phase 14). Both sections are normative
now; anything a phase has not implemented yet says so where it is described. Rationale:
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
- **Delta emission against the whole cascade** (§3.1): nothing is written that the paragraph's
  style, the run's style or a style's `based-on` chain already implies, and a heading omits the
  class its level names.
- **Attribute-set dictionary** (§3.7): `attribute-sets:` in the front matter, `{.g1}` in the
  body, deterministic names, at `full` and `standard`.
- **Readable units** (§2): a length is written in the unit of what it measures, zero carries no
  unit, and `--precision` sets how many decimals a readable unit may use. Still exact: the
  round-trip tolerance is zero.
- **Partial documents** (§2.1): `partial: true` and the `etags:` map, written by
  `docsai read --select`. This is where the etag finally reaches a file — derived on every write,
  never stored, and only for a selection.

#### The rest of 1.1 (not yet implemented)

- **Etags on a whole document**: a complete document still writes none. It carries its content,
  so a hash of it would be a second copy to keep in step; the map exists for a *selection*, which
  is what an if-match write-back is sent against.

### 11.2 DocMark 1.2 — the presentation profile (DocMark-P)

Third document class, alongside text documents (§3) and spreadsheets (§4). **Normative**, as of
plan v2 Phase 14; what an increment has not implemented yet is marked as such in-place. The design
is the one spike P2 measured — [`spikes/P2-docmark-p.md`](spikes/P2-docmark-p.md) §4 — and it
**supersedes the earlier sketch of this section on two points**: placeholders are implicit, and
notes degrade to a blockquote. The sketch wrote every placeholder as a container, which said each
slide's title twice and left a plain viewer printing it twice.

Design rule, enforced by test: **`--fidelity standard` must remain hand-editable by a human**, and
a plain Markdown viewer must show, per slide, title + bullets + images and nothing else. Both
halves are checked by CI over `corpus/pptx`
(`docsai-convert/tests/plain_residue.rs`), rendering with a viewer that has no container and no
attribute extension:

- At **`plain`** the rule is absolute: **zero** residue, over every deck in the corpus. A plain
  deck is headings, lists, images and GFM tables, and there is nothing else to see.
- At **`standard`** what may leak is named, not just bounded: the `{.slide}` marker of rule 1 and
  the containers of rules 4 and 8, and nothing besides. That is 17 % of what a viewer prints over
  the whole corpus and 15 % over the decks that are documents rather than diagrams — worse than
  the 11.4 % / 2.6 % spike P2 measured, by exactly one construct: P2's hand-written samples had no
  `{.slide}`, which costs eight characters per slide and is the *entire* residue of ten of the
  seventeen decks. These fixtures are two slides long, so those eight characters weigh what they
  never would in a real deck: `forty-slides`, the only one of document size, sits at 13 %.

````markdown
---
docmark: "1.2"
source-format: pptx
next-id: 128
layouts:
  L1: { name: "Title and Content", master: M1, title: 0, body: 1 }
skeleton: assets/_skeleton/deck-9f3a21c8.pptx
---

## Q3 results {#n1 .slide layout=L1}

- Revenue up 12 %
- Churn flat

::: {#n4 .ph idx=2 pos="88px,5200000emu" size="784px,1000000emu"}
1. One
2. Two
:::

::: {#n5 .shape geom=rightArrow name="Arrow 1" pos="3200000emu,2200000emu" size="1400000emu,500000emu" raw=r7}
:::

::: {#n6 .notes}
Open with the churn number, it is the one they ask about.
:::
````

#### The eight rules

1. **A slide is an `##` heading carrying `.slide`, and the heading is the title placeholder.** No
   container repeats it. A slide whose layout has no title placeholder, or whose title is empty,
   writes an empty heading — `## {#n7 .slide layout=L3}` — which is ugly and bounded; the
   alternative is a container on every slide.
2. **The layout's primary body placeholder is implicit**: its blocks are written at slide level,
   as ordinary Markdown.
3. **`layouts:` declares, per layout, `name`, `master`, and which placeholder index is the `title`
   and which is the `body`.** This is what makes rules 1 and 2 a catalogue lookup rather than a
   guess, and it is why the IR carries those indices (`LayoutCatalog`, architecture §9.1).
4. **Every other placeholder is `::: {.ph idx=…}`; free shapes are `::: {.shape geom=…}` and
   connectors `::: {.connector geom=…}`**, each with its geometry and, when the original XML was
   preserved, a `raw=` reference to the raw-block (§7) that holds it.
5. **Speaker notes are `::: {.notes}` at `full` and `agent`, and a blockquote at `standard`.** A
   blockquote is native CommonMark and cannot collide with slide content: PresentationML has no
   blockquote construct for a placeholder to occupy.
6. **`standard` writes no ids, no geometry, no raw payload, no image size, no `[]{.empty}` and no
   layout catalogue** — nothing in the body refers to one. `standard` does not write back, and
   that is what it buys.
7. **Geometry is written in readable units (§2) with `emu` as the exact fallback, at `full` and
   `agent` only.** `pos` and `size` are `"x,y"` and `"w,h"` pairs. PowerPoint stores positions an
   author *dragged*, not typography an author chose, so a deck shows more raw `emu` than a docx
   ever does.
8. **A shape with no Markdown representation is a visible stub at every level, `standard`
   included.** Charts, SmartArt, OLE and custom geometries are stubs with a `raw=` reference —
   present so a human or an agent knows not to delete what they cannot see, cheap because the
   payload lives in the sidecar.

#### Slide attributes

The heading of rule 1 carries everything the slide itself knows. Nothing else is written per slide.

| Attribute | Meaning | Levels |
|---|---|---|
| `#id` | The slide's address (§11.1). The two implicit shapes take none: there is nowhere to write it | `full`, `agent` |
| `.slide` | What the heading is. Its presence is what makes an `##` a slide rather than a section heading | every level but `plain` |
| `layout=` | The layout in `layouts:`, against which the implicit title and body resolve. It follows its catalogue: rule 6 writes no `layouts:` at `standard`, and a reference to an absent catalogue would be residue in a viewer and a dangling name in a parser | `full`, `agent` |
| `section=` | `p14:sectionLst`, written on **every** slide of the section, not once at its start: a slide read on its own must still know where it belongs | every level but `plain` |
| `hidden=true` | `p:sld@show="0"`. Present only when true | every level but `plain` |
| `name=` | `p:cSld@name`, what the writer puts back. The levels that do not write back drop it | `full`, `agent` |

A title placeholder holding more than one paragraph is joined into the one heading line, with a
warning: the text survives, the paragraph break does not.

#### Shape container attributes

Rule 4's containers — `.ph`, `.shape`, `.connector` — carry what the shape knows. A container's
body is the shape's content: blocks for a placeholder or a text box, the shape's own label for a
preset shape. An empty shape is written as an empty container, because a box with nothing in it is
still a box the author placed.

| Attribute | Meaning | Levels |
|---|---|---|
| `#id` | The shape's address (§11.1) | `full`, `agent` |
| `.ph` / `.shape` / `.connector` | What the container is: a placeholder the layout does not make implicit, a free shape or a text box, a connector | every level but `plain` |
| `idx=` | `p:ph@idx`, which is what matches the shape to its layout placeholder. Only the levels that write back need it | `full`, `agent` |
| `type=` | `p:ph@type`, when it is not `body` — the PresentationML default. A footer and a chart slot are not the same box, and that is what a *reader* needs | every level but `plain` |
| `geom=` | `a:prstGeom@prst`, when it is not `rect` — the DrawingML default, and unwritten for the same reason `type=body` is. Identity, not measurement: it is the only thing that says a box is an arrow, so it survives where `pos=` does not | every level but `plain` |
| `name=` | `p:cNvPr@name`, what the selection pane shows | `full`, `agent` |
| `pos=` / `size=` | `"x,y"` and `"w,h"` in the units of rule 7 | `full`, `agent` |
| `rotation=` / `flip=` | `a:xfrm@rot` in degrees, and `h` / `v` / `hv` | `full`, `agent` |
| `raw=` | The raw-block (§7) holding the original markup | `full`, `agent` |

Two consequences of rule 6 are worth stating, because both are losses and both are warned. At
`standard` a stub keeps its container and loses the `raw=` reference: the reader still knows the
shape is there, and the bytes are not in the document. And **slide furniture** — the slide-number,
date, footer and header placeholders — is written only at `full` and `agent`: it is inherited from
the layout, carries nothing the author wrote, and costs a container on every slide.

At `plain` there are no containers at all: a `:::` fence is literal text to a CommonMark viewer.
What a shape *says* still reaches the reader, in slide order; the box does not.

#### Pictures, tables, groups and stubs

Rule 4 is about shapes Markdown has no form for. Three kinds are not in that position, and are
written as what Markdown already has:

| Shape | Form | Notes |
|---|---|---|
| A picture | An image line, `![alt](path){…}` | No container. The shape's `#id`, `name=`, `pos=`, `rotation=` and `flip=` ride on the image's own attribute block; the **size** is the image's `width=`/`height=`, never written twice |
| A table | The GFM table of §3.4 | Its container, when it has one, carries the shape's `#id` and placement. The table takes no id of its own: a table shape is one addressable node, not two |
| A group | `::: {.group}` holding its shapes | Every shape inside a group is addressable in its own right, so the group is a box around them and never a substitute for them. At `plain` the children are written in order and the grouping is what is lost |

An image on a slide carries **no measurements at `standard`** (rule 6): a plain viewer draws the
picture at its own size regardless, so `width=`/`height=` would be residue. The same holds for a
table's `col-widths=`, and for the same reason — with it gone a slide table at `standard` needs no
container at all and is bare GFM. The same image and the same table in a *text* document keep their
measurements at that level: §3.5's round-trip rule is unchanged, and this is a property of the
document class, not of the fidelity level.

Everything else is a rule-8 stub, and the class names what it is: `.chart`, `.smartart`, `.ole`,
`.media`, `.object` for what the reader could not classify, and `.shape` for a custom geometry
whose path list has no preset name. A stub's body is the text the object shows, when it shows any,
so it survives even at `plain`.

| Attribute | Meaning | Levels |
|---|---|---|
| `kind=` | What kind of chart — `barChart`, `lineChart` — as the chart part names it | every level but `plain` |
| `data=` | The embedded workbook holding the series, as an asset reference. Not a raw payload: the file sits beside the document | every level but `plain` |

#### Speaker notes

Rule 5 in full. The notes of a slide are written after its shapes, and they are the one node whose
*syntax* depends on the level, so a parser has to read both forms.

| Level | Form |
|---|---|
| `full`, `agent` | `::: {.notes}` … `:::`. The container carries no id: what is addressable inside it are the blocks it holds, exactly as on a slide |
| `standard` | A CommonMark blockquote, one `> ` per line and a bare `>` for the blank lines inside it |
| `plain` | Nothing, with a warning. A notes page is not what the slide shows |

A slide with **no notes page** and a slide with an **empty** one are different documents: the first
writes nothing, the second writes an empty `::: {.notes}` container at the levels that write back.
At `standard` an empty notes page writes nothing at all — a lone `>` is noise in a document that
never goes back into a package.

#### Front matter keys added by 1.2

| Key | Meaning |
|---|---|
| `layouts:` | The layout catalogue of rule 3. Written at `full` and `agent`; absent at `standard` and `plain` |
| `skeleton:` | The preserved non-slide package parts (architecture §9.4), as an asset reference. Absent for a document authored from scratch, in which case the writer uses the embedded default template and selects a layout from the content shape |

#### Reading a deck back

The parser is the mirror of the rules above, and three of its answers are part of the format rather
than of an implementation, because a second reader has to give the same ones.

- **What makes a file a deck** is `source-format: pptx` in the front matter, a `layouts:` or
  `skeleton:` key, or a `##` heading carrying `.slide`. Nothing else: `##` alone is a heading, and
  guessing a deck from heading levels would turn every report into a presentation. A `plain` deck
  writes no front matter and no marker, so it reads back as a text document — that is what a
  one-way projection is.
- **Input is tolerant** (analysis §6.6). A deck a human typed carries no attributes at all: a `#`
  or `##` heading opens a slide with or without `.slide`, its blocks are the implicit body of
  rule 2, content before the first heading is a slide with no title, and a container class the
  reader does not know keeps its text as a shape and is warned rather than refused. A heading
  inside a `:::` container or a code fence is content, not a new slide.
- **`.shape` is disambiguated by `raw=`**, not by `geom=`: a stub (rule 8) is a marker over markup
  only the original package can reproduce, and a text box is content. A text box with a rounded
  outline carries a preset too, and reading it as a stub would freeze editable text into an opaque
  object. At `standard`, where `raw=` is not written, a stub therefore reads back as a text box —
  the level does not write back, so it spends nothing it was going to use.

- **A slide has one notes page.** More than one notes block under the same slide — two blockquotes,
  or a `::: {.notes}` and a blockquote — is one page with their blocks in order, never the last one
  replacing the others. Writing a second blockquote is what «add a note» looks like at `standard`,
  and dropping the first would lose text that is on the screen.

Two more things a reader will notice. `##` is written both for a slide whose title placeholder is
empty and for a slide that has none, so it reads back as the second: the two write the same line,
and the empty box is restored from the skeleton. And `p:sldSz` has no front-matter key — the canvas
lives in the preserved package, and a deck read from DocMark alone does not invent one.

#### Version rule and compatibility

A presentation declares `docmark: "1.2"` whether or not it carries ids: the version describes the
*profile* the body is written in, and a deck's body uses `.slide` even at `standard`. The bump
stays additive in the direction that matters — a 1.0 or 1.1 document parses unchanged under a 1.2
reader, and no text document or workbook changes shape because this profile exists.

A 1.2 document read by a 1.0 parser loses the profile's attributes and keeps its headings, lists,
tables and images: principle §0.1 holds, which is the whole point of the implicit form.

#### Not yet implemented

Charts and SmartArt have no representation of their own: both are rule-8 stubs, and turning a
chart into data (series and categories as a table) is plan v2 Phase 16. `data=` is written when the
IR names the workbook; the pptx reader of Phase 13 keeps it inside the skeleton instead, so today
the attribute is the exception rather than the rule. The `.odp` reader
(Phase 18) and the pptx writer (Phase 15) consume this profile but do not change it.
