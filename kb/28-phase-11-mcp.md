# 28 — Phase 11 H: the primitives over MCP, and what an image costs

Increment H of [[19-phase-11-plan]], after [[27-phase-11-search]], and the one that closes
Phase 11. Plan task 8: *"MCP: expose `outline`, `read_selection`, `search_document`;
`include_images=none|refs|thumbnails|full` with `refs` as the new default (documented breaking
change of the MCP default, with a migration note in CHANGELOG)."*

## What it does

Three new tools — `outline_document`, `search_document`, `read_selection` — with the same
answers as [[17-phase-10-outline]], [[27-phase-11-search]] and [[25-phase-11-read-select]], over
`path` or `content_base64` like every other tool. Seven tools now, where Phase 7 left four.

```text
outline_document { path }                    → n1 heading, n12 heading, …   3.8 % of the document
search_document  { path, query: "riesgo" }   → s12 #n12 + select "#n12"      4.2 %
read_selection   { path, select: "#n12" }    → that node, with its etag      0.3 %
```

That sequence is the point of the whole phase, and until this increment none of it was reachable
from an agent: MCP had one way in and it was `convert_to_markdown`, the whole document.

## The two decisions

### An agent picks a tool by reading one paragraph

`get_info().instructions` is the only text a client sees before choosing, and it listed four
tools alphabetically. Adding three more to that list would have left the expensive one first.
The instructions now say what to reach for and in what order, and `convert_to_markdown` is
described as the whole document and the expensive path — in its own tool description too, since
some clients show only that. A primitive nothing routes to is a primitive nobody calls.

### `include_images` is a ladder of cost, not of fidelity

The default was every image inline as base64, which on any document holding a screenshot
outweighs every word around it. Four rungs — `none`, `refs` (default), `thumbnails`, `full` —
with two invariants that keep it honest:

- **The markdown never changes.** The body keeps its `![](assets/…)` links at every rung, so no
  rung is a lossy conversion, and a client that started cheap can ask for `full` later and get
  the same document. This is what makes the ladder a *payload* choice rather than a fourth
  fidelity level.
- **An image is always accounted for.** Every rung reports `image_count` and `image_bytes`,
  including `none`. "No images in the response" and "no images in the document" are different
  facts, and an agent that cannot tell them apart will conclude the wrong one.

Measured on `corpus/docx/images-inline.docx` with its `word/media/image1.png` replaced by a
1200 × 900 PNG (665 kB), which is what a real screenshot in a report looks like and what the
corpus, whose images are a few hundred bytes, cannot measure:

| Rung | Response |
|---|---|
| `full` (the old default) | 906 709 bytes |
| `thumbnails` | 83 956 (9.3 %) |
| `refs` (the new default) | 2 289 (0.25 %) |
| `none` | 2 061 |

## Smaller decisions, and why

- **An explicit `assets: "inline-base64"` still returns the bytes.** Only the *default* moved,
  so the break hits clients that never expressed a preference, and a client that asked for the
  old behaviour by name keeps it. That is the smallest break that achieves the point.
- **A thumbnail never costs more than the image it stands for.** A logo already smaller than the
  256 px box re-encodes to a *larger* PNG than the original, so the rung would charge more for
  less; when downscaling does not pay, the original is sent, with its own type and dimensions
  declared. The name has to be true or the rung is a trap.
- **An image that cannot be decoded says so on its row.** EMF, WMF and SVG are not raster and
  are not decoded here (§4.5 of the analysis explains what was rejected and why); the row comes
  back with `thumbnail_base64: null` and a sentence. The standing rule — a loss is reported, not
  hidden — applies to a payload just as it does to a conversion.
- **The decoder runs under a limit.** A few kB of PNG can declare 60 000 × 60 000 pixels, and
  this is a server pointed at whatever a client names. `image::Limits` caps the allocation at
  64 MB and any failure degrades that one image to a ref rather than failing the call.
- **`image` is a dependency of `docsai-mcp` alone**, with `default-features = false` and the
  four raster formats Office packages actually embed. The model, the format crates and the CLI
  never link it. Justification and rejected alternatives: `docs/technical-analysis.md` §4.5.
- **`read_selection` refuses the lossy levels, `search_document` allows them** — the same split
  the CLI makes, for the same reason: a selection with no ids cannot be written back, while text
  is findable wherever it is written.
- **One loader for all three primitives.** `outline_path`, `select_path` and `search_path` each
  had their own copy of "temp directory, read, build DocMark options"; they now share
  `service::with_scratch_document` and each gained a `*_input` sibling taking a `SourceInput`.
  Without that, base64 mode would have been a fourth copy, and three copies of a thing is where
  they start to disagree.

## How it was verified

`crates/docsai-mcp/tests/stdio_protocol.rs`, over a real client handshake rather than by calling
the functions:

- **the workflow holds end to end** — outline the report, search it, take a hit's selector, feed
  it to `read_selection`, and the DocMark that comes back contains the text the snippet quoted
  and declares `partial: true`. That join is what makes three tools one workflow, so it is
  checked, not assumed;
- **each answer is a fraction of the document** — the outline under a tenth, the search under a
  fifth, the selection under a tenth;
- **images are not sent unless asked for** — the default response carries no `content_base64`,
  and `full` returns the *same markdown* with the bytes attached.

Plus, in `src/`: the rungs' shapes and the `<=` invariant on thumbnail cost, the strict
reduction on a 1024 × 768 image, an undecodable image degrading with a reason, `none` still
counting what it did not send, the legacy `assets` argument still honoured, and the three
primitives answering identically over a path and over base64.

Gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check`, `python3 corpus/generate.py --check`. No golden moved and no
corpus fixture was added — the big-image measurement is taken on a document built from a corpus
one at measuring time, precisely so the token budget does not move for a number.

## Phase 11 is closed

Eight increments: raw sidecars ([[20-phase-11-raw-sidecar]]), `--fidelity agent`
([[21-phase-11-agent-fidelity]]), delta emission ([[22-phase-11-delta-emission]]), the attribute
dictionary ([[23-phase-11-attr-dictionary]]), readable units ([[24-phase-11-readable-units]]),
`read --select` ([[25-phase-11-read-select]]), `search` ([[27-phase-11-search]]) and this one.

Next is **Phase 12 — presentation spikes**, a different kind of work: `.pptx` reading, the
skeleton, and the deck corpus. Start at [[11-plan-v2-onramp]] before touching it.

The one thing Phase 11 leaves open on purpose, and it is written down in both
[[27-phase-11-search]] and the plan: a search hit on unaddressed prose gives a relative address
(`n12.b2`) that nothing can read back yet. Relative-path *selection* is machinery Phase 17
(patch editing) has to build anyway, and faking an address in the meantime would have been the
silent wrongness this project refuses.
