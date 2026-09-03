---
tags:
    - plan
    - phase-19
    - legacy
    - ppt
---
# 95 — Phase 19 plan: legacy `.ppt`, read-only

Implementation plan for **Phase 19** of [`docs/development-plan-v2.md:368`](../docs/development-plan-v2.md),
opened after Phase 17 closed patch editing ([[93-phase-17-close]]). The plan marks it
**parallelizable with Phase 18**, and it is: this phase lives beside the legacy `.doc` path, Phase 18
inside `crates/docsai-odf`.

Objective as the plan states it: **the `.doc` strategy applied to `.ppt`, read-only. Writing `.ppt`
stays out of scope.**

## What is already decided, and this phase must obey

- **The `.doc` strategy exists and works** ([[08-phase-5-legacy-doc]]): a native degraded reader over
  `cfb`, plus a LibreOffice fallback governed by `--use-loffice auto|never|require`. This phase
  reuses that shape rather than inventing a second one. Read
  `crates/docsai-convert/src/loffice.rs` first — Phase 16-G hardened its detection, and
  `crates/docsai-convert/src/render.rs` shows the isolated-profile and timeout discipline a child
  LibreOffice runs under.
- **Read-only means read-only.** No `.ppt` writer, no `.ppt` in the round-trip gates, no skeleton.
  A `.ppt` converts *out of* the legacy format and never back into it; say so wherever a user could
  assume otherwise.
- **`office_oxide` is evaluated before it is adopted**, and adopting it is justified in writing
  under AGENTS.md §7.4. The plan says so explicitly. Weigh it against `cfb`, which is already a
  dependency, the way [[31-phase-12-spike-p1]] weighed `ooxmlsdk`.
- **Degraded is a status, not a silence.** The degraded route records what it could not recover in
  the `ConversionReport`, and the message says plainly what LibreOffice would have given.
- **Never degrade silently, never warn falsely** (AGENTS.md §7 rule 3). This phase's whole surface
  is degradation, so the second half matters as much as the first: the native route must not warn
  about something it did in fact recover.

## The security surface, named up front

A `.ppt` is an OLE2 compound file from an era with no threat model. This phase parses attacker-shaped
bytes in two ways at once — natively, and by handing the file to LibreOffice. Both are in scope for
Phase 20's fuzzing, but neither may wait for it:

- Every offset read out of the file is bounds-checked; a truncated file is an error, never a panic
  and never a read past the end.
- The LibreOffice child runs in an isolated profile in a temporary directory, with a timeout, and
  writes nothing outside it.
- An encrypted or protected `.ppt` is **rejected with a clear error** — that is the third acceptance
  criterion — rather than half-read into a document that silently lacks its content.

## Increments

Each is independently testable and ends green on `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`,
`python3 corpus/generate.py --check`, `python3 corpus/schema_gate.py` and
`python3 corpus/render_gate.py --require-renderer`. Each ends in a KB entry and one commit.

### 19-A — the corpus, including what is meant to fail

Plan task 4, first, because the phase's three criteria are all statements about files. `corpus/ppt/`
with a truncated sample and an encrypted one, both of which exist to be **refused** rather than read.
Generation is a problem to solve honestly: `corpus/generate.py` builds packages deterministically,
and a `.ppt` is not a ZIP — say how each fixture is produced and how `--check` verifies it. If
LibreOffice is what produces them, that is a build-time dependency of the corpus and must be
recorded, not hidden.

### 19-B — the LibreOffice route

Plan task 1: `soffice --convert-to pptx` into a sandboxed temporary directory, then the Phase 13
pipeline unchanged. First acceptance criterion: **with LibreOffice, fidelity equivalent to the pptx
path** — which is measurable, because the converted deck goes through the same reader and the same
goldens. Reuses the detection Phase 16-G hardened, including the machine where plain `soffice` is
present but cannot load its own libraries.

### 19-C — `office_oxide` weighed, and the native route's shape

Plan task 2's first half. The evaluation is the deliverable: what `office_oxide` gives, what it
costs, what it pulls in, and whether `cfb` plus our own atom walk is smaller. A decision either way
is acceptable; an unargued dependency is not.

### 19-D — the degraded native route

Plan task 2's second half and task 3: `TextCharsAtom`/`TextBytesAtom` for text, the `Pictures`
stream for BLIPs, slide-boundary reconstruction, and the degraded status in the report. Second
acceptance criterion: **without LibreOffice, full text with correct slide boundaries, no panics.**

### 19-E — refusal, and the phase closes

Third acceptance criterion — **encrypted/protected `.ppt` rejected with a clear error** — proven on
the fixture 19-A built, plus the truncated one. Then the close: criteria answered with evidence,
`docs/`, `CHANGELOG.md`, `AGENTS.md` §1 and `kb/README.md` updated, and what is carried forward
written down rather than discovered later.

## Standing constraints for every increment

- Do not expand scope: no `.ppt` writing, ever, in this phase; no Phase 18 or 20 work.
- Do not loosen a gate or a metric. The residue budget
  (`crates/docsai-convert/tests/plain_residue.rs:401`, 18.0 % and 15.0 %) is not raisable, and
  `LOOP_MAX_CALLS`/`LOOP_MAX_TOKENS` in `crates/docsai-mcp/tests/wire_cost.rs` are Phase 17's
  acceptance criterion made permanent.
- Every loss is a typed warning; a false warning is the same failure as a missing one.
- No new crate (§3); no heavy dependency without written justification (§7.4).
- English in code, comments, docs and commit messages; Conventional Commits with a crate scope.
- Never run a writing `git` command — this is a colocated jj repository (AGENTS.md §4.1).
- The owner's uncommitted files are never committed, reverted or overwritten:
  `.github/workflows/release.yml`, `.github/workflows/sign-macos.yml`, `dist-workspace.toml`,
  `docs/release-signing.md`.

## Next

19-A — the corpus, including what is meant to fail.
