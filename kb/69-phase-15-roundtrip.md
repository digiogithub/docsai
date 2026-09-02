---
tags:
    - phase-15
    - pptx
    - presentations
    - roundtrip
    - idempotence
    - fidelity
---
# 69 — Phase 15 H: `roundtrip` for decks, and what idempotence found

Increment **15-H** of [[61-phase-15-plan]], on top of [[68-phase-15-template]]. The increment that
does not add a feature so much as **ask a question**: `pptx → DocMark → pptx → DocMark`, is the
second pass the first? Over the whole corpus, for every deck, the answer is now yes — and it was
no when the question was first asked, for two separate reasons, both of them real defects in the
writer and neither of them visible to any test the phase had.

That is the whole shape of this entry. The code that extends `roundtrip` to decks is four lines;
the code that makes the answer *true* is the increment.

## What changed

- `crates/docsai-convert/src/pipeline.rs`: `Format::Pptx` joins the formats `roundtrip_file`
  round-trips (one match arm); `RoundtripOutcome::fidelity`; the per-category comparison and its
  warning; a deck's preserved package excluded from the assets dumped beside the output.
- `crates/docsai-convert/src/fidelity.rs` (**new module**): `CategoryFidelity`, `categories`,
  `compare`, `intact`, `changed`, `summary` — the metric of `docs/architecture.md` §4, which that
  document has described since Phase 0 and no code had ever computed.
- `crates/docsai-convert/src/inspect.rs`: `stats_from_text`, `stats_from_workbook` and
  `stats_from_presentation` are `pub(crate)`, so the metric counts with the tally that already
  exists rather than a second one that could drift from it.
- `crates/docsai-cli/src/main.rs`: `roundtrip` prints the categories, one line each, and `--json`
  carries them.
- `crates/docsai-office/src/pptx/write.rs`: `restore_external_attributes` and `central_directory`,
  called from `reemit` (defect 1); the shape-to-element map is `order::of_tree` and no longer
  `Shape::z_index` (defect 2).
- `crates/docsai-office/src/pptx/order.rs`: `of_tree` and `stub` — the reading-order policy applied
  to the package's own shape tree. `pptx/raw.rs`: `geometry_of` is `pub(super)`.

## The two defects the measurement found

### 1. The package came out with a permission bit it did not go in with

`zip`'s writer normalises every entry's mode (`FileOptions::normalize` ORs `S_IFREG` in), so a deck
written by Python's `zipfile` with mode `0o600` came back as `0o100600`: fifteen bytes different,
one per central-directory entry, in the **external file attributes** — a field no OPC consumer
reads. [[62-phase-15-package]] knew about it and said so in a comment; the corpus test was written
to compare member by member because of it.

Idempotence is what made it matter. A deck's DocMark states `skeleton:
assets/_skeleton/deck-<hash>.pptx`, and that hash is the content hash of the package. A permission
bit that moves **renames the preserved package**, so the second pass differed from the first for
all seventeen decks, on that one line — and a user converting a deck back and forth accumulates a
new `deck-<hash>.pptx` in `assets/_skeleton/` every time.

The fix restores the four bytes: after `reemit` has built the archive, the central directory of the
source is read and each entry's external attributes are written back over the ones `zip` chose. It
is a fixed-width overwrite at a known offset of a record whose position is found by walking the
directory from the end-of-central-directory record — never by scanning for the `PK\x01\x02`
signature, which a deflated part can contain. An archive this simple reader does not fully
understand (zip64, a truncated directory, a record that does not start where the last one ended) is
left exactly as the writer produced it: this is a cosmetic correction, and a wrong guess at an
offset would not be.

The corpus test of 15-A now asserts the whole archive rather than its members, which is a
**strengthening** of the guard, and the honest statement of what this writer does: what goes out is
the file that came in.

### 2. A deck whose reading order is not its z-order had its text permuted

`corpus/pptx/reading-order.pptx` came back with the text of every shape in a different shape. The
cause is a chain across three phases:

- The reader returns a slide's shapes in **reading order** — placeholders by type, then the rest by
  top-left — and keeps the `p:spTree` index in `Shape::z_index`, which is what makes the policy
  reversible ([[41-phase-13-reading-order]]).
- **DocMark-P does not write that index.** It writes the shapes in reading order and says nothing
  about z-order, so the parser numbers them by position (`deck_parser.rs`: «the source order of a
  deck that came from DocMark is the order it was read in»).
- **The writer indexed the shape tree with `z_index`.** For every deck whose two orders agree that
  is right by accident; for a deck where they disagree, every shape's text went into a different
  shape's box. Silently: no warning, no error, and a package that opens.

The fix is in the writer, and it is the inverse the reader always implied: the model's shapes are in
reading order, so the map from model shape to tree element is the **package's own reading order**,
computed from the tree by `order::of_tree`. `Shape::z_index` is no longer consulted when writing.

Why there and not in the serializer: adding a `z-index=` attribute to a slide shape would change
DocMark 1.2, which is normative and whose phase is closed (`AGENTS.md` §7 rule 2), and it would put
into the text a fact the *package already holds*. The whole phase's argument is that the package is
the truth and the IR is the delta over it; z-order is one more thing that never had to travel
through Markdown.

The risk of the fix is that `of_tree` and the reader could drift apart — two statements of one
policy. Two things hold them together: they are the same `key` function, in the same module, and
the corpus-wide byte-identity tests of 15-A and 15-B are a permutation detector. If `of_tree`
disagreed with the reader on any corpus deck, every text body of that deck would land in the wrong
element and the part would not come back byte for byte.

## What the categories are for a deck, and why those

The metric is per **category** because one number is a lie: a deck that kept every paragraph and
lost every picture is not «95 % faithful», it is a deck with no pictures. Each category is reported
with **both counts** — `pictures 0/1` is a sentence a reader can act on, `0 %` is not — and a
category neither side has is not reported at all, because a report full of `tables 0/0` hides the
line that matters.

| category | what it counts | why it is one |
| --- | --- | --- |
| `slides` | the deck's slides | the unit a person navigates and an agent addresses |
| `shapes` | every shape on every slide, a group's children included | the unit a person selects and moves; a group that came back empty has lost the slide even though the box is still there |
| `paragraphs` | the paragraphs inside the shapes' text bodies | what a reader actually reads, and the only category that measures the text |
| `pictures` | `ShapeKind::Picture` | a bitmap is re-injected from the asset store and has its own way of going missing (15-C) |
| `tables` | `ShapeKind::Table` | a table is spliced cell by cell into a preserved grid, which is a mechanism of its own (15-C, 15-G) |
| `notes` | slides carrying a notes page | a notes page is present or it is not, so it is counted per slide and not per paragraph; it also travels by a different door — the slide's own relationships (15-E) |
| `objects` | `ShapeKind::Chart` and `ShapeKind::Raw` | charts, SmartArt, OLE, media, custom geometry: what the IR holds only as a stub over preserved markup, and therefore exactly where a round trip is most likely to lose something without saying so (15-D) |

A text document is counted as paragraphs, headings, lists, tables, images and footnotes; a workbook
as sheets, cells and formulas. Those two were free — the counters already existed in `inspect` —
and having all three kinds go through one function is what keeps the CLI's output uniform.

## Non-obvious decisions

1. **The metric compares documents, not texts.** It is computed between the IR that was read and the
   IR that came back, never between the two DocMark strings. A category is a fact about the
   document; the text is one rendering of it, and comparing renderings is what `identical` already
   does. `corpus/docx/footnotes.docx` is the case that shows the two are not the same question: its
   second pass differs and its categories are all 100 % — the difference is not a lost node.
2. **The two assertions are both kept, and neither subsumes the other.** `identical` is the
   idempotence claim: a hand edit changes what it touches and nothing else. `intact` is the fidelity
   claim: everything that went in came back. A deck can fail the first and pass the second (the
   footnotes case above, on the docx side), and in principle the reverse.
3. **Over 100 % is reported as over 100 %.** `preserved()` does not clamp. A round trip that
   *invents* nodes has a defect of its own, and hiding it behind a ceiling is the same lie as hiding
   a loss.
4. **The warning names only the categories that moved.** Reciting the intact ones beside them would
   bury the loss in the report that exists to surface it — and a warning must never say something
   that is not true, which in this phase has bitten four times (`AGENTS.md` §7 rule 3).
5. **A deck's preserved package is not written to `assets/`.** `roundtrip -o out.pptx` dumps the
   asset store beside the output; for a deck that store holds the whole original package, which
   would land as `assets/img-<hash>.bin` — a second copy of the package, beside the package, under a
   name nothing refers to. It is skipped by id. Its media are still written, as a docx's are.
6. **`roundtrip` over the corpus writes nothing.** With no output path `roundtrip_file` writes
   neither the package nor the assets, so the corpus-wide test reads every deck in `corpus/pptx` and
   leaves the directory as it found it. This is worth stating because `default_assets_dir` puts
   assets beside the **source** when there is no output to put them beside.

## Known limits, written down rather than discovered later

- **The external-attributes restoration is a `.pptx` writer's, not the workspace's.** The docx,
  xlsx, odt and ods writers build their packages rather than copying one, so the question does not
  arise for them; if a preserved-package mechanism ever reaches another format, this correction does
  not come with it.
- **Only the external attributes are restored, not «version made by».** For the corpus, and for any
  package written by a Unix `zip`/`zipfile`, that is the whole difference. A package whose entries
  declare the MS-DOS host would still come back with `zip`'s own host byte, and would be equal part
  for part but not byte for byte. No such deck is in the corpus, and inventing one to test against
  is 15-I's kind of work, not this increment's.
- **`of_tree` recovers only the fields the ranking reads** — whether the element fills a placeholder
  slot, which one, and where it sits. That is by construction (they are the only inputs to `key`),
  but it means a future ranking that consults something else must extend both halves.
- **The metric counts nodes, not properties.** A slide that came back with every shape and none of
  its formatting scores 100 % on every category. Geometry compared «with the documented tolerance»
  is a separate acceptance criterion of this phase and is not this metric; what covers formatting
  today is the byte-identity of the written package and the DocMark identity of the second pass.
- **`--fidelity agent` is untouched.** The round trip runs at `full` in both directions, as it did
  before, so the inherited Phase 14 criterion — `agent` measured at 96–102 % of `full` instead of
  the ≤ 15 % analysis §6.5 asked for, encoded as an `#[ignore]`d test and left as a specification
  decision for the repository's owner ([[56-phase-14-convert]]) — is neither resolved nor depended
  on here.

## How it was verified

- **The idempotence test the increment owns**: `pptx_roundtrip_is_idempotent` in
  `crates/docsai-convert/tests/goldens.rs`, over the **whole** `corpus/pptx` — sixteen `.pptx` and
  the `.pptm`, seventeen decks — asserting both the per-category metric and `identical`. It names
  the deck that fails, which is how `reading-order.pptx` was diagnosed. Beside it,
  `the_deck_metric_counts_what_a_deck_is_made_of`.
- **The measured numbers, per deck**, from `docsai roundtrip <deck>` run on a copy of each corpus
  deck in a scratch directory. Every category of every deck is **100 %**, and every second pass
  equals its first:

  | deck | slides | shapes | paragraphs | pictures | tables | notes | objects | 2nd == 1st |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | `autofit-stale.pptx` | 1/1 | 2/2 | 9/9 | — | — | — | — | yes |
  | `basic-slides.pptx` | 2/2 | 4/4 | 5/5 | — | — | — | — | yes |
  | `bullets-levels.pptx` | 1/1 | 3/3 | 7/7 | — | — | — | — | yes |
  | `charts-embedded.pptx` | 1/1 | 2/2 | 1/1 | — | — | — | 1/1 | yes |
  | `forty-slides.pptx` | 40/40 | 80/80 | 160/160 | — | — | 40/40 | — | yes |
  | `images-anchored.pptx` | 1/1 | 2/2 | 1/1 | 1/1 | — | — | — | yes |
  | `macro-enabled.pptm` | 1/1 | 2/2 | 3/3 | — | — | — | — | yes |
  | `notes-crossed.pptx` | 2/2 | 4/4 | 6/6 | — | — | 2/2 | — | yes |
  | `notes-speaker.pptx` | 2/2 | 4/4 | 7/7 | — | — | 2/2 | — | yes |
  | `placeholders-cascade.pptx` | 1/1 | 2/2 | 3/3 | — | — | — | — | yes |
  | `placeholders-empty.pptx` | 1/1 | 3/3 | 5/5 | — | — | — | — | yes |
  | `raw-preserved.pptx` | 1/1 | 3/3 | 1/1 | — | — | — | 2/2 | yes |
  | `reading-order.pptx` | 1/1 | 5/5 | 5/5 | — | — | — | — | yes |
  | `shapes-geometry.pptx` | 1/1 | 4/4 | 2/2 | — | — | — | 1/1 | yes |
  | `slide-order.pptx` | 3/3 | 6/6 | 6/6 | — | — | — | — | yes |
  | `smartart-fallback.pptx` | 1/1 | 2/2 | 1/1 | — | — | — | 1/1 | yes |
  | `tables-simple.pptx` | 1/1 | 2/2 | 1/1 | — | 1/1 | — | — | yes |

  A dash is a category the deck does not have, and is not printed. Before the two fixes, every deck
  in this table failed `identical` on the `skeleton:` line, and `reading-order.pptx` failed it on
  four lines of text as well.
- **The package, per deck**: each corpus deck round-tripped with `-o`, then `corpus/opc_check.py`
  over the output (**17 × `ok`**: every part typed, every `r:id` resolving, no dangling
  relationship) and `cmp` against the input (**17 × byte-identical**). The `assets/` directory
  beside the output holds only the media — one `.png` for `images-anchored`, nothing for the decks
  with no picture — and no copy of the package.
- **Three new tests in `pptx::write` and `pptx::order`** (168 in `docsai-office`, up from 166):
  `a_deck_renumbered_by_the_docmark_parser_still_lands_in_the_right_shapes` (defect 2, reproduced by
  renumbering the model exactly as the DocMark parser does),
  `a_shape_tree_is_ordered_the_way_its_shapes_are` (`of_tree` over hand-written XML), and
  `a_written_deck_is_the_package_that_was_read` strengthened from member-for-member to byte-for-byte
  (defect 1).
- **Two new tests in `pipeline`** (55 in the crate lib, up from 53):
  `a_deck_roundtrip_writes_the_package_and_not_a_copy_of_it` and
  `a_deck_roundtrip_writes_back_the_bytes_it_read`; and three in `fidelity`, including
  `an_empty_category_is_not_reported` and `a_lost_slide_is_named_with_its_two_counts`.
- `cargo test --workspace --no-fail-fast`: **666 passing, 1 ignored**, the only failure the
  pre-existing `docx_roundtrip_is_idempotent` on `corpus/docx/footnotes.docx` (from `88ff8e11`,
  untouched by this increment — and its categories all read 100 %, which is a fact about that bug
  worth having). `clippy --workspace --all-targets -- -D warnings` clean, `fmt --all` applied,
  `python3 corpus/generate.py --check` clean at 129 files, and `git status` shows no corpus file
  touched.
- The **LibreOffice render was not run**: the installation on this machine is broken
  (`soffice.bin: error while loading shared libraries: libreglo.so`), as it was for 15-D..15-G. It
  matters less here than in [[68-phase-15-template]] — every package this increment produced is the
  package that was read, byte for byte, so a renderer that opened the corpus opens these — but it is
  still true that **no application has opened them**. Risk **P2** is 15-K's checklist.

## Next

15-I, done in [[70-phase-15-xsd]]: CI gate 1 — the parts the writer produces validated against the
ECMA-376 XSDs, vendored in `schemas/` and run by `xmllint`, the gate naming the part, the line and
the element when a package is not schema-valid. It found two corpus fixtures that were not
schema-valid, in decks this increment had just declared 100 % faithful.
