# 21 — Phase 11 B: `--fidelity agent`, the first projection

Increment B of [[19-phase-11-plan]], on top of the raw sidecar of
[[20-phase-11-raw-sidecar]] — a level that still carried raw payloads in the body would not be
an agent level.

## What it is

A fourth `Fidelity` variant, between `full` and `standard` in cost but **different in kind**.
`standard` answers *what survives if a human reads and edits this in a text editor*. `agent`
answers *what a program needs in order to change one node and leave the rest alone*.

One rule decides every case:

> keep what a node **is** and what it **says**; drop how it **looks**.

| Kept | Dropped |
|---|---|
| Text, headings, lists, tables, footnotes | Style and list catalogues, `style-defaults` |
| Node ids (`{#n7}`, `list-id=`), `next-id` | Indents, spacing, alignment, backgrounds |
| Style *names* (`{.Quote}`), `.sup`/`.sub`/`.underline`/`.caps` | Page geometry, section columns and paper size |
| Raw-block stubs with their id and `src=` | Raw payloads (always the sidecar, whatever `--raw` says) |
| Image link, alt, `title`, `link`, and on a sheet its anchor cell | Image width/height/EMUs, rotation, crop, border, z-index |
| Sheet formulas, merges, sheet ids | Number formats, cell styles, column widths |
| Document metadata | — |

`.sup` survives because superscript changes what the text *says*; `size=11pt` does not. A
formula survives because a formula is what a spreadsheet *is*.

## Why it declares itself

The output writes `fidelity: agent` in the front matter. Without it the level would be a trap:
the file parses, looks like a document, and writing it back whole would silently throw away the
catalogues. The key says "read me whole, write me back node by node" — which is decision 3 of
[[19-phase-11-plan]] made visible in the file rather than only in the docs.

## How it is enforced

`crates/docsai-convert/tests/agent_projection.rs`, over every docx/odt corpus document:

1. **Says everything the document says** — `plain(parse(agent)) == plain(parse(full))`. Only
   appearance may differ. This also proves the projection re-parses, sidecars included.
2. **Addresses what the document addresses** — the id sets are equal. A node with no id is a
   node you cannot write back, so a projection with fewer ids is not editable.
3. **Never pays for what it cannot edit** — `agent` keeps ≤ 50 % of the overhead `full` pays
   over `plain`. Corpus worst case: 45 %.
4. **Declares itself** — `fidelity: agent` present at `agent`, absent at `full`.

Plus the `agent` column in `corpus/token-budget.md`, gated in CI like the others
([[18-phase-10-token-gate]]).

## The measurement, and where the plan's criterion was wrong

Corpus total: **24 883 → 15 332** tokens. On text documents the cut against `full` is **62–75 %**
on 24 of 25 — except `docx/long-report.docx`, at **3.7 %**.

That is not a defect, and chasing it would be. long-report's cost *is* its prose, and a
projection that dropped prose would not be a projection. The plan's phrasing — "≥ 60 % on the
biggest corpus documents" — happens to select the one document where the criterion cannot hold,
because size and formatting overhead are unrelated. Hence the size-independent bound in the test.
The answer for a big document is not a cheaper level but reading less of it: `docsai outline`
(Phase 10 E) and `read --select` (11-F).

Spreadsheets barely move (some are a few tokens *dearer* than `full`, the cost of the ids and
the `fidelity:` line). Their cost is data — values, formulas, merges — all of which `agent` has
to keep. They are excluded from the test corpus on purpose, with that reason written down.

## Traps met on the way

- **`collect_cell_meta` was `full`-only**, and formulas live in it. A first cut of `agent` had
  spreadsheets losing every formula — the single most editable thing in a workbook. It now runs
  at `agent` in a `formulas_only` mode (formula, dialect, shared/array ranges, merges).
- **Sheet images have their own writer** (`sheet_writer::render_image`), so gating
  `writer::render_image_body` left them untouched and `ods/images-anchored.ods` came out *more*
  expensive at `agent` than at `full`. Two renderers for one concept is a standing trap here.
- **Attribute order is canonical** (`Attrs` sorts classes and keys), so reordering the class
  pushes in `style_attrs` to put them before the value guard changed no byte of any golden. Worth
  knowing before any similar surgery.
- Prose paragraphs still carry no id, by Phase 10's `paragraph_is_container` rule; they are
  reached by selector. `agent` inherits that rather than re-opening it.

## Next — 11-C: delta emission against inheritance

Ordering from [[19-phase-11-plan]]: 11-C before 11-D, so the attribute dictionary does not
intern patterns that delta emission would have deleted. Note that 11-C moves the `full` column
of the token budget, which the 5 % gate watches — that is the point.
