# AGENTS.md — Guide for developers and AI agents

This file is the operational reference for anyone or any AI agent working
on the `docsai` repository. Read it fully before touching code.

## MANDATORY operating rules for AI agents

These rules are **MANDATORY**, not best-effort guidance. Every AI agent working in this
repository **MUST** follow all of them on every task, even small or one-line changes. When a
habit or a shortcut conflicts with a rule, the rule wins.

The rules assume you have the project's agent tool suite: the knowledge base (the `kb/`
directory, served through the KB and memory tools below), a code-indexing / remembrance
subsystem, web search, the Context7 library-docs tools, and a headless browser. The tool names
below are exact; if one is unavailable in your environment, fall back to the closest equivalent
and say so explicitly.

### MANDATORY 1 — Gather context before starting any task

Before writing code, designing, fixing, or planning anything that builds on prior work, you
**MUST** recover the relevant context first. Never start from a blank slate when prior knowledge
exists. In order:

1. **Knowledge base first.** Search with `kb_search_documents` (or `hybrid_search_remembrances`
   when you also want indexed sessions and code in the same query). This returns both the stored
   documentation in `kb/` and everything saved via the memory subsystem (`remember`) in one
   semantic + full-text query. Search **without** a `tags` filter for broad context recovery; only
   add `tags` when you deliberately want to narrow to one document type.
2. **Follow the graph, do not only search.** KB documents link each other with `[[wiki links]]`:
   `kb_search_documents` reports how connected each hit is and lists the neighbours of the best
   one, `kb_get_document` returns a hit's outgoing links and backlinks, and `kb_related_documents`
   hops through them — a document's own links beat a second guess at the query. Calling
   `kb_related_documents` with no `file_path` lists the concepts the KB references but never
   documents. The on-disk knowledge base in `kb/` (start at `kb/README.md`, as §2 item 5 requires)
   is the same content this search surfaces.
3. **Then the code index.** Use the code-remembrance tools to understand structure and prior
   decisions in the codebase before editing: `code_get_symbols_overview` (file/package shape),
   `code_find_symbol` (locate a specific function/type), `code_hybrid_search` (semantic search
   across the codebase and related indexed projects), and `code_search_pattern` (literal or regex
   matches). Prefer these over blind file reads when locating code.
4. **Recall only when you know the key.** Use `recall` only when you already know roughly which
   short fact/key you are after. Do not use it as a substitute for the KB search above.

If, after searching, no relevant context exists, state that briefly and proceed.

### MANDATORY 2 — External research when the answer is not in the repo

When the task needs knowledge that is not in the repository or the KB, you **MUST** research it
instead of guessing:

- **Library / framework / API usage** → use the Context7 tools: first `c7_resolve_library_id`
  to resolve the library name to its ID, then `c7_get_library_docs` for current,
  version-accurate documentation and usage patterns. Prefer Context7 over memory for any
  third-party API surface. When you choose or replace a dependency, record the reason where
  `docs/technical-analysis.md` / `docs/technical-analysis-presentations.md` require it (§2 item 4).
- **General/unknown facts, current events, error messages, release notes** → use web search and
  `fetch` the most relevant sources. Cross-check more than one source for anything load-bearing.
- **Frontend / web-UI work** (rendering, layout, DOM, console errors, visual verification) → use
  the browser tools: `browser_navigate`, `browser_get_content`, `browser_evaluate`,
  `browser_click`, `browser_fill`, `browser_screenshot`, `browser_console_logs`, and
  `browser_network`. Verify UI behavior by actually driving the page, not by assuming. (docsai
  ships no web UI today; still apply these tools if a task touches one.)

### MANDATORY 3 — Plan before non-trivial work

For anything larger than a trivial change you **MUST** produce a written plan before implementing:

- Break the work into **phases**, each independently testable.
- First identify the matching phase in `docs/development-plan-v2.md` (§2 item 1) and, per §7 rule 1,
  do not advance future phases.
- Save the plan with `kb_add_document` under a clear `file_path` following the existing
  `kb/NN-<slug>.md` naming (e.g. `kb/12-my-topic-plan.md`) so it survives the session and can be
  recovered later with `kb_search_documents`.
- If you are unsure whether a plan already exists, search for it first and confirm with the user
  before diverging from it.
- Keep the plan updated as phases complete.

### MANDATORY 4 — Implement in small, verified increments

- Write code in small, testable increments. After each increment, run the build and tests
  (§4: `cargo build --workspace`, `cargo test --workspace`, `cargo clippy`,
  `cargo fmt --all -- --check`) to confirm the change works before moving on.
- Match the surrounding code: naming, style, comment density, and idioms, following the
  conventions in §5.
- Add or update tests for new behavior, placed where the project expects them (§6).
- Never report something as done unless you verified it (tests passed, build succeeded, or the
  behavior was observed). If a step was skipped or a test failed, say so plainly with the evidence.

### MANDATORY 5 — Document every change in the knowledge base

Every time you modify, implement, fix, or refactor anything — even a one-line change — you
**MUST** record a summary with `kb_add_document` once the change is done. This keeps the living
documentation in `kb/` current and is **not optional**.

The summary **MUST** capture at least:

- **What changed** — a concise description of the behavior/code change.
- **Files & symbols touched** — the concrete paths and functions/types.
- **Why** — the motivation or the bug being fixed.
- **How it was verified** — tests run, build status, manual checks.

Store it under a clear `file_path` (`kb/<slug>.md`, following the existing `NN-<slug>.md`
convention; use `kb/03-technical-decisions.md` for a non-obvious decision, per §2 item 5). If a
related document already exists, **update it** instead of creating a duplicate.

**Link what the document builds on.** Write `[[concept]]` (or `[[concept|label]]`) in the body to
point at the plan, feature or fix the change continues — a full path (`[[kb/03-technical-decisions.md]]`)
or a bare name (`[[foo]]`) both work. These links are indexed as a navigable graph, which is what
keeps the knowledge base connected instead of a pile of loose files. Linking a document that does
not exist yet is **correct and useful**: it records a concept worth documenting later, and
`kb_related_documents` lists those pending concepts.

> Plan-mode note: while a harness "plan mode" is active you may only edit the plan file, so defer
> the `kb_add_document` write until the plan is approved and writes are allowed again — but do not
> skip it.

### MANDATORY 6 — Choosing the right memory tool

- **`remember` / `recall`** — ONLY for short, durable facts identified by a known key (e.g.
  `project.test_command`, `user.preferred_lang`). Call `remember` to upsert; call `recall` when
  you know roughly what key you want. Never use `remember` for long or structured content.
- **`kb_add_document` / `kb_search_documents`** — for any extensive or ordered information:
  plans, analyses, design notes, multi-step decisions, references. This is also what MANDATORY 1's
  pre-task search uses.

### MANDATORY 7 — General conduct

- Use **English** for code, comments, and documentation (the hard rule in §5).
- Parallelize independent work when possible, but never at the cost of losing context. When
  delegating to sub-agents, give them clear, self-contained instructions and the context they
  need.
- For actions that are hard to reverse or outward-facing (pushing, merging, publishing, changing
  shared state), confirm first unless durably authorized — see §7 rule 7 (push only to the branch
  you are told, never to `main`).

## 1. What this project is

`docsai` is a cross-platform Rust binary (Windows/Linux/macOS) that converts
Office documents (`.doc`, `.docx`, `.xls`, `.xlsx`) and LibreOffice documents
(`.odt`, `.ods`) to an extended Markdown called **DocMark**, and back, with
minimal format loss. It is invoked as a CLI or as an **MCP server over stdio**.

**Current status**: **Phases 0–7 are closed** for the core path. The workspace
has the seven crates, the IR (`docsai-model`), the `.docx` and `.xlsx` readers
**and writers** plus `.xls` and degraded `.doc` read (`docsai-office`), DocMark
serialize **and parse** for text and workbooks (`docsai-docmark`), ODT/ODS
readers **and writers** (`docsai-odf`), orchestration (`docsai-convert`, including
optional LibreOffice headless for `.doc`, plus `inspect`, batch, style-map, and
in-memory MCP helpers), the CLI with `convert`, `inspect`, `formats`, `roundtrip`
and `mcp`, and the MCP stdio server (`docsai-mcp` / `rmcp`) with the tools from
architecture §6 (four converters plus the three Phase 11 addressing primitives). The DOCX writer covers floating DrawingML, image transforms,
and full footnote bodies; the full docx corpus round-trips with DocMark identity.
ODF packages use the same IR with automatic-style de-automatization and OpenFormula
preserved. Phase 6 also adds stdin/stdout pipelines, `--out-dir` batch conversion,
`--style-map`, and `cargo-dist` packaging.

**Phase 10 of plan v2 is closed**: DocMark 1.1 with stable node ids (`{#n7}` + `next-id`,
`--ids assign|preserve|never`), derived etags, `docsai tokens` and `docsai outline` measured
with a vendored BPE tokenizer, and the corpus token budget committed as
`corpus/token-budget.md` and gated in CI.

**Plan v1 (`docs/development-plan.md`, Phases 0–9) is delivered and deprecated.** The active
plan is **`docs/development-plan-v2.md` (Phases 10–20)**: agent-native primitives first (stable
node ids + etags, `outline`/`read --select`/`search`, raw-block sidecar, `--fidelity agent`,
measured token budget), then presentations (`.pptx` ⇄ DocMark, charts, `.odp`, legacy `.ppt`),
then patch editing (`apply_edits`), then hardening — which absorbs the open items of v1 Phases
8–9 (fuzzing, adversarial suite, benchmarks, audit).

**Phases 11 and 12 of plan v2 are closed** too: the raw-block sidecar, `--fidelity agent`,
`read --select`, `search` and the three addressing primitives over MCP; then the three
presentation spikes (`docs/spikes/P1`–`P3`) and the `corpus/pptx` deck corpus.

Current work is **Phase 13**: `.pptx` reading into the IR. `Document::Presentation` exists
(`docsai-model::presentation`) and `docsai-office::pptx` reads the package layer — parts,
layouts, masters, slide order, sections — plus the shapes and text of each slide, resolved
against the placeholder cascade so that only the delta over layout, master and theme is stored,
plus the pictures and tables of a slide — the same `ImageRef` and `Table` a `.docx` carries — and
the speaker notes, reached through the slide's own relationships. The shapes come back in a
computed reading order — placeholders by type, then the rest by top-left — that stays reversible
because every shape keeps its source `p:spTree` index. The original package is kept whole in the
asset store as `Presentation::skeleton`, so nothing the reader does not model is lost. Groups,
connectors, SmartArt, custom geometries and a slide's animation subtrees come back as a visible
stub plus a verbatim raw fragment plus a typed warning, and a chart is recorded with its markup.
A `.pptm` reads as its macro-free equivalent with `Warning::MacrosIgnored`, and a broken package —
truncated, corrupted, malformed, dangling — is a typed `ReadError` or a warning, never a panic.
`.pptx` is now **readable**: it is in the `read` dispatch, `docsai formats` says `pptx: read yes,
write no`, and `docsai inspect` reports the slide inventory — layout, shape counts, notes,
SmartArt/OLE — so an agent can decide where to edit without loading the deck. Converting a deck to
DocMark is still refused (`unsupported conversion: pptx -> docmark`): the DocMark-P profile is
Phase 14, and an empty body written to a file would lose every slide silently. **Phase 13 is
closed** with that.

Current work is **Phase 14**: the DocMark-P serializer and parser (plan `kb/46-phase-14-plan.md`).
Its first increment is done — `docs/docmark-specification.md` §11.2 is now **normative** DocMark
1.2: the eight rules spike P2 measured, the `layouts:` and `skeleton:` front-matter keys, and the
version rule (a deck declares `1.2` with or without ids). The parser accepts exactly `1.0`, `1.1`
and `1.2` and refuses anything else by name. The second is done too: a deck **writes its front
matter** — the 1.2 version, the layout catalogue with the title and body placeholder indices, and
the `skeleton:` path — at `full` and `agent`, and neither catalogue nor skeleton at `standard`.
The third is done: a deck **writes its slides** — an `##` heading per slide carrying `.slide`, the
title placeholder consumed by that heading, and the layout's primary body written as ordinary
blocks under it, decided by the catalogue and never by a heuristic. The fourth is done: every
other shape is a **container** — `::: {.ph idx=…}`, `::: {.shape geom=…}`, `::: {.connector …}` —
with its geometry in readable units and `emu` as the exact fallback at `full` and `agent`, `geom=`
and `type=` kept at `standard` because they are identity, slide furniture dropped where it is not
written back, and no containers at all at `plain`. The fifth is done: **speaker notes** are
`::: {.notes}` at `full` and `agent` and a **blockquote** at `standard` — the one node whose syntax
depends on the level, which the parser will have to read both ways — and dropped with a warning at
`plain`. The sixth is done: **pictures, tables, groups and stubs** — a picture is an image line and
a table a GFM table, both carrying their *shape's* id and placement; a group is `::: {.group}`
around children that keep their own addresses; a chart, SmartArt, an OLE object, a media clip or a
custom geometry is a stub of its own class over the sidecar. An image on a slide carries no
measurements at `standard`, where a text document keeps them. Every kind of shape is now written.
The seventh is done: **`plain` is proven**, not asserted — spike P2's residue probe is a repository
test (`docsai-convert/tests/plain_residue.rs`) that renders every corpus deck with a viewer having
no container and no attribute extension: zero residue at `plain`, and at `standard` only the
`{.slide}` marker and the rule-4/8 containers may show. It found three writer defects, now fixed:
`layout=` at `standard` (its catalogue is not written there), `col-widths=` on a slide table at
`standard`, and `geom=rect`, the default nobody chose. The eighth is done: the **parser** reads
DocMark-P back — front matter, `.slide` headings, every container class, pictures, tables, groups,
and notes in both syntaxes — and every corpus deck makes the round trip at `full`, `agent` and
`standard` with its addresses and the kind of every shape intact. Input is tolerant: a deck typed by
hand, with no attribute anywhere, parses. The ninth is done: **goldens and byte idempotence** — the
seventeen decks of `corpus/pptx` are pinned by their DocMark-P as `<name>.expected.dmk.md`, in the
same test and by the same mechanism as the other three corpora, and `serialize(parse(md)) == md`
holds byte for byte over all of them, which is what makes a hand edit change what it touches and
nothing else. The tenth is done: **the deck converts** — the 13-K refusal is gone, `docsai convert
deck.pptx -o deck.dmk.md` writes DocMark-P with the preserved package beside it as
`assets/_skeleton/deck-<hash>.pptx`, and `outline`, `tokens`, `search` and `read --select` follow
from the serializer with no work of their own. The Phase 13 criterion deferred into this phase was
measured and **is not met**: `--fidelity agent` costs 96–102 % of `full` over the corpus, not the
≤ 15 % of analysis §6.5 — the two levels differ by one line, because what `agent` drops these decks
barely carry and the geometry §6.5 wanted collapsed is written at `agent` by design. The criterion
is in the suite as an `#[ignore]`d test rather than softened; closing it changes what a level means,
which is a specification decision. The eleventh is done and **Phase 14 is closed**: the P4 gate was
run — three `standard` decks edited as text without consulting the spec — and it found the one thing
four increments of writer tests could not, because they never asked what happens when a *person*
writes: a second notes blockquote replaced the first instead of joining it, losing visible text
without a warning. A slide has one notes page; fixed in the parser and written into the spec. The
phase's three criteria are met and the fourth, the inherited `agent` ≤ 15 %, is open in writing.
Next is **Phase 15**, the pptx writer and the anti-repair gate — a new phase, so read
`docs/development-plan-v2.md` and open its plan in the KB before touching anything.

## 2. Documents you must read before implementing

Mandatory reading order:

1. `docs/development-plan-v2.md` — **the active phased plan** (Phases 10–20). **Identify
   which phase the project is in before writing anything.** Do not implement items from
   future phases. `docs/development-plan.md` is plan v1: delivered, deprecated, historical
   reference only — never open new work against it.
2. `docs/architecture.md` — workspace structure, IR model, contracts between
   crates.
3. `docs/docmark-specification.md` — the extended Markdown format. It is a
   contract: any change requires bumping the `docmark` field version in the
   front matter and documenting the migration.
4. `docs/technical-analysis.md` — why each library was chosen. Do not replace a
   key dependency (calamine, docx-rs, rmcp, comrak…) without leaving a written
   record of the reason in that document.
   `docs/technical-analysis-presentations.md` — same role for presentations
   (`.pptx`/`.ppt`/`.odp`) and for the agent context-cost decisions of plan v2.
5. `kb/` — knowledge base of what is **already built**: summary of closed
   phases, real code structure, technical decisions with their rationale, and
   considerations for the phase you are about to tackle. Start with
   `kb/README.md`. If your change alters the structure or takes a non-obvious
   technical decision, update it in the same PR. Before starting a task, recovering
   this context is **mandatory** — see MANDATORY 1 in the section above.

## 3. Repository structure (target)

```
docsai/
├── Cargo.toml                # root workspace
├── crates/
│   ├── docsai-model/         # IR: intermediate document model (no I/O, no heavy deps)
│   ├── docsai-docmark/       # DocMark serializer + parser (IR ⇄ extended Markdown)
│   ├── docsai-office/        # OOXML readers/writers: docx, xlsx (+ xls, doc read)
│   ├── docsai-odf/           # ODF readers/writers: odt, ods
│   ├── docsai-convert/       # orchestration: pipelines, format detection, assets, fidelity reports
│   ├── docsai-cli/           # CLI binary (clap)
│   └── docsai-mcp/           # MCP stdio server (rmcp)
├── docs/                     # design documentation (this set)
├── corpus/                   # versioned test documents (see §6)
└── tests/                    # integration and round-trip tests
```

Dependency rules between crates (violating them is an architecture error):

- `docsai-model` depends on no other crate in the workspace.
- `docsai-docmark`, `docsai-office`, `docsai-odf` depend **only** on `docsai-model`.
- `docsai-convert` depends on the four above. `docsai-cli` and `docsai-mcp`
  depend only on `docsai-convert` (and `docsai-model` for types).
- No format crate imports another format crate.

## 4. Working commands

When the workspace exists, these are the canonical commands (they must stay green):

```bash
cargo build --workspace
cargo test --workspace                 # unit + integration + round-trip
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p docsai-cli -- convert corpus/docx/basic-styles.docx -o /tmp/out.dmk.md
python3 corpus/generate.py --check     # the corpus is generated; CI checks it is up to date
```

Updating a golden is a deliberate act, and its diff is reviewed by hand:

```bash
DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens
```

CI (GitHub Actions) runs the matrix `{ubuntu-latest, windows-latest, macos-latest}` ×
`{stable}`. A PR is not merged with red CI.

### 4.1 Version control is **jj**, not git

This is a **colocated jj repository** (`.jj/` and `.git/` side by side): git is the storage
backend and the transport, [jj](https://jj-vcs.github.io/) is the interface.

**Never run a `git` command that writes.** No `commit`, `add`, `reset`, `checkout`, `stash`,
`rebase`, `merge`, `restore`, `read-tree`. Read-only git (`log`, `show`, `diff`, `cat-file`) is
allowed, and jj has an equivalent for all of it.

Two things git reports about this repository are **normal, not damage**, and trying to fix them
is how the damage actually happens:

- `HEAD` is detached. Git's `HEAD` tracks `@-`, the parent of jj's working-copy commit. There is
  no branch to be on.
- `git status` shows files staged with a blob identical to `HEAD`. jj rewrites `.git/index` on
  every snapshot; jj has no staging area, and neither does this repository.

```bash
jj status                       # what changed
jj log                          # history, including the working-copy commit `@`
jj diff                         # the current change
jj commit -m "…"                # describe `@` and start a new empty `@`
jj describe -r @-               # reword the last commit
jj squash                       # fold the working copy into the last commit
jj new <rev>                    # put work aside — no stash, the change stays a change
jj bookmark create <name> -r @- # name a line of work (a git branch)
jj git push --bookmark <name>   # push it
jj undo                         # undo the last operation; see also `jj op log`
```

Rationale and the history of getting this wrong: `kb/26-jj-vcs.md`. The session skill is
`.claude/skills/jj-vcs/SKILL.md`.

## 5. Code conventions

- **Rust edition 2021+ / stable toolchain**. No `nightly` in the main tree.
- Default `rustfmt` and `clippy -D warnings`. No exceptions without a justified
  `#[allow]` comment.
- Errors: `thiserror` in libraries, `anyhow` only in binaries (`docsai-cli`,
  `docsai-mcp`).
- Logging: `tracing`. In the MCP server **never write to stdout** except for the
  protocol (stdout is the JSON-RPC channel); logs always go to stderr.
- **Hard language rule: everything is English.** All documentation (`docs/`,
  `kb/`, `README.md`, `AGENTS.md`), code identifiers, commit messages, and code
  comments **must** be in English. Do not write Spanish (or any other language)
  in those places.
- Commits: Conventional Commits (`feat:`, `fix:`, `docs:`, `test:`, `refactor:`,
  `chore:`), with crate scope when applicable: `feat(office): read numbering.xml`.
- `unsafe` is forbidden unless justified in the PR documentation (it should not
  be necessary).
- Every public function in library crates has a doc-comment with an example when
  reasonable.

## 6. Testing strategy (summary; detail in the plan §Phase 0 and §Phase 8)

- **Versioned corpus** in `corpus/`: small documents created on purpose, one
  trait per file (`basic-styles.docx`, `nested-lists.docx`,
  `formulas-basic.xlsx`…). Never add documents with real or private data.
- **Golden files**: each corpus document has its expected DocMark beside it
  (`.expected.dmk.md`). Tests compare output against the golden; updating a
  golden requires reviewing the diff by hand.
- **Round-trip tests**: `Office → DocMark → Office → DocMark` must produce
  identical DocMark on the second pass (idempotence). First-vs-second pass
  comparison of the Office file is done at the normalized IR level, not bytes.
- **Fuzzing** (`cargo-fuzz`) on input parsers from Phase 8 onward; parsers must
  never panic on corrupt input: always `Err`, never `panic!`.

## 7. Specific rules for AI agents

1. **Do not expand scope.** If the task asks for Phase N, do not advance Phase
   N+1 work "while you are at it". The plan defines order by real dependencies.
2. **Do not change the DocMark specification to make a test pass.** If the
   format cannot represent something, document the limitation and use the
   `raw-block` mechanism described in the specification.
3. **Never degrade fidelity silently.** Every information loss in a conversion
   must emit a structured warning (see `ConversionReport` in
   `docs/architecture.md`).
4. **Do not add heavy dependencies without justification.** The goal is a single,
   reasonably small binary. Before adding a crate: is it maintained? is it
   pure-Rust? what does it add to binary size? Leave the justification in the PR.
5. **Document when you finish.** If your change alters visible behavior (CLI,
   format, MCP tools), update README.md and the corresponding document under
   `docs/` in the same PR. Recording the change in the knowledge base with
   `kb_add_document` is mandatory for every change, not just visible ones —
   see MANDATORY 5 in the section above.
6. **Verify against the three mental platforms.** Paths with `std::path` (never
   concatenate with `/`), line endings (the DocMark serializer always emits
   `\n`; the parser accepts `\r\n`), and no POSIX tool dependencies in production
   code.
7. Work on branches and push to the branch you are told; do not push to `main`. Branches are
   **jj bookmarks** here (`jj bookmark create`, `jj git push --bookmark`) — see §4.1, and never
   reach for a `git` command that writes.

## 8. Definition of Done

A task/phase is considered finished when:

- [ ] It compiles and passes `cargo test --workspace` on all three CI platforms.
- [ ] `clippy` and `fmt` are clean.
- [ ] New tests cover the added behavior (including error cases).
- [ ] Golden files and corpus are updated if applicable.
- [ ] Documentation is updated (README, docs/, CLI `--help`).
- [ ] The acceptance criteria for the corresponding phase in
      `docs/development-plan.md` are marked and verified.
