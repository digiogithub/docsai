# 24 — Phase 11 E: readable units, and the tolerance that was not needed

Increment E of [[19-phase-11-plan]], after [[23-phase-11-attr-dictionary]]. Spec §2.

## What was wrong

The old rule picked the **first unit that divided the length exactly**, in the order
`px → cm → pt → emu`. Exact, and unreadable: a Word indent of 720 twips came out `48px`, a page
margin `70.85pt`, a column width `200px`, and a zero margin `0px`. Pixels are a screen unit; no
one authoring a document thinks in them. Worse, sibling values landed in different units
depending on arithmetic accident.

## The rule now

A length is written in the unit of **what it measures**:

| Kind | Order | Why |
|---|---|---|
| Layout and typography — indents, spacing, margins, page size, column widths, list levels | `pt` → `cm` → `emu` | Word stores layout in twips and a twip is exactly `0.05pt`, so two decimals name **every** value a text document can hold |
| Drawings and bitmaps — image sizes, anchor offsets | `px` → `cm` → `pt` → `emu` | a bitmap has a natural size in pixels |

Zero names no unit: `0`, not `0px`.

One implementation: `Length::render(LengthStyle, precision)` in the model, wrapped by
`docmark::units::{len, geometry}`. `Display` delegates to the geometric rendering, so there is no
second copy of the rule to drift.

Exactness is decided on **integers** — `emu * 10^precision % emu_per_unit == 0` — because asking
a float whether it has exactly two decimals is a question floats cannot answer.

## The tolerance the plan asked for is not needed

The plan's task 5 said round-trip comparison should use "a documented tolerance instead of byte
equality". It does not, and that is a better outcome:

- A unit is only used when it names the length **exactly** at the configured precision.
- `--precision N` (default 2) is the knob: `1.251cm` needs three decimals, so at 2 it is written
  `450360emu` and at 3 `1.251cm`. It buys *readable units*, never rounding.
- `emu` is the escape hatch and always exists.

So the documented tolerance is **zero**, and `tests/readable_units.rs` measures it rather than
asserting it: every twip from −2000 to 2000, every pixel to 2000, and a set of awkward values
(ODF hundredths of a millimetre, thirds of an inch, raw EMU) re-parse to the identical EMU at
precisions 0 through 6. Accepting a tolerance would have bought nothing and spent the round-trip
identity the project rests on. Recorded as a correction in the plan's acceptance criteria, the
same way [[21-phase-11-agent-fidelity]] corrected the `agent` criterion.

## What moved

17 goldens, all in the same three shapes: layout `px` → `pt`, `0px` → `0`, and image sizes
unchanged. The corpus costs **25 811 → 25 776** tokens at `full` (−0.1 %) — this increment is
about reading, not cost, and saying otherwise would be inventing a benefit.

`--precision` changed nothing on the corpus, because no generated fixture holds a length that two
decimals cannot name. Unlike 11-C and 11-D no fixture was added for it: the knob's whole surface
is the exactness test, which the unit tests cover directly and a fixture would only restate.

## Ripples worth knowing

- `frontmatter::write` now takes `&Options` instead of `source`/`fidelity` separately — it needed
  a third field and the argument list was already six long.
- `ConvertOptions`, `DocMarkOptions` and the CLI all carry `precision`.
- `para_flow` gained a precision parameter; it is `pub` but only the front matter calls it.

## Next — 11-F: `docsai read --select`

`s4`, `s7-s9`, `#id`, `type:notes`, `text:foo` → valid standalone DocMark with the minimum front
matter. Two things this phase has already put in its path: the selection's front matter must
carry the `attribute-sets` entries its nodes reference ([[23-phase-11-attr-dictionary]]), and it
is where the etag finally has to be written, since a partial read is what an if-match precondition
is for.
