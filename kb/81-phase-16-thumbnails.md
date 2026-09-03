---
tags:
    - phase-16
    - pptx
    - presentations
    - thumbnails
    - rendering
    - libreoffice
    - mcp
    - cli
    - naming
---
# 81 — Phase 16-G: the picture of a slide, and the word that named two things

Task 4 of [[75-phase-16-plan]], on top of the render gate of [[71-phase-15-render]] and the image
ladder of [[28-phase-11-mcp]]. What it adds is small — `--thumbnails` on `docsai convert`, a
`render_slides` tool on the MCP server — and what it had to settle first is not: **one word named
two different pictures**, and the place it named both was the tool schema an agent reads before it
decides which one to ask for.

## 1. The naming defect, and why an alias was the wrong fix

`ImagePolicy::Thumbnails` has existed since 11-H. It means *downscaled copies of the pictures a
document embeds*: `image::thumbnail` at 256 px over bytes that came out of the package, with the
invariant that the copy never costs more than the original. Nothing about a page.

This increment adds the other thing entirely: a raster of a **whole slide**, background and layout
and every shape on it, produced by an external renderer, existing nowhere in the package, and not
even reproducible from it — render the same deck on a machine with different fonts and the bytes
differ. In every consumer an agent has met, *that* is what «thumbnail» means.

Three options were weighed.

| option | why not / why |
| --- | --- |
| **leave both, and rely on prose to separate them** | Rejected. The two would sit in the same `tools/list` response, one as an argument value (`include_images=thumbnails`) and one as a tool name, and an agent choosing between them would be choosing on prose alone. The failure mode is silent and expensive: it asks for thumbnails, gets twelve 40×40 logos, and concludes the deck has nothing on it. |
| **rename the new one — `slide_previews`, `page_rasters`** | Rejected. It cedes the accurate word to the inaccurate use. `docs/technical-analysis-presentations.md:305` had already written «for pptx, `thumbnails` renders the whole slide to PNG» — the project's own design note used the word for *this*, which is evidence about what the word means to a reader, not about what the code happened to do. |
| **rename the rung: `thumbnails` → `downscaled`** | **Chosen.** It says what it does, it frees the accurate word for the accurate thing, and it is the *smaller* surface: one argument value on one tool, against a whole new tool's name, description and every mention of it. |

So: `include_images` = `none | refs | downscaled | full`, and the response fields it writes are
`downscaled_base64`, `downscaled_content_type`, `downscaled_width`, `downscaled_height`,
`downscaled_note`. The field names moved with the value on purpose — a half-rename leaves the old
word in the payload, which is exactly where an agent reads it.

### The old word is refused, not aliased

The tempting compromise is to keep accepting `thumbnails` as an alias for `downscaled`. It was
rejected, and this is the one decision here worth arguing at length, because it deliberately breaks
callers.

An alias assumes the caller meant the old thing. After this increment that assumption is unsafe in
a new way: a caller who writes `include_images=thumbnails` may equally have meant *«render me the
slides»* — which is now a thing this server does, under a different call. Aliasing hands them the
embedded logos and tells them nothing. That is the project's standing failure mode with a new
coat: an answer to a question that was not asked, delivered in silence (§7 rule 3).

So the value is refused with a message that names **both** answers:

```
`include_images=thumbnails` was renamed in Phase 16-G because it named two different things.
For downscaled copies of the pictures this document embeds, pass `downscaled`.
For a raster of a whole slide, call the `render_slides` tool.
```

A caller that breaks reads that once and is fixed. A caller that is silently wrong may never find
out. The CHANGELOG carries it as a breaking change, as the `refs` default change was carried in
Phase 11.

## 2. What produces the image: LibreOffice → PDF → `pdftoppm`, and no new dependency

The same two steps as `corpus/render_gate.py`, for the reason [[71-phase-15-render]] decision 1
already established and this increment re-confirmed by hand: **`soffice --convert-to png` on a
deck exports one page.** A `--thumbnails` built on it would render slide 1 of `forty-slides.pptx`
and report the other thirty-nine as drawn. PDF is the only LibreOffice export that carries the
whole deck, and `pdftoppm -png -r <dpi>` turns it into one raster per page.

The Impress *HTML* export was tried first, because it advertises one image per slide and would
have needed only one external tool. Run headless with no filter options it produces a text-only
HTML file — a `<h1>` and a `<ul>` per slide, no images at all. Measured, not assumed; that is why
it is written down.

`§7` rule 7 and the plan both forbid an image-encoding or PDF-rasterising crate without an
argument. **None was added, and none was needed**: `pdftoppm` writes the PNG, and the only thing
this code reads out of that PNG is its `IHDR` — width and height, eight bytes at a fixed offset
behind a fixed signature, fifteen lines with no decoder anywhere near them. `cargo tree` is
unchanged by this increment except for `tempfile` moving from `dev-dependencies` to
`dependencies` in `docsai-mcp`, and `tempfile` was already a runtime dependency of
`docsai-convert` next door, so the workspace's dependency *graph* did not grow at all.

The cost is a second system tool. `poppler-utils` joins LibreOffice on the list of things that are
**off the main path**: absent, they cost a warning and nothing else, and the message names which
one is missing and what it is for.

## 3. What a thumbnail is addressed by: the slide number, and the node id when it can be earned

Both, and the asymmetry between them is the point.

- **The slide number, 1-based, is the primary address.** It is the only thing the renderer can be
  held to: what comes back is page *n* of what LibreOffice drew, and calling it anything else would
  be asserting a correspondence nothing checked.
- **The node id is attached when it can be justified.** An agent that has called
  `outline_document` holds `n1`, `n3`, `n5`; without the id it would have to *assume* that page 2
  is slide 2. That assumption is right almost always, and wrong exactly where it matters — a
  hidden slide, which most exporters drop, or a deck whose `p:sldIdLst` was restated ([[66-phase-15-notes]]).

So the ids are computed — one extra read of the document, `docsai_convert::render::slide_ids` —
and then **used only if the deck's slide count equals the renderer's page count**. When they
disagree the ids are dropped for every slide and a note says so in the response:

> the deck has 12 slide(s) and the renderer produced 11 page(s); the thumbnails are numbered by
> page and carry no node ids, because pairing them by position would attach one slide's id to
> another slide's picture

That is not caution for its own sake: an id on the wrong picture is a *false* claim, which §7 rule
3 makes the same failure as a missing one. No corpus deck hides a slide, so this branch is
reasoned rather than observed, and that is recorded here rather than implied by a passing test.

One thing this cost: ids are handed out by the addressing pass, and on the conversion path the
*serializer* is what runs it — a document that is merely read carries `id: None` on every slide.
`slide_ids` therefore calls `assign_ids` explicitly, with the caller's own `IdPolicy`, so a
thumbnail's id is the id `outline_document` would give for the same options and not a second
numbering of the same deck. The MCP tool's first draft returned `node_id: null` for everything and
a test caught it.

## 4. Where the bytes go

Following [[59-mcp-e1-e5]]'s rule rather than inventing a second mechanism: **a file path is the
cheap answer and it is the one the tool is written to prefer.**

- **CLI.** `docsai convert deck.pptx -o deck.dmk.md --thumbnails` writes
  `<dir>/<name>-thumbnails/slide-NN.png`. The directory is `--thumbnails-dir` if given, else
  wherever the converted document goes (`--out-dir`, or `--output`'s parent); converting to stdout
  with `--thumbnails` and no directory is refused, because there is nowhere to put them.
  `--thumbnails-dpi` is 24..=300, default 96 — a 10 × 7.5 in slide becomes 960 × 720. The PNGs are
  **not** mixed into the assets directory and not added to `assets_written`: they are not media the
  document contains, and reporting them as such would be a small lie in a report other things read.
  All reporting goes to stderr, so `--json` still prints exactly the conversion report.
- **MCP.** `render_slides` with `output_dir` writes the files and answers with a receipt — path,
  size, dimensions, slide number, node id per slide. Without `output_dir` the PNGs come back
  base64 in rows shaped like `image_payloads`', capped at 2 MiB total: over that the call is
  refused with the three arguments that fix it (`output_dir`, `slides`, `dpi`), which is the
  `convert_to_markdown` inline-ceiling pattern verbatim. `slides` takes `2`, `1-4`, `1,3,7` —
  deliberately *not* the DocMark selector syntax, which addresses nodes rather than pages.

## 5. The absent-renderer path, and the false warning it would have been

The plan's clause: *«Skipped with a clean warning when LibreOffice is absent — and the warning must
not be false on a machine like this one.»* On this machine plain `soffice` prints

```
/usr/lib/libreoffice/program/soffice.bin: error while loading shared libraries: libreglo.so:
cannot open shared object file: No such file or directory
```

and **exits 0**. LibreOffice 24.2.7.2 is installed, complete, and perfectly able to render — the
fault is the dynamic loader, diagnosed in [[71-phase-15-render]] §«The loader fault».

`loffice::find_soffice` (`crates/docsai-convert/src/loffice.rs:51`) answers «is there an executable
file called soffice», which on this machine is **yes** — and every increment that trusted it would
either have reported the renderer present and then failed mid-corpus, or, with a `which`-shaped
check, reported «LibreOffice is not installed» on a machine where it is. Both are false statements.
So detection was hardened, in two steps, and the hardening is the substance of this increment:

1. **The probe is a measurement.** `probe_renderer` *runs* `soffice --headless --version` and
   requires the string `LibreOffice` in its own stdout before it counts it as present, then does
   the equivalent for `pdftoppm`. Exit status is not the check — this install exits 0 while
   failing. This is the same discipline as `render_gate.py:219`, in Rust.
2. **When the plain invocation fails, the installation's own directory is tried.** The parent of
   the *resolved* `soffice` — `/usr/bin/soffice` is a symlink into `/usr/lib/libreoffice/program`,
   so `canonicalize().parent()` finds it — goes on the child's `LD_LIBRARY_PATH` and the version
   probe is repeated. If that works, it is used for the real render too.

   **No machine-specific path is written down anywhere in the repository.** The directory is
   derived from wherever the binary turned out to be, on whatever machine is running; the
   documented workaround from [[71-phase-15-render]] is *applied*, not *hard-coded*.
   `DOCSAI_LIBREOFFICE_LIBPATH` overrides the derivation for an install that keeps its libraries
   elsewhere, and `DOCSAI_PDFTOPPM` names the rasteriser.

Measured on this machine, all four paths:

| invocation | what it says | exit |
| --- | --- | --- |
| plain (`soffice` broken by the loader) | `40 slide thumbnail(s) → …-thumbnails (96 dpi, LibreOffice 24.2.7.2 420(Build:2))` | 0 |
| `DOCSAI_LIBREOFFICE=/nope/soffice` | `slide thumbnails skipped — LibreOffice was not found: install it, or set DOCSAI_LIBREOFFICE to the soffice binary` | 1 |
| a stub that prints the loader error and exits 0 | `slide thumbnails skipped — \`…/fake-soffice\` is installed but does not run: soffice.bin: error while loading shared libraries: libreglo.so` | 1 |
| `DOCSAI_PDFTOPPM=/nope/pdftoppm` | `slide thumbnails skipped — pdftoppm was not found: install poppler-utils, or set DOCSAI_PDFTOPPM to the binary. LibreOffice alone exports one page of a deck, so the rasteriser is not optional` | 1 |

The first row is the one that matters: **on this machine, `--thumbnails` works with no environment
variable set at all**, and it did not before this increment. The other three are warnings that are
true, each naming the thing that is missing and how to supply it, and each exiting 1 — the user
asked for pictures and did not get them, and a `0` there would be the silent degradation §7 rule 3
forbids.

### The same fault, fixed on the path it was already breaking

`loffice::convert_to_docx` — the legacy `.doc` fallback of [[08-phase-5-legacy-doc]] — has the same
bug and always had it: on this machine `docsai convert old.doc --use-loffice require` failed with a
loader error, and the message blamed the conversion. It now resolves the binary through the same
`resolve_soffice` and applies the same library path. That is a defect **found and fixed**, not
scope creep: it is the same detection, in the same module, and the increment's brief was to harden
it honestly.

## 6. What this cost on the wire, and why it is worth it

`crates/docsai-mcp/tests/wire_cost.rs` budgets how much of an agent's context the tool schemas
cost, and refuses an update that inflates the suite by more than 5 %. A ninth tool inflates it by
**6.6 % (4 915 → 5 239 tokens)**, of which ~260 is the `tools/list` schema and the rest is the
handshake instructions repeated in every scenario. The budget was **not** lowered and the gate was
not weakened; the golden was regenerated with `DOCSAI_ACCEPT_TOKEN_INFLATION=1`, which is the
escape hatch the gate itself provides on condition the increase is argued. This is the argument.

First, the schema was squeezed before it was accepted: the tool description and all six argument
descriptions were rewritten shorter, which took the figure from 7.4 % to 6.6 %. What is left is
close to the floor for a tool with a document input, an output directory, a range and a resolution.

Second, what an agent gets for ~320 tokens a session is a capability it has no other way to reach.
Every other tool on this server answers in text: `outline_document` says a slide has three shapes,
`read_selection` says what they say. **None of them can say what the slide looks like** — whether
the chart overlaps the title, whether the text overflowed its placeholder, whether the picture
landed where it was meant to. For a vision model one 15 kB raster settles questions that no amount
of DocMark does, and the alternative on offer is not a cheaper tool, it is guessing.
`docs/technical-analysis-presentations.md:305` made this trade in advance: «for a vision model a
slide thumbnail is worth more than forty serialized shapes and costs less».

Third, the cost is bounded and mostly optional: the schema is paid once per session, and the images
themselves are paid **only when asked for**, and by default not through the context at all —
`output_dir` puts them on disk and returns a receipt.

## 7. Measured, by hand, on the corpus

```
docsai convert corpus/pptx/*.pptx --out-dir <tmp> --thumbnails
```

19 decks, **65 PNGs**, 1.8 MB in total, exit 0 — and 65 is exactly the page count
`corpus/render_gate.py` reports for the same decks, which is a small independent confirmation that
no deck lost a slide on the way through. `forty-slides.pptx` alone produces 40 files, 708 kB,
each 960 × 720 at 96 dpi, the largest 19 kB. `slide-07.png` was opened and looked at: it is
«Sección 7» with its two bullets, black on white, correctly the *seventh* slide and not the first —
which is the specific failure a `--convert-to png` implementation would have shipped.

## What this does not do

- **A page of a `.docx` is not a thumbnail here.** `render_slides` and `--thumbnails` refuse
  anything that is not a presentation. A page is a different unit with a different address, and
  answering the question about slides with an answer about pages would be worse than not answering.
  Reading a report's pages is a later increment's decision, not this one's.
- **The renderer is not the oracle.** Same caveat as [[71-phase-15-render]]: LibreOffice is not
  PowerPoint, and a thumbnail shows what LibreOffice drew. Risk **P2** is unchanged and still open.
- **Nothing is cached.** A second `render_slides` on the same deck spawns LibreOffice again. The
  16-H cache ([[79-phase-16-cache]]) covers parsed documents, not rendered pages; a render cache
  would need a key over the renderer and its fonts as well as the bytes, and it is not this
  increment's.
- **Hidden slides are reasoned about, not tested.** No corpus deck hides one (§3). The guard is
  written and its message is written; the fixture that would exercise it belongs to whoever adds a
  hidden-slide deck.

## Next

16-I — the phase closes.
