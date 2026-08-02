# AGENTS.md — Guide for developers and AI agents

This file is the operational reference for anyone or any AI agent working
on the `docsai` repository. Read it fully before touching code.

## 1. What this project is

`docsai` is a cross-platform Rust binary (Windows/Linux/macOS) that converts
Office documents (`.doc`, `.docx`, `.xls`, `.xlsx`) and LibreOffice documents
(`.odt`, `.ods`) to an extended Markdown called **DocMark**, and back, with
minimal format loss. It is invoked as a CLI or as an **MCP server over stdio**.

**Current status**: **Phases 0–2 are closed** for the core path. The workspace
has the seven crates, the IR (`docsai-model`), the `.docx` reader **and writer**
(`docsai-office`), DocMark serialize **and parse** (`docsai-docmark`),
orchestration (`docsai-convert`), and the CLI with `convert`, `formats` and
`roundtrip`. `docsai-odf` and `docsai-mcp` remain skeletons that only fix the
dependency rules.

Next up is **Phase 3**: spreadsheets (`.xlsx` / `.xls` ⇄ DocMark).

## 2. Documents you must read before implementing

Mandatory reading order:

1. `docs/development-plan.md` — the phased plan. **Identify which phase the
   project is in before writing anything.** Do not implement items from future
   phases.
2. `docs/architecture.md` — workspace structure, IR model, contracts between
   crates.
3. `docs/docmark-specification.md` — the extended Markdown format. It is a
   contract: any change requires bumping the `docmark` field version in the
   front matter and documenting the migration.
4. `docs/technical-analysis.md` — why each library was chosen. Do not replace a
   key dependency (calamine, docx-rs, rmcp, comrak…) without leaving a written
   record of the reason in that document.
5. `kb/` — knowledge base of what is **already built**: summary of closed
   phases, real code structure, technical decisions with their rationale, and
   considerations for the phase you are about to tackle. Start with
   `kb/README.md`. If your change alters the structure or takes a non-obvious
   technical decision, update it in the same PR.

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
   `docs/` in the same PR.
6. **Verify against the three mental platforms.** Paths with `std::path` (never
   concatenate with `/`), line endings (the DocMark serializer always emits
   `\n`; the parser accepts `\r\n`), and no POSIX tool dependencies in production
   code.
7. Work on branches and push to the branch you are told; do not push to `main`.

## 8. Definition of Done

A task/phase is considered finished when:

- [ ] It compiles and passes `cargo test --workspace` on all three CI platforms.
- [ ] `clippy` and `fmt` are clean.
- [ ] New tests cover the added behavior (including error cases).
- [ ] Golden files and corpus are updated if applicable.
- [ ] Documentation is updated (README, docs/, CLI `--help`).
- [ ] The acceptance criteria for the corresponding phase in
      `docs/development-plan.md` are marked and verified.
