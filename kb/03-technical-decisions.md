# 03 — Technical decisions

The decisions that do not follow from the plan alone and that a future collaborator could undo
unintentionally. Each one carries its rationale and its cost.

## 1. Custom docx parser, without `docx-rs`

**Decision**: `.docx` reading is done with our own `zip` + `quick-xml`.

**Rationale** (spike R1, measured on the corpus, not assumed):

- `docx-rs` handles styles, numbering, and headers/footers well. But it loses **almost the entire
  image model**: text wrapping and side, `behindDoc`, crop, flips, alt text, title,
  object name, hyperlink on the image, and linked images. Rotation exists as a field but
  returns `0` for `rot="2700000"`, and the type is `u16`, which cannot represent sixty-thousandths
  of a degree or negative values.
- It drops `w:footnoteReference` and the `w:instr` of `w:fldSimple`.
- VML (`w:pict`) collapses to a generic node with no `r:id`, no style, and no `alt`.
- It does not preserve unknown elements, which is exactly what the raw-block needs.
- **204 panics out of 903 corrupt inputs** (23 %), against an acceptance criterion that requires
  always `Err`.

Images are a first-class requirement of the project and half of the Phase 1 criteria. What
remained delegable to `docx-rs` was the mechanical part; a custom complement would have been the
bulk of `document.xml` anyway, with two XML parsers in the binary and two trees of the same
document to reconcile.

**Accepted cost**: more custom surface to maintain. Mitigated by the corpus, the goldens, and
generic capture of unknown elements, which makes what is missing *visible* rather than silent.

**Detail**: `docx-rs` remains a candidate for the Phase 2 **writer**. That is an independent and
still open decision: writing is much simpler than reading, because we control the output XML.

## 2. XML tree with byte spans

**Decision**: `quick-xml` feeds an in-memory tree where **each node remembers the byte range it
occupies in the source**.

**Rationale**: a raw-block must preserve the original bytes. Re-serializing the subtree would
yield equivalent but not identical XML, and Phase 2 re-injection would no longer be exact. With
the range it is enough to slice the source string. Bonus: a tree is far more reviewable than a
state machine over events, and OOXML parts fit comfortably in memory.

**Cost**: memory proportional to the size of the XML part. Acceptable for documents; it will need
review in Phase 3 for sheets with 100k cells, where `calamine` already streams per sheet.

## 3. Match by local name, not by namespace

**Decision**: elements are looked up by their local name (`p`, not `w:p`).

**Rationale**: OOXML prefixes are conventional but not mandatory, and all reader navigation is
contextual (children of a known parent), so there is no ambiguity. A document that renames its
prefixes is read the same way.

**Exception**: attributes with different meaning depending on the namespace — `r:embed` and
`r:link` next to an `a:blip` — are looked up by **qualified** name with `attr_qualified`.

## 4. Length formatting: exactness before readability

**Decision**: when writing a length, choose the **first unit that represents it exactly**, in the
order `px` (96 dpi) → `cm` → `pt` → `emu`.

**Rationale**: a Word margin of 1417 twips is not a round number of centimeters. Writing it as
`2.499cm` shifts it by 155 EMU on every round-trip, and those errors accumulate. Now it comes out
as `70.85pt`, which is exact. `emu` is the final escape hatch and always exists.

**Cost**: some lengths read worse (`85.05pt` instead of “about 3 cm”). That is the price so that
the Phase 2 round-trip can aim for 95 % fidelity.

## 5. Externally tagged `Document` in serde

**Decision**: `#[serde(rename_all = "kebab-case")]` without `tag`, unlike `Block` and
`Inline`, which are adjacently tagged.

**Rationale**: an *internally* tagged enum makes serde store the content in an intermediate
`Content` buffer, and that buffer **rewrites every map key as a string**. That breaks maps with
integer keys in `Workbook` (`cols`, `rows`). It is a failure that only appears on deserialize and
with an unclear message (`invalid type: string "0", expected u32`).

**Related**: `CellRef` implements `Serialize`/`Deserialize` by hand as its A1 reference
(`"B2"`), so it can be a map key and also keep `inspect --json` readable.

## 6. `serde_json` with the `float_roundtrip` feature

**Decision**: enabled in the workspace.

**Rationale**: without it, `serde_json`'s fast floating-point parser can shift a value by one ULP.
On spreadsheet cells that is **silently corrupting user data**. It was detected by comparing IR
before and after a JSON round-trip in the proptest test.

## 7. Non-cryptographic hash for assets

**Decision**: 64-bit FNV-1a, mixing in the length, rendered as 16 hex digits.

**Rationale**: the id only needs to be stable and practically collision-free for the media of one
document. Avoids pulling `sha2` into `docsai-model`, which is the crate that must stay light. The
`AssetStore` trait lets the conversion layer replace the hash without touching the model.

**When to revisit**: if someday the asset name is used as a trust boundary. Today it is not.

## 8. Custom image sniffing, without the `image` crate

**Decision**: read the header of PNG, JPEG, GIF, BMP, TIFF, WebP, EMF, and WMF by hand.

**Rationale**: docsai **never re-encodes** a bitmap; it only needs to name it (extension and
content-type) and measure it (native dimensions). That is a few dozen lines against a considerable
dependency tree in the final binary.

**Security detail**: the extension comes from the **content**, never from the name the document
carries.

## 9. Handwritten YAML front matter

**Decision**: no YAML library.

**Rationale**: the spec requires byte-for-byte determinism, the schema is small and known, and
`serde_yaml` has no active maintenance. Writing it by hand gives full control over key order and
quoting.

**Cost**: the emitter must be maintained by hand. When Phase 2 writes the parser, both sides will
have to move together; the idempotence test will make it obvious if they drift.

## 10. Golden files as text files, not `insta` snapshots

**Decision**: `corpus/docx/<name>.expected.dmk.md` next to the document.

**Rationale**: that is what `AGENTS.md` §6 prescribes, and a golden that is readable DocMark is
reviewed in the diff like any other text. An `insta` `.snap` adds an intermediate format between
the reviewer and what the program produces.

Updating is deliberate: `DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens`,
and **the diff is reviewed**.

## 11. Generated corpus, not hand-drawn

**Decision**: `corpus/generate.py` produces the 20 files; CI checks that the tree is up to date.

**Rationale**: each document's XML lives in the generator, where it is reviewed; a `.docx` made
with Word is opaque in review. Packages come out with fixed timestamps and member order, so
regenerating yields byte-identical files and the repository does not accumulate binary noise.
Media are synthesized in pure Python so it works the same on all three CI platforms.

**Cost**: these are *minimal* documents, not real Word documents. Anonymized real ones remain
pending (Phase 1, task 10). The decision paid for itself immediately: the spike uncovered that the
generator placed `w:drawing` outside a `w:r`, which is invalid OOXML, before that contaminated any
golden.

## 12. `w:sdt` is flattened instead of kept opaque

**Decision**: a content control emits its child blocks and a `Degraded` warning, instead of a
raw-block with everything inside.

**Rationale**: putting the whole thing in a raw-block would hide perfectly readable text behind an
opaque block, and in `plain` mode it would disappear entirely. The text is what the user wants to
see.

**Cost**: control properties are lost (`w:alias`, data binding). It is reported, not silent, and
Phase 2 will have to decide whether it needs them.

## 13. The warning is part of the output, not decoration

Every degradation emits a **typed** `Warning` with severity. The CLI counts them on stderr,
details them with `--verbose`, and serializes them with `--json`; exit code 1 marks a conversion
that lost something. `--strict` raises the bar so minor ones also count.

It is rule 3 of `AGENTS.md` made code: **nothing is degraded silently**.

## 14. Security limits from the start

Although hardening is Phase 8, three things were too cheap to postpone:

- **Decompression cap**: 512 MB per package, 128 MB per part.
- **XML depth limit**: 256 levels.
- **Member name sanitization**: a `word/media/../../evil.png` never becomes a part or an asset.
  There is a dedicated test.
