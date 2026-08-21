---
tags:
    - mcp
    - tokens
    - agent-loop
    - plan
---
# 58 — MCP token efficiency: measured waste and the plan to remove it

The MCP surface of [[28-phase-11-mcp]] gives an agent the right primitives (`outline_document`,
`search_document`, `read_selection`) but delivers them through a response shape that pays for the
same bytes twice, and keeps two tools — `convert_to_markdown` and `convert_from_markdown` — whose
contract is "the document travels through the context window". This document records what was
measured on the current build, and the increments that fix it.

Everything below was measured against `target/release/docsai mcp` (rmcp 3.1.0) driven by raw
JSON-RPC over stdio, and against the CLI for the same inputs.

## What was measured

| Observation | Evidence |
|---|---|
| **Every response is serialized twice.** `CallToolResult::structured` (rmcp 3.1 `model.rs:3964`) sets `content: vec![ContentBlock::text(value.to_string())]` *and* `structured_content: Some(value)`. Both cross the wire on every call. | `outline_document` on `corpus/docx/basic-styles.docx`: the JSON body appears once as an escaped string in `content[0].text` and once as an object in `structuredContent` |
| **`outline_document` is not cheap on a deck.** Full-depth outline of `corpus/pptx/forty-slides.pptx` = **2418 tokens** against a document of **2597** — 93 %, before the ×2 duplication. `--depth 1` is 1138. | `docsai outline corpus/pptx/forty-slides.pptx --json` |
| The tool description claims outline "costs a few percent of the document" (`server.rs`, `outline_document`). On the 40-slide deck that claim is false, and an agent that trusts it burns the document to read its map. | same |
| **`convert_to_markdown` cannot write.** Its arguments are `path`, `content_base64`, `filename`, `fidelity`, `assets`, `assets_dir`, `include_images` — there is no output path. The full DocMark always returns in the response, ×2. The CLI has had `-o/--output` since Phase 6; the tool never got it. | `crates/docsai-mcp/src/server.rs:41-65`, `tools.rs:22` |
| **`convert_from_markdown` cannot read.** `markdown: String` is a required argument, so writing a 30-page report back means the agent emits the whole document as *input* tokens. `convert_from_markdown` already accepts an output `path`; the input side has no equivalent. | `server.rs:127-141`, `tools.rs:195` |
| **`tools/list` costs ~7 KB** on every session, largely from seven copies of the same `path` / `content_base64` / `filename` / `fidelity` argument prose. | `tools/list` response = 6984 bytes |
| **No `tokens` tool.** `docsai tokens` exists in the CLI (Phase 10 D) and is the one call that answers "can I afford to read this?" for a fraction of an outline. It is not exposed over MCP. | `crates/docsai-cli/src/main.rs:182` |
| **No editing surface.** Changing one style or one heading requires `read_selection` → rewrite → `convert_from_markdown` of *something*, and there is no in-place path. `apply_edits` is plan v2 Phase 17 and still unopened. | `docs/development-plan-v2.md:303` |

What is already right and should not be re-litigated: `include_images` defaults to `refs`
([[28-phase-11-mcp]]), `read_selection` returns self-contained DocMark with per-node etags
([[25-phase-11-read-select]]), and `read_path_with_options` (`pipeline.rs:176`) parses
`.dmk.md` as a first-class input — so **`outline` / `search` / `read --select` already work on a
DocMark file**, not only on the Office original. That last fact is the backbone of the plan: the
agent's loop becomes *convert once to a file*, then address the file.

## The contract this plan moves to

1. **A conversion tool returns a path and a receipt, never a document.** Bytes go to disk; the
   agent reads what it needs with the addressing primitives, or with its own file tools.
2. **A response carries one representation, not two.**
3. **Every response has a bounded size**, and says what it omitted.
4. **The map is cheaper than the territory, at every document size** — if an outline is not, it is
   truncated and says so.

## Increments

> **Status**: E1–E5 delivered ([[59-mcp-e1-e5]]), E7 and E8 delivered ([[60-mcp-e7-e8]]).
> **E6 is the one still open.** E7's "under 4 KB" target below was wrong and is restated as
> under 7 KB in [[60-mcp-e7-e8]], with the arithmetic that shows why.

Numbered E1–E8; E1–E4 are the ones that pay for themselves immediately and depend on nothing.

### E1 — One representation per response

`json_result` (`server.rs:352`) stops using `CallToolResult::structured`. Build the result by hand:
`content` carries the compact text render (`Outline::render_text`, `SearchResults::render_text`, and
new equivalents for the other tools), `structured_content` stays `None` unless
`DOCSAI_MCP_STRUCTURED=1` is set for clients that consume it.

- Halves every response on the current corpus with no other change.
- Test: `stdio_protocol.rs` asserts `structuredContent` is absent by default and present under the
  env var, and that `content[0].text` parses back to the same facts.

### E2 — `convert_to_markdown` writes to a file

Add `output_path: Option<String>` (validated with `validate_output_path`, parents created, atomic
temp-file + rename). When present, the response omits `markdown` entirely and returns:
`{ output_path, source_format, bytes_written, document_tokens, node_count, assets_dir, image_count,
image_bytes, report }` plus a `next` hint naming `outline_document` on the written file.

- Keep the inline return only when `output_path` is absent **and** the result is under a cap
  (`DOCSAI_MCP_MAX_INLINE_TOKENS`, default ~2000); over the cap without an output path is a typed
  error that names the fix, not a silent 50 k-token answer.
- Same shape for the `content_base64` input case: the caller must give an `output_path`.

### E3 — `convert_from_markdown` reads from a file

Add `markdown_path: Option<String>`, mutually exclusive with `markdown`; exactly one required. The
`assets` array gains the same treatment: `assets_dir: Option<String>` so media is picked up from
disk instead of base64 in the arguments.

- With E2 this closes the loop: `docx → file → edit the file → file → docx`, and the document text
  never enters the context window unless the agent chooses to read it.

### E4 — Bounded, honest `outline_document`

- Default `depth` becomes 1 (was: all levels) — the map first, the sub-map on request.
- New `max_nodes` (default 200) and `cursor`, with `omitted` in the response, mirroring the
  `limit`/`omitted` pair `search_document` already has.
- `preview` truncated to a configurable width (default ~60 chars), measured in tokens, not bytes.
- New `outline-ratio` field = `outline-tokens / document-tokens`, and the tool description states
  the measured ratio range instead of the false "a few percent".
- CI gate: extend the corpus token-budget golden ([[18-phase-10-token-gate]]) with an
  **outline-ratio ceiling** per document class; a deck whose default outline exceeds 15 % of the
  document fails the build.

### E5 — `document_tokens` as its own tool

Expose `docsai tokens` as `estimate_tokens` (path or base64, per fidelity level): a few dozen tokens
that tell the agent which fidelity and which strategy the document can afford. Include the
per-fidelity table (`full` / `agent` / `standard` / `plain`) in one response so the choice is one
call, not four.

### E6 — Style reading and style editing without reading the document

The gap the user named: "modify styles without reading the whole document."

- `read_styles` — the style cascade only: named styles, their properties, and the node count using
  each. It is `inspect_document`'s style section, promoted, with usage counts added. Answers "what
  does this document call its heading style" for ~100 tokens.
- `set_style` — a *narrow, non-transactional* precursor to Phase 17's `apply_edits`: change the
  properties of a named style, or re-map nodes from one style to another (the `--style-map`
  machinery of `style_map.rs`, Phase 6, applied in place). Targets by style name, not by node, so it
  needs no etag per node and no IR patch language.
- Both are read/write against a `.dmk.md` file *or* an Office package, atomic write, `dry_run`
  returning the count of affected nodes.

E6 deliberately stops short of general patch editing so it can land before Phase 15/17.

### E7 — Trim the tool surface

- Factor the repeated argument prose (`path`, `content_base64`, `filename`, `fidelity`) into one
  sentence each, referenced rather than repeated; the schema descriptions are per-field and per-tool
  today.
- Shorten the six long tool descriptions to two sentences plus the cost note. The server
  `instructions` string is the right place for the loop narrative, and it is sent once.
- Target: `tools/list` under 4 KB with two tools *added*. **Revised on measurement** to under
  7 KB — met at 6 470 bytes; see [[60-mcp-e7-e8]] for why 4 KB would have meant an undocumented
  surface rather than a cheaper one.

### E8 — A measured gate, so this does not regress

A scripted scenario suite under `crates/docsai-mcp/tests/`, driven over real stdio, that records the
**total bytes and tokens crossing the wire** for the canonical agent tasks of plan v2 (§"Project
tracking metrics"): retitle a slide in a 40-slide deck, add a row to a sheet, fix a typo in a
report, restyle every `Heading 2`. Goldens committed; CI fails on regression. The numbers are the
deliverable — "token cost is not estimated, it is measured" applies to the transport too.

## Order, and what depends on what

| Increment | Depends on | Effort | Payoff |
|---|---|---|---|
| E1 one representation | — | S | ×2 on every call, immediately |
| E2 convert writes a file | — | S–M | removes the largest single response |
| E3 convert reads a file | — | S | removes the largest single *request* |
| E4 bounded outline | — | M | makes the map cheap again at deck scale |
| E5 estimate_tokens | — | S | one call replaces a speculative read |
| E7 trim schemas | E2, E3, E4 (arguments change) | S | fixed per-session cost |
| E6 style tools | E2/E3 for the file-first loop | M–L | the named use case |
| E8 wire-cost gate | all of the above | M | keeps it |

E1–E5 and E7 are a single self-contained pass; E6 is the one that needs design review against
Phase 17's operation set so `set_style` does not become an orphan API when `apply_edits` lands.

## Breaking changes, stated up front

- `convert_to_markdown` no longer returns `markdown` for a large document without an `output_path`
  (E2) — same class of documented break as `include_images` defaulting to `refs` in Phase 11 H.
- `outline_document` defaults to `depth: 1` (E4). A client that wants the old behaviour asks for it.
- `structuredContent` disappears unless requested (E1).

All three are the same trade the project already made once: **silence now means cheap.**
