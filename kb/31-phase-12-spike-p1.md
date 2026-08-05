---
created_at: 2026-08-05T14:07:26.468370969Z
updated_at: 2026-08-05T14:07:26.468370969Z
tags:
    - phase-12
    - spike
    - pptx
    - ooxmlsdk
    - decision
---
# 31 — Phase 12 B: spike P1, `ooxmlsdk` vs a custom pptx parser

Increment 12-B of [[29-phase-12-plan]]. Deliverable:
[`docs/spikes/P1-pptx-strategy.md`](../docs/spikes/P1-pptx-strategy.md). Consumes the corpus
built in [[30-phase-12-pptx-corpus]]. Same shape as
[`docs/spikes/R1-docx-strategy.md`](../docs/spikes/R1-docx-strategy.md).

## Decision

**Custom parser on `zip` + `quick-xml`**, as `.docx`, `.xlsx` and ODF already are. `ooxmlsdk`
0.12.0 is not linked. It stays as the XML mapping reference (it is generated from ECMA-376) and
as a possible fallback for spike P3's package skeleton.

## What was measured, and what it says

Four metrics over the ten `corpus/pptx` fixtures, with a probe binary outside the tree.

| Metric | Result | Verdict |
|---|---|---|
| Fidelity | Every trait the corpus isolates comes back typed: `sldIdLst` order, placeholder type/idx, `lvl`, `buAutoNum`, notes, `a:tbl`, `r:embed` + `a:xfrm` + `descr`, `prstGeom`, `normAutofit`. SmartArt (`dgm:relIds`) modelled too | **For** `ooxmlsdk` |
| Corrupt input | 1396 truncations and byte flips → Ok 140, Err 1256, **PANIC 0** | **For** `ooxmlsdk` (`docx-rs` panicked on 23% in R1) |
| Retention | No part lost, media byte-identical, `mc:AlternateContent` preserved — but a foreign-namespace element is **dropped with no error and no warning** | **Against, decisively** |
| Size / build | +28.7 MB stripped (0.85 → 29.5 MB) and +311 s per clean release build (11.0 → 322.4 s), under the workspace `[profile.release]`. `docsai` is 12.3 MB today | **Against** |

Three of four favoured the library, two of those more than the analysis predicted. The decision
still went the other way, and the reasoning is the point:

1. Silent loss of unknown elements makes the raw-block contract unimplementable for `.pptx`.
   Coverage would be unmeasurable and `--fidelity full` would quietly be less than full. A
   complementary pass to find what it dropped means parsing every slide twice with two models —
   the R1 trap, at larger scale, because both trees would be complete rather than one patching
   the other.
2. Tripling a local CLI's binary and adding five minutes to each of three CI legs is a lot to pay
   for the mechanical half of the work — the half the corpus and goldens already make cheap.

## Two findings worth carrying forward

**Our own corpus was wrong.** `notes-speaker.pptx` would not open:
`expected NotesSlide, found p:notesSlide`. The library was right — ECMA-376 binds `CT_NotesSlide`
to the element **`p:notes`**; only the part name and the content type say `notesSlide`. Fixed in
`corpus/generate.py`, fixtures regenerated, `--check` green.

That is the **second** schema-invalid fixture this phase (the first was a double `a:pPr`, see
[[30-phase-12-pptx-corpus]]), and `corpus/opc_check.py` passed both: it checks structure, not
schema. P1's recommendation is that the pptx corpus needs a schema gate; what form it takes is
spike P3's call, since P3 is the increment that has to open files in a real renderer anyway.

**`ooxmlsdk`'s package round trip is evidence for P3, not against it.** Ten packages written
back with no part lost, no media touched and `mc:AlternateContent` intact says the
preserved-skeleton idea is sound at package level. One caveat recorded for P3:
`docProps/core.xml` came back with the `cp:` prefix replaced by a default namespace and its
children reordered — namespace-equivalent, unverified in PowerPoint.

## Risk status

- **P6** (`ooxmlsdk` inflates binary and compile time) — closed, confirmed, and retired by not
  linking it.
- **P1** (cascade costs more than docx's) — still open, Phase 13 work, but mildly encouraging:
  the cascade is fully addressable in a typed model, so the difficulty is resolving it as
  reference + delta, not reaching it.

## Still not verified

No fixture has been opened by PowerPoint or LibreOffice (LibreOffice is broken on this machine,
`libreglo.so` missing). `ooxmlsdk` parsing a package is not the same as PowerPoint opening it —
the `p:notes` bug is exactly the kind of defect only a real renderer or a schema validator finds,
and it took a library to find it, not `opc_check.py`. The corpus stays provisional until 12-D.
