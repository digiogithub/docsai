# Spike P1 — PPTX reading strategy

**Risk mitigated**: P6 from `technical-analysis-presentations.md` §7 — *«`ooxmlsdk` inflates
binary and compile time»* — and, by extension, the library question P1 in §3 leaves open.

**Question**: is `ooxmlsdk` 0.12 enough to read `.pptx`, or is a custom parser on `zip` +
`quick-xml` required, as Phases 1–7 did for `.docx`, `.xlsx` and ODF?

**Date**: August 2026 · **Version evaluated**: `ooxmlsdk` 0.12.0 (default features: `parts`) ·
**Toolchain**: rustc 1.96.0-nightly (900485642, 2026-04-08). CI builds on stable; nothing measured
here depends on a nightly feature, but the absolute timings would shift on another toolchain.

**Decision**: **custom parser on `zip` + `quick-xml`** in `docsai-office::pptx`. `ooxmlsdk` is not
used on the read path. It is kept as an *XML mapping reference* — the best one available, being
generated from the ECMA-376 schemas — and as a candidate fallback for spike P3, where its
package-level round trip is genuinely strong.

This is the same verdict spike R1 reached for `docx-rs`, but **not for the same reasons**, and the
difference matters: `ooxmlsdk` is a much better library than `docx-rs` was. It is refused on two
measurements, not on a pile of gaps.

---

## 1. Method

Four metrics, all measured against `corpus/pptx` (the ten fixtures built in increment 12-A), with
a probe binary outside the tree — a discarded dependency is not versioned. Reproduction steps in
§6.

1. **Fidelity** — open every fixture, walk the typed model, and print one line per trait the
   presentations track has a question about: `p:sldIdLst` order, placeholder type and index,
   `a:pPr@lvl`, `buChar`/`buAutoNum`, notes slides, `a:tbl`, `r:embed` and `a:xfrm` on pictures,
   `prstGeom` on free shapes and connectors, `a:normAutofit@fontScale`. A trait the model dropped
   simply does not appear.
2. **Retention** — open with `PackageOpenMode::Eager` so every root passes through the typed
   model, write the package back, and compare it part by part with the original. This is the R1
   question restated: *what disappears between reading and writing?*
3. **Binary size and compile time** — measured under the workspace's own
   `[profile.release]` (`lto = true`, `strip = true`, `codegen-units = 1`), against a baseline
   crate doing the same zip work with `zip` alone.
4. **Behaviour on corrupt input** — truncations every 7 bytes and single-byte flips every 13, the
   shape spike R1 used, counting panics. `AGENTS.md` §6: a parser returns `Err`, it does not
   panic.

## 2. Results

### 2.1 Fidelity — everything the corpus isolates, resolved

All ten fixtures parse, and every trait comes back typed:

| Trait | Fixture | Result |
|---|---|---|
| `p:sldIdLst` order vs file names | `slide-order` | ✅ `["rIdSlide2", "rIdSlide3", "rIdSlide1"]`, resolved to the right parts through the relationship table |
| Placeholder type + `idx` | `placeholders-cascade` | ✅ `ph(type=Some(Title) idx=None)`, `ph(type=Some(Body) idx=Some(1))` |
| Empty paragraph as content | `placeholders-empty` | ✅ the empty `a:p` is present, not skipped — the Phase 1 bug does not repeat here |
| `a:pPr@lvl` | `bullets-levels` | ✅ levels 0/1/2 as `Option<i32>` |
| `buAutoNum` / `buChar` | `bullets-levels` | ✅ `AutoNumberedBullet { type: ArabicPeriod, start_at: None }` |
| Notes slides | `notes-speaker` | ✅ both notes parts, text intact |
| `p:graphicFrame` → `a:tbl` | `tables-simple` | ✅ typed `Table` with `graphic_data.uri` preserved |
| Picture `r:embed` + geometry + alt | `images-anchored` | ✅ `a:blip@r:embed`, `a:off`/`a:ext` as EMU newtypes, `descr` kept |
| `prstGeom`, `p:cxnSp` | `shapes-geometry` | ✅ `RoundRectangle`, `RightArrow`, `Line` on the connector |
| `a:normAutofit` | `autofit-stale` | ✅ `font_scale: Some(Decimal(62500))`, `line_space_reduction: Some(Decimal(20000))` |

SmartArt is modelled too (`dgm:relIds` is a typed variant of `a:graphicData`), which is more than
the analysis in §3 assumed.

**This metric was found in `docsai`'s favour and it is worth saying plainly: `ooxmlsdk` reads
`.pptx` well.** The typed model is complete, the EMU newtypes match ours, and the placeholder
cascade — the trait the whole phase was nervous about — arrives fully addressable.

**The spike also found a defect in our own corpus.** `notes-speaker.pptx` failed to open with
`UnexpectedTag { ty: "NotesSlide", expected: "NotesSlide", found: "p:notesSlide" }`. The library
was right: the part is *named* `notesSlide` and so is its content type, but ECMA-376 binds
`CT_NotesSlide` to the element **`p:notes`**. The generator wrote `p:notesSlide`. Fixed in
`corpus/generate.py` and the fixtures regenerated. `corpus/opc_check.py` had passed the file —
it checks structure, not schema — which is the second time in this phase that gap has cost
something (the first was a double `a:pPr`, recorded in `kb/30-phase-12-pptx-corpus.md`).

### 2.2 Retention — modelled content survives, unmodelled content vanishes silently

Loading every root and writing the package back:

- **No part is lost or added.** Ten fixtures, zero `LOST PART`, zero `NEW PART`.
- **Media is byte-identical.** `ppt/media/image1.png` comes back unchanged.
- **Every XML part is rewritten**, but the differences are serialization style, not content:
  `<a:avLst/>` becomes `<a:avLst></a:avLst>`, `<a:off x="0" y="0"/>` gains a space. No modelled
  attribute or element moved or disappeared.
- **`docProps/core.xml` is rewritten more aggressively**: the `cp:` prefix is replaced by a
  default namespace (`<cp:coreProperties>` → `<coreProperties xmlns="…core-properties">`,
  `cp:lastModifiedBy` → `lastModifiedBy`) and the children are reordered. Namespace-equivalent,
  but a consumer that matches on prefixes — or a schema that fixes the child order — would see a
  different document. Not verified in PowerPoint.

Then the load-bearing test. A slide was given two things the schema does not define:

```xml
<x:widget xmlns:x="urn:example:widget" kind="counter"><x:value>7</x:value></x:widget>
<mc:AlternateContent><mc:Choice Requires="p14">…</mc:Choice><mc:Fallback>…</mc:Fallback></mc:AlternateContent>
```

Result:

```
LOST  x:widget
LOST  urn:example:widget
LOST  <x:value>7
KEPT  mc:AlternateContent
KEPT  mc:Choice
KEPT  mc:Fallback
```

`mc:AlternateContent` survives — it is a first-class variant of the shape-tree choice, which is a
real strength. **The foreign element is dropped, and nothing says so**: no error, no warning, no
residue. That is exactly the failure mode that made R1 refuse `docx-rs`, and it is precisely what
`raw-block` (spec §7) and the measurable coverage criterion exist to prevent. An unknown element
that vanishes silently cannot be reported, cannot be preserved on write, and cannot be counted
against a coverage number.

The `validators` feature does not change this: validation reports what is invalid, it does not
retain what is unmodelled.

### 2.3 Binary size and compile time — risk P6, confirmed

Both crates built with the workspace's release profile, from clean:

| | Baseline (`zip` only) | With `ooxmlsdk` | Delta |
|---|---|---|---|
| Stripped binary | 845 440 B (0.85 MB) | 29 520 328 B (29.5 MB) | **+28.7 MB** |
| Clean release build | 11.0 s | 322.4 s | **+311 s (×29)** |

For scale: the `docsai` release binary is 12 297 808 B (12.3 MB) today. Linking `ooxmlsdk` would
roughly **triple** it, for one of four supported formats. LTO and `codegen-units = 1` are already
doing their work in that 29.5 MB — the unstripped, non-LTO build was 45.0 MB.

The compile-time number is per clean build, and CI does three of those (ubuntu, windows, macos)
on every push.

### 2.4 Corrupt input — the analysis was wrong here, in `ooxmlsdk`'s favour

```
1396 corrupt inputs (truncations + byte flips on images-anchored.pptx)
  → Ok: 140   Err: 1256   PANIC: 0
```

**Zero panics.** `docx-rs` panicked on 204 of 903 (23%) in spike R1. `ooxmlsdk` meets the
`AGENTS.md` §6 criterion outright. The 140 `Ok` results are inputs whose corruption fell in
compressed payloads the package never had to decode — not a fidelity claim.

## 3. Analysis

Three of the four metrics came out well, and two of those better than §3 of the analysis
predicted. The decision turns on the two that did not.

**Silent loss of unknown elements is disqualifying on its own.** `docsai`'s contract is that
anything it cannot model is preserved as a raw-block and *counted*. A reader that drops
`urn:example:widget` without a word makes that contract unimplementable for `.pptx`: coverage
would be unmeasurable, `--fidelity full` would silently be less than full, and the round trip
would quietly delete a vendor extension the user cared about. Wrapping `ooxmlsdk` in a
complementary pass that re-reads the same XML to find what it dropped means parsing every slide
twice with two models and reconciling them — the same trap R1 identified, at a larger scale,
because here we would be reconciling two *complete* trees rather than patching gaps in one.

**+28.7 MB and +5 minutes per clean build is a lot to pay for the easy half.** The typed model
covers pml and dml exhaustively, which is real work saved. But what it saves is the mechanical
part — the part the corpus and the golden tests make cheap to write and cheap to verify. It does
not save the placeholder cascade resolution (reference + delta is our model, not the schema's),
the DocMark-P mapping, or the skeleton. And `docsai` is a local CLI an agent shells out to; a
30 MB binary and a five-minute CI leg are felt on every use and every push.

**Consistency has a cost too.** `docsai-office` already reads `.docx` and `.xlsx`, and
`docsai-odf` reads ODF, all on `zip` + `quick-xml`, all with the same raw-block hatch and the same
`ImageRef`/`ImageGeometry`/`Length` types. The DrawingML in `.pptx` is *the same DrawingML* those
readers already handle. Adding `ooxmlsdk` means two XML parsers and two shape models in one
binary, with `.pptx` behaving differently from every other format on the one axis — unknown
content — where behaving the same is the whole point.

## 4. Decision

**Custom parser on `zip` + `quick-xml`** in `docsai-office::pptx`, with these consequences:

1. **No `ooxmlsdk` in the workspace.** Recorded here per the `AGENTS.md` §2 rule that a key
   dependency is not chosen or refused without leaving a record.
2. **A single event-mode pass** per part, producing the IR directly and capturing every
   unrecognized element as a raw-block with its part and path — same as `.docx`. Coverage is
   measurable from the first slide read.
3. **`ooxmlsdk` remains the mapping reference.** When the question is *what is the Rust-shaped
   name of `a:normAutofit@fontScale`, and what type is it*, its generated schemas answer faster
   and more reliably than reading ECMA-376, because they **are** ECMA-376. Consulting it costs
   nothing; linking it costs 28.7 MB.
4. **Its package round trip is an input to spike P3, not a dead end.** No part lost, no media
   touched, `mc:AlternateContent` preserved: that is evidence the preserved-skeleton idea is
   sound at package level. If P3 finds writing the OPC container back by hand harder than
   expected, `ooxmlsdk` behind a non-default feature is a fallback worth re-costing — with the
   28.7 MB stated up front.
5. **The corpus gets a schema gate.** Two schema-invalid fixtures in one phase, both caught by
   accident, is a pattern. What form the gate takes is spike P3's call (it is the increment that
   has to open files in a real renderer anyway), but P1's finding is that `opc_check.py` alone is
   not enough.

### Risks this decision introduces

| Risk | Mitigation |
|---|---|
| pml + dml is a large surface to type by hand | Only what the IR models gets typed; the rest is raw-block. `ooxmlsdk`'s schemas are the reference for every mapping, which removes the research cost, not the typing cost |
| A trait forgotten through ignorance | Generic unknown-element capture makes it **visible** (warning + raw-block) instead of silent — the exact property `ooxmlsdk` lacks |
| The `.pptx` writer might want a typed model | Independent later decision. Writing is the easier direction (we control the output), and §4 point 4 keeps `ooxmlsdk` on the table for the skeleton |

## 5. Status of the risks

- **P6** (`ooxmlsdk` inflates binary and compile time) — **closed, confirmed**: +28.7 MB and
  +311 s measured. The decision not to link it retires the risk.
- **P1** (cascade costs more than docx's) — **not closed here**, and the evidence is mildly
  encouraging: the cascade is fully addressable in a typed model, so the difficulty is in
  resolving it as reference + delta, not in reaching it. Phase 13 work.
- **P3** (freeform / SmartArt / animations fragment the effort) — unchanged, but the mapping
  reference is better than assumed: SmartArt and `mc:AlternateContent` both have schema shapes to
  copy when deciding where the raw-block boundary goes.

## 6. Reproducing this spike

The probe lived outside the tree. To rebuild it:

```bash
python3 corpus/generate.py
cargo new /tmp/spike-pptx && cd /tmp/spike-pptx
cargo add ooxmlsdk@0.12.0 zip@8
# append the workspace release profile to Cargo.toml, or the size numbers are not comparable:
#   [profile.release]
#   lto = true
#   strip = true
#   codegen-units = 1
```

The probe has three modes over `PresentationDocument`:

- `dump <file>` — open with `PackageOpenMode::Eager`, print `p:sldIdLst`, then per slide walk
  `common_slide_data.shape_tree.shape_tree_choice`, printing placeholder type/index, `a:pPr@lvl`,
  the bullet choice, `a:xfrm`, the geometry choice and the autofit choice, plus the notes-slide
  text.
- `roundtrip <file>` — open eagerly, `save_as_file`, then compare the two zips member by member
  (same / CHANGED / LOST PART / NEW PART).
- `fuzz <file>` — truncate every 7 bytes and flip every 13th byte, open each under
  `catch_unwind`, count Ok / Err / PANIC.

For the retention test, inject into a slide's `p:spTree`, before `</p:spTree>`, an element in a
namespace the schema does not define (`<x:widget xmlns:x="urn:example:widget">`) and an
`mc:AlternateContent` with a `p14` Choice and a Fallback, then round-trip and grep the output for
both.

Size and time baseline: a second crate with `zip` only, doing the same member-by-member
comparison, built from clean with the same profile.
