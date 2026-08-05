# Spike P2 — DocMark-P, the presentation profile

**Risk mitigated**: P4 from `technical-analysis-presentations.md` §7 — *«DocMark-P becomes
unreadable (stops being Markdown, becomes XML with `:::`)»*.

**Question**: a slide is a canvas of positioned shapes and Markdown is a flow of blocks. Written
down, does a deck still read as Markdown, or does the profile turn into XML wearing `:::` fences?

**Date**: August 2026 · **Profile drafted**: DocMark 1.2 (sketch in
[`docmark-specification.md`](../docmark-specification.md) §11.2) · **Measured over**: the ten
fixtures of `corpus/pptx` (increment 12-A), eight of which carry a trait a deck body can show.

**Decision**: the profile is viable and stays Markdown, on one design change to the §11.2 sketch:
**placeholders are implicit, not containers**. The slide heading *is* the title placeholder and
the layout's primary body placeholder is written as ordinary Markdown blocks under it; a `:::`
container appears only where a slide holds something Markdown has no shape for. Measured at
`--fidelity standard` over the eight body-bearing fixtures, **11.4 % of what a plain Markdown
viewer prints is syntax** — and that number is **2.6 % once the free-shape fixture is set aside**,
which is the one file where the noise is deliberate.

The verdict risk P4 asks for, stated as a verdict: **a human can hand-edit `--fidelity standard`
DocMark-P.** For a deck of titles, bullets, tables, images and notes the output is
indistinguishable from Markdown a human would have typed from scratch — four of the eight
fixtures render with *zero* residue. The design failed for exactly one construct, free shapes,
and it fails there on purpose.

---

## 1. Method

The spike is a paper design, so the measurements are taken over hand-written DocMark-P: each
corpus fixture serialised by hand, as the writer would have to, in four variants. Hand-written on
purpose — the question is what a human reads, and a generator would have answered a different
question. The samples live outside the tree (§6): a rejected design is not versioned.

The four variants:

| Variant | What it is |
|---|---|
| **A** | The §11.2 sketch taken literally: every placeholder is a `::: {.ph type=… idx=…}` container, at full fidelity |
| **B** | Implicit placeholders: heading = title, body = blocks under it, containers only for the rest — at full fidelity |
| **S** | Variant B at `--fidelity standard`: no ids, no geometry, no raw payloads |
| **S2** | Variant S plus the three refinements §3.3 argues for (notes as a blockquote, `.ph` without an index, no image geometry) |

Three metrics.

1. **Residue — what a plain Markdown viewer prints that is not the document.** This is risk P4
   made countable. The viewer is real: `comrak` 0.39 with the GFM extensions and **no** container
   or attribute extension, which is what a wiki, a code host or an editor preview does. The
   rendered HTML is reduced to its visible text and every character of it is classed as *content*
   or *residue*, residue being a `:::` marker, an attribute block `{…}` or an `[]{.empty}` span
   that the viewer had no idea how to hide.

   Counted per character, not per line, and that detail is not a nicety: CommonMark's lazy
   continuation glues an opening fence, the paragraph under it and the closing fence into **one**
   rendered paragraph, so a line-level count would charge a whole bullet to the syntax that
   happened to precede it.

2. **Token cost**, with the tokenizer the repository already commits to — `tiktoken-rs`,
   `o200k_base`, the encoding `docsai tokens` uses (`docsai-convert::tokens::ENCODING`). Front
   matter and body counted separately, since a deck amortises the front matter over its slides and
   a fixture of one slide does not.

3. **Editability**, by inspection rather than measurement, and labelled as such: four edit tasks a
   human or an agent actually performs — change a title, add a bullet, add a slide, delete a
   shape — counting what has to be typed and naming what breaks when the edit is done naively.

## 2. Results

### 2.1 Residue and cost — the five fixtures every variant covers

`basic-slides`, `bullets-levels`, `notes-speaker`, `shapes-geometry`, `tables-simple`.

| Variant | Visible lines | Lines carrying residue | Visible chars | Residue chars | Residue | Tokens |
|---|---|---|---|---|---|---|
| A — containers, full | 45 | 34 | 1600 | 1076 | **67.2 %** | 899 |
| B — implicit, full | 33 | 17 | 1118 | 692 | **61.9 %** | 719 |
| S — implicit, standard | 31 | 8 | 558 | 132 | **23.7 %** | 298 |
| S2 — S + §3.3 refinements | 31 | 5 | 524 | 98 | **18.7 %** | 287 |

The same five with `shapes-geometry` set aside — the fixture whose entire content is objects with
no Markdown representation:

| Variant | Visible chars | Residue chars | Residue | Tokens |
|---|---|---|---|---|
| A | 1182 | 705 | **59.6 %** | 684 |
| B | 745 | 353 | **47.4 %** | 514 |
| S | 437 | 45 | **10.3 %** | 243 |
| S2 | 403 | 11 | **2.7 %** | 232 |

### 2.2 The recommended profile over all eight body-bearing fixtures

Variant S2, per fixture. `slide-order` and `placeholders-cascade` are excluded: their trait is
invisible in the body — one is about `p:sldIdLst`, the other about emitting *nothing* — and both
serialise to the same shape as `basic-slides`.

| Fixture | Visible lines | Residue chars | Residue | Tokens (front + body) |
|---|---|---|---|---|
| `basic-slides` | 5 | 0 | **0 %** | 47 (13 + 32) |
| `tables-simple` | 7 | 0 | **0 %** | 47 (13 + 32) |
| `notes-speaker` | 7 | 0 | **0 %** | 78 (13 + 63) |
| `images-anchored` | 2 | 0 | **0 %** | 40 (13 + 25) |
| `autofit-stale` | 9 | 0 | **0 %** | 110 (13 + 95) |
| `bullets-levels` | 8 | 11 | 12.5 % | 60 (13 + 45) |
| `placeholders-empty` | 3 | 11 | 18.3 % | 38 (13 + 23) |
| `shapes-geometry` | 4 | 87 | **71.9 %** | 55 (13 + 40) |
| **Total** | 45 | 109 | **11.4 %** | 475 |
| **Total without `shapes-geometry`** | 41 | 22 | **2.6 %** | 420 |

Five fixtures render with zero residue. What a viewer shows of `notes-speaker`, verbatim from the
probe:

```
content Resultados
content Crecimiento del 12 %
content Insistir en que el 12 % es interanual.
content No entrar en el desglose por región.
content Riesgos
content Dependencia de un proveedor
content Si preguntan por el proveedor, remitir al anexo.
```

And what it shows of `shapes-geometry`, the one file where the design does not hide:

```
content Formas libres
RESIDUE ::: {.shape geom=roundRect}
        Dentro del rectángulo
        :::
RESIDUE ::: {.shape geom=rightArrow}
        :::
RESIDUE ::: {.connector geom=line}
        :::
```

### 2.3 Editability

| Task | Variant A | Variant B / S2 |
|---|---|---|
| Change a slide title | Edit **two** places — the heading and the `.ph type=title` body — and the sketch does not say which wins | Edit the heading. One place, and it is the one a Markdown user would edit anyway |
| Add a bullet | Must land **inside** the fence. Typed after the closing `:::` it becomes a slide-level paragraph belonging to no placeholder, and the writer has to invent a text box or drop it | Add a list item. Nothing to know |
| Add a slide | **117 characters**, of which 99 are structure, and every one of `type`, `idx` and the id has to be right | **18 characters**: a heading and a bullet. Layout selected from content shape (analysis §6.6) |
| Delete a shape | Delete a visible stub — same in both, and the point of the stub |

### 2.4 What a viewer still leaks, and whose fault it is

Two of the three residue sources at `standard` are **not** pptx problems; they are DocMark 1.0
behaviours the presentation profile inherits.

- **Attribute blocks are visible in a plain viewer.** `## Título {#n1 .slide layout=L1}` renders
  as the literal text `Título {#n1 .slide layout=L1}`. This is why `--ids never` is already the
  default at the lossy levels (spec §11.1) and it is what makes `standard` clean; it is also why
  variants A and B, which are *full*-fidelity, cannot get below ~50 % residue. Pandoc, which does
  implement the attribute syntax, shows none of it.
- **`[]{.empty}`** for an empty paragraph — the DocMark 1.0 answer to the Phase 1 empty-paragraph
  bug — prints as `[]` in a plain viewer.

The third source, `::: {.shape …}`, is the profile's own and is deliberate.

## 3. Analysis

### 3.1 The sketch duplicates the title

Written out, §11.2's example says the title twice: once as `## Q3 results` and once inside
`::: {.ph type=title idx=0}`. In a plain viewer the reader sees the title of every slide twice; in
an editor the two can disagree and nothing in the sketch says which one the writer believes. That
is not a cosmetic defect — it is the shape of the whole design showing through. If the heading is
already the title, then the container that repeats it is pure structure, and structure that
carries no content is exactly what turns a format into XML with fences.

### 3.2 Implicit placeholders are what makes it Markdown

The cascade already gives the writer everything the implicit form needs. A layout declares which
placeholder is the title and which is the primary body — that is what a layout *is* — so a
document that names its layout (`layouts:` in the front matter, `layout=` on the slide at the
round-trip levels) resolves both without guessing. Implicitness here is not a heuristic: it is a
lookup in a catalogue the document carries.

What it buys is measured in §2.1: at full fidelity, **-20 % tokens and 340 fewer residue
characters** than the container form, with three of five fixtures losing their containers
entirely. What it costs is one wart, named in §5: a slide with no title has no heading to hang
its attributes on.

### 3.3 Three refinements, all of them removals

- **Speaker notes as a blockquote at `standard`.** A blockquote is native CommonMark, renders as a
  quote, and cannot collide with slide content: a placeholder has no blockquote construct in
  PresentationML, so `>` is free. `notes-speaker` goes from 28 residue characters to zero, and a
  reader sees notes set apart from the slide — which is what notes *are*. `::: {.notes}` stays at
  `full` and `agent`, where the id has to live somewhere.
- **`::: {.ph}` without the index at `standard`.** The container's job at a lossy level is to say
  *this is a second box*, not to say which. The index belongs to the levels that write back.
- **No image geometry at `standard`.** `{width=120px height=90px}` is 25 residue characters that a
  viewer cannot use — it draws the image at its own size regardless. Dropping it takes
  `images-anchored` to zero residue and 40 tokens. Spec §3.5's *«`width`/`height` are always
  required»* is a round-trip rule and stays in force at `full` and `agent`.

### 3.4 Free shapes: the exception, and why it is kept

`shapes-geometry` is 71.9 % residue at every level, and no refinement changes that, because there
is nothing to refine: an arrow and a connector have no content. The alternatives are to drop them
(a human then deletes shapes they were never told about, and the round trip silently loses the
deck's diagram) or to keep a visible stub. The analysis took this decision up front
(`technical-analysis-presentations.md` §2) and the measurement supports it rather than reopening
it: 87 characters of stub is what it costs to make a human aware of three objects they must not
edit by hand.

The honest framing for the format's reputation is that these are two different documents. A deck
of titles, bullets, tables and notes reads as Markdown (2.6 % residue). A deck that is a diagram
does not, and neither would any Markdown representation of it.

### 3.5 Readable units buy less here than in docx

The unit rule (spec §2) picks the first unit that is *exact* at the configured precision. Over the
corpus geometry that is `88px`, `784px`, `392px`, `160px`, `2.5cm` — and, for eight of the values,
raw `emu`: `2000000emu`, `1825625emu`, `1112520emu`, `1400000emu`. The reason is structural. Word
stores typography an author chose in points; PowerPoint stores positions an author *dragged*, and
a dragged position is not round in any human unit. Nothing to fix — the rule already falls back
correctly — but it does mean the `full` level of a deck will show more `emu` than a docx ever
does, which is a second argument for keeping geometry out of `standard`.

### 3.6 Autofit, in passing

`autofit-stale` serialises to eight clean bullets and 110 tokens: `a:normAutofit@fontScale` is
appearance, so `standard` drops it, exactly as analysis §5.4 decided. The fixture confirms that
the decision costs nothing in the body — the whole of risk P5 lives in the warning and in what
`full` writes back, not in what a human reads.

## 4. Decision

**DocMark-P is drafted as variant S2/B and risk P4 is closed as survivable.** The draft, in the
form Phase 14 will finalise:

````markdown
---
docmark: "1.2"
source-format: pptx
next-id: 128
layouts:
  L1: { name: "Title and Content", master: M1, title: 0, body: 1 }
skeleton: assets/_skeleton/deck-9f3a21c8.pptx
---

## Q3 results {#n1 .slide layout=L1}

- Revenue up 12 %
- Churn flat

::: {#n4 .ph idx=2 pos="88px,5200000emu" size="784px,1000000emu"}
1. One
2. Two
:::

::: {#n5 .shape geom=rightArrow name="Arrow 1" pos="3200000emu,2200000emu" size="1400000emu,500000emu" raw=r7}
:::

::: {#n6 .notes}
Open with the churn number, it is the one they ask about.
:::
````

Normative points of the draft, each one a change from or a confirmation of §11.2:

| # | Rule | Status vs §11.2 |
|---|---|---|
| 1 | A slide is an `##` heading with `.slide`; **the heading is the title placeholder**, and no container repeats it | **Changes** the sketch (§3.1) |
| 2 | The layout's primary body placeholder is **implicit**: its blocks are written at slide level | **Changes** the sketch (§3.2) |
| 3 | `layouts:` declares, per layout, which placeholder index is the title and which is the body — that is what makes 1 and 2 a lookup rather than a guess | **Adds** to the sketch |
| 4 | Every other placeholder is `::: {.ph idx=…}`; free shapes and connectors are `::: {.shape geom=…}` / `::: {.connector …}` with a `raw=` reference | Confirms |
| 5 | Notes are `::: {.notes}` at `full`/`agent` and a **blockquote** at `standard` | **Adds** to the sketch |
| 6 | `standard` writes no ids, no geometry, no raw payload, no image size, no `[]{.empty}`, and no layout catalogue — nothing in the body refers to one | **Adds** to the sketch |
| 7 | Geometry in readable units with `emu` as the exact fallback, at `full` and `agent` only | Confirms |
| 8 | A shape with no Markdown representation is always a **visible** stub, at every level including `standard` | Confirms |

Costs, for the record and for the CI budget gate (analysis §6.4): a slide of title + three bullets
costs **~25 tokens** at `standard` and **~40** at `agent`; the front matter is 13 tokens at
`standard`, 24 at `agent`, 49 at `full` for these fixtures and grows with the layout catalogue,
not with the deck.

### Risks this decision accepts

| Risk | Accepted because |
|---|---|
| A slide with no title placeholder has no heading to carry its attributes | Rare, and it degrades to `## {#n7 .slide layout=L3}` — an empty heading. Ugly, bounded, and the alternative is a container on every slide |
| Two slides with the same title are indistinguishable at `standard` | `standard` does not write back; `full` and `agent` carry ids |
| `standard` merges nothing, but it does *forget* which box a second placeholder was | The container survives, its index does not. A level that writes back is not what `standard` is |
| Free shapes remain noisy | §3.4 — the alternative is silent data loss |

## 5. Status of the risks

- **P4** — *DocMark-P becomes unreadable*: **mitigated, with a number attached**. 2.6 % residue at
  `standard` over the fixtures that are a document; 11.4 % including the one that is a diagram. The
  spec's own test — *«a plain Markdown viewer must show title + bullets + images per slide and
  nothing else»* — passes verbatim for five of eight fixtures and fails only where §2 of the
  analysis already said it would.
- **P3** — *freeform shapes fragment the effort*: unchanged, but now with a measured price for the
  stub decision (87 characters for three objects).
- **P5** — *autofit*: unchanged. Confirmed that dropping `fontScale` costs nothing at `standard`.
- **P1** — *the cascade*: **untouched by this spike**. The draft's rules 2 and 3 *depend* on
  resolving layout → master, so the reader work Phase 13 does is what makes the implicit form
  legal. Nothing here proves the cascade is cheap.

### Still not verified

- No DocMark-P has been produced by a program: there is no reader (Phase 13) and no serializer
  (Phase 14). Everything above was written by hand, which is the right method for a readability
  question and the wrong one for a determinism question. Serializer determinism (spec §8) over
  slides is unproven.
- The reverse direction is unproven too: that a hand-written `standard` deck with no attributes at
  all produces a valid `.pptx` (analysis §6.6, tolerant input) is a claim of the design, not a
  measurement.
- Residue is measured against one viewer, `comrak`. A viewer that implements Pandoc's attribute
  syntax shows less; one that renders `:::` differently shows the same or more.
- Charts (`c:chart`) and SmartArt have **no draft representation here** — their fixtures
  (`charts-embedded`, `smartart-fallback`) are not built yet. Both are raw-block cases by analysis
  §2, so the draft's rule 4 is expected to cover them, but that is expectation, not evidence.

## 6. Reproducing

The samples and the probe live in the session scratchpad, not in the tree. To rebuild them:

1. Write the four variants by hand from `corpus/generate.py`'s pptx section — the fixture bodies
   are readable as source there, which is the reason 12-A built the corpus that way. One directory
   per variant: `A/`, `B/`, `S/`, `S2/`.
2. Build the probe: a binary depending on `comrak = { version = "0.39", default-features = false }`
   and `tiktoken-rs = "0.12"` that, for each file, splits off the YAML front matter, renders the
   body with `markdown_to_html` (extensions: `table`, `strikethrough`, `autolink`), reduces the
   HTML to visible text one block per line, splits each line into content and residue characters
   (residue = `:::`, `{…}`, `[]{…}`), and counts tokens with `o200k_base_singleton()`.
3. `p2-probe <variant>/*.md` prints the per-fixture table of §2.2; `p2-probe --show <file>` prints
   the classified visible lines quoted in §2.2.

The character counts in §2.3 are `wc -m` over the text a human types to add one slide in each
variant.
