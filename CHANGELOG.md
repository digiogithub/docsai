# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
