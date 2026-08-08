# 44 — Phase 13 J: `.pptm` and corrupt input

Increment **13-J** of [[34-phase-13-plan]], on top of [[43-phase-13-raw]]. The previous increments
made the reader keep everything a *valid* deck holds; this one is about the decks that are not
valid, and about the one variant of the format that carries executable code.

## What changed

- `pptx/mod.rs`: `Warning::MacrosIgnored` when the package carries a `vbaProject.bin`, the rule
  `docx/mod.rs` and `xlsx/mod.rs` have followed since Phase 8.
- `corpus/generate.py`: `build_pptx(macro_enabled=…)` and the `macro-enabled.pptm` fixture.
- `crates/docsai-office/tests/robustness.rs`: the presentation half of the suite — truncation,
  byte corruption, malformed XML, random noise, an unresolved slide relationship and a slide part
  that is not in the package.

## Non-obvious decisions

1. **A `.pptm` is not a different format.** It differs from a `.pptx` in its main content type and
   in carrying a VBA project; nothing about a slide changes. `Format::parse` already mapped `pptm`
   to `Format::Pptx` and `main_part` already accepted the macro-enabled content type, so the only
   thing missing was the statement that the project was ignored.
2. **The warning names the part it found, not a constant.** `docx` and `xlsx` hardcode
   `word/vbaProject.bin` and `xl/vbaProject.bin`; the pptx reader reports the actual part name,
   because it detected the part by suffix and a report that names a part the package does not have
   is worse than no report.
3. **Macros are ignored, not dropped.** The project part is never inspected or executed, but the
   skeleton ([[42-phase-13-skeleton]]) still holds the package byte for byte, so a lossless round
   trip neither runs the macros nor silently disarms the deck. Disarming a file is a change; a
   reader does not make changes.
4. **Detection stays content-based, and the fixture proves it.** The `.pptm` is detected as
   `Format::Pptx` with `DetectScore::Certain` from `ppt/presentation.xml`, not from its name — a
   deck renamed either way reaches the same reader (architecture §4).
5. **A dangling relationship and a missing part are graded differently.** A `p:sldId` whose `r:id`
   does not resolve is a warning and the deck reads without that slide: the package never claimed
   the slide was readable, and the rest of the deck is intact. A relationship that *does* resolve,
   to a part the package does not carry, is `ReadError::MissingPart` — the package promised a slide
   and has not got it, and handing back a deck one slide short is exactly how an agent deletes a
   slide by writing the deck out again. This is the only place in the increment where the answer
   was not already decided by the plan's "always a typed `Err`".
6. **The truncation fixture is `charts-embedded`.** It carries a package inside a package, so a
   byte prefix can cut the nested ZIP as well as the outer one — the case a deck of plain slides
   cannot produce. The corruption fixture is `raw-preserved`, whose slides hold the byte spans the
   raw sink slices, so a flipped byte lands in an offset the reader computed itself.
7. **The `.pptm` is invisible to the corpus sweep.** `every_deck_in_the_corpus_gets_through_the_package_layer`
   filters on the `.pptx` suffix, so the macro deck is read by its own test instead — which is what
   is wanted: the sweep asserts no deck degrades at the package layer, and this one warns by design.

## How it was verified

- `macro-enabled.pptm`: one slide with its two paragraphs, `Warning::MacrosIgnored { part:
  "ppt/vbaProject.bin" }` at `Severity::Info`, and a skeleton. The VBA part is an OLE2 header and
  filler — a fixture that could actually run something would be a hazard with no test value.
- Truncation: every 7th prefix of `charts-embedded.pptx`, 1 270 of them, none panicking.
- Corruption: every 11th byte of `raw-preserved.pptx` flipped, none panicking.
- Malformed XML in a valid ZIP, a ZIP with no presentation part, and 32 blocks of non-ZIP noise:
  all `Err`.
- `cargo test --workspace` (48 pptx tests, 13 robustness tests), `clippy --all-targets -D
  warnings`, `fmt --check`, `generate.py --check` and `opc_check.py` green; corpus is 93 files.

## Known gaps, written down rather than left implicit

- **No fuzzing, only the deterministic sweeps.** Prefixes and single-byte flips are what the R1
  spike measured `docx-rs` against; a structure-aware fuzzer would reach cases neither does.
- **A dangling *image* relationship inside a slide is not covered here.** The picture path warns
  through `AssetIssue`, and the increment that added it tested it, but the robustness suite does
  not synthesise one.
- **`docsai formats` still says `pptx: no`.** Reading a deck is not exposed through `read`
  dispatch yet, so a user cannot reach any of this from the CLI; that is 13-K onwards.

## Next

13-K: the `inspect` slide inventory — per slide, the layout used, the shape count, whether it has
notes and whether it has SmartArt or OLE, so an agent can decide where to edit without loading the
deck.
