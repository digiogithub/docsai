# 23 — Phase 11 D: the attribute-set dictionary

Increment D of [[19-phase-11-plan]], after [[22-phase-11-delta-emission]], which had to run
first: interning patterns that delta emission deletes would freeze noise into the front matter.

Spec section: **§3.7** of [`docmark-specification.md`](../docs/docmark-specification.md) — not
§3.2 as the plan's shorthand suggested; §3.2 is inline formatting and was already taken.

## What it does

A pattern of key/value pairs repeated ≥ 3 times, ≥ 12 characters long, is written once:

```yaml
attribute-sets:
  g1: "color=#1F4E79 font=Consolas size=12pt"
```

and every node that carried it carries `{.g1}` instead. The entry's value is the attribute-block
payload verbatim, so the reader parses it with the same parser it uses for the body — one syntax,
one implementation, no second escaping scheme.

## The invariant that makes it safe

**Expansion happens before interpretation.** `BodyParser::expanded` runs on every attribute block
read from the body, so `ParaExtras::from_attrs`, `run_props_from_attrs`, `load_image` and the rest
never learn the dictionary exists. The dictionary is a compression of the bytes and nothing else:
the IR from an interned document equals the IR from a spelled-out one, which is what
`the_dictionary_says_exactly_what_the_document_said` asserts, together with the byte identity of
re-serialising the re-parsed document.

Corollaries that are enforced rather than assumed:

- A pair on the node wins over the entry's. The dictionary is a default, never an override.
- Only pairs are interned; the id and the other classes stay — they are what a node *is*.
- A raw-block is never interned: `raw::raw_sidecars` scans `src=` out of the serialised text, and
  an interned `src=` would make it invisible. `Attrs::pattern` returns `None` for `.raw`.
- A generated name skips every name the document already uses (style ids, list names, the
  structural classes of `dict::RESERVED_CLASSES`). A collision would change what a node is, not
  how it is written.

## Two passes, and why the common document pays for one

Counting has to finish before substituting, so the body is rendered twice — but the *first* pass
writes exactly the bytes a run with no dictionary writes, so when nothing earns a name that pass
is the answer and the second never happens. That is every corpus document but one.

`AttrDict` carries its counters behind a `RefCell` so the writer can hold it as `&AttrDict`
alongside its `&mut IdSource`: 20 call sites route through `Attrs::render_with` without a single
borrow conflict.

## The thresholds are arithmetic, not taste

A pattern costing `p` tokens used `k` times costs `k·p` inline and `p + e + k·c` interned, with
`e ≈ 5` for the entry and `c ≈ 3` for `.g1`. At `k = 2` the dictionary loses for every pattern
short enough to be common; `k = 3` starts paying from `p ≈ 7`. Hence `MIN_USES = 3` and
`MIN_LEN = 12`, both in `dict.rs` with the reasoning next to them.

## Sheets are out, deliberately

The dictionary applies to text documents only. A sheet already compacts identical cell metadata
into ranges (`sheet_writer::collect_cell_meta`), so a dictionary over it would be a second answer
to a question already answered. `agent` is out too: it has already dropped the formatting a
dictionary would compress, and indirection is the wrong trade for the level built to be read
directly.

## The corpus, again, could not measure it

Before writing any code: the existing goldens contain **no** repeated attribute pattern at all —
the best was `scope=default`, 4 times, 13 characters. One trait per file means nothing repeats.
Third time this phase that the corpus cannot measure the thing being built (OMML in
[[20-phase-11-raw-sidecar]], redundancy in [[22-phase-11-delta-emission]]).

So `docx/repeated-formatting.docx` was added: twelve nodes carrying two patterns that **no style
implies**, which is the complement of `redundant-formatting.docx`. It is what a document written
without styles looks like.

| | without the dictionary | with it |
|---|---|---|
| `full` | 725 | **616** (−15 %) |
| `standard` | 499 | **390** (−22 %) |

No existing golden moved — correct, and the honest reading: the mechanism is inert where there is
nothing to compress. The corpus total goes 25 195 → 25 811 purely because the fixture joined it.

Measuring the "without" number was done by flipping `dictionary_applies` to `false` and restoring
the file from a copy — not by `git stash`, which corrupted the index last increment.

## Next — 11-E: readable units with tolerance

`indent-left=48px` in the new golden is 11-E's problem, not this one: geometry in pt/cm/in with
configurable precision, round-trip compared against a documented tolerance, EMU kept as the exact
escape hatch.

And a note for **11-F** (`read --select`): a selection's minimum front matter must carry the
`attribute-sets` entries its nodes reference, or the extracted DocMark loses their formatting.
That is the first place the dictionary stops being invisible.
