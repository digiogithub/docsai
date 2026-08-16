---
tags:
    - phase-14
    - docmark
    - presentations
    - frontmatter
---
# 48 — Phase 14 B: the deck's front matter

Increment **14-B** of [[46-phase-14-plan]], on top of the normative spec of [[47-phase-14-spec-1-2]].
The first bytes a presentation writes.

## What changed

- `docsai-docmark::frontmatter`: a `Document::Presentation` declares `docmark: "1.2"`, and at the
  levels that write back it writes `layouts:` and `skeleton:`. New `skeleton_path`, public for the
  same reason `raw::sidecar_path` is.
- `docsai-docmark::frontmatter_parse`: `FrontMatter` gains `layouts: LayoutCatalog` and
  `skeleton: Option<String>`; `read_layout` turns an entry into a `Layout` with real placeholders.
- `crates/docsai-docmark/tests/presentation_frontmatter.rs` (new): eight tests over the four
  fidelity levels.

## Non-obvious decisions

1. **The version is chosen by the document kind, not by the ids.** 1.1 works the other way round —
   a document is 1.1 *because* it carries ids — and copying that rule here would have written
   `docmark: "1.0"` on a deck at `standard`, which declares a profile whose body it does not use.
   The match is on `(doc, next_id)` so the deck arm wins first.
2. **`title:` and `body:` are `p:ph@idx` values, with PresentationML's own default.** A title
   placeholder carries no `idx` attribute and *is* index 0, which is why the spike's example reads
   `title: 0, body: 1`. The alternative — the placeholder's position in the layout's list — is a
   second numbering scheme that agrees with the first only by accident, and the two would diverge
   on the first layout whose placeholders are out of index order.
3. **The catalogue comes back as placeholders, not as two numbers.** `read_layout` builds
   `LayoutPlaceholder`s of the right `PhType`, so a parsed catalogue answers `layout.title()` the
   same way a read one does. Everything downstream asks the layout, never the YAML.
4. **`layouts:` and `skeleton:` are gated on `Fidelity::addresses()`, not `formatting()`.** The
   predicate that means «this level writes back» is the ids one: `full` and `agent`. `formatting()`
   is `full` and `standard`, which is the opposite pair for this purpose — `agent` needs the
   catalogue precisely because it is written back node by node, and `standard` must not have it
   (P2 rule 6).
5. **The skeleton is a path, and the naming lives in one function.** The serializer writes the
   reference and some later caller writes the file; if they disagreed a deck would point at a
   package nobody wrote — the argument that already put `sidecar_path` in one place. The extension
   is `.pptx` even for a deck that came from a `.pptm`, because 13-J reads a `.pptm` as its
   macro-free equivalent and the profile's `source-format` says `pptx` either way.
6. **The parser keeps the skeleton as text.** Turning it into a `SkeletonRef` needs the asset store
   and the base directory, which belong to the body parser — 14-H. Storing a half-resolved asset id
   here would be a second resolution path to keep in step with `resolve_asset_path`.

## What is deliberately not written

- **`p:sldSz`** (the canvas every geometry is relative to) has no front-matter key. The preserved
  skeleton holds `ppt/presentation.xml` verbatim, so the slide size survives a round trip through
  the package; a deck authored from scratch takes it from the default template (Phase 15). Adding a
  key would mean two sources for one number.
- **Masters** are written as a reference (`master: M1`) and nothing more. A master's placeholders
  are resolved cascade values, and the cascade lives in the skeleton. A `masters:` block would be
  the same data twice, and the second copy is the one that goes stale.
- **Nothing writes the skeleton file yet.** `skeleton_path` names it; the code that extracts assets
  next to a converted document does not know about it. That lands with the conversion path in 14-J,
  and until then no deck reaches a file at all — `write_document` still refuses `pptx -> docmark`.

## How it was verified

- `a_deck_declares_the_presentation_profile`, `the_layout_catalogue_says_which_placeholder_is_the_title_and_which_the_body`,
  `the_skeleton_is_a_path_a_reader_can_open`, `standard_carries_neither_catalogue_nor_skeleton`,
  `agent_carries_both_because_it_writes_back`, `plain_has_no_front_matter_at_all`,
  `a_deck_authored_from_scratch_has_no_skeleton_line`, `the_front_matter_is_byte_deterministic`.
- Parser side: `the_layout_catalogue_comes_back_as_placeholders` (including a layout with neither
  title nor body), `the_skeleton_is_read_as_a_path`,
  `a_document_with_no_layouts_carries_an_empty_catalogue`.
- `cargo test --workspace` 34 suites green, `clippy --all-targets -D warnings`, `fmt --check`.

## Next

**14-C — the slide**: `## Title {#n1 .slide layout=L1}`, the title placeholder consumed by the
heading, the primary body written as ordinary blocks under it, and the titleless slide degrading to
an empty heading.
