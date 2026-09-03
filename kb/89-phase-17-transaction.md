---
tags:
    - phase-17
    - apply-edits
    - transaction
    - dry-run
    - convert
    - model
---
# 89 — Phase 17 C and D: the transaction, and the report that makes a read unnecessary

Increments **17-C** and **17-D** of [[85-phase-17-plan]], implemented together against
[`docs/edit-operations.md`](../docs/edit-operations.md) and on top of 17-B's addressing
([[87-phase-17-etags]]). They are one commit on purpose: `dry_run` is the transaction stopped one
step short of the write, and splitting them would have meant building the apply engine twice.

What lands: **every operation of the set applies**, all-or-nothing, to a file that is replaced by
rename; `dry_run` runs everything except that rename and answers with a report that is *equal* to
the real one; the report carries the new etags. What does not land, on purpose: the CLI flag and
the MCP tool, which are 17-E's.

## The decision that shaped the increment: how application gets a mutable place

17-B's `resolve` hands back a **shared** borrow, deliberately, and the brief said so: «making
application possible is your problem to solve, and how you solve it is a decision to argue.»
Three ways existed.

1. **A second, mutable resolver.** `resolve_mut` returning a `PlaceMut`. It means two copies of
   every rule in §5 — what `doc` means in a single- and a multi-section document, which walk finds
   an id, which sequences an index addresses, which of the six typed failures a miss is. Two
   copies of a resolution policy drift, and on the day they drift the read path says `n12` is one
   node and the write path edits another. That is the failure ids exist to prevent, reached
   through the back door, and it would be invisible until it corrupted something.
2. **Rebuild the document functionally.** A `map`-shaped transformation producing a new tree. It
   costs a clone of the whole document per operation, and — worse — it discards identity by
   default: the `Addressing` counter, the `RawId` links into the raw sidecar and the `AssetId`
   links into the store survive only as long as somebody remembers to copy them.
3. **Record the route and replay it.** Resolution stays the single implementation 17-B wrote and
   pinned by test; it additionally reports the `Step` path it walked, and a small module replays
   that path against `&mut Document`.

**Chosen: (3).** `crates/docsai-model/src/addressing/target.rs` gained `Step`, `child_places` and
`resolve_path` (the old `resolve` and `children` are now thin wrappers, so no caller changed), and
`crates/docsai-model/src/addressing/mutate.rs` is the replay: `with_place_mut` for the operations
that change what a node *says*, `with_seq_mut` for the ones that change what a document
*contains*.

The cost is bounded and worth naming: one `Vec<Step>` per resolution — a dozen machine words for
a target six levels deep — and one `match` arm in `mutate.rs` per `(cursor, step)` pair the read
walk can produce, about forty of them. The gain is that **the mutable half carries no policy at
all**. It cannot decide what `doc/3` means; it can only walk where it was told, and when a step
and a cursor do not fit the answer is `None` and the caller refuses. A test pins the invariant
that matters (`mutate.rs::the_route_resolution_recorded_reaches_the_same_node_mutably`).

Two details fell out of the design rather than being imposed on it. `Step` fuses two hops in
`HeaderBlock`, `FooterBlock` and `NotesBlock`, because the middle of each pair — a `HeaderFooter`,
a slide's `Option<Vec<Block>>` — is not a `Place` and would otherwise need a cursor variant
nothing ever resolves to. And `child_places` returns a `Vec<Step>` per child rather than one
`Step`, for exactly one case: a single-section text document, where `doc/3` is the third *block*
and the section it lives in is transparent (§5.1). That child is two hops away; every other is
one.

## Ordering in a batch, and what it costs a caller

§5.3 is implemented literally: a target is resolved **at the moment its operation is applied**,
inside the loop, never before it. *Ids are stable, positions are not.*

- `[delete n7, replace_text n9]` — the second operation is untouched, whatever `n7` was and
  wherever it was.
- `[delete n7, replace_text doc/4]` — if `n7` was `doc/2`, `doc/4` now names what was `doc/5`.

**What it costs a caller who batches a delete before an index-path target: the index-path target
addresses a document the caller has not read.** It is not a trap that can be closed, only one that
can be lit: the convenient rule — resolve every target against the document as the request
arrived — would make `insert_after doc/1` followed by `insert_after doc/2` insert both fragments
in the same place, which is worse and less teachable. The cure is free and is in the set already:
every operation that creates a node returns its id, and an id target is immune. Both halves are a
test (`edit_ops.rs::an_id_target_is_immune_to_a_preceding_delete_and_an_index_path_is_not`).

The one place the rule is *not* enough is inside `move`, where the destination is named against
the document as it was before the target was detached. Re-resolving the destination after the
removal would silently retarget `move n8 after doc/5` onto a different node. So the destination's
recorded path is **adjusted** for the removal instead — the same sequence, a later index, one
less — which is arithmetic rather than a second guess.

## What the diff is, and what it measures on the wire

17-A reserved the answer twice (§13: «the full response shape» and «`dry_run`'s diff»). 17-D
answers: **the diff is the identity and the new state of everything the request touched, and
nothing else.** Created ids with their kinds, deleted targets, changed targets with their new
etags, and the warnings. Not the new content; not a textual diff.

Measured with the repository's own tokenizer over the framed JSON, the report for a two-operation
batch on a forty-slide deck is:

```json
{"changed":[{"id":"n10/1","etag":"74e8ca"},{"id":"n10","etag":"af08f3"}]}
```

**31 tokens.** The alternatives were priced before they were rejected: carrying each touched
node's new DocMark is ~200 tokens for the same batch *and is unbounded*, because a
`duplicate_slide` would return the whole slide and an `insert_after` of a table the whole table —
the response stops being a function of the request and becomes a function of the document. A
unified diff costs the document twice, which is the cost this entire phase exists to remove. And
the expensive half of the content option is a receipt for a sentence the caller composed:
`replace_text n12 "Q3 results"` returning «`n12` now reads "Q3 results"» tells the caller nothing
it did not just say.

What a caller genuinely does not know after a successful call is exactly two things — the **ids**
of what it created and the **etags** of what it changed — and both are in the report. That pair is
what makes plan task 5's «no confirmation read is needed» a property:
`edit_transaction.rs::the_report_carries_etags_a_reader_would_compute_so_no_confirmation_read_is_needed`
takes the etag out of one call's report, spends it as the `if_match` of the next, and then shows a
stale one being refused with `StaleEtag` and never overwriting.

The gate is a test with a number in it (`::the_report_for_a_realistic_batch_is_small_enough_to_be_free`,
120 tokens), so the day somebody adds a field to the report they are told what it cost.

### One amendment to §5.6, argued rather than smuggled

§5.6 writes `changed` as `[{ "id": "n12", "etag": "…" }]`, and §6.1 writes «returns
`changed: [target]`». Those coincide only when the target names a node that *has* an id — and
Phase 10 deliberately gives an ordinary paragraph none («a plain run of text is reached by
relative path instead, which keeps the id noise out of ordinary prose»). `replace_text doc/3` on a
text document has nothing but `doc/3` to report.

So `changed`'s `id` carries **the target in canonical form**, which for an addressable node *is*
its id — §5.6's example, unchanged — and otherwise is the index path the caller wrote. §6.1 read
literally. The row is true of the finished document either way, which is the property that
matters: the place named has that etag, so the caller's next `if_match` on the same target holds.
`docs/edit-operations.md` §13's entry is struck through with the reading spelled out.

## The transaction

1. Read into memory. Nothing on disk is touched.
2. Validate the whole list (§4's early pass): target grammar, `move`'s `after`/`before` pair,
   `set_props`'s closed key set, `replace_image`'s two forms, and the class refusals §6.6 and §6.8
   say come «before the target is even resolved».
3. Apply in order. A refusal at operation *k* returns, and the in-memory document is dropped —
   **there is no rollback to get wrong**, because the only thing mutated was a value the failure
   path discards. That is why the in-memory half needs no machinery: the transaction is the
   ownership.
4. Write to a temporary file **in the destination's own directory**, `fsync` it.
5. Rename over the original, then `fsync` the directory.

The temporary file is owned by a `tempfile::NamedTempFile`, so every failure from step 4 onwards
unlinks it on the way out. The original's permissions are copied onto the temporary file before
the rename, because the replacement inherits the temporary file's and a group-readable document
coming back private is a change nobody asked for.

### Where `rename` does not give what is wanted

Stated in the module header of `crates/docsai-convert/src/edit/mod.rs` as well as here, because a
guarantee is only worth having if its edges are written down.

- **POSIX, local filesystem.** `rename(2)` is atomic with respect to other processes: a reader
  sees the old file or the new one, never a partial one and never a missing one. This is the
  guarantee the construction rests on and it is real. The temporary file is in the destination's
  own directory precisely so that this holds: a rename across filesystems is not a rename, it is a
  copy followed by an unlink, and a copy can be interrupted halfway — which is the failure the
  whole thing exists to prevent. `/tmp` is very often a different filesystem.
- **Durability is not atomicity.** After a power loss a filesystem may replay the rename without
  the data behind it. The file is `fsync`ed before the rename and the directory after it. A
  directory `fsync` is unavailable on some platforms and a no-op on others; where it fails the
  edit is *not* failed for it, because refusing an otherwise finished edit over a hint the
  filesystem declined to take would be a false failure. So, plainly: **on a crash this guarantees
  you never read a half-written document; it does not guarantee that a completed edit survives a
  power cut on a filesystem that reorders the directory entry.**
- **Windows.** `std::fs::rename` is `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`. No observer
  sees the destination missing, but three things differ and none is fixable here. It **fails** if
  the destination is open by another process without `FILE_SHARE_DELETE` — a deck open in
  PowerPoint is the ordinary case, and the edit is refused with the operating system's own error
  rather than retried. The replacement's ACLs, alternate data streams and compression attributes
  are the temporary file's, not the original's, because the original's directory entry is
  discarded. Crash-atomicity on FAT and ReFS is weaker than on NTFS.
- **Network filesystems.** On NFS atomicity is the server's property, not the client's: a client
  retrying a rename after a lost reply can see `ENOENT` for an operation that succeeded, and
  whether `fsync` reaches stable storage depends on the export options. On SMB/CIFS a replacing
  rename is not guaranteed atomic at all. **No atomicity is claimed on a network filesystem**, and
  it cannot be detected cheaply enough to refuse.
- **A killed process.** Every ordinary failure unlinks the temporary file; `SIGKILL` runs no
  destructor, so a hard crash between steps 4 and 5 can leave a `.docsai-edit-*.tmp` beside the
  document. The name starts with a dot and says what it is, which is the most that can be done.

### The injected failure

Proven, not argued. A refusal a caller can trigger always happens *before* a byte is written, so
no request can exercise the write half — which is why `EditOptions::fault` exists, is
`#[doc(hidden)]`, and is set only by tests. Two points:

- `AfterApply` — the operations applied to the in-memory document and it is thrown away.
- `BeforeRename` — the **whole edited document is on disk, complete, under a temporary name**, and
  the process fails. This is the interesting one.

Evidence, in `crates/docsai-convert/tests/edit_transaction.rs`:

| test | what it shows |
|---|---|
| `a_batch_whose_third_operation_fails_leaves_the_file_byte_identical` | five operations, the third naming a node that is not there; the file's hash is unchanged and the refusal names index 2 |
| `a_refused_batch_leaves_every_package_family_byte_identical` | the same for `.docx`, `.odt`, `.xlsx`, `.ods`, `.pptx` — five writers, one property |
| `an_injected_failure_after_the_operations_applied_leaves_the_original_untouched` | hash unchanged, nothing left behind |
| `an_injected_failure_before_the_rename_leaves_the_original_untouched` | hash unchanged, the temporary file's parent **is** the destination's parent, and the file is gone afterwards |

## `dry_run` runs everything except step 5

The requirement is that a dry run answers with «the same report the real call would return». That
is only true if it does everything the real call does apart from replacing the file — **including
writing the document out**, because a writer's refusals and warnings (§8.1) are not knowable any
other way. So `dry_run` writes the temporary file, throws it away, and returns a report that is
equal, field for field and byte for byte on the wire, to the real one.

Nothing in the report can only be known after the rename, so nothing in the report differs, and
the test asserts **equality** over five formats rather than describing a similarity
(`::a_dry_run_and_the_real_call_return_the_same_report_and_only_one_writes`). Whether the call was
a dry run is on `EditOutcome`, outside the report, because a field that differed between the two
would make that equality impossible to state — and the caller knows what it asked for.

The price is stated rather than hidden: **a dry run needs write permission on the destination's
directory.** That is a feature. A dry run that passed and a real call that then failed on a
read-only directory would be a dry run that lied.

## Byte identity survives

- **A batch of zero operations leaves a preserved deck byte-identical**, over four fixtures
  including `raw-preserved.pptx` (`::an_empty_batch_leaves_a_preserved_deck_byte_identical`).
  15-B's rule reaches here unchanged.
- **An operation that changes nothing** — writing a shape's own text back over itself — is the
  same case from the other side, and also byte-identical: the writer compares against the part,
  not against the request.
- **An edit touches exactly the members it has to.** `::an_edit_touches_exactly_the_members_it_has_to`
  unzips both packages and compares every member: the member list is identical and exactly one
  member differs, the slide part holding the new title.

The hand demonstration, run outside the test suite on `corpus/pptx/basic-slides.pptx`:

```
target: n1/1
hash before      : 66e95c571cb848bb
failed batch: operation 1 (`replace_text`) refused: unknown target `n9999`: no node has the id `n9999`
hash after fail  : 66e95c571cb848bb
report           : changed: [{ id: "n1/1", etag: "74e8ca" }]
hash after edit  : 3bc4ff82304fdc8c

$ diff -rq orig new
Los archivos orig/ppt/slides/slide1.xml y new/ppt/slides/slide1.xml son distintos
```

and the single difference inside that part, character for character:

```
<a:p><a:r><a:rPr lang="es-ES"/><a:t>Informe trimestral</a:t></a:r></a:p>
<a:p><a:r><a:rPr lang="es-ES"/><a:t>Resultados del Q3</a:t></a:r></a:p>
```

One member, one element, and the run properties the paragraph had are still on it — which is
§6.1's «the run properties of the **first** run of the target are applied to the replacement»,
visible rather than asserted.

## The fragment is parsed *for* the sequence it is going into

§6.3's «there is no coercion» turned out to need enforcing rather than merely obeying. DocMark's
dialect selection is contextual, and a deck body whose content precedes any heading parses as *a
slide with no title* — the right reading for a whole document and a coercion for a fragment. So
`insert_after` on a slide with the content `Un párrafo suelto` is `ContentKindMismatch { expected:
"slides", got: "blocks" }` and not a slide nobody asked for
(`edit_ops.rs::a_fragment_of_the_wrong_kind_is_refused_and_names_both_sides`). The mirror rule
gives `Shapes` its parse for free: a shape fragment is the one that must *not* open a slide.

Three sequences take no fragment at all, and each refusal names §9 rather than saying «no»: a
table's cells (a column is not a node in the IR, §12.7), inline content (§12.9), and comments
(nothing in version 1 creates one, §6.10).

## Defects and limits found, and where they belong

1. **A package stores no node ids, so a structural edit renumbers them on the next read.** The
   readers do not put ids in the IR at all — the DocMark *writer* allocates them as it goes — so
   `edit_path` materialises them with `assign_ids` in document order before resolving anything,
   which is provably the same numbering a read hands out
   (`edit_ops.rs::ids_match_the_readers`, five formats). But because Phase 16 decided a node id is
   not a bookmark (commit `286a51f`), nothing writes them into a `.docx`/`.pptx`/`.xlsx`. The
   consequence is unavoidable and must be said: **after an operation that changes document order,
   the next read of that package numbers the nodes differently.** A `.dmk.md` does carry its ids
   and is unaffected. This does not weaken `if_match` inside one call, and it does not affect the
   ids in a report — which describe the document the call produced — but an agent holding `n7`
   across a `move` on a package is holding a position, not an identity. **Carried to 17-H**; it is
   a Phase 10/16 property, not something 17-C may change.
2. **`docs/edit-operations.md` §6.2 types `highlight` as `RRGGBB`; the IR holds a highlight
   *name*** (`yellow`, `cyan`), which is what OOXML's `w:highlight/@val` takes. The value is
   stored verbatim, so both spellings reach the writer and the writer does what it does. Not
   amended here, because it is a one-word typing error rather than a decision, and amending a
   normative document to match an implementation is the wrong direction. **Filed for 17-H.**
3. **The ODF writer normalises an all-zero crop rectangle away** on any write-back, an edit or a
   plain conversion alike. Two spellings of «not cropped» are not a change to the placement, so it
   is benign, but it means an ODT is not IR-identical across a no-op round trip. Observed at
   `crates/docsai-convert/tests/edit_ops.rs::replace_image_swaps_the_bytes_and_keeps_the_geometry`,
   which normalises it in the comparison and says why. Not fixed: it is an ODF writer question.
4. **`set_notes` cannot always remove a notes page**, and says so. Taking a notes part out of a
   preserved package is one of Phase 16's named refusals, and the writer emits it by name. The
   test accepts either outcome — the notes gone, or a warning that says they are not — and refuses
   silence, which is the rule that actually matters.
5. **`set_cell` writing a formula warns**, because §6.7's «whether the writers can state
   `fullCalcOnLoad`» is explicitly 17-E's question and today they cannot. The cached value is
   cleared, as specified, and the warning names the cell and says it will read as blank until the
   sheet recalculates. When 17-E teaches a writer to request recalculation, the warning goes: a
   warning that fires when nothing is lost is the false warning §0.5 forbids.

## What was refused

- **No CLI flag and no MCP tool.** 17-E owns that surface, and finds `docsai_convert::edit`'s
  `edit_path` and `apply_edits_in_memory` waiting with no logic left in the doors to write.
- **No selector support, no new operation, no relaxed refusal.** The set is closed and this
  increment implements it.
- **No new crate.** One new *dependency edge*: `base64`, already a workspace dependency compiled
  into the same binary through `docsai-mcp`, is now also a dependency of `docsai-convert`. No new
  third-party code enters the build. The justification is 17-E's own rule that «neither door may
  carry logic the other lacks»: §6.9's `content_base64` form has to be understood by the shared
  implementation, or the CLI's `--ops` file would accept a key the MCP tool honours and it does
  not.
- **No image decoder.** §6.9's aspect-ratio warning needs a picture's pixel dimensions, and a
  fifty-line header reader covers PNG, JPEG, GIF and BMP — the four formats an Office package
  embeds. Anything else answers `None`, which turns the warning **off**: a guess about a picture
  nobody could measure would be a false warning.

## Next

17-E — one implementation, two doors: `docsai edit <in> --ops <json>` and the `apply_edits` MCP
tool, over `docsai_convert::edit::edit_path`. Its schema is written for size from the first line,
and §6.7's recalculation question is answered there.
