---
tags:
    - phase-19
    - legacy
    - ppt
    - security
    - reader
---
# 105 — Phase 19-D: the degraded native `.ppt` reader

Fourth increment of [[95-phase-19-plan]], the second half of plan task 2 plus task 3. Its
acceptance criterion is the phase's second: **without LibreOffice, full text with correct slide
boundaries, no panics.** It is built to the shape [[99-phase-19-parser-choice]] specified, on the
fixtures [[97-phase-19-corpus]] built, in the seam [[102-phase-19-loffice]] left.

`--use-loffice never` on a `.ppt` **reads it now.** `ReadError::NoNativeRoute` — 19-B's honest
placeholder — is no longer constructed anywhere; the variant stays because it is public API and
the next legacy format to be routed before it is parsed will want it.

## What changed

- `crates/docsai-office/src/ppt/` — the module 19-B left as a seam, filled: `window.rs` (122),
  `records.rs` (197), `persist.rs` (214), `text.rs` (305), `images.rs` (125) and `mod.rs` (545)
  of production Rust — **1 508 lines**, against 19-C's estimate of 1 000–1 400. It is over, by
  about 8 %, and the overrun is comment: these files are roughly a quarter documentation, because
  the record layouts are the kind of thing nobody re-derives twice. Another 529 lines of tests
  bring the module to 2 037.
- `crates/docsai-office/src/error.rs` — one additive variant, `ReadError::Truncated { part, at,
  need, have }`, exactly as 19-C proposed and for the reason it gave.
- `crates/docsai-office/src/pptx/{order,text}.rs` — `order::sort` and `text::build_list` widened
  from `pub(super)` to `pub(crate)`. **No algorithm was copied.** The reading-order policy and the
  outline-level-to-nested-list rebuild are the *IR's* shape rather than either format's, and a
  second copy of either is how they drift apart.
- `crates/docsai-office/src/doc/mod.rs` — `read_stream` and `read_summary_meta` likewise, for the
  same reason: the OLE2 stream cap and the `\5SummaryInformation` title/author scrape are the
  container's, not `.doc`'s.
- `crates/docsai-convert/tests/ppt_phase19.rs` — 19-B's `never_refuses_by_saying_what_is_missing`
  replaced by six tests of the native route, none of which needs LibreOffice, so none is ever
  skipped.

## What the native route recovers, measured rather than claimed

Both fixtures were converted with `--use-loffice never` and compared, **line for line**, against
the `.pptx` originals 19-A kept in the tree for exactly this.

| | `basic-slides.ppt` | `mixed-media.ppt` |
|---|---|---|
| Slides, in order | **2 / 2** | **3 / 3** |
| Titles | identical to the `.pptx` | identical |
| Every bullet, at its own indent level | identical | identical, **two levels deep** |
| The picture | — | `img-40e10599.png`, the **same content hash** as the original, at `pos="160px,240px"` — the original's own position |

`the_native_route_reads_the_same_text_and_boundaries_as_the_pptx_original` is that comparison as
an assertion: the headings and bullets of the legacy deck, with the DocMark attribute syntax
stripped, must equal the headings and bullets of the `.pptx`. It passes on both pairs.

Against the **LibreOffice** route on the same file, the difference is one thing and it is the
expected one:

```
## Informe trimestral                                     ← native
## [Informe trimestral]{bold=false color=#000000 font="Calibri Light" size=44pt}   ← via LibreOffice
```

LibreOffice's export has no OOXML layout parts to inherit from, so it writes every run's
formatting explicitly ([[102-phase-19-loffice]] §«fidelity»). The native route reads no character
formatting at all. **Both are further from the original than they look**: the LibreOffice line
carries formatting the original inherited and never stated, the native line carries none. On the
text and the boundaries — which is what criterion 2 asks about — they agree, and both agree with
the `.pptx`.

## What it does not recover, and the one warning that says so

One `Warning::Degraded { what: "ppt" }`, unconditional, listing character and paragraph
formatting, layouts, masters, themes, tables, charts, speaker notes and any shape that is neither
text nor picture. The list is **invariant** — no `.ppt` read by this path gets any of it — so the
warning is never false, which is the property 19-B's route warning was built for too.

`the_native_route_warns_once_and_only_about_what_it_really_lost` asserts the *other* half of
AGENTS.md §7 rule 3 on both ordinary decks: **exactly one warning**, and it is the route. Nothing
warns about a loss that did not happen.

### The picture, and a correction to 19-C

19-C wrote that «a `Pictures` BLIP has no shape anchoring in the stream, so a recovered picture is
an asset with no geometry → `Warning::ImageGeometryDegraded`, emitted per picture recovered». That
is wrong, and following it would have shipped a **false warning on every picture in every deck**.

The anchoring exists and is three hops: the shape's `OfficeArtFOPT` property `pib` (0x0104) is a
one-based index into the `OfficeArtBStoreContainer` of the drawing group; that entry's `foDelay`
is the BLIP's byte offset in the `Pictures` stream; and the shape's own `OfficeArtClientAnchor`
(0xF010) is where it sits, in master units. `mixed-media.ppt`'s picture comes out at
`pos="160px,240px"`, which is the position `mixed-media.source.pptx` states, so the geometry is
recovered rather than approximated.

`ImageGeometryDegraded` therefore fires **only** for a BLIP in the `Pictures` stream that no
shape's `pib` points at: that one really does arrive with no position and no size. Neither fixture
has one, and neither fixture warns.

Two BLIP families are recorded as `AssetIssue` rather than recovered: metafiles (EMF/WMF/PICT),
whose payload is deflate-compressed behind a 34-byte header, and DIB, which is a
`BITMAPINFOHEADER` with no file header and is not a file anyone can open. Handing either on as
bytes would be worse than saying so. No fixture has one; the code path is unit-tested.

## How slide boundaries are reconstructed, and how we know they are right

Not by scanning for `RT_Slide` records, which would find the master and the notes masters too and
would put them in file order. The route is [MS-PPT] 2.1.2, followed exactly:

1. `Current User` → `CurrentUserAtom.offsetToCurrentEdit`.
2. That offset → the newest `UserEditAtom`; its `offsetLastEdit` chains back through every earlier
   edit, bounded by `MAX_EDIT_CHAIN` (4 096) **and** a visited set.
3. Each edit's `offsetPersistDirectory` → a `PersistDirectoryAtom`, merged **newest first**, so an
   id an earlier edit also names keeps the newer offset.
4. The newest edit's `docPersistIdRef` → the `DocumentContainer`.
5. Inside it, the `SlideListWithTextContainer` — `RT_SlideListWithText` with **`recInstance`
   0x000**; instance 1 is the masters and 2 the notes, and getting that backwards is how a reader
   ends up converting the master. Its `SlidePersistAtom`s are the deck's own order, and each one's
   `persistIdRef` resolves through the directory to a `SlideContainer`.

We know it is right three ways: the slide *count* matches the `.pptx` on both fixtures, the slide
*order* matches by title, and — the check that would catch a list we misread — every persist entry
the list did **not** name is looked at, and a `Warning::Degraded { what: "ppt-slides" }` counts
any that is a `RT_Slide`. Neither fixture produces one, which is what says the list named them
all.

Within a slide, text is not read from `SlideListWithText` at all: these decks' `SlidePersistAtom`s
carry no text records, so the outline is empty and a reader that trusted it would return nothing.
It is read from the shape tree — `RT_Drawing` → `OfficeArtDgContainer` →
`OfficeArtSpgrContainer` → `OfficeArtSpContainer` → `OfficeArtClientTextbox` — which is also what
gives each run a shape, a position and a placeholder identity.

### Title against body, without guessing

`TextHeaderAtom.textType` says which: `Tx_TYPE_TITLE` and `Tx_TYPE_CENTERTITLE` become
`PhType::Title`/`CenterTitle`, the four body types become `PhType::Body`, and `Tx_TYPE_OTHER` with
no `PlaceholderAtom` beside it becomes a `ShapeKind::TextBox`. That last case is not a fallback:
LibreOffice really did write these decks' bodies as free text boxes, and the LibreOffice route
reads its own export back the same way ([[102-phase-19-loffice]] records the `::: {.shape}`
wrapper as one of the differences from the `.pptx`). The native route agreeing with it is a check
on both.

### Outline levels, read rather than flattened

`StyleTextPropAtom`'s `rgTextPFRun` carries the level, so the nesting is rebuilt rather than lost —
`mixed-media.ppt` slide 2 comes out `- Nivel cero` / `  - Nivel uno`, which is what the `.pptx`
says. Reaching the level means walking a whole `TextPFException`, every field of which is optional
behind a bit in `PFMasks` (including a variable-length `tabStops`), so the parser walks all
seventeen in order. The bullet flag is read the same way and **only believed when the mask says it
is stated**: a title states `fHasBullet = 0` and a body states 1, so neither is a guess and neither
is inherited-and-assumed.

This is also 19-C's third objection to `office_oxide` answered: we are the ones reading the atom,
so we know whether a deck *had* a level to lose. Nothing is flattened, so nothing warns about
flattening.

## The bounds-checking structure as built

19-C's five rules, and where each one lives:

1. **One `Window` is the only way to read.** `window.rs`. It carries the bytes, its absolute base
   in the stream, the stream length and the stream name; every accessor (`u8_at`, `u16_at`,
   `i16_at`, `u32_at`, `i32_at`, `array::<N>`, `slice`, `sub`, `advance`) returns `Result`. There
   is exactly **one** range expression in the module — `bytes.get(off..end)` — and it is reached
   only after `checked_add`.
2. **Refuse, never clamp.** There is no clamping accessor, so the `.min(bytes.len())` pattern is
   *unavailable* rather than discouraged. `#![deny(clippy::indexing_slicing)]` at the top of
   `ppt/mod.rs` covers every submodule and makes it mechanical; the test modules carry an explicit
   `#[allow]` with the reason, because a panicking index in a test is a failing test.
3. **`recLen` is checked before it is used.** `RecordIter::next` validates the declared length with
   `Window::sub` *before* any read of the body. Three further properties it needs and has: a
   zero-length record still advances by eight, so no walk stands still; a child's body is a strict
   sub-window eight bytes further in, so no record can contain itself; and trailing bytes too short
   to be a header are an error rather than padding to skip.
4. **`offsetToCurrentEdit` is checked by the same `Window`, with no scan fallback.** `persist.rs`.
   Every persist directory offset is header-checked once when the directory is built, so a
   resolved persist id is a real container everywhere it is used.
5. **Encryption is read first**, before any content — see below.

Plus the bounds a hostile file needs that the specification does not: `MAX_RECORD_DEPTH` (24) on
container recursion, `MAX_SHAPE_DEPTH` (16) on the shape tree, `MAX_EDIT_CHAIN` (4 096) plus a
visited set on the edit chain, and a `StyleTextPropAtom` run expanded only as far as the text
really is — a run that declares `u32::MAX` characters produces exactly `text.len() + 1` of them,
which is unit-tested.

## The exact point at which `truncated.ppt` is refused

```
$ docsai convert corpus/ppt/truncated.ppt --use-loffice never
docsai: `PowerPoint Document` is truncated: 8 byte(s) wanted at offset 10946, but the stream holds 5632
exit=2
```

`persist::read_persist_directory` → `records::header_at` → `Window::array::<8>` →
`Window::slice(10946, 8)` on a 5 632-byte stream. The eight bytes are the `UserEditAtom`'s record
header, and `10946` is what `CurrentUserAtom.offsetToCurrentEdit` states — the first thing
[MS-PPT] tells a reader to follow. There is no second attempt. `office_oxide` returns `Ok` here
with one slide of PowerPoint's placeholder prompt text ([[99-phase-19-parser-choice]]) precisely
because it has one.

The fixture carries **two** traps and the reader above only trips the first, so the second is
asserted directly rather than left untested:
`a_naive_reclen_walk_of_the_truncated_stream_is_refused_too` walks the top-level records of the
same stream and stops at the header at offset 5 300, whose declared body ends at **7 164** —
1 532 bytes past the end — with an error carrying `at = 5308`, `need = 1856`, `have = 5632`.
**From the declared length alone, with no seek and no read**, which is the shape 19-A specified.

`the_truncated_fixture_is_refused_and_the_refusal_says_where_and_why` pins all four numbers of the
first refusal, so a future change that turns it into a generic «malformed» fails.

## What the reader does with `encrypted.ppt` today — for 19-E

It **reads it, and never silently.** `CurrentUserAtom.headerToken` is the first thing read, before
the `PowerPoint Document` stream is even opened, and `0xF3D1C4DF` produces a **`Severe`** warning
naming the token, the stream it came from, and the fact that what follows is not the document's
content:

```
$ docsai convert corpus/ppt/encrypted.ppt --use-loffice never --verbose
docsai: 2 warning(s), 1 severe, 0 raw-block(s)
  [Minor] degraded ppt: the native MS-PPT path recovers slides, …
  [Severe] unsupported element <encrypted .ppt (CurrentUserAtom.headerToken = 0xF3D1C4DF)> at
           Current User: read anyway; whatever comes out is not this document's content, and
           refusing it is increment 19-E
```

An unrecognised token — neither `0xE391C05F` nor `0xF3D1C4DF` — warns too, at `Severe`.

**Refusing is deliberately not done here**, and the reason is 19-B's finding rather than
timidity: LibreOffice reads the encrypted fixture happily and never consults the token, so a
refusal that lived only in this reader would leave `--use-loffice auto` reading encrypted decks
through the other route. 19-E owns criterion 3 and the check belongs **in front of** the hand-off,
shared by both routes. Making it here would have to be undone there.

## Defects found

1. **[[99-phase-19-parser-choice]]'s picture warning is wrong.** «A `Pictures` BLIP has no shape
   anchoring in the stream» — it has, through `pib` → `foDelay` → the client anchor. Following the
   entry as written would have emitted `ImageGeometryDegraded` on every picture in every deck: a
   false warning, which §7 rule 3 calls the same failure as a missing one. **Not fixed in 19-C's
   text** — the entry is that increment's record and rewriting it would erase the decision it
   documents — but corrected here, and the warning is emitted only for an unreferenced BLIP.
   Repro: the entry's §«How the degraded status reaches the report», against
   `crates/docsai-office/src/ppt/images.rs:1`.
2. **`master_units` truncated on the first attempt.** `914_400 / 576` is **1 587.5**, and a
   `const EMU_PER_MASTER_UNIT: i64 = 914_400 / 576` silently becomes 1 587, which put a 10-inch
   slide at 9 141 120 EMU instead of 9 144 000 — 2 880 EMU short, and every shape's position wrong
   with it. Caught by `basic_slides_comes_back_whole` before it left the increment. Fixed at
   `crates/docsai-office/src/ppt/mod.rs:79` by multiplying before dividing. Repro: the test, on the
   first version of that constant.
3. **`ReadError::NoNativeRoute` is now unconstructed.** Not dead — it is `pub` and its message
   shape is the right one for a format recognised but not parsed — but nothing in the workspace
   builds one today. Recorded rather than removed, because removing a public variant is a breaking
   change to make deliberately, not as a side effect. `crates/docsai-office/src/error.rs:38`.

Nothing else. The `.doc` piece table's two `.min(bytes.len())` clamps
(`piece_table.rs:237`, `:253`) were re-read and are still what 19-C says: not a defect today, and
the first place Phase 20's fuzzing should look.

## Gates

All six run, all green, and the corpus is **clean after two consecutive `cargo test --workspace`
runs** (`git status --short corpus/` empty both times — every conversion in the test file gets its
own `tempfile::TempDir`, as 19-B established):

| | |
|---|---|
| `cargo test --workspace` | **60 suites, 968 passed, 0 failed, 1 ignored** (was 936; +27 unit, +5 integration) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `python3 corpus/generate.py --check` | 201 generated files, unchanged by this increment |
| `python3 corpus/schema_gate.py` | 21 decks, 42 packages, **691 parts**, 0 invalid |
| `render_gate.py --require-renderer` | 21 decks + 17 `.odp`, 38 identical by bytes, **254 pages**, 0 failures |

No gate loosened, no budget raised, no golden moved. `corpus/` is untouched: this increment adds
no fixture and needs none, because 19-A built the four it uses.

## Carried into 19-E and beyond

- **Criterion 3 is 19-E's, and the token check must move in front of the hand-off.** This reader
  already reads `headerToken` first and warns `Severe`; 19-E turns that into a refusal in a place
  both routes pass through, and then this warning becomes unreachable and should be replaced
  rather than left.
- **`ReadError::NoNativeRoute` has no constructor** (defect 3 above).
- **Speaker notes are not read.** The `SlideAtom` carries a `notesIdRef` and the persist directory
  resolves it, so it is a bounded piece of work — it was left out because plan task 3 does not name
  it and the blanket warning states the loss.
- **No character or paragraph formatting.** `StyleTextPropAtom`'s `rgTextCFRun` is walked past
  rather than parsed; the level and the bullet flag are all this increment takes from the atom.
- **`TextBytesAtom` is decoded as Latin-1**, which is what [MS-PPT] says (the high byte is
  implicitly zero). Neither fixture contains one — LibreOffice writes `TextCharsAtom` throughout —
  so that path is unit-tested but not fixture-tested. A deck written by PowerPoint itself would
  exercise it.
- **The reading-order policy and the list rebuild are now shared with `pptx`.** If either changes,
  it changes for both, which is the point; but the `.ppt` goldens are the DocMark in this entry and
  the tests, not a golden file, so a change there will surface as a test failure rather than a
  diff.
- **No `.ppt` writer, and still no route to one.** Read-only for the whole of Phase 19.

[[95-phase-19-plan]] · [[97-phase-19-corpus]] · [[99-phase-19-parser-choice]] ·
[[102-phase-19-loffice]] · [[08-phase-5-legacy-doc]]
