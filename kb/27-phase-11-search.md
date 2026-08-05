# 27 — Phase 11 G: `docsai search`, an address and some context

Increment G of [[19-phase-11-plan]], after [[25-phase-11-read-select]]. Plan task 7: *"returns
ids + surrounding context, not the document."*

## What it does

```
docsai search report.docx "rendimiento medido" --limit 5
docsai search report.docx "riesgo" --json --context 80
```

```
s12 #n12 heading 14 tokens ×1
  …## \6. Estado de «rendimiento medido»…
n12.b1 text 81 tokens ×4
  …El equipo revisa «rendimiento medido» en cada iteracion y deja constancia escr…
… 6 more block(s) not listed (--limit)
35 match(es) in 11 block(s) · hits 386 tokens · document 9083 tokens (4.2 %)
```

## The decision the increment turned on

The obvious implementation — search the traced fragments, the way `read --select text:` does —
was written first and thrown away, because it **finds almost nothing in a real document**.
`corpus/docx/long-report.docx` is 9 000 tokens of prose under 40 headings, and its outline is 40
headings: an ordinary paragraph carries no id by design ([[14-phase-10-addressing-core]] rule 4,
spec §11.1 — *"ordinary prose is reached by relative path"*). A search restricted to addressed
nodes would answer "where does it say *rendimiento*" with the title of the section and never the
sentence, which is the question nobody asked.

So **the unit is the DocMark block, not the addressed node**: search reads the body a conversion
would write, splits it on blank lines, and matches there. Everything the document says is
findable. What differs is the *address* each hit can be given, and that is now explicit in the
type rather than implied:

| Block | Hit | Reads back with |
|---|---|---|
| carries an id | `Location::Node { position, id, kind, etag, tokens, select }` | `docsai read --select <select>` |
| carries none | `Location::Relative { anchor, block, path, tokens }` — `n12.b2` | nothing yet, on purpose |

A relative hit names **no** selector. `read --select` has no `.bN` term, because returning an
unaddressed paragraph needs a body it cannot stitch out of fragments — the whole selection
mechanism is fragment-based ([[25-phase-11-read-select]]). Handing back an address that would
read a different node is exactly the silent wrongness the project refuses, so the hit says what
it is and stops there. Closing that loop is a real follow-up, and it is not Phase 11's: it needs
relative-path *selection*, which is the machinery Phase 17 (patch editing) has to build anyway.

## Smaller decisions, and why

- **The block-level id is the last one in the block.** An attribute block is written after what
  it describes, so an earlier `{#n9}` in the same block belongs to something inline — a footnote
  reference, an image — that sits *inside* the text rather than owning it. One rule, no
  ambiguity: `A note[^1]{#n9} in prose. {#n5}` is block `n5`.
- **A footnote is found at the foot and handed over at its reference.** Its definition block
  carries no id (the reference does), so it is recognised by its `[^3]:` marker and resolved
  back to the *n*-th footnote fragment. Its `select` then names the block that refers to it —
  the mirror of the rule in 11-F where a selected block always carries its definition.
- **Case folding is one char per char**, not `str::to_lowercase`. Lowercasing `İ` yields two
  chars, which would shift every offset after it and make a snippet quote the wrong span. The
  handful of characters whose lowering is not a single char cost a missed match; every other
  document gets exact positions.
- **Literal matching, no regular expressions.** A literal is what an agent quoting a phrase out
  of an outline preview actually has. A pattern language is a second syntax to get wrong and a
  dependency to justify ([`AGENTS.md`] §, `docs/technical-analysis.md`) for a case that has not
  appeared.
- **Overlapping matches are not counted twice**, and a block reports at most 3 snippets however
  often it matched: a block that says the word twenty times is still one place to go and one edit
  to make.
- **Every fidelity level is allowed**, unlike `read --select`, which refuses the levels that
  address nothing. A level that writes no id still writes text; saying where the text is —
  relative to nothing, `.b7`, if that is what the level left — beats pretending the document is
  empty.

## How it was verified

`crates/docsai-convert/tests/search.rs`, over the corpus rather than a fixture:

- **it finds prose** — the long report's unaddressed paragraphs match and carry a relative path
  anchored at a real id;
- **it composes** — every hit that names a selector is fed to `select_path`, and the selection
  contains the text the snippet quoted;
- **a position means what `outline` prints** — `sN` from a hit selects the node the hit named;
- **it is not the document** — a phrase appearing 35 times answers in 4.2 % of the document's
  tokens, and a query that matches everywhere is capped with the remainder counted;
- **every corpus document is searchable for what it says** — a word taken out of each document's
  own golden is looked up in the document (26 of them);
- a footnote hit reads back at its reference, and `plain` finds the text while naming no id.

Gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `python3 corpus/generate.py --check`. No golden moved — `search`
writes no document, so it cannot change what a conversion writes.

## Next — 11-H: MCP

Done, in [[28-phase-11-mcp]]: `outline_document`, `search_document` and `read_selection` over
MCP, plus `include_images=none|refs|thumbnails|full` with `refs` as the new default. That closed
Phase 11.
