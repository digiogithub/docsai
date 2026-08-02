# Spike R1 — DOCX reading strategy

**Risk mitigated**: R1 from technical analysis §6 — *«The OOXML style cascade turns out more
expensive than expected and delays Phase 1»*.

**Question**: is `docx-rs` (+ a custom complement with `quick-xml` where it falls short) enough
for Phase 1, or is a full custom OOXML parser required?

**Date**: August 2026 · **Version evaluated**: `docx-rs` 0.4.22 · **Toolchain**: rustc 1.94.1

**Decision**: **custom parser on `zip` + `quick-xml`.** `docx-rs` is not used on the read path.
It is kept as an XML mapping reference and as a possible base for the Phase 2 *writer*
(independent decision, to be revisited in due course).

---

## 1. Method

The Phase 0 corpus was generated (`corpus/generate.py`) and read with `docx_rs::read_docx`,
serializing the result to JSON to inspect what information survives. Each trait was checked
against what the DocMark specification and the IR in `architecture.md` §3 require.

Additionally the reader was subjected to 903 synthetic corrupt inputs (truncations every 7 bytes
and single-byte flips every 13 bytes on `images-floating.docx`) capturing *panics*.

Documents used: `basic-styles`, `custom-styles`, `nested-lists`, `footnotes`, `fields-raw`,
`headers-footers`, `images-inline`, `images-floating`, `images-transformed`, `images-vml`.

## 2. Results

### 2.1 What `docx-rs` does resolve

| Trait | Status | Note |
|---|---|---|
| `styles.xml` with `docDefaults` | ✅ | `runPropertyDefault` and `paragraphPropertyDefault` exposed |
| `basedOn`, `styleType`, `name` | ✅ | Enough to rebuild inheritance |
| Paragraph/run formatting as a **delta** | ✅ | The model separates `property.style` from direct formatting, which is exactly the «reference + delta» principle |
| `numbering.xml` | ✅ | `abstractNums` + `numberings` + levels with `format`, `lvlText`, indentation |
| Headers and footers | ✅ | Resolved and attached to `sectionProperty` (`header`, `firstHeader`, `footer`) |
| `sectPr` (size, margins, `titlePg`) | ✅ | `columns` is exposed but not the rest of `w:cols` |
| Complex fields (`fldChar`/`instrText`) | ✅ | `instrTextString` preserved |
| Tables with `gridSpan`/`vMerge` | ✅ | Present in `tableCellProperty` |

The 4-level cascade **is solvable** with what it exposes: risk R1, as originally framed (the
*styles*), does not materialize. It is the rest that fails.

### 2.2 What `docx-rs` loses

Measured on the corpus, not inferred from documentation.

**Images** — the `Pic` model exposes `size`, `positionType`, `positionH/V`, `relativeFromH/V`,
`distT/B/L/R`, `relativeHeight`, and `rot`. It does not expose:

| DocMark §3.5 attribute | OOXML origin | In `docx-rs` |
|---|---|---|
| `wrap`, `wrap-side` | `wp:wrapSquare/Tight/Through/TopAndBottom` | ❌ absent |
| `anchor=behind` | `wp:anchor @behindDoc` | ❌ absent |
| `crop` | `a:srcRect` | ❌ absent |
| `flip` | `a:xfrm @flipH/@flipV` | ❌ absent |
| `rotation` | `a:xfrm @rot` | ⚠️ field `rot: u16` present but returns `0` for `rot="2700000"` (45°); also `u16` cannot represent 60000ths of a degree nor negative values |
| alt (`![…]`) | `wp:docPr @descr` | ❌ absent |
| `title`, `name` | `wp:docPr @title/@name` | ❌ absent |
| `link` | `a:hlinkClick` | ❌ absent |
| `external-src` | `r:link` | ❌ absent |
| `border` | `pic:spPr/a:ln` | ❌ absent |
| media bytes | `word/media/*` | ❌ not loaded on read (`image` is «for writer only») |

**Legacy VML** (`w:pict`, `images-vml.docx`): collapses to a generic `shape` node of 813 bytes
of total JSON; the `r:id` of `v:imagedata`, the `style` (position and size), the `alt`, and
`w10:wrap` are lost. Total loss of the object.

**Footnotes** (`footnotes.docx`): `w:footnoteReference` is discarded — the run ends with
`children: []`. `word/footnotes.xml` is not exposed in the API.

**Simple fields** (`fields-raw.docx`): `w:fldSimple` loses the `w:instr` attribute; only the
cached text remains. «Date: 01/01/2026» stops being a `DATE` field.

**Fidelity escape hatch**: `w:sdt` is represented as `structuredDataTag` with `alias: null` and
without the original XML. There is no generic mechanism that preserves the bytes of an unknown
element, which is exactly what the `raw-block` of the spec §7 and the measurable coverage
criterion of Phase 1 (task 9) need.

**Noise in the model**: every style read comes out with a `tableProperty` with invented borders
(`single/2/000000`) even when the style is paragraph — it would have to be filtered so as not to
contaminate the front-matter catalog.

### 2.3 Robustness against corrupt input

```
903 corrupt inputs (truncations + byte flips)
  → Ok: 88   Err: 611   PANIC: 204
```

23% of corrupt inputs cause a *panic*. The Phase 1 acceptance criterion is explicit: *«Zero
panics with synthetic corrupt corpus (truncated ZIP, malformed XML): always `Err`»*. Wrapping the
whole reader in `catch_unwind` is not an acceptable mitigation (it does not work with
`panic=abort`, and `AGENTS.md` §6 requires parsers to return `Err`, not to recover).

## 3. Analysis

The custom complement needed to close the gaps includes: all of `w:drawing` (full DrawingML),
all of `w:pict` (VML), `word/footnotes.xml`, `w:fldSimple`, capture of unknown elements for
raw-blocks, and loading `word/media/*`. That is: **the bulk of `document.xml`**. What would
remain delegated to `docx-rs` is `styles.xml`, `numbering.xml`, and the paragraph/table tree —
the most mechanical and best-documented part of the format.

Keeping both paths also implies:

- Two XML parsers in the binary (`xml-rs` inside `docx-rs`, `quick-xml` in ours).
- Reconciling two distinct trees of the same `document.xml` (`docx-rs`'s and ours for
  drawings/fields), with the risk of position desynchronization.
- A dependency we rely on for the easy part and not the hard part, with a *panic* risk that must
  be assumed or patched upstream.

## 4. Decision

**Custom parser on `zip` + `quick-xml`** in `docsai-office`, with these consequences:

1. **No `docx-rs` in `docsai-office`.** Noted in `technical-analysis.md` §4.1 per the
   `AGENTS.md` §2 rule (a key dependency is not replaced without leaving a record).
2. **A single pass** over `document.xml` with event-mode `quick-xml`, producing the IR
   directly and capturing any unrecognized element as a raw-block, with its part and path. This
   makes coverage measurable from day one.
3. **No `unwrap`/`expect`/unchecked indexes** on the read path: every error is a typed
   `ReadError`. The «zero panics» criterion is verified with a synthetic corruption test
   equivalent to this spike's, run in CI from Phase 1.
4. **`quick-xml` with external entities disabled** (default behavior: does not expand external
   entities), which advances part of Phase 8.
5. The estimated cost of the custom parser (≈2 of the 4–6 weeks of Phase 1) is comparable to the
   complement that would have to be written anyway, and eliminates tree reconciliation.

### Risks this decision introduces

| Risk | Mitigation |
|---|---|
| More custom surface to maintain | Corpus + golden tests from Phase 0; the parser covers only what the IR models, the rest goes to raw-block |
| OOXML traits forgotten through ignorance | Generic capture of unknown elements makes them **visible** (warning + raw-block) instead of silent |
| The Phase 2 writer might need `docx-rs` | Independent later decision; writing is much simpler than reading (we control the output XML) and will likely also be done by hand |

## 5. Status of risk R1

**Closed.** The style cascade is not the bottleneck; the image model was, and the custom-parser
decision neutralizes it. R8 (diversity of image models) is covered by the same decision:
DrawingML and VML are read in the same pass into `ImageGeometry`.

## 6. Reproducing this spike

The probe program lived outside the tree (a discarded dependency is not versioned). To
reproduce:

```bash
python3 corpus/generate.py
cargo new /tmp/spike-docx && cd /tmp/spike-docx
cargo add docx-rs@0.4.22 serde_json
# read corpus/docx documents with read_docx() and serialize `docx.document` to JSON;
# for the robustness test, truncate and flip bytes of the .docx and count catch_unwind(Err)
```
