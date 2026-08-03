---
created_at: 2026-08-03T13:16:56.892994484Z
updated_at: 2026-08-03T13:16:56.892994484Z
tags:
    - agents
    - docs
    - rules
    - agents.md
---
# 12 — AGENTS.md now carries MANDATORY operating rules for AI agents

## What changed

Added a clearly-labelled **"MANDATORY operating rules for AI agents"** section to
`AGENTS.md`, covering all seven canonical MANDATORY clauses, and merged (rather than
duplicated) them into the existing document. The section sits right after the intro and
before §1, so it has top prominence while keeping the existing §1–§8 numbering intact —
external references such as "AGENTS.md §3", "§5", "§6" and "rule 3 of AGENTS.md" in
`docs/` and `kb/` remain valid.

The seven clauses, adapted to this project's tools and conventions:

1. **Gather context before any task** — KB first (`kb_search_documents` /
   `hybrid_search_remembrances`, no `tags` filter), follow `[[wiki links]]` via the KB
   graph (`kb_get_document` / `kb_related_documents`), then the code index
   (`code_get_symbols_overview`, `code_find_symbol`, `code_hybrid_search`, `code_search_pattern`),
   and `recall` only when the key is known.
2. **External research** — Context7 (`c7_resolve_library_id` → `c7_get_library_docs`) for
   library/API usage (with a pointer to where dependency choices get recorded per
   `technical-analysis*.md`), web search + `fetch` for unknown facts, browser tools for any
   web-UI work (with a note that docsai ships no web UI today).
3. **Plan before non-trivial work** — identify the phase in
   `docs/development-plan-v2.md`, break into testable phases, save the plan to the KB under
   `kb/NN-<slug>.md`.
4. **Small, verified increments** — run §4 build/test/`clippy`/`fmt` after each increment,
   match §5 conventions, place tests per §6, never report done without evidence.
5. **Document every change in the KB** — `kb_add_document` after every change, capturing
   what changed / files & symbols touched / why / how verified, storing under `kb/<slug>.md`
   (or `kb/03-technical-decisions.md` for decisions), updating existing docs instead of
   duplicating, and linking with `[[wiki links]]`. Includes the plan-mode deferral note.
6. **Choosing the right memory tool** — `remember`/`recall` only for short keyed facts;
   `kb_add_document`/`kb_search_documents` for extensive or ordered content.
7. **General conduct** — English (§5 hard rule), parallelize without losing context and give
   sub-agents self-contained instructions, confirm irreversible/outward-facing actions
   (§7 rule 7: never push to `main`).

Two existing spots were strengthened rather than duplicated:
- §2 item 5 (`kb/`) now notes that recovering this context before a task is **mandatory**
  (points to MANDATORY 1).
- §7 rule 5 ("Document when you finish") now states that recording the change in the KB with
  `kb_add_document` is mandatory for every change, not just visible ones (points to MANDATORY 5).

No project-specific section (what the project is, documents to read, repository structure,
working commands, code conventions, testing strategy, rules for AI agents, definition of done)
was removed, weakened, or renumbered.

## Files & symbols touched

- `AGENTS.md` — new "MANDATORY operating rules for AI agents" section and two strengthened
  bullet/passages (no new code files).

## Why

The repository's AGENTS.md had an "AI agents" rules section but no explicit, mandatory,
tool-precise operating rules (context recovery, external research, planning, small verified
increments, KB documentation of every change, memory-tool selection, general conduct). The
goal was to make these mandatory and clearly labelled, merging with — not replacing — the
existing project content so nothing project-specific is lost.

## How it was verified

- Re-read the full AGENTS.md after editing.
- Confirmed every one of the seven canonical MANDATORY clauses is present, labelled
  "MANDATORY N —", and adapted to the project's actual tools and conventions.
- Confirmed §1–§8 and their numbering are intact, so external "AGENTS.md §N" references in
  `docs/` and `kb/` are unchanged and still valid.
- Markdown is well-formed (headings, lists, fenced code, block quote).

See also: [[AGENTS.md]], [[kb/README.md]], [[02-project-structure.md]],
[[03-technical-decisions.md]], [[11-plan-v2-onramp.md]].