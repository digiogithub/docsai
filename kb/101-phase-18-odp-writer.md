---
tags:
    - phase-18
    - odp
    - odf
    - presentations
    - writer
    - skeleton
    - gates
---
# 101 — Phase 18 D: the `.odp` skeleton and writer

Increment 18-D of [[94-phase-18-plan]], on the reader [[98-phase-18-odp-reader]] and the
DocMark-P half [[100-phase-18-odp-docmark]]. `docsai-odf` now **writes** a `.odp`, and the
phase's first acceptance criterion is a test rather than a plan:

> odp round-trip with DocMark identity on the second pass over the odp corpus.

`docsai formats` says `odp: read yes, write yes`.

## The question the plan asked by name: is byte identity attainable?

**Yes, for all seventeen fixtures, and the `mimetype` rule costs nothing at all.**

ODF requires `mimetype` to be the *first* member of the archive and *stored uncompressed*, so
that a consumer can sniff the format from the first bytes of the file (ODF 1.3 §3.3). That is
the one rule OPC has no counterpart for, and it is the rule a rebuilt package has to remember.
`Package::write_to` does remember it — it special-cases `mimetype` — but it remembers it by
**rebuilding**, and a rebuilt archive is not the archive that was read: it alphabetises members
through a `BTreeMap`, re-deflates every one of them and stamps a fixed timestamp on each.
[[98-phase-18-odp-reader]]'s third finding predicted exactly this.

The writer therefore does what the pptx one does and works on the ZIP itself
([[62-phase-15-package]]): `reemit` copies each entry **raw**, in the original order, keeping its
compression method, its CRC, its timestamp and its extra fields, and substitutes bytes only for
the parts that changed. `mimetype` stays first and stays stored **because it was**, and there is
no rule about it written down anywhere in `crates/docsai-odf/src/odp/write.rs`. That is the whole
cost: zero lines and zero special cases, bought by not rebuilding.

Three ZIP fields had to be put back by hand afterwards, and each is a measurement rather than a
guess — the fixtures are written by Python's `zipfile` and the `zip` crate states these
differently:

| Field | Where | What the `zip` writer does |
|---|---|---|
| external file attributes | central directory | rebuilds them from the Unix mode with `S_IFREG` ORed in (`0o600` → `0o100600`) |
| version made by | central directory | derives it from the compression method |
| version needed to extract | central directory **and** local header | 1.0 for a stored entry, 2.0 for a deflated one |

The pptx writer already restores the first. The other two are ODF's own bill: `mimetype` is the
one **stored** member in the package, so it is the one entry whose "version needed" the writer
derives differently from every producer that wrote 2.0 across the board — the field moves on
exactly the member ODF cares most about. Repro before the fix:
`cmp corpus/odp/notes-speaker.odp <round trip>` → *«byte 5»*, which is the local header's version
field of `mimetype`.

A fourth difference is not a field but a member: `corpus/odp/charts-embedded.odp` carries an
`Object 1/` **directory entry** for its chart sub-document. The pptx re-emission drops directory
members on the argument that they carry no content and no relationship, and that is true and
still wrong here: an entry the reader ignores is an entry the file has, and dropping it makes an
untouched deck come back one member shorter. They are copied.

`crates/docsai-convert/tests/odp_write.rs` pins all of it:
`an_untouched_odp_comes_back_byte_for_byte` over the seventeen, and
`mimetype_is_the_first_member_and_stored_uncompressed` over the two rules by name.

## What `rebuilt_parts` holds for ODF, and why it is shorter

`SkeletonRef::rebuilt_parts` holds exactly **`content.xml`** — one name, whatever the deck, where
a pptx skeleton names one part per slide plus one per notes page (seventeen for
`corpus/pptx/forty-slides.pptx`). ODF puts every page in a single part, and that part is the only
one whose *content* becomes IR:

- **`styles.xml` is read and is not on the list.** The masters, the page layouts and the page size
  reach the IR as references and deltas — `Layout`'s placeholders carry no properties and
  `Master::theme` is `None` for every `.odp`, both on purpose — so the model could not restate the
  part if it tried. It is copied verbatim, always.
- **`meta.xml` is read and is not on the list**, for the same reason `ppt/presentation.xml` is
  absent from the pptx one: the IR holds facts stated in it, not its content.
- **`settings.xml`, `META-INF/manifest.xml`, `Pictures/*`, the chart sub-documents and a `Basic/`
  project** reach the IR not at all, or only as an asset id, and are copied.

The pressure 18-B warned about was to widen it, and the pressure never arrived, because the writer
splices. **`META-INF/manifest.xml` is deliberately not on the list and is still edited** when a
rewritten body brings a picture the package did not carry: a `<manifest:file-entry>` inserted
before the closing tag is an edit *inside* a part, not a regeneration of one — exactly what the
pptx writer does to a slide's `_rels` and to `[Content_Types].xml`, neither of which is on its list
either.

**The writer does not read the list, and says so.** `rebuilt_parts` does not survive a round trip
through DocMark — the parser resolves `skeleton:` and hands back an empty vector
(`deck_parser.rs:83`) — so a writer that consulted it would refuse every edit a person actually
made. It goes through the *same door* the reader went through, which is the pptx writer's own
argument (`pptx/skeleton.rs`) and the reason the two sides agree without a list to compare. The
list is the reader's honest statement of what it turned into IR.

## 15-B's rule, and the one place ODF cannot follow the pptx recipe

> Nothing is written unless it differs from what the part already says, compared in the model's
> units against the model's own reader, never against the part's bytes.

The pptx writer can compare *generated bytes against the part's bytes*, because it reproduces
PresentationML closely enough that an unedited body comes out identical. **ODF cannot**: an ODF
text body is written against automatic styles the writer has to invent, so its markup is legal and
equivalent without being identical, and byte-comparing it would report an edit on every body of
every unedited deck.

So the comparison is the plan's: `write_odp` re-reads the preserved package with
`odp::read_package` — the very function that produced the deck — and compares body against body.
Two refinements, each with a reason that is not an optimisation:

1. **The comparison is made on the *rendered* body, not on the two `Vec<Block>`.** A deck that has
   been through DocMark carries `NodeId`s on every paragraph that a package never states, so `==`
   would differ everywhere while saying the same thing. Rendering both through the same writer and
   comparing the strings is blind to everything the writer does not emit and to nothing it does.
   The same treatment reaches a **table** and a **picture** (`kinds_differ`), which also carry ids,
   and a **comment** (`comments_differ`), which is compared field by field.
2. **An absent placeholder index and index 0 are one identity.** 18-B gives the first frame of a
   `presentation:class` on a page no index, because that is what the file says; DocMark-P's rule 1
   writes the layout's primary body as the slide's implicit blocks and the parser hands it back at
   index 0. Treating them as different reported an edit on **every body of every deck** — measured,
   forty-one false warnings on the corpus before the fix.

The result is that the seventeen fixtures produce **no edit at all**, which is what byte identity
means from the inside. `odp::write::tests::a_deck_nobody_edited_produces_no_edit_at_all` asserts
both halves — no warning and no byte moved — with no DocMark hop at all, at the crate's own door.

### Splicing, concretely

`content.xml` is parsed with the reader's own `xml::Element`, whose nodes carry their **byte span**
in the source. A body that changed replaces the whole span of its `draw:text-box` with a start tag
rebuilt from the parse (an attribute value may legally contain `>`, so no start tag is ever found
by scanning), the blocks rendered by the `.odt` block writer, and the closing tag. The automatic
styles those blocks needed are spliced into `<office:automatic-styles>` after its last child, under
the prefix **`ds`** so a `dsP1` cannot collide with the package's own `P1`. Edits are applied
sorted and non-overlapping, and two rewrites of the same bytes are an error rather than a
last-writer-wins.

`crates/docsai-odf/src/odt/write.rs` gained the minimum to make that possible: `AutoStyles`,
`WriterCtx`, `write_blocks` and `guess_image_mime` are `pub(crate)`, `write_automatic_styles` is
split so its `<style:style>` elements can be had without their container, and `AutoStyles` learned
a name prefix that defaults to `""`. **No `.odt` or `.ods` output moved.**

## 18-B's four findings, closed

1. **«`rebuilt_parts` for ODF is shorter — resist widening it.»** One name, `content.xml`, and it
   did not grow. Above.
2. **«A raw fragment can come from `styles.xml`, and the corpus will not catch a splicer that
   assumes otherwise.»** Closed by construction and by a test. The writer never edits `styles.xml`
   at all, so a page transition declared there survives even a round trip that dropped it from
   DocMark. `a_transition_declared_in_styles_xml_survives_the_round_trip` builds the fixture 18-A
   does not have — `raw-preserved.odp` with its `dp-fade` drawing-page style moved into
   `styles.xml` — asserts the reader names `styles.xml` in `RawFragment::part`, and asserts the
   transition is still in the written package.
3. **«`Package::write_to` is not byte-identical; ODF must keep the original bytes and splice.»**
   Done, and measured. Above.
4. **«The `LayoutId`-names-the-pair question is the writer's to close.»** **Closed by keeping the
   layout id as the page-layout name alone, and the argument is the writer's round trip.** A
   `LayoutId` naming the `(master, page layout)` pair would remove 18-B's warning and buy nothing
   the writer can spend: this writer never rewrites a page's `draw:master-page-name` or its
   `presentation:presentation-page-layout-name` — they are two attributes of `draw:page` that
   the package states and the splice copies — so a deck with a layout reused under two masters
   comes back **byte for byte** with both pairings intact, whatever the catalogue collapsed. The
   loss is in what an *agent reading DocMark* is told, not in what the file keeps, and inventing a
   compound id would change the `layouts:` catalogue's keys in DocMark-P — a normative change to
   the profile, argued in a specification increment and not smuggled in by a writer. It stays
   carried, with its cost now measured at zero for the package and non-zero only for the reader's
   view.

## What is spliced, what is named, and what is refused

| The model changed | The writer |
|---|---|
| a placeholder's or text box's body | splices the `draw:text-box` |
| speaker notes | splices the `presentation:notes` frame's text box |
| a picture inside a rewritten body | adds the part and splices `META-INF/manifest.xml` |
| a table's cells, a picture, a chart, a preserved object | copies, `Degraded` naming the kind |
| a comment | copies, `Degraded` — no writer in this project writes an ODF annotation yet |
| a shape the package does not have | copies, `Degraded` — a `draw:frame` with a class, a style and a rectangle nobody stated |
| a shape the package has and the model does not | copies, `Degraded` — this writer removes nothing |
| a shape whose `presentation:class` disagrees | copies, `Degraded` — the class is what the cascade resolves through |
| a page added or removed | copies every page, `Degraded` naming both counts |
| a deck with no preserved package | **`WriteError::Invalid`**, by name |

The last row is a decision and not a gap. The pptx writer answers the same question with an
embedded template plus a warning; there is no ODF template in this tree, and inventing a master
page, a page layout and a presentation style family would be a design nobody chose, substituted
silently. `a_deck_with_no_preserved_package_is_refused_by_name` pins the refusal and the reason it
gives.

### Shapes pair by position, and that is load-bearing

When the counts agree, the *n*-th shape of the model is the *n*-th shape of the package: a deck's
shapes reach DocMark in reading order and come back in the order they were written. Pairing by
identity instead is **worse**, and the corpus proves it —
`corpus/odp/presentation-classes.odp`'s `subtitle` frame comes back from DocMark as a body, and an
identity walk hands its text to the `outline` frame beside it. Measured: it swapped the two
frames' text and the round trip stopped being byte-identical.

When the counts differ, position cannot be trusted, and the pairing is a greedy walk in document
order — strictly by identity first, then by kind for what is left — so a deleted shape leaves a
hole the caller is told about rather than a neighbour being rewritten with somebody else's text.

## The two gates, and what they now cover for `.odp`

**`schema_gate.py`.** 18-A's ODF arm validated the fixtures and its docstring said so, explicitly,
because there was no writer. That is closed: the arm now runs the OOXML arm's own procedure —
convert the deck to DocMark, retype every page title, write it back on the preserved package, and
validate every XML part whose bytes are new or changed, found by diffing so the set cannot drift.
**Every fixture's rewritten `content.xml` validates against the vendored ODF 1.3 RELAX NG.** The
fixtures are still validated too; the arm gained a pass rather than trading one. 452 → **469
parts** validated, 89 → **106** of them `.odp`, 0 invalid.

There is no `generated` variant, and the docstring says why: the writer refuses a deck with no
package, so there is nothing for a schema to look at.

**`render_gate.py`.** 18-A rendered the seventeen fixtures and compared no round trip, because
there was none. It now does exactly what the pptx arm does, on the same comparator and the same
threshold: convert, convert back, and compare — and because the writer preserves the package, all
seventeen come back **byte for byte** and are counted under "by bytes", which is strictly stronger
than any render diff and costs no renderer time. The gate's own summary line says so:
`21 deck(s) + 17 .odp: 38 round trip(s) identical by bytes, 17 rendered, 127 page(s) compared`.

## The hand demonstration

`corpus/odp/bullets-levels.odp` → DocMark → one bullet retyped
(`Segundo nivel` → `Segundo nivel revisado`) → `.odp`, then both packages unzipped and compared
member by member:

```
same member list: True
  SAME  mimetype               (47 -> 47 bytes)
  SAME  META-INF/manifest.xml  (632 -> 632 bytes)
  DIFF  content.xml            (4683 -> 4772 bytes)
  SAME  meta.xml               (1795 -> 1795 bytes)
  SAME  settings.xml           (1932 -> 1932 bytes)
  SAME  styles.xml             (5441 -> 5441 bytes)
```

Exactly one member moved, and inside it exactly one `draw:text-box` — the `outline` frame that
holds the edited list. Its three nesting levels and its fourth first-level item come back in the
same shape; the second frame's ordered list is not in the diff at all, because its bytes did not
change. Reading the written package back gives DocMark identical to the DocMark that was edited,
except for the `skeleton:` hash, which must move because the package's bytes did. `xmllint
--relaxng` validates the new `content.xml` directly, and LibreOffice opens the deck and draws
`Niveles de viñeta / Primer nivel / Segundo nivel revisado / Tercer nivel / Vuelve al primero /
1. Uno / 2. Dos` — the edit in place, the nesting intact.

## Defects found

1. **A `.odp`'s preserved package was named `deck-<hash>.pptx`. Fixed.**
   `crates/docsai-docmark/src/frontmatter.rs:401` hard-coded the extension, and its own docstring
   justified it with `.pptm` — which is the same package family, where ODF is not. The front matter
   of every `.odp` was stating something about a file that was not true. `skeleton_path` now takes
   the source format, `skeleton_hash` accepts every family's extension, and the seventeen
   `corpus/odp/*.expected.dmk.md` goldens say `.odp`. Repro before the fix:
   `docsai convert corpus/odp/basic-slides.odp -o /tmp/x.dmk.md && grep skeleton: /tmp/x.dmk.md`.
2. **DocMark-P loses an empty body placeholder, silently.** `corpus/odp/placeholders-empty.odp`
   has a `<draw:text-box></draw:text-box>` on an `outline` frame, which reads as an empty
   `Vec<Block>`; rule 1 consumes it as the slide's implicit body, writes nothing for it, and the
   parser hands back a deck with **two** shapes where the package has three. The `.pptx` fixture
   never hits it because its empty body carries one empty `a:p` and writes `[]{.empty}`. Not fixed:
   it is `crates/docsai-docmark/src/deck_writer.rs`, shared with `.pptx`, and moving it moves pptx
   goldens. It is no longer silent — the writer names it — and it is carried forward. Repro:
   `docsai convert corpus/odp/placeholders-empty.odp -o /tmp/e.dmk.md && docsai convert /tmp/e.dmk.md -o /tmp/e.odp --verbose`.
3. **DocMark-P turns an ODF `subtitle` placeholder into a body, silently.**
   `corpus/odp/presentation-classes.odp`'s layout `AL2T1` declares a title index and no body index,
   so rule 1 consumes the `subtitle` frame as the implicit body and it comes back
   `PhType::Body`. Same crate, same reason for not fixing it here, same treatment: the writer names
   it and the package keeps `presentation:class="subtitle"`.
4. **`crates/docsai-convert/tests/ppt_phase19.rs` writes into the corpus.** Running the workspace
   suite leaves `corpus/ppt/assets/_skeleton/*.pptx`, `corpus/ppt/assets/img-*.png` and
   `corpus/pptx/assets/_skeleton/deck-66e95c57.pptx` behind, and a second run of the same suite
   then fails on the directory it deleted. Phase 19's, uncommitted, untouched here; recorded
   because the files turn up in `jj status` and must not be committed by anybody.

## What was refused

- **Widening `rebuilt_parts`.** It is one name and the writer never needed a second.
- **Consulting `rebuilt_parts` in the writer.** It does not survive DocMark; the pptx writer refuses
  the same temptation for the same reason.
- **An embedded ODF template.** A deck with no package is an error, not a design nobody chose.
- **A compound `LayoutId`.** It changes the `layouts:` catalogue's keys in normative DocMark-P and
  buys nothing the writer can spend; the package keeps both pairings either way.
- **Fixing defects 2 and 3 in `docsai-docmark`.** Out of this increment's territory and they move
  `.pptx` goldens. Named, warned, carried.
- **Touching the residue budget, `LOOP_MAX_*` or `corpus/token-budget.md`.** None moved. The token
  budget is a `.pptx`/`.docx`/`.xlsx`/`.odt`/`.ods` measurement and the `skeleton:` line this
  increment added to the `.odp` front matter does not enter it.
- **Slide and shape addition and removal.** 15-E and 15-G's ODF counterparts. Named in a warning
  rather than half-written.

## Carried forward

1. **A page layout used under two master pages** still collapses to one `Layout` and still warns.
   Its cost to the *package* is now measured and is zero. The fix, if it is ever wanted, is a
   specification decision about `layouts:`.
2. **A slide or a shape the model added or removed is not written**, and neither is a table cell, a
   picture, a chart's data, an ODF annotation or a preserved object's payload. Each is a typed
   warning naming the construct.
3. **Defects 2 and 3 above**, both in the shared deck writer, both now visible in a warning.
4. **`presentation:settings` is carried by the skeleton**, which is what 18-B asked 18-D for: it is
   in `content.xml`, the writer never touches the bytes it lives in, and it comes back untouched.
   The IR still has no node for it and still refers to no fragment, which is the honest statement.
5. **`draw:rotation-angle`** stays carried from [[96-phase-18-corpus]] and
   [[98-phase-18-odp-reader]], untouched a third time.

## How it was verified

- `cargo test --workspace` — green **except** two targets that are Phase 19's uncommitted work in
  Phase 19's files: `docsai-convert --lib`'s `every_format_appears_exactly_once` (they added a
  tenth `SUPPORT` entry and left the assertion at 9) and `docsai-convert --test ppt_phase19`. Every
  other target passes, and `-p docsai-odf -p docsai-model -p docsai-docmark -p docsai-cli
  -p docsai-mcp` is green on its own.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean but for Phase 19's `tests/scratch_probe.rs`, left alone.
- `python3 corpus/generate.py --check` — 205 generated files.
- `python3 corpus/schema_gate.py` — 469 parts validated (106 `.odp`), 0 invalid, 0 not measured.
- `LD_LIBRARY_PATH=/usr/lib/libreoffice/program python3 corpus/render_gate.py --require-renderer` —
  38 round trips identical by bytes, 17 `.odp` rendered, 127 pages compared, 0 failures.
- The hand demonstration above.

## Next

18-E — the cross conversion. The `.odp` writer it will write a `.pptx` body into is a splicer into
a preserved package, so a `.pptx` trait that has no `draw:frame` to land in will be a
`Degraded` warning and not a new frame; that is the shape of the matrix's «drops are warned, never
silent».
