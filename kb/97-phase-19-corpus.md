---
tags:
    - phase-19
    - corpus
    - legacy
    - ppt
    - security
---
# 97 — Phase 19-A: the legacy `.ppt` corpus, including what is meant to fail

First increment of [[95-phase-19-plan]], plan task 4. It produces `corpus/ppt/` and the machinery
that checks it. **No reader of any kind** — the LibreOffice route is 19-B, the `office_oxide`
evaluation 19-C, the native degraded route 19-D, the refusal path 19-E.

## What changed

- `corpus/generate.py` — a new *Legacy `.ppt` fixtures* section: two recorded decks, two derived
  refusal fixtures, a `.pptx` source deck, and enough of [MS-CFB] to move a boundary inside an
  existing compound file, strip an optional property out of it and compact what that frees.
  `build_pptx` gained one additive keyword, `subdir`, so a package can land outside `corpus/pptx`.
  The five generators are in `GENERATORS`.
- `corpus/generate.py:digest_tree` — **a defect fixed**: it walked corpus directories with
  `iterdir()`, so any *subdirectory* was invisible to `--check` and a fixture inside one would have
  been written by the generator and then checked by nothing. It walks with `rglob("*")` now, keyed
  by the path relative to `corpus/`. It finds one more file — `docx/assets/_raw/raw-0001.xml`, the
  hand-maintained raw-block sidecar, which the generator does not write, so that key is inert
  rather than a new check. Nothing else moved: for a flat directory `rglob` yields exactly what
  `iterdir` did, under exactly the same key.
- `corpus/ppt/` — `basic-slides.ppt`, `mixed-media.ppt`, `mixed-media.source.pptx`,
  `truncated.ppt`, `encrypted.ppt`.
- `corpus/README.md` — the section that says how each fixture came to exist, what `--check` does
  with it, what it costs, and what can and cannot gate a `.ppt`.

## The problem, and the answer given to it

Every other fixture in this repository is a ZIP whose bytes `generate.py` writes itself, which is
what lets `--check` assert the checked-in tree is what the script produces. A `.ppt` is an OLE2
compound file and nothing in this tree writes one. The three options were all imperfect:

1. **Drive LibreOffice at generation time.** Rejected: it makes LibreOffice a build-time dependency
   of the corpus and of CI, and stakes `--check` on another program's determinism forever.
2. **Hand-assemble the OLE2 container with `cfb`.** Rejected for the *ordinary* decks: deterministic
   by construction, but the fixture would then be one this project wrote, and 19-B and 19-D exist to
   read files this project did **not** write. A reader proved against its own generator proves
   little. (It is the right answer for a different phase's fuzzing corpus, and Phase 20 may want it.)
3. **Record the bytes with a hash and verify rather than regenerate.** Rejected as stated, because
   it breaks the corpus's own rule.

What was taken is a fourth shape, and it is the one the legacy `.doc` fixtures already use: **record
the bytes as a source constant.** LibreOffice produced them once; they live in `generate.py`
zlib-compressed and base64-encoded; `--check` **regenerates** them by decompressing a constant, which
is deterministic on every Python this project supports. So the digest is fixed by a reviewed source
file, and **LibreOffice is a dependency of refreshing the constant — a deliberate act with a written
recipe — not of running the generator or any gate.** That is the recording, and it is stated in
`corpus/README.md` rather than hidden.

The two fixtures that exist to be **refused** are not recorded at all: they are **derived** from the
first deck by [MS-CFB] surgery this repository owns, so every byte in which they differ from their
base is computed here.

**None of this is a `.ppt` writer.** The code writes fixtures, lives in the corpus generator, and
reaches no crate and no shipped binary. Phase 19 is read-only and stays read-only.

### Determinism, measured rather than assumed

The recording is only honest if LibreOffice's output is stable. It was checked before it was relied
on. LibreOffice **24.2.7.2 420(Build:2)**, `--convert-to ppt` on `corpus/pptx/basic-slides.pptx`:

| Run | Result |
|---|---|
| Twice in a row, same profile | byte-identical |
| Again 95 seconds later | byte-identical |
| Again on a freshly created user profile | byte-identical |

No timestamp, no UUID. That is a statement about this version on this machine and nothing more; a
refresh under another LibreOffice yields a different constant, which is a reviewed diff in a source
file rather than a digest that varies by machine — which is exactly what generating at `--check`
time would have produced.

### What it costs

Both costs are real and neither is hidden:

- The recorded bytes are **not reviewable in a diff**. Written-down provenance is the compensation
  and it is weaker than the XML of a generated `.pptx`.
- Nothing else. The size cost was real and has been paid off — see the next section.

## The thumbnail, and why the fixtures are 23 KB rather than 608 KB

As first recorded, each deck was ~600 KB of which about **95 % was a preview bitmap**: LibreOffice
embeds a `PIDSI_THUMBNAIL` — property 17, `VT_CF`, 588 344 bytes — in `\5SummaryInformation`. That
would have been ~2.3 MB of permanent weight in every clone, for bytes **no increment of this phase
reads**: 19-B hands the file to LibreOffice, 19-D walks `TextCharsAtom`/`TextBytesAtom` and the
`Pictures` stream, and neither goes near the summary stream.

LibreOffice cannot be talked out of writing it. `Common/Save/Document/GenerateThumbnail` set to
`false` — both in a hand-written profile and fused into one LibreOffice had already created —
changed nothing: that setting governs the *ODF package* thumbnail, not the OLE2 property. So it is
removed by surgery, in `_ppt_deck()`, in three steps:

1. `_property_set_without_thumbnail` rebuilds the property set without property 17 — **rebuilt, not
   blanked**: the property count and every offset are restated, so what comes out is well-formed at
   its new size (588 628 → **276 bytes**). The property is optional in [MS-OSHARED] 2.3.3.2.1.
2. `_cfb_shrink_stream` writes it back, freeing and zeroing the 1 150 sectors it no longer needs.
   It pads the stream to the **mini stream cutoff**, and that is not fussiness: a 276-byte stream
   left in the FAT would, per [MS-CFB], have to live in the mini stream instead, and a reader would
   take its start sector as a *mini*-sector index and hand back the wrong bytes.
3. `_cfb_compact` renumbers the sectors still in use into a dense run at the front and truncates.
   Freeing alone would not have shrunk the file at all — the directory of these files lives in the
   **last two sectors**, so the freed 1 150 would have stayed as a hole in the middle. The
   compaction is a *permutation of an existing container*, not a new one: no allocation policy is
   invented, the FAT keeps the number of sectors it already had, and every stream's bytes are the
   bytes they were.

**608 KB → 23 KB, and `corpus/ppt/` in total is 116 KB** (was 2.4 MB).

What did **not** change, asserted in `_ppt_deck()` on every run: `PowerPoint Document`, `Pictures`,
`Current User`, `\1CompObj`, `\1Ole` and `\5DocumentSummaryInformation` are byte for byte the
streams LibreOffice wrote. That is what keeps these fixtures files this project did not author — the
recorded constant is still LibreOffice's exact output, and every deviation from it is code you can
read rather than a blob you cannot.

Verified rather than argued:

| Check | Result |
|---|---|
| `cfb::open` on all four fixtures | opens, 8 entries each, expected stream lengths |
| `soffice --convert-to pptx basic-slides.ppt` | 2 slides, all five text runs |
| `soffice --convert-to pptx mixed-media.ppt` | 3 slides, all six text runs, `ppt/media/image1.png` present |
| `truncated.ppt`'s two dangerous properties | **unchanged** — they are offsets *inside* the `PowerPoint Document` stream, whose content the strip never touches |
| `encrypted.ppt` | still exactly 4 bytes from its base, still `0xF3D1C4DF` |


## The fixtures

| File | What it is for |
|---|---|
| `basic-slides.ppt` | Two slides, title and body. Recorded from `corpus/pptx/basic-slides.pptx`, so criterion 1 («with LibreOffice, fidelity equivalent to the pptx path») is measurable against a deck the corpus already pins with a golden. The pairing *is* the fixture |
| `mixed-media.ppt` | Three slides, titles, bullets at two levels, and a picture. Slide boundaries, text extraction, and a 225-byte `Pictures` stream — criterion 2's whole surface |
| `mixed-media.source.pptx` | The generated `.pptx` the deck above was converted from. Lives in `corpus/ppt/` rather than `corpus/pptx/` so **no gate reads it**: it is an input to a conversion, not a deck under measurement. It keeps the recipe runnable and the legacy deck's content reviewable as XML |
| `truncated.ppt` | Derived. Exists to be refused |
| `encrypted.ppt` | Derived. Exists to be refused — criterion 3 |

## The truncation, which is the security surface

A fixture that only *looks* truncated proves nothing, so this one is built to make a naive offset
read genuinely run off the end. `head -c` would not do it: the LibreOffice file puts its directory
in the **last two sectors**, so cutting the file makes it fail at open, and the refusal would come
from the `cfb` crate rather than from anything this project will write.

Instead `_cfb_truncate_stream` cuts the **content** of the `PowerPoint Document` stream to half,
rounded up to a sector, and keeps the container consistent while doing it: the sector chain is
re-terminated, the dropped sectors are freed **and zeroed** so the content is gone rather than merely
unreferenced, and the directory entry's size is restated. The cut is held above the 4 096-byte mini
stream cutoff on purpose — below it the stream would have to move into a different allocation
entirely.

Verified with the real `cfb` crate: the file **opens without a word** and hands back a 5 632-byte
stream. Every trap is therefore in front of our own code, not behind the container's:

- `CurrentUserAtom.offsetToCurrentEdit` still says **10 946** — the `UserEditAtom` sits **5 314
  bytes past the end** of what exists. [MS-PPT] makes that offset the first thing a reader follows.
- The cut lands **inside** a record: the header at **5 300** survives and declares a length reaching
  to **7 164**, so a walk that trusts `recLen` overruns with no seek at all.

Both are asserted in `ppt_truncated()`, so a refreshed constant whose record layout happened to put
a boundary at the cut fails at generation time instead of quietly becoming a fixture that tests
nothing.

## The encrypted fixture, and the caveat that goes with it

`CurrentUserAtom.headerToken` is set to `0xF3D1C4DF` ([MS-PPT] 2.3.2), which is how a conforming
reader learns a document is encrypted before it parses anything else. Exactly **four bytes** differ
from the base deck, and the generator asserts that.

The caveat, stated because a fixture that overstates what it proves is worse than none: **the content
behind that flag is not ciphertext.** Nothing in this tree writes RC4-encrypted binary PowerPoint,
and LibreOffice cannot either — `--convert-to 'ppt:MS PowerPoint 97:{"EncryptFile":…,"Password":…}'`
was tried and produced a file **byte-identical to the unencrypted one**, which is itself worth
recording: the option is accepted and silently ignored. So the fixture proves the **refusal**, which
is what criterion 3 asks for, and proves nothing about decryption. Nothing in this phase claims
otherwise.

## What can and cannot gate a `.ppt`

Named here rather than left silent, because [[93-phase-17-close]] carried forward exactly this defect
for spreadsheets and it should not be repeated:

- **`schema_gate.py` — not applicable.** It validates OOXML parts against the ECMA-376 XSDs. A
  `.ppt` has no XML and no schema.
- **`render_gate.py` — not applicable.** Its property is a round trip, and `.ppt` is read-only by
  design; there is no written package to render against the one that was read. (A *render* of the
  legacy file against a render of its `.pptx` twin is a thinkable 19-B measurement, but it would
  measure LibreOffice's importer, not this project's, and it is not proposed.)
- **`opc_check.py` — not applicable.** No OPC package.
- **Golden DocMark — not yet.** There is no reader until 19-B/19-D; the goldens arrive with it.
- **`generate.py` — this is the gate, and it is the only one.** `_assert_ppt` checks each recorded
  deck's `RT_Slide` (0x03EE) count, its `Pictures` stream and its plaintext `headerToken`;
  `ppt_truncated` checks both dangerous properties of the cut; `ppt_encrypted` checks the token and
  the four-byte blast radius. `--check` runs the generators, so CI runs all of it on every push.

## Verification

`python3 corpus/generate.py --check` passes; the corpus went from 143 to 148 generated files (this
increment's five), to 165 counting the concurrent Phase 18 `.odp` work sharing the same working
copy, and to 166 once `digest_tree` started seeing subdirectories. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `python3 corpus/schema_gate.py` and
`LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer` were
run; the numbers are in the increment's report. Nothing in `corpus/pptx/` moved, so the 21 decks
still round-trip byte-identical and the schema gate still validates the same parts.

## Carried into 19-B and later

- **No `.ppt` golden exists**, by construction. The first reader increment owns creating them, and
  `crates/docsai-convert/tests/doc_phase5.rs` is the shape to copy.
- **The recorded constants are version-pinned to LibreOffice 24.2.7.2 in provenance only.** If a
  future refresh changes them, the `_assert_ppt` structural assertions are what will catch a
  silently different deck.
- **The compaction is only ever applied to a file LibreOffice wrote.** `_cfb_compact` assumes a
  512-byte sector, no DIFAT sector chain and at most 109 FAT sectors, and asserts all three. It is
  not a general compound-file rewriter and should not be used as one.
- **`corpus/ppt/mixed-media.source.pptx` is an ungated `.pptx`.** That is deliberate and stated, but
  it is a package no gate validates. If it ever needs to be, it should move to `corpus/pptx/` and
  take a golden, an `.expected.inspect.json` and a token-budget line with it.
- **`corpus/generate.py` is shared by two concurrent phases, and the shared working copy bit
  once.** The `subdir` keyword on `build_pptx` is the only shared surface this increment's code
  touched, but the *history* is tangled: 19-A landed as `a100024e`, and the thumbnail strip that
  followed it was sitting in the shared working copy when the Phase 18 agent committed, so
  `corpus/generate.py`, `corpus/README.md` and the four `corpus/ppt/*.ppt` files of that follow-up
  are inside **`4e1db8b2` "feat(corpus): Phase 18 A"** rather than a commit of this phase's. The
  tree is correct and every gate is green; the attribution is not. Untangling it means rewriting
  another agent's commit while that agent is still working, and separating it by path is impossible
  anyway because both phases edit the same two files. It is recorded here rather than silently
  fixed or silently left.

[[95-phase-19-plan]] · [[08-phase-5-legacy-doc]] · [[93-phase-17-close]]
