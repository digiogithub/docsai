---
tags:
    - phase-18
    - odp
    - odf
    - presentations
    - reader
    - ir
---
# 98 — Phase 18 B: the `.odp` reader, and the two mappings settled

Increment 18-B of [[94-phase-18-plan]], on top of the seventeen fixtures [[96-phase-18-corpus]]
built. A `.odp` now reads into `Document::Presentation` through `docsai-odf`, and every one of
those fixtures is pinned by an `.expected.inspect.json` golden whose **warning list is part of
the golden** — which is the only mechanical way to hold «never degrade silently, never warn
falsely» to account across an increment that adds a format.

`docsai-model` gained one thing: `Format::Odp`. The presentation IR itself gained nothing, which
was the constraint and is also the finding — see below.

## The first mapping: `presentation:class` versus `p:ph`

ODF names a frame's **role** out of a closed vocabulary; PresentationML names a **type and an
index**. The plan said the mapping is not a bijection and that the increment that first needs it
must say which side bends. It bends on the ODF side, in three separate decisions.

### Eleven classes map straight across, and two keep their own word

`title`, `subtitle`, `outline`, `object`, `chart`, `table`, `graphic`, `orgchart`, `header`,
`footer`, `date-time` and `page-number` all name something `PhType` already names. Two do not,
and they are stored as `PhType::Other` with their ODF keyword verbatim — which is exactly what
`Other`'s doc comment says it is for («a type this version does not model, kept verbatim so the
writer can put it back»):

- **`text`.** PresentationML has *one* body type where ODF has `outline` and `text`. Folding
  `text` into `PhType::Body` was the obvious move and is the wrong one twice over: it loses the
  distinction on every read, so no writer can put it back without the package; and it
  manufactures an index collision on every page that carries both, which is the very thing the
  rest of this section exists to avoid. `PhType::Other("text")` costs nothing and keeps the
  word.
- **`page`.** The slide thumbnail on a notes page. PresentationML spells it `sldImg`, which is
  not a `PhType` either, so there is nothing to map onto.

The reverse direction — 18-E writing a `.pptx` body into a `.odp` — will produce `outline` and
never `text`. That is correct: `outline` is what an ODF body placeholder is, and `text` is a
distinction only ODF makes.

### Two `outline` frames on one page: an ordinal, and a warning

`presentation-classes.odp` exists to force this, and here is what it gets. **The first frame of
a class on a page carries no index** — which is what the file says, and what keeps
`placeholders-cascade.odp`'s frames indistinguishable from a `.pptx` title that also carries
none. **Every later frame of the same class carries an ordinal**, 1, 2, 3 …, assigned from
document order.

The ordinal is not in the file. So it is warned, once per class per page, and the warning says
precisely what it is:

> ODF distinguishes frames of one `presentation:class` by document order alone, so every one
> after the first carries an index this reader assigned and the file does not state; no layout
> declares it, so it does not resolve against the cascade.

That last clause is the actual degradation, and it is why this is a `Degraded` warning and not a
note. A slide placeholder resolves against its layout by `(ph_type, idx)`. ODF's page layout
declares one `outline` rectangle; the second `outline` frame therefore points at a layout
placeholder that does not exist. Nothing is lost from the *page* — the frame's text, geometry
and style delta are all read — but its **identity in the cascade** is not resolvable, and the
plan's instruction was that the answer must not be a silent collision. Giving both frames
`idx: None` would have been exactly that: two placeholders the IR calls identical.

`reading-order.odp` fires the same warning for its three `text` frames, and it is the same true
statement about the same construct. It reads as noise only if one expects a positioned frame
with a `presentation:class` not to be a presentation shape. In ODF it is one.

### Reading order comes from the canvas, not from an index

The pptx reader ranks placeholders by type and then orders bodies by `p:ph@idx`, «because that
is the order the layout intended». ODF has no index to intend anything with, and the ordinal
above is this reader's own invention — ordering by it would be circular. So within a rank, the
`.odp` reader orders by **position**: row band (an eighth of an inch, the same constant
`pptx/order.rs` uses), then left to right. That is the policy the pptx reader already applies to
everything that is not a placeholder; applying it one rank wider is not a new idea, it is the
same idea where the index is absent.

`reading-order.odp` is the measurement. Its document order is `text(foot)`, `outline`,
`text(right)`, `title`, `text(left)`; it reads as **title, cuerpo, izquierda, derecha, pie**,
which is what the fixture's own frame labels say it should be. Ordering by the synthesised
ordinal instead would have produced `pie, cuerpo, derecha, izquierda` — the fixture's names are
what caught it.

The policy is admissible for the same reason the pptx one is: `Shape::z_index` keeps document
order, so sorting by it puts the page back exactly as written.

## The second mapping: master pages versus layout + master

The plan states the problem as «OOXML has two levels above a slide; ODF has one». **That
framing is wrong, and correcting it is what made this increment cheap.**

ODF has two references above a page. They are simply not nested:

| ODF | what it carries | IR |
|---|---|---|
| `draw:master-page-name` → `style:master-page` | background objects and the placeholder **styles** a page inherits, plus a `style:page-layout` for the page size | `Master` |
| `presentation:presentation-page-layout-name` → `style:presentation-page-layout` | a naked list of `presentation:placeholder` **rectangles**, with no style of its own | `Layout` |

That is not an approximation. `Layout`'s doc comment already reads «a named set of placeholder
positions inherited from a master», which is a description of `style:presentation-page-layout`
written before anybody looked at one. So `LayoutPlaceholder::props` is empty for every `.odp`
layout, on purpose: a layout that stated a font would be stating one the file does not.
`Master::theme` is `None` for every `.odp` master, also on purpose: ODF has no theme part, a
master page carries its own styles, and there is no reference to keep.

### What ODF has that the IR does not

The pair is chosen **per page**, not fixed. `AL1T19` under master `Default` and `AL1T19` under
master `Wide` are two different sets of resolved placeholders in ODF and one `Layout` in the IR,
because `Layout::master` is a single field. The reader catalogues the first pairing it meets and
warns by name on any second one. No fixture triggers it — every `.odp` in the corpus has one
master — so the warning is real and, today, unfired. It is carried forward rather than closed:
the honest fix is a `LayoutId` that names the pair, and that is a decision for the writer (18-D)
to take with the round trip in front of it, not for the reader to take alone.

**A `.odp` round trip therefore loses nothing at this level for a single-master deck**, which is
every deck LibreOffice writes for a normal presentation, and loses the master-to-layout binding
for a multi-master deck that reuses a page layout across masters.

### De-automatization, unchanged

`styles.rs` gained one map, `presentation: BTreeMap<String, PresentationStyle>`, filled by the
same `read_one_style` that already fills the paragraph, text, table and graphic families, from
the same `format::text_properties`. The two families it holds — `presentation` and
`drawing-page` — are the ones `.odt` and `.ods` never needed and which the whole `.odp` cascade
runs through. Resolution walks `style:parent-style-name` at the point of use, capped at 32 hops
because a name can point back at itself.

The measurable outcome is `placeholders-cascade.odp`: its title frame names `pr-title`, an
automatic style whose parent `Default-title` states 40 pt; the master's title placeholder names
`Default-title` directly. Resolved they are equal, so `font.minus(&inherited)` is empty and the
placeholder stores **no delta and no geometry** — which is what its docstring demanded and what
the reader's test asserts.

## What reads, what stubs, and what warns

Nothing in the reader is allowed to vanish. The table is the whole claim of the increment:

| ODF construct | becomes | warns |
|---|---|---|
| `draw:page` | `Slide` (document order; `draw:name` is a name, never an order) | — |
| `draw:frame` + `presentation:class` + `draw:text-box` | `ShapeKind::Placeholder` | on a class collision |
| `draw:frame` + `draw:text-box`, no class | `ShapeKind::TextBox` | — |
| `draw:frame` + `draw:image` | `ShapeKind::Picture` | if it also fills a class slot |
| `draw:frame` + `table:table` | `ShapeKind::Table` | — |
| `draw:frame` + `draw:object` → chart sub-document | `ShapeKind::Chart` + raw | raw-block |
| `draw:frame` + `draw:object-ole` | stub `ole` + raw | raw-block |
| `draw:g` | stub `shape` + raw, children included | raw-block |
| `draw:custom-shape` (`draw:enhanced-geometry`) | stub `shape` + raw, `draw:type` as the preset | raw-block |
| `draw:rect`, `draw:line`, `draw:ellipse`, … | stub `shape`/`connector` + raw | raw-block |
| `presentation:transition-*` on the page's drawing-page style | slide-level raw over the `style:style` | raw-block |
| `anim:par`/`anim:seq` tree | slide-level raw | raw-block |
| `presentation:notes` | `Slide::notes` | — |
| `officeooo:annotation` (and `office:annotation`) | `Comment`, anchored at its `svg:x`/`svg:y` | — |
| `draw:z-index` disagreeing with document order | document order kept | named |
| `Basic/` | ignored, never executed | `MacrosIgnored` |

Three of those deserve their reason written down.

**An OLE object is looked for before a picture.** The frame that holds a `draw:object-ole` also
holds the replacement bitmap the application drew for it. Reading that bitmap as the frame's
picture would replace an object with a thumbnail of it, quietly, and the object would be gone.
`ole-embedded.odp` is the fixture and `an_ole_object_is_not_read_as_its_replacement_bitmap` is
the test.

**`draw:z-index` is a second order the IR has one field for.** ODF states stacking explicitly
where OOXML leaves it implicit in `p:spTree` order, so the two can disagree — and in
`reading-order.odp` they do, deliberately. `Shape::z_index` keeps **document order**, because
that is what makes the reading-order policy reversible; the stated stacking order is then a real
loss, so it is compared and named rather than dropped. Note it is compared, not merely detected:
a page whose `draw:z-index` agrees with document order loses nothing and warns about nothing.

**The page transition is not on the page.** `presentation:transition-type` and its siblings sit
on the `style:style` of family `drawing-page` that the page names, which is a style and not a
shape. The raw fragment is therefore that `<style:style>` element, sliced from whichever part
declares it — `content.xml` for `raw-preserved.odp`. This is the one place where an `.odp` raw
fragment does not come out of `content.xml`'s body, and 18-D needs to know it (below).

### The chart, and the thing that cannot go stale

An ODF chart keeps a `table:table` named `local-table` **inside itself**. There is one copy of
the numbers, so `ChartRef::workbook` is `None` for every `.odp` chart and there is nothing to
compare, nothing to reconcile, and nothing to warn about — which is why `charts-stale-workbook`
has no `.odp` mirror and why its absence is a fact about the format rather than a gap.

The series ranges are **resolved rather than assumed**: `chart:values-cell-range-address` is
parsed into a column of `local-table` and read from there. Reading the second column because the
generator happens to put the first series in it would be reading the corpus, not the format.

`chart:class` is normalised to the PresentationML plot name — `chart:bar` → `barChart`.
`ChartRef::kind` is one field that DocMark-P writes and 18-E's matrix compares; a bar chart that
read as `bar` from one dialect and `barChart` from the other would fail that matrix on a
difference that is not one. A class with no counterpart keeps its ODF keyword verbatim.

## Two deliberate silences, argued rather than assumed

The brief allows an argued exception. There are exactly two.

1. **`draw:page-thumbnail` on a notes page.** It is the slide preview the application draws on
   the printed notes page. It holds no authored content and is regenerated from the notes
   master. The pptx reader skips its `sldImg` counterpart on identical grounds
   (`pptx/notes.rs`), and warning here and not there would be two readers disagreeing about the
   same object.
2. **`presentation:settings`.** A playback setting (`full-screen`), deck-level, and not document
   content. It has no IR node and no shape to hang a stub on — `Presentation::raw` is addressed
   by ids that slides and shapes hold, so a fragment nothing refers to would be a payload no
   consumer will ever write back, which is a silent loss wearing a preserved-markup costume.
   **18-D must carry it in the skeleton**, and until the skeleton exists this is the honest
   statement. `odp_goldens.rs` asserts the invariant that made this decision necessary: every
   fragment held is referred to, and every reference resolves.

## What 18-D needs from this increment

The skeleton's shape for ODF was explicitly left to 18-D. Four things reading taught, written
down here rather than built:

1. **The parts the IR actually holds are `content.xml` and `styles.xml`, and not all of either.**
   `styles.xml` is read for the masters, the page layouts, the page size and the presentation
   styles; nothing else in it reaches the IR. `meta.xml` reaches `DocumentMeta`. `settings.xml`
   reaches nothing. `Pictures/*` and the chart sub-documents reach the asset store and the
   `ChartRef` respectively. So the ODF equivalent of `SkeletonRef::rebuilt_parts` is a *shorter*
   list than the pptx one, and the pressure will be to widen it — resist, because
   «preserve everything, rebuild as the exception» is what makes byte identity reachable.
2. **A raw fragment can come from `styles.xml`.** `RawFragment::part` already carries the part
   name and this reader fills it honestly, but a splicer that assumes every fragment belongs to
   `content.xml` will silently drop a page transition declared in `styles.xml`. The corpus's own
   transition is in `content.xml`, so a test that only reads `raw-preserved.odp` will not catch
   it.
3. **`mimetype` must be first and stored.** `package.rs` already does this, and its round trip is
   asserted; what is not yet asserted anywhere is that an untouched `.odp` re-zips to the same
   bytes. `Package::write_to` normalises timestamps and re-orders parts into a `BTreeMap`, so a
   package written through it is **not** byte-identical to the one read — the pptx path solves
   this by keeping the original bytes and splicing, and ODF will have to as well.
4. **The `LayoutId` question above is the writer's to close.** A layout id that names the
   `(master, page layout)` pair removes the warning entirely; a layout id that names the page
   layout alone keeps it. Either is defensible and only the writer can measure the cost.

## Defects found

**One, in the existing corpus tooling, not fixed here because it is not this increment's.**
[[96-phase-18-corpus]] carried `draw:rotation-angle` forward as items 1 and 2 (an attribute ODF
does not have, written by `crates/docsai-odf/src/odt/write.rs:791` and read by
`crates/docsai-odf/src/styles.rs`). This increment touched `styles.rs` and deliberately left it
alone: correcting it moves an `.odt` golden and belongs with the fixture change, exactly as 18-A
argued. It stays carried.

Nothing else. The reader was written against the fixtures and the fixtures were written against
the grammar and the renderer, which is the whole point of doing the corpus first.

## Carried forward

1. **A page layout used under two master pages** collapses to one `Layout`, and the second
   pairing warns. Closes in 18-D or is accepted with the argument.
2. **`presentation:settings` has no home in the IR** and depends on the skeleton for 18-D.
3. **`.odp` is readable and not writable**, and `SUPPORT` says so. 18-D.
4. **`Presentation::styles` is empty for every `.odp`**: ODF's presentation styles are resolved
   into deltas and its graphic styles into the image model, so nothing reaches `StyleCatalog`.
   That is correct de-automatization and it means `inspect` reports `styles: 0` for a deck that
   plainly has styles. If that reads as a defect to a user, the fix is in `inspect`, not here.
5. **`stats.tables` and `stats.paragraphs` do not count a slide's table.** Inherited behaviour
   from `stats_from_presentation`, identical on the pptx side (`corpus/pptx/tables-simple`
   reports the same zero), and therefore not a regression this increment introduced — but it is
   now visible in seventeen more goldens.

## Next

18-C — the `.odp` in DocMark. `PhType::Other("text")` is the first thing it has to have an
opinion about: it is content, and DocMark-P's body rule keys on `PhType::is_body`.
