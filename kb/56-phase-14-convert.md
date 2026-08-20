---
tags:
    - phase-14
    - docmark
    - presentations
    - cli
    - tokens
---
# 56 — Phase 14 J: the deck converts

Increment **14-J** of [[46-phase-14-plan]], after the goldens of [[55-phase-14-goldens]]. Phase 13
made a deck readable and then refused to write it: DocMark-P did not exist, the serializer would
have handed back an empty body, and a caller redirecting stdout to a file would have lost every
slide and been told it worked. The profile exists now, so the refusal goes.

## What changed

- `docsai-convert::pipeline`: the `write_document` refusal on a presentation is removed, and
  `write_skeleton_package` puts the preserved package where the front matter says it is.
- `docsai-convert::assets::DirAssetStore::relocate`: moves the file written for an asset.
- `docsai-docmark` re-exports `skeleton_path`, which the pipeline needs to agree on the name.
- `SUPPORT`: pptx reads as «Phase 14 read-only: converts to DocMark-P; writing a pptx package is
  Phase 15». `README.md` says the same.
- `docsai-convert/tests/deck_convert.rs` (new, 4 tests and one `#[ignore]`d criterion).

## Non-obvious decisions

1. **The pipeline writes the package, and moves it rather than copying it.** `DirAssetStore` writes
   every asset a reader puts in, under `img-<hash8>.<ext>`. For a deck that means the whole original
   `.pptx` lands in `assets/` under an image's name, while the front matter points at
   `_skeleton/deck-<hash>.pptx` — the same bytes twice, one copy of them referenced by nothing.
   `relocate` moves the file the store already wrote, so one asset stays one file and `written()`
   keeps reporting the truth.
2. **The name comes from the serializer, never from the pipeline.** `skeleton_path` is public for
   the same reason `raw::sidecar_path` is: the side that writes the reference and the side that
   writes the file cannot be allowed to drift. The pipeline takes the reference and uses its last
   segment.
3. **`standard` and `plain` leave no package behind.** They write no `skeleton:` (rule 6), so
   writing one would put an unreferenced copy of the original deck next to a document whose whole
   point is being readable. The test asserts the directory does not exist.
4. **`outline`, `tokens`, `search` and `read --select` needed no work.** All four go through
   `serialize_traced`, so a deck became addressable the moment the serializer did. `search` finds a
   slide as `s34 #n34 slide`, `read --select s2` returns a partial document with etags.
5. **The Phase 13 criterion is measured, fails, and is written down as failing.** See below. The
   alternative — quietly widening the target, or making `agent` thinner without deciding what the
   level means — is `AGENTS.md` §7 rule 2 with extra steps.

## The 15 % criterion: measured, not met

Plan v2's Phase 13 asked for *«`--fidelity agent` on that deck is ≤ 15 % of the `full` token
count»* (analysis §6.5). Over the seventeen decks:

| level | forty-slides | corpus |
|---|---|---|
| `full` | 2597 | 200–2597 |
| `agent` | 2602 (100 %) | 96–102 % |
| `standard` | 1368 (53 %) | ~50 % |
| `plain` | 840 (32 %) | ~40 % |

`agent` and `full` differ **by one line**, `fidelity: agent`. Two reasons, and neither is a bug in
this increment:

- What `agent` drops is formatting (`Fidelity::formatting`), and these decks carry almost none.
  Their `full` output is already ~65 tokens a slide; no projection reaches 15 % of a document that
  is already minimal. §6.5's number came from a real 60-slide deck at ~45k tokens.
- What §6.5 wanted collapsed — the geometry of shapes nobody edits by hand — is written at `agent`
  on purpose: `Fidelity::measurements` is `!(deck && Standard)`. Changing that changes what the
  level *means*, which is a specification decision with a version bump behind it, not a patch.

So the criterion lives in the suite as `agent_fidelity_is_at_most_fifteen_percent_of_full`, marked
`#[ignore]` with the reason in the attribute, and printing every deck's number when run with
`--ignored --nocapture`. It is red on purpose and cannot be forgotten.

## A wart left alone, on purpose

Converting to **stdout** (`-o -`) still writes the assets directory beside the *input*, because
`default_assets_dir` falls back to the input when there is no output file. For a docx that means an
image or two; for a deck it now means a copy of the whole package. The behaviour predates this
increment and is the same one that makes a stdout conversion's image references resolve at all, so
changing it is a question about `-o -` in general — not about presentations. Worth remembering
before running `convert deck.pptx -o -` inside `corpus/`.

## What is deliberately not written

- **No pptx writer** (Phase 15): `SUPPORT` still says `write: no`, and `convert deck.dmk.md -o
  deck.pptx` refuses exactly as before.
- **No change to what `agent` writes.** That is the decision above, and it is not this increment's
  to take.

## How it was verified

- `deck_convert.rs`: every corpus deck converts; every path the output names is a file that was
  written *and* is in `Outcome::assets_written`; no package left loose in `assets/`; the converted
  file parses back into a deck with its skeleton found, with nothing seeded — only the file and the
  directory beside it, which is all a recipient has; `standard` and `plain` leave no `_skeleton/`.
- `pipeline.rs`: `a_deck_reads_but_does_not_convert_yet` is replaced by
  `a_deck_converts_and_takes_its_package_with_it`.
- By hand: `convert`, `outline`, `tokens`, `search`, `read --select s2` and `formats` over the
  corpus decks.
- `cargo test --workspace --no-fail-fast`: green but for the pre-existing docx failure recorded in
  [[55-phase-14-goldens]]. `clippy -D warnings` clean; `fmt --check` clean over every file touched.

## Next

**14-K — the P4 gate and the phase close**: three `standard` decks hand-edited without consulting
the spec, recorded with what broke, then docs, README, `AGENTS.md` status and the acceptance
criteria.
