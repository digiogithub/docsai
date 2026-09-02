---
tags:
    - phase-15
    - pptx
    - presentations
    - ci
    - rendering
    - libreoffice
    - perceptual-diff
---
# 71 — Phase 15 J: the render gate, written on a machine that cannot render

Increment **15-J** of [[61-phase-15-plan]], on top of [[70-phase-15-xsd]]. The phase's second CI
gate: *headless LibreOffice renders the corpus to PNG before and after the round trip, a perceptual
diff over a threshold fails the build, and LibreOffice absent is a skip with a clean message, never
a green tick that measured nothing.*

The last clause is the increment. **LibreOffice is broken on this machine** —
`/usr/lib/libreoffice/program/soffice.bin: error while loading shared libraries: libreglo.so` —
as every increment since 15-D has recorded, so this gate was written by someone who could not watch
it render a single slide. That is an uncomfortable position and the honest response to it is not to
write something plausible and call it green: it is to make the skip unmistakable, to execute
everything that does not need the renderer, and to say in the record exactly which half was
measured. All three are done, and the third is the section «What was verified here, and what was
not» below.

The gate is `corpus/render_gate.py`, its CI job is `render-gate` in `.github/workflows/test.yml`,
and it runs locally as `python3 corpus/render_gate.py` (or `--self-test` for the comparator alone).

> **Amended after the fact.** The «cannot render» of the title turned out to be a *dynamic loader*
> fault in this machine's LibreOffice install, not a missing or corrupt package, and it has since
> been diagnosed and worked around. The gate has now been run for real here, with a renderer, and it
> exits 0. What that run proves — and the considerably smaller thing it does **not** — is written
> out in «The loader fault, and the gate's first real run» below. Nothing in the five decisions, the
> threshold or the negative controls changed; what changed is the account of what has been executed.

## The five decisions

### 1. What produces the images: PDF, then `pdftoppm`

LibreOffice can be asked for a PNG directly — and for a presentation it exports **one page**. A gate
built on `--convert-to png` would render the first slide of every deck and report the other sixty as
measured. PDF is the only LibreOffice export that carries the whole deck, so the render is two
steps: `soffice --headless --convert-to pdf`, then `pdftoppm -gray -r 100` for one greyscale raster
per page.

The split buys three things beyond correctness. The resolution is *stated* (100 dpi: a 10 × 7.5 inch
slide becomes a 1000 × 750 raster) rather than inherited from whatever the exporter felt like. Colour
is dropped deliberately — a perceptual diff over luminance is what a person notices first, and it
divides the work by three. And the raster arrives as a **PGM**, which matters for decision 2.

What must be installed grows by one package (`poppler-utils`), which is present on every GitHub
runner image and is a 1 MB apt install. The CI job installs `libreoffice-impress poppler-utils
fonts-liberation` with `--no-install-recommends`; the fonts are named on purpose, because a runner
without them substitutes and a substituted font is a real difference this metric would score (§3).

### 2. What computes the diff: nothing new at all — the §7 rule 4 justification in full

**No image dependency is added, anywhere.** Not to the workspace, not to CI, not to the script.
`cargo tree` is unchanged by this increment, as it was by 15-I; the shipped binaries' dependency
graph does not know this gate exists.

`AGENTS.md` §7 rule 4 asks four questions of a new dependency — is it maintained, is it pure Rust,
what does it add to binary size, where is the justification — and the plan pre-answers the shape of
the acceptable answer: an image-diff dependency «is a §7 rule 4 justification, written in the
increment that adds it — and **CI-only**». The precedent from 15-I is stronger than that: gate 1 is
a stdlib-only Python script driving an already-present system tool, and it is a better gate for it.
So this increment asked whether the perceptual diff could be had for **zero** dependencies rather
than for a justified one, and it can:

- The renderer is asked for **PGM**, not PNG. A binary PGM is a five-token ASCII header followed by
  one byte per pixel — `read_pgm` is fifteen lines of `pathlib` and `int()`. A PNG would need zlib
  (stdlib, fine) *and* a filter-reconstruction pass (not fine to hand-roll), which is precisely the
  step that makes people reach for Pillow. Asking the rasteriser for the format the stdlib can read
  is the whole trick. The plan's word was «PNG»; the thing it names is a rasterisation of the page,
  and PGM is the same raster with a header the standard library can read. If an artefact ever has to
  be *looked at* rather than measured, `pdftoppm -png` writes one in the same call.
- The metric is thirty lines of arithmetic over those bytes (`block_means`, `score`), using `sum()`
  over `bytes` slices so the inner loop runs at C speed. The whole corpus — 61 pages, two variants
  per deck — compares in **3 seconds**.

Candidates weighed and rejected, each for a reason that survives repetition:

| candidate | why not |
| --- | --- |
| Pillow / NumPy in CI | a real dependency, a real install, and a version that can change the metric under the gate. It buys a `.resize()` this needs thirty lines to do. |
| ImageMagick `compare -metric RMSE` / `DSSIM` | present here and on the runners, and it *would* have been the 15-I-shaped answer. Rejected because the metric would then be ImageMagick's: its RMSE is per-pixel, which anti-aliasing dominates (see §3), and `-fuzz`/`DSSIM` semantics differ between IM6 and IM7, so the threshold would mean different things on different machines. A threshold this project chose from measurements it can reprint is worth more than a metric it merely invokes. |
| an image crate in the workspace (`image`, `dssim`) | forbidden by the plan and by §3 before §7 rule 4 is even reached: it would land in a binary whose users convert documents and never diff pictures. |

### 3. The metric, and the threshold measured rather than guessed

The score is the **RMSE over the means of 8 × 8 pixel blocks**, on a 0..255 scale, printed as
`100 × RMSE / 255` — so `1.0` means the average block differs by 1 % of full scale.

The block mean is the perceptual part, and it is aimed squarely at the noise floor the increment was
told to characterise. Anti-aliasing, hinting and sub-pixel positioning move ink **within** a glyph:
inside a block, the mean barely moves. Anything that moves ink **between** blocks — a shape in the
wrong place, a picture that did not come back, a different string, a slide gone blank — changes
block means and is scored. A per-pixel RMSE would invert this: it is dominated by exactly the edge
pixels that are noise.

`python3 corpus/render_gate.py --self-test` prints the numbers, and they are the numbers the
threshold was chosen from. It runs with no renderer; the rasterised half needs `pdftoppm` only, and
builds its own PDFs — hand-written, because a PDF library would be a dependency for a self-test.

```
comparator self-test (block 8px at 100 dpi, threshold 2.5)
     0.0000  under  identical rasters
     0.0144  under  one pixel of 750000 changed
     8.1927  n/a    content shifted by 1 pixel          (synthetic hard-edged bars)
    28.3804  over   content shifted by 8 pixels
     4.7301  over   one line of twelve removed
    32.3932  over   a wholly different page
     0.0000  under  same PDF rendered twice
     1.7216  under  anti-aliasing on vs off
     1.8411  under  text shifted by half a point
     3.5511  n/a    text shifted by one point
     5.0775  n/a    text shifted by two points
     8.8076  over   text shifted by four points
     3.5490  over   one line of twelve gone
     8.9285  n/a    font substituted
          -  n/a    a page of another size: raster geometry changed: 1000x750 -> 1000x375
```

**The noise floor, stated precisely.** Two renders of *the same file* by *the same* LibreOffice on
*the same* machine are bit-identical: `0.0000`, and the gate always renders both sides inside one
job, so that is the operative floor. It is not, however, a floor anybody should build on: the moment
the runner image ships a different LibreOffice or a different font, the two sides of a comparison
still agree (both are re-rendered) but a *golden* would not. So the threshold is set against
deliberately pessimistic proxies for renderer drift, which is what the two `1.7`–`1.8` rows are:
anti-aliasing turned off entirely, and every glyph moved half a point. Font substitution — the other
half of the brief's question — scores **8.93**, an order of magnitude above them: this metric is
*sensitive* to substitution, which is why both renders must happen on one machine and why the CI job
pins `fonts-liberation`.

**The threshold is 2.5.** It sits 1.36× above the worst same-content pair measured (1.84) and 1.42×
below the smallest genuine content difference (3.55, one text line of twelve gone). There is no
wider gap available: this metric cannot cleanly separate «every glyph moved one point» (3.55) from
«a line of text disappeared» (3.55), and pretending otherwise by picking a number in between would
be a threshold chosen to look decisive. 2.5 is under both, so both are caught.

### 4. What is compared, and why not simply «the round trip»

This is where the increment repeats 15-I's central lesson. The pptx writer **preserves the package**:
for every deck in `corpus/pptx` the round trip is byte for byte the deck that was read (15-H,
[[69-phase-15-roundtrip]]). Rendering both sides of a byte-identical pair measures nothing, expensively.
So the gate does not:

- **Round trip** — the gate proper. Original against `write(read(deck))`, every page scored against
  the threshold. When the two packages have **the same bytes** the gate says so and skips the
  render. That is not an unmeasured skip: byte identity is strictly stronger than any render diff,
  it is already asserted by a repository test, and it is counted in its own column («identical by
  bytes») so no one can read the summary as «rendered and agreed». The day the writer produces
  different bytes — an edit, Phase 16, `apply_edits` — the pair is rendered and scored, with no
  change to the gate.
- **Generated** — the same DocMark with its `skeleton:` line removed, so the whole package is
  written on the embedded template (15-G, [[68-phase-15-template]]). This is the path that really
  produces new bytes today. Its render **cannot** be scored against the original: a different master
  and theme are entitled to look different, and a threshold over that comparison would be a number
  chosen to pass. What is asserted instead is what a render can honestly say: the deck has the
  **same number of pages**, and no page that had ink came out **blank**. A slide that silently
  renders empty is exactly the failure schema validation cannot see. The diff score is printed for
  the record and explicitly labelled «reported, not a verdict».

**A page count that changed is a failure of its own kind**, not a large score. When the two sides
have different page counts there is nothing to pair, so the gate says
`the page count changed, 2 -> 1. There is no score for this: the pages cannot be paired.` and fails.
The same applies one level down: two rasters of different sizes are reported as
`raster geometry changed: 1000x750 -> 1000x375` rather than scored, because a changed page geometry
is a fact, and a number computed across it would be a fiction.

**Which decks**: all seventeen of `corpus/pptx`, `.pptm` included; **which pages**: every page of
every deck, 61 in total.

### 5. Unavailable renderer: a skip locally, a failure in CI

The brief asked for this decision explicitly, and the two answers are different because the two
situations are.

**Locally it is a skip — with exit 3**, never 0. A contributor on a machine with no LibreOffice
should not be blocked, but the run must be impossible to mistake for a pass, so it prints a banner
naming what is missing, what to install, what was therefore *not* measured, and the sentence «This
is NOT a pass», then exits 3 — a code distinct from 0 (pass), 1 (a real diff) and 2 (measured
nothing that it should have measured).

**In CI it is a failure.** `--require-renderer`, which is what the workflow passes, turns the same
condition into exit 2 with the reason on stderr. The reasoning: that runner installs LibreOffice on
the line above. If it is not there, the install step broke — the gate is not inapplicable, it is
unrun, and a job that goes green having measured nothing is the precise failure this increment was
written against.

**«Missing» includes «installed and broken».** This machine's LibreOffice is on `PATH`, exits 0 for
`--version`, and prints a loader error instead of a version. Keying the skip on `shutil.which` alone
would have marched into the corpus and produced seventeen «not measured» notes and an exit 2 —
technically not a green tick, but a bad diagnosis of a fine machine. So the probe is what `soffice
--version` *says*, not whether it ran, and a broken install takes the same clean skip path with its
loader error quoted in the banner.

## What was verified here, and what was not

Split honestly, because half of this gate has never touched a real render.

**Executed, and green:**

- **The comparator, on fourteen pairs** — `--self-test`, exit 0, the table above. Six synthetic
  pairs built in memory (identical, one pixel of 750 000, shifted by 1 and 8 pixels, one bar of
  twelve gone, a wholly different page) and eight rasterised by `pdftoppm` from hand-written PDFs
  (the same file twice, anti-aliasing on/off, four sub-point and multi-point shifts, one text line
  removed, a substituted font). The self-test **asserts the ordering** the threshold depends on, so
  a future change to `BLOCK`, `DPI` or the metric that broke it would fail in CI.
- **The skip path, on this machine's real broken LibreOffice.** `python3 corpus/render_gate.py` →
  the banner, `reason: soffice is installed but does not run: … libreglo.so: cannot open shared
  object file`, exit **3**. With `--require-renderer` → the same banner plus the stderr line and exit
  **2**. Neither is 0.
- **The whole harness end to end, over all seventeen decks**, with a stand-in renderer (below):
  17 decks, 17 round trips identical by bytes, 61 pages compared, 0 failures, 0 not measured, exit 0,
  3.1 s. This exercises conversion, the byte-identity short circuit, the generated variant, the
  rasteriser, the PGM reader, the metric, the ink measure and the reporting.
- **Four negative controls**, each red and each naming the deck, the page and the number (§below).
- `cargo test --workspace --no-fail-fast`: **666 passed, 1 failed, 1 ignored** — the failure the
  known pre-existing `docx_roundtrip_is_idempotent` on `corpus/docx/footnotes.docx` (`88ff8e11`),
  untouched here. `clippy --workspace --all-targets -- -D warnings` clean, `cargo fmt --all` applied,
  `python3 corpus/generate.py --check` clean at 129 files, `python3 corpus/schema_gate.py` still
  **17 decks, 34 packages, 322 parts, 0 invalid, 0 not measured**. This increment adds no Rust and
  changes no corpus byte; those runs are the proof that it did not.

**Not verified when this increment closed, and it had to be said plainly** — the first and the third
of these have since changed, and the next section says exactly how far:

- **No corpus slide has ever been rendered by LibreOffice for this gate.** Not once. The
  `soffice --convert-to pdf` invocation, its `-env:UserInstallation` profile isolation, its
  behaviour on a `.pptm`, its timeout, its page order, the actual scores real renders produce — all
  unmeasured. The first genuine numbers will come from the first CI run, and **the threshold may
  have to move when they do**; if it does, the new number belongs here with its measurement, not in
  a silent commit.
- **The stand-in renderer is not evidence about LibreOffice.** To exercise the harness at all, the
  scratchpad held a fake `soffice` (a Python script that reads a package's `<a:t>` text and writes a
  PDF with one page per slide) placed ahead of the real one on `PATH`. It proves the gate's own
  plumbing and nothing whatsoever about how a deck looks. It lives in the scratchpad by design and
  is not committed: a fake renderer in the repository would eventually be mistaken for a fallback.
- **`--require-renderer` has never returned 0**, because that requires a working renderer.
- **The CI job has not run.** Its apt package list, in particular, is reasoned rather than observed:
  `libreoffice-impress` (not the full suite) plus `poppler-utils` and `fonts-liberation`.

## The loader fault, and the gate's first real run

Written after 15-J closed, and after 15-K closed the phase. Both of them recorded «LibreOffice is
broken on this machine» as a fact about the installation. It was a fact about the *dynamic loader*,
which is a smaller thing, and once it is named the fix is one environment variable.

### The diagnosis

LibreOffice **24.2.7.2 420(Build:2)** is installed and complete. Plain `soffice --version` exits
**127**:

```
/usr/lib/libreoffice/program/soffice.bin: error while loading shared libraries: libreglo.so:
cannot open shared object file: No such file or directory
```

and the file it cannot open **is there**: `/usr/lib/libreoffice/program/libreglo.so`, 89144 bytes,
owned by the `uno-libs-private` package, one of 146 `.so` files in that directory. Reinstalling
changed nothing, which is the point at which «the package is broken» stops being a hypothesis.

What is actually wrong is that nothing tells the loader to look in that directory. Two mechanisms
should, and on this machine neither does:

- `soffice.bin` carries `RUNPATH $ORIGIN` — objdump confirms it — and it is **not being honoured**:
  `ldd /usr/lib/libreoffice/program/soffice.bin` reports `libreglo.so => not found` and
  `libunoidllo.so => not found` for two files sitting in the very directory the binary lives in.
- The `/usr/lib/libreoffice/program/soffice` wrapper would paper over that, except that its
  `case $(uname -s)` block exports `LD_LIBRARY_PATH="$sd_prog…"` only under **OpenBSD** and under
  **NetBSD|DragonFly**. Linux falls through the `esac` with the variable untouched — the two
  branches are commented «a temporary hack until we can live with the default search paths», and
  this machine is a machine where we cannot.

It fails identically outside any sandbox, so it is not an artefact of how an agent runs commands.

**The workaround, verified:** name the directory explicitly.

```bash
LD_LIBRARY_PATH=/usr/lib/libreoffice/program soffice --headless --convert-to pdf …
```

With that set, `soffice --version` prints `LibreOffice 24.2.7.2 420(Build:2)` and exits 0. The
permanent local fix is `/etc/ld.so.conf.d/libreoffice.conf` containing
`/usr/lib/libreoffice/program`, followed by `ldconfig` — an owner action, root, and outside this
repository.

### The first real run, and what it proves

```
LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer
17 deck(s): 17 round trip(s) identical by bytes, 0 rendered, 61 page(s) compared,
0 failure(s), 0 not measured
```

exit **0**. That is the first time `--require-renderer` has returned 0, and the first time this
gate has driven a real LibreOffice over the corpus. It is also more subtle than «the gate is
green», and the subtlety is the whole value of writing it down.

**`0 rendered` is the round trips.** All seventeen took the byte-identity short circuit of §4: the
written package is the package that was read, so there is nothing for a render to disagree about,
and the gate says so per deck («identical package, byte for byte (a render can only agree)») rather
than claiming a render. Byte identity is strictly stronger than any diff score, so this is not a
gap — but it does mean the round-trip half of the gate still did not touch the renderer, on a
machine where the renderer now works.

**The 61 pages are the generated packages** — the skeleton-less path, every deck rewritten onto the
embedded default template. Every one of those pages reports `diff n/a (geometry)`: the template's
canvas is not the corpus deck's, the rasters therefore have different dimensions, and the
comparator refuses to score pages it cannot pair pixel for pixel. What ran on them is the ink check
alone, e.g.

```
ok  tables-simple.pptx [generated] page 1: ink 0.9882%, diff n/a (geometry) (reported, not a verdict)
ok  autofit-stale.pptx [generated] page 1: ink 5.6244%, diff n/a (geometry) (reported, not a verdict)
```

So, precisely: **LibreOffice opened and rendered every generated package without failing, the page
counts matched, and no page that should have carried ink came out blank.** That is a real result —
it is the first evidence that the packages this project authors end to end are loadable by a real
consumer at all, and it exercises the conversion invocation, the `-env:UserInstallation` profile
isolation, the `.pptm` path, the timeout, the page order, the PGM reader and the ink measure against
the actual renderer instead of a stand-in.

**What it does not do** is exercise the perceptual diff on real renders. Not one page in this run
produced a score. The threshold of **2.5 remains chosen from `--self-test` proxies and unvalidated
against real LibreOffice noise**, exactly as 15-J left it; nothing here has measured renderer drift,
font substitution or anti-aliasing on output this renderer produced. The first genuinely scored
pages will come the day the writer emits bytes that differ from its input — an edit, Phase 16,
`apply_edits` — or from a CI run on a runner whose LibreOffice or fonts differ. **The threshold may
still have to move**, and this run is not evidence that it will not.

### The skip path still probes honestly

Re-run **without** the variable, on the same machine, to be sure the fix did not quietly become a
requirement:

```
python3 corpus/render_gate.py
========================================================================
RENDER GATE SKIPPED — NOTHING WAS MEASURED
  reason:  soffice is installed but does not run: … libreglo.so: cannot open shared
           object file: No such file or directory
…
```

exit **3**. The gate still detects «installed and broken» as unavailable, still refuses to call it a
pass, and still quotes the loader error verbatim — which is what would let the next person diagnose
this in minutes rather than in an afternoon. The design decision of §5 survives contact with a
machine where the fault is real.

## The negative controls

A gate that has never been red is not known to work. Four, run against the stand-in renderer
instructed to corrupt one side of a pair; the round-trip ones with `identical()` forced to `False`
in a scratchpad copy of the script, because the corpus's byte-identical round trips otherwise never
reach the render path.

| control | what the gate said | exit |
| --- | --- | --- |
| a text line lost in the round-trip render | `FAIL basic-slides.pptx [round trip] page 1: perceptual diff 2.9538 over the threshold 2.5` (and page 2 at 2.8824), plus `worst page: … page 1 at 2.9538` | 1 |
| a page lost in the round-trip render | `FAIL basic-slides.pptx [round trip]: the page count changed, 2 -> 1. There is no score for this: the pages cannot be paired.` | 1 |
| a generated slide rendering blank | `FAIL basic-slides.pptx [generated] page 1: rendered blank (ink 0.9613% -> 0.0000%); diff 4.9302` | 1 |
| a generated deck losing a page | `FAIL basic-slides.pptx [generated]: 2 page(s) in the original, 1 written on the template` | 1 |

and the same harness with a faithful renderer scores `0.0000` on every page and exits 0, which is
the control that matters most: the red ones prove sensitivity, the green one proves the reds are not
an artefact of the setup.

The first row carries a finding worth keeping: losing one of a slide's two text lines scores
**2.95** — only 1.18× over the threshold. Corpus slides are sparse, and the metric averages over the
whole page including its white space, so the same loss on a dense slide would score higher and a
loss on an even sparser one could score under. That is the blind spot named below, measured rather
than suspected.

## What this gate cannot catch

Stated here so the next increment does not inherit an overclaim.

- **It is not the oracle, and LibreOffice is not the oracle** ([[61-phase-15-plan]]; spike P3 §2.4
  measured LibreOffice opening a deck with five dangling relationships without a murmur). Risk **P2**
  closes with the PowerPoint checklist of P3 §6.2, run by a human — that is 15-K, and nothing here
  moves it.
- **A small change is under the threshold.** One wrong word, a recoloured line, a slightly wrong font
  size, a picture 2 mm out of place: all score below 2.5 and pass. The gate catches *gross* visual
  regressions — a blank slide, text in the wrong shape, a lost picture, a dropped page.
- **Sparse pages dilute the score** (2.95 above). The obvious refinement — normalising over the
  inked blocks rather than the whole page — would raise sensitivity and also raise the noise, and it
  needs real LibreOffice noise measurements to be set safely. It is not this increment's.
- **A regression LibreOffice itself cannot show.** Anything LibreOffice renders wrong, or ignores,
  is invisible: a `p:transition`, animation timing, a font PowerPoint would substitute differently.
- **The generated variant has no threshold**, by design. Its score is printed, not judged.
- **Byte-identical round trips are not rendered.** If the byte comparison were ever wrong, this gate
  would inherit the error — but it is asserted independently by `pptx_roundtrip_is_idempotent`.
- **The gate is Ubuntu-only**, like gate 1, for the same reason: the writer's bytes are identical on
  the three platforms, so a second and third render would buy copies of one answer.

## Non-obvious decisions

1. **It is a script, not a `cargo test`** — the same argument as 15-I: it needs a built CLI and two
   external tools, and a render regression should read as a render regression in the checks list.
2. **The comparator self-test runs in CI as its own step**, before the gate. It needs no LibreOffice,
   so if the render half of this job ever became unavailable, CI would still be measuring the part of
   this increment that can be measured — and the threshold would still be defended by an assertion.
3. **`corpus/render_gate.py` imports four helpers from `corpus/schema_gate.py`** (`CORPUS`,
   `find_binary`, `run`, `without_skeleton`) rather than copying them. The two gates convert the same
   corpus with the same CLI and must agree about where it is; a second copy of `find_binary` would
   drift.
4. **Greyscale, not colour.** A colour regression that keeps luminance (red text turned green at the
   same brightness) is invisible to this gate. It is a real hole, priced at a third of the work and
   a third of the noise, and worth reopening if it ever bites.
5. **The profile directory is per-run** (`-env:UserInstallation=file://…` under the scratch dir), so
   two gates on one machine cannot fight over a LibreOffice profile lock, and a stale profile cannot
   change a render.

## Next

15-K: the P2 gate and the phase close — [[72-phase-15-close]]. The checklist could not be *run*
here (no PowerPoint, and at the time no working LibreOffice either — with the `LD_LIBRARY_PATH`
above, the LibreOffice half of that form is now doable locally, the PowerPoint half is not), so what
it delivers is everything around the human
step: three decks and a negative control, the fill-in form `docs/spikes/P2-powerpoint-gate.md`, risk
P2 left **open** with the state of the evidence written out in
`docs/technical-analysis-presentations.md` §7.1 and the closing wording prepared there, and the
phase's acceptance criteria settled — two true, three waiting on an application opening a file. This
gate's own missing half is carried forward there as item 2 of what Phase 15 leaves open — rewritten,
since the run above, from «no corpus slide has ever been rendered» to what is still true: the
threshold has never been scored against a real render.
