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
  existing compound file. `build_pptx` gained one additive keyword, `subdir`, so a package can land
  outside `corpus/pptx`. `digest_tree` covers `ppt/`, and the five generators are in `GENERATORS`.
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
- ~600 KB per deck, of which about **95 % is a thumbnail** LibreOffice embeds in
  `\5SummaryInformation` (`PIDSI_THUMBNAIL`, a 588 580-byte `VT_CF` property). No command-line
  option suppresses it — `GenerateThumbnail=false` fused into a fresh profile's
  `registrymodifications.xcu` was tried and had no effect — and removing it would mean rewriting the
  compound file, which is writing one. Total added: **2.4 MB** across four `.ppt` files, three of
  which are near-copies that delta-compress against each other.

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
increment's five), and to 165 counting the concurrent Phase 18 `.odp` work sharing the same working
copy. `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
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
- **`corpus/ppt/mixed-media.source.pptx` is an ungated `.pptx`.** That is deliberate and stated, but
  it is a package no gate validates. If it ever needs to be, it should move to `corpus/pptx/` and
  take a golden, an `.expected.inspect.json` and a token-budget line with it.
- **`corpus/generate.py` is now shared by two concurrent phases.** The `subdir` keyword on
  `build_pptx` is the only shared surface this increment touched.

[[95-phase-19-plan]] · [[08-phase-5-legacy-doc]] · [[93-phase-17-close]]
