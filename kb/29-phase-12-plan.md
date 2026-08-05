---
created_at: 2026-08-05T13:07:49.253020866Z
updated_at: 2026-08-05T13:08:31.091386236Z
tags:
    - plan
    - phase-12
    - pptx
    - presentations
    - spike
---
# 29 — Phase 12 plan: presentation spikes

Implementation plan for **Phase 12** of [`docs/development-plan-v2.md`](../docs/development-plan-v2.md),
the first phase of the presentations track, opened after Phase 11 closed in [[28-phase-11-mcp]].
Read [[11-plan-v2-onramp]] first: it lists what transfers from Phases 0–7 and must not be rebuilt.

Objective restated: **kill three unknowns before a single line of the pptx reader is written.**
The output of this phase is decision documents and a corpus, not a reader. Phase 13 is where
`Document::Presentation` gets built, and per `AGENTS.md` §7 rule 1 none of it happens here.

## Execution order — deliberately not the plan's numbering

The plan lists the corpus bootstrap as task 4. It runs **first**. P1 measures fidelity "over a
real corpus", P2 drafts DocMark-P "over 5 presentations" and P3 re-injects slides into "a
package": all three consume decks, and none of them can start without one. Building the corpus
last would mean measuring the three spikes against fixtures invented ad hoc inside each spike,
which is how three spikes end up disagreeing about what a deck is.

| # | Increment | Depends on |
|---|---|---|
| 12-A | `corpus/pptx/` bootstrap in `corpus/generate.py` | — |
| 12-B | Spike P1 — `ooxmlsdk` vs custom parser → `docs/spikes/P1-pptx-strategy.md` | 12-A |
| 12-C | Spike P2 — DocMark-P draft → `docs/spikes/P2-docmark-p.md` | 12-A |
| 12-D | Spike P3 — preserved skeleton → `docs/spikes/P3-preserved-skeleton.md` | 12-A |

12-B, 12-C and 12-D are independent of each other once 12-A lands.

## Deviations from the written plan, and why

Two, both forced by facts on the ground rather than preference. Recorded here because
`AGENTS.md` MANDATORY 3 requires diverging from a plan to be deliberate and visible.

### 1. The pptx corpus is built by hand, not with `python-pptx`

The plan says "via `corpus/generate.py` (`python-pptx`)". `corpus/generate.py` states its own
contrary rule in its module docstring — *"Media are synthesised in pure Python (no Pillow). That
keeps the generator dependency-free on the three CI platforms"* — and every existing docx, xlsx,
odt and ods fixture is assembled from hand-written XML through `write_package`. Adding
`python-pptx` would make `python3 corpus/generate.py --check`, which CI runs on
ubuntu/windows/macos, depend on a pip install that is not there today.

It would also lose the property the corpus is built around: the XML is reviewable **as source**,
and a failing golden points at a line a human wrote. `python-pptx` emits its own XML, so a
fixture's content would no longer be visible in the generator.

Decision: `build_pptx()` alongside `build_docx()`, same `write_package`, same fixed ZIP
timestamp, same one-trait-per-file discipline. The plan's intent — a reproducible generated
corpus — is met; only the library it named is dropped.

### 2. P3 can only be verified on LibreOffice in this environment

P3's acceptance criterion is "no-repair round-trip on at least 3 real decks in **both PowerPoint
and LibreOffice**". LibreOffice is present (`/usr/bin/soffice`) and that half is executable and
will be executed. PowerPoint is not, and no amount of care substitutes for it: "PowerPoint offers
to repair this file" is precisely the failure that only PowerPoint reports.

Decision: run and record the LibreOffice half, and leave the PowerPoint half as an **explicitly
open acceptance criterion** with the exact reproduction steps written into the spike, so whoever
has PowerPoint can close it in minutes. It is not marked green. Risk P2 (§7 of
`docs/technical-analysis-presentations.md`) stays open until it is.

"Real decks" is a second problem with the same shape: `AGENTS.md` §6 forbids corpus documents
with real or private data, so P3 runs against the 12-A corpus plus, where a trait needs it,
decks built to be adversarial. A generated deck that carries SmartArt, animations and an OLE
object exercises the skeleton exactly as a corporate one does.

## 12-A — the pptx corpus

One trait per file, mirroring `corpus/docx`. Proposed fixtures, each chosen because a later
phase has a specific question about it:

| Fixture | Trait it isolates |
|---|---|
| `basic-slides.pptx` | Title + content placeholders, two slides, nothing else |
| `slide-order.pptx` | `p:sldIdLst` order deliberately disagreeing with the file names |
| `placeholders-cascade.pptx` | A placeholder inheriting everything from layout/master (must emit zero attributes) |
| `placeholders-empty.pptx` | An empty placeholder — real content, the pptx echo of the Phase 1 empty-paragraph bug |
| `bullets-levels.pptx` | `a:pPr@lvl` nesting, `buChar` and `buAutoNum` |
| `notes-speaker.pptx` | `ppt/notesSlides/*` — the highest-value part for an agent |
| `tables-simple.pptx` | `p:graphicFrame` → `a:tbl` |
| `images-anchored.pptx` | DrawingML picture with explicit `a:xfrm` geometry |
| `shapes-geometry.pptx` | `prstGeom` free shapes → the raw-block stub decision |
| `autofit-stale.pptx` | `a:normAutofit@fontScale` — the trap behind risk P5 |
| `charts-embedded.pptx` | `c:chart` + its embedded xlsx workbook in `ppt/embeddings/` |
| `smartart-fallback.pptx` | `dgm:*` + `mc:AlternateContent` — the raw-block case, and skeleton input for P3 |

No golden files in this phase. Goldens describe a reader's output and there is no reader until
Phase 13; committing goldens now would be committing fiction. 12-A's test is
`generate.py --check` being reproducible, nothing more.

## 12-B — spike P1

Timeboxed to one week, as R1 was. Same shape as
[`docs/spikes/R1-docx-strategy.md`](../docs/spikes/R1-docx-strategy.md): method, results,
analysis, signed decision, reproduction steps.

Metrics, all four measured rather than argued:

1. **Fidelity** over `corpus/pptx` — what `ooxmlsdk` resolves and, more importantly, what it
   loses. R1's finding for `docx-rs` was that unknown elements vanish; the same question is the
   load-bearing one here.
2. **Binary size delta** and **compile time delta** — risk P6. `ooxmlsdk` is generated code
   large enough to need a raised `recursion_limit`.
3. **Behaviour on corrupt input** — truncation and byte flips, counting panics. `AGENTS.md` §6:
   parsers must return `Err`, never `panic!`.

The expected answer (analysis §3) is *custom parser*. The spike exists to make that evidence, not
expectation — exactly the role R1 played. A spike that can only confirm what it assumed is not a
spike. Measurements are taken on the CI toolchain (stable), noted where the local toolchain
differs.

## 12-C — spike P2

The DocMark-P draft, judged against **risk P4**: does it stay Markdown, or does it become XML
wearing `:::` fences? The verdict is a readability one and has to be stated as a verdict, not
implied by an example.

The rule from analysis §5.2 is the test: *a plain Markdown viewer must show title + bullets +
images per slide, and nothing else*. If `--fidelity standard` output is not hand-editable by a
human, the design failed and the draft says so.

## 12-D — spike P3

Proof of concept, not production code: read a package, keep every non-slide part opaquely, write
the slides back into it, and open the result. The mechanism is the raw-block idea lifted to
package level (analysis §5.1) and it is what decides whether risk P2 is survivable.

Where the proof lives is itself a decision the spike has to take: a throwaway script proves
nothing that survives, and a crate is Phase 13 work. Expect a test-only harness under the spike's
reproduction steps.

## Acceptance criteria (tracked here)

- [ ] Three spike documents under `docs/spikes/`, each with an explicit signed decision and its
      measurements.
- [ ] `corpus/generate.py --check` green with `corpus/pptx/` present, on all three CI platforms.
- [ ] P3 proves no-repair round-trip on ≥ 3 decks in **LibreOffice**.
- [ ] P3 in **PowerPoint** — open, blocked on the environment; steps documented.

## Rules that bind this phase

- **No Phase 13+ work.** No `Document::Presentation`, no reader, no DocMark-P serializer. A spike
  that grows into an implementation has stopped being a spike (`AGENTS.md` §7 rule 1).
- No golden files for pptx yet — see 12-A.
- Any dependency the spike adds is a *spike* dependency and does not enter the workspace
  `Cargo.toml` without the written justification `AGENTS.md` §7 rule 4 requires.
- Every increment recorded in the KB when done (MANDATORY 5), linked back to this plan.
