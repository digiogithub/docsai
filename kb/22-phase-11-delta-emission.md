# 22 — Phase 11 C: delta emission against the whole chain

Increment C of [[19-phase-11-plan]], after [[21-phase-11-agent-fidelity]]. It runs **before** the
attribute dictionary (11-D) so the dictionary cannot intern patterns this increment deletes.

## What was already right, and what was missing

The economy rule (spec §3.1) was implemented — `direct.minus(&resolved)` — but "resolved" meant
the wrong chain in three places:

| Case | Before | Now |
|---|---|---|
| A run's formatting | resolved against its own character style + document defaults | resolved against **the paragraph's style chain**, then its character style — the real OOXML cascade |
| A style in the catalogue | written as stored | written minus what its `based-on` chain resolves to |
| The default paragraph style on a paragraph | `{.Standard}` on every ODT paragraph | never named — it is what applies where nothing else does |
| A heading's style class | `# Title {.Heading1}` | omitted when exactly one style declares that outline level; the parser re-derives it |

The run case is the one that matters in the wild. Word writes a style's own run properties again
in every `w:rPr` of the text typed under it, and all of it was reaching the DocMark.

## Everything dropped here is reversible

That is the rule that decides whether an omission is legitimate: a value equal to the inherited
one resolves to the inherited one, so re-reading gives it back. The heading class is the only
case where re-deriving takes code — `StyleCatalog::heading_style(level)` in the model, used by
the writer to drop and by the parser to restore, so the two cannot disagree. When two styles
claim the same outline level the level names neither, and the class stays; the test
`an_ambiguous_level_keeps_its_class` pins that.

`heading_style` matches a style's **own** outline level, not its resolved one. A `TOC Heading`
based on `Heading1` inherits level 0, and counting it would make almost every real document
ambiguous.

## The corpus had nothing to drop

Corpus at `full`: **24 883 → 24 667 (−0.9 %)**, and all of that is heading and `.Standard`
classes. The cascade fix — the important one — saved exactly zero, because `generate.py` writes
minimal fixtures by hand and no fixture repeated its style.

Same trap as the raw-block in [[20-phase-11-raw-sidecar]]: a criterion measured on documents
lacking the trait is vacuous. So `docx/redundant-formatting.docx` was added, carrying the
redundancy the way Word emits it — runs repeating their paragraph style, `w:pPr` repeating the
same style, a derived style repeating its parent, a heading whose `pStyle` its level names. It
costs **600 tokens without this increment and 528 with it (−12 %)**, and its body goes from five
attribute blocks to one.

The corpus total rises to 25 195 because that fixture joined it; the like-for-like number is the
24 667 above. Worth remembering when reading the budget gate's diff: adding a fixture and making
documents cheaper both move the same total, in opposite directions.

## Goldens

Twelve moved, all in one shape: an implied class disappeared. One extra line in
`odt/basic-styles`: `Heading_20_2` no longer repeats `size: 14pt`, which its parent `Heading`
already sets — the catalogue delta catching a real duplicate in a fixture nobody had noticed.

## Trap: `git stash` on this tree

Measuring the before/after of the fixture by stashing the source files ended with
`git stash pop` aborting (`merge-ort.c: resolve_trivial_directory_merge` assertion) and leaving a
stale `.git/index.lock`. Recovery: remove the lock, `git checkout stash@{0} -- <files>`, drop the
stash. Prefer measuring by building the fixture and comparing against a recorded number rather
than stashing mid-increment.

## Next — 11-D: the attribute-set dictionary

Patterns repeating ≥ N times interned in the front matter as `{.g1}`, deterministic naming, part
of serializer idempotence. It only makes sense on what survives this increment.
