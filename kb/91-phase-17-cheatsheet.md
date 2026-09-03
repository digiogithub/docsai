---
tags:
    - phase-17
    - mcp
    - docmark
    - token-budget
    - wire-cost
---
# 91 — Phase 17 G: the contextual cheat-sheet

Increment **17-G** of [[85-phase-17-plan]], plan task 8, on top of the two doors of
[[90-phase-17-doors]] and `validate_docmark` of [[88-phase-17-validate]]. It is a projection of
the normative [`docs/docmark-specification.md`](../docs/docmark-specification.md); the
specification is not changed by it and could not be, because the sheet is derived from it rather
than beside it.

What lands: an MCP **resource**, `docmark://cheatsheet/{path}`, that answers with the DocMark
syntax the document at that path **actually uses**. What does not land, on purpose: the
`deck-retitle` return leg (17-H), the phase close (17-I), and any CLI door — the increment names a
resource, and an agent is the only reader a cheat-sheet has.

## The measurement, first, because it is the reason the feature exists

| document | entries | tokens |
|---|---|---|
| `corpus/docx/basic-text.docx` — prose, an empty paragraph, a manual line break | 3 | **221** |
| `corpus/pptx/charts-multi-series.pptx` | 6 | 502 |
| a deck with charts, a table, an image, a stub, notes and a comment | 12 | **914** |

Four times the sheet for four times the document. A cheat-sheet that were the same size whatever
it described would have failed its only reason to exist, so this table is the acceptance evidence
and not a nice-to-have. The plain report is told about the front matter, `[]{.empty}` and
`[]{.break kind=page}`, and is told nothing about slides, charts, comments, sheets, footnotes,
sections, images or tables — none of which it contains.

Both numbers are `o200k_base`, the encoding every other budget in this repository uses.

## Where it lives, and why it is not a tool

The derivation is `docsai-convert::cheatsheet` — the crate that already owns every question asked
*about* a document, and the only one allowed to see all three document classes. `docsai-mcp` owns
only the URI: `crates/docsai-mcp/src/resources.rs` parses it, decodes percent-escapes without a new
dependency, and hands the path to the same `resolve_document_input` and the same read cache the
tools use. A resource must not be a second door onto the filesystem with rules of its own.

**A tool would have been the wrong shape, and the cost says so.** A tool's schema and prose are
published in `tools/list` before a session has done any work; this server already pays 2 481 tokens
for that response, and a feature whose entire purpose is a smaller token bill has no business
inflating the one response nobody can decline. A resource costs the preamble the 15 bytes of
`"resources":{}` in the `initialize` capabilities and nothing else — `resources/templates/list` and
`resources/read` are fetched by a client that wants them.

Measured rather than assumed, which is what the increment asked for:

| | before | after |
|---|---|---|
| session preamble | 2 477 | **2 481** (+4 tokens, +15 bytes) |
| suite total | 5 616 | **5 632** (+0.3 %) |

The +4 lands in every scenario row too, because every scenario opens a session. `MAX_INFLATION`
was not touched and `DOCSAI_ACCEPT_TOKEN_INFLATION` was not set: +0.3 % against a 5 % allowance.
17-H, which has to complete `deck-retitle`'s return leg, still has ~4.7 % of headroom, or about
265 tokens of suite growth.

`resources/list` is deliberately **empty**. There is no cheat-sheet without a document: a static
entry would either be the whole specification — which is exactly what this increment exists not to
send — or a name for a document the server has not been given.

## How it is derived

`features(&Document) -> BTreeSet<Feature>` walks the IR: sections, blocks, inlines, cells, slides,
shapes. Thirty-one features, each a construct the specification gives syntax for. A few of the
decisions are worth writing down, because they are the ones a second implementation would get
wrong:

- **The placeholder question is asked of the writer, not re-derived.** Whether a shape needs a
  `::: {.ph}` container depends on rules 1 and 2 of §11.2, and the deck writer already asks
  `docsai_model::addressing::implicit_shapes`. So does the detector. Two answers to that question
  would drift, and the sheet would document a container the writer never writes.
- **Emphasis and a styled span are separate features**, because `**bold**` needs no syntax lesson
  and `[x]{.underline color="#FF0000"}` does. A run whose direct formatting is only bold, italic
  or strikethrough triggers the first; anything else triggers the second.
- **The front-matter entry is the one written from the document**: its `docmark:` is the version
  the document declares (1.3 with a comment, 1.2 for a deck, else 1.1 — ids are always assigned at
  `full` and `agent`) and its `source-format:` is the format read. Reading a `.dmk.md` detects
  `docmark`, which is true of the file and useless in an example, so the class supplies the answer
  that key can only have. A test asserts the substituted form equals the static constant the parse
  test validates, so the two cannot drift.
- **`attribute-sets` (§3.7) is the one approximation, and it over-reports.** The dictionary is a
  function of the *serialised text* — a payload repeated three times and at least twelve characters
  long — and the IR cannot see that. The detector asks the closest honest question: are there at
  least three nodes carrying direct formatting? Over-reporting costs one entry; under-reporting
  would leave `{.g1}` undocumented in a document that writes it, which is the failure that
  matters.

## What keeps it honest as the specification grows

A hand-maintained list nobody updates is a defect waiting to happen, and the increment said so.
Three tests, and each fails on a different kind of rot:

1. **`the_examples_parse`** (`cheatsheet.rs`). Every entry's snippet is validated as DocMark inside
   the smallest wrapper of its class — a workbook snippet under a sheet heading, a deck snippet
   under a slide, the raw-block probe as a *file on disk* with its sidecar beside it, because a
   sidecar `src=` resolves against the document's directory. An entry that drifts from the
   specification fails the build rather than misleading an agent. This is the cheap version of «the
   sheet does not contradict the specification», and it is worth exactly what it costs.
2. **`the_specification_is_fully_accounted_for`** (`tests/cheatsheet.rs`). The test scans every
   `##`/`###`/`####` heading of the specification — tracking fences, because the specification's
   examples are DocMark and full of `#` lines — and requires each to appear in `SPEC_MAP` as either
   the features that document it or a written reason it is not syntax an agent writes. A DocMark
   1.4, or a new block type, turns the suite red until somebody decides which it is. The reverse
   direction is asserted too: a feature that claims no section, and a `SPEC_MAP` row for a section
   the specification no longer has, both fail.
3. **`every_feature_is_reachable`**. The detector runs over the whole corpus, and every feature must
   be produced by a real document — or by one of two hand-built IR fixtures for the three the
   corpus does not yet carry (a `.docx` text box, a two-section document, a grouped shape). An
   entry nothing can trigger is one every reader pays for and no reader uses.

Between them: a construct present and undocumented is caught by 2, a construct documented and
absent by 3, and a construct documented *wrongly* by 1.

## Refusals

- **No CLI door.** 17-E's «two doors» rule is about `apply_edits`, an operation over a document;
  this increment's deliverable is named as an MCP resource, and adding `docsai cheatsheet` would be
  scope the plan did not ask for.
- **No new corpus document.** The corpus does not carry a deck with charts *and* comments *and* a
  table *and* notes, so the third row of the table above is a hand-written `.dmk.md`, which the
  readers take as readily as a package. Generating a new fixture is `corpus/generate.py`'s
  business, not this increment's.
- **No mention of the resource in the server `instructions`.** That string is part of the
  `initialize` response and therefore of the preamble every session pays; discovery is what
  `resources/templates/list` is for.

## Defects found

None outside the increment. The specification and the sheet agree everywhere the entries were
checked against it, which is what test 1 now asserts continuously.

## Next

17-H — the loop, measured: `deck-retitle`'s return leg, under 2 000 tokens, with the preamble
reported beside it.
