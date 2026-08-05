---
created_at: 2026-08-05T14:32:18.139866411Z
updated_at: 2026-08-05T14:32:18.139866411Z
tags:
    - phase-12
    - spike
    - pptx
    - docmark
    - decision
---
# 32 — Phase 12 C: spike P2, the DocMark-P draft

Increment 12-C of [[29-phase-12-plan]]. Deliverable:
[`docs/spikes/P2-docmark-p.md`](../docs/spikes/P2-docmark-p.md). Serialises by hand the corpus
built in [[30-phase-12-pptx-corpus]]. Independent of [[31-phase-12-spike-p1]].

## Decision

**DocMark-P stays Markdown, and placeholders are implicit.** The slide heading *is* the title
placeholder; the layout's primary body placeholder is written as ordinary Markdown blocks under
it; a `:::` container appears only for other placeholders, free shapes and notes. The sketch in
`docmark-specification.md` §11.2 is superseded on that point (it wrote every placeholder as a
container, and wrote the title twice).

## What was measured

Risk P4 asks a readability question, so the metric is **residue**: what a real plain Markdown
viewer (`comrak` 0.39, GFM only, no attribute or container extension) prints that is DocMark
syntax rather than document content — counted per character, because CommonMark lazy continuation
glues a `:::` fence to the paragraph under it. Tokens counted with the repository's own encoding
(`tiktoken-rs`, `o200k_base`).

Four hand-written variants over the corpus: A = containers at `full`, B = implicit at `full`,
S = B at `standard`, S2 = S with three refinements.

| Variant | Residue (5 shared fixtures) | Without the free-shape fixture | Tokens |
|---|---|---|---|
| A — containers, full | 67.2 % | 59.6 % | 899 |
| B — implicit, full | 61.9 % | 47.4 % | 719 |
| S — implicit, standard | 23.7 % | 10.3 % | 298 |
| S2 — recommended | 18.7 % | **2.7 %** | 287 |

Over the eight body-bearing fixtures at S2: **11.4 % residue, 2.6 % without `shapes-geometry`**,
475 tokens, and **five fixtures render with zero residue**. Adding a slide costs 18 typed
characters in the implicit form against 117 in the container form.

## The draft's rules, and where they came from

1. `##` heading with `.slide` **is** the title placeholder — no container repeats it.
2. The layout's primary body placeholder is implicit.
3. `layouts:` names, per layout, which index is title and which is body. This is what makes 1 and
   2 a catalogue lookup rather than a heuristic.
4. Other placeholders, free shapes and connectors keep their containers with a `raw=` reference.
5. Notes: `::: {.notes}` at `full`/`agent`, a **blockquote** at `standard` (native CommonMark, and
   PresentationML has no blockquote to collide with).
6. `standard` writes no ids, no geometry, no raw payload, no image size, no `[]{.empty}`.
7. Readable units at `full`/`agent`, `emu` as the exact fallback.
8. A shape with no Markdown representation is a **visible** stub at every level, `standard`
   included.

## Findings worth carrying forward

**Two of the three residue sources at `standard` are DocMark 1.0's, not the presentation
profile's**: attribute blocks print literally in a plain viewer (which is why `--ids never` is
already the default at lossy levels), and `[]{.empty}` prints as `[]`. Only `::: {.shape …}` is
the profile's own, and it is deliberate.

**Readable units buy less in pptx than in docx.** Word stores typography an author chose, in
points; PowerPoint stores positions an author *dragged*, and eight of the corpus geometry values
are exact in no human unit and fall back to `emu`. A second argument for keeping geometry out of
`standard`.

## Risk status

- **P4** (DocMark-P becomes unreadable) — **mitigated, with a number**: 2.6 % residue at
  `standard`.
- **P3** (freeform shapes) — unchanged, now with a price: 87 characters of stub for three objects.
- **P5** (autofit) — unchanged; confirmed that dropping `fontScale` costs nothing at `standard`.
- **P1** (the cascade) — untouched, and now *load-bearing*: rules 2 and 3 depend on resolving
  layout → master, so Phase 13's reader is what makes the implicit form legal.

## Still not verified

Everything was written by hand — the right method for a readability question, the wrong one for
determinism. No serializer exists, so spec §8 determinism over slides is unproven; the reverse
claim (a hand-written attribute-free deck produces a valid `.pptx`) is design intent, not a
measurement. Residue is measured against one viewer. Charts and SmartArt have no draft
representation yet: their fixtures are still unbuilt, and rule 4 is *expected* to cover them.
