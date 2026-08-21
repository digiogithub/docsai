---
tags:
    - phase-15
    - pptx
    - writer
    - skeleton
    - zip
    - opc
---
# 62 — Phase 15-A: the package comes back out

First increment of [[61-phase-15-plan]]. The writer side of [[42-phase-13-skeleton]]: a deck read
from a `.pptx` goes back out as the package it came in as, entry by entry.

## What was built

`crates/docsai-office/src/pptx/write.rs`, reached through `docsai_office::write_pptx` and
`docsai_office::write(Format::Pptx, …)`:

- The preserved skeleton is fetched from the `AssetStore` by the id in `Presentation::skeleton`.
- The archive is re-emitted with `zip::ZipWriter::raw_copy_file` over `by_index_raw`: **member
  order, compression method, CRC, timestamp and the stored (still compressed) bytes** all come
  from the source entry. Nothing is inflated on the way through, so nothing can come back
  re-deflated by a different encoder into different bytes.
- No slide is regenerated yet. Writing emits `Warning::Degraded` naming Phase 15-B, because a
  deck whose model was edited comes out unchanged and the caller has to be able to learn that from
  the report rather than from the file (`AGENTS.md` §7 rule 3).
- A deck with no skeleton is `WriteError::Invalid` naming 15-G; a `Document` that is not a
  presentation is refused by shape. Neither writes a byte.

## Why not `Package::write_to`

`crate::package::Package` is a `BTreeMap<String, Vec<u8>>`. A package rebuilt through it comes out
**alphabetised, recompressed and re-timestamped** even when every part is byte-identical — three
differences no diff of the *parts* would show, in the one file format whose failure mode is an
application deciding the container looks wrong. Spike P3 (`docs/spikes/P3-preserved-skeleton.md`)
is the measurement behind that sentence, and this is the increment where it becomes code.

## The one field that does move

The **external file attributes** of each central-directory entry. `zip` derives them from the
source entry's Unix mode but ORs `S_IFREG` in (`ZipFile::options`), so a corpus deck written by
Python's `zipfile` with mode `0o600` returns as `0o100600`. There is no public API in `zip` 2.4
that takes both a raw copy and an options override, and the field is a permission bit no OPC
consumer reads. Consequence for the tests: equality is asserted **member by member** — name,
compression method, CRC, size, and the decompressed bytes — plus total archive length, never over
the whole file. Worth knowing before someone spends an afternoon chasing a byte diff at
offset 41 of a central header.

## Tests

Five, in the module (`cargo test -p docsai-office pptx::write`):

- `a_written_deck_is_the_package_that_was_read` — the whole corpus (≥ 17 decks), member table and
  archive length.
- `every_part_survives_byte_for_byte` — decompressed parts of `charts-embedded`,
  `smartart-fallback` and `macro-enabled.pptm`: the chart's embedded workbook, the diagram's four
  `dgm:` parts and the macro payload, all identical.
- `the_writer_admits_the_slides_are_not_regenerated` — the warning exists, is `Minor`, and names
  15-B.
- `a_deck_without_a_skeleton_is_refused_by_name`, `a_text_document_is_not_written_as_a_deck` —
  typed refusals that write nothing.

## What stayed closed on purpose

`SUPPORT` still answers `pptx: write no`, and `docsai convert … -o deck.pptx` is still refused. A
conversion that produced a valid package while silently dropping every edit in the document would
be worse than the refusal it replaced; the pipeline opens in **15-B**, when the slides come from
the IR.

Done, in [[61-phase-15-plan]] · next: 15-B, the slide part regenerated.
