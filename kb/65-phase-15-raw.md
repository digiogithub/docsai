---
tags:
    - phase-15
    - pptx
    - presentations
    - writer
    - raw
    - skeleton
---
# 65 — Phase 15 D: the preserved object goes back as itself

Increment **15-D** of [[61-phase-15-plan]], on top of [[64-phase-15-media]]. The write side of
[[43-phase-13-raw]]: what the reader kept verbatim, the writer puts back verbatim.

## What changed

- `pptx/write.rs`: `reinject`, `Reinjected`, `fragment_name` and an `Outside<'_, '_>` context that
  carries the part name, the package, the relationships, the media and the fragment index into the
  slide rewrite (`rewrite_slide` had reached eight arguments).
- `ShapeKind::Raw` is no longer a `Degraded` warning. It is either silence — the object is in the
  file — or a `Warning::RawBlockDropped`, or a named `Degraded` for a part this writer may not
  rebuild.
- Slide-level fragments (`Slide::raw`) are handled before the shape loop, matched among the
  children of `p:sld`.
- `docsai-convert`: the `SUPPORT` note, and a pipeline test through DocMark and back.

## Non-obvious decisions

1. **The skeleton had already done the work, and the honest increment is admitting it.** The
   writer splices byte ranges into the preserved part ([[63-phase-15-slides]]); an element nobody
   edited is never in an edit, so a group, a custom geometry, an `mc:AlternateContent`, a
   `p:transition` **already** left the writer as the bytes that came in, at the z-order they came
   in at — which is exactly what 15-D promises. What the increment adds is subtraction: the
   `Degraded` warning that said «it was copied from the preserved package, so an edit to it in the
   model is not in the file» was *false*, and a false warning is the same failure as a missing one
   (`AGENTS.md` §7 rule 3 cuts both ways). A caller who reads warnings has to be able to believe
   them.
2. **The payload is still needed, for the one case the skeleton cannot cover**: a document whose
   `RawFragment::content` differs from the package's bytes — a caller that edited the markup
   directly, or a model assembled from a sidecar. Then the fragment is spliced over the element it
   belongs to. Comparison first, edit second: an identical fragment produces no edit, which is
   what keeps the corpus byte-identical.
3. **A fragment carries no id in the file, so the match is by element name.** `fragment_name`
   reads the qualified name off the fragment's first tag — the fragment is a *slice* of the part,
   so its first `<` opens the element it preserves. A shape fragment has exactly one candidate
   (the element at its z-index) and the name only confirms it; a slide-level fragment has the
   children of `p:sld` to choose among, and the name is what tells `p:transition` from `p:timing`.
   A name that cannot be read matches **nothing** — it is reported as unplaceable and the
   package's bytes stand. The first version of this increment fell back to the first candidate,
   which for a slide-level fragment is `p:cSld`: not a degraded slide but a corrupt one, caught in
   review and pinned by a test. `p:cSld` is excluded from the candidates outright for the same
   reason, because `splice` drops an overlapping edit without a word.
4. **A round trip through DocMark carries no payloads at all.** `deck_parser` leaves
   `Presentation::raw` empty and says so: the sidecar the writer would read is 15-E's or later's
   bookkeeping. So the common case is a stub pointing at a fragment nobody supplied — and that is
   **not** a warning, because the object is in the written package. Inventing markup for it, or
   warning about a loss that did not happen, would both be worse. The pipeline test in
   `docsai-convert` is what pins this: the group, the custom geometry, the transition and the
   timing are all in the package after `pptx → DocMark → pptx`.
5. **A foreign dialect is dropped, not translated.** `odf` markup inside a `p:spTree` is not a
   degraded deck, it is a broken one — a repair dialog, which is the failure mode this phase is
   measured on. `Warning::RawBlockDropped { id, format }` names it and the package's own bytes
   stand.
6. **`rebuilt_parts` is still the licence.** A fragment whose `part` is not the slide being
   rewritten — a chart's XML, which the reader captures whole — is copied and named, never written.
   Rewriting `ppt/charts/chart1.xml` from a payload is Phase 16's, and a part outside the licence
   is copied whatever the IR seems to know about it.
7. **A chart is not routed through here.** `ShapeKind::Chart` keeps its `Degraded` warning: the
   frame's markup is in the slide and untouched, but the *data* the model records is not written
   back, and that is a real gap with a real phase behind it.

## Known losses, written down

- **A group's structure is still not modelled** ([[43-phase-13-raw]] decision 5). The group goes
  back whole; editing a shape inside it in the IR is not possible, and never was.
- **A raw shape's geometry and name are read but not written.** Moving a stub in the model does
  not move the object: its `p:spPr` is inside the fragment. Silent today only because nothing can
  produce that edit through DocMark — the stub carries no position to change.
- **A payload edit is trusted, not validated.** A fragment the caller replaced with ill-formed XML
  reaches the package. The XSD gate of 15-I is where that becomes visible; guessing at it here
  would mean parsing every fragment on every write.

## How it was verified

- Five new writer tests: the preserved objects of `raw-preserved.pptx` leave the writer with
  `ppt/slides/slide1.xml` byte for byte the part that was read and no warning about them; an edited
  shape fragment lands over its shape with the neighbours untouched; an edited `p:transition`
  lands at slide level; an `odf` fragment is dropped, named, and its markup is not in the package;
  a fragment whose part is `ppt/charts/chart1.xml` is kept and the part is named.
- A review pass over the diff found the fallback of decision 3 (a fragment whose root element
  cannot be named landing on the first candidate) and a slide-level test that asserted the new
  attribute without asserting the old one was gone. Both fixed here: the fallback is a named
  `Degraded`, and the test now compares the whole part against the original with the one attribute
  changed back.
- One pipeline test: `raw-preserved.pptx → DocMark → pptx` keeps `p:grpSp`, `a:custGeom`,
  `p:transition` and `p:timing` while the retyped title reaches the package.
- End to end outside the suite: `convert raw-preserved.pptx -o deck.dmk.md`, retype the title,
  `convert deck.dmk.md -o out.pptx` — the group, the custom geometry, the transition, the timing
  and the triangle's own label are all in the written slide, and the write side emits **no
  warning at all**, where before this increment it emitted two. The LibreOffice render was not
  run: the installation on this machine is broken (`soffice.bin: error while loading shared
  libraries: libreglo.so`), which is an environment fault, not a finding about the deck.
- `cargo test --workspace --no-fail-fast`: only the pre-existing `docx_roundtrip_is_idempotent`
  fails (`corpus/docx/footnotes.docx`, from `88ff8e11`, untouched by this increment).
  `clippy --workspace --all-targets -- -D warnings` clean, `fmt --check` clean.

## Next

15-E: notes pages regenerated for the slides that have them, `p:sldIdLst` and `p14:sectionLst`
updated when slides move — the only edits to `ppt/presentation.xml`, which is otherwise copied.
