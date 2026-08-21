---
tags:
    - mcp
    - tokens
    - agent-loop
---
# 59 — MCP efficiency E1–E5: one representation, and documents that stay on disk

Increments **E1–E5** of [[58-mcp-token-efficiency-plan]], implemented in one pass over
`crates/docsai-mcp` plus one addition to `docsai-convert`. E6 (style tools), E7 (schema trim) and
E8 (wire-cost gate) are not in this pass.

## What changed, and what it bought

Measured over raw JSON-RPC on stdio against `target/release/docsai mcp`, on
`corpus/pptx/forty-slides.pptx` (2 597 DocMark tokens):

| Call | Before | After |
|---|---|---|
| `convert_to_markdown` | 16 923 bytes (whole deck, twice) | **263 bytes** with `output_path` — a receipt |
| `outline_document` (default) | 24 626 bytes, every level, twice | **3 173 bytes**, one level, once |
| `estimate_tokens` | did not exist | **310 bytes**, four fidelity levels priced |

### E1 — one representation per response

`CallToolResult::structured` (rmcp 3.1 `model.rs:3964`) sets `content: vec![text(value.to_string())]`
**and** `structured_content: Some(value)`. Every response therefore crossed the wire twice, once
escaped and once as an object, and a client that reads the text block paid for an object it never
looked at.

`DocsaiServer::result` now builds the block itself. Tools return a [`ToolOutput`] — the JSON *and*
the text — and the text is what is sent; `DOCSAI_MCP_STRUCTURED=1` adds the object back for a client
that consumes it.

The text is **not** always the JSON. Where a result already had a compact reading form, that form is
the text: `Outline::render_text`, `SearchResults::render_text`, the DocMark of a selection with its
etags above it, the four lines of a budget. `convert_*`, `inspect_document` and
`list_supported_formats` keep JSON as their text, because their shape *is* the answer.

### E2 — `convert_to_markdown` writes a file

New `output_path`. With it the tool routes through `convert_file` / `convert_bytes` — the same
engine the CLI uses, so raw sidecars and the skeleton package are written exactly as `docsai convert`
writes them — and the response is a receipt: `output_path`, `bytes_written`, `document_tokens`,
`assets_written`, `assets_dir`, `report`, and a `next` naming the primitives. `markdown` is **absent**,
not empty: a field that is not there cannot be read by accident.

`options.target` is pinned to `Format::DocMark` rather than inferred from the extension, so
`output_path: "notes.txt"` writes DocMark instead of failing on an unknown format.

Without an `output_path` the DocMark still comes back inline, but only under
`DOCSAI_MCP_MAX_INLINE_TOKENS` (default 2 000, `0` disables). Over it the call fails with the
argument that fixes it named in the message. This is the E-pass's loudest break, and it is
deliberate: the old behaviour was a 50 k-token answer to a question the client did not know it was
asking.

### E3 — `convert_from_markdown` reads a file

New `markdown_path`, exclusive with `markdown`; exactly one is required. `markdown_path` **requires**
an output `path` — converting from a file only to hand back base64 would put the document back in
the context window through the other door. The conversion goes through `convert_file`, which resolves
media relative to the `.dmk.md` (`parse_with_base`), so a document written by E2 converts back with
its images without a single base64 row.

`assets_dir` was added to the inline branch too: media come from a directory instead of an array of
base64 payloads.

### E4 — an outline that is cheaper than the document

The old default was *every level*, and on the 40-slide deck that cost 2 418 tokens against a document
of 2 597 — 93 %, while the tool description claimed "a few percent". Now:

- `depth` defaults to **1**; `depth: 0` is how a caller asks for every level.
- `max_nodes` (default 200) and `cursor` window the top-level nodes, with `omitted` and `next-cursor`
  in the response — the pair `search_document` already had. The window unit is a top-level node:
  half a subtree is not a map of anything.
- `preview_chars` truncates previews below the 60 the serializer already caps them at.
- `outline-ratio` reports `outline-tokens / document-tokens`, so the claim is in the response instead
  of in the prose. `outline-tokens` is recomputed **after** truncation: the number describes what was
  sent, not what was built.

The deck's default outline is now 0.438 of the document — 40 slides listed for a document that is
only 2 597 tokens is inherently map-heavy, and the honest number is better than the old promise. A
per-class ratio ceiling in CI belongs to E8, where the corpus goldens live.

### E5 — `estimate_tokens`

`docsai-convert::tokens::token_budget_input` reads the document **once** and serializes it at all four
levels from the same IR, reporting total / front-matter / body / bytes / addressable nodes per level.
The MCP tool is a thin wrapper. On `long-report.docx` the whole answer is four lines:

```text
docx o200k_base
full 9083 tokens 42727 bytes 25 nodes
agent 8744 tokens 41748 bytes 25 nodes
standard 8698 tokens 41703 bytes 0 nodes
plain 8545 tokens 41321 bytes 0 nodes
```

`nodes: 0` at `standard` and `plain` is the fact that matters as much as the price: those levels
write no ids, so `read_selection` has nothing to address there.

## Shape of the code

The six argument structs moved from `server.rs` into `tools.rs`, and every tool now takes
`(&Args, &McpConfig)` instead of up to eight positional `Option<&str>`. `server.rs` is left with what
it should own: the router, the descriptions, and how a result crosses the wire. `DocumentArgs` is
shared by `inspect_document` and `estimate_tokens`.

## Traps met on the way

- **Argument order is part of the error message.** `markdown_path` first validated the file and only
  then noticed the missing output `path`, so a caller who forgot `path` got "no such file" about a
  file that was not the problem. Precondition checks come before I/O.
- **`tools/list` grew, not shrank**: 6 984 → 9 394 bytes, one more tool with more arguments. That is
  E7's job and it is now overdue, not accidental.
- The old positional signatures were what made the tools hard to extend; converting to structs was
  not tidying, it was the precondition for E2 and E3.

## Not in this pass

E6 (`read_styles` / `set_style`), E7 (schema trim), E8 (wire-cost goldens in CI). Also **untouched**:
`docx_roundtrip_is_idempotent` fails on `corpus/docx/footnotes.docx` at `HEAD` and still fails — a
footnote reference is written as `{#n2}` on the first pass and `{#n4}` on the second. Verified against
a clean checkout of `HEAD`: it predates this work and belongs to the docx footnote id assignment, not
to the MCP surface.
