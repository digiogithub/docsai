# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Every formula in every `.ods` this project wrote read as `Err:510`.** The ODS writer emits
  formulas as `table:formula="of:=…"` but never declared the `of:` (OpenFormula) namespace on
  `office:document-content`, and an undeclared prefix inside that attribute is not a formula at all.
  A plain round trip of `corpus/ods/formulas-basic.ods` produced a spreadsheet whose every formula
  cell displayed an error in LibreOffice. No test caught it because the ODS tests compare DocMark,
  which round-trips the formula text perfectly whether or not any application can evaluate it, and
  there is no render gate for spreadsheets. One namespace declaration.
- **`w:commentReference` and `w:annotationRef` became raw fragments.** Word never writes a
  `w:commentReference` as a child of `w:p` — it is always inside a `w:r` — and `read_run` had no
  arm for it, so it fell through to the raw branch: every `.docx` with a comment showed a
  `::: {.raw}` block holding `<w:commentReference w:id="1"/>` in the middle of a sentence. The
  paragraph-level match that was supposed to catch it also carried a `Warning` saying comments were
  «out of scope in v1», which therefore **never fired** — a false claim and a missing warning in the
  same three lines. Both are gone, and `w:annotationRef` — the comment's own mark, which says
  nothing the model does not know from *being* a comment — is ignored in the same arm as
  `w:footnoteRef` instead of becoming a raw block of its own.
- **A selection lost the comment its text was under review by.** The comment container is written
  after the body, outside every fragment, so `docsai read <in> --select <sel>` produced
  `[text]{.comment ref=c1}` with nothing declaring `c1` — the specification's own «anchor with no
  definition» case, in which the annotation is silently gone. `select.rs` already had exactly this
  repair for footnotes and now has it for comments, bound by `ref=` because the anchor carries no
  id. A selection carrying a comment also declares `docmark: "1.3"` now, in whichever profile it is
  in, instead of `1.2` or `1.1`.
- **`p:cm/p:pos` is not in EMU.** It is PowerPoint's master unit, 1/576 in — an eighth of a point —
  and reading it as EMU put every comment marker in the top-left corner of the slide. Corrected from
  files rather than from prose: ECMA-376's own example `x="1426" y="660"` is 1/640 of an inch from
  the corner as EMU and 2.48 × 1.15 in as master units; an `.odp` with one annotation at 5 cm,
  converted by LibreOffice, comes out as `x="1134"`, which is exactly 1134 × 1/576 in; and
  LibreOffice's exporter says so in source (`convertMm100ToMasterUnit`). The round trip is exact for
  the values a producer writes and rounds back to itself for the rest.
- **A generated deck dropped `p:transition` and `p:timing` in silence.** Slide-level markup hangs off
  `p:sld` *beside* the shape tree, and `write_gen::slide_xml` walked only the tree — the same blind
  spot the reader had until Phase 13-I, on the write side. A deck written without its preserved
  package now reports each of them. It is a warning rather than a rewrite: splicing a preserved
  subtree into a slide built on a different master and theme is a design decision, not a bug fix.
- **A chart nobody edited used to report itself as a loss.** Every chart in every deck raised «its
  chart: it was copied from the preserved package», on decks where nothing had been copied instead
  of anything. The writer now asks «did this change?» exactly once, in the model's own units, by
  running the model's own chart reader over the preserved part — so an untouched chart produces no
  edit to any part and no warning at all.
- **A non-image asset was named `img-…`.** The asset store learned only bitmaps, so a chart's
  embedded `.xlsx` workbook would have been stored as `img-48b54984.bin` in every manifest, every
  `assets/` directory and every DocMark reference. `sniff` now recognises a ZIP and, inside it, an
  OPC package, by scanning the local file headers — no unzip, and none wanted in `docsai-model` —
  and a non-image is named `asset-<hash8>.<ext>`.

- pptx writer: a shape added to a preserved slide no longer discards that
  slide's other edits. The shape that cannot be added — and, symmetrically, one
  the model removed — is named on its own, and every other edit to the slide is
  written. A pairing with more than one answer is still refused rather than
  guessed.
- pptx writer: an empty paragraph spelt `<a:p/>` survives a round trip. The
  writer compared a rewritten text body against the part's bytes rather than
  against what the part says, so a deck using that spelling came back changed on
  a shape nobody had edited — and reported a stale autofit scale that was not.
- **The `.docx` writer no longer writes DocMark's own node ids into the package
  as Word bookmarks.** A node id has two provenances and only one of them is
  document content: a name read off a `w:bookmarkStart` in the source
  (`_Toc234329254`) is what a TOC's hyperlinks and `PAGEREF` fields address the
  paragraph by, and it is still written back; a counter id the allocator handed
  out (`n7`) is DocMark's own address, derived deterministically from document
  order, and nothing in the package refers to it. Writing the counter ids back
  invented bookmarks the document never had and broke round-trip idempotence:
  only a paragraph and a heading own a `w:p` to carry one, so their ids stuck
  across a round trip while a footnote's, a table's and an image's were handed
  out afresh, and the second pass numbered the rest around the survivors. It is
  what had kept `docx_roundtrip_is_idempotent` red on `corpus/docx/footnotes.docx`
  since commit `88ff8e11`, where the bookmark path was added: its two footnotes
  came back as `{#n4}` and `{#n5}` where the first pass had said `{#n2}` and
  `{#n4}`.

- **Two corpus decks were not schema-valid, and the packages written from them inherited it.**
  The new schema gate found both on its first run, in fixtures nine increments of writer tests had
  round-tripped without complaint. `corpus/generate.py` gave a shape with no text a `p:txBody` with
  **no `a:p` at all**, which `CT_TextBody` forbids and no application writes; and it built a
  DrawingML table cell with a `p:txBody` where an `a:txBody` belongs — same content model, different
  namespace, and the XSD is the only thing that ever said so out loud. Both are fixed in the
  generator, the corpus is regenerated, and the pptx goldens are updated (the DocMark changes only
  on the `skeleton:` hash line; `shapes-geometry`'s inspection count goes from two paragraphs to
  three, which is the empty paragraph that shape always should have had). The writer itself was
  right in both cases: `write_gen` has always written `a:txBody` into a cell.

- **A written `.pptx` is now the package that was read, byte for byte.** The `zip` writer ORs
  `S_IFREG` into every entry's mode, so a deck's central directory came out with a permission bit it
  did not go in with — fifteen bytes no OPC consumer reads. It renamed the deck: a preserved package
  is addressed by the content hash of its bytes (`skeleton: assets/_skeleton/deck-<hash>.pptx`), so
  converting a deck back and forth minted a new copy of the package every time and made every round
  trip look like an edit. The external file attributes are now restored entry by entry after the
  archive is built.
- **A `.pptx` whose reading order is not its z-order is no longer written with its text permuted.**
  The reader returns a slide's shapes in reading order and keeps the shape tree index in
  `Shape::z_index`; DocMark writes the shapes in reading order and does not carry that index, so a
  deck that travelled through DocMark arrived at the writer numbered by position. The writer took
  those numbers for tree indices and wrote every shape's text into a different shape's box —
  silently, and only for a deck whose two orders disagree. The map from a model shape to its element
  is now the package's own reading order, computed from the tree.
- **The `.pptx` writer names the slides whose part alignment was a guess.** Which package part a
  model slide *is* has to be recovered from what the slide says, because nothing in the chain
  carries an identity for a slide. When two or more slides were both **edited and reordered**,
  nothing is left to match on and the writer pairs them with the parts left over, in order. That
  pairing is a guess: the text still lands and the deck comes out in the model's order, but where
  the order is wrong a slide takes over another one's layout, its relationships and its notes page.
  It is now reported with `Warning::Degraded`, naming each slide and the part it was written into.
  A single slide reaching that last pass is a *forced* pairing — there is one free part — and says
  nothing, as does an unedited deck and a reorder with no edit.
- **The `.pptx` writer no longer names 15-G for what it now does.** The messages that pointed at a
  future increment for a table whose grid moved and for a notes page the deck does not have say
  what the writer actually does instead — an `a:tbl` is written from the model only onto a slide
  the writer generates, and a notes page is refused only when there is no notes master to hang one
  from.

### Added

- **`docsai edit <in> --ops <json>` and the `apply_edits` MCP tool** (plan v2 Phase 17, increment
  E): the closed operation set of `docs/edit-operations.md`, applied in order and all-or-nothing,
  through **two doors over one implementation**. The answer is the change report and nothing else —
  the ids of what was created with their kind, the targets deleted, and the targets changed **with
  their new etag** — which is what makes the phase's claim that no confirmation read is needed true
  rather than asserted: an agent that retitled `n12` holds `n12`'s new etag and can guard its next
  edit with it. `--dry-run` / `dry_run` runs everything except the replacement of the file and
  returns a report equal to the real one, and `--output` / `output_path` writes elsewhere and
  leaves the input byte-identical. A refusal is **typed at both doors** — `{"index":0,
  "op":"set_notes","refusal":"StaleEtag","target":"n4","expected":"000000","actual":"da5e97",
  "message":…}` — and never a sentence alone; on the CLI it also exits `2`. Neither door carries
  logic the other lacks, and that is a test rather than an intention:
  `crates/docsai-cli/tests/two_doors.rs` runs the same operations through the real binary and a
  real MCP server on a transport and compares the written bytes and the reports. Cost: **+249
  tokens** in the session preamble (2 228 → 2 477) for a schema deliberately written as one line of
  prose instead of a thousand-token derived enum; the wire-cost suite moved 5 355 → 5 616 tokens,
  **+4.9 %**, inside the gate's 5 % allowance and without the inflation hatch.
- **A workbook whose formula has no cached value now asks to be recalculated.** `docs/edit-operations.md`
  §6.7 left open whether a writer could state that; both can, so `set_cell` writing a formula no
  longer emits a `Warning::Degraded` about a value that «reads as blank». `.xlsx` gains
  `<calcPr calcId="0" fullCalcOnLoad="1"/>` in `xl/workbook.xml`, and only when the workbook really
  holds an uncached formula — forcing a recalculation of a fully cached workbook would change values
  nobody asked to change. `.ods` needs no flag: ODF keeps the cached result in `office:value`, so a
  `table:formula` cell written without one is already a request to compute it, measured against
  LibreOffice Calc rather than assumed. A warning that survives its reason is a false warning.

- **`validate_docmark`: every problem in a DocMark document at once** (plan v2 Phase 17, increment
  F), with `docsai validate <file> [--json]` as its CLI door. An agent that handed bad DocMark to
  `convert_from_markdown` used to get one `ParseError` and had to fix-and-retry; it now gets the
  list, each problem carrying a line and column, a stable code (`container/unclosed`,
  `id/duplicate`, `front-matter/unsupported-version`…), the node id in force at that place, the rule
  it breaks cited by its section of the DocMark specification, and — where one can be made honestly
  — a fix concrete enough to apply. **The parser is the only authority**: an *error* is text
  `parse` refused, a *warning* is text it accepted and read as something other than what it looks
  like, and no error can be emitted about a document that parses. The two DocMark profiles genuinely
  disagree about an unclosed `:::` container — a deck refuses it, a text document swallows the rest
  of itself into it — so the same finding comes back as an error in one and a warning in the other,
  with the consequence spelled out in the message rather than assumed. Cost: **+104 tokens** in the
  session preamble (2 124 → 2 228), inside the wire-cost gate's allowance without touching it.

- **A chart is data, not an opaque part** (plan v2 Phase 16, increments A, B and C). A `.pptx` chart
  used to round-trip byte for byte while telling an agent the word `barChart` and nothing else. It
  now reads as its **categories, its series, its per-series number format, its title and its embedded
  workbook**, and DocMark writes them as the GFM table the specification had reserved since Phase 14
  — categories down the first column, one column per series, `data=` pointing at the workbook:

  ```markdown
  ::: {#n2 .chart data="assets/asset-48b54984.xlsx" kind=barChart}
  Sales by region

  |       | North {format="#,##0"} | South |
  | ----- | ---------------------- | ----- |
  | Q1    | 1200                   | 800   |
  | Q2    | 980                    |       |
  :::
  ```

  An **empty cell is a gap, never a zero** — PowerPoint draws a break where a zero draws a line
  through the floor — and that survives from `c:ptCount`'s sparse indices, through the table, back
  into the part. Retyping a number in that table changes **exactly one member** of the written
  `.pptx` and exactly one `c:v` inside it: `ppt/charts/chart1.xml` is *spliced*, never regenerated,
  because reading a chart as data bought no licence to rebuild the part, and the stale `c:f` range
  beside an edited cache is restated so that «Edit Data» does not silently truncate the edit that was
  just made. A chart the model did not touch produces no edit and no warning. The chart carries its
  numbers twice — the caches in the chart part and the workbook in `ppt/embeddings/` — and **the
  cache is authoritative**, because it is what the slide shows and is present even when the workbook
  is linked or missing; a disagreement between the two is a real property of the file and is
  **warned, never averaged**. The workbook is preserved rather than rewritten (the model holds cached
  columns, not a spreadsheet, and rebuilding one would invent a provenance while destroying the real
  one), and a deck generated from Markdown with no package at all now gets a **real chart part**
  where it used to get a text box. What cannot be spliced is refused by name rather than guessed: a
  series added or removed, the chart's kind, its `c:rich` title, an emptied category axis, and a
  multi-level axis.
- **Comments, in both formats, through one IR node and one DocMark syntax** (plan v2 Phase 16,
  increments E and F; DocMark **1.3**, specification §11.3). `ppt/comments/*` with
  `ppt/commentAuthors.xml`, and `word/comments.xml` with `word/commentsExtended.xml`, read into the
  *same* node and write back from it. A docx comment covers a **range** (`[viernes]{.comment ref=c1}`
  in the flow, with a `::: {.comment author=… date=… initials=… ref=c1}` container after the body); a
  pptx comment is pinned to a **point** (`pos=`); a reply is the same container **nested** inside the
  one it answers, so the nesting *is* the thread. DocMark names an author and never carries the
  package-local `authorId`, which is resolved on read and re-established on write by **matching the
  name against the table the package already has** — so ids never move for a deck nobody edited and a
  new name is appended after the last. Comments are written at `full` and `agent` only; at `standard`
  and `plain` the commented text survives and the annotation is a warned loss. On the pptx side the
  comment parts stay **outside** the rebuilt set: an edited comment is spliced into the part that was
  read, a comment added to a slide with no part brings the part, its `Override` and its relationship
  into existence, and the last comment deleted takes all three away. What a classic `p:cmLst` cannot
  hold — a reply, a resolved mark, a block that is not a paragraph, a date that is not an
  `xsd:dateTime` — is refused by name, never dropped in silence. PowerPoint's **modern** comments
  (`modernComment_*.xml`) are not read; the part is preserved unchanged and the refusal names the
  content type and what it carries.
- **Slide thumbnails: a picture of each slide, drawn by headless LibreOffice** (plan v2 Phase 16,
  seventh increment). `docsai convert deck.pptx -o deck.dmk.md --thumbnails` writes one PNG per
  slide into `<dir>/<name>-thumbnails/`, and the MCP server gains a `render_slides` tool that does
  the same for an agent — with `output_dir` it answers with a receipt, without one with base64 up
  to 2 MiB. Each raster carries its 1-based slide number **and** the slide's node id, so the
  picture joins to what `outline_document` addresses; when the renderer's page count and the deck's
  slide count disagree the ids are dropped and a note says why, because pairing them by position
  after that would attach one slide's id to another slide's picture. Rendering is `soffice
  --convert-to pdf` followed by `pdftoppm -png`, the two steps the render gate already uses,
  because LibreOffice asked for a PNG directly exports only the first page. No image or PDF
  dependency was added anywhere. `--thumbnails-dir` and `--thumbnails-dpi` (24..=300, default 96)
  tune it.
- Slide rendering, and the legacy `.doc` LibreOffice fallback with it, now decide whether
  LibreOffice is available by **running** it and requiring it to identify itself, rather than by
  finding an executable file. An installed-but-mis-linked LibreOffice is on `PATH`, is executable,
  exits **0** and prints a dynamic-loader error instead of a version; the old check called that
  machine able. When the plain invocation fails, the installation's own `program` directory — the
  parent of the resolved `soffice`, derived, never hard-coded — is put on the child's
  `LD_LIBRARY_PATH` and the invocation retried, which is what makes `.doc` conversion and slide
  rendering work on such a machine at all. `DOCSAI_LIBREOFFICE_LIBPATH` overrides the derivation;
  `DOCSAI_PDFTOPPM` names the rasteriser.
- **A conversion cache, so an agent's fourth question about a document does not re-parse it** (plan
  v2 Phase 16, cache increment). `estimate_tokens`, then `outline_document`, then `search_document`,
  then `read_selection` used to parse the same bytes four times. A content-hash LRU in
  `docsai-convert` — 64 MiB and 16 entries by default, `DOCSAI_MCP_CACHE_BYTES=0` to turn it off —
  makes that **one** read. The protocol stays stateless and the cache is a value the caller owns, not
  a process global: the MCP server keeps one for the life of the process, the CLI passes none, and
  every test brings its own. **A hit is indistinguishable from a miss**, which decided every design
  question: the `ConversionReport` is stored with the document, so a degradation reported on the
  first call is reported on the fourth; the asset store is *replayed* rather than bypassed, so a hit
  still writes the same files with the same names in the same order; and the key covers the origin,
  the content hash, the byte length and a fingerprint of every conversion option — deliberately wider
  than a read consumes, because a key that is too wide loses a hit while a key that is too narrow
  returns the wrong document. It refuses to serve rather than serve wrongly: an unhashable input, a
  non-empty asset store, an error, and a legacy `.doc` — whose read depends on what was installed at
  that moment, which is a property of the machine and not of the document — are never cached.
- **The objects that stay opaque, confirmed end to end** (plan v2 Phase 16, opaque-objects
  increment). SmartArt, OLE, transitions and animations have read as a stub plus a sidecar raw
  fragment since Phase 13 and written back since Phase 15; what had never been asserted anywhere was
  the *whole loop* on packages that carry them. `corpus/pptx/ole-embedded.pptx` is new and carries
  both spellings — the plain `p:graphicFrame` of ECMA-376 and the `mc:AlternateContent` PowerPoint
  actually writes — with an OPC package behind one object and an OLE2 compound file behind the other,
  and the members' bytes are compared individually at the end of the loop so a regression names
  *which* object was lost.

### Changed

- **Phase 16 is closed — and risk P2 is still open** (plan v2 Phase 16, ninth increment). Its three
  acceptance criteria are met, each measured rather than asserted: a chart's data reads as a table,
  is edited through DocMark and reaches the package as one changed member; `--thumbnails` writes 67
  PNGs over the deck corpus on this machine with no environment variable set, and says which of
  LibreOffice or `pdftoppm` is missing — and exits 1 — when one is; and the cache turns 20 reads into
  1 while `disabling_the_cache_changes_no_output` compares the DocMark, the assets **and** the report
  across four documents, four fidelity levels and three conversions each. Underneath all of it the
  property Phase 15 established never moved: **21 decks that nobody edited come back byte for byte**,
  checked both by `corpus/render_gate.py` and by hand outside the suite. `corpus/schema_gate.py` is
  at 21 decks, 42 packages, **363 parts validated, 0 invalid**. What is still not proven is what was
  not proven a phase ago: **no PowerPoint has opened any of it.** The checklist of
  `docs/spikes/P2-powerpoint-gate.md` needs PowerPoint and a person; the phase was directed to start
  without it, and paid for that by keeping byte-identity in every increment, so the form is
  re-runnable on any commit of the phase. `kb/84-phase-16-close.md` records the phase, the three
  places a budget or a golden moved and what each bought, the thirty things it carries forward, and
  the question its wire-cost measurement raises for Phase 17: the MCP `session-preamble` — the
  handshake a session pays once — now costs **2 124 tokens** where Phase 17's acceptance criterion
  budgets 2 000 for a whole agent loop, and whether the two are the same budget is Phase 17's to
  decide.

- **Breaking (MCP): `include_images=thumbnails` is now `include_images=downscaled`**, and the
  response fields it writes are `downscaled_base64`, `downscaled_content_type`,
  `downscaled_width`, `downscaled_height` and `downscaled_note`. The rung has always meant
  «downscaled copies of the pictures this document embeds»; with `render_slides` in the same tool
  list, «thumbnail» would have named both that and a raster of a whole slide, in the one place an
  agent cannot afford to guess. The old word is **refused** rather than aliased, with a message
  naming both answers: a caller who wrote it may have meant either, and silently handing over one
  of the two is the failure the rename exists to prevent.

- **The PowerPoint gate, prepared and unrun — and Phase 15 closed with it open** (plan v2 Phase 15,
  eleventh increment). `docs/spikes/P2-powerpoint-gate.md` is the fill-in form for the one thing
  this project cannot test itself: whether **PowerPoint** opens a deck docsai wrote without offering
  to repair it. It builds, in one paste, the three decks that cover Phase 15's three paths to bytes
  — a preserved package spliced with one rebuilt slide, a SmartArt deck whose diagram only
  PowerPoint resolves (LibreOffice takes the `mc:Fallback`), and a package generated end to end on
  the embedded template that **no application has ever opened** — plus a negative control, a valid
  deck with three parts removed and their relationships left dangling, which `corpus/opc_check.py`
  correctly refuses and which spike P3 §2.4 measured LibreOffice accepting in silence. Seven items,
  the repair dialog first, with what counts as a pass and what counts as a failure written before
  the first tick. The decks are not committed: they are derived byte for byte from the tracked
  corpus and the tracked writer, and they are built into the gitignored `target/p2-gate/`.
  **Risk P2 stays OPEN.** `docs/technical-analysis-presentations.md` §7.1 now says in writing what
  the two CI gates do and do not prove — byte-identical round trips over seventeen decks, 322 parts
  schema-valid, every relationship resolving, and *nothing at all* about how any of it looks or
  about PowerPoint's own loader — and carries the exact table row that closes the risk once the form
  is signed. Phase 15's acceptance criteria are settled in `kb/61-phase-15-plan.md`: two are true,
  three wait on an application opening a file, and one of those three was **un-ticked** here because
  its verb is «opens» and nothing has opened it. `kb/72-phase-15-close.md` records the phase and the
  thirteen things it carries forward.

- **CI gate 2: the render diff, before and after the round trip** (plan v2 Phase 15, tenth
  increment). `corpus/render_gate.py` has headless LibreOffice render each corpus deck to PDF and
  `pdftoppm` rasterise every page to greyscale, then scores the pairs with a low-frequency
  perceptual diff — the RMSE over 8 × 8 pixel block means at 100 dpi, which averages away
  anti-aliasing and hinting while keeping anything that moves ink from one part of the slide to
  another. A page over the threshold of **2.5** fails the build naming the deck, the page and the
  number; a deck whose **page count changed** is a failure of its own kind, because pages that
  cannot be paired have no score. The threshold is measured, not guessed: `--self-test` builds
  fourteen pairs whose answer is known — 0.0000 for the same file rasterised twice, 1.72 for
  anti-aliasing on against off, 3.55 for one text line of twelve gone — prints them all, and asserts
  the ordering the threshold depends on, with no renderer needed. Like gate 1 it refuses to pretend
  that a byte-identical round trip was measured by a render: it says so and counts it separately.
  The path that does write new bytes — the whole package generated on the embedded template — is
  held to what a render can honestly say about it: the same page count, and no page that came out
  **blank**. A renderer that is missing *or installed and broken* is a **skip** with a banner naming
  what was not measured and exit 3, never a green tick; in CI, which installs LibreOffice on the line
  above, `--require-renderer` makes it a failure instead. Wired into `.github/workflows/test.yml` as
  its own `render-gate` job. **CI-only and dependency-free**: no image library is added anywhere —
  the rasteriser is asked for PGM, which the standard library can read in fifteen lines — and
  `cargo tree` is unchanged (`AGENTS.md` §7 rule 4).

- **CI gate 1: the `.pptx` parts we write, validated against the ECMA-376 XSDs** (plan v2 Phase 15,
  ninth increment; analysis §5.5). `corpus/schema_gate.py` round-trips every deck of `corpus/pptx`
  twice — once edited on its **preserved package**, once with the `skeleton:` line removed so the
  whole package is written on the **embedded template** — diffs each result against the package it
  was written on, and validates every XML part whose bytes are new or changed. A part that is not
  schema-valid fails the build **naming the part, the line and the element**; a part xmllint reached
  no verdict on fails too, because a gate that measures nothing is worse than no gate. A part copied
  byte for byte out of the skeleton is deliberately not validated: it is not ours, and validating it
  would measure whoever wrote the deck. Markup-compatibility (`mc:AlternateContent`, ECMA-376
  Part 3) is resolved to its fallback before validating, which is what a consumer does and what the
  Part 4 schemas assume. Wired into `.github/workflows/test.yml` as its own job; runnable locally
  with `python3 corpus/schema_gate.py`. The schemas are vendored unmodified in `schemas/` (1.0 MB,
  ECMA-376 Part 4 Transitional plus the Part 2 OPC schemas), with their provenance, licence and the
  reason they are vendored rather than fetched in `schemas/README.md`. It is **CI-only**: `xmllint`
  is not a workspace dependency and reaches no shipped binary (`AGENTS.md` §7 rule 4).

- **`roundtrip` for presentations, with the per-category fidelity metric** (plan v2 Phase 15, eighth
  increment): `docsai roundtrip deck.pptx` runs `pptx → DocMark → pptx → DocMark` and compares both
  the two DocMark passes and the two documents. The metric is `docs/architecture.md` §4's, computed
  for the first time and for every format: paragraphs, headings, lists, tables, images and footnotes
  for a text document; sheets, cells and formulas for a workbook; and for a deck **slides, shapes,
  paragraphs, pictures, tables, notes and objects** — the units a person navigates, selects and
  reads, plus the ones this project keeps as a stub over preserved markup. Each category prints both
  counts (`pictures 0/1`, not `0 %`), a category neither side has is not printed, and `--json`
  carries them. The whole `corpus/pptx` — seventeen decks, `.pptm` included — round-trips idempotent
  with every category at 100 %, gated by `pptx_roundtrip_is_idempotent`.

- **The deck with no skeleton** (plan v2 Phase 15, seventh increment): a deck that never had a
  package — Markdown typed by hand, with no attribute anywhere — is written on a **default template
  embedded in the binary**, generated from source by `corpus/generate.py` and checked by
  `corpus/generate.py --check`: a master, four layouts, a notes master, a theme, and no slides at
  all. Each slide is **written from the model** against a layout chosen from the shape of its
  content (analysis §6.6): a title with body text is «Title and Content», a title alone is «Title
  Slide», a layout the document names wins outright, and content matching no rule falls back to the
  title-and-content layout with a warning saying which layout it went on. Placeholders, text boxes,
  pictures and tables are generated — an `a:tbl` written from the model, which only a generated
  slide gets — and a shape whose markup only its own package holds (a chart, a diagram, an embedded
  object) is a text box carrying its label plus a warning naming it.
- **A slide, and a notes page, the package does not have.** The same generation lifts two refusals
  of the fifth increment: a slide the model **added** to a preserved deck gets a part of its own,
  spliced into `p:sldIdLst`, into the deck's relationships and into `[Content_Types].xml`, and a
  note typed onto a slide that never had a notes page gets one, hanging from the deck's notes
  master. Every slide that was already there stays byte for byte the part that was read, and a deck
  whose model matches its package never reaches any of this.
- **The stale autofit scale on the way out** (plan v2 Phase 15, sixth increment): the one thing the
  `.pptx` writer deliberately does *not* preserve. `a:normAutofit@fontScale` is a number PowerPoint
  computed for the text that was in the box when a human last edited it, and it does not recompute
  it when a file changes underneath; left over text that is no longer there it shrinks the box for
  content that was never measured, and no reader can detect it. So when a text body's text actually
  changed, the scale and its `lnSpcReduction` are dropped from `a:bodyPr` and
  `Warning::AutofitStale` names the shape and the number that was in the file (analysis §5.4).
  «Changed» is the writer's own notion of change and not a second one: the body's new bytes differ
  from the bytes the part already said. A shape whose text did not change keeps its scale and is
  **not** reported — a warning about a loss that did not happen is the same failure as a missing
  one — and a body that carries no scale produces no edit at all, so an unedited deck still comes
  out byte for byte the deck that went in. The same rule reaches a slide's notes page. A
  `lnSpcReduction` stated with no `fontScale` is left standing: the shape claims no scale, so there
  is nothing to call stale.

- **Speaker notes, and the deck's own slide list** (plan v2 Phase 15, fifth increment): a slide's
  **notes page** is rewritten from the model like any other text body — reached through the slide's
  own relationships and never by number, so a deck whose notes were attached out of order does not
  get every note put under the wrong slide — while the printed page around it (the slide's
  thumbnail, the header, the footer, the date, the page number) stays the bytes that were read.
  A note typed onto a slide the deck has **no** notes page for is refused by name (writing the part
  is 15-G), never dropped in silence.

  And the deck's own part is edited, in two places and nowhere else: when the model's slides are
  not the package's, `p:sldIdLst` is restated in the model's order and `p14:sectionLst` keeps every
  section's name and GUID while listing the slides the model puts in it. A **removed** slide takes
  its part, its relationships, its notes page and its content type out of the archive with it,
  because unlisting a slide while leaving its text in the file is not deleting it. A **reordered**
  deck moves the list and not the parts: every slide part comes out byte for byte the one that was
  read. Which package slide a model slide *is* has to be recovered from the content — nothing in
  the chain carries an identity for a slide — and a slide the model **added** has no part to be
  spliced into, so it is refused by the name of the increment that writes one (15-G).
  `ppt/presentation.xml` is otherwise copied, and an unedited deck does not touch it at all.

- **Nothing the writer does not model is lost on the way out** (plan v2 Phase 15, fourth
  increment): a **preserved object** — SmartArt, an OLE object, a group, a custom geometry, an
  animation timeline, anything the IR keeps only as a stub over a raw fragment — goes back into the
  package as the original markup at its original z-order, and is no longer reported as a copied
  loss, because nothing was lost: the splice never touched it. When the document carries a payload
  that *differs* from the package's bytes, that payload is written back over the element it belongs
  to; a slide-level fragment (`p:transition`, `p:timing`) goes back as a child of `p:sld` rather
  than into the shape tree. A fragment in another dialect is **dropped** with
  `Warning::RawBlockDropped` naming it and the dialect, never emitted as nonsense inside a
  `p:spTree`, and a fragment whose markup belongs to a part outside `rebuilt_parts` — a chart's XML
  — is copied and named, because the licence to rebuild a part is the skeleton's, not the model's.


- **Pictures, tables and media come back too** (plan v2 Phase 15, third increment): the splice that
  rebuilt a slide's text now reaches what a text body is not. A picture's bitmap, alt text, drawn
  size and crop; the text of a table's cells; and the hyperlinks a run carries. Media goes into the
  package **as the asset store holds it** — the bytes that came out of the source document, with no
  re-encoding — under a new `ppt/media/` part, and the parts that have to agree with it are spliced
  rather than regenerated: the slide's `_rels` gains only the relationships the deck did not have,
  keeping every id it already used, and `[Content_Types].xml` gains a `Default` only for an
  extension the deck had never carried. An unedited deck still comes out byte for byte as it went
  in over the whole corpus, `_rels` included. A table whose *grid* moved is still copied whole and
  named, because writing an `a:tbl` from the model rather than splicing it belongs with the deck
  that has no skeleton to preserve (15-G), and a link whose target no relationship can state as it
  stands is still refused rather than written as an `r:id` pointing at nothing.

- **A deck goes back into its package** (plan v2 Phase 15, second increment): `docsai convert
  deck.dmk.md -o deck.pptx` writes a `.pptx` again, and `SUPPORT` says `pptx: write yes`. The unit
  of rewriting is a **text body**, not a package and not a slide: the `a:p` sequence inside a
  placeholder or a text box is rebuilt from the document, and everything around it — `p:spPr`, the
  `a:bodyPr` with its autofit scale, the shapes that are not text, the transition and the timing —
  is the bytes that were read. A deck nobody edited therefore comes out byte for byte as it went
  in over the whole corpus, and a retitled slide differs in exactly the run that was retyped: a
  conversion is not a diff.

  What the writer cannot yet put back it **keeps and names**: a picture, a table, a chart or a
  preserved object (15-C, 15-D), a speaker notes page (15-E), a hyperlink needing a new
  relationship, a field whose GUID nothing recorded, a slide added or removed. Each is a
  `Degraded` warning saying what was copied and which increment lifts it, so a caller who edited
  the model and got the original back learns it from the report rather than from the file.

  One attribute is written from outside the model on purpose: `a:rPr@lang`. It is spell-check
  metadata with no IR field and it is on every run PowerPoint writes, so the slide's own language
  is read off its runs and put back — and only when they all agree.

- **The pptx package comes back out** (plan v2 Phase 15, first increment): `docsai-office` writes a
  `.pptx` for the first time — as the package it read. `write_pptx` re-emits the preserved skeleton
  ZIP entry by entry, in the original member order, copying each entry's *stored* bytes with its
  compression method, CRC and timestamp rather than inflating and re-deflating it. That is the
  difference spike P3 measured: a package rebuilt through the in-memory `Package` comes out
  alphabetised, recompressed and re-timestamped even when every part is identical, and a different
  file is a file PowerPoint gets to have an opinion about. Over the whole pptx corpus the written
  archive matches the one that was read member for member, byte for byte, including a chart's
  embedded workbook, a diagram's `dgm:` parts and a `.pptm` macro payload.

  What this increment does *not* do is regenerate a slide from the model, and it says so: writing a
  deck emits a `Degraded` warning naming the increment that lifts it (15-B). `SUPPORT` still
  answers `pptx: write no` and `docsai convert … -o deck.pptx` is still refused, on purpose — a
  conversion that silently ignored the edits in the document would be worse than the refusal it
  replaced. A deck with no preserved package is a typed error naming 15-G, not an empty file.

- **The P4 gate, and Phase 14 closed** (plan v2 Phase 14, eleventh and last increment): risk P4 —
  *«DocMark-P becomes unreadable»* — is answered the way plan v2 asks, by editing three `standard`
  decks as text without consulting the specification: retitle a slide, add a bullet, swap an image,
  and then everything else a person does while they are in the file — add a slide by typing a
  heading, delete one, type a table, drop the front matter, mistype a fence. All of it works, and
  the sixteen edits are pinned in `docsai-convert/tests/p4_hand_edit.rs` so they cannot quietly
  stop working.

  One defect found, and it is the edit a reviewer is most likely to make: **adding a note to a
  slide that already had one replaced it**. At `standard` the notes are a blockquote, so a second
  note looks like a second blockquote, and the parser assigned rather than joined — text visible on
  screen disappeared with no warning. A slide has one notes page: more than one notes block under
  it, blockquote or `::: {.notes}`, is that page with the blocks in order. Written into the
  specification, not just into the parser.

  A second defect went with it: converting a deck at `standard` or `plain` left a copy of the whole
  original package beside the document as `assets/img-<hash>.bin`, referenced by nothing. The
  levels that name no package now leave none, and the empty `assets/` goes too.

- **A deck converts** (plan v2 Phase 14, tenth increment): the refusal added in Phase 13 is gone —
  `docsai convert deck.pptx -o deck.dmk.md` writes DocMark-P, and `outline`, `tokens`, `search` and
  `read --select` work over a presentation with no work of their own, because they all go through
  the serializer. The preserved package travels with the document: the pipeline writes it to the
  `assets/_skeleton/deck-<hash>.pptx` the front matter names, *moving* it there rather than leaving
  a second copy under an image's name, and `standard` and `plain` — which reference no package —
  leave none behind. `docsai formats` now says «Phase 14 read-only: converts to DocMark-P; writing
  a pptx package is Phase 15».

  Measured and **not met**, recorded rather than softened: the criterion inherited from Phase 13,
  *«`--fidelity agent` on a deck is ≤ 15 % of the `full` token count»*, comes out at 96–102 % over
  the corpus. `agent` and `full` differ by a single line: what `agent` drops is formatting, which
  these decks barely carry, and the shape geometry that analysis §6.5 wanted collapsed is written
  at `agent` by design. Closing that gap changes what a fidelity level means, so it is a
  specification decision and not a patch; the criterion sits in the test suite as an `#[ignore]`d
  test that prints the numbers.

- **Deck goldens and byte idempotence** (plan v2 Phase 14, ninth increment): the seventeen decks of
  `corpus/pptx` are pinned by their DocMark-P as `<name>.expected.dmk.md`, in the same test and with
  the same `DOCSAI_UPDATE_GOLDENS=1` ritual as the docx, xlsx, odt and ods corpora; the
  `<name>.expected.inspect.json` of Phase 13 stay beside them and answer a different question. On
  top of the goldens, `serialize(parse(md)) == md` is checked byte for byte over every deck: what a
  round trip through DocMark-P costs a document that nobody edited is now zero, so an edit changes
  what it touches and nothing else. A `skeleton:` reference resolves against the asset store when
  the package is not beside the document — the reference is built from the content hash, so the
  name is enough to find the bytes again.

- **The deck parser** (plan v2 Phase 14, eighth increment): DocMark-P is read back into the IR —
  front matter, `.slide` headings, the container classes of rules 4 and 8, pictures, tables and
  groups, and speaker notes in **both** syntaxes, the `::: {.notes}` container and the `standard`
  blockquote. Every deck of `corpus/pptx` now makes the round trip at `full`, `agent` and
  `standard` with its slides, its addresses and the *kind* of every shape intact. Input is
  tolerant, which is what lets a deck be drafted in a text editor: a `#` or `##` heading opens a
  slide with or without `.slide`, content before the first heading is a slide with no title, and a
  container class the reader does not know keeps its text as a shape and says so with a warning
  instead of refusing the file. A `.shape` is a stub when it carries `raw=` and a text box
  otherwise — `geom=` alone would freeze an editable rounded box into an opaque object. Also
  fixed: a parse error in the body named a line one short of the real one, because the blank line
  after the front matter was skipped without being counted.

- **`plain` proven, and the degradation rule as a test** (plan v2 Phase 14, seventh increment): the
  spec's own sentence — *«a plain Markdown viewer must show, per slide, title + bullets + images and
  nothing else»* — is now checked by CI instead of by a reviewer. Spike P2's residue probe moved
  into the tree (`docsai-convert/tests/plain_residue.rs`): every deck of `corpus/pptx` is rendered
  with a viewer that has no container and no attribute extension, and every visible character is
  classed as content or as syntax that leaked. At `plain` the budget is **zero**, over every deck;
  at `standard` what may leak is *named* — the `{.slide}` marker and the containers of rules 4 and
  8, nothing else — so an attribute cannot creep back in behind a percentage. Three writer defects
  the measurement found are fixed: `layout=` is no longer written at `standard`, where the
  `layouts:` catalogue it names is absent and the reference pointed at nothing; a slide table drops
  `col-widths=` at `standard`, the same rule that already dropped an image's size, which leaves a
  bare GFM table with no container; and `geom=rect` — the DrawingML default, said by every plain box
  — is written nowhere, exactly as `type=body` already was.

- **Pictures, tables, groups and object stubs on a slide** (plan v2 Phase 14, sixth increment): a
  picture is written as an image line and a table as a GFM table — Markdown has both, so neither
  gets a container — each carrying its *shape's* id, name and position, with the picture's size
  left to the image's own `width=`/`height=` so no measurement is written twice. A group is
  `::: {.group}` around children that keep their own addresses, because a group is a box around
  shapes and not a substitute for them. Everything else Markdown has no form for is a stub of its
  own class — `.chart` (with `kind=` and, when the IR names it, `data=` for the embedded
  workbook), `.smartart`, `.ole`, `.media`, `.object` — holding the text the object shows and the
  `raw=` reference to the sidecar. An image on a slide carries **no measurements at `standard`**,
  where a plain viewer draws it at its own size regardless; the same image in a text document keeps
  them, because the rule is a property of the document class and not of the level. A table with no
  rows is warned rather than written as a header rule over nothing.

- **Speaker notes** (plan v2 Phase 14, fifth increment): a slide's notes are written after its
  shapes as `::: {.notes}` at `--fidelity full` and `agent`, and as a **CommonMark blockquote** at
  `standard` — the one node whose syntax depends on the level, because a blockquote is native
  Markdown and PresentationML has no blockquote a placeholder could occupy, so the level that has
  to stay hand-editable pays no syntax for its notes. At `plain` the notes are dropped with a
  warning: a notes page is not what the slide shows. A slide with no notes page and a slide with an
  empty one stay different documents — the second writes an empty container where the document
  writes back.

- **A slide's shapes become containers** (plan v2 Phase 14, fourth increment): every shape the
  layout does not make implicit is written as `::: {.ph idx=…}`, `::: {.shape geom=…}` or
  `::: {.connector …}`, holding its own content and its geometry — `pos=` and `size=` in readable
  units with `emu` as the exact fallback, plus `rotation=`, `flip=`, `name=` and the `raw=`
  reference to the sidecar — at `full` and `agent`. `standard` keeps the container, its `geom=` and
  its `type=`, because those say what the box *is*, and drops every measurement, index and raw
  reference; the dropped payload is a warning, not a silence. Slide furniture — slide numbers,
  dates, footers and headers — is written only where the document writes back: it is inherited from
  the layout and costs a container on every slide. At `--fidelity plain` there are no containers at
  all, and what a shape says still reaches the reader. A selection of a deck now declares
  `docmark: "1.2"` as well, since its body is written in the presentation profile.

- **A deck writes its slides** (plan v2 Phase 14, third increment): every slide is an `##` heading
  carrying `.slide`, and the heading **is** the title placeholder — no container repeats it — while
  the layout's primary body placeholder is written as ordinary Markdown under it. Which shape is
  which is the catalogue lookup of `layouts:`, so a layout whose body sits at index 2 puts index 2
  in the body and leaves index 1 for a container, where a «first body wins» guess would swap them.
  The heading carries the slide's id, its `layout=`, its `section=` (`p14:sectionLst`, on every
  slide of the section so a slide read on its own still knows where it belongs), `hidden=true` and,
  at the levels that write back, its `name=`. A slide with no title, or with an empty one, writes
  an empty `##` heading: ugly and bounded, against a container on every slide. At `--fidelity
  plain` a slide is a heading and its bullets and nothing else. What this increment does not write
  yet — the other placeholders, free shapes, pictures, tables and speaker notes — is reported as a
  warning per shape, never omitted in silence.

- **A deck writes its front matter** (plan v2 Phase 14, second increment): a presentation
  serialises `docmark: "1.2"` — the version names the *profile*, not the addressing, so a deck
  declares it with or without node ids — plus `layouts:` and `skeleton:` at the levels that write
  back. Each layout entry says its name, its master and **which placeholder index is the title and
  which is the body**, which is what turns the implicit heading and the implicit body of the
  profile into a catalogue lookup instead of a guess. `skeleton:` is a path
  (`assets/_skeleton/deck-<hash>.pptx`), not an asset id, for the same reason an image is a path.
  At `--fidelity standard` neither appears: it does not write back, and a catalogue nothing in the
  body refers to is pure cost to whoever reads it. The body of a deck is still empty — the slide
  writer is the next increment.

- **DocMark 1.2, the presentation profile, is specified** (plan v2 Phase 14, first increment):
  `docs/docmark-specification.md` §11.2 stops being a sketch and becomes normative — eight rules,
  the front-matter keys a deck adds (`layouts:`, `skeleton:`), the version rule and the
  compatibility contract. The design is the one spike P2 measured, which **supersedes the earlier
  sketch on two points**: placeholders are *implicit* (the `##` heading is the title placeholder
  and the layout's primary body is written as ordinary blocks under it, so a plain viewer no longer
  prints every title twice), and speaker notes degrade to a blockquote at `--fidelity standard`.
  Nothing serialises yet; this is the contract the rest of the phase is written against.

- **`docsai inspect` reports the slide inventory** (plan v2 Phase 13, eleventh increment): `.pptx`
  is finally **readable** — it joins `docsai_office::read`, `docsai formats` says `pptx: read yes`,
  and `inspect` reports, per slide, the layout it hangs from (by `p:cSld@name`, the name a human
  recognises), how many shapes are on it and of what kind, whether it carries speaker notes, and
  whether it holds SmartArt or an embedded OLE object — what an agent needs to decide *where* to
  edit without loading the deck. `--json` carries the same under a new `slides` key. An
  `mc:AlternateContent` stub is now named for what it wraps rather than always «other»: naming a
  thing is not the same as reading it, and a stub that hid SmartArt left an agent blind to the one
  object on the slide it must not hand-edit. The preserved package no longer shows up as a media
  asset, so a deck with no pictures reports none. Corpus layouts now carry a `p:cSld@name`, as real
  ones do.

  Reading is **not** converting: `docsai convert deck.pptx` still refuses with
  `unsupported conversion: pptx -> docmark`, because the DocMark-P profile is Phase 14 and a
  serializer that wrote an empty body would lose every slide in a file that looked like a success.

- **`.pptm` and corrupt decks** (plan v2 Phase 13, tenth increment): a macro-enabled presentation
  reads as its macro-free equivalent — the slides come back whole, the VBA project is never
  inspected or executed, and `Warning::MacrosIgnored` names the part, the same rule `.docm` and
  `.xlsm` already followed. Detection stays content-based: a `.pptm` renamed `.pptx`, or the other
  way round, reaches the same reader. New corpus deck `macro-enabled.pptm`. The robustness suite
  now covers presentations too: truncated packages, flipped bytes, malformed XML, random noise and
  dangling relationships are a typed `Err` or a warning, never a panic. A slide relationship that
  does not resolve is a warning and the deck reads without that slide; a relationship that resolves
  to a part the package does not carry is a typed `ReadError::MissingPart`, because handing back a
  deck one slide short is how an agent deletes it by writing it out again.

- **Nothing disappears from a slide** (plan v2 Phase 13, ninth increment): a group, a connector, a
  custom geometry, SmartArt and anything else the IR has no node for now comes back as three
  things at once — a **visible stub** so an agent knows the object is there and does not delete it
  by writing the slide back without it, the **markup verbatim** in a raw fragment the stub points
  at by id (sliced from the source part, never re-serialised), and a **typed warning**. A slide's
  `p:transition` and `p:timing` are preserved the same way; until now they vanished without even a
  warning. A chart is recorded as a chart — its kind read from the chart part reached through the
  frame's relationship, its XML preserved — rather than reported and skipped. A shape's
  `a:prstGeom@prst` is kept, so a `rightArrow` no longer comes back a plain box. New
  `Warning::AutofitStale`: the `a:normAutofit@fontScale` a deck carries is kept, because only
  PowerPoint can recompute it, and reported, because an agent that adds a line has just made it a
  lie. New corpus deck `raw-preserved.pptx`.

- **The preserved package skeleton** (plan v2 Phase 13, eighth increment): reading a deck now
  keeps the original `.pptx` whole and opaque in the asset store, referenced from
  `Presentation::skeleton`, so a writer re-injects the slides it can rebuild into the deck as it
  was written instead of regenerating a theme, a master, `tableStyles.xml` or the workbook
  embedded in a chart. Spike P3 measured what the alternative costs: a package whose unmodelled
  parts are dropped still opens without a word, and loses a chart's values and five SmartArt parts
  invisibly. `SkeletonRef::rebuilt_parts` names exactly the parts whose content the IR holds — the
  slides that were read and their notes pages, never a part the reader skipped — which is the
  writer's licence to regenerate one. The package is content-hashed like every other asset, so the
  same deck read twice is stored once, and reading it is now capped at 512 MiB, the same limit
  that already bounded what a package may expand to.

- **Deterministic, reversible reading order** (plan v2 Phase 13, seventh increment): a slide's
  shapes come back in the order a human reads them, not in the order the file stores them.
  `p:spTree` is z-order, and it stops agreeing with reading order the moment a shape is sent to
  the back or a title is added last. Placeholders come first by type — title, then the bodies in
  the layout's own `p:ph@idx` order, then the rest, with the furniture a deck repeats on every
  slide last — and the remaining shapes follow by top-left, with tops within an eighth of an inch
  read as one row from left to right. Nothing is lost by the move: every shape still carries its
  source `p:spTree` index, so sorting by it reproduces the tree exactly, and every comparison ends
  in that index, so the order is total and independent of the sort algorithm. New corpus deck
  `reading-order.pptx`, whose shape tree is deliberately not its reading order.

- **Speaker notes** (plan v2 Phase 13, sixth increment): `ppt/notesSlides/*` becomes
  `Slide::notes`, reached through the slide's own `notesSlide` relationship and never by the
  number in the part name — `notesSlide1.xml` belonging to the first slide is PowerPoint's habit,
  not a rule, and pairing them by number puts every note under the wrong slide. The notes page's
  furniture (the slide thumbnail, header, footer, date and page number) is regenerated from the
  notes master and is not read as text; the note itself stays prose, because what would bullet it
  is a notes master that is not in the slide cascade. A slide with no notes part reads `None`, a
  slide with an empty one reads `Some([])`, and the writer needs the difference. New corpus deck
  `notes-crossed.pptx`, whose notes parts are numbered against the slides on purpose.

- **Slide pictures and tables** (plan v2 Phase 13, fifth increment): `p:pic` becomes the same
  normalised `ImageRef` a `.docx` picture does, stored in the `AssetStore` by content hash — one
  bitmap on four slides is one stored file — with its alt text, crop, native pixel size and
  hyperlink; a linked picture is still never fetched. `p:graphicFrame` holding an `a:tbl`
  becomes the IR `Table`, spans and all: DrawingML writes the span on the origin cell and marks
  what it swallowed, so a horizontally merged cell leaves the grid and a vertically merged one
  stays as a covered cell, exactly as a `.docx` `vMerge` continuation does. What a graphic frame
  holds that is *not* a table is reported **by what it is** — `p:graphicFrame (chart)` — read
  from the `a:graphicData@uri` rather than guessed from the first child.

- **The placeholder cascade, as reference plus delta** (plan v2 Phase 13, fourth increment): a
  slide's fonts, sizes and colours are read against what its layout, master and theme already
  decide. Theme references are **resolved** — `+mj-lt`/`+mn-lt` become the font the
  `a:fontScheme` names, `a:schemeClr` becomes a hex colour through the master's `p:clrMap`, so
  `tx1` on an inverted master resolves to what it really is — and what the cascade already says
  is then **subtracted**: a placeholder that changes nothing stores nothing, and the resolved
  values live on the layout and master placeholders, where they belong. The reference is never
  flattened away: the slide keeps its layout id. A colour transform (`a:lumMod`, `a:alpha`) and
  a theme colour that resolves to nothing are reported rather than dropped in silence.

- **Slide text** (plan v2 Phase 13, third increment): `p:sp` becomes a `Placeholder` or a
  `TextBox`, and `p:txBody` becomes blocks — `a:p`/`a:r` with the same run-property model the
  docx reader uses, `a:br`, `a:fld` (a slide number is `FieldKind::Page`, and the DrawingML type
  travels as the instruction so the writer puts back the same field), hyperlinks, and `a:pPr`
  properties in DrawingML's units. **A body placeholder bullets by inheritance**: the master's
  `bodyStyle` carries the `a:buChar` and the slide says nothing, so body paragraphs become a
  list, `a:pPr@lvl` nests it, `a:buAutoNum` numbers it and `a:buNone` opts out. An **empty
  paragraph is content and never a bullet**. `a:normAutofit@fontScale` is read now so that the
  increment that warns about it has a number to warn about. Shape kinds this reader does not
  model yet — pictures, tables, groups, connectors — each produce a warning naming what was
  skipped; none of them disappears quietly.

- **The pptx package layer** (plan v2 Phase 13, second increment): `docsai-office::pptx` opens a
  deck, resolves `[Content_Types].xml`, walks `ppt/presentation.xml` and its relationships, and
  produces a `Presentation` whose slides are ordered, named, layout-referenced and assigned to
  their `p14:sectionLst` section — with the layout and master catalogue behind them, placeholder
  positions included. **Parts are found by content type, never by file name**, and order comes
  from `p:sldIdLst`, never from `slideN.xml` (spike P3). The slides are still empty: shapes are
  the next increment, so `read_pptx` is deliberately kept out of the `read` dispatch and
  `docsai formats` still says `pptx: no`. A deck that converted to an empty document would be
  worse than one the tool honestly refuses. `read_meta` and the new `ContentTypes` are shared by
  the docx, xlsx and pptx readers rather than copied a third time.

- **`Document::Presentation`, the third IR root** (plan v2 Phase 13, first increment): a
  `Presentation` of layouts, masters and slides, with `Shape` carrying identity, name, geometry
  and its index in the source `p:spTree` so the reading-order policy is reversible. The
  placeholder cascade is stored the way styles already are — **reference plus delta** — and a
  layout names its title and its primary body, which is what lets DocMark-P write the title as
  the slide heading (spike P2). Slides carry a stable id; the title and primary body do not,
  because there is nowhere in the profile to write one, and `addressing::implicit_shapes` is the
  single rule that says so for both the id walker and the future serializer.
  `pptx`/`pptm` is now a recognised format, honestly reported by `docsai formats` as **not yet
  readable** — the reader is the next increment. Converting a presentation to DocMark produces
  an empty body and a severe warning until DocMark-P lands in Phase 14; it never fails silently.
  `ConversionStats` gains `slides`, and `inspect` reports `kind: "presentation"`.

- **The three addressing primitives over MCP** (plan v2 Phase 11, closing the phase):
  `outline_document`, `search_document` and `read_selection`, the same answers as
  `docsai outline`, `docsai search` and `docsai read --select`, over a filesystem `path` or
  `content_base64` + `filename` like every other tool. They are the intended path for a
  document of any size — map it, find where it says the thing, read back that part as
  self-contained DocMark with an etag per node — and the server's `instructions` now say so,
  since that is the only text an agent reads before choosing a tool. `convert_to_markdown`
  stays what it was: the whole document, the expensive one. On the 9 000-token report the
  three cost 3.8 %, 4.2 % and 0.3 % of reading it.
- **`include_images=none|refs|thumbnails|full`** on MCP `convert_to_markdown`, default
  **`refs`** (see *Changed*). `none` sends no image payload, `refs` sends each image's name,
  MIME type and size, `thumbnails` adds a PNG downscaled to 256 px on the long side, and
  `full` sends the original bytes. The **markdown never changes**: the body keeps its
  `![](assets/…)` links at every rung, so no rung is a lossy conversion and a client that
  started cheap can come back for `full`. Every rung reports `image_count` and `image_bytes`
  — an empty payload list must not read as a document without images. A thumbnail never
  costs more than the image it stands for: for the icons and logos already smaller than the
  box, re-encoding would *add* bytes, so the original is sent instead. Images that no
  pure-Rust decoder in the dependency budget reads (EMF, WMF, SVG) come back as a ref with
  the reason on the row, never dropped in silence.
- `docsai_convert::outline_input`, `select_input` and `search_input`: the three primitives
  over a `SourceInput`, so they answer about the same document from a path or from bytes.
  The `*_path` functions remain, as wrappers.
- **`docsai search <in> <query>`** (plan v2 Phase 11): where a document says something, with an
  address and the words around each match — never the document. The query is a case-insensitive
  literal; `--context N` (default 48) sets the characters quoted either side of a match,
  `--limit N` (default 20) how many blocks are listed, and the rest are counted rather than
  dropped. The unit is the DocMark **block**, not the addressed node, because ordinary prose
  carries no id (spec §11.1): a block with an id is reported at it together with the selector
  that reads it back, and a block without one is reported relative to the last id before it
  (`n12.b2`). A footnote is reported at the block that refers to it, since it cannot be selected
  on its own. Searching a 9 000-token report for a phrase that appears 35 times costs 386 tokens
  (4.2 %). A relative hit deliberately names **no** selector: `read --select` has no `.bN` term
  yet, and an address that would read something else is worse than none.
- **`docsai read <in> --select <selector>`** (plan v2 Phase 11, DocMark spec §2.1): part of a
  document as valid, self-contained DocMark. Selector terms are `s4` and `s7-s9` (positions in
  the order `docsai outline` prints, 1-based, inclusive), `#n7`, `type:heading` and `text:foo`;
  comma-separated terms are unioned and the output is always in document order. The body is the
  bytes the whole document wrote for those nodes, so nothing is re-derived; the front matter is
  the minimum needed to parse and re-write it — version, source format, the *source document's*
  `next-id`, `partial: true` and an `etags:` map — and deliberately carries no metadata, page
  geometry, catalogue or attribute-set dictionary. A footnote is not selectable on its own, and a
  selected block that refers to one always carries its definition. Two headings of a 9 000-token
  report cost 26 tokens.
- **Etags in the output**, at last (spec §2.1, §11.1): Phase 10 computed them and wrote none. A
  selection now carries one per addressed node, recomputed on every write rather than stored, so
  an edited node's etag moves with it and an if-match write-back has something to check.
- `Warning::PartialDocument`: raised on every serialisation of a document with `partial: true`.
  Severe on purpose — the loss is not in that document, it is in the one a careless whole-file
  write would replace with it.
- `docsai_docmark::Options::dictionary`, and `selection_front_matter`. A selection turns the
  attribute-set dictionary off so it depends on nothing outside itself.
- **Readable units** (plan v2 Phase 11, DocMark spec §2): a length is now written in the unit of
  **what it measures** rather than in whichever unit happened to divide it exactly — points for
  layout and typography (indents, margins, column widths, list levels), pixels for drawings and
  bitmaps. An indent of 720 twips reads `36pt` instead of `48px`, and zero carries no unit at all
  (`0`, not `0px`).
- `docsai convert --precision N` (default 2): how many decimals a readable unit may use before a
  length falls back to `emu`. It buys readable units, never rounding — `1.251cm` is written
  `450360emu` at precision 2 and `1.251cm` at precision 3, never rounded to `1.25cm`. The
  round-trip tolerance for a length therefore stays **zero** at every precision, which
  `crates/docsai-docmark/tests/readable_units.rs` checks over every Word twip value.
- `docsai_model::units::LengthStyle` and `Length::render(style, precision)`, the one place the
  rule lives. `Display` keeps the geometric rendering for logs and messages.
- **Attribute-set dictionary** (plan v2 Phase 11, DocMark spec §3.7): a pattern of attributes
  that repeats at least three times and is at least twelve characters long is written once in the
  front matter under `attribute-sets:` and referenced by class in the body (`{.g1}`). The reader
  expands the class into its pairs before anything interprets the block, so the IR is identical
  either way and no consumer can tell a dictionary was used; a pair written on the node wins over
  the entry's. Names follow first appearance and skip anything the document already uses — a
  style id, a list name, a structural class — so the dictionary is a function of the document and
  stays part of serializer determinism. Active at `full` and `standard`. Measured on
  `docx/repeated-formatting.docx`: 725 → 616 tokens at `full` (−15 %) and 499 → 390 at
  `standard` (−22 %).
- Corpus fixture `docx/repeated-formatting.docx`: the same direct formatting applied by hand over
  and over, which no style implies and the economy rule therefore cannot remove. Without it the
  dictionary would be measured on a corpus that repeats nothing.
- **Delta emission against the whole inheritance chain** (plan v2 Phase 11): a run is now
  resolved against its **paragraph's** style before its own, which is the middle of the OOXML
  cascade and the level the serializer used to skip — text typed after applying a style carries
  that style's run properties again in `w:rPr`, and all of it was being written out. A style in
  the catalogue emits only what its `based-on` chain does not already say; the default paragraph
  style is no longer named on a paragraph; a heading omits the class its `#` level already names
  (and the parser puts it back). Every omission is reversible: a value equal to the inherited one
  resolves to the inherited one.
- Corpus fixture `docx/redundant-formatting.docx`, which carries that redundancy on purpose —
  600 → 528 tokens with the change, and the existing corpus 24 883 → 24 667 (−0.9 %), small only
  because generated fixtures were already clean.
- **`--fidelity agent`** (plan v2 Phase 11), a fourth level and the first that is a *projection*
  rather than a conversion: text, structure, node ids and raw stubs, without the style and list
  catalogues, indents, spacing, page geometry, image geometry or column widths. A sheet keeps its
  formulas and merges — those are what a sheet is. The output declares `fidelity: agent` in its
  front matter and is meant to be read whole and written back node by node, so ids are assigned
  as at `full`. Measured on the corpus: 62–75 % fewer tokens than `full` on 24 of the 25 text
  documents; the exception is the one whose cost is prose rather than formatting.
- `corpus/token-budget.md` now has an `agent` column, so the projection's cost is gated in CI
  like every other level.
- **Raw-block sidecars** (plan v2 Phase 11): at `--fidelity full` the bytes of a raw-block go
  to `assets/_raw/<id>.xml` and the body keeps a stub, `::: {.raw … src="assets/_raw/r7.xml"}`.
  `docsai convert --raw inline|sidecar` (default `sidecar`) chooses; `--raw inline` restores the
  1.0 form. A missing or unreachable sidecar is a typed error, never a silent loss.
  `docsai_docmark::raw::raw_sidecars` gives a caller the files a serialisation refers to.
- Corpus fixture `docx/fields-raw.docx` now carries block-level OMML maths, so the raw-block
  path — and its sidecar — is exercised end to end by the corpus instead of only by unit tests.
- **DocMark 1.1 — stable node addressing** (plan v2 Phase 10): addressable nodes carry
  `{#n7}` and the front matter declares `next-id`, a monotonic counter that is never
  renumbered on insertion and never reused after deletion. Ids ride in the attribute
  block the node already has (headings, container paragraphs, images, tables, complex
  table rows, sheets, multi-section containers), on the first item for lists
  (`list-id=`) and on the reference for footnotes (`[^1]{#n9}`).
- `docsai convert --ids assign|preserve|never`; the default is `assign` at
  `--fidelity full` and `never` at the lossy levels. `plain` never carries ids.
- `docsai-model::addressing`: `NodeId`, `Etag`, `IdPolicy`, the `Addressable` trait and
  the document walkers (`assign_ids`, `for_each_addressable`, `node_ids`). Etags are a
  6-character hash of the node's normalised content, derived on demand and never stored.
- **`docsai tokens <in> [--fidelity …] [--ids …] [--top N] [--json]`** — the cost of a
  document measured with a real BPE tokenizer (`tiktoken-rs`, `o200k_base`, vocabulary
  embedded: no network, no Python), split into front matter and body, plus the cost of
  every addressed node over the exact DocMark it wrote. Rationale for the tokenizer
  choice in `docs/technical-analysis.md` §4.4.
- **`docsai outline <in> [--depth N] [--fidelity …] [--json]`** — the tree of addressable
  nodes with id, kind, a ~60-character preview and the measured token cost of each, plus what
  the whole document would cost. On the largest corpus document the outline is 3.8 % of the
  document's own tokens (Phase 10 budget: under 5 %).
- **Corpus token budget** `corpus/token-budget.md`: what every corpus document costs at each
  fidelity level, committed as a golden and checked in CI. An update that inflates the corpus
  total by more than 5 % is refused unless `DOCSAI_ACCEPT_TOKEN_INFLATION=1` says so.
- Corpus fixture `docx/long-report.docx` (~9 000 tokens): the first corpus document sized for
  measuring rather than for isolating a trait.
- `docsai_docmark::serialize_traced` and `NodeFragment`: the same output as `serialize`,
  byte for byte, plus what each addressed node contributed, with `descendants` recording how
  many fragments it contains so the flat list rebuilds into a tree. `docsai-convert::tokens`
  (`token_report`, `token_report_path`) and `docsai-convert::outline` (`outline`,
  `outline_path`) build on it.

- **Phase 7 MCP server**: `docsai mcp` over stdio (`rmcp`) with tools
  `convert_to_markdown`, `convert_from_markdown`, `inspect_document`, and
  `list_supported_formats`. Path and base64 input modes, inline or on-disk assets,
  `DOCSAI_MCP_MAX_INPUT_BYTES` / `DOCSAI_MCP_TIMEOUT_SECS` limits.
- `docsai-convert::service` helpers for in-memory / bytes conversion shared by MCP.
- `docsai inspect` — structure report (metadata, styles, sections/sheets, media, stats)
  without converting; `--json` for the machine-readable shape shared with MCP.
- Batch conversion: `docsai convert *.docx --out-dir md/` with rayon parallelism and an
  aggregated summary (`--json` supported).
- Stdin/stdout pipelines: pass `-` as the input and/or `--output -` for DocMark text.
- `--style-map <file>` publication mode (DocMark spec §5): map source styles to `h1`–`h6`,
  `p`, `ignore`, or `code-block` (unidirectional).
- `--max-cells N` safety cap for workbook conversions.
- Actionable CLI error hints for unknown formats, unsupported conversions, and LibreOffice.
- `cargo-dist` configuration and release installers (shell/powershell) for the five
  primary targets.

### Changed

- **The front-matter parser names the versions it accepts.** `docmark: "1.0"`, `"1.1"` and `"1.2"`
  parse; anything else is refused by name. Previously any `1.x` was accepted, so a future `1.3`
  would have been read as if it were the current profile — a silent misreading rather than an
  error a caller can act on.

- **BREAKING (MCP): `convert_to_markdown` no longer returns image bytes by default.** The
  default is now `include_images=refs` — name, MIME type and size per image — where it used
  to be the whole base64 payload. On a document holding one 1200 × 900 screenshot the
  response drops from **906 709 bytes to 2 289** (0.25 %); at `thumbnails` it is 83 956
  (9.3 %), still a picture the client can look at.

  *Migration*: a client that wants the old behaviour passes `include_images: "full"`. A
  client already passing `assets: "inline-base64"` **needs no change at all** — an explicit
  request is still honoured, only the default moved. The `markdown` field is byte for byte
  identical at every rung, so a client that only reads the text is unaffected.

  Why break it: an image outweighs every word around it once it is base64, which made the
  tool unusable on exactly the documents an agent most wants to read. The old default
  charged for content the caller had not asked for and usually could not use; the new one
  makes wanting the pixels an explicit choice.
- MCP `list_tools` now returns **seven** tools rather than four.
- `--fidelity full` output is now **DocMark 1.1** and declares `next-id`; documents
  without ids (and `--ids never`) still declare `1.0`. A 1.0 document parses unchanged
  and gains ids on its next write. MCP `convert_to_markdown` inherits the new default.
- `Inline::Footnote` now carries a `Footnote { id, blocks }` and `Inline::Image` boxes
  its `ImageRef` (library API change; serialised JSON is unchanged).
- CLI `--help` documents the full Phase 6 command surface with examples.
- README usage section covers inspect, batch, style maps, and install paths.
- Workspace MSRV raised to **1.88** (required by `rmcp` 3.x).

## [0.1.0] — 2026-08-01

### Added

- Initial public workspace: Phases 0–5 core path (docx/odt/xlsx/ods ⇄ DocMark, xls read,
  legacy `.doc` read with optional LibreOffice fallback).
- `docsai convert`, `formats`, and `roundtrip` commands.
