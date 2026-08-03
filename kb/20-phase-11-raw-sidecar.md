# 20 — Phase 11 increment A: raw-blocks move to a sidecar

Increment 11-A of [[19-phase-11-plan]]. The bytes nobody can edit stop being part of what an
agent reads.

## What changed

- **`RawPolicy { Inline, Sidecar }`** in `docsai-docmark`, `Options.raw` and
  `ConvertOptions.raw`, **`sidecar` by default**; `docsai convert --raw inline|sidecar`.
- **The stub**: `::: {#raw-0001 .raw format=ooxml part="word/document.xml"
  src="assets/_raw/raw-0001.xml"}` with an empty body. `--raw inline` still writes the fenced
  payload, unchanged from 1.0.
- **`docsai_docmark::raw::raw_sidecars(doc, options)`**: the files a serialisation refers to,
  as a pure function of the document. The pipeline (and the goldens updater) writes them.
- **Parser**: `src=` is read relative to the base directory. A missing file is
  `ParseError::Io` naming the path; a `src=` with no base directory is a typed error too.
- **Corpus**: `docx/fields-raw.docx` now carries block-level OMML maths and therefore a real
  raw-block, with its sidecar committed at `corpus/docx/assets/_raw/raw-0001.xml`.
- Spec §7 rewritten with the sidecar form; §11.1 moves the sidecar from "not yet implemented"
  to implemented.

## Non-obvious decisions

1. **The sidecar list is a function of the document, not an output of the writer.** The writer
   only has `&dyn AssetStore` and cannot store anything, and threading a payload collector
   through it would have made `serialize` a function you can call wrongly (payloads silently
   dropped). Instead the name is computed in one place, `raw::sidecar_path`, used both by the
   writer that emits `src=` and by the walk that produces the bytes: the two cannot drift.
2. **A missing sidecar is an error, not a warning.** A raw-block exists precisely to hold what
   no other construct can hold. A tolerant parser here would be a way to lose exactly the data
   the format promised to keep.
3. **In-memory paths stay `Inline`**: the round-trip check, `convert_to_markdown` with inline
   assets, and the DocMark view returned after writing a binary target. There is no directory
   for a sidecar to live in, so a `src=` would point at nothing.
4. **The id is sanitised into a file name.** A raw id comes from the source package, and a
   package is not a trustworthy source of file names (`../../etc/passwd` becomes
   `etc-passwd.xml`).
5. **A corpus fixture, because the path was untested.** No corpus document produced a single
   raw-block: `w:sdt` is flattened on purpose, and everything else was modelled. OMML maths is
   both realistic and genuinely unmodelled, so `fields-raw.docx` — the fixture already named
   for this — now earns its name. Same lesson as the outline budget in [[17-phase-10-outline]]:
   a criterion measured on a document that lacks the trait is vacuous.

## How it was verified

- `cargo test --workspace` (26 suites), clippy `-D warnings`, `cargo fmt --all -- --check`,
  `python3 corpus/generate.py --check`.
- `crates/docsai-docmark/tests/raw_sidecar.rs`: the stub names its sidecar and drops the
  payload, the round trip returns the same blocks and re-serialises byte for byte, a missing
  sidecar names the file it could not read, a stub with no base directory says so, `inline`
  stays self-contained, and the sidecar form is smaller.
- End to end through the CLI: `fields-raw.docx` → DocMark + `assets/_raw/raw-0001.xml` →
  `.docx` again, with `oMathPara` present in the rebuilt `word/document.xml`.
- Corpus token budget regenerated: 24 851 → **24 883** at `full` (+0.1 %), the cost of the new
  fixture's stub. Inline it would have cost more; the sidecar is why it did not.

## Next

11-B: `--fidelity agent`, which needs this — an agent level that still had raw payloads in the
body would not be an agent level.
