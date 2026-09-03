---
tags:
    - plan
    - phase-17
    - apply-edits
    - mcp
    - cli
---
# 85 — Phase 17 plan: `apply_edits`, the patch editor

Implementation plan for **Phase 17** of [`docs/development-plan-v2.md:310`](../docs/development-plan-v2.md),
opened after Phase 16 closed charts, opaque objects, comments, thumbnails and the cache
([[84-phase-16-close]]).

Objective restated by the plan itself: **the change that moves docsai from converter to document
tool.** Until now the only way to change a document is to convert the whole of it to DocMark, edit
the text and convert the whole of it back. That works, and it is what every fidelity property in
this repository is built on, but it costs an agent the entire document twice for a one-word change.
This phase adds a second door: a closed set of operations addressed by node id, applied
transactionally, answering with the diff and the new etags.

## The number this phase exists for

> A scripted realistic agent loop ("retitle slide 4, add a bullet, bold it, verify") costs
> **≤ 3 tool calls and ≤ 2k tokens** on a 40-slide deck. This number is the phase's reason to
> exist; if it is not met, the phase is not done.

`crates/docsai-mcp/tests/wire_cost.rs` already measures exactly this and already has the scenario:
`deck-retitle` is checked in as a **read half** only, with the note «waits on pptx write (Phase 15)».
Phase 17 completes its return leg. Today the read half costs **1 238 tokens** of the 2 000.

**What the 2 000 covers, decided here so it is not decided by whoever is losing:** the budget is the
task's own calls — the scenario rows — and **not** the session preamble, which `wire_cost.rs`
tallies as its own row and which every task in a session pays once. That reading is the one the
harness was already built for: four rows, one of them the preamble, the gate summing all of them
only to detect drift. It is also the honest one, because the preamble is a function of how many
tools the server offers, not of how expensive an edit is. The preamble is nonetheless **reported**
in the acceptance evidence, at its current 2 124 tokens, so the phase cannot quietly move cost from
one column to the other. If the complete `deck-retitle` row exceeds 2 000 the phase is not done —
that is the plan's sentence and it is not reinterpretable.

## What is already decided, and this phase must obey

- **The IR is the definition of every operation.** The plan says so: «every operation must be
  defined against the IR and proven in round-trip». An operation is a function on the document
  model, not a text substitution on DocMark and not a splice into a package.
- **The preserved skeleton is still the rule.** An edit reaches the package through the writer that
  already exists, with 15-B's rule intact: *nothing is written unless it differs from what the part
  already says*. The acceptance criterion «editing a node never perturbs a sibling node's
  serialization» is that rule restated, and it is already true — this phase must not break it.
- **Addressing already exists.** Node ids (`#n3`, `#s4.b2`) and the `IdPolicy` that hands them out
  are Phase 10's. This phase addresses by them; it does not invent a second addressing scheme.
- **Never degrade silently, never warn falsely** (AGENTS.md §7 rule 3).
- **No new crate** (§3), no heavy dependency without written justification (§7.4).
- **Comments, charts and thumbnails landed in Phase 16.** An operation set that cannot touch them
  is a smaller phase, not a different one — see the scope note under 17-B.

## Increments

Each is independently testable and ends green on `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
`python3 corpus/generate.py --check`, `python3 corpus/schema_gate.py` and
`python3 corpus/render_gate.py --require-renderer`. Each ends in a KB entry and one commit.

### 17-A — the operation set, specified before it is written

Plan task 1, the specification half. The nine operations — `replace_text`, `set_props`,
`insert_after`, `insert_before`, `delete`, `move`, `duplicate_slide`, `set_cell`, `set_notes`,
`replace_image` — are defined against the IR in a normative document: what each takes, what it
means when the target is of the wrong kind, what it does to ids, and what it refuses. **Closed and
versioned**: the set carries a version, an unknown operation is a typed refusal naming the version
that would accept it, and the escape hatch is the one the plan names — `read --select` and rewrite.
Nothing is implemented in this increment. Its deliverable is the document and the refusals.

### 17-B — etags, and the failure that is loud

Plan task 2. An etag over a node's serialized form, stable across a read that changed nothing and
different after any change that reaches the package. Targeting by id, with the etag as a
precondition: a stale one is a typed, named failure carrying both etags, **never an overwrite**.
Decide here what an etag covers — the node alone, or the node and its subtree — and argue it, because
`move` and `delete` operate on subtrees and the answer decides whether a concurrent edit inside a
subtree is detected.

### 17-C — the transaction

Plan task 3. All-or-nothing: the operations apply to an in-memory document, the document is written
to a temporary file in the destination's own directory, and the original is replaced by rename. A
failure at any point leaves the original byte-identical — proven by injecting one, not by argument.
The atomicity of `rename` across the platforms this ships to is stated honestly, including where it
does not hold.

### 17-D — `dry_run` and the change report

Plan task 4 and task 5's first half. `dry_run` runs everything except the write and answers with the
same report the real call would return. The report carries the applied diff and the **new etags**,
so the plan's «no confirmation read is needed» is a property the wire-cost scenario proves, not a
claim.

### 17-E — one implementation, two doors

Plan task 6. `docsai edit <in> --ops <json>` and the `apply_edits` MCP tool over the same function
in `docsai-convert::edit`. Neither door may carry logic the other lacks. Note for the MCP side:
Phase 16-G spent the wire-cost gate's inflation allowance once already; a new tool costs preamble
tokens, so `apply_edits`'s schema is written for size from the first line.

### 17-F — `validate_docmark`

Plan task 7. Typed errors with a node id and a suggested fix, over the parser that already produces
them. The tool is small; its value is that an agent that wrote bad DocMark learns *where* rather
than being handed a parse failure.

### 17-G — the contextual cheat-sheet

Plan task 8. An MCP resource carrying a compact DocMark summary **limited to the features the
document at hand actually uses** — a deck with no charts and no comments does not pay for their
syntax. This is a token-budget feature and is measured as one.

### 17-H — the loop, measured

The acceptance criteria answered with evidence: a round-trip test per operation per document class,
the byte-level sibling test, the injected mid-transaction failure, and `wire_cost.rs`'s
`deck-retitle` completed and under 2 000 tokens with the preamble reported beside it. If the number
is missed, this increment's job is to make it, not to reinterpret it.

### 17-I — the phase closes

Acceptance criteria answered one by one, `docs/`, `CHANGELOG.md`, `AGENTS.md` §1 and `kb/README.md`
updated, and what is carried forward written down rather than discovered later.

## Standing constraints for every increment

- Do not expand scope: increment N does not do N+1's work, and no Phase 18/19 work.
- Do not change normative DocMark to make a test pass; 17-A's operation document, once written, is
  normative in the same way.
- Do not loosen a gate or a metric. The residue budget
  (`crates/docsai-convert/tests/plain_residue.rs:401`, 18.0 % and 15.0 %) is not raisable, and the
  wire-cost gate's inflation allowance has already been spent once in Phase 16.
- Every loss is a typed warning; a false warning is the same failure as a missing one.
- English in code, comments, docs and commit messages; Conventional Commits with a crate scope.
- Never run a writing `git` command — this is a colocated jj repository (AGENTS.md §4.1).
- The owner's uncommitted files are never committed, reverted or overwritten:
  `.github/workflows/release.yml`, `.github/workflows/sign-macos.yml`, `dist-workspace.toml`,
  `docs/release-signing.md`.

## Next

17-A — the operation set, specified before it is written.
