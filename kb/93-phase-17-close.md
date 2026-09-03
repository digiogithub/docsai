---
tags:
    - phase-17
    - close
    - apply-edits
    - etags
    - transaction
    - wire-cost
    - risk-p2
---
# 93 — Phase 17 I: the phase closed, and the number it existed for

Increment **17-I** of [[85-phase-17-plan]], the ninth and last. It adds no feature. What it owes is
the discipline of [[84-phase-16-close]] and [[72-phase-15-close]]: the four acceptance criteria
answered one by one with evidence **re-run here**, what the phase carries forward written down
rather than discovered later, the state of risk P2 said plainly, and every budget and golden the
phase touched accounted for with what was paid and what was bought.

Eight commits, in the order they landed: `dc74dc4` the plan, `6117cb8` 17-A, `215b833` 17-B,
`e85ad13` 17-F, `fa58cad` 17-C+D, `93cf418` 17-E, `3818c2a` 17-G, `79636db` 17-H.

## What the phase set out to do, and what it did

The objective as `docs/development-plan-v2.md:310` states it: **the change that moves docsai from
converter to document tool.** Before this phase the only way to change a document was to convert
the whole of it to DocMark, edit the text and convert the whole of it back. That path still exists
and every fidelity property in this repository still rests on it. What Phase 17 added is a second
door — a **closed, versioned set of ten operations**, addressed by node id, applied
transactionally, answering with the diff and the new etags — and it did so without touching the
first:

- **The set was specified before it was written** (17-A). `docs/edit-operations.md` is normative
  version **1**: ten operations, what each does to ids, what a target is, nineteen typed refusals,
  and two things refused on principle rather than on difficulty — nothing creates or edits a
  comment, and nothing touches a chart.
- **An etag covers a node and its whole subtree** (17-B), argued from what each choice does when it
  is wrong rather than from symmetry, and target resolution became **total**: six typed failures,
  each naming what was expected.
- **The transaction is ownership, not rollback** (17-C/D). Operations apply to an in-memory
  document; a refusal drops it. The write is a temporary file in the destination's own directory,
  `fsync`ed, renamed over the original — with an honest map of where `rename` does not give what is
  wanted. `dry_run` runs everything except that rename and returns a report *equal* to the real one.
- **Two doors over one implementation** (17-E): `docsai edit --ops` and the `apply_edits` MCP tool,
  with the equality asserted against the **real binary and a real server on a transport** rather
  than intended.
- **`validate_docmark`** (17-F): the list of everything wrong with a piece of DocMark, not the
  first failure, with the parser as the only authority for what is an error and what is a warning.
- **The contextual cheat-sheet** (17-G): an MCP *resource* carrying the DocMark syntax the document
  at hand actually uses, so a plain report is never told about charts.
- **The loop measured** (17-H), and four defects found by asking for the round trip the criterion
  asks for rather than for a non-empty report.

Underneath it the property Phase 15 established never moved: **a deck nobody edited comes back byte
for byte.** An empty batch through `docsai edit` on a preserved deck is byte-identical, and so is an
operation that writes a shape's own text back over itself.

## The four acceptance criteria, each with evidence re-run in this increment

Nothing below is quoted from an increment's report. Every number was produced by a command run
here, today, on this tree.

### 1 — «Every operation has a round-trip test on all four document classes it applies to» ✔

The phrase «all four document classes» is read by `docs/edit-operations.md` §2 as **text**
(satisfied only by both `.docx` *and* `.odt`), **workbook** (only by both `.xlsx` *and* `.ods`),
**presentation**, and **DocMark itself** — the serialization the operations are defined over and
the one class every operation applies to. §11 is the resulting matrix, ten rows by four columns.

`cargo test -p docsai-convert --test edit_matrix` — **green**. The matrix is a constant in
`crates/docsai-convert/tests/edit_matrix.rs`, walked cell by cell, and the walk asserts its own
size: `assert_eq!(visited, 72)`. **72 cells** is ten operations over four classes with §2's «both
packages» expansion for text and workbook, plus one `.dmk.md` per IR root an operation applies to.
A `✔` cell must apply **and survive a round trip through the document's own DocMark**; a `—` cell
must be a `NotApplicableToClass` or a `WrongKind` **and leave the file byte-identical**. A
combination the harness cannot exercise panics, so the table cannot be covered by omission.

That is the criterion read literally. Beside it, `edit_ops.rs` (18 tests) proves what each operation
*means*, `edit_transaction.rs` (14 tests) the transactional half, and `two_doors.rs` (6 tests) that
the two doors agree — all green here.

### 2 — «Editing a node never perturbs a sibling node's serialization (byte-level test)» ✔

Two hand demonstrations, run outside the suite, because the interesting half of this criterion is
different for a deck than for everything else.

**A deck, where a slide is a part of its own.** `corpus/pptx/basic-slides.pptx`, one
`replace_text` on the title:

```console
$ docsai edit deck.pptx --ops '[{"op":"replace_text","target":"n1/1","text":"Resultados del Q3"}]'
{"changed":[{"etag":"74e8ca","id":"n1/1"}],"path":"…/deck.pptx"}
$ diff -rq orig/ new/
Los archivos orig/ppt/slides/slide1.xml y new/ppt/slides/slide1.xml son distintos
```

Fifteen members before, **fifteen after**, and exactly **one** of them differs. Inside that one part,
three `<a:p>` elements before and three after, and exactly **one** differs:

```
before: <a:p><a:r><a:rPr lang="es-ES"/><a:t>Informe trimestral</a:t></a:r></a:p>
after : <a:p><a:r><a:rPr lang="es-ES"/><a:t>Resultados del Q3</a:t></a:r></a:p>
```

The run properties the paragraph had are still on it, which is §6.1's rule visible rather than
asserted.

**A text document, where every sibling lives in the same XML as the target.** This is where the
criterion has teeth, and where «before» had to be decided. The preserved skeleton is a **pptx**
property; the docx, odt, xlsx and ods writers build their parts from the model, so an *empty* batch
already changes their bytes:

```console
$ docsai edit base.docx --ops '[]'
empty batch on docx: hash e21b0202abc8 vs original 265d9a9187df
```

So the baseline for those four families is what the writer produces from the same document with no
operations at all — the strongest form these writers support — and against it the criterion means
exactly what it says. `corpus/docx/basic-text.docx`, one `replace_text` on `doc/2`:

```
member lists equal: True
members differing: ['word/document.xml']
  word/document.xml: 5 w:p elements, differing indices [1]
```

Five paragraphs, one changed, four byte-identical, in the part they all share.

In the suite this is `an_edit_touches_exactly_the_members_it_has_to` (the deck's member list) and
`an_edit_leaves_every_sibling_byte_identical_inside_the_part_it_changed` (all five families, split
on each family's own element boundary — `</w:p>`, `</text:p>`, `</a:p>`, `</row>`,
`</table:table-row>`). The pptx asymmetry is asserted rather than tolerated: the test requires the
empty batch to be byte-identical **if and only if** the family is pptx, so a writer that later gains
a preserved skeleton turns the test red and earns a stronger assertion.

### 3 — «Failure injected mid-transaction leaves the original file untouched» ✔

**The ordinary path, by hand.** A batch whose second operation names a node that is not there:

```console
$ sha256sum deck.pptx        →  e28b733d96b2db20…
$ docsai edit deck.pptx --ops '[{"op":"replace_text","target":"n1/1","text":"Nuevo"},
                                {"op":"replace_text","target":"n9999","text":"x"}]'
docsai: operation 1 (`replace_text`) refused: unknown target `n9999`: no node has the id `n9999`
exit=2
$ sha256sum deck.pptx        →  e28b733d96b2db20…      unchanged
```

The first operation applied — to a value the failure path then dropped. There is no rollback to get
wrong.

**The injected path**, which no request can reach, because every refusal a caller can trigger
happens *before* a byte is written. `EditOptions::fault` is `#[doc(hidden)]` and set only by tests.
`cargo test -p docsai-convert --test edit_transaction` — **14 passed**, including:

| test | what it shows |
|---|---|
| `a_batch_whose_third_operation_fails_leaves_the_file_byte_identical` | five operations, the third refused; the hash is unchanged and the refusal names index 2 |
| `a_refused_batch_leaves_every_package_family_byte_identical` | the same over `.docx`, `.odt`, `.xlsx`, `.ods`, `.pptx` |
| `an_injected_failure_after_the_operations_applied_leaves_the_original_untouched` | `AfterApply`: nothing serialised, nothing left behind |
| `an_injected_failure_before_the_rename_leaves_the_original_untouched` | `BeforeRename`: the **whole edited document is on disk under a temporary name**, the process fails, the original's hash is unchanged, and the temporary file's parent **is** the destination's parent |
| `an_injected_failure_leaves_every_package_family_untouched` | both fault points on all five families |

`BeforeRename` is the interesting one, and the temporary file's parent being the destination's
parent is not decoration: a rename across filesystems is not a rename but a copy and an unlink, and
a copy can be interrupted halfway — which is the failure the whole construction exists to prevent.

### 4 — «A scripted realistic agent loop costs ≤ 3 tool calls and ≤ 2k tokens on a 40-slide deck» ✔

This is the phase's reason to exist and the plan says so. `cargo test -p docsai-mcp --test
wire_cost` — **green**, and `crates/docsai-mcp/tests/goldens/wire-cost.md` on
`corpus/pptx/forty-slides.pptx`:

| scenario | request tokens | response tokens | total | calls |
|---|---|---|---|---|
| **deck-retitle** | **242** | **562** | **804** | **2** |
| session-preamble | 93 | 2 388 | **2 481** | — |

**2 tool calls and 804 tokens against a budget of 3 and 2 000.** The loop is «retitle slide 4, add a
bullet, bold it, verify» and it is two calls, not three, because 17-D's report *is* the
verification:

1. `read_selection` on `s4` — the node ids, the etags, and `next-id: 121`, which is how the next
   request knows `n121` is free to write on the bullet it creates.
2. `apply_edits` with three operations in one call — `replace_text` on the title, `insert_after` a
   bullet carrying that id, `set_props` bold on it. The response carries both changed nodes' new
   etags and the created id, so **there is no confirmation read**. Plan task 5's «no confirmation
   read is needed» is a property here, not a claim.

**Which reading of the budget was used, and where it was fixed.** The 2 000 are **the task's own
calls — the `deck-retitle` row — and not the session preamble.** That reading was fixed in
[[85-phase-17-plan]] §«The number this phase exists for», before any measurement existed and so
before anyone could be losing. It is also the reading the harness was built for: four rows, one of
them the preamble, the gate summing all of them only to detect drift. And it is the honest one,
because the preamble is a function of how many tools the server offers, not of how expensive an
edit is.

[[84-phase-16-close]] raised exactly this question and deliberately did not answer it, because
answering it changes what the criterion measures. Phase 17 answered it in its plan, in writing, and
the preamble is **reported beside the row** — 2 481 tokens today, up from the 2 124 that made
kb/84 ask — so cost cannot be quietly moved from one column to the other. The golden now carries
that sentence in prose beneath its own table, generated from the same numbers, for the same reason.

## The gate run, in full

Every command of this increment's definition of done, run here:

| what | command | result |
|---|---|---|
| the suite | `cargo test --workspace` | **890 passed, 0 failed, 1 ignored**, exit 0 |
| lints | `cargo clippy --workspace --all-targets -- -D warnings` | clean, exit 0 |
| format | `cargo fmt --all -- --check` | clean, exit 0 |
| the corpus is what its generator says | `python3 corpus/generate.py --check` | **143 generated files**, exit 0 |
| CI gate 1 | `python3 corpus/schema_gate.py` | **21 decks, 42 packages, 363 parts validated, 0 invalid, 0 not measured**, exit 0 |
| CI gate 2 | `LD_LIBRARY_PATH=… python3 corpus/render_gate.py --require-renderer` | **21 decks, 21 round trips identical by bytes, 0 rendered, 67 pages compared, 0 failures, 0 not measured**, exit 0 |

`776/0/1` at Phase 16's close is **`890/0/1`** here: **114 tests added** across the eight commits,
nothing red, and the one ignored test is still the inherited Phase 14 criterion (carried item 24).
The corpus did not move at all this phase — `git diff dc74dc4~1..HEAD -- corpus/` is empty apart
from the wire-cost golden's own directory, which is not in it — so gate 1 and gate 2 measure the
same 21 decks they measured at Phase 16's close.

## Risk P2: still OPEN, and now more expensive

`docs/spikes/P2-powerpoint-gate.md` **is still unsigned.** It needs PowerPoint and a person, and
this machine has neither. Phase 16 was started on the owner's direction with the form open;
[[84-phase-16-close]] wrote that Phase 17 «inherits the debt with interest». Phase 17 was started
the same way, and the interest is now countable.

17-A named the reason and it is structural rather than rhetorical: **every operation in the set
reaches a package as a splice into a preserved part.** That is precisely the construction the P2
form asks about. A chart's numbers, a comment's deletion, a slide's notes, a picture's bytes, a
run's properties — each of them now leaves this project through a write that keeps the surrounding
XML byte for byte and rewrites one element inside it. Before this phase a person had to convert a
whole document to change one word; the splice happened once per conversion, under human eyes. Now
an agent does it, in batches, without a person ever seeing the package.

What is proven at this close is what was proven at the last one, over the same corpus: 21
byte-identical round trips, 363 parts of 42 packages schema-valid, 67 pages rendered by a real
consumer with no failure and no blank page — and **nothing at all** about PowerPoint's own loader,
which is stricter than every one of them. Phase 17 multiplied the number of ways bytes leave this
project and added no new evidence that any of them opens.

**For Phase 18 this matters in a specific way.** Phase 18 is ODP, a *new* writer for a *new*
package family, and `apply_edits` will reach it the moment the IR does — §11's matrix has a
presentation column and no clause restricting it to pptx. So Phase 18 will extend the patch surface
onto a family that has never had a schema gate, never had a render gate and never had a human
gate. The P2 form should be signed before that happens, not after. It has now waited through two
whole phases.

## Where every budget and golden stands

Named individually, because a moved number that is not argued is a loosened gate. Each was checked
here rather than repeated from an increment's report — `git show <commit>:crates/docsai-mcp/tests/
goldens/wire-cost.md` for the history, and the tests themselves for today.

### 1 — The wire-cost suite: up three times, then a net **fall**

| after | suite total | Δ | preamble |
|---|---|---|---|
| Phase 16's close (`78b792c`…`ed8b64d`) | 5 239 | — | 2 124 |
| 17-F `validate_docmark` (`e85ad13`) | **5 355** | +2.2 % | **2 228** (+104) |
| 17-E `apply_edits` (`93cf418`) | **5 616** | +4.9 % | **2 477** (+249) |
| 17-G the cheat-sheet (`3818c2a`) | **5 632** | +0.3 % | **2 481** (+4) |
| 17-H the loop completed (`79636db`) | **5 186** | **−7.9 %** | 2 481 |

The suite total on this tree, summed from the golden's own table, is **5 186** — **1.0 % below**
where Phase 16 left it, after a phase that added two tools and a resource.

What was paid and what it bought, tool by tool:

- **`validate_docmark`, +104 preamble tokens.** It buys an agent that wrote bad DocMark the whole
  list of what is wrong with it, each item with a line, a stable code, the specification rule and a
  fix, instead of one `ParseError` and a fix-and-retry loop. Four of those 104 tokens are the clause
  added to the server instructions so the tool is findable at all.
- **`apply_edits`, +249 preamble tokens.** The derived JSON Schema for a ten-variant internally
  tagged enum is over a thousand tokens, and `tools/list` charges it to every session before any
  work happens. So `operations` is published as an array of anything and the ten shapes are **one
  line of prose** in its description. Four passes over that prose took the tool from 329 tokens to
  246. Nothing is lost by it, because a JSON Schema could only reject a bad request at the client
  and `parse_request` refuses an unknown field, a missing one and a mistyped one **by name, on the
  server**, which is where §0.4 has to hold anyway.
- **The cheat-sheet, +4 preamble tokens.** It is a *resource*, not a tool: the preamble pays the 15
  bytes of `"resources":{}` in the `initialize` capabilities and nothing else. A feature whose
  entire purpose is a smaller token bill has no business inflating the one response nobody can
  decline. It answers 221 tokens for a plain report and 914 for a deck with charts, a table, an
  image, a stub, notes and a comment — four times the sheet for four times the document, which is
  the whole point.
- **The loop's own completion, −446 tokens.** This is the one that surprises. The old
  `deck-retitle (read half)` cost 1 250 tokens over **five** calls (`estimate_tokens`,
  `convert_to_markdown`, `outline_document`, `search_document`, `read_selection`); the complete loop
  costs 804 over **two**. Measuring the return leg made the row *cheaper*, because the return leg is
  what let the read half stop converting the whole deck to a file first.

**The `DOCSAI_ACCEPT_TOKEN_INFLATION` hatch was authorised for 17-H and not used.** 17-G's own
entry warned that 17-H had «~4.7 % of headroom, or about 265 tokens of suite growth», and 17-E's
warned that «the return leg's own cost has to come out of the response side or the gate is a red».
Completing a scenario checked in as a half is new measurement rather than drift, so opening the
hatch would have been legitimate. It was not opened, at any point in the phase: 17-F's +2.2 %,
17-E's +4.9 % and 17-G's +0.3 % are each inside the gate's own 5 % allowance, and 17-H's regeneration
was a *fall*. `MAX_INFLATION` is still **5.0** and untouched. **Phase 16's `render_slides` remains
the only place in this project's history where that hatch has been opened.**

### 2 — The residue budget: untouched, and unchanged

`crates/docsai-convert/tests/plain_residue.rs` measures what a `standard` rendering leaks that a
plain Markdown viewer would have to hide — risk **P4**'s budget. Measured here:

| | measured today | budget |
|---|---|---|
| corpus-wide | **15.1 %** | 18.0 % |
| the decks that are documents | **12.2 %** | 15.0 % |

Identical to Phase 16's close, and it could not have been otherwise: the file was last modified by
`cb531a1` (Phase 14 G), the corpus did not move this phase, and nothing on the *read* path changed.
Recorded because a budget nobody checks is a budget nobody notices being raised.

### 3 — The corpus token budget: not regenerated

`corpus/token-budget.md` is untouched this phase, because no corpus document was added or changed.
The `agent_projection.rs` ceiling of 50 % was not approached and not raised.

## The new hard gate: `LOOP_MAX_CALLS` and `LOOP_MAX_TOKENS`

`crates/docsai-mcp/tests/wire_cost.rs` now carries the acceptance criterion as two constants —
`LOOP_MAX_CALLS = 3` and `LOOP_MAX_TOKENS = 2_000` — and asserts them **before the golden is
consulted**, outside the `DOCSAI_UPDATE_GOLDENS` branch entirely:

```rust
// The criterion, asserted before the golden is even consulted: a golden is
// a record of what something costs, and this is a budget. A regenerated
// golden must never be able to make the loop affordable.
assert!(loop_cost.calls <= LOOP_MAX_CALLS && loop_cost.total_tokens() <= LOOP_MAX_TOKENS, …);
```

`Tally::calls` counts the `tools/call` frames that actually crossed the transport, so the
criterion's «tool calls» is measured where every other number in that file is measured rather than
kept on the side by the test.

**What that changes for anyone who later makes the loop more expensive.** The wire-cost golden has
always been *soft*: a number moves, the diff shows it, and `DOCSAI_UPDATE_GOLDENS=1` records the new
truth — with `DOCSAI_ACCEPT_TOKEN_INFLATION=1` as the argued escape when the move is large. That is
the right shape for a record of cost. It is the wrong shape for a budget, because a budget that can
be regenerated is not a budget. So the loop's two numbers are now outside that mechanism: neither
environment variable reaches them, and the only way to make a red one green is to make the loop
cheaper or to change the constants in the source, in a diff, with an argument. A third tool call
added to `deck-retitle`, or 1 200 tokens added to its response, fails `cargo test` on a machine with
no environment set — which is exactly what the plan meant by «if it is not met, the phase is not
done». The golden also now prints the criterion in prose beneath its table, with the preamble beside
it, so the two columns cannot be swapped quietly by a later reader who never saw this argument.

## What `apply_edits` can and cannot reach on a user's filesystem

`apply_edits` is the first tool in this project that writes to a document **in place**, so its reach
is stated rather than inferred. [[90-phase-17-doors]] argued it and it is written at the top of
`crates/docsai-mcp/src/edit.rs`, where the person changing it will see it; it is restated here
because this is where a reader looking for the security surface will come.

**It reads** exactly one document: `path`. Plus, for `replace_image`, the picture that operation
names — the only other path a request can contain, and the caller wrote it.

**It writes** exactly one file: `output_path` when given, otherwise `path` itself. In place is the
default because that is what an edit means, and either way the destination is a path the caller
typed. For a **DocMark destination only**, it also writes the `assets/` directory beside that
destination, which is where the document it wrote points its media.

**It creates** a `.docsai-edit-*.tmp` file beside the destination for the length of the transaction,
and unlinks it on every ordinary failure. The temporary file has to be on the destination's own
filesystem or the rename is not a rename. A `SIGKILL` runs no destructor, so a hard crash between
the write and the rename can leave one behind; the name starts with a dot and says what it is, which
is the most that can be done.

**It never** creates a directory, walks a glob, edits more than one document, or converts. That last
one is a refusal rather than an omission: `docsai edit report.docx -o out.pptx` is **refused**,
because writing docx bytes under a `.pptx` name is a silent degradation. An extension the detector
does not recognise is left alone — a caller may choose its own.

**It never loses the input on a refusal**, and that is not weakened at the door: both doors call
`edit_path` and nothing else, and `two_doors.rs` re-asserts after each of its four refusal cases
that both copies are byte-identical to the corpus original.

Two limits of that guarantee, both from 17-C's honest map. **Durability is not atomicity**: the file
is `fsync`ed before the rename and the directory after it, but a directory `fsync` is a no-op on
some platforms, and on a crash this guarantees you never read a half-written document — it does not
guarantee that a completed edit survives a power cut on a filesystem that reorders the directory
entry. And **no atomicity is claimed on a network filesystem**: on NFS it is the server's property,
on SMB/CIFS a replacing rename is not guaranteed atomic at all, and neither can be detected cheaply
enough to refuse.

The two doors also differ in one deliberate place. `DOCSAI_MCP_MAX_INPUT_BYTES` and
`DOCSAI_MCP_TIMEOUT_SECS` bound what a *server* accepts from a client it does not trust; they are
not applied to `docsai edit`, because making the CLI refuse a large local file that `docsai convert`
reads happily would be a difference between the doors that has nothing to do with either's logic.

## What Phase 17 carries forward

Collected here so Phase 18 finds them in one place instead of in seven increments' prose. **Every
item below was reproduced in this increment** before it was written down; the ones that no longer
reproduce are in the «dropped» section with the reason.

### Found or reinforced during Phase 17

1. **The preserved skeleton is a pptx property only.** The docx, odt, xlsx and ods writers rebuild
   their parts from the model on any write-back, so an *empty* batch changes those files' hashes
   (measured above: `265d9a9187df → e21b0202abc8` on `basic-text.docx`) and criterion 2 is met for
   four of five families against the writer's own output rather than against the original bytes.
   Extending 15-B's construction to the other writers is a phase, not a patch.
   ([[92-phase-17-loop]])
2. **`odt/table-merged.odt`'s etag moves on a re-parse that changed nothing.** The ODF table reader
   yields rows of three cells where the DocMark of that table has four columns.
   `the_merged_table_etag_defect_is_measured_and_not_merely_described` is green here and names the
   blast radius: **exactly one node**, the table `n1` — the rows and the section of that document
   are not separately addressable, so 17-B's subtree scope did not widen it. The cost is a **false**
   `StaleEtag` on `n1`, never an overwrite, so it fails in the safe direction. The fix belongs in the
   ODF table reader. ([[87-phase-17-etags]], [[92-phase-17-loop]])
3. **There is no schema gate and no render gate for spreadsheets or text documents.**
   `corpus/schema_gate.py` and `corpus/render_gate.py` are pptx-only by construction. This is the
   reason two real `.ods` write-back defects lived in the tree unseen until 17-E and 17-H went
   looking (both now fixed — see «dropped»), and it is the strongest carried item in this list,
   because it is the one that hides the others. ([[90-phase-17-doors]], [[92-phase-17-loop]])
4. **`fullCalcOnLoad` is verified against LibreOffice Calc, not Excel.** The attribute is
   ECMA-376's and its meaning is not in doubt, but this machine has no Excel and «Excel
   recalculates» is read from the specification rather than measured. Owner action if it matters.
   ([[90-phase-17-doors]])
5. **The text parser drops an unknown container class *silently* when the container's body flattens
   to exactly one block**, and errors when it does not (`parser.rs:746`); the deck parser warns in
   the same situation (`deck_parser.rs:434`). Reproduced here: `docsai validate` on a file holding
   `::: {.sidebar}` / blank / `A` / blank / `:::` reports `container/unknown-class` as a warning,
   and `docsai convert` on the same file says **nothing** and exits 0. A class quietly discarded is
   AGENTS.md §7 rule 3's silent degradation. Fixing it means adding a `Warning` to the text parser,
   which moves `inspect --json` goldens. ([[88-phase-17-validate]])
6. **All 21 pptx goldens carry a true warning.** `assets/_skeleton/deck-<hash>.pptx` is referenced
   by their `skeleton:` key and is not in the corpus, so parsing one warns that the preserved
   package is not beside the document. Verified here: **21 of 21**. The warning is correct — writing
   one of those goldens back would lose the package it was cut from — and the corpus, not the
   parser, is where it would be fixed. ([[88-phase-17-validate]])
7. **`set_notes` cannot remove a notes page from a preserved deck, and says so.** Reproduced:
   `set_notes` with empty content on `corpus/pptx/notes-speaker.pptx` returns `changed` **and** a
   `Degraded` warning — «its speaker notes, which the model does not hold and
   `ppt/notesSlides/notesSlide1.xml` still states» — and the part is still in the archive. Taking a
   notes part out of a preserved package is one of Phase 16's named refusals. The rule that matters
   holds: it is never silent. ([[89-phase-17-transaction]])
8. **A selector is not a target, and that is version 1's narrowing.** `docs/edit-operations.md`
   §5.2 refuses `s4.b2` and the plan's task 2 names it. The argument is now measured rather than
   asserted: the loop that task exists to serve costs 2 calls and 804 tokens with ids alone, so a
   selector target saves a caller nothing it does not already hold. Adding a target form is a
   **version 2** decision — that is what §1's version number is for — and if the owner reads task 2
   as requiring selectors, the disagreement is about one sentence and not about the test plan.
   ([[86-phase-17-operations]], [[92-phase-17-loop]])
9. **A package stores no node ids, so a structural edit renumbers them on the next read.** Settled
   as a **property**, not a defect, and written into `docs/edit-operations.md` §5.4.1 with a test:
   after `move n1 after n3` on a deck, a re-read hands `n1` to the slide that used to be `n3`. No
   Office format stores a node id and this project deliberately refuses to annotate a package to
   make one stick (commit `286a51f` removed exactly that). A `.dmk.md` writes its ids down and does
   not have the property. The honest sentence is *an id is stable for the lifetime of a reading*,
   and the guard is the etag: an id that has come to mean a different node carries a different one.
   Carried because a caller holding `n7` across a `move` on a package is holding a position.
   ([[89-phase-17-transaction]], [[92-phase-17-loop]])

### Inherited from `kb/84` and still open

Each re-checked here. Numbering continues; kb/84's own numbers are given so the two lists can be
matched.

10. *(kb/84 #1)* **`parse_shape` warns with the *slide's* line number, not the container's.**
    Reproduced: a deck whose slide heading is on line 7 and whose `.chart` container opens on line
    11 reports `degraded chart table at line 7`. Fixing it means threading a line offset through
    `split_one_fence`.
11. *(kb/84 #2)* **`count_shape_images` no longer only counts images** — it counts a chart's
    embedded workbook too, correctly, and the name now lies about what the function does
    (`crates/docsai-convert/src/inspect.rs:435`).
12. *(kb/84 #3)* **A path input is read twice on a cache miss.** `service.rs:412` does
    `std::fs::read(&canonical)` to hash the bytes and `read_source` opens the file again.
13. *(kb/84 #4)* **Legacy `.doc` is never cached**, and that is a decision rather than an oversight
    (`benefits_from_loffice` gates the insert): whether a `.doc` read went through LibreOffice is a
    property of the machine at that moment and no key can carry it honestly.
14. *(kb/84 #5)* **`sniff` labels a `.pptm` as `.pptx`.** `assets.rs`'s ZIP branch is a bounded
    substring search for `ppt/presentation.xml`, and a macro-enabled deck contains that string too.
    It affects only the name an asset is stored under, never what is read.
15. *(kb/84 #6)* **A stub over `mc:AlternateContent` gets no name and no geometry**, so
    `order::sort` puts it last on its slide. Reproduced on `corpus/pptx/ole-embedded.pptx`: slide
    1's stub is `::: {#n3 .ole name="Hoja de cálculo 1" pos=… size=…}` and slide 2's is
    `::: {#n6 .ole raw=raw-0002}` and nothing else. Harmless on both fixtures, wrong on a deck where
    the object is not last.
16. *(kb/84 #7)* **A picture inside an opaque object is not extracted as an asset of the deck.**
    Reproduced: `ole-embedded.pptx` carries `ppt/media/image1.png` and `docsai inspect --json`
    reports `"images": 0` and `"media": []`. The bytes are safe; what is lost is that an agent
    extracting a deck's images gets everything except the one image an OLE object is *seen* as.
17. *(kb/84 #8)* **DocMark carries nothing for a slide transition or a timeline**, so the write-side
    warning cannot fire on the parse-and-write path: the direct model emits two
    `raw-block-dropped` warnings and the model parsed back from DocMark emits none.
18. *(kb/84 #9)* **`corpus/schema_gate.py` selects `mc:Fallback` before validating**, which is the
    compatibility processing a consumer does and therefore correct — and means `ole-embedded.pptx`'s
    slide 2, the branch that holds the actual `p:oleObj`, has never been validated.
19. *(kb/84 #10)* **No corpus deck hides a slide**, re-checked here (`show="0"` appears nowhere in
    `corpus/`), so 16-G's page-count-mismatch guard is reasoned rather than exercised.
20. *(kb/84 #11)* **`***bold italic***` under `{.underline}` does not survive a docx round trip.**
    Reproduced: `[***text***]{.underline}` comes back as
    `[**\*text**]{.underline}[\*]{.underline}` — the emphasis markers escaped and the span split.
    Open and belonging to nobody.
21. *(kb/84 #12)* **PowerPoint's modern comments are not read.** Reproduced on
    `comments-slide.pptx`: `[Severe] unsupported element <p188:cmLst> at
    ppt/comments/modernComment_1.xml`, with the part preserved unchanged and the refusal naming the
    content type and what it carries.
22. *(kb/84 #13)* **The `.ole` stub does not carry `progId`**, so an agent cannot tell an embedded
    worksheet from an embedded equation. Reproduced: no `progId` anywhere in the DocMark of
    `ole-embedded.pptx`. Carrying it is a normative DocMark change.
23. *(kb/84 #14)* **A preserved chart's *shape* cannot be edited through DocMark — only its
    numbers.** Reinforced this phase rather than fixed: `docs/edit-operations.md` §12 states in one
    line that **no operation of version 1 touches a chart**, and §9 is how you do it — read the
    chart's DocMark table, retype a number, convert back.
24. *(kb/84 #15)* **A generated chart has no design and no embedded workbook, and a generated combo
    chart is one plot.** `scatterChart`, `radarChart` and the 3-D plots fall back to `barChart` with
    a warning naming the kind.
25. *(kb/84 #16)* **A multi-level category axis is not read, and the `c:f` formulas are not in
    DocMark** — the first warned on read, the second deliberate, because 16-C splices inside the
    preserved part where the formula still is and a copy in DocMark could contradict it.
26. *(kb/84 #17)* **The cache charges an entry its *input* length plus its media bytes, not the size
    of the tree.** Bounded and one-directional; worst for a small package that expands into a large
    tree.
27. *(kb/84 #18)* **Nothing caches a render.** A second `render_slides` on the same deck spawns
    LibreOffice again.
28. *(kb/84 #19)* **A classic pptx comment carries no thread and no resolved flag**, because
    `p:cmLst` is flat. This is the format, not a defect — recorded so nobody reads the asymmetry
    with docx as a reader bug.
29. *(kb/84 #20)* **There is no `comments` counter in `ConversionStats` and no comments column in
    `inspect`'s slide inventory.** Re-checked: `docsai inspect --json` on `comments-slide.pptx`
    contains the string `comments` **zero** times.
30. *(kb/84 #21)* **The human P2 gate itself.** `docs/spikes/P2-powerpoint-gate.md`, unrun. Owner
    action. §«Risk P2» above.
31. *(kb/84 #22)* **Gate 2's threshold of 2.5 has still never been scored against a real render.**
    Re-confirmed by this increment's own gate run: *21 round trips identical by bytes, **0
    rendered**, 67 pages compared*, every generated page reporting `diff n/a (geometry)`.
32. *(kb/84 #23)* **The LibreOffice loader path is narrowed, not closed.** 16-G derives the
    installation's `program` directory for the child process, so `--thumbnails`, `render_slides`
    and the `.doc` fallback work here with no environment variable — but `corpus/render_gate.py` is
    a Python script and still needs `LD_LIBRARY_PATH` in the command, which this increment set. The
    permanent fix is root and an owner action.
33. *(kb/84 #24)* **The inherited Phase 14 criterion is still open.** `--fidelity agent` was to cost
    ≤ 15 % of `full` and measures ~100 % over the deck corpus. Still `#[ignore]`d rather than
    softened, because closing it changes what a fidelity level *means*. It is the `1 ignored` in
    `890/0/1`.
34. *(kb/84 #25)* **A preserved table whose grid the model moved is copied with a warning**
    (`crates/docsai-office/src/pptx/write.rs:2501`). Nothing owns writing a moved grid back into a
    preserved slide.
35. *(kb/84 #26)* **The refused internal hyperlink in pptx, and its docx twin.** The docx half
    reproduces: `[the section](#_Toc12345)` is written as a relationship with
    `Target="#_Toc12345" TargetMode="External"` and `<w:hyperlink r:id="rId2">`, where
    WordprocessingML wants `<w:hyperlink w:anchor="_Toc12345">` and no relationship at all.
36. *(kb/84 #27)* **Preserved parts are never schema-validated**, by construction — the gate
    validates only the parts whose bytes the writer changed or created.
37. *(kb/84 #28)* **Both gates run only at `full`.** Neither script passes `--fidelity` anywhere, so
    whether an `agent` or `standard` rendering writes a valid package has never been asked. (Item 3
    is the wider form of this.)
38. *(kb/84 #29)* **`align`'s content-based pairing can still guess** when two or more slides are
    both edited *and* reordered; the writer pairs them with the parts left over, in order, and
    reports it (`crates/docsai-office/src/pptx/write.rs:1097` — the function has moved since kb/84
    recorded line 693). The real fix is an identity for a slide, which nothing in this chain
    carries.
39. *(kb/84 #30)* **No real-world decks in the corpus.** All 143 corpus files are generated by
    `corpus/generate.py`, so no deck any real producer wrote has ever been through this reader.
    Item 7 of the P2 form is still the first chance to get PowerPoint's own bytes back through it.

### Dropped from the list, with the reason

Five things earlier increments of this phase recorded as open no longer reproduce, because a later
increment of the same phase closed them. They are listed so nobody re-opens them from an increment's
own entry.

- **`set_props` was a silent no-op on docx and pptx.** ([[89-phase-17-transaction]]'s §6.2 as
  implemented.) `run_direct` is the *paragraph mark's* run properties: ODF writes it where it reaches
  the text, docx writes it to `w:pPr/w:rPr` and pptx to `a:endParaRPr`, neither of which formats
  anything a reader sees. **Fixed in 17-H** in the edit layer, not the writers, so no conversion's
  bytes moved.
- **`highlight` was typed as `RRGGBB` and validated as anything.** ([[89-phase-17-transaction]]
  defect 2.) `w:highlight/@val` is the `ST_HighlightColor` enumeration. **Fixed in 17-H**: validated
  against the seventeen names and refused by `UnknownField` listing them; §6.2's table corrected.
- **`set_cell` writing a formula warned that the cell would read blank.**
  ([[89-phase-17-transaction]] defect 5.) **Removed in 17-E**: both writers can request a
  recalculation — `.xlsx` gains `<calcPr fullCalcOnLoad="1"/>` and only when the workbook really
  holds an uncached formula, `.ods` needs no flag at all. A warning that outlives its reason is a
  false warning. (What remains is carried item 4: the verification is against LibreOffice, not
  Excel.)
- **Every formula in every `.ods` this project wrote read as `Err:510`.** The ODS writer emitted
  `table:formula="of:=…"` and never declared the `of:` namespace. **Fixed in 17-E.** Older than this
  phase, and found only because 17-E asked an application to open a file this project wrote.
- **The ODS writer dropped a picture's name and description on every write-back.** `draw:name`,
  `svg:title` and `svg:desc` were never emitted, so a plain `.ods → DocMark → .ods` conversion lost
  both silently. **Fixed in 17-H**, mirroring what the `.odt` writer has always done.

One more is settled rather than dropped: **the ODF writer normalises an all-zero crop rectangle
away** ([[89-phase-17-transaction]] defect 3). A crop of zero on every side is not a crop; the two
spellings mean the same placement, nothing a person chose is lost, and there is no degradation to
warn about. `edit_ops.rs::placement` normalises the two spellings and says why.

## What Phase 18 inherits

A converter that is now a document tool. Ten operations, addressed by id, guarded by etags, applied
all-or-nothing, reachable through a CLI and an MCP tool that cannot drift apart, answering with a
report that makes a confirmation read unnecessary — and a validator and a contextual cheat-sheet
beside them, so an agent that writes DocMark learns *where* it went wrong and is told only the
syntax the document in front of it uses.

Two things Phase 18 should read before it starts. **Risk P2**, because ODP adds a fourth package
family to a patch surface no application but LibreOffice has ever opened, and the form has now
waited through two phases. And **carried item 3**, because ODP will be the third format family with
no schema gate and no render gate, and this phase found two silent `.ods` write-back defects that
had been in the tree for months for exactly that reason.

## Next

Phase 18 (`docs/development-plan-v2.md`) — ODP ⇄ DocMark. **First, though**:
`docs/spikes/P2-powerpoint-gate.md`.
