---
tags:
    - phase-14
    - docmark
    - presentations
    - goldens
    - corpus
---
# 55 — Phase 14 I: deck goldens and byte idempotence

Increment **14-I** of [[46-phase-14-plan]], after the parser of [[54-phase-14-parser]]. The parser
was proven structurally over the corpus: slides, shapes, kinds. This increment asks the stronger
question — does the *text* come back unchanged.

## What changed

- `corpus/pptx/<name>.expected.dmk.md`, seventeen of them: the deck corpus is pinned by its
  DocMark-P exactly as the other three corpora are pinned by theirs.
- `docsai-convert/tests/goldens.rs`: the pptx reader joins the corpus dispatch (extracted from
  `convert` into a `read` function), `the_pptx_corpus_matches_its_goldens` pins the text, and
  `serialize_parse_is_identity_on_docx_goldens` is generalised into
  `assert_serialize_parse_identity`, which the decks then reuse.
- `docsai-docmark::parser::find_skeleton`: a `skeleton:` reference resolves against the asset store
  when the package is not beside the document.
- `corpus/README.md`: the golden section names `pptx/` and says what its two golden kinds each pin.

## Non-obvious decisions

1. **The goldens are `.expected.dmk.md`, not `.expected.md`.** The plan wrote the shorter name, but
   the corpus already has a convention and a second one buys nothing: `goldens.rs` finds the file
   with one `golden_path`, and a deck that spelled its golden differently would need its own copy of
   every helper around it. The plan's name was shorthand for «the golden the other corpora have».
2. **One mechanism, not a second test file.** The decks go through `assert_goldens` and
   `DOCSAI_UPDATE_GOLDENS=1`, which is the ritual `AGENTS.md` §6 already describes. A separate deck
   golden runner would be a second place to forget to review a diff.
3. **The IR goldens of 13-K stay.** `<name>.expected.inspect.json` pins the IR seen from outside —
   layouts, shape counts, read-time warnings — and the DocMark golden pins the text a person edits.
   A change that alters one and not the other is exactly the change worth seeing.
4. **`skeleton:` resolves by content hash, and the corpus is why.** The serializer writes
   `assets/_skeleton/deck-<hash>.pptx`, where the hash is the head of the asset id; the corpus keeps
   no such file, so the parse dropped the skeleton and the re-serialisation lost the line — the
   round trip failed on every deck. `find_asset`'s existing fallback matches by *file name*, and the
   store holds the package under the name it had, not under the one the reference was built from.
   `find_skeleton` reverses `frontmatter::skeleton_path`, which is public for precisely this reason:
   the two sides have to agree on the name. The alternative was checking seventeen more copies of
   the corpus packages into git as `_skeleton/` blobs — the same bytes twice, to prove a name.
5. **The identity test seeds the store from the package.** Same as the docx goldens: image
   references point at files that only exist inside the companion package, so the reader runs first
   and the asset ids stay content-stable without loose media in the corpus.

## What is deliberately not written

- **`rebuilt_parts` does not survive a round trip.** The front matter writes the skeleton as a path
  and nothing else, so a parsed `SkeletonRef` comes back with an empty `rebuilt_parts`. Nothing
  reads it yet; the writer of Phase 15 is what decides whether it needs to be written down.
- **No CLI path** (14-J): the tests still drive `read_pptx`, `serialize` and `parse` directly.

## What was found and not fixed

`docx_roundtrip_is_idempotent` fails on `corpus/docx/footnotes.docx`: the second pass renumbers the
footnote reference ids (`#n2`/`#n4` become `#n4`/`#n5`, `next-id` 5 becomes 6). It is **not** this
increment's — it comes from `88ff8e11` *«fix: problems in docx TOC and headers-footers»*, which
touched `docx/body.rs` and `docx/write.rs`, and it reproduces with none of Phase 14 in the path.
`cargo fmt --check` also reports two files from that same commit, `docmark/src/escape.rs` and
`office/src/docx/body.rs`. Both left alone: §7 rule 1, and folding them into a Phase 14 commit would
hide a docx regression inside a presentation change.

## How it was verified

- `cargo test -p docsai-convert --test goldens`: the four corpus goldens, the deck goldens, and
  `serialize(parse(md)) == md` over the docx and the pptx goldens.
- `cargo test --workspace --no-fail-fast`: one failure, the pre-existing docx one above.
- `clippy --workspace --all-targets -D warnings` clean; `fmt --check` clean over every file this
  increment touched.

## Next

**14-J — the deck converts**: lift the `write_document` refusal so `docsai convert deck.pptx -o
deck.dmk.md` works end to end, with `--fidelity agent` at or under 15 % of `full`.
