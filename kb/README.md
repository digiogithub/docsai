# Knowledge base for `docsai`

Working documentation on **what is already built**, how it is organized, and what to keep in
mind when tackling the next phases. It complements — does not replace — the design documentation
in [`docs/`](../docs/), which describes the full project as originally conceived.

Practical difference between the two folders:

| Folder | Answers |
|---|---|
| [`docs/`](../docs/) | What we want to build and why: analysis, DocMark spec, architecture, phased plan |
| `kb/` (this one) | What is built today, how, and what is already known about what comes next |

## Index

| Document | Contents |
|---|---|
| [01 — Summary of phases 0 and 1](01-phases-0-1-summary.md) | What was delivered, acceptance criteria, what was left out and why |
| [02 — Project structure](02-project-structure.md) | Crates, modules, dependency rules, and where each piece enters |
| [03 — Technical decisions](03-technical-decisions.md) | The non-obvious decisions, with their rationale and cost |
| [04 — Considerations for the next phases](04-next-phases.md) | What Phase 2 and later will already find solved, and what awaits them |
| [05 — Phase 3 spreadsheets](05-phase-3-spreadsheets.md) | XLSX/XLS ⇄ DocMark: what landed, decisions, gaps |
| [06 — Phase 2 round-trip closure](06-phase-2-roundtrip-closure.md) | Residual DOCX writer fidelity + corpus idempotence |
| [07 — Phase 4 ODF](07-phase-4-odf.md) | ODT/ODS ⇄ DocMark: package, de-automatization, OpenFormula |
| [08 — Phase 5 legacy DOC](08-phase-5-legacy-doc.md) | `.doc` native degraded read + LibreOffice fallback |
| [09 — Phase 6 CLI & distribution](09-phase-6-cli.md) | inspect, batch, style-map, stdin/stdout, cargo-dist |
| [10 — Phase 7 MCP](10-phase-7-mcp.md) | `docsai mcp`, the first four tools, path/base64, limits |
| [11 — Plan v2 on-ramp](11-plan-v2-onramp.md) | What agent-native + presentations work reuses, what is new, traps that will bite again |
| [13 — Phase 10 plan](13-phase-10-addressing-plan.md) | Increments A–F of stable addressing and the token budget |
| [14 — Phase 10 A/B](14-phase-10-addressing-core.md) | `docsai-model::addressing`, ids in the IR, etags, risk-P7 property tests |
| [15 — Phase 10 C](15-phase-10-docmark-1-1.md) | DocMark 1.1: `{#n7}` emission and parse, `next-id`, `--ids` |
| [16 — Phase 10 D](16-phase-10-tokens.md) | `docsai tokens`: tokenizer decision, traced fragments, first budget numbers |
| [17 — Phase 10 E](17-phase-10-outline.md) | `docsai outline`: the containment tree, previews, the 5 % budget |
| [18 — Phase 10 F](18-phase-10-token-gate.md) | The corpus token budget golden, the CI gate, and the phase close |
| [19 — Phase 11 plan](19-phase-11-plan.md) | Projections, raw sidecar, `--fidelity agent`, `read --select`: the increments |
| [20 — Phase 11 A](20-phase-11-raw-sidecar.md) | Raw-block bytes move to `assets/_raw/`, and a missing sidecar is an error |
| [21 — Phase 11 B](21-phase-11-agent-fidelity.md) | `--fidelity agent`: the projection rule, what it enforces, and what the plan's criterion got wrong |
| [22 — Phase 11 C](22-phase-11-delta-emission.md) | Delta emission against the whole cascade, and the fixture that made it measurable |
| [23 — Phase 11 D](23-phase-11-attr-dictionary.md) | Repeated attribute patterns interned as `{.g1}`, and why expansion happens before interpretation |
| [24 — Phase 11 E](24-phase-11-readable-units.md) | Units chosen by what a length measures, `--precision`, and the tolerance that turned out to be zero |
| [25 — Phase 11 F](25-phase-11-read-select.md) | `read --select`: a selection that stands on its own, the minimum front matter, and where the etag finally gets written |
| [26 — jj, not git](26-jj-vcs.md) | This repository is driven by Jujutsu; what git misreports, what `git commit` left behind, and the command map |
| [27 — Phase 11 G](27-phase-11-search.md) | `docsai search`: why the unit is the block and not the addressed node, and the relative address prose gets |
| [28 — Phase 11 H](28-phase-11-mcp.md) | The three primitives over MCP, `include_images` as a ladder of cost, and the phase close |
| [29 — Phase 12 plan](29-phase-12-plan.md) | The three presentation spikes, what each has to answer before any code is written |
| [30 — Phase 12 A](30-phase-12-pptx-corpus.md) | The `corpus/pptx` decks, built by hand: one trait per file, and why they are generated |
| [31 — Phase 12 B](31-phase-12-spike-p1.md) | Spike P1: `ooxmlsdk` weighed against a parser of our own, and the decision |
| [32 — Phase 12 C](32-phase-12-spike-p2.md) | Spike P2: the DocMark-P draft, its eight rules and the residue probe |
| [33 — Phase 12 D](33-phase-12-spike-p3.md) | Spike P3: the preserved skeleton, and what LibreOffice accepts in silence |
| [34 — Phase 13 plan](34-phase-13-plan.md) | Increments A–K of reading a `.pptx` into the IR |
| [35 — Phase 13 A](35-phase-13-ir-root.md) | `Document::Presentation`: the third IR root beside text and workbook |
| [36 — Phase 13 B](36-phase-13-package-layer.md) | Parts, layouts, masters, slide order and sections |
| [37 — Phase 13 C](37-phase-13-slide-text.md) | The shapes and text of a slide |
| [38 — Phase 13 D](38-phase-13-cascade.md) | The placeholder cascade stored as reference + delta, never as a copy |
| [39 — Phase 13 E](39-phase-13-pictures-tables.md) | A slide's pictures and tables: the same `ImageRef` and `Table` a `.docx` carries |
| [40 — Phase 13 F](40-phase-13-notes.md) | Speaker notes, reached through the slide's own relationships |
| [41 — Phase 13 G](41-phase-13-reading-order.md) | A computed reading order that stays reversible because every shape keeps its tree index |
| [42 — Phase 13 H](42-phase-13-skeleton.md) | `Presentation::skeleton`: the original package kept whole in the asset store |
| [43 — Phase 13 I](43-phase-13-raw.md) | Groups, connectors, SmartArt and animations as stub + raw fragment + warning |
| [44 — Phase 13 J](44-phase-13-pptm-robustness.md) | `.pptm` as its macro-free equivalent, and a broken package that is never a panic |
| [45 — Phase 13 K](45-phase-13-inspect-inventory.md) | The `inspect` slide inventory, the refused conversion, and Phase 13 closed |
| [46 — Phase 14 plan](46-phase-14-plan.md) | Increments A–K of DocMark-P, the presentation profile |
| [47 — Phase 14 A](47-phase-14-spec-1-2.md) | DocMark 1.2 becomes normative: the eight rules, `layouts:`, `skeleton:`, the version rule |
| [48 — Phase 14 B](48-phase-14-deck-frontmatter.md) | A deck's front matter, and what each fidelity level writes of it |
| [49 — Phase 14 C](49-phase-14-slide.md) | The `##` slide heading, the consumed title, and the implicit body |
| [50 — Phase 14 D](50-phase-14-containers.md) | `::: {.ph}`, `{.shape}`, `{.connector}`: geometry in readable units with `emu` as the fallback |
| [51 — Phase 14 E](51-phase-14-notes.md) | Speaker notes: the one node whose syntax depends on the level |
| [52 — Phase 14 F](52-phase-14-objects.md) | Pictures, tables, groups and the stubs over the sidecar |
| [53 — Phase 14 G](53-phase-14-plain.md) | `plain` proven by the residue probe, and the three writer defects it found |
| [54 — Phase 14 H](54-phase-14-parser.md) | The deck parser, tolerant of Markdown typed by hand |
| [55 — Phase 14 I](55-phase-14-goldens.md) | Deck goldens and `serialize(parse(md)) == md`, byte for byte |
| [56 — Phase 14 J](56-phase-14-convert.md) | The deck converts: the refusal lifted, the skeleton written beside the Markdown |
| [57 — Phase 14 K](57-phase-14-p4-gate.md) | The P4 hand-edit gate, the second notes blockquote it caught, and Phase 14 closed |
| [58 — MCP token efficiency plan](58-mcp-token-efficiency-plan.md) | Measured waste in the MCP responses (double serialization, outline at 93 % of a deck, documents through the context window) and the E1–E8 fix |
| [59 — MCP efficiency E1–E5](59-mcp-e1-e5.md) | One representation per response, `output_path` / `markdown_path`, a bounded outline, and `estimate_tokens` |
| [60 — MCP efficiency E7–E8](60-mcp-e7-e8.md) | A trimmed `tools/list` (9 403 → 6 470 bytes) and a golden that measures the bytes and tokens each agent task puts on the wire |
| [61 — Phase 15 plan](61-phase-15-plan.md) | Increments A–K of the pptx writer and the anti-repair gate |
| [62 — Phase 15 A](62-phase-15-package.md) | The preserved package re-emitted entry by entry, bytes copied rather than re-deflated |
| [63 — Phase 15 B](63-phase-15-slides.md) | The text body as the unit of rewriting: everything around it stays the bytes that were read |
| [64 — Phase 15 C](64-phase-15-media.md) | Pictures, tables and media from the model, with `_rels` and `[Content_Types].xml` spliced |
| [65 — Phase 15 D](65-phase-15-raw.md) | A preserved object goes back as itself, at its original z-order |
| [66 — Phase 15 E](66-phase-15-notes.md) | The notes page, and `p:sldIdLst` restated when the model's slides are not the package's |
| [67 — Phase 15 F](67-phase-15-autofit.md) | The stale autofit scale dropped — the one thing this writer deliberately does not preserve |
| [68 — Phase 15 G](68-phase-15-template.md) | The deck with no skeleton: the template embedded in the binary, and the layout chosen from the content |
| [69 — Phase 15 H](69-phase-15-roundtrip.md) | `roundtrip` for decks, the per-category metric, and the two silent defects idempotence found |
| [70 — Phase 15 I](70-phase-15-xsd.md) | CI gate 1: the vendored ECMA-376 XSDs, and the two corpus fixtures they caught |
| [71 — Phase 15 J](71-phase-15-render.md) | CI gate 2: the render diff, its measured threshold, and the loader fault behind `soffice` |
| [72 — Phase 15 K](72-phase-15-close.md) | The P2 gate prepared and unrun, and Phase 15 closed with one box open |
| [73 — Phase 15 carried](73-phase-15-carried.md) | The shape added to a preserved slide, and `<a:p/>` surviving a round trip |
| [74 — The docx bookmark id](74-docx-bookmark-node-ids.md) | Why `docx_roundtrip_is_idempotent` was red on `footnotes.docx`, and the node ids that are not document content |
| [75 — Phase 16 plan](75-phase-16-plan.md) | Charts as data, objects confirmed end to end, comments, thumbnails and the cache |
| [76 — Phase 16 A](76-phase-16-chart-data.md) | The chart's series in the IR, its workbook as an asset, and the two copies of the truth compared |
| [77 — Phase 16 B](77-phase-16-chart-docmark.md) | The chart's series as a GFM table in DocMark, gaps that stay gaps, and the parser that reads a hand-typed one |
| [78 — Phase 16 C](78-phase-16-chart-write.md) | The chart's caches spliced inside the preserved part, `c:f` restated, the workbook preserved and named, and a chart generated whole |
| [79 — Phase 16 H](79-phase-16-cache.md) | The read cache: a hit indistinguishable from a miss, the key that carries the origin, and the asset store replayed |
| [80 — Phase 16 D](80-phase-16-opaque.md) | SmartArt, OLE, transitions and animations proved through the whole loop; the `p:oleObj` fixture; a generated deck that no longer drops a transition in silence |
| [81 — Phase 16 G](81-phase-16-thumbnails.md) | Slide thumbnails through LibreOffice, the word «thumbnail» taken back from the image rung, and a renderer probe that runs the tool instead of trusting the file |
| [82 — Phase 16 E](82-phase-16-comments.md) | Comments read from both formats into one IR node, DocMark 1.3 and the anchor-plus-container syntax, and the `w:commentReference` that was becoming a raw fragment |
| [83 — Phase 16 F](83-phase-16-comment-write.md) | Comments written back: the deck's comment parts spliced and still byte-identical, the docx thread through `w14:paraId`, the author table matched by name, and `p:cm/p:pos` proved to be master units and not EMU |
| [84 — Phase 16 close](84-phase-16-close.md) | The three criteria answered with re-run evidence, the thirty things carried forward, the three places a budget or a golden moved — and risk P2 still open, with what that means for Phase 17 |
| [85 — Phase 17 plan](85-phase-17-plan.md) | `apply_edits`: the nine operations, etags, the transaction, and the reading of the 2k-token budget decided before anyone is losing |
| [86 — Phase 17 A](86-phase-17-operations.md) | The operation set specified before it is written: ten names, what each does to ids, why a selector is not a target, the nineteen refusals, and 118 tokens for one edit |
| [87 — Phase 17 B](87-phase-17-etags.md) | The etag covers the node **and its subtree**, argued from what each choice does when it is wrong; the appearance it deliberately does not cover; total target resolution with six typed failures, and one etag of twenty-five moving through a real package |
| [88 — Phase 17 F](88-phase-17-validate.md) | `validate_docmark`: the parser is the only authority, so an error is text it refuses and a warning is text it reads as something else — the same unclosed container being an error in a deck and a warning in a text document; the list instead of the first failure; +104 preamble tokens |
| [90 — Phase 17 E](90-phase-17-doors.md) | One implementation, two doors: `docsai edit --ops` and `apply_edits` over one function in `docsai-mcp::edit`, with the equality asserted against the real binary and a real server rather than intended; the schema written for size — `operations` as an array of anything and ten shapes in one line of prose — **246 tokens**, +4.9 % of the gate's 5 % and no hatch; recalculation settled (xlsx `fullCalcOnLoad`, ods needs none, measured), so the false warning goes; the ODS `of:` namespace bug that made every formula this project wrote read as `Err:510` |
| [91 — Phase 17 G](91-phase-17-cheatsheet.md) | The contextual cheat-sheet as the MCP resource `docmark://cheatsheet/{path}`: the DocMark syntax a document actually uses, derived by walking the IR — **221 tokens** for a plain report against **914** for a deck with charts, a table, notes and a comment; a resource rather than a tool because a tool is published to every session, measured at **+4 preamble tokens** (2 477 → 2 481, suite +0.3 %); kept honest by three tests — every example parses, every specification heading is claimed or excluded with a reason, every entry is produced by a real document |
| [92 — Phase 17 H](92-phase-17-loop.md) | The loop, measured: `deck-retitle` completed and the acceptance number spent — **2 tool calls, 804 tokens** against 3 and 2 000, preamble **2 481** beside it, suite total 5 632 → 5 186 (**−7.9 %**), so the inflation hatch was authorised and **not needed**; §11's matrix walked as a constant, **72 cells**, every `—` a typed refusal that leaves the file byte-identical; the sibling criterion taken inside the changed part on all five families and the injected fault run on all five; three defects found by asking for the round trip the criterion asks for — `set_props` a silent no-op on docx and pptx (`run_direct` is the paragraph *mark*), `highlight` typed as a colour when the IR holds an enumeration, the ODS writer dropping a picture's `draw:name` and `svg:desc` on every write-back; ids on a package re-read settled as a property and written into §5.4.1 |
| [93 — Phase 17 close](93-phase-17-close.md) | The four criteria answered with re-run evidence — §11's matrix at **72 cells**, one member and one element changing in a deck, one `w:p` of five in a docx against the empty-batch baseline, the injected fault on all five families, and the number the phase existed for: **2 tool calls, 804 tokens** against 3 and 2 000, with the preamble reported beside it at **2 481** and the reading of the budget fixed in the plan before anyone could be losing; the wire-cost suite ending **below** where it started (5 239 → 5 186) with the inflation hatch authorised and unused; the loop's budget made a **hard gate** asserted before the golden; what `apply_edits` can and cannot reach on a filesystem; risk P2 still open and now more expensive; and the **thirty-nine** things carried forward |
| [94 — Phase 18 plan](94-phase-18-plan.md) | ODP ⇄ DocMark: the corpus first, the two mappings ODF and OOXML do not share, and the cross conversion whose drops must all be warned |
| [95 — Phase 19 plan](95-phase-19-plan.md) | Legacy `.ppt`, read-only: the LibreOffice route, `office_oxide` weighed before it is adopted, and the encrypted file that must be refused |
| [96 — Phase 18 A](96-phase-18-corpus.md) | The `corpus/odp/` fixtures: seventeen traits named after the pptx deck each mirrors, five that ODF cannot have and one it has alone, ODF's RELAX NG taught to `schema_gate.py`, and the slide comment no placement makes both legal and readable |
| [97 — Phase 19 A](97-phase-19-corpus.md) | The `corpus/ppt/` fixtures: recorded bytes with measured determinism, a truncation that is dangerous rather than decorative, the encrypted file's honest caveat, and what can and cannot gate a `.ppt` |
| [98 — Phase 18 B](98-phase-18-odp-reader.md) | The `.odp` reader: `presentation:class` mapped onto `PhType` with `text` and `page` keeping their own word, two `outline` frames on one page told apart by an ordinal that is **warned because the file does not state it**, reading order taken from the canvas since ODF has no `idx` to intend anything with; the plan's «ODF has one level» corrected — `style:master-page` and `style:presentation-page-layout` *are* the IR's master and layout, and only the per-page pairing has no home; charts read out of their own `local-table`, so nothing can go stale; the OLE object looked for before the replacement bitmap that would have swallowed it; seventeen `.expected.inspect.json` goldens whose warning lists are part of the golden |
| [99 — Phase 19 C](99-phase-19-parser-choice.md) | `office_oxide` weighed and **not adopted**: it exists, is actively released and is *cheap* — **+98 KB** stripped and **+30 s** clean release build, against `ooxmlsdk`'s +28.7 MB — so the rejection rests on behaviour, measured on 19-A's fixtures. `encrypted.ppt` comes back as two ordinary slides because `CurrentUserAtom.headerToken` is never consulted anywhere in the crate (upstream #119 is the open feature request), and `truncated.ppt` comes back `Ok` with one slide of **master placeholder prompt text** because a failed `offsetToCurrentEdit` falls back to scanning — criterion 3 unreachable, §7 rule 3 broken, and no panic either time, so memory safety is not the complaint; its `TextRun { text_type, text }` has no indent level and `\r`-joins a slide's paragraphs, and the seam's losses could not even be warned about honestly. Against it, `cfb` is already here and the walk is **~1 000–1 400 lines**, the size of the `.doc` reader. Plus the native route's shape for 19-D: a `ppt/` sibling of `doc/`, one `Window` type that is the only way an offset becomes bytes, **refuse rather than clamp** (the `.doc` piece table's `.min(len)` is the pattern that would ship the very failure just rejected), `recLen` checked before any seek, **no scan fallback**, and `headerToken` read first |
| [100 — Phase 18 C](100-phase-18-odp-docmark.md) | The `.odp` in DocMark-P: seventeen `.expected.dmk.md` goldens from the **same** deck writer, with `serialize(parse(md)) == md` byte for byte, and `the_deck_body_does_not_depend_on_the_package_it_came_from` proving the writer never learns which package it came from — the one `source_format` branch that existed, `looks_like_deck`'s `== Format::Pptx`, was already wrong and is now `is_presentation()`; `PhType::Other` settled by splitting the two ODF classes — `text` keeps its own word because PresentationML has none, and `page` is re-read as `sldImg` because both dialects name that construct and 18-B's own `chart:class` argument applies unchanged; three shared defects the new corpus surfaced and all fixed — a raw-block dropped as *«dialect is pml»* for ODF markup (a false warning), `list=` leaking into `standard`, and an image title that was residue at `standard` and a **silent loss** at `plain`, now in CommonMark's own title slot; the residue budget untouched at 15.1 % and the `.odp` corpus's 29.2 % explained rather than gated — `forty-slides` costs the **same 320 characters** of syntax in both packages and differs only in how much prose 18-A wrote |
| [101 — Phase 18 D](101-phase-18-odp-writer.md) | The `.odp` skeleton and writer: **byte identity is attainable and `mimetype` costs nothing** — the archive is copied entry by entry, so the first-and-stored member stays first and stored because it *was*, with no rule about it written anywhere; three ZIP fields put back by hand (external attributes, version made by, and the version needed to extract, which moves on exactly the one **stored** member ODF cares most about) and `charts-embedded.odp`'s `Object 1/` directory entry kept, which the OPC re-emission drops. `rebuilt_parts` is **one name, `content.xml`**, and the writer does not read it — it does not survive DocMark — but goes through the same door the reader did; the manifest is spliced without being on it, as `_rels` is on the pptx side. 15-B's rule with the one recipe ODF cannot copy: an ODF body is written against automatic styles the writer invents, so bodies are compared **rendered** rather than by bytes or by `Vec<Block>` (node ids a package never states would report an edit on every paragraph), and an absent placeholder index and index 0 are **one identity** — forty-one false warnings before that fix. Shapes pair by **position**, load-bearing: `presentation-classes.odp`'s `subtitle` comes back a body and an identity walk swapped two frames' text. 18-B's four findings closed, the `LayoutId` pair question closed by *measuring its cost to the package at zero*. Both gates learn the round trip: `schema_gate.py` validates the writer's output (469 parts, 106 `.odp`) and `render_gate.py` compares the `.odp` round trip (38 identical by bytes). Defects: a `.odp`'s preserved package was named `deck-<hash>.pptx` (**fixed**), and DocMark-P silently loses an empty body placeholder and an ODF `subtitle` class (**named, warned, carried**) |
| [102 — Phase 19 B](102-phase-19-loffice.md) | The LibreOffice route for legacy `.ppt`: `soffice --convert-to pptx` into the sandbox 16-G built — isolated `UserInstallation` profile inside the caller's temp dir, a polled 300 s ceiling, and the library path **derived from the resolved `soffice`** so the machine whose plain `soffice` cannot load its own libraries needs no path in the repository — then the Phase 13 reader **unchanged**, which is what makes criterion 1 a measurement. `.doc`→`.docx` and `.ppt`→`.pptx` become one `loffice_route` table, one `convert_with_loffice`, one `try_loffice`, one `--use-loffice`. «Equivalent» measured against the `.pptx` originals 19-A kept: slide count, order, titles, every bullet at its own indent level, the picture **byte for byte** and all document metadata survive; the layout's *name* and body placeholder index, alt text, shape name and placeholder identity do not, and every run comes back with explicit formatting — because **a `.ppt` has no OOXML layout parts at all**, so LibreOffice synthesises them. The content is equivalent; the package around it is LibreOffice's reconstruction, which is exactly what the one `Degraded` warning says and no more — a per-deck loss cannot be named without sometimes lying. `never` refuses through a real `ppt/` module (the 19-D seam) with `ReadError::NoNativeRoute` naming the increment, the flag and `DOCSAI_LIBREOFFICE`. **Criterion 3 is unreachable by this route**: `encrypted.ppt` converts with exit 0 and full content because LibreOffice never consults `CurrentUserAtom.headerToken` — the same failure `office_oxide` was rejected for — so 19-E's check must sit **in front of** the hand-off; `truncated.ppt` *is* refused, but only because `soffice` exits **zero** and produces nothing and the output-file check catches it. Defects fixed: the increment's own test wrote **fourteen** files into `corpus/` (`assets_dir: None` means beside the *input*) and a leftover `scratch_probe.rs` was the tree's sole `fmt` failure. `every_format_appears_exactly_once` moved 9 → 10, completing the `Format::Ppt` wiring that landed early in `958ac1e` |
| [103 — Phase 18 E](103-phase-18-cross.md) | The cross conversion, both directions: **both were broken and one lied** — `pptx -> odp` refused naming a `content.xml` the caller never had, and `odp -> pptx` wrote an ODF package under a `.pptx` name with a warning about slide naming. A preserved package of the wrong dialect is now never spliced into: both writers fall back on their embedded template and name the dialect the package was. 18-D's refusal of a template-less `.odp` is **reversed with its own argument** — a deck read from a `.pptx` has no ODF package at all — so `docsai-odf` gains a generated `default-template.odp` (source in `generate.py`, `--check`ed) and `write_gen.rs`, whose pages the splicer never touches, because splicing over a package just written from the model undoes what the first writing knew. The matrix is a **22-cell constant** (16 shared traits, 5 `.pptx`-only, 1 ODF-only) that asserts its own size and that every fixture of both corpora is in exactly one cell; the property is **the visible text as a multiset, minus the lines the cell declares lost, with the warnings covering the losses both ways round** — not byte identity, which two dialects cannot have. Nothing at all is lost going to `.pptx`; four things cannot cross into `.odp` (chart, OLE payload, ODF annotation, raw fragment) and each is a typed warning naming construct and dialect. Five defects found — an ODF package renamed `.pptx`, an invalid `p:ph type="text"` outside ECMA-376's closed enumeration (found by the gate), two generated placeholders **drawn on top of each other** because `p:spPr` was empty, every ODF table's header row lost silently, and a crossed numbered list coming back bulleted — all fixed. 18-D's two silent losses are **both one function's** (`implicit_shapes`) and both fixed with the specification's own words, moving two `.odp` goldens and no `.pptx` one. A sixth is argued rather than fixed: **DocMark-P carries no canvas size**, so a 4:3 deck crossing onto a 16:9 template puts a shape off the page — warned by name and in centimetres. Both gates grow a cross arm: 469 -> **691 parts** validated, 127 -> **254 pages** compared |
| [104 — Phase 18 close](104-phase-18-close.md) | The phase closed, with both criteria re-run here: **17 of 17** `.odp` fixtures identical on the second DocMark pass *and* byte for byte as packages, and the 22-cell cross matrix green with every crossing's drops named — in centimetres where it is a placement and by dialect where it is a vocabulary. The two mapping decisions stated where they will be looked for: `presentation:class` bends on the ODF side three times (eleven classes across, `text` and `sldImg` kept verbatim, an ordinal that is warned because no layout declares it, reading order from the canvas), and the plan's «ODF has one level above a page» was **wrong** — it has two references that are simply not nested. Byte identity for ODF cost **zero lines** for `mimetype` (bought by not rebuilding) and three ZIP fields plus one directory entry restored by hand. The gates: `schema_gate.py` 363 → **691 parts** against two grammars, `render_gate.py` 67 → **254 pages** — and the **spreadsheet gap is still open**, `xlsx/` and `ods/` with no oracle of any kind, one glob and one `draw:rotation-angle` fix from being closed. Every budget checked and none loosened: residue **15.1 %/12.2 %**, the token budget **down** 31 tokens at `standard` from a defect fix, the wire-cost suite and `LOOP_MAX_*` untouched at **5 186**. Risk P2 unsigned through a **third** phase, now on a fourth package family; **fifty-two** items carried forward, six dropped. The commits crossed with Phase 19 twice, and the close says so |
| [105 — Phase 19 D](105-phase-19-native.md) | The degraded native `.ppt` reader, so `--use-loffice never` **reads** rather than refuses: `cfb` plus 1 508 lines of our own atom walk, to the shape 19-C specified. Criterion 2 measured against the `.pptx` originals rather than asserted — **2/2 and 3/3 slides, every title and every bullet at its own indent level identical**, and the picture at the original's own position with the original's own content hash; against the LibreOffice route the only difference is the run formatting LibreOffice synthesises because a `.ppt` has no layout parts to inherit from. Boundaries come from [MS-PPT] 2.1.2 followed exactly — `Current User` → the `UserEditAtom` chain → merged persist directories newest-first → the `SlideListWithTextContainer` at **`recInstance` 0x000**, not 1 (the masters) — and text from the shape tree, because these decks' `SlidePersistAtom`s carry none; outline levels are **read** from `StyleTextPropAtom`, so nothing is flattened and nothing warns about flattening, and the bullet flag is believed only where the mask says it is stated. One invariant `Degraded` and, on both ordinary decks, **exactly one warning**. `truncated.ppt` is refused by our own code at `Window::slice(10946, 8)` on a 5 632-byte stream, with no scan fallback, and the fixture's *second* trap — the header at 5 300 declaring a body ending at 7 164 — is asserted directly, refused from the declared length alone. `encrypted.ppt` is read but never silently: `headerToken` first, a **`Severe`** warning, and the refusal left to 19-E because it must sit in front of both routes. Defect: **19-C's own picture warning was wrong** — a BLIP *is* anchored, through `pib` → `foDelay` → the client anchor — and emitting it as written would have been a false warning on every picture in every deck |
| [89 — Phase 17 C+D](89-phase-17-transaction.md) | Every operation applies: resolution records the route it walked and a policy-free walk replays it mutably, so there is one resolver and not two; all-or-nothing by ownership rather than rollback, a temporary file in the destination's own directory and an honest map of where `rename` is not atomic; `dry_run` stops one step short and returns a report equal byte for byte; the diff is identity and new etags, 31 tokens |

## Status in one line

**Phases 0–7 closed for the core path**: docx ⇄ DocMark with full corpus round-trip
identity (Phases 1–2, including floating DrawingML and footnote bodies), xlsx ⇄
DocMark plus xls read (Phase 3), odt/ods ⇄ DocMark (Phase 4), legacy `.doc`
read (Phase 5: native degraded + optional LibreOffice → docx), the full CLI
product surface (Phase 6: `inspect`, batch `--out-dir`, `--style-map`, stdin/stdout,
`cargo-dist`), and the MCP stdio server (Phase 7: four tools, path/base64; three more in
Phase 11).

**Phase 10 of plan v2 is closed**: DocMark 1.1 stable node ids and derived etags, `docsai tokens`
and `docsai outline` measured with a vendored BPE tokenizer, and the corpus token budget
(`corpus/token-budget.md`) gated in CI.

**Phase 11 is closed**: raw-block bytes live in `assets/_raw/` sidecars (11-A) and
`--fidelity agent` projects a document down to what a program can edit (11-B); no attribute
the inheritance chain already implies is written (11-C), and a pattern that repeats anyway is
written once in the front matter and referenced by class (11-D), and a length is written in the
unit of what it measures, exactly (11-E). `docsai read --select` hands over part of a document as
self-contained DocMark (11-F), and `docsai search` says where a document says something without
handing over the document (11-G). All three are MCP tools (11-H), where `include_images` now
defaults to `refs` instead of every image inline — a documented breaking change. Next is Phase
12: the presentation spikes.

**Phase 12 is closed**: the three presentation spikes (`docs/spikes/P1`–`P3`) — the parser
decision, the DocMark-P draft with its eight rules, and the preserved skeleton — with the
`corpus/pptx` deck corpus built by hand beside them.

**Phase 13 is closed**: `.pptx` **reads** into the IR. `Document::Presentation` is the third IR
root; `docsai-office::pptx` reads the package layer, the shapes and text of every slide resolved
against the placeholder cascade as a delta, the pictures, tables and speaker notes, in a computed
reading order that stays reversible. The original package is kept whole as `Presentation::skeleton`,
what the IR does not model comes back as a stub plus a raw fragment plus a typed warning, a `.pptm`
reads macro-free with a warning, and a broken package is a typed error and never a panic. `docsai
inspect` reports the slide inventory; converting a deck to DocMark was still refused here.

**Phase 14 is closed**: DocMark-P, the presentation profile. DocMark 1.2 is normative; a deck writes
its front matter, an `##` heading per slide with the title consumed and the primary body implicit,
a container for every other shape, notes in two syntaxes, and pictures, tables, groups and stubs.
`plain` is proven by a residue probe rather than asserted, the parser reads all of it back — a deck
typed by hand, with no attribute anywhere, parses — the seventeen decks are pinned by their goldens
with byte idempotence, and `docsai convert deck.pptx -o deck.dmk.md` works. Its fourth acceptance
criterion, the inherited `--fidelity agent` ≤ 15 %, is **not met** and is in the suite as an
`#[ignore]`d test rather than softened: closing it is a specification decision.

**Phase 15 is closed, with one item outstanding**: the pptx **writer**. The preserved package is
re-emitted byte for byte and only what the model changed is rewritten — a text body, a picture, a
table, a notes page, the slide list — with `_rels` and `[Content_Types].xml` spliced, raw objects
re-injected as themselves, and the stale autofit scale the one thing deliberately not preserved. A
deck with no skeleton is written on a template embedded in the binary. The whole corpus is
idempotent, seventeen decks byte for byte, and the two CI gates are real: the schema gate against
the vendored ECMA-376 XSDs (17 decks, 34 packages, 322 parts, 0 invalid) and the render gate through
headless LibreOffice. What stays open is in [`72-phase-15-close.md`](72-phase-15-close.md) and
[`73-phase-15-carried.md`](73-phase-15-carried.md): the P2 PowerPoint gate is **prepared and unrun**,
because running it needs PowerPoint and a person.

**Phase 16 is closed, and P2 is still that open item**: charts, embedded objects, comments,
thumbnails and the cache. A chart is now its categories, series, format codes, title and embedded
workbook, written into DocMark as a GFM table a person can edit and spliced back inside the
preserved `ppt/charts/chartN.xml` — a gap stays a gap, a chart nobody touched produces no edit, and
what cannot be spliced is refused by name. Comments read and write back in **both** formats through
one IR node and one syntax (DocMark **1.3**, spec §11.3), with the pptx comment parts spliced rather
than regenerated. SmartArt, OLE, transitions and animations were confirmed end to end with the
`p:oleObj` fixture the corpus never had. `--thumbnails` and the MCP `render_slides` tool draw a
slide through headless LibreOffice and `pdftoppm`, deciding availability by *running* the renderer
— which is what makes it work on a machine whose `soffice` exits 0 with a loader error — and
`include_images=thumbnails` became `downscaled`, a documented breaking change. A content-hash LRU
cache makes an agent's four questions about one document cost one parse, with a hit
indistinguishable from a miss. Twenty-one decks round-trip byte for byte; the schema gate validates
363 parts of 42 packages with 0 invalid; the render gate compares 67 pages with 0 failures. What
stays open is in [`84-phase-16-close.md`](84-phase-16-close.md), thirty items, the unsigned P2 form
first among them.

**Phase 17 is closed — docsai is a document tool, not only a converter.** A closed, versioned set of
**ten operations** (`docs/edit-operations.md`, version 1), specified before any of them was written,
defined against the IR, addressed by node id, guarded by **etags that cover a node and its whole
subtree**, applied **all-or-nothing** through a temporary file in the destination's own directory
renamed over the original — proven by *injecting* a fault after the operations applied and after the
complete file is on disk but before the rename, on all five package families. `dry_run` returns a
report equal to the real one; the report carries the created ids and the **new etags**, so no
confirmation read is needed. `docsai edit --ops` and the `apply_edits` MCP tool are two doors over
one function, asserted against the real binary and a real server. Beside them, `validate_docmark`
(`docsai validate`) answers with every problem in a piece of DocMark at once, with the parser as the
only authority, and the MCP resource `docmark://cheatsheet/{path}` carries only the syntax the
document at hand uses. The number the phase existed for: the scripted loop on a 40-slide deck costs
**2 tool calls and 804 tokens** against a budget of 3 and 2 000, and that budget is now a hard gate
asserted before the golden is consulted. The wire-cost suite ended the phase *cheaper* than it
started it (5 239 → 5 186) with the inflation hatch authorised and unused. What stays open is in
[`93-phase-17-close.md`](93-phase-17-close.md), thirty-nine items — the unsigned P2 form still first,
and now more expensive, because every operation reaches a package as a splice into a preserved part.

**Phase 18 of plan v2 is closed** ([`104-phase-18-close.md`](104-phase-18-close.md)): `.odp` reaches
parity with `.pptx`. The third ODF class reads into the same presentation IR, serialises through the
same DocMark-P writer with **no branch on the package**, and writes back as a splice into the
preserved archive — so an untouched `.odp` comes back byte for byte, `mimetype` first and stored,
and all seventeen fixtures are identical on the second DocMark pass. `pptx ⇄ odp` crosses through
the IR against a **22-cell matrix** that asserts its own size, with every drop named and no warning
for a loss that did not happen. Both gates learned ODF against a vendored OpenDocument 1.3 RELAX NG
grammar: **691 parts validated** (106 of them `.odp`) and **254 pages compared**, 0 invalid, 0
failures. The **spreadsheet gap is still open** and is now the only format family with no oracle at
all. Fifty-two items carried forward; risk P2 unsigned through a third phase.

**Plan v1 is delivered and deprecated.** Active plan:
[`docs/development-plan-v2.md`](../docs/development-plan-v2.md) — agent-native primitives
(Phases 10–11), presentations (12–16, 18–19), patch editing (17), hardening + v2.0 (20).
Start at [`11-plan-v2-onramp.md`](11-plan-v2-onramp.md) before touching any of it.

## Quick checks

```bash
cargo test --workspace                                  # workspace tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
python3 corpus/generate.py --check                      # corpus is up to date
cargo run -p docsai-cli -- formats                      # real support matrix of the binary
cargo run -p docsai-cli -- inspect corpus/docx/basic-text.docx
cargo run -p docsai-cli -- tokens corpus/docx/long-report.docx   # measured cost (Phase 10)
cargo run -p docsai-cli -- outline corpus/docx/long-report.docx  # the map an agent reads first
cargo run -p docsai-cli -- read corpus/docx/long-report.docx --select s2-s3  # part of it (Phase 11)
cargo run -p docsai-cli -- search corpus/docx/long-report.docx "rendimiento"  # where it says it
cargo run -p docsai-cli -- mcp   # stdio MCP server (Phase 7; all of the above are tools since 11-H)
```