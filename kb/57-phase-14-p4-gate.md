---
tags:
    - phase-14
    - docmark
    - presentations
    - risk-p4
    - gate
---
# 57 — Phase 14 K: the P4 gate and the phase close

Increment **14-K** of [[46-phase-14-plan]], after the conversion path of [[56-phase-14-convert]].
This is the increment that closes Phase 14, and the gate it has to pass is not a number: risk **P4**
of `technical-analysis-presentations.md` §7 — *«DocMark-P becomes unreadable»* — is answered by a
reviewer hand-editing three `--fidelity standard` decks **without consulting the specification**,
and what breaks blocks the phase.

## The gate, as it was run

Three decks converted at `standard` into a directory of their own, then edited as text: the three
edits plan v2 names — retitle a slide, add a bullet, swap an image — plus everything else a person
does to a document while they are in there. The edits were deliberately naive: retype the heading
without noticing `{.slide}`, copy the line above to add a bullet, change the file name inside the
parentheses.

| Edit | Result |
|---|---|
| Retitle a slide, dropping `{.slide}` | Still a slide, still two slides, notes intact |
| Add a bullet by copying the line above | Arrives as a bullet, no other slide moves |
| Swap an image by renaming inside `(...)` | The picture holds the new file's bytes |
| Add a whole slide by typing `## Conclusiones` | Third slide, no marker needed |
| Delete a slide by deleting its block | Two become one |
| `**negrita**` inside a bullet | Read as emphasis |
| `#` instead of `##` for a slide | Still a slide |
| A nested bullet, two spaces in | Nested list |
| A `### Detalle` under a slide | Content of the body, kept on the round trip |
| A GFM table typed by hand | A table shape |
| A loose paragraph | A paragraph of the body |
| `## ` with nothing after it | An untitled slide |
| A `:: {.notes}` with the fence mistyped | Stays as visible text; the typo is on screen, not swallowed |
| Deleting the front matter entirely | Still a deck: `.slide` is enough |
| An image pointing at a file that is not there | Read, with the reference kept |
| **Add a note where the slide already had one** | **Broke — see below** |

## The defect the gate found

At `standard` the notes are a blockquote under the slide, so «add a note» looks like «type another
blockquote». The parser assigned `slide.notes` on every notes block it met, so the second one
**replaced** the first, and text a reader can see on screen disappeared without a warning — the
silent loss `AGENTS.md` §7 rule 3 exists to forbid, in the one edit a reviewer of a deck is most
likely to make.

Fixed in `deck_parser::add_notes`: a slide has one notes page, and more than one notes block under
it — two blockquotes, or a `::: {.notes}` and a blockquote — is that page with their blocks in
order. Written into the spec's «Reading a deck back» as a normative sentence, because a second
reader has to answer it the same way. Pinned by
`presentation_parse::two_notes_blocks_on_one_slide_are_one_notes_page` and by the gate test.

This is what a gate is worth: four increments of tests over what the *writer* writes never asked
what happens when a person writes it.

## A second defect, found while closing

Converting a deck at `standard` or `plain` left `assets/img-<hash>.bin` beside the document: the
whole original package, ten kilobytes of it, under an image's name, referenced by nothing. The
reader stores the package whatever the level asked for, and `DirAssetStore` writes what it is given;
14-J moved that file into `_skeleton/` at `full` and `agent` and said nothing about the levels that
name no package. `DirAssetStore::discard` now removes it — the bytes stay in the store because the
IR still names them — and the empty `assets/` goes with it. The 14-J test only looked for a
`_skeleton/` directory, which is why it passed; it now compares file contents against the original
package.

## Non-obvious decisions

1. **The gate is a human act, and the test file says so.** `p4_hand_edit.rs` cannot be the
   reviewer; what it can do is hold the reviewer's edits still so they never quietly stop working.
   Each test is the literal text substitution a person makes in an editor, applied to the real
   `standard` output and read back — a regression net under a verdict, not the verdict.
2. **The edits assume the reviewer never read the spec.** That is the risk being tested. An edit
   that only works because the editor knew about `{.slide}` would prove the opposite of what P4
   asks.
3. **«Changed what it touches and nothing else» is the assertion that matters.** Every gate test
   checks the untouched slide too: its title, its bullets, its notes. A format survives hand editing
   when an edit is local, and a test that only looks at the edited slide would miss the day it stops
   being.

## Phase 14, closed

| Increment | What it left |
|---|---|
| 14-A … 14-E | The 1.2 profile in the spec, the fidelity rules, the addressing walk |
| 14-F | The serializer: every kind of shape written |
| 14-G | `plain` proven by a residue probe, not asserted ([[53-phase-14-plain]]) |
| 14-H | The parser, tolerant input, `raw=` as the `.shape` discriminator ([[54-phase-14-parser]]) |
| 14-I | Goldens and byte idempotence over the corpus ([[55-phase-14-goldens]]) |
| 14-J | `convert`, `outline`, `tokens`, `search`, `read --select` over a deck ([[56-phase-14-convert]]) |
| 14-K | The P4 gate, one defect found and fixed |

Acceptance criteria: byte idempotence ✅, the P4 gate ✅, `plain` as CommonMark ✅, and the one
inherited from Phase 13 — `--fidelity agent` ≤ 15 % of `full` — **measured and not met** at 96–102 %,
recorded in [[56-phase-14-convert]] with why it is a decision about what a level means rather than a
bug to fix. It is the one thing Phase 14 leaves open, and it is open in writing.

## What Phase 15 inherits

- A `Presentation` that survives DocMark-P in both directions, with its skeleton beside it.
- `SkeletonRef::rebuilt_parts` empty after a parse: the front matter writes the path and nothing
  else. Phase 15 decides whether re-injection needs it written down.
- A pre-existing docx defect this phase did not touch: `docx_roundtrip_is_idempotent` on
  `corpus/docx/footnotes.docx`, from `88ff8e11` ([[55-phase-14-goldens]]).

## Next

**Phase 15** — the pptx writer and the anti-repair gate. Not started here on purpose (`AGENTS.md`
§7 rule 1).
