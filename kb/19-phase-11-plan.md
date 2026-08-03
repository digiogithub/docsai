# 19 — Phase 11 plan: projections, raw sidecar and `--fidelity agent`

Implementation plan for **Phase 11** of [`docs/development-plan-v2.md`](../docs/development-plan-v2.md),
on top of Phase 10 ([[13-phase-10-addressing-plan]], closed in [[18-phase-10-token-gate]]).

Objective restated: an agent can locate and read *part* of a document, and never pays for content
it cannot edit.

Phase 10 left the number that defines this phase: the corpus costs **24 851** tokens at `full`,
and on every small document the front matter — the style catalogue — is 47–85 % of it.
`corpus/token-budget.md` is the instrument that says whether this phase worked.

## Increments (each independently testable, in order)

| # | Increment | Scope |
|---|---|---|
| 11-A | Raw blocks to a sidecar | `--raw inline\|sidecar`; `assets/_raw/<id>.xml`; body carries `::: {.raw format=ooxml part=… id=r7 src=…}` with no payload; parser reads the sidecar back; a missing sidecar is a typed error. |
| 11-B | `--fidelity agent` | Fourth level: editable content as text, everything else a one-line stub with id (+ etag). Requires 11-A. |
| 11-C | Delta emission against inheritance | No attribute serialized when the resolved style chain already implies it. docx/odt/xlsx paths. |
| 11-D | Attribute-set dictionary | Attribute patterns repeating ≥ N times interned in the front matter, referenced by class (`{.g1}`). Deterministic naming, part of idempotence. |
| 11-E | Readable units with tolerance | pt/cm/in with configurable precision; round-trip compared with a documented tolerance, EMU kept as the exact escape hatch. |
| 11-F | `docsai read --select` | `s4`, `s7-s9`, `#id`, `type:notes`, `text:foo` → valid standalone DocMark with the minimum front matter. |
| 11-G | `docsai search` | ids + surrounding context, `--json`. |
| 11-H | MCP surface | `outline`, `read_selection`, `search_document`; `include_images=none\|refs\|thumbnails\|full` with `refs` as the new default (breaking change, CHANGELOG migration note). |

## Design decisions taken up front

1. **The sidecar is addressed, not embedded.** `src=` names a file under the asset directory;
   the writer re-injects from it. A raw block whose sidecar is missing is a **typed error**
   (`ConvertError`), never a silent drop — the raw block exists precisely to hold what nothing
   else can hold, so losing it quietly would be the one failure this project refuses.
2. **Sidecar is the default for `full` and `agent`, not for `standard`/`plain`**, which drop raw
   blocks entirely today and keep doing so.
3. **`agent` is a projection, not a lossy conversion.** Its output is not meant to be written back
   as a whole document; it is meant to be *read*, and to be written back **node by node** with the
   etag proving nothing else moved. That is what makes the acceptance criterion ("a write from the
   `agent` output reproduces the same file as a write from `full` for every node not touched")
   testable at all.
4. **Selectors are not ids** (settled in Phase 10): `s4` is positional, `#n7` is stable. `read
   --select` takes both, and says which it used.
5. **DocMark version**: 1.1 is still unreleased, introduced this cycle in Phase 10; the sidecar
   attribute and the dictionary classes extend 1.1 rather than opening a 1.2, which the plan
   reserves for the presentation profile (Phase 14). Every change documented in the spec changelog.
6. **The dictionary must not fight the delta emission.** 11-C runs first: interning patterns that
   delta emission would have deleted anyway would freeze noise into the front matter.

## Acceptance criteria (from the plan, tracked here)

- [ ] Round-trip identity preserved with sidecar raw blocks on the whole existing corpus.
- [ ] `--fidelity agent` on the biggest corpus documents: ≥ 60 % token reduction vs `full`, and a
      write from the `agent` output reproduces the same file as a write from `full` for every node
      not touched.
- [ ] `read --select` output re-parses standalone; re-writing it warns only the documented
      "partial document" warning.
- [ ] Delta emission does not change any golden's semantics (goldens updated by hand, diff
      reviewed, fidelity metric unchanged or better).

## Rules that bind this phase

- No Phase 12+ work (no pptx, no spikes) — `AGENTS.md` §7 rule 1.
- Spec change ⇒ documented version/changelog entry in `docs/docmark-specification.md`.
- Golden updates are deliberate and hand-reviewed (`DOCSAI_UPDATE_GOLDENS=1`); the token budget
  gate ([[18-phase-10-token-gate]]) now also guards them — this phase should move it **down**.
- No heavy dependency without a written justification in `docs/technical-analysis.md`. None is
  expected here: every increment is our own code.
