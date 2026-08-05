# Technical analysis — presentations and the agent-native profile

Companion to [`technical-analysis.md`](technical-analysis.md). That document analysed flow
documents (`.docx`/`.odt`/`.doc`) and grids (`.xlsx`/`.xls`/`.ods`) and produced the decisions
that shaped Phases 0–7. This one covers the two problems that drive **development plan v2**
([`development-plan-v2.md`](development-plan-v2.md)):

1. **Presentations** — `.pptx`, `.ppt`, `.odp`: a third document class that the current IR and
   the current DocMark profile cannot express.
2. **Context economics** — the cost, in tokens and in round trips, of an AI agent doing real
   work on a document through `docsai`. Phases 0–7 optimised *fidelity*. From v2 on, the second
   optimisation axis is the **agent loop**.

Numbering continues the conventions of `technical-analysis.md`; risk ids continue the `R*`
series (R1, R3 already used by the spikes in `docs/spikes/`), and presentation-specific risks
use the `P*` series.

---

## 1. The structural problem: a presentation is not a flow document

This is the difference that conditions everything else. `.docx`/`.odt` are **block flows**;
`.xlsx`/`.ods` are a **grid**. A `.pptx` is a **canvas of absolutely positioned shapes**
(`p:spTree`, each shape carrying `a:xfrm` in EMU), with:

- **No semantic reading order.** `spTree` order is z-order, not narrative order.
- **A four-level inheritance cascade different from Word's**: shape → layout placeholder →
  master → theme (`clrMap`, `schemeClr`, `+mj-lt`/`+mn-lt`), plus `defaultTextStyle` in
  `presentation.xml`. Word's cascade (docDefaults → style → numbering → direct) resolves to
  *properties*; PowerPoint's resolves to *a placeholder identity plus properties*.
- **A package-level template** (masters, layouts, theme, `tableStyles.xml`) that is shared
  state across slides and is impossible to reconstruct faithfully from Markdown.

Consequences for docsai:

- The IR needs a **third root**, `Document::Presentation` (architecture §3), not a
  reinterpretation of `TextDocument`.
- DocMark needs a **presentation profile** (**DocMark-P**, spec addendum) in which the fenced
  container `::: {...}` is the *primary structural unit* (slide → placeholder → shape), not an
  exception as it is in the text profile.
- Round-trip fidelity depends on a mechanism the current codebase does not have: **preserving
  the non-slide parts of the original package opaquely** (§5.1).

This is more specification work than parsing work, and it is why plan v2 puts a spike phase in
front of the reader.

## 2. Anatomy of `.pptx` and where the difficulty actually is

| Element | Where | Round-trip difficulty |
|---|---|---|
| Slides and their order | `ppt/slides/slideN.xml`; real order in `p:sldIdLst` (not in the file name) | Low, but `p:sldIdLst` and sections (`p14:sectionLst` inside `extLst`) must be honoured |
| Placeholders | `p:ph` (`type`, `idx`) + layout + master | **High**: without resolving the cascade you cannot tell what a title is; without preserving the reference, the reverse write destroys the design |
| Text | `a:bodyPr`, `a:lstStyle`, `a:p`/`a:pPr` (`lvl`, `buChar`/`buAutoNum`), `a:r`/`a:rPr`, `a:fld` | Medium. Bullet levels map cleanly to Markdown lists; `a:fld` (slide number, date) behave like `w:fldSimple`, already modelled as `Inline::Field` |
| Autofit | `a:normAutofit@fontScale`, `lnSpcReduction` | **High and treacherous**: if an agent adds a line, `fontScale` is stale and PowerPoint does not recompute it until the box is edited |
| Tables | `p:graphicFrame` → `a:tbl` + `tableStyles.xml` | Medium; a different XML model from `w:tbl`, but the IR `Table` covers it |
| Charts | `p:graphicFrame` → `c:chart` + an **embedded `.xlsx` workbook** in `ppt/embeddings/` | Medium-high, with an advantage: the workbook is a real xlsx and we already own a reader for it |
| SmartArt | `dgm:*` (data/layout/style/colors) + `mc:AlternateContent` fallback drawing | **Very high** → clear raw-block candidate |
| Transitions / animations | `p:transition`, `p:timing` | Huge subtree, no possible Markdown representation → raw-block in full |
| Speaker notes | `ppt/notesSlides/*` | Low, and **the highest-value content for an AI agent** |
| OLE, media, macros | `p:oleObj`, `ppt/media/*`, `.pptm` | Preserve opaquely + security warning (macros are never executed; `.pptm` read as its macro-free equivalent, as with `.docm`) |

Geometric shapes (`prstGeom`/`custGeom`), connectors (`p:cxnSp`) and groups (`p:grpSp`) have no
Markdown equivalent at all. Decision taken here rather than deferred: **raw-block with a visible
stub** (`::: {.shape geom=rect id=s4.sh3}`) so an agent knows the object exists, knows it must
not hand-edit it, and does not delete it by accident.

## 3. Rust library evaluation

Same criteria as `technical-analysis.md` §4: maintained, pure Rust, no panics on corrupt input,
preserves unknown elements, reasonable binary size.

| Crate | Possible role | State | Verdict |
|---|---|---|---|
| `ooxmlsdk` (KaiserY) | Typed access to pml + dml, read and write | Active, v0.12; `PresentationDocument` with typed accessors, `mce` feature for Markup Compatibility, optional `validators`, MSRV 1.88 | **Worth a real spike.** MSRV matches our `rmcp` floor. Against: very large generated code (raised `recursion_limit`), no serde, and no particle model — XML children arrive as a flat vector of enums, which is worse ergonomics than our current readers and a binary-size/compile-time risk |
| `pptx` (hidemi-ito) | Pure-Rust read + write | v0.1.0, very young | Only 4 dependencies (`zip`, `quick-xml`, `thiserror`, `sha1`), `forbid(unsafe_code)`, python-pptx-inspired API, covers animations, 3D effects, SmartArt, freeform and groups. Risk profile identical to the one that made spike R1 discard `docx-rs`: unknown-element preservation, geometry loss and panics on corrupt corpus all unmeasured |
| `ppt-rs` | Writing | Active | Aimed at **generation**, not read fidelity; ships its own `rmcp` MCP server, which makes it a partial competitor rather than a dependency. Useful as a reference for the minimum parts PowerPoint requires to not offer "repair" |
| `pptx-to-md` | — | Active | Converts PPTX **and ODP** to Markdown with metadata, slide markers, spatial reading order and images. Unidirectional: it is MarkItDown for decks. **Do not adopt; do study** its spatial reading-order heuristic and use it as the baseline for `--fidelity plain` |
| `msoffice_pptx` | — | WIP since 2019, low-level deserializer generated from the ECMA-376 XSDs | Abandoned. Type reference only |
| `office_oxide` | Text from legacy `.ppt` | Active | Covers DOCX/XLSX/PPTX + DOC/XLS/PPT, MIT/Apache, extracts text from `.doc`/`.xls`/`.ppt` with no JVM and no external binaries. Candidate for the **native degraded route** for `.ppt` |
| `calamine` | Embedded chart workbooks | Already in the tree | Reusable as-is |

**Decision (consistent with R1 and R3): custom reader and writer in `docsai-office::pptx` over
`zip` + `quick-xml`.** What Phases 0–7 already built transfers almost entirely: the OPC layer,
`_rels` resolution, `AssetStore` with content-hash dedupe, `Length`/EMU newtypes, `ImageRef` +
`ImageGeometry` (the DrawingML model in `.pptx` is the same one already implemented for
`.docx`/`.xlsx`) and the raw-block hatch. No crate gives all three of the things we need at
once: unknown-element preservation, placeholder cascade resolved as *reference + delta*, and
no-panic behaviour on corrupt input.

`ooxmlsdk` still earns a one-week spike with objective metrics (fidelity over the corpus, binary
size, compile time) — if it wins, it removes months of pml/dml typing. That is spike **P1** in
plan v2 Phase 12.

**Spike P1 ran, and the decision above stands** — see
[`spikes/P1-pptx-strategy.md`](spikes/P1-pptx-strategy.md). Two of this table's assumptions about
`ooxmlsdk` were wrong in its favour: it reads every trait the corpus isolates, and it panics on
zero of 1396 corrupt inputs. It was refused on the other two: a foreign-namespace element is
dropped with no error and no warning, which makes the raw-block contract unimplementable, and
linking it costs **+28.7 MB** stripped and **+311 s** per clean release build.

## 4. Legacy `.ppt` and `.odp`

**`.ppt`** — the `.doc` decision (analysis §1.3) applies, with a better prognosis. No Rust crate
reads it with styles and images; the format is OLE2/CFB with records ([MS-PPT] +
[MS-ODRAW]/Escher). Route A: LibreOffice headless → `.pptx` → the normal pipeline. Route B
(degraded native): `cfb` + `TextCharsAtom`/`TextBytesAtom` records for text and the `Pictures`
stream for BLIPs — notably **simpler than the `.doc` piece table**, so the native route is worth
more per hour here than it was for `.doc`. `office_oxide` may cover it without writing the
parser. Writing `.ppt` is out of scope, exactly like `.doc`/`.xls`.

**`.odp`** — not in the original request, but leaving it out breaks the project's symmetry
(`.odt`/`.ods` exist as the free counterparts). No mature crate exists — the same gap we had for
`.odt`, which `docsai-odf` filled with a custom parser. Parser over `draw:page`, `draw:frame`,
`presentation:class` and master pages; the effort is smaller than `.odt` was, because Impress is
more regular than Writer.

## 5. What no library gives you (the actual work)

### 5.1 Preserve the template package

Reconstructing masters, layouts and theme from Markdown is a lost battle, and an unnecessary
one. Decision: in `--fidelity full` and `--fidelity agent`, the reader stores the **non-slide
parts of the original package opaquely** (a skeleton blob under `assets/_skeleton/`, referenced
from the front matter by content hash), and the writer **re-injects the slides into that
skeleton** instead of regenerating it. This is the raw-block idea lifted to package level. It
solves SmartArt, animations, OLE, theme and `tableStyles` in one move, and it is the difference
between "PowerPoint opens the file" and "PowerPoint offers to repair it". For generation from
scratch, an embedded default template.

**Spike P3 built it and it holds at the package boundary** — see
[`spikes/P3-preserved-skeleton.md`](spikes/P3-preserved-skeleton.md). Over 48 packages, including
twelve written by LibreOffice rather than by our own generator, no part was lost, added or
reordered, and every one still opened. Two corrections to the paragraph above. First, the rule is
not «preserve the non-slide parts»: it is **preserve everything and treat rebuilding as the
exception**, with slide parts found through `[Content_Types].xml` and never by file name. Second,
the skeleton is necessary and not sufficient — the spike's own writer silently dropped every run
property *inside* the slides it rebuilt, so a writer may rebuild an element only when it can model
or carry every child of it. Cost measured: holding a package in memory runs at 2.7× its size, so
parts that are never inspected have to be streamed.

### 5.2 DocMark-P

Slide as a `:::` container, semantic placeholders (`::: {.ph type=title idx=1}`) versus free
shapes with explicit geometry, notes as their own container, a layout catalogue in the front
matter (parallel to the existing style catalogue), and a degradation rule: **a plain Markdown
viewer must show title + bullets + images per slide and nothing else**.

**Spike P2 ran and the degradation rule holds** — see
[`spikes/P2-docmark-p.md`](spikes/P2-docmark-p.md). One correction to the paragraph above: a
placeholder is *not* a container. The slide heading is the title and the primary body is written
as plain blocks under it, which costs 20 % fewer tokens than the container form and leaves 2.6 %
of a plain viewer's output as syntax, against 59.6 % for the containers. Free shapes are the one
construct that stays noisy at every level, and stay so on purpose.

### 5.3 Deterministic, reversible reading order

`spTree` gives z-order. We need a policy (placeholders first by type, then remaining shapes by
top-left position) **and** preservation of the original index as an attribute — otherwise the
round trip permutes shapes. This is exactly the problem `pptx-to-md` solves heuristically
without needing reversibility; we do need it.

### 5.4 Autofit and overflow

When an agent adds three bullets, text overflows the box. Solving it properly requires text
measurement (`ttf-parser` + `rustybuzz`/`cosmic-text` plus font metrics that may not be
installed). v1 decision: **drop the stale `fontScale` and emit a typed warning**
(`Warning::AutofitStale`); real measurement goes to the post-2.0 backlog.

### 5.5 Validation

The current suite (goldens + structural round-trip) does not catch the most expensive failure:
PowerPoint asking to repair the file. Plan v2 adds two CI gates: **schema validation** (the
ECMA-376 XSDs, or the `ooxmlsdk` `validators` feature) and **headless LibreOffice rendering to
PNG with perceptual diff** over the corpus. The corpus is generated with `python-pptx`, which
fits `corpus/generate.py` as it exists today.

---

## 6. Context economics: what an agent actually pays

Phases 0–7 produce excellent documents for a *human* diff and expensive documents for an *agent
loop*. The measurements that matter are not bytes, they are **tokens per task** and **tool calls
per edit**.

Today, changing one slide title costs: read whole deck (~20k tokens) → regenerate whole deck
(~20k tokens) → convert. Three calls, 40k tokens, and every regeneration risks collateral
damage. With the primitives below it costs ~350 tokens and one call.

### 6.1 Stable addressing — the primitive that enables everything else

Every addressable node (slide, heading, placeholder, image, table, row, note, raw block) carries
a short **persistent** id, emitted with the existing attribute syntax: `{#s4.b2}`.

- **Monotonic, never renumbered.** Counter in the front matter (`next-id: 128`). Inserting a
  bullet at the top does not move existing ids. This is the Notion/Google-Docs block model, and
  it is what lets an agent chain edits without re-reading.
- **Not on everything.** An id per paragraph in a 400-paragraph docx is expensive noise. Rule:
  emit ids on addressable containers; everything else is addressed by relative path
  (`s4.b2:3` = third run).
- **Per-node etag**: 6-char hash of normalised content, enabling optimistic concurrency — an
  agent edits `#s4.b2@a3f9c1` and fails loudly if the node changed instead of silently
  overwriting.

Cost: ~4 tokens per addressable node, amortised on the first edit.

### 6.2 Projections instead of dumps

- **`outline`** — the map: id tree with type, first ~60 characters, and **estimated token cost
  per node**. A 40-slide deck fits in ~400 tokens. The cost annotation is what lets an agent
  *plan* ("read only slides 12–15, 1.8k tokens") instead of discovering the cost after blowing
  the context window.
- **`read --select`** — by selector: `s4,s7-s9`, `#s4.notes`, `type:notes`, `text:invoicing`.
  Returns **valid, self-contained DocMark** (minimum necessary front matter), never a broken
  fragment.
- **`search`** — light grep over the IR returning ids + context, not the document. Kills the
  "read everything to find where X is" pattern.

### 6.3 Patch editing

Highest return of the whole list. An `apply_edits` tool with operations over the IR — not
textual diffs, which reintroduce serializer ambiguity:

```
{op: "replace_text",    target: "s4.title", value: "Q3 results"}
{op: "set_props",       target: "s4.b2",    props: {bold: true}}
{op: "insert_after",    target: "s4.b2",    docmark: "- New line"}
{op: "duplicate_slide", target: "s4",       after: "s7"}
{op: "set_cell",        target: "s9.tbl1!B3", value: 1420, formula: "=SUM(B1:B2)"}
```

Three details that matter for an agent:

- **`dry_run`** returning the change report without writing: a free verification step.
- **The response carries the applied diff and the new etags**, so the agent needs no
  confirmation read. This removes roughly half the calls in a typical edit loop.
- **Transactional**: all operations apply or none, with atomic write. An agent that dies
  half-way never leaves a corrupt `.pptx`.

### 6.4 Slimming the format itself

- **Raw blocks out of the body.** A `p:timing` subtree can be thousands of tokens; no agent ever
  needs it. Sidecar (`assets/_raw/`, or `<doc>.raw.json`) referenced by id:
  `::: {.animation raw=r7}`. In pptx this single change cuts more context than everything else
  combined.
- **Delta emission against inheritance.** Never serialize an attribute already implied by
  layout/master/theme. In a corporate deck most placeholders should emit *zero* geometry or font
  attributes. It is the "style = reference + delta" decision already taken for docx, applied to
  the presentation cascade — simultaneously a fidelity and a token improvement.
- **Dictionary of repeated attribute sets.** When an attribute pattern repeats N times, intern
  it in the front matter and reference it (`{.g1}`). Applies to geometry repeated across slides
  and to cell formats in xlsx.
- **Readable numbers with tolerance.** `914400` EMU is 3–4 tokens; `1in` is one. Serialize in
  pt/cm/in with configurable precision and compare geometry in round-trip with tolerance instead
  of byte equality.
- **Measure, do not estimate.** `docsai tokens <file> --fidelity full|standard|plain|agent` with
  a real tokenizer, plus a CI gate that blocks PRs inflating the corpus by more than X %.

### 6.5 A fourth fidelity level: `agent`

The three existing levels are axes of *loss*. What is missing is an axis of *editable surface*.
`--fidelity agent`: everything an agent can safely modify, as text; everything else present but
collapsed to one line with id and etag. A 60-slide deck drops from ~45k to ~6k tokens with no
loss of editing capability, because the ids are still there and the writer recomposes from the
original package. This is the same insight as §5.1: if the original package is the source of
truth for everything non-editable, the Markdown can be far smaller without sacrificing fidelity.

### 6.6 Fewer steps, not only fewer tokens

- **Tolerant input.** An agent writing plain Markdown with no attributes at all must produce a
  correct pptx: automatic layout selection from content shape (title + bullets → "Title and
  Content"; title only → "Title Slide"). Making the full specification optional *for writing* is
  what avoids the "generate → fail → fix" cycle.
- **Validation as a tool, not an error.** `validate_docmark` returning typed errors with node id
  and a suggested fix. One step instead of three.
- **Contextual cheat-sheet.** Expose, as an MCP resource, a compact summary of the DocMark
  syntax *relevant to this document* (no spreadsheet metadata explanation when there are no
  sheets). Avoids putting the whole spec in the system prompt.
- **Images: `inline-base64` by default is expensive.** Default changes to references, with
  `include_images=none|refs|thumbnails|full`. For pptx, `thumbnails` renders the whole slide to
  PNG via LibreOffice: for a vision model a slide thumbnail is worth more than forty serialized
  shapes and costs less.
- **Content-hash cache.** Still stateless at the protocol level, but an LRU keyed by content
  hash avoids re-parsing the same deck across the 8 calls of a single loop.

### 6.7 Architectural consequence, stated on purpose

Stable ids (§6.1) and patch editing (§6.3) push docsai from "converter" towards "document server
with state". Plan v2 accepts this **explicitly and in a bounded form**: the ids live *inside the
document* (front matter + attributes), not in a server-side database, and the cache is a pure
performance optimisation that can be dropped without changing behaviour. Statelessness of the
MCP protocol is preserved. The line we do not cross in v2: no sessions, no locks, no server-held
document handles.

---

## 7. Risks and mitigations

| # | Risk | Prob. | Impact | Mitigation |
|---|---|---|---|---|
| P1 | The slide→layout→master→theme cascade costs more than docx's | High | High | Spike (Phase 12); v1 resolves colour/font/size and leaves the rest to the layout reference |
| P2 | Round-trip is valid but PowerPoint offers to "repair" | Medium | **Very high** | Preserved-skeleton strategy (§5.1) + schema and render CI gates (§5.5). **Open after spike P3**: the mechanism loses nothing over 48 packages in LibreOffice, but LibreOffice was measured to accept a deck with five dangling relationships, so it cannot stand in for PowerPoint. Steps to close it in [`spikes/P3-preserved-skeleton.md`](spikes/P3-preserved-skeleton.md) §6.2 |
| P3 | Freeform shapes / SmartArt / animations fragment the effort | High | Medium | Aggressive raw-block from day one; only model what has a Markdown representation |
| P4 | DocMark-P becomes unreadable (stops being Markdown, becomes XML with `:::`) | Medium | High | Golden rule: `--fidelity standard` must be hand-editable by a human; if it is not, the design failed |
| P5 | Text overflow after an agent edit | High | Medium | Explicit `Warning::AutofitStale`; no measurement in v1 |
| P6 | `ooxmlsdk` inflates binary and compile time | Medium | Medium | Measured in the spike; `cargo bloat` already in CI |
| P7 | Stable ids drift or collide across round trips | Medium | **Very high** (silently wrong edits) | Ids are part of the golden tests; dedicated property test: ids survive N round trips and never get reused |
| P8 | `apply_edits` surface grows without bound (one op per IR feature) | High | Medium | Closed op set in v2, versioned; anything not covered is done by `read --select` + rewrite of that node |
| P9 | The skeleton blob makes `.dmk.md` packages heavy on disk | Medium | Low | Content-hash dedupe (already in `AssetStore`) + `--no-skeleton` for plain/standard fidelity |

## 8. Sources consulted

- ECMA-376 Part 1, §19 (PresentationML) and §20 (DrawingML).
- [MS-PPT], [MS-ODRAW] (legacy binary PowerPoint and the Escher/OfficeArt drawing layer).
- OASIS OpenDocument 1.3, §10 (drawing and presentation).
- Crate documentation: `ooxmlsdk`, `pptx`, `ppt-rs`, `pptx-to-md`, `msoffice_pptx`,
  `office_oxide`, `calamine`.
- `python-pptx` documentation, used as the corpus generator and as a model for the
  placeholder API.
- Prior docsai spikes: [`spikes/R1-docx-strategy.md`](spikes/R1-docx-strategy.md),
  [`spikes/R3-xlsx-writer.md`](spikes/R3-xlsx-writer.md).
