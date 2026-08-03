# Development plan v2 — agent-native docsai + presentations

**Supersedes** [`development-plan.md`](development-plan.md) (plan v1, Phases 0–9), which is
**delivered and deprecated**: Phases 0–7 are closed in tree, and the open items of Phases 8–9
are absorbed here (Phase 20). Plan v1 stays in the repository as the historical record of how
the current code came to be; it is no longer the source of truth for what to build next.

Rationale for a second plan instead of more phases appended to the first: v1 optimised a single
axis, **fidelity**. The product is now used by AI agents through MCP, and the binding constraint
has moved to a second axis, **context cost per task** — plus a document class (presentations)
that v1 explicitly deferred to the post-1.0 backlog. Those are different design pressures, and
they change the IR, the DocMark specification and the tool surface. See
[`technical-analysis-presentations.md`](technical-analysis-presentations.md) for the analysis
behind every decision referenced below.

Same management rules as v1: a phase is not opened until the previous one's acceptance criteria
are closed (except those marked parallelizable), each phase ends with a `v0.x.0` tag, estimates
are person-weeks for one senior Rust developer.

## Tracks and ordering

Two tracks, deliberately sequenced **agent-core first**:

| Track | Phases | Why this order |
|---|---|---|
| **A — agent-native core** | 10, 11 | Low/medium effort, high return, applies to the formats already supported, and defines the primitives (ids, projections, sidecar raws, token budget) that presentations then get for free |
| **P — presentations** | 12–16, 18, 19 | The big new format family; its DocMark profile is designed *after* the addressing primitives exist, so it never has to be retrofitted |
| **A2 — editing surface** | 17 | Depends on both: patch operations are defined against the IR, and every one must round-trip. Opened only once the pptx reader/writer are closed |
| **Close** | 20 | Hardening carried over from v1 Phase 8/9 + v2.0 release |

Track A ships value on `.docx`/`.xlsx`/`.odt`/`.ods` before a single line of PresentationML is
written. If presentations were dropped tomorrow, Phases 10–11 and 17 would still be worth doing.

---

## Phase 10 — Stable addressing and token budget (3–4 weeks)

**Objective**: every addressable node in a DocMark document has a persistent id and an etag, and
the cost of any document is a measured number, not an estimate. This is the enabling phase;
nothing else in track A works without it.

Tasks:
1. `docsai-model`: `NodeId` newtype and an `addressable` trait/derive over the IR. Ids on:
   section, heading, paragraph *only when it is a list root or a container*, list, table, row,
   image, footnote, sheet, raw block. Not on runs (addressed by relative path `id:N`).
2. Id allocation policy: **monotonic counter in the front matter (`next-id`)**, never renumbered
   on insertion, never reused after deletion. Ids assigned on read if absent, preserved verbatim
   if present. Documented in the spec addendum as normative.
3. Per-node **etag**: 6-char hash of the normalised node content (same normalisation as the
   round-trip structural diff, so formatting-only changes do not churn etags).
4. DocMark 1.1 emission: `{#s4.b2}` on addressable nodes, `next-id` and `docmark: "1.1"` in the
   front matter; parser accepts and preserves ids and etags. `--ids never|preserve|assign`
   (default `assign` for `full`/`agent`, `never` for `plain`).
5. `docsai tokens <in> [--fidelity …] [--json]`: real tokenizer, per-node and per-document cost.
   Vendored tokenizer decided in this phase (pure-Rust BPE, no network, no Python).
6. CI gate: corpus token report committed as a golden; a PR inflating total corpus tokens by
   > 5 % fails and must justify it in the diff.
7. `docsai outline <in> [--depth N] [--json]`: id tree with node type, ~60-character preview and
   token cost per node.

Deliverables: DocMark 1.1 (ids + etags), `tokens`, `outline`, token CI gate.

Acceptance criteria:
- [ ] Property test: ids survive N=10 round trips unchanged, are never reused, and never
      collide across insert/delete sequences (risk P7).
- [ ] Etag changes iff normalised node content changes (test both directions).
- [ ] `outline` of the largest corpus document is < 5 % of the tokens of the document itself.
- [ ] Corpus token report generated in CI and diffable.
- [ ] Documents written against DocMark 1.0 (no ids) still parse; ids are added on next write.

## Phase 11 — Projections, raw sidecar and `--fidelity agent` (3–4 weeks)

**Objective**: an agent can locate and read *part* of a document, and never pays for content it
cannot edit.

Tasks:
1. **Raw blocks to sidecar**: `assets/_raw/<id>.xml` (or a single `<doc>.raw.json`), referenced
   in the body as `::: {.raw format=ooxml id=r7}` with no inline payload. `--raw
   inline|sidecar` (`sidecar` default for `full` and `agent`). Writer re-injects from the
   sidecar; a missing sidecar is a typed error, never silent loss.
2. **`--fidelity agent`** (fourth level): editable content as text; everything else collapsed to
   a one-line stub with id + etag. Requires the sidecar (1) and, for pptx later, the skeleton.
3. **Delta emission against inheritance**, generalised: no attribute is serialized when it is
   already implied by the resolved style/inheritance chain. Applies to the existing docx/odt/
   xlsx paths now; the pptx cascade inherits the mechanism in Phase 13.
4. **Attribute-set dictionary**: patterns repeating ≥ N times interned into the front matter and
   referenced by class (`{.g1}`). Deterministic naming, part of serializer idempotence.
5. **Readable units with tolerance**: geometry serialized in pt/cm/in with configurable
   precision; round-trip comparison uses a documented tolerance instead of byte equality (EMU
   remains available as the exact escape hatch, spec §2).
6. `docsai read <in> --select <selector>`: `s4`, `s7-s9`, `#id`, `type:notes`, `text:foo`.
   Output is **valid self-contained DocMark**, with the minimum front matter needed to parse and
   re-write it.
7. `docsai search <in> <query> [--json]`: returns ids + surrounding context, not the document.
8. MCP: expose `outline`, `read_selection`, `search_document`; `include_images=none|refs|
   thumbnails|full` with `refs` as the new default (documented breaking change of the MCP
   default, with a migration note in CHANGELOG).

Acceptance criteria:
- [ ] Round-trip identity preserved with sidecar raw blocks on the whole existing corpus.
- [ ] `--fidelity agent` on the biggest corpus documents: ≥ 60 % token reduction vs `full`, and
      a write from the `agent` output reproduces the same file as a write from `full` for every
      node not touched.
- [ ] `read --select` output re-parses standalone, and re-writing it produces no warnings other
      than the documented "partial document" one.
- [ ] Delta emission does not change any golden's semantics (goldens updated by hand, diff
      reviewed, fidelity metric unchanged or better).

## Phase 12 — Presentation spikes (2–3 weeks)

**Objective**: kill the three unknowns before writing the reader. Output is three decision
documents under `docs/spikes/`, in the style of R1/R3.

Tasks:
1. **Spike P1 — `ooxmlsdk` vs custom parser** (1 week, timeboxed). Objective metrics over a real
   corpus: fidelity, binary size delta, compile time delta, behaviour on corrupt input. Written
   decision, signed, in `docs/spikes/P1-pptx-strategy.md`. Default expectation (analysis §3) is
   custom parser; the spike exists to make that decision evidence-based, as R1 did.
2. **Spike P2 — DocMark-P draft** over 5 real presentations of different nature (corporate with
   master, technical with code, sales with bleed images, a deck with SmartArt, a deck with
   charts). Deliverable: the profile draft + a readability verdict against risk P4.
3. **Spike P3 — preserved skeleton**: proof of concept that slides can be re-injected into the
   original package and PowerPoint + LibreOffice open the result with no repair dialog.
4. Presentation corpus bootstrap in `corpus/pptx/` via `corpus/generate.py` (`python-pptx`),
   one trait per file, mirroring the existing corpus discipline.

Acceptance criteria:
- [ ] Three spike documents with explicit decisions and measurements.
- [ ] P3 proves no-repair round-trip on at least 3 real decks in both PowerPoint and LibreOffice.
- [ ] Corpus generator produces the pptx corpus reproducibly (`generate.py --check` green).

## Phase 13 — PPTX reading → IR (5–6 weeks)

**Objective**: `Document::Presentation` in the IR, and a reader that fills it.

Tasks:
1. `docsai-model`: third root `Document::Presentation` — `Presentation { meta, styles,
   layouts: LayoutCatalog, slides: Vec<Slide>, skeleton: Option<SkeletonRef> }`;
   `Slide { id, layout_ref, shapes: Vec<Shape>, notes: Option<Vec<Block>>, transition_raw }`;
   `Shape` = placeholder | text box | picture | table | chart | group | raw stub, each with
   `ShapeGeometry` (reusing `Length`/EMU) and an optional `z_index`/original index.
2. `docsai-office::pptx` reader over `zip` + `quick-xml`: OPC + `_rels` reuse, `p:sldIdLst`
   order, sections, slide/layout/master/theme parts.
3. **Cascade resolution as reference + delta**: layout/master/theme resolved for colour, font
   and size; anything unresolved stays as a layout reference. Never flatten (architecture IR
   principle).
4. Text: `a:p`/`a:r` → existing `Paragraph`/`Inline` types; bullet levels → `List`; `a:fld` →
   `Inline::Field`.
5. Images and media → existing `ImageRef`/`ImageGeometry` + `AssetStore` (no new model).
6. Tables (`a:tbl`) → IR `Table`. Notes (`ppt/notesSlides/*`) → `Slide::notes`.
7. **Deterministic reading order** (analysis §5.3): placeholders first by type, then remaining
   shapes by top-left; the original `spTree` index preserved as an attribute so the order is
   reversible.
8. Skeleton capture: non-slide package parts stored opaquely via `AssetStore` (content-hash
   dedupe already exists), referenced from the front matter.
9. Everything unmodelled (SmartArt, `p:timing`, `p:transition`, OLE, custGeom, connectors,
   groups beyond a stub) → raw block **in the sidecar** (Phase 11) with a visible stub.
10. `.pptm`: read as its macro-free equivalent with a security warning, exactly as `.docm`.
11. `inspect` extended with a **slide inventory**: layout used, shape count, has-notes,
    has-SmartArt/OLE — what an agent needs to decide where to edit without loading the deck.

Acceptance criteria:
- [ ] Corpus pptx goldens pass, including reading order, notes and placeholder identity.
- [ ] Zero panics on the synthetic corrupt corpus (truncated ZIP, malformed XML): always `Err`.
- [ ] A real 40-slide deck reads in < 1 s.
- [ ] `--fidelity agent` on that deck is ≤ 15 % of the `full` token count (analysis §6.5 target).
- [ ] Every unmodelled element produces a stub + sidecar raw + typed warning; none disappears.

## Phase 14 — DocMark-P serializer and parser (3–4 weeks)

**Objective**: presentations are readable, hand-editable Markdown, and parse back losslessly.

Tasks:
1. Spec addendum **DocMark 1.2 (presentation profile)**: slide container, `::: {.ph type=title
   idx=1}` placeholders, free shapes with explicit geometry, `::: {.notes}`, layout catalogue in
   the front matter, shape stubs, skeleton reference. Version bump documented per AGENTS.md §2.
2. Serializer: IR presentation → DocMark-P, deterministic, delta-emitting (Phase 11.3), ids and
   etags (Phase 10).
3. Parser: the exact mirror; useful line-level errors.
4. Degradation rule enforced by test: in `plain`, each slide renders as heading + bullets +
   images and nothing else, and the result is clean CommonMark verified with comrak.
5. Goldens for the whole pptx corpus; serializer idempotence (`serialize(parse(md)) == md`) in
   CI, as for docx.

Acceptance criteria:
- [ ] `serialize(parse(md)) == md` byte-for-byte on every pptx golden.
- [ ] Risk P4 gate: a reviewer hand-edits three `--fidelity standard` decks (retitle a slide,
      add a bullet, swap an image) without consulting the spec; failures block the phase.
- [ ] `plain` output of a deck is valid CommonMark and readable as a document.

## Phase 15 — PPTX writing and the anti-repair gate (4–5 weeks)

**Objective**: close the presentation cycle, with the failure mode that matters actually tested.

Tasks:
1. `docsai-office::pptx` writer: slides regenerated from the IR and **re-injected into the
   preserved skeleton**; raw blocks re-injected from the sidecar; media from `AssetStore`
   without recompression.
2. Generation without a skeleton: embedded default template + automatic layout selection from
   content shape (title + bullets → "Title and Content", title only → "Title Slide") — this is
   the "tolerant input" requirement of analysis §6.6, needed for agent-authored decks.
3. `Warning::AutofitStale`: stale `fontScale`/`lnSpcReduction` dropped, warning emitted (§5.4).
4. `roundtrip` support for presentations with the per-category fidelity metric.
5. **CI gate 1 — schema validation**: generated packages validated against the ECMA-376 XSDs.
6. **CI gate 2 — render diff**: headless LibreOffice renders the corpus to PNG before and after
   the round trip; perceptual diff over a threshold fails the build.
7. Manual checklist per release: PowerPoint and LibreOffice open every corpus output with no
   repair dialog (risk P2).

Acceptance criteria:
- [ ] pptx round-trip idempotent (2nd pass == 1st pass) over the whole pptx corpus.
- [ ] Both CI gates green on the corpus.
- [ ] No repair dialog in PowerPoint or LibreOffice for any corpus output (checklist signed).
- [ ] A deck written from *attribute-free* Markdown opens correctly with a sensible layout.
- [ ] Images rewritten without recompression; geometry compared with the documented tolerance.

## Phase 16 — Charts, embedded objects, thumbnails (3–4 weeks)

**Objective**: the content classes that survive as data rather than as opaque XML.

Tasks:
1. Charts: `c:chart` + the embedded workbook in `ppt/embeddings/` read **with the existing xlsx
   reader** into an IR chart node (series + categories as a table + a raw block for the chart
   XML). Write path restores both parts.
2. SmartArt, OLE, animations, transitions: sidecar raw + stub, confirmed end to end.
3. Comments (`ppt/comments/*`) as a DocMark extension, sharing syntax with the docx comments
   item carried over from the v1 backlog.
4. **Slide thumbnails**: `--thumbnails` / MCP `include_images=thumbnails` renders each slide to
   PNG via headless LibreOffice (skipped with a warning when absent). For a vision model this is
   the cheapest useful representation of a slide (analysis §6.6).
5. Content-hash LRU cache in `docsai-convert` so repeated MCP calls on the same deck do not
   re-parse it (pure optimisation; protocol stays stateless).

Acceptance criteria:
- [ ] Chart data readable as a table and editable through DocMark; chart survives round-trip.
- [ ] Thumbnails generated for the corpus when LibreOffice is present; clean warning when not.
- [ ] Cache demonstrably removes re-parsing in a repeated-call benchmark, and disabling it
      changes no output.

## Phase 17 — `apply_edits`: patch editing (4–5 weeks)

**Objective**: the change that moves docsai from converter to document tool. Opened only after
Phase 15, because every operation must be defined against the IR and proven in round-trip.

Tasks:
1. `docsai-convert::edit`: closed, versioned operation set — `replace_text`, `set_props`,
   `insert_after`/`insert_before`, `delete`, `move`, `duplicate_slide`, `set_cell`,
   `set_notes`, `replace_image`. Anything outside the set is done with `read --select` + rewrite.
2. Targeting by id (`#s4.b2`) or relative path, with **etag preconditions**; a stale etag is a
   typed, loud failure, never an overwrite.
3. **Transactional semantics**: all-or-nothing, atomic write (temp file + rename), no partial
   packages on failure.
4. **`dry_run`** returning the change report without writing.
5. Response carries the applied diff **and the new etags**, so no confirmation read is needed.
6. `docsai edit <in> --ops <json>` CLI and `apply_edits` MCP tool sharing one implementation.
7. `validate_docmark` tool: typed errors with node id and suggested fix.
8. MCP resource: **contextual cheat-sheet** — compact DocMark syntax summary limited to the
   features the document at hand actually uses.

Acceptance criteria:
- [ ] Every operation has a round-trip test on all four document classes it applies to.
- [ ] Editing a node never perturbs a sibling node's serialization (byte-level test).
- [ ] Failure injected mid-transaction leaves the original file untouched.
- [ ] A scripted realistic agent loop ("retitle slide 4, add a bullet, bold it, verify") costs
      **≤ 3 tool calls and ≤ 2k tokens** on a 40-slide deck. This number is the phase's reason to
      exist; if it is not met, the phase is not done.

## Phase 18 — ODP ⇄ DocMark (3–4 weeks) — *parallelizable with Phase 19*

**Objective**: symmetry with `.odt`/`.ods`.

Tasks:
1. `docsai-odf::odp` reader/writer over `draw:page`, `draw:frame`, `presentation:class`, master
   pages; automatic-style de-automatization exactly as `docsai-odf` already does.
2. Same IR presentation root, same DocMark-P profile, same skeleton mechanism.
3. Corpus mirroring the pptx traits; cross conversion pptx⇄odp through the IR, with dialect raw
   blocks dropped with a warning (the documented v1 behaviour).

Acceptance criteria:
- [ ] odp round-trip with DocMark identity on the second pass over the odp corpus.
- [ ] pptx→DocMark→odp and back work for common traits; drops are warned, never silent.

## Phase 19 — Legacy `.ppt` reading (2–3 weeks) — *parallelizable with Phase 18*

**Objective**: the `.doc` strategy applied to `.ppt`, read-only. Writing `.ppt` stays out of
scope.

Tasks:
1. LibreOffice headless route: `soffice --convert-to pptx` in a sandboxed temp directory, then
   the Phase 13 pipeline. Reuses the existing detection and `--use-loffice auto|never|require`.
2. Degraded native route: `cfb` + `TextCharsAtom`/`TextBytesAtom` for text, `Pictures` stream
   for BLIPs. Evaluate `office_oxide` first (analysis §4) — adopting it must be justified in the
   PR under AGENTS.md §7.4.
3. Slide-boundary reconstruction in the degraded route; degraded status recorded in
   `ConversionReport`; clear messaging about what is lost without LibreOffice.
4. Corpus under `corpus/ppt/`, including a truncated and an encrypted sample.

Acceptance criteria:
- [ ] With LibreOffice: fidelity equivalent to the pptx path.
- [ ] Without it: full text with correct slide boundaries, no panics.
- [ ] Encrypted/protected `.ppt` rejected with a clear error.

## Phase 20 — Hardening and v2.0 (3–4 weeks, partially continuous)

**Objective**: absorb what plan v1 Phases 8–9 left open, extended to the new surface, and ship.

Carried over from v1 (still open):
1. `cargo-fuzz` on the input parsers — now six (docx, xlsx, odf, docmark, pptx, ppt) — seeded
   from the corpus, weekly scheduled CI. Target: 72 h accumulated with no crash.
2. Adversarial suite: ZIP bombs, entity expansion, path traversal in media names, extreme sizes;
   extended with **oversized skeletons and sidecar raw-block bombs**, new in v2.
3. `criterion` benchmarks + performance budget in CI (> 20 % regression blocks).
4. `cargo audit` / `cargo deny`; `cargo bloat` reporting.
5. Published **fidelity matrix** per trait, now including presentations.
6. Security review of the MCP surface, extended to `apply_edits`: writes confined to the
   indicated directories, path normalisation, macros never executed.
7. Close the v1 leftovers: proptest IR→md→IR identity (v1 Phase 2.5), 100k-cell performance
   budget (v1 Phase 3), first-release installer verification on clean machines (v1 Phase 6).

New in v2:
8. **Token budget matrix** published alongside the fidelity matrix: tokens per corpus document
   per fidelity level, tracked release over release.
9. Freeze **DocMark 1.2** and the `apply_edits` operation set as versioned contracts; migration
   notes for 1.0 → 1.2.
10. v2.0 release: CHANGELOG, README, announcement.

Acceptance criteria:
- [ ] 72 h accumulated fuzzing, six parsers, no crashes.
- [ ] Adversarial suite green, including the two new v2 vectors.
- [ ] Fidelity matrix and token matrix published; fidelity ≥ 95 % OOXML, ≥ 90 % ODF, ≥ 90 %
      presentations.
- [ ] All v1 leftovers in item 7 closed or explicitly retired with a reason.

---

## Post-2.0 backlog (not committed)

- Real text measurement for autofit (`ttf-parser` + `rustybuzz`/`cosmic-text`), replacing the
  `AutofitStale` warning.
- OOXML ⇄ OpenFormula formula dialect translation (v1 risk R5, still open).
- WMF/EMF → PNG/SVG conversion.
- Track changes (`w:ins`/`w:del`) as a DocMark extension (CriticMarkup).
- Library mode: `docsai-convert` as a stable published crate + WASM bindings.
- `.doc`/`.xls`/`.ppt` writing via LibreOffice fallback if demand warrants it.
- Semantic search over the IR (embeddings) as an optional feature, if `search` proves
  insufficient in practice.

## Schedule summary (indicative, 1 senior developer)

| Phase | Duration | Cumulative |
|---|---|---|
| 10 Stable addressing + token budget | 3–4 wk | 4 |
| 11 Projections, sidecar, `agent` fidelity | 3–4 wk | 8 |
| 12 Presentation spikes | 2–3 wk | 11 |
| 13 PPTX reading | 5–6 wk | 17 |
| 14 DocMark-P | 3–4 wk | 21 |
| 15 PPTX writing + anti-repair gate | 4–5 wk | 26 |
| 16 Charts, embedded objects, thumbnails | 3–4 wk | 30 |
| 17 `apply_edits` | 4–5 wk | 35 |
| 18 ODP | 3–4 wk | 38* |
| 19 Legacy PPT | 2–3 wk | 38* (*parallel 18‖19 with 2 devs) |
| 20 Hardening + v2.0 | 3–4 wk | ~42 wk (~9.5 months; ~7 with 2 devs from Phase 13) |

**Minimum viable slice**: Phases 10 + 11 alone (7–8 weeks) deliver most of the agent-loop
benefit on the formats already supported, and do not commit the project to presentations.

## Project tracking metrics

Carried from v1:
- **Fidelity per category** (`roundtrip` on the corpus): ≥ 95 % OOXML, ≥ 90 % ODF, ≥ 90 %
  presentations.
- **Raw blocks per real corpus document**: downward trend per phase.
- **Test coverage** of library crates ≥ 80 %.
- **Performance**: budgets from `architecture.md` §8, extended with a presentation budget.

New in v2:
- **Tokens per corpus document per fidelity level** — measured with `docsai tokens`, gated in CI
  (Phase 10.6). "Token cost is not estimated, it is measured."
- **Tool calls per canonical agent task** — scripted scenarios (retitle a slide, add a row to a
  sheet, fix a typo in a report): target ≤ 3 calls after Phase 17.
- **Repair-dialog incidents**: must be zero for every release (risk P2).
