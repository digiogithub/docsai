---
tags:
    - phase-14
    - docmark
    - presentations
    - fidelity
    - testing
---
# 53 — Phase 14 G: `plain`, and the degradation rule as a test

Increment **14-G** of [[46-phase-14-plan]], after the objects of [[52-phase-14-objects]]. The
serialiser was finished in 14-F; this increment does not add output, it *measures* it. Spec §11.2
carried a design rule — «a plain Markdown viewer must show, per slide, title + bullets + images and
nothing else», with a number from spike P2 — that no test enforced and that had been true of
hand-written samples only.

## What changed

- `docsai-convert/tests/plain_residue.rs` (new, six tests): spike P2's residue probe, in the tree.
  Renders every deck of `corpus/pptx` through `comrak` with the GFM extensions and **no** container
  or attribute extension, classes every visible character as content or residue, and asserts the
  rule at `plain` and at `standard`.
- `docsai-docmark::deck_writer`: `layout=` now follows `addresses()`; `geom=rect` is written
  nowhere (`DEFAULT_PRESET`).
- `docsai-docmark::writer`: `image_geometry()` split into `measurements()` — the document-class
  predicate — and its image-specific use; `col-widths` now also asks it.
- `docs/docmark-specification.md` §11.2: the design rule states both halves and its enforcing test,
  with the measured numbers; the `layout=` and `geom=` rows corrected; `col-widths` added to the
  no-measurements-on-a-slide rule.

## Non-obvious decisions

1. **The `standard` rule is stated as *what* may leak, not as a percentage.** A budget alone lets
   an attribute creep back in as long as the file grows around it. The test keeps each residue
   *span* as text and demands it be a container fence or the `{.slide}` marker with only the keys
   §11.2 allows there — an id, a `layout=`, a `name=` or a measurement fails, whatever the ratio
   says. The percentage is kept too, as a second test, because a structural rule cannot see a
   regression that stays inside the rule.
2. **Three writer defects, found by measuring rather than by reading.** All three were invisible to
   the unit tests because those assert the bytes the writer produces, not what a reader sees:
   - `layout=L1` at `standard`, where rule 6 writes no `layouts:` catalogue. The body referred to
     something the front matter did not contain — residue for a viewer, a dangling name for the
     parser of 14-H. The spec's attribute table said «every level but `plain`» and rule 6 said the
     opposite; rule 6 is the normative one and is what the spike measured, so the table was the
     error. This is not §7 rule 2 («do not change the spec to pass a test»): the document
     contradicted itself and the contradiction was resolved towards the rule.
   - `col-widths=` on a slide table at `standard`. It is a measurement, and it was the only
     attribute left on the container — with it gone, `tables-simple` is bare GFM at `standard`,
     which is what P2 measured and what a hand-editor wants.
   - `geom=rect`. `rect` is the DrawingML default: every plain box says it, so it says nothing.
     Written nowhere now, for the same reason `type=body` already was, and dropped at *every* level
     rather than only the readable ones — a default is not information at `full` either, and the
     pptx writer of Phase 15 has to emit `rect` for a shape with no preset regardless.
3. **`measurements()` is the deck predicate, `image_geometry()` its first caller.** 14-F named the
   rule after the one thing that used it; a second caller made the real name obvious. What it says
   is a property of the document class: a slide at `standard` writes no measurement, a text
   document at `standard` writes all of them (§3.5).
4. **The measured residue at `standard` is *worse* than P2's, and the spec now says so.** 17 % over
   the corpus, 15 % over the decks that are documents, against P2's 11.4 % / 2.6 %. The whole
   difference is `{.slide}`, which rule 1 requires and which P2's hand-written samples omitted: 8
   characters per slide, and the entire residue of ten of the seventeen decks. Rewriting rule 1 to
   drop the marker would beat the number, and it is not this increment's call — `.slide` is what
   makes an `##` a slide for the parser of 14-H, and P4 is *mitigated* at 15 %, not breached. The
   honest form is a number in the spec next to the reason it is what it is.
5. **The budget is a corpus total, and the exceptions are named.** `shapes-geometry`,
   `smartart-fallback`, `charts-embedded` and `raw-preserved` are decks whose content *is* objects
   Markdown has no form for; rule 8 accepts their noise deliberately. They are listed one by one in
   `is_objects`, so a new deck lands in the strict bucket unless someone writes down why it should
   not.
6. **The probe lives in `docsai-convert`, not in `docsai-docmark`.** It needs the pptx reader and
   the serialiser at once, and `comrak` is already a dev-dependency there — §3's rule that format
   crates never depend on each other stays intact, and no dependency was added (§7 rule 4).
7. **Residue is counted per character.** CommonMark's lazy continuation glues an opening fence, the
   paragraph under it and the closing fence into one rendered paragraph, so a line-level count
   would charge a whole bullet to the syntax that happened to precede it. P2 made this point; the
   test inherits it and says so where it is implemented.

## What is deliberately not written

- **No parser** (14-H). This increment reads the output with a *Markdown* viewer, which is exactly
  the reader the rule is about; the DocMark parser is a different question.
- **No CLI path.** `docsai convert deck.pptx -o deck.dmk.md` still refuses (14-J lifts it), so the
  test drives `docsai_office::read_pptx` and `docsai_docmark::serialize` directly, with the same
  options the CLI would build.

## How it was verified

- `crates/docsai-convert/tests/plain_residue.rs`, six tests: zero residue at `plain` over all
  seventeen decks; no `:::` and no attribute block in the `plain` bytes; the named-syntax rule at
  `standard`; the corpus budgets; the exact `plain` body of `basic-slides`, `bullets-levels`,
  `images-anchored` and `tables-simple`; and the notes that `plain` drops, with one typed warning
  per notes page.
- Three expectations of [[51-phase-14-notes]] and [[52-phase-14-objects]] lost their `layout=L1` at
  `standard`. That is the defect, not a regression.
- `cargo test --workspace` 39 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## Next

**14-H — the parser**: front matter, `.slide` headings, the container classes, notes in both
syntaxes, line-level errors and tolerant input — a hand-written deck with no attributes at all has
to parse. Done, in [[54-phase-14-parser]].
