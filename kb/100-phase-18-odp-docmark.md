---
tags:
    - phase-18
    - odp
    - odf
    - presentations
    - docmark
    - goldens
---
# 100 — Phase 18 C: the `.odp` in DocMark

Increment 18-C of [[94-phase-18-plan]], on the reader [[98-phase-18-odp-reader]] built. The
seventeen `.odp` fixtures of [[96-phase-18-corpus]] now have `.expected.dmk.md` goldens beside
their `.expected.inspect.json`, produced by the **same** deck writer a `.pptx` goes through, and
each one satisfies the property [[55-phase-14-goldens]] established for decks:
`serialize(parse(md)) == md`, byte for byte.

## The claim, and the test that makes it one

The plan's word was *the same*: a deck is a deck, and the serialiser must not need to know which
package it came from. That is now a test rather than an intention —
`the_deck_body_does_not_depend_on_the_package_it_came_from`
(`crates/docsai-convert/tests/goldens.rs`) serialises every `.odp` twice, once claiming
`Format::Odp` and once claiming `Format::Pptx`, and asserts the **bodies** are identical. Only the
front matter may differ, because only the front matter states a fact about the package
(`source-format:`).

`deck_writer.rs` needed **no branch on the source format**, and gained none. What it did need is
below, and none of it is a branch on the dialect: one of the three is a branch on the *fragment*,
which is a different thing and the honest one.

### The one place a source format was branched on, and it was already wrong

`deck_parser::looks_like_deck` opened with `fm.source_format == Format::Pptx`. An `.odp` reached
the deck parser anyway — through the `layouts:` catalogue, or through the `.slide` scan — so the
goldens were green before the fix and would have stayed green after removing it. It is now
`fm.source_format.is_presentation()`, which is what the line meant to say: `Format` already
answers the question and the enumeration of one format was a fact about which reader existed in
Phase 14, not about DocMark.

## `PhType::Other`: what DocMark writes for `text` and for `page`

18-B left two ODF classes in `PhType::Other`. They do **not** get the same answer, and the
difference is the whole of the argument.

- **`text` is written `type=text`.** PresentationML has no keyword for ODF's second body class,
  so the ODF word is the only word there is, and `PhType::Other`'s docstring — «kept verbatim so
  the writer can put it back» — is exactly satisfied.
- **`page` is now read as `PhType::Other("sldImg")`,** not `Other("page")`. This reverses 18-B on
  one line of `crates/docsai-odf/src/odp/mod.rs::ph_type_of`, and the argument is 18-B's own.
  `page` and `sldImg` are one construct — the slide thumbnail on a notes page — that both
  dialects name. DocMark-P writes the placeholder type into a **single** field, `type=`, and
  18-B already refused to let one construct reach one field spelled two ways when it normalised
  `chart:class` to the PresentationML plot name, *«a bar chart that read as `bar` from one dialect
  and `barChart` from the other would fail that matrix on a difference that is not one»*. The same
  sentence with `page`/`sldImg` substituted is the same sentence. `PhType::as_str`'s own docstring
  says the value is «the PresentationML keyword», and `Other("page")` made that false.

  Nothing in the corpus reaches it: `presentation:class="page"` lives on a notes page's
  `draw:page-thumbnail`, which 18-B skips with an argued silence. So the change is pinned by a
  unit test (`the_two_unmodelled_classes_keep_the_word_the_dialects_share`) rather than by a
  golden, and it is recorded here because it moves a decision the previous increment wrote down.

**Can a hand-typed deck accidentally mean one?** It can type `type=text` and it will mean exactly
what an `.odp` means by it — which is the correct outcome, not an accident, because there is no
other thing `type=text` could denote: `text` is not in PresentationML's closed vocabulary. What
matters more is that the distinction cannot be *lost silently*: `PhType::Other` is neither
`is_title()` nor `is_body()`, so such a placeholder is never the implicit heading or the implicit
body of rule 1 — it always writes its own `::: {.ph type=…}` container, and a round trip through
DocMark cannot quietly turn it into a body. `placeholders-empty.odp` and `reading-order.odp` are
where that is visible.

## The specification, and why neither change is a version bump

Two edits to `docs/docmark-specification.md`, and **no normative rule was changed to make a
`.odp` fit**.

1. **§11.2, after the shape container attribute table.** The closed PresentationML vocabulary
   `type=` carries is now written down, `sldImg` included, together with the rule that a dialect
   naming a placeholder role PresentationML does not name writes **its own keyword** there, and
   the consequence that such a placeholder is never implicit. This documents behaviour that
   already existed; no syntax, key or attribute is added, and no document that parsed before
   fails to parse now. **Not a version bump.**
2. **§3.5, the image rules.** `title` now follows the fidelity level: the `title=` attribute at
   `full` and `agent`, and CommonMark's own title slot `![alt](path "title")` at `standard` and
   `plain`. This *is* a change to what the writer emits — see the defect below — but the form it
   emits is base CommonMark, valid in DocMark 1.0 and read by every viewer; a parser accepts both
   and the attribute wins when a document carries both. **Not a version bump**: nothing new is
   introduced beyond CommonMark, and no existing form is withdrawn.

## Three defects the `.odp` corpus surfaced, all of them format-independent, all fixed

The `.odp` corpus exercises constructs the `.pptx` one never did, and it found three things wrong
with code shared by both.

1. **A dropped raw-block named the wrong dialect.** `deck_writer` hard-coded
   `Warning::RawBlockDropped { format: "pml" }`, whose message reads *«source dialect is pml»*.
   For an `.odp`'s page transition or `anim:par` tree that is a **false warning**, which
   `AGENTS.md` §7 rule 3 forbids as firmly as a missing one. The dialect now comes off the
   `RawFragment` itself (`raw_dialect`), which is the only source that can be right for both
   packages — `odf` for an `.odp`, `ooxml` for a `.pptx`. Pinned by
   `a_dropped_raw_block_names_its_own_dialect`. Repro before the fix:
   `cargo run -p docsai-cli -- convert corpus/odp/raw-preserved.odp -o /tmp/x.dmk.md --verbose`.
2. **`list=` leaked into `standard`.** `writer.rs::render_list` wrote the numbering-definition
   name at every level but `plain`. It names what the *writer* puts back and says nothing a
   reader can use, so by §6 — and by §11.2 rule 6, where a viewer would print it — it belongs to
   the levels that write back. Now gated on `Fidelity::addresses()`. Visible in
   `corpus/odp/bullets-levels.odp` at `standard` as `1. Uno {list=L1}`; the same leak was in
   `corpus/docx/nested-lists.docx` and `corpus/odt/nested-lists.odt` and had never been measured,
   because only decks have a residue gate. Costs 31 tokens off the `standard` column of
   `corpus/token-budget.md`.
3. **An image title was residue at `standard` and a silent loss at `plain`.**
   `writer.rs::image_line` wrote `{title="…"}` at `standard` — an attribute block a plain viewer
   prints — and dropped the title entirely at `plain` with no warning at all. Both are now the
   CommonMark title slot, which is zero residue and zero loss; `parser.rs::split_link_title`
   reads it back. Repro before the fix:
   `cargo run -p docsai-cli -- convert corpus/docx/images-inline.docx --fidelity plain` — the
   `Figura 1` title was gone and nothing said so. Costs +8 tokens on the `plain` column, which is
   the price of no longer losing the text.

## The residue budget, with seventeen new decks in the corpus

**It did not move, and it must not be made to.** `plain_residue.rs::decks()` reads
`corpus/pptx` only, so nothing entered the 18.0 %/15.0 % measurement: it stands at **15.1 %**,
exactly as 14-G measured it.

The `.odp` corpus was measured anyway, and the number is **29.2 %** at `standard`. Folding it in
would fail. It is not folded in, and the reason is that the ratio would be measuring the wrong
thing:

| | visible | residue | ratio |
|---|---|---|---|
| `pptx/forty-slides` | 2 404 | 320 | 13.3 % |
| `odp/forty-slides` | 1 493 | **320** | 21.4 % |

**The syntax cost is identical — 320 characters, forty `{.slide}` markers — and only the prose
differs.** 18-A's fixtures are minimal one-trait files; 14-G's budget was calibrated on decks
carrying document-length bullets. A ratio over the `.odp` corpus measures how much text 18-A
wrote, not what DocMark-P costs, and a gate on it would be a gate on the wrong quantity.

What *is* comparable is the **kind** of syntax that leaks, and that is now gated:
`an_odp_deck_leaks_what_a_pptx_deck_leaks_and_nothing_else` asserts zero residue at `plain` and,
at `standard`, nothing but the slide marker of rule 1 and the containers of rules 4 and 8 — the
same predicate `standard_leaks_the_slide_marker_and_the_containers_and_nothing_else` applies to
the decks. It is what found defects 2 and 3 above.

## Every construct 18-B put in the IR either reaches DocMark or warns truly

| From 18-B | In DocMark | Warns |
|---|---|---|
| `draw:page` | `##` heading + `.slide` | — |
| placeholder frames | heading, implicit body, or `::: {.ph}` | furniture below `agent` |
| text boxes, presets | `::: {.shape geom=…}` | at `plain`, named |
| `draw:g` | `::: {.shape}` stub + `raw=` | — |
| `draw:custom-shape` | `::: {.shape geom=…}` stub + `raw=` | — |
| `draw:line` etc. | `::: {.connector}` stub + `raw=` | — |
| `draw:object-ole` | `::: {.ole}` stub + `raw=` | at `plain`, named |
| chart | `::: {.chart kind=…}` + title + the series table | — |
| picture | image line | — |
| `table:table` | GFM table in a `.table` container | if it has no rows |
| notes | `::: {.notes}` / blockquote | at `plain`, named |
| comments | `::: {.comment pos=…}` | below `agent`, named |
| transitions, `anim:` tree | **nothing** | `RawBlockDropped … dialect is odf` |

The last row is the only outright loss, it is the same loss a `.pptx` takes, and it is now warned
with the right dialect.

## What was refused

- **A branch on the source format in the deck writer.** None was added and none was needed.
- **Raising or reinterpreting the residue budget.** The 18.0 %/15.0 % gate is untouched and still
  a `.pptx` measurement; the `.odp` number is reported here rather than gated at a weaker value.
- **Making `corpus/generate.py` produce the new goldens.** It is off-limits this phase; the
  goldens come from `DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test goldens`, which
  is the ritual `AGENTS.md` §6 already prescribes, and `--check` counts them (183 → 200).
- **Any `.odp` writing, cross-conversion or skeleton work.** 18-D and 18-E.

## Carried forward

1. **`col-widths="0,0"` on an `.odp` slide table.** The ODF reader does not resolve
   `table:table-column` widths into the IR, so every column is `0`. It is the **existing ODF
   behaviour** — `corpus/odt/table-simple.expected.dmk.md` says `col-widths="0,0,0"` — not
   something this increment introduced, and fixing it moves `.odt` goldens.
2. **A `draw:line` reaches the IR with no geometry.** ODF states a connector as
   `svg:x1/y1/x2/y2` and the reader does not translate that into `Geometry`, so
   `shapes-geometry.odp`'s connector has no `pos=`/`size=`. Nothing is lost silently — the whole
   `draw:line` is preserved as its raw fragment and the raw-block warning names it — but the
   stub is less informative than its `.pptx` counterpart. `crates/docsai-odf/src/draw.rs`.
3. **A `draw:g`'s child paragraphs are concatenated into one label** —
   `raw-preserved.odp` shows `Dentro del grupoTambién dentro`. The `.pptx` reader does exactly
   the same on its own `raw-preserved` fixture, so this is shared behaviour of the raw-shape stub
   and not an ODF defect.
4. **`draw:rotation-angle`** stays carried from [[96-phase-18-corpus]] and [[98-phase-18-odp-reader]],
   untouched again.

## How it was verified

- `cargo test --workspace` — green.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.
- `python3 corpus/generate.py --check` — 200 generated files.
- `python3 corpus/schema_gate.py` — 21 decks, 42 packages, **452 parts** validated (89 `.odp`),
  0 invalid.
- `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer`
  — **21 decks byte-identical**, 17 `.odp` rendered, **127 pages** compared, 0 failures.

## Next

18-D — the `.odp` skeleton and writer.
