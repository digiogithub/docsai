---
tags:
    - plan
    - phase-14
    - docmark
    - pptx
    - presentations
    - serializer
    - parser
---
# 46 — Phase 14 plan: DocMark-P, the presentation profile

Implementation plan for **Phase 14** of [`docs/development-plan-v2.md`](../docs/development-plan-v2.md),
opened after Phase 13 closed with a complete pptx reader and the `inspect` slide inventory
([[45-phase-13-inspect-inventory]]). The design this phase finalises is **not open**: spike P2
decided it and measured it ([[32-phase-12-spike-p2]], `docs/spikes/P2-docmark-p.md` §4). Read
[[11-plan-v2-onramp]] first: it lists what transfers from the earlier phases and must not be
rebuilt.

Objective restated: **a presentation is readable, hand-editable Markdown, and parses back
losslessly.** No pptx writer — that is Phase 15 and `AGENTS.md` §7 rule 1 keeps it out.

## What is already decided, and this phase must obey

- **Placeholders are implicit** (P2 rules 1–3). The `##` heading *is* the title placeholder; the
  layout's primary body placeholder is written as ordinary blocks under it. `layouts:` in the
  front matter says which index is which, so the implicit form is a catalogue lookup, never a
  heuristic. The sketch in `docmark-specification.md` §11.2 is superseded on that point.
- **Containers only where Markdown has no shape**: every other placeholder (`::: {.ph idx=…}`),
  free shapes (`::: {.shape geom=…}`), connectors, notes.
- **`standard` is the readability contract**: no ids, no geometry, no raw payload, no image size,
  no `[]{.empty}`, no layout catalogue, and notes as a blockquote. Residue measured at 2.6 % over
  the fixtures that are documents, 11.4 % counting the one that is a diagram.
- **A shape with no Markdown representation is a visible stub at every level**, `standard`
  included — the alternative is silent loss (`AGENTS.md` §7 rule 3).
- **The reader's model is the input, unchanged.** Phase 13 stores deltas against the cascade
  ([[38-phase-13-cascade]]) and keeps the package in `Presentation::skeleton`
  ([[42-phase-13-skeleton]]); the serializer writes what the IR holds and invents nothing.

## Increments

Each is independently testable and ends green on `cargo test --workspace`, `clippy -D warnings`
and `fmt --check`.

### 14-A — the spec stops being a sketch

`docs/docmark-specification.md`: §11.2 becomes a **normative** section for DocMark 1.2, folding in
the eight rules of P2 §4 and the compatibility contract. `DOCMARK_VERSION_PRESENTATION = "1.2"` in
`docsai-model`, and the front-matter parser accepts 1.2 explicitly rather than by the «starts with
1» fallback. No serializer yet: this increment is the contract the next ten are written against.

### 14-B — the deck's front matter

`layouts:` (name, master, title index, body index — the P2 rule 3 catalogue), `skeleton:`, and the
version rule: a presentation declares 1.2, with or without ids. Written at `full`/`agent`, absent
at `standard` (rule 6). Parsed back into `LayoutCatalog` + `SkeletonRef`.

### 14-C — the slide, its heading and the implicit body

`## Title {#n1 .slide layout=L1}` per slide, the title placeholder consumed by the heading, the
primary body placeholder written as ordinary blocks under it. The titleless slide degrades to an
empty heading (P2 §4, accepted risk). Sections from `p14:sectionLst` travel here too.

### 14-D — the other placeholders, free shapes and connectors

`::: {.ph idx=…}`, `::: {.shape geom=… name=… pos=… size=…}`, `::: {.connector …}`, geometry in
readable units with `emu` as the exact fallback (spec §2), `raw=` pointing at the sidecar stub.
Index at `full`/`agent` only.

### 14-E — notes

`::: {.notes}` at `full`/`agent`; a **blockquote** at `standard` (P2 rule 5). The one place where
two fidelity levels emit different *syntax* for the same node, so the parser must read both.

### 14-F — pictures, tables and the shape stub

The image and table writers already exist; this wires them into a slide, with no image geometry at
`standard` (P2 §3.3). SmartArt, OLE, charts and custom geometry are visible stubs with their raw
reference, which is Phase 13's sidecar ([[43-phase-13-raw]]) read from the other side.

### 14-G — `plain`, and the degradation rule as a test

At `plain` a slide is heading + bullets + images and nothing else, and the result is clean
CommonMark verified with `comrak` — the spec's own test, run by CI rather than by a reviewer. The
P2 residue probe becomes a repository test at this point, not a scratchpad binary.

### 14-H — the parser

The exact mirror: front matter, `.slide` headings, the four container classes, the blockquote
notes, useful line-level errors. A deck with *no* attributes at all parses into a valid
`Presentation` — the tolerant-input claim of analysis §6.6, still unmeasured after P2.

### 14-I — goldens and idempotence

`<name>.expected.md` beside every pptx fixture, at the levels the docx goldens use, and
`serialize(parse(md)) == md` byte for byte over all of them. The `inspect` IR goldens of 13-K stay:
they check a different thing.

### 14-J — the deck converts

Lift the `write_document` refusal added in 13-K, `SUPPORT` says `pptx: read yes, write no` still
(writing a *package* is Phase 15) but `convert deck.pptx -o deck.md` works. `outline`, `tokens`,
`search` and `read --select` over a deck follow from the serializer, and this is where the deferred
Phase 13 criterion is finally measurable: `--fidelity agent` ≤ 15 % of the `full` token count.

### 14-K — the P4 gate and the phase close

The hand-edit review of three `standard` decks (retitle a slide, add a bullet, swap an image)
without consulting the spec, recorded with what broke. Docs, CHANGELOG, README, `AGENTS.md`
status, and the acceptance criteria checked here.

## Acceptance criteria (tracked here)

- [x] `serialize(parse(md)) == md` byte-for-byte on every pptx golden ([[55-phase-14-goldens]]).
- [x] Risk P4 gate: a reviewer hand-edits three `--fidelity standard` decks without consulting the
      spec; failures block the phase. Passed, with one defect found and fixed
      ([[57-phase-14-p4-gate]]).
- [x] `plain` output of a deck is valid CommonMark and readable as a document ([[53-phase-14-plain]]).
- [ ] **Inherited from Phase 13, deferred with a reason**: `--fidelity agent` on a deck is ≤ 15 %
      of the `full` token count. It could not be measured without this phase's serializer
      ([[34-phase-13-plan]]), and 14-J is where it becomes a number. **Measured and not met**:
      96–102 % over the corpus ([[56-phase-14-convert]]). The gap is what a *level means*, not a
      bug in the pipeline, so closing it is a decision to take rather than a fix to apply.

## Rules that bind this phase

- No Phase 15+ work: no pptx writer, no skeleton re-injection, no anti-repair gate.
- The spec is a contract (`AGENTS.md` §2 item 3): the version bump is documented and additive — a
  1.0/1.1 document keeps parsing.
- Nothing lost silently: stub + sidecar raw + typed warning, at every fidelity level.
- No new crate, no format crate importing another (§3).
- Every increment recorded in the KB when done (MANDATORY 5), linked back to this plan.
