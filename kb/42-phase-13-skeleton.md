# 42 — Phase 13 H: the preserved skeleton

Increment **13-H** of [[34-phase-13-plan]], on top of [[41-phase-13-reading-order]]. The increment
that makes [[33-phase-12-spike-p3]]'s decision real in the reader: **preserve everything, rebuild
as the exception**.

## What changed

- `crates/docsai-office/src/pptx/skeleton.rs` (new): stores the original package and builds the
  `SkeletonRef`.
- `pptx/mod.rs`: `read` reads the file's bytes once and hands them to `read_package`, which fills
  `Presentation::skeleton` and tracks the parts the IR holds.
- `pptx/notes.rs`: `notes::part`, the one place that names a slide's notes part, now used by both
  the notes reader and the skeleton.

No new fixture: every deck in the corpus is a package, so every one of them is a test of this.

## Non-obvious decisions

1. **The skeleton is the whole file, not the non-slide parts.** The plan's wording is «non-slide
   parts stored opaquely»; the model's `SkeletonRef` (written in 13-A) says «the whole original
   package», and P3's rule is stronger than both. Storing a selection would mean reassembling a
   package at write time, and a reassembled ZIP has already lost the member order and the
   per-entry compression the producer chose — the two things P3 verified were preserved across 48
   packages. `rebuilt_parts` carries the selection instead, as an exception list.
2. **`rebuilt_parts` is what the reader actually read, not what the content types call a slide.**
   A deck can hold a slide part that no `p:sldIdLst` entry points at, or one whose relationship
   was unresolvable; the IR does not hold those, so a writer that regenerated them would be
   inventing content. The list is built inside the slide loop, from the parts that produced a
   `Slide`.
3. **A notes part is listed only when its notes were read.** `notes::read` refuses an external or
   mis-typed notes part; the IR then holds nothing of it and the writer must copy it. Both the
   reader and this list go through `notes::part`, so they cannot drift into disagreeing about
   which part is the notes part.
4. **`read` reads the bytes before opening the package.** The file is two things at once — the
   package to decompress and the skeleton to keep — and asking a `Read + Seek` for it twice would
   read or decompress the deck twice.
5. **That move needed a size cap.** `Package::open` used to be the first thing to see the stream,
   and it caps parts and total expansion. Reading ahead of it puts the first check on the new
   line, so `MAX_PACKAGE_BYTES` (512 MiB, the same number `package.rs` caps expansion at) refuses
   a huge file before it is in memory. Without it a 5 GiB file that is not even a ZIP would be
   pulled in whole before anything rejected it.
6. **`read_package` takes `original: Option<&[u8]>`.** A package assembled in memory (the tests,
   and any future caller that never had a file) reads into a deck with **no** skeleton rather than
   one reconstructed from parts that have already lost their ZIP order. Absence stated, not faked.
7. **A store failure warns.** `Warning::Degraded` naming what the loss means — theme, masters and
   every unmodelled part would have to be regenerated from the IR — because a deck silently
   without a skeleton is the exact failure P3 measured (`AGENTS.md` §7 rule 3).

## How it was verified

- `charts-embedded.pptx`: the stored bytes equal the file on disk byte for byte, and re-opening
  them yields a package that still has the embedded `.xlsx` and `ppt/theme/theme1.xml` — the parts
  P3's naive control lost twelve chart values and five diagram parts to.
- `notes-speaker.pptx`: `rebuilt_parts` is exactly the two slides and the two notes pages; no
  theme, master, layout or `docProps`. `basic-slides.pptx` lists two slides and no notes part.
- Reading the same deck twice into one store leaves one asset and the same `SkeletonRef`: the
  content hash deduplicates the skeleton like any other asset.
- `cargo test --workspace` (42 pptx tests), `clippy --all-targets -- -D warnings`, `fmt --check`,
  `corpus/generate.py --check` green.

## Known gaps, written down rather than left implicit

- **«Streamed, not held» is not achieved.** The plan asks for it and `AssetStore::put` takes
  `&[u8]`, so the compressed package is in memory at once, on top of the parts `Package::open`
  already holds decompressed. Peak grows by roughly the file's own size; P3's measured 2.7× is
  against the deck size and stays the same order of magnitude. Real streaming needs a store API
  that takes a `Read`, which is a `docsai-model` change and belongs to whoever needs it.
- **The store names the skeleton like an image**: `assets::describe` produces
  `img-<hash8>.bin` with `application/octet-stream`, because `sniff` is image-oriented and the
  prefix is hard-coded. Harmless while `read_pptx` is out of the `read` dispatch and the tests use
  `MemoryAssetStore`, but a `DirAssetStore` would write the whole deck into the media directory
  under an image-looking name. Phase 14 wires pptx into `convert`; it is the increment that has to
  decide whether the skeleton belongs in the same store at all.
- Nothing yet *reads* the skeleton back. It is written, hashed and referenced; the writer that
  re-injects slides into it is Phase 15.

## Next

13-I: nothing disappears — SmartArt, `p:timing`, `p:transition`, OLE, `custGeom`, connectors and
groups become a visible stub plus a raw block in the Phase 11 sidecar plus a typed warning, and
`Warning::AutofitStale` fires for a dropped `a:normAutofit@fontScale`.
