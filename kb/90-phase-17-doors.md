---
tags:
    - phase-17
    - apply-edits
    - mcp
    - cli
    - wire-cost
    - ods
    - xlsx
---
# 90 — Phase 17 E: one implementation, two doors

Increment **17-E** of [[85-phase-17-plan]], plan task 6, on top of the apply engine 17-C and 17-D
landed ([[89-phase-17-transaction]]) and against the normative
[`docs/edit-operations.md`](../docs/edit-operations.md).

What lands: **`docsai edit <in> --ops <json>`** and the **`apply_edits` MCP tool**, over one
function; the report reaching the caller with its new etags intact; typed refusals through both
doors; and the answer to the question 17-A handed 17-E about recalculation after `set_cell`
writes a formula. What does not land, on purpose: the cheat-sheet (17-G), the wire-cost
scenario's return leg (17-H), the phase close (17-I).

## The decision that shaped the increment: where the shared half lives

«Neither door may carry logic the other lacks» is a property, and a property needs a place to be
true. There were three candidates and the choice was not obvious.

1. **`docsai-convert::edit`.** The operation set's own crate, which is where the honest reader
   looks. But `edit/request.rs` says out loud that it is «hand-written rather than
   `serde_json::Value` so that this crate does not take a JSON dependency to describe a
   spreadsheet cell», and the shared half *is* JSON: turning `{"op":"replace_text","txt":…}` into
   §8's `UnknownField`. Putting it there means adding `serde_json` to a crate that deliberately
   refused it, to serve two callers that both already have it.
2. **Twice, once per door.** Rejected on sight. It is the exact failure mode the increment exists
   to prevent, and «we will keep them in sync» is not a mechanism.
3. **`docsai-mcp::edit`, called by both.** Chosen. The precedent is already in the tree and was
   set by 17-F: `docsai validate` calls `docsai_mcp::validation_output` so «the two doors cannot
   drift into saying different things about the same file»
   ([`crates/docsai-cli/src/main.rs:469`](../crates/docsai-cli/src/main.rs)). The CLI already
   depends on `docsai-mcp` — it is the crate that hosts `docsai mcp` — so nothing is inverted.

So `crates/docsai-mcp/src/edit.rs` holds the whole of both doors: `EditCall`, `parse_request`,
`report_output`, `EditFailure::output`. `crates/docsai-cli/src/main.rs::run_edit` reads a file or
a flag into a `serde_json::Value` and prints what comes back; `server.rs::apply_edits` builds the
same `EditCall` from named arguments. Neither has a branch the other lacks, and
[`crates/docsai-cli/tests/two_doors.rs`](../crates/docsai-cli/tests/two_doors.rs) proves it
against the **real binary** and a **real server on a transport**, not against the shared function:
the same operations on two copies of `corpus/pptx/forty-slides.pptx`, then the two written files
compared byte for byte and the two reports compared field for field. Six cases: a successful
batch, a dry run, four refusals, a stale etag's two values, `--output`, and a destination of
another format.

The hand demonstration, outside the suite, agrees:

```
$ docsai edit cli.pptx --ops '[…replace_text, insert_after, set_notes…]'
{"changed":[{"etag":"00505d","id":"n4/1"},{"etag":"e39d88","id":"n7"}],
 "created":[{"id":"n121","kind":"list"}],"path":"cli.pptx"}

$ … apply_edits over `docsai mcp` on stdio, same operations, on mcp.pptx
{"changed":[{"etag":"00505d","id":"n4/1"},{"etag":"e39d88","id":"n7"}],
 "created":[{"id":"n121","kind":"list"}],"path":"mcp.pptx"}

$ cmp cli.pptx mcp.pptx   →  identical, sha256 f969784b96716772…
```

## What the schema cost, measured

17-A reserved «the `apply_edits` tool schema, written for size from the first line» for this
increment, and the constraint was hard: 16-G already spent the wire-cost gate's 5 % inflation
allowance once, and 17-F spent 2.2 % of it legitimately.

The derived JSON Schema for `docsai_convert::edit::Operation` — a ten-variant internally tagged
enum — is over a thousand tokens, and `tools/list` charges it to every session before any work
happens. So `operations` is published as **an array of anything**, and the ten shapes are one
line of prose inside its description:

> In order, all-or-nothing. `{op, target, …}`; target is an id (`n7`) or an index path
> (`doc/3`, `n7/2`), never a selector. Ops: replace_text+text, set_props+props,
> insert_after/insert_before+content (DocMark), delete, move+after|before, duplicate_slide,
> set_cell+cell,value, set_notes+content, replace_image+path|content_base64,filename,alt. Each
> also takes if_match (an etag).

Nothing is lost by it, because a JSON Schema could only reject a bad request *at the client*:
`parse_request` still refuses an unknown field, a missing one and a mistyped one **by name**, on
the server, which is where §0.4's «a refusal is typed and names both sides» has to hold anyway.

| measurement | tokens |
|---|---|
| `apply_edits`, as published in `tools/list` | **246** |
| its effect on the session preamble (2 228 → 2 477) | **+249** |
| the whole wire-cost suite (5 355 → 5 616) | **+4.9 %** |

For scale, 16-G's `render_slides` cost 261 and 17-F's `validate_docmark` 104. The gate's ceiling
is 5 % and the golden was regenerated **without** `DOCSAI_ACCEPT_TOKEN_INFLATION`, which is the
point: the hatch stayed shut. Getting there took four passes over the prose — the first draft
cost 329 tokens and would have been +6.4 %.

The **report** did not inflate either. 17-C/D measured a two-target report at 31 tokens; the door
adds `path` and nothing else — no echo of the request, no per-operation status, no format name —
so the three-operation demonstration above is 129 bytes.

## The recalculation question, settled

17-A: «whether either writer can request recalculation after `set_cell` writes a formula
(`fullCalcOnLoad` / ODF equivalent), and the `Degraded` warning if not». 17-C/D left a
`Warning::Degraded` in place saying the cell «reads as blank until the sheet recalculates».

**Both writers can, so the warning is gone.** A warning that outlives its reason is a false
warning, which §0.5 and AGENTS.md §7 rule 3 forbid exactly as firmly as a silent degradation.

- **xlsx** — `xl/workbook.xml` now carries `<calcPr calcId="0" fullCalcOnLoad="1"/>`, which is
  ECMA-376's own way to ask. It is emitted **only** when the workbook actually holds a formula
  cell with no cached value, because forcing a recalculation of a workbook whose values are all
  cached would change values nobody asked to change — a volatile `NOW()` first among them.
  Verified both ways: `set_cell C6 "=B2*3"` writes the flag and LibreOffice reads `300`; a
  `set_cell C6 7` on the same workbook writes no `calcPr` at all.
- **ods** — no flag exists in ODF and none is needed. ODF keeps the cached result in
  `office:value`, so a `table:formula` cell written without one has nothing to display and the
  application computes it. Measured rather than assumed: `set_cell A3 "=A1*A2"` on
  `corpus/ods/formulas-basic.ods`, then LibreOffice Calc → `300`.

### A defect found on the way there, and fixed

Proving the ods half turned up a bug older than this phase. The ODS writer emits every formula as
`table:formula="of:=…"` but **never declared the `of:` namespace** in `content.xml`. An
undeclared prefix inside that attribute is not a formula: LibreOffice reads every one of them as
`Err:510`. Before the fix, a plain round trip of `corpus/ods/formulas-basic.ods` produced a
spreadsheet whose every formula cell displayed an error.

* repro (before): `docsai convert corpus/ods/formulas-basic.ods -o a.dmk.md && docsai convert
  a.dmk.md -o b.ods && soffice --headless --convert-to csv b.ods` → `10,20,Err:510`.
* fix: [`crates/docsai-odf/src/ods/write.rs`](../crates/docsai-odf/src/ods/write.rs), `NS_OF`
  declared on `office:document-content`. After: `10,20,30`.

No test caught it because every ODS test compares DocMark, and DocMark round-trips the formula
text perfectly whether or not any application can evaluate it. The render gate does not cover
`.ods`. That gap is real and is **not** closed by this increment; it is named here for whoever
owns the spreadsheet render gate.

## What the tool can and cannot reach on the filesystem

`apply_edits` is the most dangerous surface in the project — it is the first MCP tool that
overwrites a document — so the reach is stated rather than left to be inferred. It is also
written at the top of [`crates/docsai-mcp/src/edit.rs`](../crates/docsai-mcp/src/edit.rs), where
the person changing it will see it.

**It reads** exactly one document: `path`. Plus, for `replace_image`, the picture that operation
names — the only other path a request can contain, and the caller wrote it.

**It writes** exactly one file: `output_path` when given, otherwise `path` itself. In place is
the default because that is what an edit means, and either way the destination is a path the
caller typed. For a DocMark destination only, it also writes the `assets/` directory beside that
destination, which is where the document it wrote points its media.

**It creates** a `.docsai-edit-*.tmp` beside the destination for the length of the transaction and
unlinks it on every failure (17-C's construction, unchanged: the temporary file has to be on the
destination's filesystem or the rename is not a rename).

**It never** creates a directory, walks a glob, edits a batch, or converts. That last one is new:
`docsai edit report.docx -o out.pptx` is **refused**, because writing docx bytes under a `.pptx`
name is a silent degradation. An extension the detector does not recognise is left alone — a
caller may choose its own.

**It never loses the input on a refusal.** The transactional guarantee is not weakened at the
door: the doors call `edit_path` and nothing else, and `two_doors.rs` re-asserts after every one
of its four refusal cases that both copies are byte-identical to the corpus original.

## Two corrections the surface forced

Both are inside `docs/edit-operations.md`, and both are argued rather than assumed.

1. **`Refusal::MalformedField { op, field, reason }`.** §8's table was written before either door
   was, and a door is where JSON meets the set: `{"op":"replace_text","target":"n7","text":5}` is
   neither an unknown field nor a missing one. Without the variant it would have refused as an
   untyped serde string, which is precisely what §0.4 forbids. §1.1's own rule keeps the version
   at 1 — «a refusal made *more* specific (a new typed variant for a case that already refused)»
   does not bump it.
2. **`Refusal::DocumentDefect` became a struct variant.** Serde cannot serialize an internally
   tagged newtype variant holding a `String`, and the refusals had to serialize for a typed
   refusal to survive the wire at all. One call site.

Neither adds an operation, a parameter, or a meaning. `OPS_VERSION` stays 1.

## What a refusal looks like, at both doors

`Refusal` is now `Serialize` with `#[serde(tag = "refusal")]`, flattened into `Refused`, so the
whole thing is one object with the operation's index in it:

```json
{"index":0,"op":"set_notes","refusal":"StaleEtag","target":"n4",
 "expected":"000000","actual":"da5e97",
 "message":"operation 0 (`set_notes`) refused: stale etag for `n4`: expected `000000`, the document says `da5e97`"}
```

Over MCP that object *is* the error result's text block (and its `structuredContent` when a
client asked for one). On the CLI it goes to stderr — as the sentence by default, as the object
with `--json` — and the exit code is **2**, `EXIT_INPUT`, which is what architecture §5 has meant
by «the caller's input was wrong» since Phase 1. `message` is kept beside the fields on purpose:
a client that only prints strings still gets a sentence, and one that wants the two etags does not
have to parse English for them.

## What 17-E deliberately did not do

- **The escape into `EditOptions`.** `edit_path` wrote to its input and nothing else; a
  destination is «the door's business» in §3's words and no operation wants one, so
  `EditOptions::output` was added rather than an `EditRequest` field. The temporary file and
  `assets/` moved with it — beside the *destination*, not the input — which is what keeps the
  rename on one filesystem when the two are on different ones.
- **`fidelity` on both doors, `assets_dir` on neither.** §3 admits `full` and `agent`, so both
  doors take `--fidelity` / `fidelity`; `assets_dir` defaults to `assets/` beside the destination
  and exposing it would have cost preamble tokens for a knob an editing loop does not turn.
- **`McpConfig::for_cli()`.** `DOCSAI_MCP_MAX_INPUT_BYTES` and `DOCSAI_MCP_TIMEOUT_SECS` bound
  what a *server* accepts from a client it does not trust. Applying them to `docsai edit` would
  make the CLI refuse a large local file that `docsai convert` reads happily — a difference
  between the doors that has nothing to do with either's logic.
- **No new dependency.** `rmcp`, `tokio`, `serde_json` and `tempfile` were added to
  `docsai-cli`'s **dev**-dependencies only, and all four are already in this workspace's tree
  through `docsai-mcp`. Nothing new entered the graph. The reason they are needed is that
  `two_doors.rs` has to drive the real MCP server and the real binary in one process; a test that
  called the shared function twice would prove nothing.

## Handed forward

- **`.ods` has no render gate.** The `of:` namespace bug lived in the tree because nothing ever
  asked an application to open an ODS this project wrote. `corpus/render_gate.py` covers decks.
  Whoever owns the spreadsheet side should decide whether a formula-evaluation gate is worth its
  runtime.
- **`fullCalcOnLoad` is verified against LibreOffice, not Excel.** The attribute is ECMA-376's and
  its meaning is not in doubt, but this machine has no Excel and the claim «Excel recalculates» is
  read from the specification rather than measured. Owner action if it matters.
- **17-H's return leg.** The `deck-retitle` scenario's write half is now buildable —
  `apply_edits` exists and pptx writes — and the wire-cost golden has **no room left**: 17-E took
  the allowance to 4.9 % of 5 %. 17-H completing the scenario will move the *suite* total, and the
  gate compares suite totals, so the return leg's own cost has to come out of the response side or
  the gate is a red. Say so early rather than at the regeneration.
