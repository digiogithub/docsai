# 45 — Phase 13 K: the `inspect` slide inventory, and Phase 13 closed

Increment **13-K** of [[34-phase-13-plan]], the last one, on top of [[44-phase-13-pptm-robustness]].
Every increment before this built the reader; this one gives a deck somewhere to go — and closes
the phase's acceptance criteria.

## What changed

- `docsai-office`: `Format::Pptx` joins `READABLE` and the `read` dispatch. An
  `mc:AlternateContent` stub is now classified by what it wraps (`wrapped_kind`), so SmartArt and
  OLE are named instead of always «other».
- `docsai-convert`: `SUPPORT` says `pptx: read yes, write no`. `InspectReport` gains
  `slides: Option<Vec<SlideSummary>>`, built by `summarize_slide` over a single `ShapeTally` walk.
  The preserved package is filtered out of `media`. Converting a deck to DocMark is refused with
  `ConvertError::Unsupported`.
- `docsai-cli`: `inspect` prints a slide line per slide and `slides=` in the stats line.
- `corpus/generate.py`: layouts carry a `p:cSld@name`; new `forty-slides.pptx`.
- `crates/docsai-convert/tests/pptx_goldens.rs`: IR goldens, determinism, and the 40-slide budget.

## Non-obvious decisions

1. **Readable is not convertible, and the support matrix already had the bit for it.** A deck
   reaches the IR and `inspect` reports it, but `docsai convert deck.pptx` still fails with
   `unsupported conversion: pptx -> docmark`. The DocMark serializer *would* have produced
   something: an empty body plus `Warning::UnsupportedElement`. A caller redirecting that to a file
   gets a document that lost every slide and looks like a success, and warnings on stderr do not
   survive a shell pipeline. The guard lives in `write_document`, so every entry point —
   `convert_file`, `convert_bytes`, the MCP tool — inherits it.
2. **The layout is reported by `p:cSld@name`, not by the part name.** `ppt/slideLayouts/slideLayout1.xml`
   answers a question nobody asked; "Titulo y objetos" is what the deck's author sees. The name is
   resolved through `LayoutCatalog`, which the reader had already populated, and falls back to the
   part stem — the reader's own rule — when the layout does not name itself.
3. **The corpus layout gained a name, which rewrote every deck.** Before this, no fixture had a
   `p:cSld@name` anywhere, so `layout-name` would have been pinned to a fallback and the field
   would have proved nothing. Real layouts always carry one. The cost was regenerating every deck
   and updating one assertion in `pptx/mod.rs`; the fallback still has direct coverage in
   `part_stems_name_the_parts_that_do_not_name_themselves`.
4. **Naming an `mc:AlternateContent` stub is not reading it.** 13-I preserved the wrapper whole and
   classified it `Other`, on the argument that choosing between `mc:Choice` and `mc:Fallback` is
   the consumer's decision. That argument is about *reading a branch*, and it still holds — neither
   branch is read, the pair is still preserved byte for byte. But a stub that says «other» about
   SmartArt leaves an agent blind to the one object on the slide it must not hand-edit, which is
   the whole reason the stub exists. `wrapped_kind` walks the subtree for a `graphicData@uri` (or a
   `p:oleObj`) and maps it through the same `held_kind` the `p:graphicFrame` path uses.
5. **The shape count includes group children.** An agent asking "how much is on this slide" wants
   the total, not the number of top-level `p:spTree` entries — a deck where everything is in one
   group would otherwise report `shapes: 1`. The trade-off is that the number is not an index into
   `Slide::shapes`; the doc comment says so.
6. **The preserved package is not media.** The skeleton is an asset like any other, so `inspect`
   was reporting a deck with no pictures as having one 7 KB `application/octet-stream` image. It is
   filtered by id. A lie by arithmetic is still a lie.
7. **The phase's goldens are `inspect --json`, one file per deck.** [[34-phase-13-plan]] had
   already settled this: DocMark goldens need DocMark-P, which is Phase 14. The report is the IR
   seen from outside — slides, layouts, counts, notes, media, and the read-time warnings — and it
   is stable enough to diff: `build_report(None, …)` drops the path, and asset ids are content
   hashes. Same `DOCSAI_UPDATE_GOLDENS=1` ritual as the DocMark goldens.
8. **The 40-slide deck is synthetic and the test says so.** The acceptance criterion says «a *real*
   40-slide deck»; no real deck can live in the repository. `forty-slides.pptx` has the right shape
   (forty slides, forty notes parts, one cascade) and not the weight — no images, no embedded
   objects. The budget of one second is deliberately generous, because what the test is really
   watching for is a reader that went quadratic in the slide count, which would miss by orders of
   magnitude, not by a margin.

## How it was verified

- `inspect` over the corpus: `smartart-fallback` reports `has-smart-art`, `notes-speaker` and
  `notes-crossed` report `has-notes` on both slides, `charts-embedded` one chart, `tables-simple`
  one table, `images-anchored` one picture and exactly one media asset.
- `has-ole` is proved over the IR, not over a package (see the gap below).
- `a_deck_reads_but_does_not_convert_yet`: the typed refusal, and no half-written output file.
- IR goldens for all seventeen decks, plus `reading_a_deck_is_deterministic` (two reads, same JSON).
- `a_forty_slide_deck_reads_well_inside_a_second`: 40 slides, well under the budget.
- `cargo test --workspace` (33 suites), `clippy --all-targets -D warnings`, `fmt --check`,
  `generate.py --check` and `opc_check.py` green; corpus is 94 generated files plus 17 goldens.

## Known gaps, written down rather than left implicit

- **No corpus deck embeds an OLE object.** `has-ole` is tested over a synthetic `Presentation`, so
  the flag's plumbing is proved but the reader's classification of a real `p:oleObj` is not. The
  fixture is the first thing to add when a deck needs one.
- **Slide ids are absent from the goldens.** The reader does not assign `NodeId`s yet, so
  `SlideSummary::id` is always `None` in practice. It is in the shape for when Phase 14 assigns
  them.
- **The inventory does not say what a slide costs.** Token cost per slide is what an agent would
  really plan against, and it is measured over DocMark — Phase 14, with `outline`/`tokens`.

## Next

**Phase 13 is closed.** Phase 14 is the DocMark-P profile: the serializer, the goldens over it,
`serialize(parse(md)) == md`, and the deferred `--fidelity agent` token-budget criterion that
could not be measured without it.
