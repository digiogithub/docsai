---
tags:
    - phase-17
    - apply-edits
    - addressing
    - etags
    - model
---
# 87 — Phase 17 B: the etag covers the subtree, and the address is total

Increment **17-B** of [[85-phase-17-plan]], implementing against [[86-phase-17-operations]] and
[`docs/edit-operations.md`](../docs/edit-operations.md) §5 and §7. It applies nothing and writes
nothing: it builds the two things every later increment stands on — *where* an operation happens,
and *whether it is still allowed to*.

## The question 17-A handed on: what an etag covers

17-A reserved the scope on purpose and named the consequence: `move` and `delete` operate on
subtrees, so the answer decides whether a concurrent edit *inside* a subtree is detected before
that subtree is moved or deleted.

**Decided: an etag covers the node and its whole subtree.**

The argument is not symmetry, it is what each choice does when it is wrong.

- With a **subtree** etag, the failure mode is a *false* refusal: I read `n4`, someone changed a
  paragraph three levels inside it, my `move n4` is refused with `StaleEtag` even though moving
  `n4` and editing inside it do not really conflict. The caller re-reads and tries again. It costs
  a round trip.
- With a **node-only** etag, the failure mode is *silence*: my `delete n4` guarded by an
  `if_match` succeeds, and the edit somebody made inside `n4` between my read and my write is gone
  with it. Nothing refuses, nothing warns, and the response says the operation applied — which it
  did. That is the precondition failing at exactly the job it exists for.

An unnecessary round trip is a cost. A destroyed edit reported as a success is the class of
failure AGENTS.md §7 rule 3 exists to forbid, and it is the reason a caller would have written
`if_match` in the first place. A precondition that guards the node's own bytes but not the thing
being moved is a lock with a hole in it, and — worse — the hole is invisible from the outside: the
caller cannot tell, from a success, which of the two etags it got.

There is no third position where the operation decides. `delete` and `move` would want the
subtree, `replace_text` would be happy with the node — but the etag is what a *read* hands out,
and the read does not know which operation the etag will be spent on. One etag per node, or the
value an agent is holding means something different depending on what it later does with it.

**This is a decision confirmed, not a mechanism invented.** Phase 10 already hashed containers
through their children ([[14-phase-10-addressing-core]]): `Section::hash_content` feeds its
blocks, `Table` its rows, `Slide` its shapes, its notes and its comments, `Comment` its replies.
17-B's contribution is to state that this is the contract rather than an implementation detail, and
to pin it with a test per container kind
(`crates/docsai-model/tests/target.rs::an_etag_covers_the_node_and_everything_under_it`,
`::a_slide_etag_sees_a_change_in_a_shape_and_in_a_group`).

## The other axis, and the honest limit

The increment's brief says an etag must be «different after any change that reaches the package».
Taken literally that is false here, and the falsehood is deliberate: **an etag hashes content and
structure, not appearance.** A `set_props` that only bolds a word reaches the package and does not
move an etag.

Phase 10 chose that and 17-B keeps it, for a reason that only became visible once §3 of the
operation document fixed which fidelity levels may be edited — `full` and `agent`. The `agent`
level *drops appearance by design* (spec §6: «only appearance is dropped… the way back is node by
node, with the etag proving nothing else moved»). An etag that hashed formatting would therefore
give the same node two different values depending on the level it was read at, and a caller that
read at `agent` and wrote with `if_match` would be refused for a change nobody made. §0.5 forbids
a false warning as firmly as a missed one, and a refusal is a warning with teeth.

The alternative — making the etag fidelity-dependent — is worse in the same direction, because
then the value's meaning depends on a parameter that does not travel with it.

So the limit is written down in three places rather than discovered later: in
`docs/edit-operations.md` §7, in the module header of
`crates/docsai-model/src/addressing/target.rs`, and as a test that asserts the current behaviour
and says what a change to it would mean
(`::formatting_alone_does_not_move_an_etag_and_that_is_the_documented_trade`). A caller that must
serialise formatting edits has to do it above this layer; version 1 does not pretend otherwise.

## Evidence, not claims

Both halves of «stable, and different» are measured rather than argued.

**Stable across a read that changed nothing.** Etags are derived and never stored, so this is
really a claim about determinism, and it is tested at four different scopes in
`crates/docsai-convert/tests/etag_stability.rs`:

- two reads of the same file agree on every etag, over four documents covering the three IR roots
  and both package families;
- a **warm cache** agrees with a cold one — and the test asserts the hit actually happened, so a
  cache that silently stopped working could not make it pass vacuously;
- the **two doors** agree: `SourceInput::Path` (what the CLI hands `select_input`) and
  `SourceInput::Bytes` (what the MCP server hands it from `content_base64`) produce the same
  etags for the same document;
- **across a process and across a release**, pinned by literal. `n3: 8eb3e5` and `n4: 36576c` are
  the values the DocMark specification's own §2.1 example prints, which is a pleasant confirmation
  that nothing has drifted since Phase 11 wrote that example.

A literal is the only honest form of the process-restart claim. «FNV-1a with fixed constants is a
pure function» is true and does not notice the day somebody changes what a *reader* puts in the
tree, which moves the etag just as effectively as changing the hash would.

**Different after a change that reaches the package.** Measured through a real package, not in
memory: read `docx/long-report.docx`, convert to DocMark, change one word, convert back to
`.docx`, read the result. Exactly **one** of the document's 25 etags moves. That the other 24 hold
is the more interesting half — every part of that package was rewritten, and nothing that did not
change what it says moved.

## Target resolution

17-A's grammar, implemented: `target ::= ("doc" | node-id) ("/" index)*`, a leading `#` accepted
and dropped, indices 1-based.

The type is `Place`, a borrowed cursor, and it is deliberately **wider than an addressable node**:
a target may land on a table cell, a list item or the body of a text box, none of which carry an
id. §6.1 depends on exactly that — «a table or row is refused; the cell's paragraph is the target,
reached by path» — so `r2/1/1` has to exist even though `r2/1` never gets an id.

**Resolution is total**, and the failures got as much design as the successes:

| case | error |
|---|---|
| not the grammar | `Malformed { target, reason }`, five reasons, each naming what was expected |
| id not in the document | `UnknownId { target, id }` |
| index off the end | `IndexOutOfRange { target, at, kind, index, len }` |
| index into something with no child sequence | `NotIndexable { target, at, kind }` |
| one id on two nodes | `AmbiguousId { target, id, count }` |
| a node whose id is `doc` | `ReservedId { kind }` |

Three of those rows are worth a sentence.

**`IndexOutOfRange` against `NotIndexable`** are separated because the fix differs. «This table
has two rows and you asked for the third» is answerable by asking for the second; «a sheet has no
indexable children» is not answerable by any index at all, and telling a caller to try a smaller
number would send it round a loop that cannot terminate. A **sheet** is the interesting instance:
its cells are a sparse `BTreeMap` addressed in A1 notation by `set_cell` (§6.7), and inventing a
positional order over it would give `sheet/3` a meaning that changes when an unrelated cell is
filled in.

**`AmbiguousId` asserts what Phase 10 guarantees** rather than assuming it. Two nodes cannot carry
one id — the counter is monotonic, ids are never renumbered and never reused. The lookup collects
*every* match anyway and refuses on two, because the alternative to refusing is resolving to
whichever copy the walk met first, which is an edit applied to a node the caller did not name,
silently. It costs one `Vec` per resolution and it is the cheapest invariant assertion in the
repository. It is filed as a document defect, not a caller mistake, and it maps onto none of §8's
refusals for that reason.

**`ReservedId`.** `doc` is a reserved root, and DocMark's id grammar happily allows a hand-written
`{#doc}`. Shadowing the node would make `doc` mean two things in one document; the root wins the
name and the collision is refused with the node's kind named.

The lookup walk is **not** `for_each_addressable`. That walker skips nodes whose id DocMark could
not write — a single document's section, a simple table's row — because it is the walker that
*hands out* ids. A lookup has to find whoever *has* one, or a document that arrived carrying an id
would have an unreachable node.

### What `doc` means, which 17-A left implicit

§5.1 says `doc/3` is «the third top-level block of a text document». The IR's top-level sequence
for `Document::Text` is its **sections**, so the two readings only coincide when there is one
section. 17-B resolved it the way the rest of the addressing already works: **`doc` names what the
document shows.** A single section is transparent, so `doc/3` is the third block; a multi-section
document's sections are addressable (spec §11.1) and are what `doc` indexes, so the third block of
the second section is `doc/2/3`. That is the same rule §11.1 states about when a section is
addressable at all, applied one level up, and it is why `doc` exists at all — §5.1 says so in the
same paragraph. `docs/edit-operations.md` §5.1 gained the sentence, marked as 17-B's, together
with the `ReservedId` rule.

## The precondition

`check_if_match(place, target, if_match)`. Absent is no precondition and §7 is explicit that this
is a normal edit, not a loophole. Present and equal passes. Present and different is
`PreconditionError::StaleEtag { target, expected, actual }` — typed, carrying **both** values, and
there is no branch in the module where a mismatch becomes a warning, a log line or an overwrite.

One variant 17-A did not name: `NoEtag`. `if_match` against a place that has no etag — the `doc`
root, a cell, a list item — is refused rather than passed over. A precondition the caller believes
was checked and was not is worse than no precondition at all, because the caller sized its
confidence to the former.

`PreconditionError::Target` carries a resolution failure through instead of flattening it:
reporting «the etag did not match» for a node that is not in the document would send a caller
hunting a concurrent writer that does not exist.

## Amendments to `docs/edit-operations.md`

Three, all of them filling places 17-A reserved or left implicit, none of them making the code
easier:

1. §7's «what 17-B decides» became the decision, with the appearance limit stated.
2. §8's `StaleEtag` row stopped saying «reserved for 17-B».
3. §5.1 gained the multi-section reading of `doc` and the `ReservedId` rule — the one genuine gap,
   argued above.

§13's open-question list has the etag-scope entry struck through.

## What was refused

- **No `apply`.** Nothing in this increment mutates a document. Resolution returns a shared
  borrow, which makes the constraint structural rather than a matter of discipline.
- **No new refusal invented for the operation set.** The model's error variants are finer than
  §8's; §1.1 says a more specific refusal costs no version bump, and mapping them onto
  `MalformedTarget` / `UnknownTarget` is the door's business in 17-E.
- **No selector support.** §5.2 argues it out and 17-B did not relitigate it.
- **No dependency and no crate.** `thiserror` and `serde` were already model dependencies.

## Defects found

- `crates/docsai-convert/tests/select.rs:139` still pins `odt/table-merged.odt` as a document
  whose etags move on re-parse, because the ODF reader gives rows of three cells and the DocMark
  of that table pads to four. 17-B's subtree decision makes this worse than it looked: the moving
  etag is the row's *and the table's and the section's*. Not fixed here — it is a defect in the
  ODF table round trip, exactly where that comment says it is — but it is now a documented reason
  an `if_match` on an ODF table can fail spuriously after a round trip. Carried to 17-H.

## Next

17-C: the transaction. All-or-nothing, a temporary file in the destination's own directory, the
original byte-identical on any failure, and the honest statement of where `rename` is atomic and
where it is not.
