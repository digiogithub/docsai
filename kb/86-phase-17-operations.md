---
tags:
    - phase-17
    - apply-edits
    - specification
    - addressing
    - normative
---
# 86 — Phase 17 A: the operation set, closed before it is written

Increment **17-A** of [[85-phase-17-plan]], the first. It adds no code. Its deliverable is
[`docs/edit-operations.md`](../docs/edit-operations.md) — normative, version **1** of the edit
operation set — and the refusals it defines.

Writing the specification first is not ceremony. It is what 16-E did for comments (one IR node and
one syntax designed before either writer touched a package) and what Phase 14 did for DocMark-P,
where the parser worked on the first run because the eight rules had already been argued. An
operation set is a worse thing to discover by implementation than a serializer is: a serializer
that guesses wrong writes a bad file, and an operation that guesses wrong hands an agent a
plausible success.

## The ten-versus-nine question, settled

`docs/development-plan-v2.md:316` is the authority and it lists **nine items**, one of which is
`insert_after`/`insert_before` joined by a slash. [[85-phase-17-plan]] then writes «the nine
operations» while enumerating that pair as two, which is where the discrepancy comes from. There is
no eleventh operation hiding in it and none missing.

**Ten names.** They are kept as two names rather than collapsed into one `insert` with a
`where: after|before`, and the reason is measured rather than aesthetic: an operation name and an
enum value cost the same one or two tokens on the wire, so collapsing saves nothing and buys a
required parameter every insert pays.

## The id decision, which is why the document exists

Phase 10 already decided the hard half and it is implemented: ids come from a monotonic counter,
are never renumbered on insertion and **never reused after deletion**
(`crates/docsai-model/src/addressing/mod.rs`). 17-A's job was to state what each of the ten
operations does inside that guarantee, and three of the answers were not obvious.

**`move` preserves every id, the subtree's included.** That is the whole reason an identity exists
separately from a position. If a move renumbered, an id would be a position with extra steps.

**`duplicate_slide` gives the copy and every node inside it fresh ids, sharing nothing.** Two nodes
holding one id is the exact failure ids were introduced to prevent, and a duplicate is the one
operation that could produce it by accident.

**`insert_*` and `duplicate_slide` allocate ids even under `--ids preserve`.** `preserve` describes
what a *conversion* writes back; a node created by this request has no id to preserve, and the
response has to be able to name it. The corollary is §3's refusal: a request that would write a
document carrying no ids at all — `--ids never`, `--fidelity plain`, and `standard`, which is a
lossy projection — is **refused** rather than served with ids the file does not contain. Handing
back `n41` for a node that is not in the file is a false statement, and the response is the thing an
agent trusts most.

Two more that took an argument each:

- **A caller's own id in an inserted fragment is honoured if free and *refused* if taken**
  (`DuplicateId`), never renumbered. Renumbering would return a document in which the caller's
  `{#n9}` means a different node than the one it wrote it on — the same failure as reusing a freed
  id, reached from the other side.
- **`deleted` in the response lists targets, not subtrees.** Enumerating forty shape ids to say a
  slide is gone would cost more than the entire rest of the response. The rule is stated instead:
  every id beneath a deleted target is deleted with it.

## Targets: ids, one reserved root, and a narrowing that is recorded rather than hidden

A target is `("doc" | node-id) ("/" 1-based-index)*`.

`doc` had to be invented. A single-section text document has **no addressable section** — spec
§11.1 gives a section an id only where the serializer writes a `::: {.section}` container, which is
a multi-section document — so without a root token there is no way to say «at the top» of the
commonest document in the corpus. The index path is the other half of the same hole: ordinary prose
paragraphs and the rows of a GFM table carry no ids by design, because DocMark has nowhere to write
one, and an operation set that could not reach them would fail on the first `.docx` anybody tried.

**Selectors are not targets, and that is a narrowing of plan task 2.** The plan says «by id
(`#s4.b2`) or relative path»; `s4.b2` is a *selector* by the specification's own words, and version
1 refuses it. Three reasons, in the document: a selector is evaluated against a document state and a
batch has no single state; a selector can match four nodes, and the set deliberately has no
operation that applies to a set of nodes; and a caller never needs one, because every read primitive
already returns ids. It is written down as an **open question for 17-H** rather than quietly
implemented as if the plan had said ids only — if the owner reads task 2 as requiring selectors,
that is a version 2 decision.

The rule that makes the whole thing teachable in one line: **ids are stable, positions are not.** A
target is resolved when its operation runs, so an id is immune to everything earlier in the batch
and an index path is not. That is the honest rule rather than the convenient one, and it is what
makes `created` in the response load-bearing: the next operation in the same call addresses what the
previous one made.

## What the set refuses, and the two it refuses on principle

Nineteen typed refusals, each naming both sides — `set_cell` takes a sheet and `n7` is a paragraph,
both halves in the error. Three shapes are worth recording:

**`NotApplicableToClass` is raised before the target is resolved.** `set_notes` on a text document
is not «no such node»; it is «this document is a text document and `set_notes` applies to
presentations». Same for `duplicate_slide` and `set_cell`. Resolving first and refusing on kind
would produce a technically true message that tells the caller nothing about what it did wrong.

**An unknown field is a refusal, not an ignored key.** `props: {bould: true}` returning success
would be a silent no-op reported as a change. This is also why §1.1 bumps the version for an added
*optional* parameter: a new client cannot otherwise tell whether the server honours the parameter or
ignores it, and a server that ignores what it does not know is the silent degradation AGENTS.md §7
rule 3 forbids.

**Two things are refused on principle rather than on difficulty.**

- **Nothing creates or edits a comment**; `delete` accepts one. A comment asserts that a *named
  person* said something. `docsai` cannot mint that assertion — there is no person on the other end
  of an `apply_edits` call, and the agent is not an author — and editing one changes what a named
  person is recorded as having said. Deleting is different in kind: it removes an assertion instead
  of fabricating one, and it is a normal act of reviewing a document. The same argument makes
  `duplicate_slide` **not** copy a slide's comments, with a `Degraded` warning only when the source
  actually had some, so the warning is never false.
- **Nothing touches a chart.** 16-C established that a preserved chart's *shape* is not editable and
  its numbers are edited by rewriting its DocMark table. Rather than leave that unstated, §12 says
  it in one line: an operation cannot edit a chart in version 1, and §9 is how you do it.

Also out, each with its reason: styles (document-wide, not an addressed edit), thumbnails (a render,
not content), raw markup (already a file in `assets/_raw/`), metadata and page geometry (front
matter, a line each), table columns (not nodes in the IR at all), layouts (re-resolving the
placeholder cascade is a different document), and anything across two documents (ids are
document-scoped).

## The version, and the channel that costs nothing

One monotonic integer. §1.1 says what bumps it, and the row that took the argument is «a parameter
added, even an optional one: **yes**», for the reason above.

How a client learns it matters more than the number. Three channels, **none of them a tool call**:
the refusal itself names the version the server speaks and the version that would accept the unknown
operation; the `tools/list` description names it in one clause, paid once per session; and
`docsai edit --help` prints it. The response of a *successful* call deliberately does not carry it —
four tokens on every successful edit forever, to answer a question only a failing client asks and
which the failure already answers.

## The measurement

Every shape in the document was priced with `docsai_convert::tokens::count` (`o200k_base`, the same
encoder as every other budget in this repository) over the framed JSON an MCP call actually carries.

| what | tokens |
|---|---|
| envelope: `tools/call`, tool name, `path`, empty list | 39 |
| `+ replace_text` | +18 |
| `+ set_cell` | +21 |
| `+ delete` | +10 |
| `+ duplicate_slide` | +11 |
| `+ if_match` on any of them | +9 |

**One edit end to end is 118 tokens** (57 request, 61 response). «Retitle slide 4, add a bullet,
bold it» is three operations in **one** call: **167 tokens, one tool call**. Against the 1 238 the
read half of `deck-retitle` already spends, the return leg is not where the 2 000-token budget will
be lost — which is worth knowing now, because the alternative was to find out in 17-H with the
shapes already fixed.

Three decisions were taken on those numbers and are recorded so they are not re-litigated by feel:
`if_match` is optional (9 tokens, half a whole `replace_text`); `duplicate_slide` has no destination
parameter (11 tokens, a destination would roughly double it for a position that is the default
almost always); and neither the version nor a deleted subtree is enumerated in the response.

## Why no Rust in this increment

The brief allowed types that are «the specification expressed in Rust and carry no behaviour», and
preferred them deferred to 17-B if they tempted anyone into implementing. None are added. An
operation enum with a serde shape is one `match` away from an `apply` function, and the parameter
shapes above are not yet load-bearing on anything — 17-B needs the etag decision before the
`if_match` field has a type worth writing down. The refusals of the increment's deliverable are the
nineteen named variants of §8, specified with their fields; 17-B turns them into a `thiserror` enum.

## Four classes, and the sentence it hangs on

Phase 17's first acceptance criterion says «all four document classes», and the IR has **three**
roots. §2 reads the fourth as **DocMark itself** — a first-class input and output of `docsai edit`,
the serialization the operations are defined over, and the one class every operation applies to —
and then makes the criterion stricter than it looks: **text** is satisfied only by both `.docx`
*and* `.odt`, **workbook** only by both `.xlsx` *and* `.ods`, because they are different writers
over the same tree and an operation that works on one and not the other is exactly the defect an
IR-level definition was supposed to prevent. §11 is the resulting matrix, ten rows by four columns,
and 17-H scores it. If the owner reads the phrase otherwise, the disagreement is about one sentence
rather than about the test plan.

## What this increment hands on

- **17-B**: the etag's *scope* — node, or node and subtree. `move` and `delete` operate on subtrees,
  so the answer decides whether a concurrent edit inside one is detected before it is moved or
  deleted. §7 fixes the field's name, type, placement and failure and leaves the scope alone on
  purpose.
- **17-D**: the full response shape (§5.6 fixes only the id-bearing part) and what `dry_run`'s diff
  is.
- **17-E**: whether either writer can request a recalculation after `set_cell` writes a formula
  (`fullCalcOnLoad` and its ODF equivalent) — and, if not, the `Degraded` warning that says the cell
  will read blank until the sheet recalculates rather than inventing a number. Also the
  `apply_edits` schema, written for size from the first line.
- **17-H**: the two readings above — selectors as targets, and «four document classes».
- **Owner**: risk **P2**, unchanged and now more expensive. Every operation in this set reaches a
  package as a splice into a preserved part, which is precisely the construction
  `docs/spikes/P2-powerpoint-gate.md` asks about, and it is still unsigned
  ([[84-phase-16-close]]).

Nothing in [[84-phase-16-close]]'s thirty carried items is re-opened by this increment. Two are
*named* in the document where an operation touches their surface — item 29's content-based slide
pairing under `move`, and item 7's unaddressable OLE snapshot under `replace_image` — in both cases
to say the operation neither improves nor worsens them.

## Verification

No code changed, so the suite is a regression check rather than a proof of anything new:
`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `python3 corpus/generate.py --check`, `python3 corpus/schema_gate.py`
and `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer`
all green. The token figures above were produced by a throwaway example against
`docsai_convert::tokens::count` and removed before the commit; they are reproducible from the JSON
literals quoted in `docs/edit-operations.md` §10.

## Next

**17-B** — etags, and the failure that is loud.
