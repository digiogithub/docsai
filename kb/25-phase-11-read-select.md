# 25 — Phase 11 F: `docsai read --select`, a partial document that stands on its own

Increment F of [[19-phase-11-plan]], after [[24-phase-11-readable-units]]. Plan task 6, and the
last unticked acceptance criterion of the phase.

## What it does

`docsai outline` tells an agent *where* the paragraph it must edit is. `read --select` is what it
calls next: those nodes and nothing else, as **valid self-contained DocMark**.

```
docsai read report.docx --select s7-s9
docsai read report.docx --select '#n7,type:heading,text:riesgo' --json
```

Selector terms, comma-separated and unioned:

| Term | Means |
|---|---|
| `s4`, `s7-s9`, `s7-9` | the N-th addressable node **in the order `outline` prints**, 1-based, ranges inclusive |
| `#n7` | by id |
| `type:heading` | by node kind |
| `text:foo` | nodes whose text contains `foo`, case-insensitively, machinery stripped |

The output is always in **document order**: `#n9,#n2` and `#n2,#n9` produce the same file. A
selection is a reading of the document, and reading it out of order would make it another one.

## The two decisions that shape it

**The body is stitched from the fragments of a traced serialisation.** A selected node's DocMark
is byte for byte what the whole document wrote for it — nothing is re-derived, so nothing can
drift, and a test asserts the fragments appear verbatim in the golden. A node already inside a
selected one is dropped: its text is there either way.

**The front matter is the minimum** (spec §2.1): version, source format, `next-id`, `partial:
true`, `etags:`. No metadata, no page geometry, no style or list catalogue, no attribute-set
dictionary. Those describe the document the selection came from, and a caller that declined to
read the document did not ask for them either; the source stays the authority and an undefined
class is inert. Consequently a selection also carries **no dictionary** — new
`Options.dictionary`, off here — so the file depends on nothing outside itself.

## The etag finally gets written

Phase 10 computed etags and deliberately wrote none, "until Phase 11's `read --select` needs an
if-match precondition" ([[16-phase-10-node-ids]]). This is that moment.

`NodeFragment` gained an `etag`, filled in `IdSource::take` (where the node is) and read in
`IdSource::record` (where its text is) through a map keyed by id — going through the id rather
than through call order means the pairing cannot drift when a writer grows a nesting level.

The map is **recomputed on every write, never stored**. An etag *is* the content, so keeping one
would only create a second thing to get out of date; editing a node moves its etag, which is
exactly the behaviour an if-match needs. That is also what makes a selection re-write to itself
byte for byte: `frontmatter::write` derives the same map from the parsed nodes.

## `partial: true` is modelled, not just printed

`Addressing.partial` carries it into the IR, so a re-parsed selection knows what it is, writes
the flag back, and raises `Warning::PartialDocument` on every serialisation. **Severe** on
purpose: the loss is not in that document, it is in the one a careless whole-file write would
replace with it.

## Two bugs this found

- **`next-id` was never read back.** `frontmatter_parse` asked for it with `as_str()`, but
  `next-id: 26` parses to `Value::Number`, so the counter was silently dropped and rebuilt by
  `observe_ids`. Nothing showed it, because a *whole* document always has `next-id == highest id
  + 1`. A selection is the first document whose counter is deliberately ahead of its own ids —
  which is what stops a node added to it from colliding with one left behind in the source.
  Fixed. (`partial: true` walked into the same trap and was caught the same way.)
- **The ODF table round trip is not IR identity.** `odt/table-merged.odt` reads as rows of three
  cells, two spanning; its DocMark pads every row to four columns, so re-parsing gives rows of
  four. The text is identical either way — which is why the goldens never showed it — but the
  etag hashes the cells, so it moves. Not fixed here: it belongs where tables are read, not in a
  selection. Pinned in `ETAGS_MOVE_ON_REPARSE`, a one-entry list; a second entry is a regression.

## Decisions taken on the plan's task text

- **A footnote is not selectable on its own.** It is addressed at its *reference*, inside a
  block, while its text is written at the foot of the document; handed over alone it is a
  definition nothing refers to, and the parser drops it — the one thing a selection may not do.
  `type:footnote` is refused with that explanation, and a footnote fragment is never chosen.
  The mirror rule matters more: a selected block that refers to a footnote **always carries its
  definition**, which the fragment does not contain because the document writes it elsewhere.
  Without that, a selection would carry a marker pointing at nothing — the silent loss the
  project refuses.
- **`type:notes`** appears in the plan's list but there is no `notes` kind until the presentation
  profile (Phase 14). `type:` validates against the kinds that exist and names them, rather than
  matching nothing in silence.
- **`standard` and `plain` select nothing**, exactly as `outline` shows nothing there: `sN`
  positions would otherwise mean something the agent never saw. The CLI refuses them up front
  with that reason.

## Measured

Two headings of the 9 000-token `long-report.docx` cost **26 tokens** — 0.3 % of the document.
The corpus token budget is unchanged: `read` writes a new file, it does not alter what a
conversion writes.

## Next — 11-G: `docsai search`

Ids plus surrounding context, `--json`. `text:` already does the matching half of it here
(`strip_machinery` over each fragment); search is that plus context and a report shape, and it
must **not** return the document. Then 11-H exposes `outline`, `read_selection` and
`search_document` over MCP, with `include_images=refs` as the new default and a migration note.
