---
tags:
    - phase-19
    - legacy
    - ppt
    - libreoffice
    - security
---
# 102 — Phase 19-B: the LibreOffice route for legacy `.ppt`

Second increment of [[95-phase-19-plan]], plan task 1. `soffice --convert-to pptx` into a sandboxed
temporary directory, then the Phase 13 pipeline unchanged. Its acceptance criterion is the phase's
first: **with LibreOffice, fidelity equivalent to the pptx path**, measured against the `.pptx`
originals [[97-phase-19-corpus]] kept in the tree precisely so it could be measured.

The native degraded reader is 19-D ([[99-phase-19-parser-choice]] specifies its shape); the refusal
of an encrypted deck is 19-E. This entry ends with what the LibreOffice route does with both refusal
fixtures today, because one of those answers is a defect 19-E must close.

## The history this increment repairs

Phase 18's increment 18-D landed as `958ac1e` while an unfinished 19-B sat in the same working copy,
and `jj commit` takes whole files: about 46 lines went in early — the `Format::Ppt` row of `SUPPORT`
in `crates/docsai-convert/src/lib.rs` and the `try_loffice_doc` → `try_loffice` generalisation in
`pipeline.rs`. Nothing was reverted; the tree was simply red, because
`every_format_appears_exactly_once` still asserted nine formats against a table that had ten. That
assertion is a gate — it is what stops a format being added twice or silently — so it was **moved to
ten**, not deleted and not loosened into an inequality. This commit completes the wiring around what
landed early, so the history reads honestly.

## The shape, and why there is only one of it

`.doc` → `.docx` (Phase 5) and `.ppt` → `.pptx` (19-B) are the same operation with a different
extension, so they are the same code rather than two:

- `loffice::loffice_route(Format) -> Option<(Format, &'static str)>` is the whole policy table —
  `Doc → (Docx, "docx")`, `Ppt → (Pptx, "pptx")`, everything else `None`. `benefits_from_loffice` is
  now `loffice_route(..).is_some()`, so the two can never disagree. `.xls` is deliberately absent:
  it reads natively through `calamine` and wants no pre-conversion.
- `loffice::convert_with_loffice(soffice, input, out_dir, extension)` is one sandboxed child;
  `convert_to_docx` and `convert_to_pptx` are two names for it.
- `pipeline::try_loffice` is one function governed by one `--use-loffice auto|never|require`. A third
  legacy format would be one line in `loffice_route` and nothing else.

The converted deck goes into `docsai_office::read(Format::Pptx, ..)` — **the Phase 13 reader, with no
accommodation of any kind**. That is what makes the criterion a measurement instead of a claim, and
it is why the answer to «did the pptx reader need changing?» had to be *no*. It was: the reader read
LibreOffice's output unchanged, and the differences below are all differences in the *deck*, not
failures of the reader.

## The sandbox

Not a second discipline — the one `crates/docsai-convert/src/render.rs` established in Phase 16-G:

- An isolated `-env:UserInstallation=` profile **inside** the caller's `tempfile::tempdir()`, so
  concurrent conversions never share a profile and the child writes nothing outside a directory that
  is removed when it drops.
- A wall-clock ceiling — `CONVERT_TIMEOUT`, the renderer's 300 s — enforced by **polling**
  `try_wait`, not by blocking on a read a hung child would never end. A legacy binary file is
  attacker-shaped input handed to a large C++ program; a child with no ceiling is a hang, not an
  error.
- The library path **derived at run time from the resolved `soffice`**, never hard-coded. On this
  machine `/usr/bin/soffice` cannot load its own libraries without its program directory on
  `LD_LIBRARY_PATH`; 16-G's `apply_library_path` finds it from the resolved binary, which is why the
  route works here with no machine-specific path anywhere in the repository.

The one change `render.rs` needed was `run_with_status`, which hands back the child's `ExitStatus`
alongside the captured log; `run` is now that, with the status dropped. The PDF export checks the
*produced file*, the legacy conversion checks the *status* — same clock, same capture, one child
process discipline.

## What «fidelity equivalent to the pptx path» turned out to mean

Measured by converting `corpus/ppt/mixed-media.ppt` through LibreOffice and
`corpus/ppt/mixed-media.source.pptx` — the deck the fixture was made *from* — directly, and diffing
the DocMark. Same for `basic-slides.ppt` against `corpus/pptx/basic-slides.pptx`.

**What survives the trip out to `.ppt` and back, exactly:**

| Thing | Evidence |
|---|---|
| Slide count, order, and title text | identical `## …` lines |
| Every bullet, at its own indent level | identical `- ` / `  - ` lines, two levels deep |
| The picture, **byte for byte** | `img-40e10599.png` — the asset name is a content hash, and it is the same hash on both sides |
| Document metadata | `title`, `author`, `last-modified-by`, `created`, `modified`, `language` all identical |
| Layout *reference* | both sides point at `ppt/slideLayouts/slideLayout1.xml` |

**What does not, and why:**

| Lost or changed | Original | Via `.ppt` |
|---|---|---|
| `application` | `docsai-corpus` | `LibreOffice/24.2.7.2…` |
| Layout **name** and body placeholder index | `Titulo y objetos`, `body: 1` | `Predeterminado`, `body: 0` |
| Picture **alternative text** | `Rectángulo azul` | *(empty)* |
| Shape **name** | `name="Imagen 1"` | *(absent)* |
| Placeholder identity of the body | body text sits directly under the slide | wrapped in an explicit `::: {.shape pos=… size=…}` |
| Character and paragraph formatting | inherited, so unwritten | **every run** carries `bold`, `color`, `font`, `size`; every paragraph carries `space-before`/`space-after` and its indents |

The single cause of most of that row is structural, and it is worth naming plainly: **a `.ppt` has no
OOXML layout parts at all.** There is nothing for LibreOffice to carry across, so on export it
*synthesises* a default master and layout and, having no placeholder inheritance to rely on, writes
every run's formatting explicitly. So «equivalent» is precisely: **the content is equivalent; the
package around it is LibreOffice's reconstruction.** Text, structure, order and media come back
whole. Styling identity — which layout, which placeholder, whose theme — does not, and cannot.

That is what the route's one warning now says, and it says no more than that:

```
[Minor] degraded ppt-via-libreoffice: legacy .ppt pre-converted to pptx with `/usr/bin/soffice`
before the pptx pipeline; the pptx package this was read from is LibreOffice's reconstruction of
the binary file, so its part structure — styles, layouts, placeholder identity — is LibreOffice's
and not the document's own
```

The claim is invariant, so it is never false. Naming a *specific* loss would be: this process never
sees the original, so it cannot know whether a given deck had alt text to lose. The measured, per
fixture losses live in this table rather than in a warning that would sometimes lie. §7 rule 3 cuts
both ways, and the second half is the one that constrained the wording.

`the_route_says_it_took_the_route_and_says_nothing_it_did_not_do` asserts there is **exactly one**
`Degraded` warning and that it is the route itself.

## `--use-loffice`, and the message `never` gives

Same three values, same meanings as `.doc`.

| Value | On a `.ppt`, today |
|---|---|
| `auto` (default) | converts through LibreOffice when `soffice` resolves; without it, falls through to the native path, which is the 19-D seam and refuses by name |
| `require` | converts, or fails saying LibreOffice is required |
| `never` | **refuses**, exit 2 |

`never` must not produce a generic «unsupported», which sends a user looking for a fault in a file
that is fine. `crates/docsai-office/src/ppt/mod.rs` is a real module — the sibling of `doc/` that
19-D will fill — whose `read` returns a new `ReadError::NoNativeRoute { format, missing }`:

```
docsai: no native reader for `ppt`: the native degraded .ppt reader is Phase 19-D and is not in
this build. Today a legacy .ppt is read by pre-converting it to .pptx with LibreOffice, which
`--use-loffice never` forbids. Re-run with `--use-loffice auto` or `--use-loffice require`, and
install LibreOffice (or point DOCSAI_LIBREOFFICE at its `soffice` binary) if it is not on this
machine
```

It is honest about *today* rather than about the plan: it names the increment, the flag that would
have worked, and the environment variable. The seven `--use-loffice` help strings in the CLI said
«for legacy `.doc`»; they now say «for legacy `.doc` and `.ppt`».

## Detection

`.ppt` is recognised two ways, and both were needed:

- `doc::classify_ole2` looks for the `PowerPoint Document` stream ([MS-PPT] 2.1.1) inside the
  compound file — content, not name, so all three fixtures come back `Certain`.
- `detect.rs` falls back to the extension (`ppt`, `pot`, `pps`) when the OLE2 payload cannot be
  classified, beside the `xls`/`xlt` case that was already there.

`Format::Ppt` parses from `ppt` and `pot`, is `is_presentation()`, and is in `docsai_office::READABLE`
but **not** `WRITABLE`. There is no `write` in `ppt/mod.rs` to grow into one.

## The refusal fixtures — what the LibreOffice route does with each today

Neither is this increment's to settle. Both were run, because 19-D and 19-E need the answers.

### `truncated.ppt` — refused, but the refusal is LibreOffice's

```
docsai: LibreOffice fallback: LibreOffice reported success but no .pptx appeared in /tmp/.tmpHxhGB9:
Error: source file could not be loaded
```

Exit 2, nothing written. The honest outcome — but note *how* it arrives: `soffice` exits **zero** and
declines to produce a file, so what catches it is `convert_with_loffice`'s check that the expected
output actually appeared. Had that check not existed, a zero exit status would have read as success.
19-D owes the same refusal without LibreOffice, from its own bounds checks; [[99-phase-19-parser-choice]]
already records that `office_oxide` returns `Ok` here, which is half of why it was rejected.

### `encrypted.ppt` — **not refused. LibreOffice reads it and hands back the whole deck.**

This is the finding 19-E needs, and it is a defect in the product as it stands:

```
$ docsai convert corpus/ppt/encrypted.ppt --use-loffice require
docsai: 1 warning(s), 0 raw-block(s)
  [Minor] degraded ppt-via-libreoffice: …
$ head -20 encrypted.md
title: "Presentación básica"
## [Informe trimestral]…
- [Ingresos al alza]…
```

Exit 0. Full content. The reason is exactly the caveat [[97-phase-19-corpus]] wrote down: the fixture
sets `CurrentUserAtom.headerToken` to `0xF3D1C4DF` ([MS-PPT] 2.3.2), which is how a *conforming*
reader learns a document is encrypted, but the four bytes behind it are not ciphertext, and
LibreOffice does not consult the token. So the third acceptance criterion — **an encrypted or
protected `.ppt` rejected with a clear error** — is **unreachable through the LibreOffice route by
construction**, no matter how the child is invoked.

The consequence for 19-E's design is concrete: **the `headerToken` check must happen in our code,
before the file is handed to LibreOffice at all** — a pre-flight on the `Current User` stream in
`try_loffice`, or better in a shared entry point both routes pass through, so the native route and
the LibreOffice route refuse the same file for the same reason. A check that lives only in the 19-D
reader would leave `--use-loffice auto` reading encrypted decks. This is not fixed here: 19-E owns
criterion 3, and guessing at its shape now would be the scope creep the plan forbids.

Note also that this is the same failure `office_oxide` was rejected for — `headerToken` never
consulted — reached by a different road. The token being ignored is evidently the common case, which
is an argument for the project checking it itself rather than for trusting any reader to.

## Two defects fixed on the way

1. **The test wrote into the corpus.** `crates/docsai-convert/tests/ppt_phase19.rs` called
   `convert_file` with a default `ConvertOptions`, and `assets_dir: None` means *«`assets/` next to
   the input»* — the input being `corpus/`. A workspace run left **fourteen** files under
   `corpus/ppt/assets/` and `corpus/pptx/assets/`, dirtying a golden tree, and a second run then
   failed on a directory the first had deleted. Every conversion in the file now gets its own
   `tempfile::TempDir`. The underlying behaviour — `docsai convert` putting sidecar assets beside the
   *input* rather than the output — is a carried defect of its own and is **not** fixed here; the
   test simply no longer depends on it.
2. **`crates/docsai-convert/tests/scratch_probe.rs`** was a throwaway probe left behind, and the sole
   `cargo fmt --check` failure in the tree. Deleted.

## What is carried forward

- **Criterion 3 cannot be met by this route** (above). 19-E, and the check belongs in front of the
  hand-off, not behind it.
- **`truncated.ppt`'s refusal depends on LibreOffice exiting zero *and* producing nothing.** The
  output-file check is load-bearing; do not simplify it into a status check.
- **`docsai convert` writes sidecar assets beside the input, not the output.** Known, carried,
  untouched. It is why every test here passes `assets_dir` explicitly.
- **No `.ppt` writer, and no route to one.** Read-only for the whole of Phase 19.

## Gates

`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `python3 corpus/generate.py --check` (200 files),
`python3 corpus/schema_gate.py` (21 decks, 42 packages, **469 parts**, 0 invalid) and
`LD_LIBRARY_PATH=… python3 corpus/render_gate.py --require-renderer` (21 decks + 17 `.odp`, 38 round
trips identical by bytes, **127 pages** compared, 0 failures). Nothing moved. `git status` is clean of
corpus files after two consecutive suite runs.

## Next

19-D — the degraded native route, to the shape [[99-phase-19-parser-choice]] specifies.
