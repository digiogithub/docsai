---
created_at: 2026-08-05T15:06:50.134004001Z
updated_at: 2026-08-05T15:07:34.543015043Z
tags:
    - phase-12
    - spike
    - pptx
    - skeleton
    - decision
---
# 33 — Phase 12 D: spike P3, the preserved skeleton

Increment **12-D** of [[29-phase-12-plan]], the last one in Phase 12. Deliverable:
[`docs/spikes/P3-preserved-skeleton.md`](../docs/spikes/P3-preserved-skeleton.md). Follows
[[31-phase-12-spike-p1]] and [[32-phase-12-spike-p2]].

## Decision

**The preserved skeleton works and is the write strategy for Phase 14.** Read every ZIP member as
`(name, bytes, compression method, timestamp)`, rebuild only the slide parts from a model, write
everything back in the incoming order. Over 24 decks × (rewrite + mutating rewrite) = 48 packages:
**no part lost, added or reordered; every one converted in LibreOffice.**

The rule is stronger than the analysis §5.1 wording: not «preserve the non-slide parts» but
**preserve everything and treat rebuilding as the exception**.

## The finding that matters

**The skeleton protects the package, not the slide.** On decks written by a foreign producer the
harness silently dropped every run property inside the slides it rebuilt — `basic-slides` went
from 3 `a:latin` + 3 `sz=` + 3 `a:endParaRPr` to zero, slide bytes 3612 → 2645, text and
paragraph count identical. One fixture's rendering changed because of it (`bullets-levels`: two
blank lines and a space gone, purely from lost glyph metrics).

Rule that falls out, normative for Phase 14:

> A writer may only rebuild an element whose every child it can either model or carry verbatim.
> Anything else stays raw.

## Numbers worth keeping

| Measurement | Value |
|---|---|
| Corpus decks (12) round-tripped | byte-identical, slides included — but the corpus shares the harness's conventions |
| LibreOffice-produced decks (12) round-tripped | only slide parts differ; 11/12 render identical text |
| Mutating round trip (retitle + add a bullet) | 24/24 convert, retitle visible 24/24, added line 7/12 (five fixtures have no second text placeholder) |
| Naive control (drop unknown parts, leave the pointers) | LibreOffice opens all 12 **without a word**; `charts-embedded` loses 12 values, `smartart-fallback` loses 5 parts with *no* visible change |
| Cost | 20 MiB deck → 56 MiB peak RSS (**2.7×**), 0.03 s |
| Robustness | 5361 truncations/byte flips, **0 panics** (5027 `Err`, 334 parsed) |

## Why risk P2 stays open

LibreOffice was measured to be **too lenient to stand in for PowerPoint**: the naive control leaves
five dangling relationships and it converts happily. So the LibreOffice half is done and green, and
the PowerPoint half is open by environment, with ten-minute reproduction steps in the spike §6.2
(including: run `naive` and confirm PowerPoint *does* reject it — that is what proves the oracle is
stricter).

Also new: LibreOffice never draws SmartArt, it renders the `mc:Fallback` shape. It structurally
cannot detect diagram-part loss.

Second-order note for whoever runs LibreOffice again: the launcher on this machine does not
resolve its own libraries. Call `/usr/lib/libreoffice/program/soffice.bin` with
`LD_LIBRARY_PATH=/usr/lib/libreoffice/program` and `HOME` set, or every run dies with
`libreglo.so: cannot open shared object file`. The install is fine; the wrapper is not.

## Corpus

Added `charts-embedded.pptx` and `smartart-fallback.pptx` (both were in the 12-A table, deferred
until a spike needed them; a skeleton spike with nothing unintelligible in the package tests
nothing). SmartArt carries four `dgm:` parts plus a `dsp:` drawing reached through a
Microsoft-only relationship type, and an `mc:AlternateContent` with a plain-shape fallback; the
chart carries an **embedded xlsx**, i.e. a package inside a package. `corpus/generate.py` grew
`package_bytes()` for that, and `rels_part()` now accepts a full URL as the relationship type.
89 corpus files, `--check` green.

## Still not verified

- PowerPoint (the acceptance criterion the plan already flagged).
- Animations, transitions, OLE and video **as slide content** — the 20 MiB `.mp4` was a package
  part, not a `p:pic` with a media relationship, and nothing checked that a raw shape's `r:id`
  still resolves after a round trip.
- **Deleting a slide**: every run here rewrote slides in place. Removal means editing
  `p:sldIdLst`, the presentation rels and the content types — three parts currently declared
  opaque. Phase 14 has to open that hole on purpose.
- Real-world decks; `AGENTS.md` §6 rules them out, LibreOffice output is the stand-in.

## Phase 12 status

All three spike documents exist. Acceptance criteria: spikes ✅, `generate.py --check` ✅,
LibreOffice round trip ✅ (12 ≫ 3 required), PowerPoint ❌ open by environment.
