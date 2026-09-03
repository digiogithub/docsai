---
tags:
    - phase-17
    - mcp
    - cli
    - docmark
    - parser
    - validation
---
# 88 — Phase 17 F: `validate_docmark`, and the line between an error and a warning

Increment **17-F** of [[85-phase-17-plan]]. It validates; it does not fix and it does not write.
No `apply_edits`, no etags, no transaction, no cheat-sheet — those are 17-B…E and 17-G.

The problem it solves is one round trip. An agent that writes DocMark and hands it to
`convert_from_markdown` gets back a single `ParseError`: one line, one sentence, and no way to know
whether fixing it finishes the job. It fixes, calls again, fixes, calls again. `validate_docmark`
answers the same question the parser answers, in the form an editor wants — the **list**, each item
with a place, a stable code, the rule from `docs/docmark-specification.md`, and a fix.

## The decision that shapes everything else: the parser is the only authority

There is no second grammar in `crates/docsai-docmark/src/validate.rs`. Every call runs
`parse_with_base` on the caller's exact text, and the severity of every diagnostic is defined
against what it did:

* **Error** — *the text will not parse.* An error can only be constructed inside the `Err` arm of
  that match. This is structural, not a convention: there is no code path that emits an error about
  text the parser accepted.
* **Warning** — *it parses, and you probably did not mean it.* Warnings are computed from the
  `Document` the parser built and from the `ConversionReport` it produced while building it, so a
  warning describes something that **happened**, never something that might.

That is the line the increment asked for, and it is drawn by the parser rather than by a table of
opinions here. It matters because DocMark is deliberately tolerant of Markdown typed by hand
([[54-phase-14-parser]]): an unknown container class, a colliding id, a missing front matter all
parse, and calling any of them an error would be a lie.

### The line is not the same in both profiles, and that is the point

The first thing the property test found was that the two profiles disagree about an unclosed
`:::` container, and that a hand-written table of severities would have been wrong for one of them:

| | text profile | presentation profile |
|---|---|---|
| `::: {.x}` never closed | `parse_fenced_container` runs off the end of the body and swallows the rest of the document into the container. It **parses** | `split_one_fence` returns `unclosed fenced container`. It **does not parse** |

So the same finding is an `error` in a deck and a `warning` in a text document, with the message
changing to match — `the document does not parse` versus `everything after it is read as its
content`. Both sentences are true, and neither is decided here.

## Getting a list out of a parser that stops at the first thing

Two mechanisms, and neither of them speculates.

1. **Unclosed constructs are located all at once.** `scan_unclosed` walks the body with the fence
   rules copied from `split_one_fence`, including its reading of a line of four or more colons as a
   *close*, and reports **every** container and code fence left open, each at its own opening line.
   Three unclosed containers are three diagnostics, not one.
2. **The document is repaired and re-parsed exactly once.** The missing closers are appended — a
   repair that can only ever close what is open, and that moves no existing line — and the parser is
   run again. Whatever it refuses next is a second problem in the caller's own text, reported beside
   the first. Two guards keep it honest: an error landing past the end of the original is an
   artefact of the repair and is dropped, and a `(line, code)` already reported is not repeated. The
   pair `(line, code)` rather than `line` alone is deliberate — a container that is both unknown and
   unclosed is two problems at one line.

Beyond that the list stops. Deleting a bad attribute block and re-parsing to "find the next one"
produces errors the caller never wrote, and a false error is the same failure as a missing one
(AGENTS.md §7 rule 3).

## The taxonomy

Errors — the parser refused, and `classify` gives its message a code and a fix:

| code | rule | fix offered |
|---|---|---|
| `front-matter/unclosed` | §2 | end it with a line containing exactly `---` |
| `front-matter/unsupported-version` | §11 | the versions this build accepts, listed |
| `front-matter/unknown-paper-size` | §2 | a name docsai knows, or explicit `width`/`height` |
| `front-matter/wrong-shape` | §2 | write the key as an indented mapping |
| `parse/front-matter` | §2 | *none* — the YAML message already says what it wanted |
| `container/unclosed` | §3.6 | add the `:::` where it ends |
| `container/unknown-class` | §3.6 | the seven classes a text container may carry |
| `container/expected` | §3.6 | open it with `::: {` |
| `fence/unclosed` | §7 | close it with the same backticks |
| `list/empty` | §3.3 | give it an item or delete it |
| `sheet/heading-missing`, `sheet/empty-header` | §4 | the heading, the header row |
| `raw-block/no-base-dir` | §7 | validate the file rather than the text, or inline the payload |
| `io/unreadable`, `asset/unreadable`, `parse/unsupported`, `parse/unexpected` | — | coarse fallbacks |

Warnings — it parses, and it does not mean what it looks like:

| code | rule | how it is known to be true |
|---|---|---|
| `container/unclosed`, `fence/unclosed` | §3.6, §7 | the scan, when the parse succeeded anyway |
| `id/duplicate` | §11.1 | **from the IR**: two addressable nodes came back carrying one id. The text is consulted only to say *where* |
| `id/above-next-id` | §11.1 | the declared `next-id` read from the front matter, against the ids the document actually holds |
| `version/ids-need-1-1` | §11.1 | `docmark: "1.0"` declared, and the parsed document carries an id |
| `front-matter/absent` | §2 | there is no front matter; the defaults that fills in are named |
| `front-matter/version-missing` | §2 | no `docmark:` key |
| `container/unknown-class` | §3.6 | text profile only, and only for a class that is neither a container class nor a name the front matter declares in `styles:`, `list-definitions:` or `attribute-sets:` |
| `parse/degraded` | §0 | **the parser's own `ConversionReport` warnings**, verbatim. Authoritative by construction: it emitted them while reading this text |

`fix` is `Option`: a suggestion that cannot be made honestly is omitted rather than invented. Two
codes deliberately carry none — `parse/front-matter`, where the YAML error already names what it
wanted, and `parse/unexpected`, the fallback that by definition does not know what went wrong.

`node` is the id in force at the place: the id written on the offending line (`{#n7}` **or**
`list-id=n7`, since spec §11.1 puts a list's id in the latter), else the id on the opening line of
the innermost container still open there. Never invented; a construct with nowhere to hold an id
gets `None`.

Diagnostics come back in document order with the unlocated ones last — "somewhere in this document"
is not a place in it.

## The property, and what it found

`crates/docsai-docmark/tests/validate_agrees_with_the_parser.rs` asserts
`validate(text).parses() == parse(text).is_ok()` over **every** `*.expected.dmk.md` in the corpus
and over eight mutations of each — the mistakes an agent actually makes: drop a container closer,
drop the front matter's closing `---`, drop a code fence closer, duplicate a line carrying `{#n…}`,
invent a container class, declare a version that does not exist, append an empty bullet, misindent
the YAML. 442 cases over 60 goldens, of which 204 genuinely break the document. Two more tests ride along: every
error on unparseable text has a line, and a warning never appears on text that does not parse.

What it found, in order:

1. The text/deck asymmetry above. The first draft called an unclosed container an error in both
   profiles; the test refused it, and the severity became a function of the verdict.
2. **All 21 pptx goldens carry a true warning**: `assets/_skeleton/deck-<hash>.pptx` is referenced
   by `skeleton:` and is not in the corpus, so `parse_deck` warns that the preserved package is not
   beside the document. The warning is correct — writing one of those goldens back would lose the
   package it was cut from. Not fixed here: the corpus is not this increment's territory.
3. **Defect, not fixed:** the *text* parser drops an unknown container class **silently** when the
   container's body flattens to exactly one block (`parser.rs:746`), and errors only when it does
   not. The deck parser warns in the same situation (`deck_parser.rs:434`). A class quietly
   discarded is AGENTS.md §7 rule 3's silent degradation. Fixing it means adding a `Warning` to the
   text parser, which changes `inspect --json` output and its goldens — out of scope for 17-F, so
   `validate_docmark` reports it as `container/unknown-class` (warning) and the defect is recorded
   here. Repro: `docsai validate` on a file containing `::: {.sidebar}` / blank / `A` / blank /
   `:::` reports the warning; `docsai convert` on the same file says nothing.

## Cost, measured

One tool, two fields, no profile switch — the validator detects the profile exactly as the parser
does and *reports* which one it read, so there is nothing to switch.

| | preamble tokens | suite total |
|---|---|---|
| before | 2 124 | 5 239 |
| after | 2 228 | 5 355 (+2.2 %) |

**+104 tokens** in the session preamble, against 261 for 16-G's `render_slides`. Inside the wire
gate's 5 % allowance without touching `MAX_INFLATION` and without
`DOCSAI_ACCEPT_TOKEN_INFLATION`; the golden was regenerated the ordinary way. `deck-retitle` is
1 242 of its 2 000. Four of those 104 tokens are the one clause added to the server instructions
(`Also: validate_docmark, …`) which every scenario pays in the handshake — the price of the tool
being findable.

## Two doors, one function

`docsai validate <file> [--json]` and the `validate_docmark` MCP tool both call
`docsai_docmark::validate_file` and render through the **same** `docsai_mcp::validation_output`, so
neither door can drift into saying something the other does not. The CLI exits 1 when there is any
error and 0 when there are only warnings. `docsai-docmark` became a direct dependency of
`docsai-mcp` and `docsai-cli`: an internal workspace crate, not a new dependency and not a new
crate (AGENTS.md §3, §7.4).

The text form is what crosses the wire, not the JSON — one line per problem is what an agent reads,
and it costs roughly half of what the object costs to say the same thing (E1).

## Worked example: one golden, broken three ways

`corpus/pptx/basic-slides.expected.dmk.md` and `corpus/docx/basic-text.expected.dmk.md`, edited the
way an agent plausibly would:

```text
$ docsai validate break1.dmk.md        # added a shape container, forgot the closing :::
1 problem(s) — deck profile, 1 error(s), 0 warning(s)
error 22:1 container/unclosed #n9 — the `:::` container opened on line 22 is never closed;
  the document does not parse (spec §3.6) Fix: add a line containing exactly `:::` where the
  container opened on line 22 ends

$ docsai validate break2.dmk.md        # copy-pasted a slide, id and all
3 problem(s) — deck profile, 0 error(s), 3 warning(s)
warning 26:1 id/duplicate #n1 — `#n1` addresses 2 different nodes; an edit aimed at it cannot
  say which (spec §11.1) Fix: give the second one a fresh id — `{#n5}` — and raise `next-id:` to 6
warning 28:1 id/above-next-id #n7 — `#n7` is at or above the declared `next-id: 5` …
  Fix: set `next-id: 8` or higher
warning - parse/degraded — asset assets/_skeleton/deck-66e95c57.pptx: the preserved package is
  not beside the document (spec §0)

$ docsai validate break3.dmk.md        # two invented container classes, one never closed
3 problem(s) — text profile, 3 error(s), 0 warning(s)
error 53:1 container/unclosed — … Fix: add a line containing exactly `:::` …
error 53:1 container/unknown-class — unknown container class ["callout"] (spec §3.6)
  Fix: drop the class, or use one DocMark defines …
error 57:1 container/unclosed — … Fix: add a line containing exactly `:::` …
```

Break 2 is the case the increment exists for: the document converts, and three separate things
about it are wrong. Break 3 is the other one: three problems, two of them at one line, in a single
call.

## What was refused

* **A looser grammar.** Every severity is the parser's verdict; nothing is decided by a table here.
* **Speculative recovery.** One repair-and-re-parse, of appended closers only. Deleting the
  offending line to "find the next error" would manufacture errors the caller never wrote.
* **A `profile:` argument** on the tool. It would cost preamble tokens to let a caller assert
  something the parser decides for itself, and a wrong assertion would be a validator that
  disagrees with `convert`.
* **Warnings on text that does not parse.** Nothing is known about what an unparseable document
  means; guessing is the false warning the increment forbids.
* **Fixing the silent unknown-class drop** in the text parser (defect 3 above) — a parser behaviour
  change with golden fallout, in an increment whose job is to report.
* **Touching the corpus** to make the skeleton warning go away.

## Files

* `crates/docsai-docmark/src/validate.rs` — the module, and the doc-comment that states the line.
* `crates/docsai-docmark/tests/validate_agrees_with_the_parser.rs` — the property.
* `crates/docsai-mcp/src/tools.rs` — `ValidateDocmarkArgs`, `tool_validate_docmark`,
  `validation_output`.
* `crates/docsai-mcp/src/server.rs` — the handler and the 44-word description.
* `crates/docsai-cli/src/main.rs` — `docsai validate`.
* `docs/architecture.md` §6 and §9.6 — the tool surface.
* `crates/docsai-mcp/tests/goldens/wire-cost.md` — regenerated, +2.2 %.

## Next

17-C — the transaction.
