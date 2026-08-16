---
tags:
    - phase-14
    - docmark
    - specification
    - presentations
---
# 47 — Phase 14 A: DocMark 1.2 stops being a sketch

Increment **14-A** of [[46-phase-14-plan]], the first of the phase, on top of the closed pptx
reader ([[45-phase-13-inspect-inventory]]). It writes no serializer: it fixes the contract the
other ten increments are written against, which is what `AGENTS.md` §2 item 3 requires before any
change to the format.

## What changed

- `docs/docmark-specification.md` §11.2 rewritten as a **normative** section: the eight rules of
  spike P2 §4 ([[32-phase-12-spike-p2]]), the front-matter keys 1.2 adds (`layouts:`, `skeleton:`),
  the version rule, the compatibility contract, and an explicit «not yet implemented» for charts
  and SmartArt. The document header and the §11 preamble no longer call §11 a sketch.
- `docsai-model`: `DOCMARK_VERSION_PRESENTATION = "1.2"` and `DOCMARK_VERSIONS`, the list of
  versions this build parses.
- `docsai-docmark::frontmatter_parse`: the version check reads that list instead of testing
  `starts_with('1')`.

## Non-obvious decisions

1. **A presentation declares 1.2 whether or not it carries ids.** 1.1 works the other way — a
   document is 1.1 *because* it has ids, so the version describes what was actually written. That
   rule does not transfer: a deck's body uses `.slide` at every fidelity level, ids or none, so a
   deck at `standard` is still written in the 1.2 profile and saying `1.0` would misdescribe it.
   Hence a separate constant rather than a second use of `DOCMARK_VERSION_ADDRESSED`.
2. **The accepted version set is named, and an unknown one is now an error.** The old check
   accepted any `1.x` «softly», which was the right call while 1.1 and 1.2 were unwritten futures.
   With 1.2 real, that leniency means a 1.3 document — a profile this build does not know — parses
   as if it were 1.2 and silently loses whatever 1.3 added. A refusal naming the version is worse
   ergonomics and better behaviour, and it is the same argument as `AGENTS.md` §7 rule 3: no silent
   degradation. Backwards compatibility is untouched, since every bump is additive.
3. **A document that declares no version still parses.** Hand-written DocMark is a first-class
   input (analysis §6.6, tolerant input), and the profile is readable off the body. Tested, because
   it is the one case the stricter check could have broken.
4. **The sketch is superseded in place, not left beside the new text.** §11.2 used to show a deck
   with `::: {.ph type=title}` repeating the heading. Leaving that example in a normative section
   would give two answers to «where does the title live», and the whole reason P2 changed the
   design is that a plain viewer printed the title twice. The superseded points are named in the
   section's opening paragraph so the history is not lost.

## How it was verified

- `every_committed_version_parses` (1.0, 1.1, 1.2), `a_version_this_build_does_not_know_is_refused_by_name`
  (1.3 and 2.0, checking the message names the version), `a_document_that_declares_no_version_still_parses`.
- `cargo test --workspace` 33 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## What this increment deliberately does not do

No presentation serialises. `frontmatter::write` still emits nothing for a
`Document::Presentation` and `write_document` still refuses `pptx -> docmark` — the refusal 13-K
added, which stands until 14-J. Writing half a profile would produce a file that looks like a deck
and is not one.

## Next

**14-B — the deck's front matter**: `layouts:` (name, master, title index, body index) and
`skeleton:`, written at `full`/`agent`, absent at `standard`, parsed back into `LayoutCatalog` and
`SkeletonRef`.
