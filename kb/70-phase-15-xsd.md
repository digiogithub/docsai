---
tags:
    - phase-15
    - pptx
    - presentations
    - ci
    - validation
    - xsd
    - ecma-376
---
# 70 — Phase 15 I: the schema gate, and the two fixtures it caught

Increment **15-I** of [[61-phase-15-plan]], on top of [[69-phase-15-roundtrip]]. The first of the
phase's two CI gates: *generated packages validated against the ECMA-376 XSDs, the gate naming the
part and the element* — analysis §5.5, the section that says the existing suite «does not catch the
most expensive failure: PowerPoint asking to repair the file».

The increment's shape is the same as 15-H's. Building the gate was an afternoon; the increment is
what it found on its first run — **two corpus fixtures that are not schema-valid**, in decks nine
increments of writer tests had round-tripped, byte-compared and declared 100 % faithful without
anyone ever asking whether the bytes were legal.

## The three decisions, and why

### 1. The schemas are vendored, not fetched

They live in `schemas/`, unmodified, with `schemas/README.md` for provenance and
`schemas/ECMA-NOTICE.txt` for the licence text the licence requires to travel with them.

**Where they came from.** ECMA-376 Part 4 (Transitional Migration Features), 5th edition,
December 2016 — `ECMA-376-4_5th_edition_december_2016.zip`, 8 471 820 bytes, sha256
`bd25da11…d34ecd` — whose inner `OfficeOpenXML-XMLSchema-Transitional.zip` is the 26-file schema
set; and ECMA-376 Part 2 (Open Packaging Conventions), 5th edition, December 2021, whose inner
`OpenPackagingConventions-XMLSchema.zip` is the four OPC schemas. Both from
`https://ecma-international.org/wp-content/uploads/`, retrieved 2026-09-02. The standalone
`.../OfficeOpenXML-XMLSchema-Transitional.zip` URL that documentation everywhere still cites is
**dead**; the schemas ship inside the standard's own distribution zip, next to the PDF. Plus
`w3c/xml.xsd`, which the published set references (`xml:lang`, `xml:space`) and does not ship —
without it the schema set does not compile.

**Transitional and not Strict**, deliberately: Part 1 ships Strict, Part 4 ships Transitional, and
every deck any real application writes — every deck in `corpus/pptx` included — is Transitional.
Validating them against Strict would report the standard's own migration features as errors.

**Size: 1.0 MB, 31 files.** That is the number that decided it, and it is an order of magnitude
smaller than the «several megabytes» the increment was briefed to weigh — the 8.5 MB of the Part 4
download is almost entirely a 1553-page PDF. Against that: fetching means the gate depends on a URL,
and the URL this project's own documentation would have named is already 404. A red build caused by
`ecma-international.org` being slow says nothing about the writer, and a gate that needs the network
is a gate a developer cannot run on a plane and therefore routes around. **A validation gate whose
oracle can change under it without a commit is not a gate.**

**Licence, in full because it matters.** ECMA-376's own PDFs carry no copyright notice at all — the
standard is from 2006 and Ecma began printing the notice in 2009, saying that «in practice the same
principle also applies» to the older ones. The governing terms are therefore Ecma's Default
copyright notice by policy: the document «may be copied, published and distributed to others… in
whole or in part, provided that the above copyright notice and this Copyright License and Disclaimer
are included on all such copies», and «the content of this document itself may not be modified in
any way». So: verbatim redistribution is squarely permitted, the notice travels beside the files in
`ECMA-NOTICE.txt`, and **nothing in those two directories may be edited** — not reformatted, not
patched, not merged. Everything the gate needed to add is added in `schemas/ooxml-transitional.xsd`,
which is this repository's own file. Patents are not the constraint: Microsoft's declaration to Ecma
for ECMA-376 makes its licence offer «on a royalty-free basis» and points at the Open Specification
Promise, which covers Office Open XML.

### 2. `xmllint` runs it — the §7 rule 4 justification, in full

**No crate is added to the workspace, and nothing new reaches a shipped binary.** The validator is
`xmllint`, the command-line front end to libxml2, invoked as a subprocess by a Python script that
uses nothing outside the standard library. `AGENTS.md` §7 rule 4 asks four questions of a new
dependency — is it maintained, is it pure Rust, what does it add to binary size, and where is the
justification. The answer to the third is **zero bytes**, and that is the whole argument: a Rust XSD
validator (there is no maintained pure-Rust one that handles a schema set of this size, and the
serious options are bindings to libxml2 anyway) would put a C library or a large parser into the
dependency graph of a binary whose users convert documents and never validate schemas. The gate is
**CI-only** by construction: it is a Python script and an `apt-get install libxml2-utils` step in
one workflow job, and `cargo tree` is unchanged by this increment. libxml2 is maintained, is
present on every GitHub runner image and on macOS out of the box, and its XSD validator names the
element it rejected, which is the increment's own acceptance criterion. If `xmllint` is missing the
script exits 2 with the package name to install — never a skip that passes.

The alternative considered and rejected was the `ooxmlsdk` crate's `validators` feature, named in
analysis §5.5. It is a workspace dependency by definition, it validates through a generated Rust
model rather than against the published schemas, and it would make the oracle *our own reading of*
the standard rather than the standard. For a gate whose entire value is being an outside opinion,
that is the wrong shape.

### 3. What is validated: the parts the writer *wrote*, and nothing else

This is the decision the increment turns on, and the first run of the gate proved why.

A preserved deck written back is **byte for byte** the deck that was read — [[69-phase-15-roundtrip]]
asserts it over all seventeen corpus decks. So a gate that validated «the output of a round trip»
would validate the input, measure `python-pptx`, and report a green tick having examined nothing
this project produced. The very first version of the gate, run over the unedited round trip, found
**zero parts to validate**, which is the correct answer to the wrong question.

So the gate makes the writer write. Per deck, two packages:

| variant | how | what it exercises |
| --- | --- | --- |
| `edited` | the deck's DocMark with **every slide title retyped**, written back on the preserved package | the slide parts the writer rebuilds: text bodies, pictures, tables, the autofit drop (15-B, 15-C, 15-F) |
| `generated` | the same DocMark with the **`skeleton:` line removed**, so the deck has no package at all | the whole generation path on the embedded template: slides, notes pages, their `_rels`, `[Content_Types].xml`, `ppt/presentation.xml` (15-G) |

and then it **diffs the written package against the package it was written on** — the original deck
for `edited`, `crates/docsai-office/assets/default-template.pptx` for `generated` — and validates
every XML part whose bytes are new or changed. Nothing is listed by name anywhere: the scope is
computed, so a part the writer starts rebuilding in Phase 16 joins the gate the day it does.

Deliberately **not** validated, each with its reason:

- **A part copied byte for byte out of the skeleton.** It is not ours and it did not change.
  Validating it would report Microsoft's or `python-pptx`'s output as our defect, and the phase's
  whole argument is that a preserved part is preserved.
- **Non-XML parts** (media). There is no schema for a PNG, and 15-C's promise about them —
  re-injected without recompression — is a byte comparison, not a validation.
- **`docProps/core.xml`.** No schema in `schemas/` declares it: `opc-coreProperties.xsd` imports the
  Dublin Core schemas *by URL*, which are not ECMA-376, are not vendored, and would make the gate
  fetch `dublincore.org` on every run. It is therefore left out of the driver — and the consequence
  is not a silent exemption: a part whose root has no global declaration is reported by xmllint as
  «No matching global declaration available for the validation root», which is a failure. If the
  writer ever starts rebuilding that part, the gate goes red and says so.

## Markup compatibility is a real step, not a loosened rule

`corpus/pptx/smartart-fallback.pptx` failed the gate on `mc:AlternateContent`, and it took a moment
to decide honestly whether that was a defect or a false positive. It is a false positive, for a
reason written into the standard: `mc:AlternateContent` (ECMA-376 **Part 3**) is how a producer
offers an extension element with a fallback, it is legal wherever an element is, and the Part 4
schemas cannot say so because they describe the markup **after** compatibility processing. Every
PowerPoint deck with a diagram carries one; this one carries it in the *source* package, unchanged
by us.

So the gate does the processing a consumer does before validating: a validator that understands none
of the `Requires` namespaces — which is exactly what a Transitional-only schema set is — selects the
`mc:Fallback` branch. `markup_compatibility()` does that and nothing else, and a part with no
`AlternateContent` in it is returned **untouched**, so xmllint's line numbers still point into the
bytes the writer produced. A processed part is reported as `ok … (after mc: fallback selection)`
rather than plain `ok`, because a reader should know which line numbers moved. `mc:Ignorable` with
`mc:ProcessContent` is not implemented and is named as a limit: no part this gate validates uses it,
and one that did would be reported invalid rather than passed.

This is the one place the increment came close to loosening a gate to make it pass, and the test it
had to survive is: *would a conforming consumer do this?* It would. Dropping the element instead, or
skipping the part, would not have.

## What the gate found

First run, 34 packages, 322 parts, **three invalid**. All three in the `edited` variant — the one
that rebuilds a slide part on a preserved package — and all three, once traced, the *fixture's*
fault rather than the writer's. Which is itself the finding worth having: the corpus contains decks
that are not legal OOXML, and every fidelity claim made about them so far was a claim about
reproducing invalid markup faithfully.

| deck | what xmllint said | verdict |
| --- | --- | --- |
| `shapes-geometry.pptx` | `Element '…}txBody': Missing child element(s). Expected is ( …}p )` | **real**, in `corpus/generate.py` |
| `tables-simple.pptx` | `Element '…}txBody': This element is not expected. Expected is one of ( …drawingml…}txBody, …}tcPr, …}extLst )`, six times | **real**, in `corpus/generate.py` |
| `smartart-fallback.pptx` | `Element '…markup-compatibility…}AlternateContent': This element is not expected` | false positive; Part 3 processing added (above) |

### Defect 1 — a text body with no paragraph

`tx_body("")` produced `<p:txBody><a:bodyPr/><a:lstStyle/></p:txBody>`. `CT_TextBody` requires at
least one `a:p`, and no application writes a body without one — an empty placeholder in PowerPoint
holds an empty paragraph, not nothing. The generator now gives such a body the empty paragraph
`a_p("")` already builds, `<a:p><a:endParaRPr/></a:p>`.

Why *that* form and not the shorter `<a:p/>`: both are schema-valid, and the short one **broke byte
identity**. The writer normalises an empty paragraph to `<a:p><a:endParaRPr/></a:p>` — deliberately,
since that is where the properties of the text that would be typed there live and it is what every
producer writes — so a package containing a bare `<a:p/>` comes back with the longer form and is no
longer the package that was read. The fixture is the thing that was wrong here (it wrote a form no
producer writes), but **the underlying asymmetry is real and is not fixed**: see «Known limits»
below.

The fix reaches every corpus deck, because layouts and masters have empty placeholders too. Its
blast radius, measured rather than assumed: **every pptx golden changed on exactly one line**, the
`skeleton:` content hash, and one inspection golden gained a paragraph
(`shapes-geometry.expected.inspect.json`, `2 → 3`) — the empty paragraph the arrow shape always
should have had. No DocMark content moved.

### Defect 2 — a table cell with the wrong namespace

`<a:tc><p:txBody>…</p:txBody></a:tc>`. A DrawingML table cell holds an `a:txBody`: the same content
model, a different namespace. PowerPoint would be entitled to offer a repair. The generator is fixed;
**the writer was already right** — `write_gen.rs` has written `a:txBody` into a cell since 15-G, and
the reader looks up the child by local name, so it read both spellings and nothing downstream ever
noticed. This is precisely the class of defect a golden test cannot find: both sides agreed, and both
were wrong.

After both fixes and the Part 3 step: **34 packages, 322 parts, 0 invalid, 0 not measured.**

## Non-obvious decisions

1. **The gate fails when it cannot measure.** Three separate ways: a part xmllint reached no verdict
   on (a schema set that did not compile) is a named failure; anything xmllint says that is neither
   a verdict nor a parsed validity error is collected under a pseudo-part called `xmllint` and
   reported; and a variant that produced no part to validate is an exit-2 «nothing was measured
   for:» with the deck named. The failure mode this gate must not have is passing.
2. **One job, on Ubuntu only.** The bytes the writer produces are the same on all three platforms —
   the corpus tests assert exactly that — so running the gate three times buys three copies of one
   answer, at the price of an `xmllint` on Windows.
3. **It is a script, not a `cargo test`.** It needs a built CLI and an external tool, which is
   neither of the things `cargo test` is good at, and a schema failure should read as a schema
   failure in the checks list rather than as «some test failed» — the same argument `test.yml`
   already makes for the token budget.
4. **The mutation is a retyped title, not a synthetic model.** It is the edit a person actually
   makes, it forces every slide part in the deck to be rebuilt, and it needs no fixture that could
   drift from what the corpus contains.
5. **The whole published schema set is vendored, not the subset a `.pptx` needs.** `wml.xsd` and
   `sml.xsd` are 420 KB the pptx gate never reads. Keeping the set whole means the vendored
   directory is *exactly* the zip Ecma published — which is checkable, and which the licence's
   no-modification clause makes the safe reading — and it is what a future docx or xlsx gate would
   need anyway.

## Known limits, written down rather than discovered later

- **`<a:p/>` does not survive a round trip.** A package containing a bare empty paragraph comes back
  with `<a:p><a:endParaRPr/></a:p>`: schema-valid, visually identical, and **not byte-identical**.
  The IR does not distinguish the two spellings, so the writer cannot choose per shape; fixing it
  means a flag in the model saying which form the source used, which is an IR change and not this
  increment's. No corpus deck contains a bare `<a:p/>` any more, so nothing hides it — but a deck in
  the wild that does will fail the byte-identity claim on that shape and nothing will say so.
- **Schema validity is not «PowerPoint opens it».** A package can be schema-valid and still be
  repaired — a dangling relationship, a `r:embed` pointing at nothing, an `[Content_Types]` gap.
  That is what `corpus/opc_check.py` covers, and the two are complementary: this gate answers «is
  the markup legal», that one answers «does the package hang together». Neither is the P2 checklist
  of 15-K, which is a human in front of PowerPoint.
- **The preserved parts are unvalidated, and two of them are known to be invalid-adjacent.** Layouts
  and masters in the corpus carried the same empty `p:txBody` as defect 1 until this increment fixed
  the generator; nothing *in the gate* would have found them, because the writer never touches those
  parts. If a real deck arrives with an invalid layout, this project will preserve it faithfully and
  say nothing — which is the correct behaviour and worth knowing.
- **`docProps/core.xml` is unvalidatable** for as long as the Dublin Core schemas are not vendored.
  Vendoring them is a different licence question (they are DCMI's, not Ecma's) and was not opened
  here.
- **The gate is not run on Windows or macOS**, so a path bug in the script itself would only show up
  when someone runs it there. It uses `pathlib` throughout and no POSIX tool other than `xmllint`.
- **`--fidelity` is `full` throughout.** The gate never asks whether an `agent` or `standard`
  rendering writes a valid package, because those levels drop what a package needs and the writing
  path is the same one. It is worth asking eventually; it is not this increment.

## How it was verified

- **The gate itself, run here, over the whole corpus**: `python3 corpus/schema_gate.py` →
  **17 decks, 34 packages, 322 parts validated, 0 invalid, 0 not measured**, exit 0. Before the two
  generator fixes it was 3 invalid, exit 1, each failure naming the part, the line and the element.
- **A negative control**, because a gate that has never been red is not known to work: with
  `schemas/ecma-376-part4-transitional/dml-main.xsd` moved aside, the gate reports
  `FAIL ppt/slides/slide1.xml — xmllint reached no verdict on this part (did the schema set
  compile?)` plus the loader's own message under `FAIL xmllint`, and exits non-zero. With a `p:bogus`
  element spliced into a slide by hand it reports `Element '{…presentationml…}bogus': This element is
  not expected. Expected is one of ( …}bg, …}spTree )`. Both name the element, which is the
  increment's stated criterion.
- `cargo test --workspace --no-fail-fast`: **666 passing, 1 ignored**, the only failure the
  pre-existing `docx_roundtrip_is_idempotent` on `corpus/docx/footnotes.docx` (`88ff8e11`, untouched
  here). `clippy --workspace --all-targets -- -D warnings` clean, `fmt --all` applied,
  `python3 corpus/generate.py --check` clean at 129 files.
- **The goldens were regenerated deliberately and their diff read**: 17 pptx DocMark goldens changed
  on the `skeleton:` line only, one inspection golden on one paragraph count. No other corpus format
  moved.
- `corpus/opc_check.py` over the gate's own output — all 34 packages it wrote, kept with `--keep`:
  **34 × `ok`**. The two checks are complementary and both are green.
- The **LibreOffice render was not run**: the installation on this machine is broken
  (`soffice.bin: error while loading shared libraries: libreglo.so`), as it has been since 15-D.
  That is 15-J's problem, and 15-J is the phase's second gate.

## Next

15-J, done in [[71-phase-15-render]]: CI gate 2 — headless LibreOffice renders the corpus to PDF and
`pdftoppm` rasterises it, a perceptual diff over a measured threshold of 2.5 fails the build naming
the deck, the page and the score, and a renderer that is missing *or broken* is a skip with a banner
and exit 3, a failure in CI. Like this gate it refuses to measure a byte-identical round trip and
says so; unlike it, it has never run against a real render on this machine.
