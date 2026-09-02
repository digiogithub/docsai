---
tags:
    - plan
    - phase-16
    - pptx
    - charts
    - ole
    - comments
    - thumbnails
    - cache
---
# 75 — Phase 16 plan: charts as data, objects confirmed, thumbnails, cache

Implementation plan for **Phase 16** of [`docs/development-plan-v2.md`](../docs/development-plan-v2.md),
opened after Phase 15 closed the pptx writer and both CI gates ([[72-phase-15-close]]), plus the two
defects fixed after the close ([[73-phase-15-carried]], [[74-docx-bookmark-node-ids]]).

Objective restated: **the content classes that survive as data rather than as opaque XML.** A chart
today is a rule-8 stub over a preserved part — the deck round-trips byte for byte and an agent
reading it learns the word `barChart` and nothing else. This phase turns the parts whose meaning is
recoverable into IR, and *confirms* end to end the ones that stay opaque.

## The debt this phase opens on

`kb/72` says Phase 16 «should not start before the P2 form is signed: a chart data model built on
top of a package PowerPoint repairs would be built on sand.» The form
(`docs/spikes/P2-powerpoint-gate.md`) needs PowerPoint and a human and **is not signed**. The owner
directed the phase to start anyway. That is recorded here rather than in a commit message: risk P2
stays OPEN, and every increment below keeps the byte-identity property that makes the form
re-runnable at any time.

## What is already decided, and this phase must obey

- **The preserved skeleton is still the rule** ([[42-phase-13-skeleton]], [[62-phase-15-package]]).
  `SkeletonRef::rebuilt_parts` is the only licence to regenerate a part. Reading a chart *as data*
  does not move `ppt/charts/chart1.xml` out of the preserved set — the part keeps coming back byte
  for byte until an edit forces otherwise, and then it is spliced, never regenerated.
- **Nothing is written unless it differs from what the part already says** (15-B's rule, restated in
  [[64-phase-15-media]]). A chart nobody touched produces no edit, and the 17/17 byte-identity
  property of the corpus must still hold at every increment.
- **Never degrade silently, and never warn falsely** (AGENTS.md §7 rule 3). Both halves cost real
  work this phase: a chart whose series the model changed but whose *workbook* cannot be rewritten
  is a warning, and a chart whose series nobody touched must not produce one.
- **The specification already reserves the shape of the answer.** `docs/docmark-specification.md:801`
  defines `kind=` and `data=` on a `.chart` stub, and `:870` names Phase 16 as what fills in the
  series. This phase *implements* the spec; it does not rewrite it to fit the code (§7 rule 2). Only
  the «Not yet implemented» paragraph and the additions the spec does not yet cover (a chart's data
  table, comments) are editable, and each edit is argued in its increment.
- **No new crate** (§3), no heavy dependency without written justification (§7.4). The xlsx reader
  the chart's workbook needs is `docsai_office::xlsx::read` (`crates/docsai-office/src/xlsx/mod.rs:34`),
  already in the same crate.

## The chart's two copies of the truth

The fixture states the problem exactly. `corpus/pptx/charts-embedded.pptx` carries the series
**twice**:

- in `ppt/charts/chart1.xml` as `c:strCache` / `c:numCache` under `c:cat` and `c:val` — what the
  chart *displays*, present in every valid chart, and what PowerPoint draws without opening
  anything else;
- in `ppt/embeddings/workbook1.xlsx`, reached by the chart part's own `_rels`
  (`Type=…/relationships/package`) — where the numbers *came from*, and what a user edits when they
  choose «Edit Data».

The plan's task 1 says «read with the existing xlsx reader». That is the provenance half and this
phase does it, but the **cache is authoritative for the IR**: it is always present, it is what the
slide shows, and a chart with a linked or missing workbook still has to read. The workbook is read
when it is there, recorded as an asset (`data=`), and **compared**: a disagreement between the two
is a real property of the file and is warned, not averaged.

## Increments

Each is independently testable and ends green on `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` and
`python3 corpus/generate.py --check`. Each ends in a KB entry and one commit.

### 16-A — the chart says what it plots

`docsai-model`: `ChartRef` gains the series. `docsai-office::pptx::read_chart`
(`crates/docsai-office/src/pptx/mod.rs:685`) stops returning `title: None, workbook: None` and
fills all four fields: the title from `c:chart/c:title`, the kind it already derives, the workbook
captured into the `AssetStore` from `ppt/embeddings/…` through the chart part's relationships, and
the series read out of the caches. `raw` is unchanged — the markup is still preserved.
The `.expected.inspect.json` goldens move; no DocMark golden does yet.

### 16-B — the chart in DocMark

`docsai-docmark`: the `.chart` container's body stops being empty and carries the GFM table the
specification describes — categories down the first column, one column per series. `data=` written
when the workbook is present (the writer branch at `deck_writer.rs:288` already exists and has
never fired). The parser reads it back into the same `ChartRef`. Fidelity levels: the table at
`full`, `agent` and `standard`; at `plain` the chart is its title and a warning, as today.
`.expected.dmk.md` goldens move; the specification's «Not yet implemented» paragraph is rewritten
to describe what now exists.

### 16-C — the chart written back

`docsai-office::pptx::write`: a chart whose table the model did not touch produces **no edit at
all** and the deck stays byte-identical. A chart whose table changed has its caches spliced inside
the preserved `chart1.xml` — values, point counts and `c:f` ranges kept consistent — and its
workbook rewritten through the xlsx writer, or, when that cannot be done faithfully, preserved
with a warning naming the divergence. A chart with no skeleton (a deck generated from Markdown,
15-G's path) is written whole through `write_gen`, chart part, workbook and content types included.

### 16-D — the objects that stay opaque, confirmed end to end

Plan task 2. SmartArt, OLE, animations and transitions already read as stubs plus sidecar raw
([[43-phase-13-raw]], [[65-phase-15-raw]]) — what has never been proved is the *whole loop* on a
package that carries them for real. `corpus/pptx/` gains the `p:oleObj` fixture `kb/72` item 13
names as missing, and the phase adds the round-trip and byte-identity coverage for each kind.
Anything that fails here is a defect report, not a redesign.

### 16-E — comments read

Plan task 3. `ppt/comments/*` and `ppt/commentAuthors.xml` into the IR, and the docx comment that
`crates/docsai-office/src/docx/body.rs:517` warns is «out of scope in v1» read with **the same IR
node and the same DocMark syntax** — the plan says the two share it, so one syntax is designed
once. The DocMark extension is specified before it is written.

### 16-F — comments written back

The pptx writer restores or rebuilds the comment parts, and the docx writer writes what the docx
reader now reads. Byte-identity holds for a deck whose comments nobody touched.

### 16-G — slide thumbnails

Plan task 4. `--thumbnails` on the CLI and a *slide render* for MCP, via headless LibreOffice reusing
the detection at `crates/docsai-convert/src/loffice.rs:51` and the isolated-profile discipline of
`corpus/render_gate.py:219`. Skipped with a clean warning when LibreOffice is absent — and the
warning must not be false on a machine like this one, where `soffice` needs
`LD_LIBRARY_PATH=/usr/lib/libreoffice/program` ([[71-phase-15-render]]).
**Naming defect to settle here**: `ImagePolicy::Thumbnails` (`crates/docsai-mcp/src/images.rs:48`)
already exists and means «downscale the pictures the document embeds», which is not this. The two
must be distinguishable or one of them renamed.

### 16-H — the cache

Plan task 5. Content-hash LRU in `docsai-convert` so repeated MCP calls on one document do not
re-parse it. Pure optimisation: the protocol stays stateless, and **disabling the cache must change
no output** — that is the acceptance criterion and it is a test, not a claim. The hash is the FNV-1a
already in `crates/docsai-model/src/assets.rs:148`; no new dependency.

### 16-I — the phase closes

Acceptance criteria answered one by one with evidence, `docs/`, `CHANGELOG.md`, `AGENTS.md` §1 and
`kb/README.md` updated, and what is carried forward written down rather than discovered later — the
discipline of [[72-phase-15-close]].

## Acceptance criteria (from the plan, with what each will be measured by)

- [ ] Chart data readable as a table and editable through DocMark; chart survives round-trip.
- [ ] Thumbnails generated for the corpus when LibreOffice is present; clean warning when not.
- [ ] Cache demonstrably removes re-parsing in a repeated-call benchmark, and disabling it changes
      no output.

## Standing constraints for every increment

- Do not expand scope: increment N does not do N+1's work, and no Phase 17 (`apply_edits`) work.
- Do not change normative DocMark to make a test pass.
- Every loss is a typed warning; a false warning is the same failure as a missing one.
- English in code, comments, docs and commit messages; Conventional Commits with a crate scope.
- Never run a writing `git` command — this is a colocated jj repository (AGENTS.md §4.1). Use
  `jj status`, `jj diff`, `jj commit <paths>`.
- The owner's uncommitted files are never committed, reverted or overwritten:
  `.github/workflows/release.yml`, `.github/workflows/sign-macos.yml`, `dist-workspace.toml`,
  `docs/release-signing.md`.

## Next

16-A — the chart says what it plots.
