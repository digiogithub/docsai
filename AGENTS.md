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
Office documents (`.doc`, `.docx`, `.xls`, `.xlsx`, `.ppt`, `.pptx`) and
LibreOffice documents (`.odt`, `.ods`, `.odp`) to an extended Markdown called
**DocMark**, and back, with minimal format loss — and, since Phase 17, **edits**
one in place through a closed set of operations without converting the whole of
it. It is invoked as a CLI or as an **MCP server over stdio**.

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
Current work is **Phase 15**: the pptx writer and the anti-repair gate (plan
`kb/61-phase-15-plan.md`). The first of eleven increments is done: **the package comes back out** —
`docsai_office::write_pptx` re-emits the preserved skeleton entry by entry, in the original member
order, with each entry's compression method, CRC and stored bytes copied rather than inflated and
re-deflated, which is what a `Package` rebuild cannot do. No slide is regenerated from the model
yet, and the writer says so with a warning instead of pretending otherwise; that is why `SUPPORT`
still answers `pptx: write no` and `convert … -o deck.pptx` is still refused. The second is done: **the slide comes from
the model** — the unit of rewriting is a text body, so a placeholder's `a:p` sequence is rebuilt
from the document and everything around it (`p:spPr`, `a:bodyPr` and its autofit, the shapes that
are not text, transition and timing) stays the bytes that were read. A deck nobody edited comes out
byte for byte as it went in; a retitled slide differs in the run that was retyped. `SUPPORT` now
says `pptx: write yes` and the convert pipeline routes it. What the writer cannot put back it keeps
and names in a warning: pictures, tables, charts and preserved objects (15-C, 15-D), notes pages
and added or removed slides (15-E), hyperlinks and fields. The third is done: **pictures, tables and
media** — a picture's bitmap, alt text, drawn size and crop, a table's cell text and a run's
hyperlinks all come from the model now, with the media re-injected as the bytes the asset store
holds (nothing re-encodes an image) and the parts that have to agree with it *spliced*: the slide's
`_rels` gains only the relationships the deck did not already have, `[Content_Types].xml` only a
`Default` for an extension it never carried. An unedited deck is still byte for byte the one that
was read, `_rels` included. A table whose grid moved is copied and named (15-G writes an `a:tbl`
from the model), and a link no relationship can state as it stands is refused rather than pointed
at nothing. The fourth is done: **raw re-injection** — a shape the IR keeps only as a stub over a
raw fragment (SmartArt, OLE, a group, a custom geometry) goes back as the original markup at its
original z-order, and is no longer reported as a loss, because the splice never touched it; a
payload the document carries that differs from the package's bytes is written back over the element
it belongs to, a slide-level fragment (`p:transition`, `p:timing`) at the level it came from. A
fragment in another dialect is dropped with `Warning::RawBlockDropped` rather than emitted as
nonsense, and one whose markup belongs to a part outside `rebuilt_parts` — a chart's XML — is
copied and named. The fifth is done: **notes pages and the deck's own slide list** — a slide's
notes page is rewritten from the model like any other text body, reached through the slide's own
relationships and never by number, while the printed page around it stays the bytes that were read;
and when the model's slides are not the package's, `p:sldIdLst` and `p14:sectionLst` are restated
in the model's order, a removed slide takes its part, its `_rels`, its notes page and its content
type out of the archive with it, and a reordered deck moves the list without touching a single
slide part. `ppt/presentation.xml` is otherwise copied, and it deliberately stays outside
`rebuilt_parts`: the IR holds two facts stated in it, not its content. Which package slide a model
slide *is* has to be recovered from the content, because nothing in this chain carries an identity
for a slide, and a slide the model **added** is refused by name — there is no part to splice it
into. The sixth is done: **the stale autofit scale on the way out** — the one thing this writer
deliberately does not preserve. `a:normAutofit@fontScale` is a number PowerPoint computed for the
text that was in the box when a human last edited it, so when a text body's text actually changed
the scale and its `lnSpcReduction` are dropped and `Warning::AutofitStale` names the shape and the
number (analysis §5.4). «Changed» is the writer's own notion of change and not a second one — the
body's new bytes differ from the bytes the part already said — so a shape nobody edited keeps its
scale and is **not** reported, and an unedited deck is still byte for byte the deck that was read.
The seventh is done: **the deck with no skeleton** — a deck that never had a package (Markdown typed
by hand, with no attribute anywhere) is written on a **template embedded in the binary**, generated
from source by `corpus/generate.py`: a master, four layouts, a notes master, a theme and no slides at
all. Each slide is written from the model against a layout chosen from the shape of its content — a
title with bullets is «Title and Content», a title alone is «Title Slide», a layout the model names
wins outright, and anything else falls back with a warning saying which layout it went on. The same
generation writes the parts 15-E refused by name: a slide the model **added** to a preserved deck now
gets a part of its own, spliced into the deck's slide list, its relationships and its content types
while every slide that was already there stays byte for byte the part that was read, and a note typed
onto a slide with no notes page gets one. A preserved deck is untouched by any of it: seeding runs
only when the model holds something the package does not.
The eighth is done: **`roundtrip` for decks, and the per-category fidelity metric** — `docsai
roundtrip deck.pptx` runs the full cycle, and the metric `docs/architecture.md` §4 has described
since Phase 0 is finally computed, for every format: a deck is counted as slides, shapes,
paragraphs, pictures, tables, notes and objects, each with both counts beside it rather than one
average. The whole pptx corpus is **idempotent** — seventeen decks, `.pptm` included, second pass
equal to the first, every category at 100 %, every written package byte for byte the package that
was read — and it was not when the question was first asked: the measurement found two silent
defects, both now fixed in the writer. The `zip` writer ORs `S_IFREG` into every entry's mode, which
changed the content hash of the preserved package and so renamed it on every write; and the writer
indexed the shape tree by `Shape::z_index`, a number DocMark does not carry, so a deck whose reading
order is not its z-order had every shape's text written into another shape.
The ninth is done: **CI gate 1, the schema gate** — the ECMA-376 XSDs are vendored unmodified in
`schemas/` (1.0 MB, from the standard's own distribution zips, with provenance and the Ecma
copyright notice beside them) and `corpus/schema_gate.py` validates against them with `xmllint`,
which is the §7 rule 4 justification: no crate is added and nothing reaches a shipped binary. What
it validates is not «the output of a round trip» — for a preserved deck that is byte for byte the
input, and measuring it would measure whoever wrote the deck — but every part whose bytes the writer
**changed or created**, found by diffing the written package against the one it was written on, over
an edited deck and over the same deck with its `skeleton:` removed so the whole package is generated
on the embedded template. A failure names the part, the line and the element, and so does a part the
validator reached no verdict on. It went red on its first run: two corpus fixtures were not legal
OOXML — a `p:txBody` with no `a:p` at all, and a DrawingML table cell holding a `p:txBody` where an
`a:txBody` belongs — both fixed in `corpus/generate.py`, both invisible to nine increments of
byte-identity tests because the writer reproduced them faithfully. The third finding was an
`mc:AlternateContent`, which is legal by ECMA-376 Part 3 and unrepresentable in the Part 4 schemas;
the gate now does the compatibility processing a consumer does instead of loosening anything.
The tenth is done: **CI gate 2, the render gate** — `corpus/render_gate.py` has headless LibreOffice
render each deck to PDF, `pdftoppm` rasterise every page to greyscale, and a low-frequency
perceptual diff (the RMSE over 8×8-pixel block means, at 100 dpi) score the pairs; a page over the
threshold of 2.5 fails the build naming the deck, the page and the number, and a deck whose page
count changed is a failure of its own kind, because there is no score for pages that cannot be
paired. The threshold is measured rather than guessed: `--self-test` prints every pair behind it —
0.0000 for the same file rasterised twice, 1.72 for anti-aliasing on against off, 3.55 for one text
line of twelve gone — and runs with no renderer at all, so the comparator is measured even where
LibreOffice is not. Like gate 1, it does not pretend to measure a byte-identical round trip: it says
so and spends no renderer time on it, and the path that does produce new bytes — the whole package
written on the embedded template — is checked for the two things a render can honestly say about it,
the same page count and no page that came out blank. A missing **or broken** renderer is a skip with
a banner and exit 3, and in CI, which installs LibreOffice deliberately, `--require-renderer` makes
it a failure. The gate **has now been run for real**: this machine's `libreglo.so` failure was a
dynamic-loader fault, not a broken install, and with `LD_LIBRARY_PATH=/usr/lib/libreoffice/program`
set, `python3 corpus/render_gate.py --require-renderer` exits 0 — *17 decks, 17 round trips
identical by bytes, 0 rendered, 61 pages compared, 0 failures, 0 not measured*. Read that precisely:
the round trips are byte-identical and take the short circuit, and the 61 rendered pages are the
generated packages, every one `diff n/a (geometry)` because the template's canvas is not the corpus
deck's. So LibreOffice opens and renders every generated package without failing and nothing came
out blank — and **no perceptual diff has ever been scored on real output**, which leaves the
threshold of 2.5 unvalidated against renderer noise. Without the variable the gate still skips with
its banner and exit 3, as it should.
The eleventh is done and **Phase 15 is closed — with one item outstanding, named rather than
buried**: the P2 gate is **prepared and unrun**. `docs/spikes/P2-powerpoint-gate.md` builds in one
paste the three decks that cover this phase's three paths to bytes — a preserved package spliced
with one rebuilt slide, a SmartArt deck whose diagram only PowerPoint resolves because LibreOffice
takes the `mc:Fallback`, and a package generated end to end on the embedded template that **no
application has ever opened** — plus a negative control, a valid deck with three parts removed and
their relationships left dangling, which `corpus/opc_check.py` refuses and which spike P3 §2.4
measured LibreOffice accepting in silence. Seven items, the repair dialog first. Running it needs
PowerPoint and a person, and this machine has neither, so **risk P2 stays open**:
`docs/technical-analysis-presentations.md` §7.1 states what the two gates do and do not prove and
carries the row that closes the risk when the form is signed — the LibreOffice half of that form is
now runnable here, the PowerPoint half is not, and LibreOffice was never the oracle anyway. Of the phase's five acceptance
criteria two are true (corpus idempotence; images without recompression, with the geometry tolerance
Phase 11-E fixed at zero), and three wait on an application opening a file — including «a deck
written from attribute-free Markdown opens correctly», **un-ticked here** because its verb is
«opens». What the phase carries forward is listed in `kb/72-phase-15-close.md`: the unrun gate, a
render gate whose threshold no real render has yet scored, the LibreOffice loader path, `<a:p/>` not
surviving a round trip, a
preserved table whose grid moved, a shape *added* to a preserved slide discarding that slide's other
edits, unvalidated preserved parts, `align`'s content-based pairing, the refused internal hyperlink,
the inherited `agent` ≤ 15 % criterion, and the red `docx_roundtrip_is_idempotent` on
`corpus/docx/footnotes.docx`. Three of those are now closed: the shape added to a preserved slide
and `<a:p/>` in `kb/73-phase-15-carried.md`, and the docx test in `kb/74-docx-bookmark-node-ids.md`
— the `.docx` writer was minting a Word bookmark from every allocated node id (`n7`), so a
paragraph's id survived a round trip while a footnote's, which has nowhere to be written, was handed
out afresh and renumbered around it. Only ids that are document content — a bookmark name read off
the source, which a TOC's `PAGEREF` fields address — are written back now.
**Phase 16 is closed**: charts, embedded objects, comments, thumbnails and the cache — the content
classes that survive as data rather than as opaque XML. It was **started with the P2 form still
unsigned**, on the owner's direction and recorded in `kb/75-phase-16-plan.md` rather than in a commit
message; what it owed in exchange it paid, because every increment kept byte-identity and the form is
re-runnable on any commit of the phase. A **chart** is now its categories, its series, its per-series
number format, its title and its embedded workbook, written into DocMark as a GFM table a person can
edit — categories down the first column, one column per series, an empty cell a *gap* and never a
zero — and an edited table is **spliced inside the preserved `ppt/charts/chartN.xml`**, with
`c:ptCount`, the sparse `c:pt@idx` and the `c:f` ranges kept consistent; a chart nobody touched
produces no edit and no warning, and what cannot be spliced (a series added or removed, the kind, a
`c:rich` title, an emptied axis, a multi-level axis) is refused by name. The chart's two copies of
the truth — the caches and `ppt/embeddings/` — are both read, the cache is authoritative because it
is what the slide shows, and a disagreement is warned rather than averaged; the workbook is preserved
rather than rewritten, because the model holds cached columns and not a spreadsheet. A deck generated
from Markdown gets a real chart part where it used to get a text box. **Comments** are read and
written back in *both* formats through one IR node and one syntax — DocMark **1.3**, specification
§11.3: a docx comment covers a range, a pptx comment is pinned to a point, a reply is the same
container nested inside the one it answers, and DocMark names an author rather than carrying a
package-local `authorId`, which is re-established on write by matching the name against the table the
package already has. The pptx comment parts stay outside `rebuilt_parts` and are spliced; what a
classic `p:cmLst` cannot hold is refused by name; PowerPoint's modern comments are preserved
unchanged and named as not read. **SmartArt, OLE, transitions and animations** were confirmed end to
end rather than assumed, with a `p:oleObj` fixture the corpus never had, in both the plain
`p:graphicFrame` spelling and the `mc:AlternateContent` one PowerPoint actually writes. A **slide has
a picture**: `--thumbnails` on the CLI and a `render_slides` tool on MCP, through headless
LibreOffice and `pdftoppm`, with availability decided by *running* the renderer and requiring it to
identify itself — an installed-but-mis-linked LibreOffice exits 0 and prints a loader error, and the
installation's own library directory is derived and applied rather than hard-coded, which also fixed
legacy `.doc` conversion on such a machine. `include_images=thumbnails` was renamed to `downscaled`,
a documented breaking change, because one word had come to name two different pictures. And a
**content-hash LRU cache** in `docsai-convert` makes an agent's four questions about one document
cost one parse instead of four, with a hit indistinguishable from a miss: the report is cached with
the tree, the asset store is replayed rather than bypassed, and the key covers the origin, the bytes
and every conversion option. The phase's three criteria are met and measured, `21` decks round-trip
byte for byte, `corpus/schema_gate.py` validates 363 parts of 42 packages with 0 invalid, and
`corpus/render_gate.py` compares 67 pages with 0 failures. **Risk P2 is still OPEN** — no application
of any kind but LibreOffice has opened any of it — and `kb/84-phase-16-close.md` records the close,
the thirty things it carries forward, and the question the wire-cost measurement raises for Phase 17:
the MCP session preamble now costs **2 124 tokens** where Phase 17's criterion budgets 2 000 for a
whole agent loop, and whether the two are the same budget is Phase 17's to decide.
**Phase 17 is closed**: patch editing — the change that moves docsai from converter to document
tool. Until this phase the only way to change a document was to convert the whole of it to DocMark,
edit the text and convert the whole of it back; that path is untouched and every fidelity property
still rests on it, but there is a **second door** beside it now. `docs/edit-operations.md` is
normative **version 1** of a closed set of **ten operations** — `replace_text`, `set_props`,
`insert_after`, `insert_before`, `delete`, `move`, `duplicate_slide`, `set_cell`, `set_notes`,
`replace_image` — specified in full **before any of them was written**, defined against the IR
rather than against text or markup, and addressed by **node id** or index path, never by a
selector. An unknown operation, an unknown field, a missing one and a mistyped one are each a typed
refusal naming both sides; nothing creates or edits a **comment** (a comment asserts that a named
person said something, and there is no person on the other end of an API call — deleting one is
different in kind and is allowed) and nothing touches a **chart** (its numbers are edited by
retyping its DocMark table, which is Phase 16's path). An **etag** covers a node **and its whole
subtree**, so an edit made inside a subtree between a read and a `delete` of it is detected rather
than destroyed, and a mismatch is `StaleEtag` carrying both values and never an overwrite; what an
etag deliberately does not cover is appearance, because `--fidelity agent` drops appearance by
design. The transaction is **ownership rather than rollback** — the operations apply to an in-memory
document a refusal simply drops — and the write is a temporary file in the destination's **own**
directory, `fsync`ed and renamed over the original, with the edges of that guarantee written down
(a crash never leaves half a document; durability under a reordering filesystem is not claimed;
Windows refuses when the file is open elsewhere; **no atomicity is claimed on NFS or SMB**). It is
proven by **injecting** a fault after the operations applied and after the complete edited file is
on disk but before the rename, on all five package families. `dry_run` runs everything except that
rename and returns a report **equal** to the real one. The report carries the ids created with
their kind, the targets deleted and the targets changed **with their new etags** — 31 tokens for a
two-target batch — which is what makes «no confirmation read is needed» a property rather than a
claim. `docsai edit <in> --ops <json>` and the `apply_edits` MCP tool are **two doors over one
function**, asserted against the real binary and a real server on a transport rather than intended.
Beside them: **`validate_docmark`** (and `docsai validate`) answers with *every* problem in a piece
of DocMark at once — line, stable code, node id, the specification rule and a fix — with the parser
as the only authority, so an error is text it refuses and a warning is text it accepts and reads as
something else; and the **contextual cheat-sheet**, the MCP resource `docmark://cheatsheet/{path}`,
carries only the DocMark syntax the document at hand actually uses (**221 tokens** for a plain
report against **914** for a deck with charts, a table, notes and a comment), as a resource rather
than a tool because a tool's schema is published to every session whether it is wanted or not.
**The phase's four acceptance criteria are met and measured**: §11's matrix walked as a constant
over **72 cells** (every `✔` a round trip, every `—` a typed refusal that leaves the file
byte-identical); an edit leaving every sibling byte-identical **inside the part it changed** on all
five families; the injected mid-transaction failure on all five; and the number the phase existed
for — a scripted «retitle slide 4, add a bullet, bold it, verify» loop on a 40-slide deck at
**2 tool calls and 804 tokens** against a budget of 3 and 2 000, with the session preamble reported
beside it at 2 481 and explicitly **not** part of that budget, the reading fixed in the phase plan
before any measurement existed. That budget is now a **hard gate**: `LOOP_MAX_CALLS` and
`LOOP_MAX_TOKENS` in `crates/docsai-mcp/tests/wire_cost.rs` are asserted *before* the golden is
consulted, so no golden regeneration and no environment variable can make the loop affordable. The
wire-cost suite ended the phase **cheaper** than it started it — 5 239 → **5 186** tokens, a net
fall, despite two new tools and a resource — and the inflation hatch, authorised for the last
increment, was **not used**. Four defects were found by asking for the round trip the criterion asks
for: `set_props` was a silent no-op on docx and pptx, `highlight` was typed as a colour when the IR
holds an enumeration, the ODS writer dropped a picture's name and description on every write-back,
and the ODS writer had never declared the `of:` namespace, so **every formula in every `.ods` this
project wrote read as `Err:510`** — all fixed. **Risk P2 is still OPEN and is now more expensive**:
every operation in the set reaches a package as a *splice into a preserved part*, which is exactly
the construction `docs/spikes/P2-powerpoint-gate.md` asks about, and the form is still unsigned
after two phases. `kb/93-phase-17-close.md` records the close, the criteria with re-run evidence,
where every budget stands, what `apply_edits` can and cannot reach on a filesystem, and the
**thirty-nine** things carried forward.
**Phase 18 is closed**: `.odp` ⇄ DocMark, the third ODF class brought to parity with `.pptx`.
`docsai-odf` read and wrote text documents and spreadsheets since Phase 4 and *refused* a
presentation; a `.odp` now reads into `Document::Presentation` — the **same** IR root a `.pptx`
reads into, which gained nothing structural — and serialises through the **same** DocMark-P writer,
which needed no branch on the source format and gained none. That is a test rather than an
intention: `the_deck_body_does_not_depend_on_the_package_it_came_from` serialises every `.odp`
twice, once claiming each format, and asserts the bodies are identical. Pages, frames,
`presentation:class` as a placeholder type, master pages and presentation page layouts as the IR's
master and layout, nested lists, notes, comments, tables, pictures and embedded charts all read,
with ODF's automatic styles resolved into the model the way `.odt` and `.ods` already do, and
everything the IR has no node for kept as a stub over its own markup. **The two mappings the plan
refused to settle in advance were settled against files.** `presentation:class` versus `p:ph` bends
on the ODF side three times — eleven classes map straight across while `text` and the notes
thumbnail keep the word both dialects share, a second frame of one class on one page gets an
ordinal that is *warned* because no layout declares it and it therefore does not resolve against
the cascade, and reading order comes from the canvas rather than from an index this reader invented.
And the plan's «OOXML has two levels above a slide; ODF has one» was **wrong**: ODF has two
references above a page (`style:master-page` and `style:presentation-page-layout`), simply not
nested, which is why `LayoutPlaceholder::props` is empty and `Master::theme` is `None` for every
`.odp` on purpose. **The writer splices into the preserved archive**, copying each member's stored
bytes, compression method, CRC and timestamp, so an untouched `.odp` comes back **byte for byte** —
`mimetype` first and stored *because it was*, at a cost of zero lines, bought by not rebuilding —
and `rebuilt_parts` is one name, `content.xml`. Three ZIP fields and one directory entry are
restored by hand because the `zip` crate derives them. `docsai formats` says `odp: read yes, write
yes`. **`pptx ⇄ odp` crosses through the IR** against a **22-cell matrix** that asserts its own size
and that every fixture of both corpora is in exactly one cell; the property is the visible text as a
multiset minus the lines each cell declares lost, with **the warnings covering the losses exactly,
both ways round** — a drop with no warning fails, and a warning for a loss that did not happen fails
as hard. Nothing at all is lost going to `.pptx`; four things cannot cross into `.odp` and each is a
`Degraded` warning naming the construct and the dialect. **Both CI gates learned ODF** against a
vendored OpenDocument 1.3 RELAX NG grammar (`schemas/odf-1.3/`), with ODF's own extended-document
rule applied before validating: `corpus/schema_gate.py` is at **691 parts validated** (106 of them
`.odp`), 0 invalid, and `corpus/render_gate.py` at **38 round trips identical by bytes, 55 rendered,
254 pages compared**, 0 failures. **The spreadsheet gap is still open** — `xlsx/` and `ods/` have
neither gate, and are now the only format family with no oracle of any kind. The phase's two
criteria are met and re-measured at the close: 17 of 17 fixtures identical on the second DocMark
pass *and* byte for byte as packages, and the cross matrix green. Six shared defects were found and
fixed because a deck was finally made to cross — among them an image title that was residue at
`standard` and a **silent loss** at `plain`, an ODF table's header row lost on every write, and
`p:ph type="text"`, invalid OOXML written silently and caught by the new gate arm. **Risk P2 is
still OPEN**, unsigned after a **third** phase, and Phase 18 put a fourth package family on the
writer surface no application but LibreOffice has ever opened.
`kb/104-phase-18-close.md` records the close, both criteria with re-run evidence, where every budget
stands, what the gates cover and what they still do not, and the **fifty-two** things carried
forward.
**Phase 19 is closed**: legacy `.ppt`, **read-only and permanently so**. It is the `.doc` strategy
applied to a second binary format, and it is the *same code*: `loffice_route` is a two-row policy
table (`Doc → docx`, `Ppt → pptx`), `convert_with_loffice` is one sandboxed child under one
timeout, and `try_loffice` is one function under one `--use-loffice auto|never|require`, so a third
legacy format would be one line. With LibreOffice the deck is pre-converted and handed to the
**unchanged** Phase 13 `.pptx` reader — which is what makes «fidelity equivalent to the pptx path»
a measurement rather than a claim, taken against the `.pptx` originals the corpus keeps beside each
legacy deck: identical slides, order, titles, bullets at their own levels, metadata, and the same
content-hashed picture asset. What does **not** survive is the package around the content, because a
`.ppt` has no OOXML layout parts at all and LibreOffice therefore synthesises a master, a layout and
explicit run formatting; the route's single warning says exactly that and nothing file-specific, so
it can never be false. Without LibreOffice, a **native [MS-PPT] reader** (1 508 lines) walks
`CurrentUserAtom` → the `UserEditAtom` chain → the merged persist directory → the
`SlideListWithTextContainer` at `recInstance` 0, and recovers the slides in the deck's own order,
the text of every shape through the shape tree with its **outline levels** from
`StyleTextPropAtom`, and the pictures of the `Pictures` stream **placed** by the shape that shows
them (`pib` → `foDelay` → the client anchor). Its one `Degraded` warning lists an invariant set of
losses, and nothing warns about a loss that did not happen. **Bounds checking is structural, not
sprinkled**: one `Window` type is the only way an offset becomes bytes, it has **no clamping
accessor at all**, `#![deny(clippy::indexing_slicing)]` covers every submodule, `recLen` is
validated before any body is read, and there are depth, chain-length and run-length caps the
specification does not require. **`office_oxide` was weighed and not adopted** — +98 KB and +30 s,
so *not* on cost: it reads `corpus/ppt/encrypted.ppt` back as two ordinary slides and
`truncated.ppt` as one slide of placeholder prompt text, both silently, and its model has no
outline level to lose (`docs/technical-analysis.md` §4.6). **An encrypted or protected `.ppt` is
refused in front of the choice of route** — `docsai_office::refuse_if_encrypted`, called before
either route is picked, because LibreOffice never consults `CurrentUserAtom.headerToken` and a
refusal living inside the native reader would leave `--use-loffice auto` reading encrypted decks on
every machine that has LibreOffice. The message names the file, the token, the fact that docsai
decrypts nothing on either route, and what to do instead. `.ppt` is also the **first format in this
repository with no schema gate, no render gate and no OPC check** — none of the three is applicable
to a binary record stream that is never written — so `corpus/generate.py`'s structural assertions
are its only gate, and the reader's evidence is a comparison against `.pptx` originals rather than a
golden. `kb/106-phase-19-close.md` records the close, the three criteria with re-run evidence, the
security posture Phase 20 inherits, and the **sixty-four** things carried forward.
Next is **Phase 20** (hardening) — and, first, the P2 form, unsigned after a **fourth** phase, which
should be signed before a hardening phase measures only the properties this repository already knows
how to measure.

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
python3 corpus/schema_gate.py          # CI gate 1: the .pptx parts we write, against the ECMA-376 XSDs
python3 corpus/render_gate.py          # CI gate 2: the render diff, before and after the round trip
python3 corpus/render_gate.py --self-test   # its comparator alone, no renderer needed
```

The schema gate needs `xmllint` (Debian/Ubuntu `libxml2-utils`) and a built CLI
(`cargo build -p docsai-cli`). Its schemas are vendored in `schemas/`; see `schemas/README.md`.
The render gate needs `soffice` and `pdftoppm` (`libreoffice-impress poppler-utils`) and the same
CLI; without a working renderer it **skips** with a banner and exit 3 — never a green tick — and in
CI it is run with `--require-renderer`, where a missing renderer is a failure.

On this machine `soffice` fails with `error while loading shared libraries: libreglo.so` unless the
library directory is named: the Linux branch of the `/usr/lib/libreoffice/program/soffice` wrapper
never exports `LD_LIBRARY_PATH` and `soffice.bin`'s `RUNPATH $ORIGIN` is not honoured here. Nothing
is missing — prefix the command and it works:

```bash
LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer
```

(Fixing it permanently is an owner action needing root: `/etc/ld.so.conf.d/libreoffice.conf`
containing `/usr/lib/libreoffice/program`, then `ldconfig`.)

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
