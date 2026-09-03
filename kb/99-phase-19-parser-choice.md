---
tags:
    - phase-19
    - decision
    - legacy
    - ppt
    - office-oxide
    - security
---
# 99 — Phase 19-C: `office_oxide` weighed, and the shape of the native `.ppt` route

Increment 19-C of [[95-phase-19-plan]], the first half of plan task 2
([`docs/development-plan-v2.md:368`](../docs/development-plan-v2.md)), which says the crate is
**evaluated before it is adopted** and that adopting it must be justified in writing under
AGENTS.md §7.4. The deliverable is a decision, not a feature. Same shape and same rigour as
[[31-phase-12-spike-p1]], which weighed `ooxmlsdk` and declined it.

**No code changed in any crate. The dependency was not added — neither was it added and then
removed; the measurements were taken in a probe crate outside the repository.**

## Decision

**`cfb` plus an atom walk of our own.** `office_oxide` 0.1.9 is **not linked**. It is kept as a
**cross-check on [MS-PPT] record layout** — the role `ooxmlsdk` was left in by 12-B — because its
`.ppt` module is careful, correct on the two ordinary fixtures, and MIT/Apache-2.0 readable.

The reason is **not** cost. The cost turned out to be small, and that is stated first so the
argument cannot be mistaken for the usual one:

| | `ooxmlsdk` (12-B, rejected) | `office_oxide` (here) |
|---|---|---|
| Stripped binary | **+28.7 MB** | **+98 KB** (310 272 → 410 912 B) |
| Clean release build | **+311 s** | **+30 s** (0 → 30.13 s) |

`office_oxide` is a **cheap** dependency. It is rejected on **behaviour**, on the two fixtures
19-A built to be refused, and the whole of the argument is in the third section below.

## What `office_oxide` turned out to be — evidence, not the plan's assumption

The plan was written earlier and the crate had to be checked rather than believed. It exists,
under that name, and is real:

| | |
|---|---|
| crates.io | `office_oxide`, first published **2026-04-28**, latest **0.1.9** on **2026-09-01** |
| Versions | 10, none yanked; a release roughly every three weeks |
| Downloads | 491 728 total, **446 319 in the last 90 days** — so nearly all of it is recent |
| Licence | **MIT OR Apache-2.0** — compatible with this project's MIT |
| Repository | `github.com/yfedoseev/office_oxide`, 117 ★, 17 forks, pushed **2026-09-02** (the day before this evaluation) |
| Open issues | **14**, of which 8 are Dependabot bumps; the substantive ones are a Go installer 404 (#110), an encryption *feature request* (#119) and a heading-level gap (#127) |
| Crate size | 346 622 B packaged; 40 540 lines of Rust across all six formats |
| MSRV / edition | **rust 1.88, edition 2024** — 1.88 is exactly this workspace's `rust-version`, so it costs no MSRV bump |

It does what the plan assumed and more: DOCX, XLSX, PPTX, **DOC, XLS, PPT**, read and write, pure
Rust, no C. It carries **its own** compound-file reader (`src/cfb/`, 1 339 lines) rather than
depending on the `cfb` crate, so adopting it would mean two OLE2 implementations in one binary.

### The dependency graph, measured

`cargo tree -e normal` on a probe with default features (all of `mmap`, `parallel`, `python`,
`wasm` are opt-in and off):

```
office_oxide 0.1.9
├── atoi_simd 0.18.1 → debug_unsafe 0.1.4, rustversion
├── encoding_rs 0.8.35 · fast-float2 0.2.4 · libc 0.2.189 · log 0.4.34
├── quick-xml 0.41.0 (serialize) · serde 1 · serde_json 1 · thiserror 2
└── zip 8.6.0 → crc32fast, flate2 → zlib-rs, indexmap, typed-path, zopfli
```

Small and pure Rust. But two of those rows are **duplicates of crates this workspace already
pins at a different major**:

- **`quick-xml` 0.41** beside the workspace's **0.37** (and a third already in the lock via
  `calamine`) — a fourth copy, compiled and linked;
- **`zip` 8.6** beside the workspace's **2** — a second ZIP implementation, with `zopfli`,
  `zlib-rs` and `typed-path` behind it.

LTO strips the unreachable halves out of the *binary* (hence the modest +98 KB when only
`office_oxide::ppt` is called), but the **compile** cost is paid unconditionally, and a second
ZIP and a fourth XML parser in the graph of a signed binary is a maintenance surface, not a
rounding error. New-to-this-tree crates: `atoi_simd`, `debug_unsafe`, `fast-float2`,
`typed-path`, `zlib-rs`, `zopfli`.

Everything above was measured on this machine, in
`/tmp/.../scratchpad/oo-probe`, outside the repository, and neither committed nor added
to the workspace.

## What it does with `corpus/ppt/`, which is what decides it

The probe calls `office_oxide::ppt::PptDocument::open` on all four 19-A fixtures. Verbatim:

```
=== corpus/ppt/basic-slides.ppt
  Ok: 2 slides, 0 images
    slide 1: Title:"Informe trimestral" | CenterBody:"Ingresos al alza\rCostes estables"
    slide 2: Title:"Siguientes pasos" | CenterBody:"Cerrar el trimestre"
=== corpus/ppt/mixed-media.ppt
  Ok: 3 slides, 1 images
    slide 1: Title:"Legado en binario" | CenterBody:"Primera viñeta\rSegunda viñeta"
    slide 2: Title:"Segunda diapositiva" | CenterBody:"Nivel cero\rNivel uno"
    slide 3: Title:"Con imagen"
=== corpus/ppt/truncated.ppt
  Ok: 1 slides, 0 images
    slide 1: Title:"Pulse para editar el formato del texto de título" | Body:"Pulse para editar…"
=== corpus/ppt/encrypted.ppt
  Ok: 2 slides, 0 images
    slide 1: Title:"Informe trimestral" | CenterBody:"Ingresos al alza\rCostes estables"
```

**The good half, and it is genuinely good.** The two ordinary decks come back with the right
slide count, the right boundaries and every text run, including the picture deck's `Pictures`
BLIP. **No panic on any of the four**, including the truncated one — memory safety is not the
complaint here and it should not be implied. Its `persist.rs` walks
`CurrentUserAtom.offsetToCurrentEdit` → the `UserEditAtom` chain via `offsetLastEdit` → merged
`PersistDirectoryAtom` entries, newest-edit-first, with a 4 096-step cap and a visited set
against cycles. That is the [MS-PPT] 2.1.2 algorithm done properly, and its
`corrupt_nested_record_length_does_not_swallow_top_level_siblings` test says the author thought
about exactly the trap 19-A built.

**The half that decides it.** Both refusal fixtures came back `Ok`:

1. **`encrypted.ppt` → two readable slides.** `grep -rn "F3D1C4DF" src/` finds nothing:
   `CurrentUserAtom.headerToken` ([MS-PPT] 2.3.2) is **never consulted anywhere in the crate**,
   for any format. The only mention of encryption in the whole source is `xls/workbook.rs:98`,
   a comment. Issue **#119** confirms it — password protection is an *open feature request*, not
   a gap in our reading. Phase 19's **third acceptance criterion** is «encrypted/protected `.ppt`
   rejected with a clear error». `office_oxide` cannot meet it, and it is not something a caller
   can bolt on: the token is a property of the container the crate does not surface.
   *(Our fixture's content behind the flag is not ciphertext — 19-A says so plainly — so the two
   slides here are the plaintext that is really there. That does not soften the finding: on a
   genuinely encrypted deck the same code path returns garbage or nothing, with no error either
   way.)*

2. **`truncated.ppt` → one slide, silently.** The deck lost half its `PowerPoint Document`
   stream; `offsetToCurrentEdit` points **5 314 bytes past the end**. `office_oxide` does not
   refuse: `persist::build` *falls back* to scanning for the highest-offset `UserEditAtom` when
   the offset does not resolve, and what it finds is the **master slide**. The text returned —
   «Pulse para editar el formato del texto de título», «Segundo nivel del esquema»… — is
   PowerPoint's **placeholder prompt text**, not one word of the deck's content. A caller gets
   `Ok`, one slide, and no way to tell this from a real presentation. That is precisely
   **AGENTS.md §7 rule 3, never degrade fidelity silently**, on the one fixture the phase built
   to test it. The fallback is a defensible choice for a text-extraction library and the wrong
   one for a converter that must report every loss.

3. **The IR is text-only, and not extensible from outside.** `TextRun` is exactly
   `{ text_type: TextType, text: String }`. There is **no indent level** — `RT_STYLE_TEXT_PROP`
   (0x0FA1) is a declared constant the crate never parses — so `mixed-media.ppt`'s two bullet
   levels are unrecoverable through its API; there is no shape identity or geometry; and a
   slide's paragraphs arrive **`\r`-joined into one `String`** rather than separated. docsai's
   target is `Presentation → Slide { shapes: Vec<Shape> }` (`docsai-model::presentation`).
   The seam is lossy, the losses are *structural* rather than per-document, and — worse than the
   loss itself — we could not warn about them accurately, because from outside the crate there is
   no way to know whether a given deck *had* a second bullet level to lose. A false warning is
   the same failure as a missing one.

## What the alternative concretely is, and how big

`cfb` 0.14 is already a workspace dependency and already carries the `.doc` reader, so the
alternative adds **no crate at all**. What it adds is code, and the size is estimable from two
independent measurements rather than guessed:

| Reference | Production Rust |
|---|---|
| Our `.doc` native reader (`crates/docsai-office/src/doc/`, excluding `test_fixture.rs`) | **~1 004 lines** — `fib.rs` 124, `piece_table.rs` 236, `images.rs` 229, `mod.rs` ~400 |
| `office_oxide`'s `.ppt` route (`src/ppt/`, excluding `#[cfg(test)]`) | **902 lines** — `records` 145, `persist` 201, `text` 383, `document` 117, `error` 26, `images` 7 |
| …plus its BLIP parser, which we would also need (`src/cfb/blip.rs`) | ~160 lines |

So **~1 000–1 400 lines**, the same order as the `.doc` reader that already exists and is
maintained here. The surface is bounded and enumerable, and both references agree on what it
contains: the [MS-PPT] record header, `CurrentUserAtom`, the `UserEditAtom` chain, the persist
directory, `SlideListWithTextContainer` / `TextHeaderAtom`, `TextCharsAtom` (UTF-16LE) /
`TextBytesAtom` (Latin-1), and the `Pictures` stream's BLIP records. That is not a 400-page
specification's worth of work; it is the same slice of one that `.doc` already took.

## The security surface, and which way it cuts

The phase plan names this up front, and it cuts **both** ways honestly:

- *Against writing our own*: a 400-page specification, and code we must get right on hostile
  input.
- *Against adopting*: a parser we did not audit, running on attacker-shaped bytes, inside a
  binary this project signs — and one whose failure mode on the corrupt fixture is to **return
  plausible-looking wrong content** rather than to stop.

The second is the sharper edge here, and the fixture proved it rather than the README. Note also
that `office_oxide` is not `#![forbid(unsafe_code)]`: it carries six `unsafe` uses, of which the
`memmap2` ones are behind an opt-in feature but `lib.rs:110`'s `libc::getrlimit` stack probe runs
on any call. None of that is wrong, and it is not the reason for the decision — it is part of the
audit surface being accepted or not, and it is recorded so the trade is visible.

Against that, the *native* route's exposure is bounded by construction: it opens one OLE2 file
through `cfb` (audited by nobody here either, but already in the binary for `.doc`, `.xls` and
the `.docx` OLE embeddings, so it is not new surface), and everything above it is a walk over one
`&[u8]` with no allocation driven by a length the file declares.

## Justification under AGENTS.md §7.4, stated as the rule asks

> *is it maintained?* — Yes, actively; four months of steady releases.
> *is it pure-Rust?* — Yes, no C, no build scripts of concern.
> *what does it add to binary size?* — +98 KB stripped, +30 s clean release build, six new
> crates, a fourth `quick-xml` and a second `zip`.
> *leave the justification* — This is it, and it is a **rejection**: the cost is acceptable and
> the behaviour is not. It fails acceptance criterion 3 outright, fails §7 rule 3 on the
> truncated fixture, and its `.ppt` model cannot carry the IR this project converts into.

If the criteria were «extract the text of a valid `.ppt` quickly», the decision would go the
other way and it would be an easy call. They are not.

## The native route's shape, as 19-D will build it

### Where it lives

```
crates/docsai-office/src/ppt/
    mod.rs       pub fn read(reader, assets) -> Result<(Document, ConversionReport), ReadError>
    window.rs    Window — the only thing that turns an offset into bytes
    records.rs   RecordHeader, RecordIter (bounds-checked, non-recursive at each level)
    persist.rs   CurrentUserAtom, the UserEditAtom chain, PersistDirectory
    text.rs      SlideListWithText, TextHeaderAtom, TextCharsAtom/TextBytesAtom, StyleTextPropAtom
    images.rs    the Pictures stream's BLIPs
```

A **sibling of `doc/` inside `docsai-office`**, because that is where the OLE2 binary formats
already live (`doc/`, `xls/`) and because AGENTS.md §3 forbids a new crate. It is `pub(crate)`
except for `read`, which `lib.rs` re-exports the way `doc::read` is.

### What it returns

The same signature as `doc::read`: `(Document, ConversionReport)`, with the document a
`Document::Presentation(Presentation)` whose `slides: Vec<Slide>` carry `shapes: Vec<Shape>`.
That is deliberate — `docsai-convert::pipeline` wires it exactly where `.doc` is wired
(`pipeline.rs:243`), so `--use-loffice auto|never|require` governs it with the code 19-B already
has and no second policy is invented.

### How the degraded status reaches the report

As `.doc` does, and no differently:

- **One `Warning::Degraded { what: "ppt", why: … }` at the top of `read`**, unconditional,
  naming what LibreOffice would have given instead — the native route is *always* degraded, so
  this warning is never false.
- **Per-loss warnings only where the loss actually happened.** A `Pictures` BLIP has no shape
  anchoring in the stream, so a recovered picture is an asset with no geometry →
  `Warning::ImageGeometryDegraded`, emitted **per picture recovered**, not per file. If 19-D
  flattens bullet levels rather than reading `StyleTextPropAtom`, that flattening is a typed
  warning emitted **only for decks that had a level to flatten** — which we can know, because
  unlike `office_oxide` we are the ones reading the atom.

### How bounds checking is structured

The point is that `truncated.ppt` is refused by **our** code, deliberately, rather than by luck
or by `cfb`. 19-A already established that `cfb::open` accepts that file **without a word** and
hands back a 5 632-byte stream, so every guard has to be ours. It is structural, not sprinkled:

1. **One `Window<'a>` is the only way to read.** It owns the `&[u8]` and every accessor —
   `u8_at`, `u16_at`, `u32_at`, `slice(off, len)`, `sub(off, len)` — returns `Result`. Offsets
   are combined with `checked_add` and compared `<=` against the length. No `&bytes[a..b]`
   anywhere in the module; `#![deny(clippy::indexing_slicing)]` on the module makes that
   mechanical rather than a habit that erodes.
2. **Refuse, never clamp.** The `.doc` reader's `piece_table.rs:237` and `:253` end
   `.min(bytes.len())`, which silently *shortens* an over-long read. That is tolerable there —
   the loss is text and text loss is warned — but it is exactly the wrong pattern here: the same
   clamp would turn `truncated.ppt`'s **1 532-byte overrun into a silently short record** and we
   would ship the very failure `office_oxide` was rejected for. `Window` has no clamping
   accessor at all, so the pattern is unavailable rather than discouraged.
3. **`recLen` is checked before it is used.** `RecordIter` yields a child only when
   `header_end.checked_add(rec_len)? <= window.end`. `truncated.ppt`'s header at offset **5 300**
   declaring a length reaching **7 164** is therefore an `Err` from the declared length alone,
   **with no seek and no read**, which is the shape 19-A specified.
4. **`offsetToCurrentEdit` is checked by the same `Window`.** 10 946 against a 5 632-byte stream
   fails at `Window::sub`. And — the decision that separates us from `office_oxide` — **there is
   no scan fallback.** A file whose `Current User` disagrees with its stream is refused, because
   a deck recovered by guessing is the silent degradation this phase exists not to produce. The
   error names the offset and the stream length so the message is diagnostic rather than
   "malformed".
5. **Encryption is checked first, before any content.** `CurrentUserAtom.headerToken ==
   0xF3D1C4DF` → `ReadError::Encrypted` (the variant already exists, and `doc/mod.rs` already
   returns it off the FIB's `fEncrypted`). It is the *first* thing read, because the whole point
   of the token is that it precedes parsing.
6. **Recursion and chains are bounded.** Container records nest, so the walk carries a
   `MAX_RECORD_DEPTH`; the `offsetLastEdit` chain carries a `MAX_EDIT_CHAIN` **and** a visited
   set against cycles. `office_oxide`'s 4 096-plus-`HashSet` pair is the same shape and is
   confirmation of the requirement, not the source of the code.

19-D proposes one additive `ReadError` variant, `Truncated { part, at, need, have }`, because
`WrongShape` would flatten four useful numbers into prose. That is a change to `crates/`, which
19-C is not permitted to make, so it is written here for 19-D to make.

### What 19-D owns as tests

All four 19-A fixtures: the two ordinary decks green against new goldens
(`crates/docsai-convert/tests/doc_phase5.rs` is the shape to copy), `truncated.ppt` → `Err`
whose message names the offset, `encrypted.ppt` → `ReadError::Encrypted`, and a unit test that
`Window` refuses an `off + len` that overflows `usize`. Phase 20's fuzzing inherits the module,
and the `Window` type is what makes that cheap.

## Provenance, so a later reader is not confused

`office_oxide` is **not linked and no code is derived from it.** [MS-PPT] is the normative
source for everything 19-D writes; the crate was read to *cross-check* record identifiers and to
confirm that the persist-directory algorithm is what the specification says it is. If any future
increment does derive code from it, its Apache-2.0 attribution requirements apply and must be
honoured in the file that does so.

## Carried forward

- **This decision is re-openable, and cheaply.** The probe is ten lines and the crate is a
  three-week release cadence away from having `headerToken`: issue #119 is exactly that feature
  request. If it lands, the two behavioural objections shrink to one (the text-only IR), and the
  cost side of the ledger was always in its favour. Re-run the four fixtures before re-deciding.
- **The `.doc` reader's `.min(bytes.len())` clamps** (`piece_table.rs:237`, `:253`) are a
  latent instance of the pattern rejected above. They are *not* a defect today — `.doc` warns its
  text loss — but if Phase 20's fuzzing looks anywhere, it should look there.
- **`cfb` opens `truncated.ppt` silently.** Every legacy reader in this tree therefore owns its
  own bounds checking; none of it can be delegated to the container crate.
- **The +98 KB / +30 s numbers are for `office_oxide::ppt` under LTO only.** A phase that used
  more of the crate would pay more, and the ZIP and XML duplicates would stop being stripped.

[[95-phase-19-plan]] · [[97-phase-19-corpus]] · [[31-phase-12-spike-p1]] · [[08-phase-5-legacy-doc]]
