---
tags:
    - mcp
    - tokens
    - schemas
    - goldens
---
# 60 — E7 and E8: the session preamble, and a gate that measures the wire

Continues [[59-mcp-e1-e5]] against the plan of [[58-mcp-token-efficiency-plan]]. E1–E5 made every
*answer* cheap; E7 attacks the one response a session pays for before it has done anything, and E8
makes all of it a golden so it cannot quietly come back.

E6 (`read_styles` / `set_style`) is still open and was **not** started: it is the one increment the
plan flags as needing design review against Phase 17's `apply_edits`.

## E7 — what `tools/list` costs

| | bytes |
|---|---|
| before E1–E5 | 6 984 |
| after E1–E5 (one tool added) | 9 403 |
| after E7 | **6 470** |

Two independent halves, and the split matters because only one of them is safe to keep doing.

### Structure: `list_tools` is now overridden, not derived

`#[tool_handler]` only generates `list_tools` when the impl does not already have one
(`rmcp-macros-3.1.0/src/tool_handler.rs:64`), so overriding it costs nothing and keeps `call_tool`
and `get_tool` derived. Each published schema goes through `crate::schema::slim`, which removes
exactly four things:

- `"$schema"` — a dialect URI, 54 bytes × 7 tools, identical every time.
- `"default": null` on an optional field — `null` is what absence already means.
- `"type": ["string", "null"]` → `"string"` — `required` is what says a field may be omitted; the
  union only adds the right to *send* `null`, which no caller needs.
- `"format": "uint"` **when a `"minimum": 0` sits beside it** — not a format any validator knows,
  and the `minimum` already says the one thing it meant. Without the `minimum` it is left alone.

Field names, `required` and descriptions are never touched: those are what a caller reads to build a
call. This half is worth 9 403 → 8 020, and it is free — no information a client can act on is gone.

### Prose: shorter, and said once

The rest is editing. The `path` / `content_base64` / `filename` triple now reads the same three short
lines in all six tools, the eight tool descriptions lost their loop narrative (it lives in the server
`instructions`, which is sent once and is where an agent actually reads it), and the long
justification prose on `output_path` and `markdown_path` came down to what a caller has to decide.
8 020 → 6 470.

### The plan's 4 KB target was wrong, and here is the arithmetic

[[58-mcp-token-efficiency-plan]] set "under 4 KB with two tools added". Measured, the residue after
E7 is **3 897 bytes of structure** (field names, types, `required`, the `$defs` block for
`IncomingAsset`, and the JSON framing) plus **2 573 bytes of description** across 8 tools and 40
fields. Hitting 4 KB total would mean roughly 100 bytes of prose for the whole surface — about
2.5 bytes per field. That is not a cheaper listing, it is an undocumented one, and an agent that has
to guess an argument pays for the guess. The target is restated as **under 7 KB**, met at 6 470,
with the note that further cuts have to come from removing *arguments*, not from removing their
descriptions. The obvious candidate is `assets` (inline base64 rows), now that `assets_dir` exists;
that is a breaking change and belongs in its own increment.

## E8 — measuring the transport, not the tools

`crates/docsai-mcp/tests/wire_cost.rs` runs a real `rmcp` client against the server over a duplex
whose client half is wrapped in a counting `AsyncRead + AsyncWrite`. Every byte counted is framed
JSON-RPC that crossed the transport, both directions, **handshake included** — not a sum of what the
tool handlers returned. Tokens are `o200k_base`, the same encoding as every other budget here.

`crates/docsai-mcp/tests/goldens/wire-cost.md`, at the time of writing:

| scenario | request bytes | response bytes | total tokens |
|---|---|---|---|
| session-preamble | 295 | 7 534 | 1 863 |
| deck-retitle (read half) | 1 131 | 2 782 | 1 217 |
| report-fix-typo | 1 204 | 2 300 | 998 |
| sheet-add-row | 1 004 | 2 011 | 837 |

The gate mirrors the corpus token budget ([[18-phase-10-token-gate]]): regenerate with
`DOCSAI_UPDATE_GOLDENS=1`, and an update that inflates the suite total by more than 5 % is refused
unless `DOCSAI_ACCEPT_TOKEN_INFLATION=1` is set as well.

### What the numbers say

- **The session preamble is now the most expensive thing in the suite.** 1 863 tokens before an
  agent has asked a single question, against 998 for a whole typo-fix loop on a 30-page report. E7
  took a third off it and it is still the top row. The next real cut is fewer arguments, not shorter
  sentences.
- **The edit itself costs nothing.** In `report-fix-typo` the change is made with `std::fs` between
  two MCP calls, and that is not a shortcut in the test — it is the loop E2/E3 built. The return leg
  is a request naming two paths and a receipt.
- **A 40-slide deck is located and read for 1 217 tokens**, against 2 597 for the deck itself, and
  that includes pricing it, converting it to a file, mapping five nodes, searching it and reading a
  slide back.

### Two scenarios the plan asked for and this does not measure

- **restyle every `Heading 2`** — needs E6. Today there is no loop for it that does not read the
  document, so measuring it would record the absence of a feature, not a cost.
- **deck-retitle, return leg** — writing a pptx package is Phase 15 (`docsai formats` says
  `pptx  yes  no`). The deck scenario stops at the read and is labelled as the read half.

Both are named in the golden itself under "Not measured yet", so the file says what it does not
cover rather than looking complete.

## Traps met on the way

- **A golden that measures request bytes must not contain absolute paths.** The first version used
  a `tempfile::tempdir()`, so the recorded request size included the length of a random directory
  name — the golden would have described the machine. The test now `set_current_dir`s to the
  repository root and every path in a request is relative and fixed-length.
- **`poll_write` may accept fewer bytes than it was offered.** Counting `buf.len()` instead of the
  returned count would over-report the request side. Only `buf[..written]` is on the wire.
- **A scenario has to do something real.** The typo fix started as `replacen("the", "the", 1)`,
  which clippy correctly called `no_effect_replace`; the corpus report is in Spanish, and the fix
  is `tecnico` → `técnico`, asserted to have changed the file.

## Still open

- **E6** — `read_styles` / `set_style`, the use case that opened [[58-mcp-token-efficiency-plan]].
  Needs the design review against Phase 17 before it is written.
- The `assets` inline-base64 argument, whose removal is the next real cut to the session preamble.
- `docx_roundtrip_is_idempotent` still fails on `corpus/docx/footnotes.docx`, pre-existing and
  unrelated (recorded in [[59-mcp-e1-e5]]).
