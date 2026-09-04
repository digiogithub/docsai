---
tags:
    - phase-19
    - close
    - ppt
    - legacy
    - security
    - gates
    - risk-p2
---
# 106 — Phase 19-E: the refusal, and the phase closed

Increment **19-E** of [[95-phase-19-plan]], the fifth and last. It adds one feature — the refusal of
an encrypted `.ppt`, which is the phase's third acceptance criterion — and then owes what
[[104-phase-18-close]], [[93-phase-17-close]] and [[84-phase-16-close]] owed: the criteria answered
with evidence **re-run here**, what the phase carries forward written down rather than discovered
later, the state of risk P2 said plainly, and every budget and golden accounted for.

Six commits, in the order they landed: `125214c` the plan (shared with Phase 18), `a100024` 19-A,
`6ea8080` 19-A's follow-up, `a9acc6f` 19-C, `3ffdf35` 19-B, `23de99a` 19-D, and this one.

## A note on the history, before anything else

**Phases 18 and 19 ran concurrently in one working copy, and their commits crossed twice.**
[[104-phase-18-close]] records it from Phase 18's side; it is recorded here too, because a reader
running `git log` on these files will otherwise be misled about who wrote what.

- **`4e1db8b` (18-A) carries 19-A's thumbnail-stripping** of `corpus/generate.py` and the
  regenerated `corpus/ppt/*.ppt` — `_property_set_without_thumbnail`, `_cfb_shrink_stream`,
  `_cfb_compact`, the `[MS-OSHARED]` summary-information surgery that took each deck from 608 KB to
  23 KB. It is inside a Phase 18 commit because `--check` regenerates the *whole* corpus from one
  script: leaving the generator half-changed would have made `python3 corpus/generate.py --check`
  red on a tree where nothing was wrong. Separating it by path was impossible anyway — both phases
  edit `corpus/generate.py` and `corpus/README.md`.
- **`958ac1e` (18-D) carries 19-B's in-flight `Format::Ppt` wiring** in
  `crates/docsai-convert/src/{lib,pipeline}.rs` — the tenth `SUPPORT` row and the read dispatch that
  reaches it — because `jj commit` takes whole files and those two were in the shared working copy.
  **`HEAD` was red for exactly one commit**: `every_format_appears_exactly_once` asserted nine
  formats against a table holding ten. `3ffdf35` (19-B) completed the wiring and moved that
  assertion to **ten** — moved, not deleted and not loosened into an inequality, because it is the
  gate that stops a format being added twice or silently.

The tree was correct at every point and every gate is green today; only the attribution is
imperfect. Untangling it would mean rewriting another agent's commits, so it is written down instead.

## What the phase set out to do, and what it did

The plan's objective was «the `.doc` strategy applied to `.ppt`, read-only». What that turned into is
worth stating as a shape rather than as a list of increments, because the shape is the result:

**There is one legacy route, not two.** `.doc` → `.docx` and `.ppt` → `.pptx` are the same operation
with a different extension, so 19-B made them the same code: `loffice::loffice_route` is a two-row
policy table, `convert_with_loffice` is one sandboxed child, and `pipeline::try_loffice` is one
function under one `--use-loffice auto|never|require`. A third legacy format is one line in
`loffice_route` and nothing else. `benefits_from_loffice` is defined *as* `loffice_route(..).is_some()`,
so the two can never disagree.

**The `.pptx` reader was not touched.** The deck LibreOffice produces goes into
`docsai_office::read(Format::Pptx, ..)` with no accommodation of any kind, which is the only thing
that makes criterion 1 a measurement instead of a claim.

**The native reader is ours, and `office_oxide` is not linked.** 19-C weighed it properly — it is
*cheap*, +98 KB and +30 s — and rejected it on behaviour. 19-D then wrote 1 508 lines of production
Rust over `cfb`, and recovered two things the plan's own sketch did not promise: outline levels, and
a picture's position on its slide.

**And the whole surface is degradation, so the second half of AGENTS.md §7 rule 3 mattered as much
as the first.** Each route emits exactly **one** `Degraded` warning, and each warning's claim is
*invariant* — «LibreOffice reconstructed the package» is true of every deck the LibreOffice route
reads, and «no formatting, no layouts, no masters, no themes, no tables, no charts, no notes» is
true of every deck the native route reads. Neither warning can be false, and neither route warns
about a loss that did not happen. That is why 19-D **corrected 19-C in writing**: 19-C's specified
`ImageGeometryDegraded`-per-picture would have been a false warning on every picture in every deck.

## The three acceptance criteria, each with evidence re-run in this increment

### 1 — «With LibreOffice: fidelity equivalent to the pptx path» ✔

Measured, not asserted, and measurable only because 19-A kept both ends of the comparison in the
tree: `corpus/ppt/mixed-media.ppt` was produced *from* `corpus/ppt/mixed-media.source.pptx`, and
`corpus/ppt/basic-slides.ppt` from `corpus/pptx/basic-slides.pptx`, which the corpus already pins
with a golden.

`the_libreoffice_route_reads_the_same_slides_as_the_pptx_original` and
`a_picture_survives_the_legacy_round_trip_byte_for_byte` in
`crates/docsai-convert/tests/ppt_phase19.rs` are that comparison as assertions. Both **ran** in this
increment — the file's LibreOffice-dependent tests print «skipped» when `soffice` does not resolve,
and nothing printed it:

```
running 14 tests … test result: ok. 14 passed; 0 failed; 0 ignored
```

By hand, outside the suite, on `mixed-media`:

```
docsai convert corpus/ppt/mixed-media.ppt        --use-loffice require -o lo.md
docsai convert corpus/ppt/mixed-media.source.pptx                      -o src.md
diff <(headings+bullets of src.md) <(headings+bullets of lo.md)   →  IDENTICAL
img-40e10599.png on both sides    (the asset name is a content hash)
```

**What «equivalent» is, precisely.** The *content* is equivalent: slide count and order, every title,
every bullet at its own indent level, the document metadata, and the picture byte for byte. The
*package* is not, and cannot be — a `.ppt` has no OOXML layout parts at all, so LibreOffice
synthesises a master and a layout on export and, having no placeholder inheritance to rely on,
writes every run's formatting explicitly. [[102-phase-19-loffice]] tabulates every difference.
The route's warning claims exactly this and nothing file-specific, so it is never false.

### 2 — «Without it: full text with correct slide boundaries, no panics» ✔

`the_native_route_reads_the_same_text_and_boundaries_as_the_pptx_original` measures the native reader
against the *same* `.pptx` originals with `--use-loffice never`, so «full» and «correct» are a
comparison rather than an opinion. Re-run by hand here:

```
docsai convert corpus/ppt/mixed-media.ppt --use-loffice never -o nat.md
diff <(headings+bullets of src.md) <(headings+bullets of nat.md)  →  IDENTICAL
                                       (two bullet levels deep, same img-40e10599.png)
```

The boundaries are not found by scanning for `RT_Slide` — that would find the masters and the notes
masters, in file order. They come from [MS-PPT] 2.1.2 followed exactly: `Current User` →
`CurrentUserAtom.offsetToCurrentEdit` → the `UserEditAtom` chain → merged persist directories
newest-first → the `SlideListWithTextContainer` at **`recInstance` 0x000** (1 is the masters, 2 the
notes). And the check that would catch a misread list is there: every persist entry the list did not
name is looked at, and an unnamed `RT_Slide` raises `Degraded { what: "ppt-slides" }`. Neither
fixture produces one.

**No panics**: the four fixtures go through the reader in the unit tests and the integration tests,
`truncated.ppt` returns an `Err` naming the offset it refused, and the module has no clamping
accessor to fall back on — see §«The security posture».

### 3 — «Encrypted/protected `.ppt` rejected with a clear error» ✔ — this increment

**Where it sits is most of the deliverable.** 19-B established the constraint: LibreOffice reads
`corpus/ppt/encrypted.ppt` happily and hands back the whole deck, because it never consults
`CurrentUserAtom.headerToken`. A refusal living only in the native reader would therefore have been
skipped by `--use-loffice auto` — the default — on every machine that has LibreOffice, which is every
machine where the phase's *first* criterion is met. So the check is **in front of the choice of
route**:

- `docsai_office::refuse_if_encrypted(format, reader)` (`crates/docsai-office/src/lib.rs`) — the
  public pre-flight. It reads one 4-byte field of one small OLE2 stream and is deliberately
  conservative: it answers `Ok(())` for a format with no such flag **and** for a file it cannot make
  sense of, because refusing a *broken* file is the reader's job and the reader can say where and
  why.
- `pipeline::read_path_with_options` calls it **above** the `benefits_from_loffice` branch. Both
  routes therefore pass through one call, and refuse the same file for the same reason with the same
  words.
- `ppt::read` refuses the same file with the same message for a caller that reaches
  `docsai-office` directly — the bytes door (stdin, the in-memory MCP helpers) has no LibreOffice
  route at all, so the reader's own check is what covers it.

The message is part of the criterion, so it is one function (`ppt::encrypted()`) rather than two
copies, and it says what the file is, why it cannot be read, and what its owner can do:

```
docsai: this legacy PowerPoint file (.ppt) is encrypted or password-protected
(CurrentUserAtom.headerToken = 0xF3D1C4DF in the `Current User` stream, [MS-PPT] 2.3.2);
docsai decrypts nothing, on either route — neither the native reader nor the LibreOffice
pre-conversion — and reading it anyway would produce a document that silently lacks its
content. Open it with its password in PowerPoint or LibreOffice, save it as .pptx (or as an
unprotected .ppt), and convert that
```

Demonstrated by hand on all three policies, exit code included:

| Command | Result |
|---|---|
| `docsai convert corpus/ppt/encrypted.ppt --use-loffice auto` | the message above, **exit 2** |
| … `--use-loffice never` | the same message, **exit 2** |
| … `--use-loffice require` | the same message, **exit 2** |

And through the MCP door, on a real stdio server: `convert_to_markdown`, `inspect_document` and
`outline_document` on the same path each return `isError: true` carrying that same text.

`an_encrypted_deck_is_refused_on_every_use_loffice_policy` is the three-policy loop as a test — it
needs no LibreOffice, so it runs everywhere — and it also asserts that **nothing was written**: the
assets directory is empty after each refusal.

### `truncated.ppt`, the other refusal fixture

Not a criterion of its own, but it is refused on every route too, and *how* differs:

| | |
|---|---|
| `--use-loffice never` | **ours**: `` `PowerPoint Document` is truncated: 8 byte(s) wanted at offset 10946, but the stream holds 5632``, exit 2. `persist::read_persist_directory` → `Window::slice(10946, 8)` on a 5 632-byte stream, on the very offset [MS-PPT] tells a reader to follow first, with no scan fallback |
| `--use-loffice auto` / `require` | **LibreOffice's**: `soffice` exits **zero** and produces no file, and what catches that is `convert_with_loffice`'s check that the expected output actually appeared. Exit 2, nothing written |

The second is worth stating because the output-file check is **load-bearing**: simplify it into a
status check and a truncated deck becomes a silent success. Carried below.

### What the encrypted fixture proves, and what it does not

Stated plainly rather than left for a reader to assume more. [[97-phase-19-corpus]] is blunt about
it: **the content behind the flag is not ciphertext.** Nothing in this tree writes RC4-encrypted
binary PowerPoint, and LibreOffice cannot either —
`--convert-to 'ppt:MS PowerPoint 97:{"EncryptFile":…,"Password":…}'` was tried in 19-A and produced a
file **byte-identical to the unencrypted one**; the option is accepted and silently ignored.
`encrypted.ppt` differs from `basic-slides.ppt` in exactly **four bytes**, asserted by the generator.

So the fixture proves **the refusal fires, on every route, with a message that says why** — which is
exactly what criterion 3 asks for, since the criterion is about *rejecting*, not about decrypting.
It proves **nothing** about decryption, nothing about RC4 or about [MS-OFFCRYPTO], and nothing about
what this reader would do with genuinely encrypted content behind that flag. Since the refusal
happens before any content is parsed, the second question is moot today — but it will stop being
moot the moment someone proposes decryption support, and the fixture will not answer it.

## The gate run, in full

Every command run by this increment, on this machine, after the change:

| Command | Result |
|---|---|
| `cargo test --workspace` | **60 suites, 971 passed, 0 failed, 1 ignored** (was 968/0/1; +2 unit and +3 integration, −1 unit and −1 integration replaced by refusal tests) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `python3 corpus/generate.py --check` | **201** generated files, unchanged |
| `python3 corpus/schema_gate.py` | 21 decks, 42 packages, **691 parts**, 106 of them `.odp`, **0 invalid** |
| `LD_LIBRARY_PATH=… python3 corpus/render_gate.py --require-renderer` | 21 decks + 17 `.odp`, **38 round trips identical by bytes**, 55 rendered, **254 pages compared**, **0 failures** |

**A test must not write into the corpus**, and it does not: `git status --short corpus/` is **empty**
after two consecutive `cargo test --workspace` runs. Every conversion in `ppt_phase19.rs` is handed
its own `tempfile::TempDir`, which is what 19-B established after a workspace run left fourteen files
under `corpus/ppt/assets/` and `corpus/pptx/assets/`.

The `1 ignored` is the inherited Phase 14 criterion (`agent` fidelity ≤ 15 % of `full`, measuring
~100 %), still `#[ignore]`d rather than softened. Carried item 46.

## What `.ppt` is, and is not, gated by

This is the part of the close that will be looked for later, because it is where this format differs
from every other one in the repository.

**Not applicable, by construction, all three:**

- **`corpus/schema_gate.py`** validates OOXML parts against the ECMA-376 XSDs (and now ODF against a
  vendored RELAX NG). A `.ppt` is a binary record stream. **There is no XML and no schema.**
- **`corpus/render_gate.py`** measures a *round trip*: read a package, write it back, render both,
  compare. `.ppt` is **read-only for ever**, so there is no written package to render against the one
  that was read. (A render of the legacy file against a render of its `.pptx` twin is thinkable, but
  it would measure LibreOffice's importer, not this project's, and it is not proposed.)
- **`corpus/opc_check.py`** — no OPC package.

**No golden either, and that is a decision rather than an omission.** Both readers exist and neither
has a `.expected.dmk.md`. The property worth pinning is not «this deck serialises to these bytes» but
«the legacy deck says what its `.pptx` original says», and 19-A put both `.pptx` originals in the
tree so that comparison can be made directly; `ppt_phase19.rs` is where it is asserted, on both
routes. A golden would additionally pin the run formatting LibreOffice *synthesises* on export, which
is LibreOffice's and would move under us on its next release.

**So `corpus/generate.py` is the only gate `.ppt` has.** `_assert_ppt` checks each recorded deck's
`RT_Slide` count, its `Pictures` stream and its plaintext `headerToken`; `ppt_truncated` asserts both
dangerous properties of the cut (the offset 5 314 bytes past the end, and the record header at 5 300
declaring a body ending at 7 164); `ppt_encrypted` asserts the token and the four-byte blast radius.
`--check` runs the generators, so CI runs all of it on every push. That is a real gate on the
*fixtures*; it is not a gate on the *reader*.

**Where that leaves the repository**, said next to the gap Phase 18 left open:

| Family | Schema | Render | Round trip | Golden |
|---|---|---|---|---|
| `pptx` | ✔ | ✔ | ✔ | ✔ |
| `odp` | ✔ (18) | ✔ (18) | ✔ (18) | ✔ |
| `docx`, `odt` | ✘ | ✘ | ✔ | ✔ |
| `xlsx`, `ods` | ✘ | ✘ | ✔ | ✔ |
| **`ppt`** | **✘ n/a** | **✘ n/a** | **✘ n/a** | **✘ by decision** |

The spreadsheet and text-document gap (carried item 16) is still the strongest item in the list,
because it hides other things. `.ppt` is a different shape of hole: nothing is *hidden*, because
there is nothing a schema or a renderer could say about a format this project never writes — what is
missing is any oracle other than the two `.pptx` files the fixtures were made from, and those were
made by the same LibreOffice that reads them back on one of the two routes.

## The security posture, and what Phase 20 inherits

This is the first format this project parses that was **designed with no threat model at all**, and
it parses it **two ways at once** — natively, and by handing the bytes to a large C++ program. Both
are in scope for Phase 20's fuzzing; neither waited for it.

**The native reader: structural bounds checking, not sprinkled checks.**

1. **One `Window` is the only way an offset becomes bytes** (`ppt/window.rs`). It carries the bytes,
   its absolute base, the stream length and the stream name; every accessor returns `Result`. There
   is exactly **one** range expression in the whole module — `bytes.get(off..end)` — reached only
   after a `checked_add`.
2. **Refuse, never clamp.** There is no clamping accessor, so `.min(bytes.len())` is *unavailable*
   rather than discouraged, and `#![deny(clippy::indexing_slicing)]` at the top of `ppt/mod.rs`
   makes it mechanical for every submodule. This is deliberate contrast: it is exactly the pattern
   the `.doc` piece table still uses (carried item 60).
3. **`recLen` is validated before the body is read.** A zero-length record still advances eight
   bytes, so no walk stands still; a child's body is a strict sub-window, so no record can contain
   itself; trailing bytes too short to be a header are an error, not padding.
4. **`offsetToCurrentEdit` is checked by the same `Window`, with no scan fallback** — the fallback
   `office_oxide` has is precisely why `truncated.ppt` comes back `Ok` from it with a slide of
   PowerPoint's placeholder prompt text.
5. **Caps the specification does not require**: `MAX_RECORD_DEPTH` 24, `MAX_SHAPE_DEPTH` 16,
   `MAX_EDIT_CHAIN` 4 096 plus a visited set, and a `StyleTextPropAtom` run expanded only as far as
   the text really is (a run declaring `u32::MAX` characters yields `text.len() + 1`).
6. **Encryption is read first**, before any content, and since this increment it is a refusal.

**What `cfb` does and does not check.** It opens the container: the header, the FAT/DIFAT, the
directory, and it will refuse a compound file whose *container* is inconsistent. It says **nothing**
about the contents of a stream. `corpus/ppt/truncated.ppt` was built to prove exactly that — the
compound file is intact and self-consistent, `cfb::open` returns without a word, and hands back a
5 632-byte `PowerPoint Document` stream whose own bookkeeping still points to offset 10 946. **Every
trap in that file is in front of our code, not behind the container's.** So the container crate is
not a safety boundary for record-level parsing, and nothing in this phase treats it as one. It is
also worth recording that `cfb` is fed attacker-shaped bytes directly and is itself unfuzzed here.

**The LibreOffice child** runs under Phase 16-G's discipline, unchanged: an isolated
`-env:UserInstallation=` profile inside the caller's own `tempfile::tempdir()`, so concurrent
conversions never share a profile and nothing is written outside a directory removed when it drops;
a wall-clock ceiling enforced by **polling** `try_wait` rather than blocking on a read a hung child
would never end; and the library path derived at run time from the resolved `soffice`. What it does
**not** have is a memory or CPU cap, or any sandbox beyond the filesystem one: a legacy binary file
is attacker-shaped input handed to a large C++ program, and the timeout is the only bound on what
that program does with it.

**What Phase 20 inherits, concretely**: six input parsers to fuzz, of which `.ppt` is the newest and
the most adversarial by construction; a `.doc` piece table with two `.min()` clamps that are the
first place to look; a `cfb` boundary that checks containers and not contents; and a corpus of `.ppt`
fixtures produced by LibreOffice rather than by PowerPoint, which is a bias in the seed corpus worth
knowing about before 72 hours are spent on it.

## The `office_oxide` decision, and how to re-open it

**Not linked.** The measurements are in [[99-phase-19-parser-choice]] and the argument is in
`docs/technical-analysis.md` §4.6. Restated here only in its load-bearing form:

- The rejection is **not on cost**. +98 KB stripped and +30 s per clean release build, against
  `ooxmlsdk`'s +28.7 MB and +311 s in spike P1. It is MIT/Apache-2.0, actively released, pure Rust,
  MSRV 1.88 — this workspace's floor.
- It is rejected on **behaviour, on the two fixtures built to be refused**: `encrypted.ppt` comes
  back as two ordinary slides (`headerToken` never consulted anywhere in the crate), and
  `truncated.ppt` comes back `Ok` with one slide of master placeholder prompt text, because a failed
  `offsetToCurrentEdit` falls back to scanning. Both silent. That is §7 rule 3 on the fixtures built
  to test it — **and no panic either time**, so memory safety is not the objection and should not be
  implied.
- And on **model**: `TextRun { text_type, text }` has no indent level, no shape identity and no
  geometry, and `\r`-joins a slide's paragraphs. The losses at that seam could not even be *warned
  about* honestly, because from outside the crate there is no way to know whether a given deck had a
  second bullet level to lose. 19-D then read exactly that level, which is what makes the objection
  concrete rather than theoretical.

**It is cheaply re-openable, and the re-test is written down**: run the four `corpus/ppt/` fixtures
through it again. If `encrypted.ppt` and `truncated.ppt` are refused and the model carries an outline
level and a shape, the decision changes and the cost was never the problem. Upstream issue #119 is
the open feature request for the first half. Nothing in `crates/docsai-office/src/ppt/` is derived
from it; it is kept as a cross-check on record layout, the role `ooxmlsdk` was left in by spike P1.

## Where every budget and golden stands

Checked in this increment, none loosened, none regenerated.

**1 — The residue budget: untouched, and untouchable.** `crates/docsai-convert/tests/plain_residue.rs`
still asserts `all <= 18.0` (line 430) and `prose <= 15.0` (line 434), and the suite is green. Phase
19 added no fixture to the deck corpora it measures, so the numbers did not move. Worth naming: the
residue probe measures what the **serialiser writes** over `corpus/pptx` and `corpus/odp`, and
**`.ppt` is not in it** — there is no `.ppt` residue figure, and there cannot usefully be one, since
the DocMark of a `.ppt` is the DocMark of whatever the two routes produced.

**2 — `corpus/token-budget.md`: not touched.** No corpus file changed, so no budget line could move.
It last moved in 18-C, downward by 31 tokens at `standard`, from a defect fix.

**3 — `LOOP_MAX_CALLS` / `LOOP_MAX_TOKENS` and the wire-cost suite: not touched.** `3` and `2 000` at
`crates/docsai-mcp/tests/wire_cost.rs:41-42`, asserted before the golden is consulted. Phase 19 adds
no tool and changes no schema, so the suite's total is where Phase 17 left it.

**4 — The goldens: none moved.** `python3 corpus/generate.py --check` counts **201** generated files,
the same number Phase 18 closed on, and reports no difference. No `.expected.dmk.md`, no
`.expected.inspect.json` and no fixture changed in this phase after 19-A. The `.ppt` fixtures
themselves are gated by the generator's structural assertions, as §«What `.ppt` is gated by» says.

**5 — `docsai formats` gained a tenth row and the row is honest.** `ppt: read yes, write no`, with a
note that now also states the refusal. `every_format_appears_exactly_once` asserts ten.

## Risk P2: still OPEN, unsigned after a fourth phase

`docs/spikes/P2-powerpoint-gate.md` is still an unrun, unsigned form — checked here: zero boxes
ticked. It asks the one question this repository cannot answer itself: **does PowerPoint open what
this project writes?** LibreOffice opens every written deck and every `.odp`, and the render gate
proves it on 254 pages; but LibreOffice is a tolerant loader and PowerPoint is the one that decides.

Phase 19 does not extend the writer surface — it writes nothing at all — so it does not make P2
*worse* in the way Phase 17 and Phase 18 did. What it does is add a fourth phase to the count, and
add a new sentence to the form's item 7: **this project now reads a format PowerPoint itself wrote
for twenty years, and the only decks it has ever read are LibreOffice's.** The obvious source of a
real-world `.ppt` — someone's own archive — is the same session that would sign the form.

It should be signed before Phase 20, because a hardening phase that measures only the properties this
repository already knows how to measure is a hardening phase that cannot surprise anyone.

## What Phase 19 carries forward

Collected here so Phase 20 finds them in one place instead of in five increments' prose. Items 1–52
are [[104-phase-18-close]]'s list, **each re-verified in this increment**; the text says where Phase
19 changed what an item says. Items 53–64 are this phase's own.

### Inherited from `kb/104`, re-verified here

1. **`docsai edit` and `apply_edits` cannot reach a `.odp`, and the refusal names the wrong thing.**
   Reproduced: an **empty** batch on `corpus/odp/basic-slides.odp` says `docsai: unsupported write
   format: odp` and leaves the file's md5 unchanged (`d9c0d182…` before and after). Fails safe, fails
   confusingly — `docsai formats` says `odp: write yes`. `crates/docsai-convert/src/edit/mod.rs:322`.
2. **`draw:rotation-angle` is not an ODF attribute, and this project reads *and* writes it.**
   Re-checked, all three still present: `crates/docsai-odf/src/styles.rs:260` reads it,
   `crates/docsai-odf/src/odt/write.rs:837` writes it, `corpus/generate.py:2100` generates it. So
   `crates/docsai-odf` writes invalid ODF, and it is why `odt/` and `ods/` are excluded from the ODF
   schema arm. Carried a sixth time.
3. **A slide or a shape the model added or removed is not written into a preserved `.odp`**, and
   neither is a table's cells, a picture, a chart's data, an ODF annotation or a preserved object's
   payload. Sixteen `Degraded` warnings in `crates/docsai-odf/src/odp/write.rs` name them.
4. **A chart, an ODF annotation and an OLE payload cannot cross into `.odp`.** Three writers this
   project does not have; each is a `Degraded` naming the construct **and** the dialect.
5. **A page layout used under two master pages collapses to one `Layout` and warns by name.**
   `Layout::master` is still `Option<MasterId>` — one field — at
   `crates/docsai-model/src/presentation.rs:166`. Cost to the package measured at zero; the cost is
   what an agent reading DocMark is told. No fixture fires it.
6. **DocMark-P carries no canvas size.** Re-checked: the string `slide-size` appears nowhere in
   `crates/` or in the specification. A deck written from its Markdown alone lands on the target
   template's page. The honest half is done (`resize_page`, `warn_outside_the_page`).
7. **`presentation:settings` has no IR node and is carried only by the skeleton.** Referenced once,
   in `crates/docsai-odf/src/odp/write_gen.rs:275`, as an ordering rule. A deck that lost its package
   loses it.
8. **`col-widths="0,0"` on a slide table.** Existing ODF behaviour, not this phase's:
   `corpus/odt/table-simple.expected.dmk.md:53` still reads `col-widths="0,0,0"`. Fixing it moves
   `.odt` goldens.
9. **A `draw:line` reaches the IR with no geometry.** Reproduced on `shapes-geometry.odp`:
   `::: {#n2 .connector geom=line name="Conector 1" raw=raw-0003}` — no `pos=`, no `size=`. The whole
   shape is preserved as a raw fragment, so nothing is lost silently. *Correction to kb/104's
   pointer*: the mapping is at `crates/docsai-odf/src/odp/mod.rs:795`, not `draw.rs`.
10. **`Presentation::styles` is empty for every `.odp`.** Reproduced: `inspect --json` on
    `basic-slides.odp` reports `styles: 0`. Correct de-automatization; if it reads as a defect, the
    fix is in `inspect`.
11. **A `draw:g`'s child paragraphs concatenate into one label.** Reproduced:
    `corpus/odp/raw-preserved.expected.dmk.md:20` is `Dentro del grupoTambién dentro`. The `.pptx`
    reader does the same on its own fixture — shared behaviour of the raw-shape stub.
12. **`macro-enabled` has no ODF mirror in the corpus.** Re-checked: 17 `.odp` fixtures, none named
    for macros. ODF has no macro-enabled MIME type; the trait exists, the *format distinction* does
    not.
13. **`docsai_model::assets::zip_kind` has no ODF branch** (`assets.rs:201`): it sniffs
    `xl/workbook.xml`, `word/document.xml`, `ppt/presentation.xml` in the first 64 KB and answers
    `application/zip` for everything else. Affects only the content type an asset is stored under.
14. **A preserved skeleton is a `.pptx` **and** `.odp` property, and the edit layer has not caught
    up.** `edit_matrix.rs`'s `Family` still has five variants — `Docx, Odt, Xlsx, Ods, Pptx` — and no
    `Odp`; the word appears nowhere in the file. See item 1.
15. **`odt/table-merged.odt`'s etag moves on a re-parse that changed nothing.** Re-run:
    `cargo test -p docsai-convert --test etag_stability` — **5 passed**, including the test that
    measures it. Blast radius one node; the cost is a false `StaleEtag`, never an overwrite.
16. **`docx/`, `xlsx/`, `odt/` and `ods/` still have no schema gate and no render gate.** Re-checked:
    `schema_gate.py` globs only `PPTX_CORPUS` and `ODP_CORPUS`, and its own comment at line 464 says
    the ODF arm is «one glob away» and that switching it on today reports exactly one failure —
    `corpus/odt/images-transformed.odt`, which is item 2. **Still the strongest item in this list**,
    because it is the one that hides the others.
17. **`fullCalcOnLoad` is verified against LibreOffice Calc, not Excel.** Owner action.
18. **The text parser drops an unknown container class *silently*.** Reproduced verbatim here: a file
    holding `::: {.sidebar}` / blank / `A` / blank / `:::` makes `docsai validate` report
    `container/unknown-class` at 5:1 — and `docsai convert` on the same file says **nothing** and
    exits 0. AGENTS.md §7 rule 3's silent degradation, in the parser.
19. **The deck goldens carry a true skeleton warning.** Re-measured by converting every golden:
    **20 of 21** `.pptx` and **16 of 17** `.odp`. The exception on each side is
    `images-anchored.expected.dmk.md`, which does not warn because it **refuses** first. The warnings
    are correct; the corpus, not the parser, is where it would be fixed.
20. **`set_notes` cannot remove a notes page from a preserved deck, and says so.**
21. **A selector is not a target** — version 1's narrowing, `docs/edit-operations.md`.
22. **A package stores no node ids, so a structural edit renumbers them on the next read.** Settled as
    a property (§5.4.1), guarded by the etag.
23. **`parse_shape` warns with the *slide's* line number, not the container's.**
    `crates/docsai-docmark/src/deck_parser.rs:569` still formats `chart table at line {line}` from
    the slide's line.
24. **`count_shape_images` no longer only counts images** — it counts a chart's embedded workbook too,
    correctly, and the name lies (`crates/docsai-convert/src/inspect.rs:424`).
25. **A path input is read twice on a cache miss** (`service.rs` canonicalises and reads the bytes to
    hash them, then `read_path_with_options` opens the file again). **Sharpened by this increment**:
    a `.ppt` on that path is now opened a **third** time, by the encryption pre-flight. Each read is
    bounded and none is of a different file; it is still one file opened three times.
26. **Legacy `.doc` is never cached**, by decision — the insert is gated on
    `!benefits_from_loffice(format)`. **Sharpened**: since 19-B, `benefits_from_loffice` also answers
    true for `.ppt`, so **a `.ppt` is never cached either**, for the same reason (the read depends on
    what is installed on the machine) and by the same line, which nobody had to remember to change.
27. **`sniff` labels a `.pptm` as `.pptx`** (`assets.rs:215`, a bounded substring search). Affects
    only the name an asset is stored under.
28. **A stub over `mc:AlternateContent` gets no name and no geometry**, so `order::sort` puts it last
    on its slide.
29. **A picture inside an opaque object is not extracted as an asset of the deck.** Reproduced:
    `inspect --json` on `ole-embedded.pptx` reports `images 0` and `media []` while the package
    carries `ppt/media/image1.png`.
30. **DocMark carries nothing for a slide transition or a timeline**, so the write-side warning cannot
    fire on the parse-and-write path. Reproduced: `raw-preserved.pptx` read directly emits **7**
    raw-related warnings; the same deck's DocMark written back emits **0**.
31. **`corpus/schema_gate.py` selects `mc:Fallback` before validating** (line 110), which is what a
    consumer does and therefore correct — and means `ole-embedded.pptx`'s slide 2, the branch holding
    the actual `p:oleObj`, has never been validated.
32. **No corpus deck hides a slide.** Re-checked: `show="0"` appears in **zero** files under
    `corpus/`, so 16-G's page-count-mismatch guard is reasoned rather than exercised.
33. **`***bold italic***` under `{.underline}` does not survive a docx round trip.** Reproduced
    verbatim: `[***text***]{.underline}` comes back as `[**\*text**]{.underline}[\*]{.underline}`.
34. **PowerPoint's modern comments are not read.** Reproduced on `comments-slide.pptx`:
    `[Severe] unsupported element <p188:cmLst> at ppt/comments/modernComment_1.xml`, part preserved.
35. **The `.ole` stub does not carry `progId`.** Reproduced: `progId` appears **0** times in the
    DocMark of `ole-embedded.pptx`. Carrying it is a normative DocMark change.
36. **A preserved chart's *shape* cannot be edited through DocMark — only its numbers.**
37. **A generated chart has no design and no embedded workbook, and a generated combo chart is one
    plot.**
38. **A multi-level category axis is not read**, and the `c:f` formulas are not in DocMark — the
    second deliberate.
39. **The cache charges an entry its *input* length plus its media bytes, not the size of the tree**
    (`cache.rs`, `weight`). Bounded and one-directional.
40. **Nothing caches a render.** Re-checked: the string `cache` appears **zero** times in
    `crates/docsai-convert/src/render.rs`. A second `render_slides` on the same deck spawns
    LibreOffice again.
41. **A classic pptx comment carries no thread and no resolved flag** — the format, not a defect.
42. **There is no `comments` counter in `ConversionStats` and no comments column in `inspect`'s slide
    inventory.**
43. **The human P2 gate itself.** `docs/spikes/P2-powerpoint-gate.md`, unrun, **zero** boxes ticked.
    Owner action.
44. **Gate 2's threshold of 2.5 has still never been scored against a real render.** Re-confirmed by
    this increment's gate run: 254 pages compared, every comparable page reporting `diff n/a
    (geometry)` because every round trip is byte-identical. `THRESHOLD = 2.5`,
    `corpus/render_gate.py:102`.
45. **The LibreOffice loader path is narrowed, not closed.** `render_gate.py` still needs
    `LD_LIBRARY_PATH=/usr/lib/libreoffice/program` in the command, which this increment set. The
    permanent fix is root and an owner action, and the variable stays out of every repository script.
    (Note the asymmetry, which is the *good* half: the Rust routes derive the library path at run
    time from the resolved `soffice`, so `docsai convert deck.ppt` needs no such variable — only the
    Python gate does.)
46. **The inherited Phase 14 criterion is still open.** `--fidelity agent` was to cost ≤ 15 % of
    `full` and measures ~100 % over the deck corpus. Still `#[ignore]`d rather than softened; it is
    the `1 ignored` in `971/0/1`, and it prints its own reason when the suite runs.
47. **A preserved table whose grid the model moved is copied with a warning**
    (`crates/docsai-office/src/pptx/write.rs:2550`). Nothing owns writing a moved grid back.
48. **The refused internal hyperlink in pptx, and its docx twin.**
49. **Preserved parts are never schema-validated**, by construction, for `.pptx`; for `.odp` the
    fixtures themselves are validated, which narrows the hole on one format without closing it.
50. **Both gates run only at `full`.** Re-checked: the string `fidelity` appears **zero** times in
    either script.
51. **`align`'s content-based pairing can still guess** when two or more slides are both edited *and*
    reordered (`pptx/write.rs:1145`). The real fix is an identity for a slide, which nothing in the
    chain carries.
52. **No real-world documents in the corpus.** All **201** corpus files are generated by
    `corpus/generate.py`. **Sharpened by this phase**: the `.ppt` fixtures are *recorded* rather than
    generated as XML, but they are LibreOffice's output, so no `.ppt` PowerPoint itself wrote has
    ever been through either route — and for a twenty-year-old format read from producers long gone,
    that gap is wider than it is for `.docx`.

### Found during Phase 19

53. **`.ppt` has no gate of any kind, and cannot have three of them.** No schema (no XML), no render
    gate (read-only, so no round trip), no OPC check, and no golden by decision.
    `corpus/generate.py`'s `_assert_ppt` / `ppt_truncated` / `ppt_encrypted` assertions gate the
    **fixtures**; nothing gates the **reader** except `crates/docsai-convert/tests/ppt_phase19.rs`'s
    comparison against the two `.pptx` originals. §«What `.ppt` is gated by» is the full statement.
54. **No genuinely encrypted fixture exists, for any format.** `corpus/ppt/encrypted.ppt` carries the
    *declaration* (`headerToken = 0xF3D1C4DF`) over plaintext content, because neither this tree nor
    LibreOffice can write RC4-encrypted binary PowerPoint. The refusal is proven; decryption, and
    anything about what a reader would do with real ciphertext, is not. `corpus/doc/encrypted.doc` is
    the same shape of fixture for the FIB's `fEncrypted`.
55. **`.doc`'s encryption check is *behind* the hand-off, where `.ppt`'s is now in front of it.**
    Reproduced here on `corpus/doc/encrypted.doc`: `--use-loffice never` gives the clear
    `docsai: document is encrypted or password-protected`, but `auto` and `require` give
    `docsai: LibreOffice fallback: LibreOffice reported success but no .docx appeared … Error: source
    file could not be loaded`, followed by advice about installing LibreOffice. It refuses — but by
    accident of LibreOffice declining the file, with a message that sends the user to the wrong
    place. `refuse_if_encrypted` was written with a `format` parameter precisely so a `.doc` arm can
    be added; it was left out of this phase because criterion 3 names `.ppt` and because the FIB
    check would need its own increment to be worth trusting in front of a route.
56. **`ReadError::NoNativeRoute` has no constructor, and stays.** Decided rather than deferred:
    removing a `pub` variant is a breaking change to make deliberately, its message shape — name what
    is absent and what to do about it, rather than «unsupported» — is the right one for the next
    legacy format routed before it is parsed, and nothing costs anything by its presence. What was
    wrong was its *documentation*, which claimed `.ppt` had no native reader; that is corrected.
    `crates/docsai-office/src/error.rs:38`.
57. **Speaker notes are not read by the native `.ppt` route.** `SlideAtom` carries a `notesIdRef` and
    the persist directory resolves it, so it is bounded work. Left out because plan task 3 does not
    name it and the blanket warning states the loss.
58. **The native `.ppt` route reads no character or paragraph formatting.** `StyleTextPropAtom`'s
    `rgTextCFRun` is walked past rather than parsed; the outline level and the bullet flag are all
    that is taken from the atom. Stated in the route's one warning.
59. **Three native-route paths are unit-tested but not fixture-tested.** `TextBytesAtom` (Latin-1, per
    [MS-PPT]) — LibreOffice writes `TextCharsAtom` throughout, so no fixture has one; metafile
    (EMF/WMF/PICT) and DIB BLIPs, recorded as `AssetIssue` rather than handed on as unopenable bytes;
    and `ImageGeometryDegraded`, which fires only for a `Pictures` BLIP no shape's `pib` points at.
    A deck PowerPoint itself wrote would exercise the first; see item 52.
60. **The `.doc` reader's clamping-instead-of-refusing pattern.** [[99-phase-19-parser-choice]] and
    [[105-phase-19-native]] name it as «the piece table's two `.min(bytes.len())` clamps at
    `piece_table.rs:237` and `:253`», and **the pointer is wrong** — checked here: the two
    `.min(bytes.len())` clamps are at `crates/docsai-office/src/doc/mod.rs:239` and `:255`, and
    `piece_table.rs` has the same pattern twice more as `.min(word.len())` at `:169` and `:183`
    (each behind an `fc >= word.len()` guard that warns). **Four sites, two files.** Not a defect
    today, and **the first place Phase 20's fuzzing should look**: it is exactly the pattern the
    `.ppt` reader was built to make unavailable, and a clamp is how a truncated document becomes a
    short document instead of an error.
61. **`truncated.ppt`'s LibreOffice refusal rests on the output-file check.** `soffice` exits **zero**
    and produces nothing; what catches it is `convert_with_loffice` asserting the expected file
    appeared. Do not simplify that into a status check.
62. **`docsai convert` writes sidecar assets beside the *input*, not the output.** Known, carried,
    untouched — it is why every test in `ppt_phase19.rs` passes `assets_dir` explicitly, and why a
    test that forgets dirties `corpus/`.
63. **The reading order and the list rebuild are now shared by `pptx` and `ppt`.** `order::sort` and
    `text::build_list` were widened to `pub(crate)` rather than copied, which is right — but `.ppt`
    has no golden, so a change to either surfaces there as a *test* failure rather than as a diff a
    reviewer can read.
64. **The encryption pre-flight is `.ppt`-only, conservative, and on the path door only.** By design
    on all three counts, and each is a thing to know: it answers `Ok(())` for a file whose `Current
    User` stream it cannot parse (so a broken file reaches the reader, which can say *where* it is
    broken); it is called from `pipeline::read_path_with_options`, which is the only entry point with
    a LibreOffice route today — if the bytes door ever gains one, the call must move with it; and
    `ppt::read`'s own identical refusal is what covers the bytes door meanwhile.

### Closed inside Phase 19, recorded so nobody re-opens them from an increment's prose

- **«Criterion 3 is unreachable through the LibreOffice route.»** ([[102-phase-19-loffice]].) True of
  that route and still true — **closed by moving the check in front of it**, which is this increment.
- **«`--use-loffice never` on a `.ppt` refuses by name.»** ([[102-phase-19-loffice]].) 19-D made it
  read. `ReadError::NoNativeRoute` became unconstructed as a result; see item 56.
- **«`encrypted.ppt` is read but never silently.»** ([[105-phase-19-native]].) The `Severe`
  `UnsupportedElement` 19-D emitted was explicitly a placeholder for this increment. It is **gone**,
  replaced by the refusal — as 19-D asked, rather than left behind as an unreachable branch.
- **«`crates/docsai-convert/tests/ppt_phase19.rs` writes into the corpus.»** Fixed in 19-B; verified
  again here, twice.
- **«19-C's picture warning is wrong.»** A `Pictures` BLIP *is* anchored, through `pib` → `foDelay` →
  the client anchor. Corrected in 19-D before it shipped; had it been followed as written it would
  have been a false warning on every picture in every deck. [[99-phase-19-parser-choice]]'s text is
  left as that increment's record, which is why the correction lives in [[105-phase-19-native]] and
  here.
- **`master_units` truncated on the first attempt.** `914_400 / 576` is 1 587.5; the integer constant
  put a 10-inch slide 2 880 EMU short. Caught inside 19-D by its own test; the multiplication now
  comes first.

## What Phase 20 inherits

A project that reads **seven** package families and writes six, converts between two presentation
dialects through one IR, and now parses a pre-XML binary format two different ways.

Four things to read before starting.

**Risk P2**, unsigned after four phases, and Phase 20 is the last place before v2.0 where signing it
could still change a decision.

**Carried item 16**, the spreadsheet and text-document gate gap — still the only format family with
no oracle of any kind, and Phase 18 demonstrated on `.odp` exactly how cheap closing it is.

**Item 53 and §«The security posture»**, together, because they are the same fact seen twice: the
newest and most adversarial parser in this repository is the one with the fewest gates around it, and
fuzzing is what would change that. Item 60 says where to point it first.

**Carried item 1**, a door that says it is open and is not: `docsai formats` says `odp: write yes`
and `docsai edit deck.odp` refuses with a message about an unsupported write format.

## Next

**Phase 20 — hardening and v2.0** (`docs/development-plan-v2.md:467`). **First, though**:
`docs/spikes/P2-powerpoint-gate.md`.

[[95-phase-19-plan]] · [[97-phase-19-corpus]] · [[99-phase-19-parser-choice]] ·
[[102-phase-19-loffice]] · [[105-phase-19-native]] · [[104-phase-18-close]] · [[93-phase-17-close]] ·
[[08-phase-5-legacy-doc]]
