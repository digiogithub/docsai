# The docsai edit operation set — version 1

**Status: normative.** This document defines the closed, versioned set of operations that
`docsai edit` (CLI) and `apply_edits` (MCP) accept, as of plan v2 Phase 17 task 1
([`development-plan-v2.md`](development-plan-v2.md) §Phase 17). It is normative in the same way
[`docmark-specification.md`](docmark-specification.md) is: an implementation that disagrees with
it is wrong, and changing it is a specification decision with a version bump attached, not a way
to make a test pass.

Implementation began with 17-B (target resolution and the etag precondition,
[`kb/87-phase-17-etags.md`](../kb/87-phase-17-etags.md)) and every operation applies as of 17-C
and 17-D ([`kb/89-phase-17-transaction.md`](../kb/89-phase-17-transaction.md)), which added the
transaction, `dry_run` and the change report. **Both doors are open as of 17-E**
([`kb/90-phase-17-doors.md`](../kb/90-phase-17-doors.md)): `docsai edit <in> --ops <json>` and the
`apply_edits` MCP tool, over one function, with the equality asserted in
`crates/docsai-cli/tests/two_doors.rs` rather than intended.
Phase 17-A was the specification half of task 1 on purpose — the
same discipline that produced §11.3 before a line of comment code, and that let Phase 14 write a
parser that worked on the first run. The increments that implement it are 17-B (etags and the
precondition), 17-C (the transaction), 17-D (`dry_run` and the report) and 17-E (the two doors).
Where a decision belongs to one of those, this document **reserves its place** and says so, so
that the implementing increment implements rather than redesigns.

## 0. Principles

These are the reasons the rest of the document says what it says. When a later section looks
arbitrary, it is discharging one of these.

1. **An operation is a function on the IR.** Not a text substitution on DocMark and not a splice
   into a package. The plan requires this («every operation must be defined against the IR and
   proven in round-trip») and it is why the same operation means the same thing on a `.docx` and
   on an `.odt`: they are the same tree.
2. **The set is closed.** Anything the set cannot express is done with `read --select` and a
   rewrite (§9). A closed set is a boundary and not a wall only if the way around it is stated,
   which is §9's job.
3. **An id is an identity.** Phase 10's addressing already guarantees ids are never renumbered on
   insertion and never reused after deletion
   ([`crates/docsai-model/src/addressing/mod.rs`](../crates/docsai-model/src/addressing/mod.rs)).
   Every rule in §5 follows from that rather than restating it.
4. **A refusal is typed and names both sides.** «wrong kind» is not a message; `set_cell` takes a
   sheet and `n7` is a paragraph, and both halves are in the error.
5. **Nothing degrades silently and nothing warns falsely** (AGENTS.md §7 rule 3). An operation
   that loses something emits a `Warning`; an operation whose specified behaviour merely surprises
   somebody does not.
6. **Every field is paid on the wire by every caller.** §10 measures what the shapes below cost.
   A parameter that could be a default is a default.

## 1. Version

The operation set carries a single monotonically increasing integer, the **operation-set
version**. This document defines **version 1**.

### 1.1 What the number versions

The version covers the caller-visible contract of the whole set:

| change | bumps the version |
|---|---|
| an operation added | yes |
| an operation removed or renamed | yes |
| a parameter added to an existing operation, even an optional one | yes |
| a parameter removed, renamed, or its type changed | yes |
| the meaning of an existing operation changed for a request that was already valid | yes |
| a refusal made *more* specific (a new typed variant for a case that already refused) | no |
| a warning's wording | no |
| a defect fixed so that an operation finally does what this document already says | no |

The middle row is the one worth arguing. An optional parameter with a backwards-compatible
default does not break an old client, so the usual instinct is not to bump. But a *new* client
cannot tell whether the server it is talking to honours the parameter or ignores it, and a server
that ignores a parameter it does not know is exactly the silent degradation rule 5 forbids. So:
an unknown key inside an operation is a refusal (§8, `UnknownField`), and adding a key bumps the
version. That pair is what makes «my request was accepted» mean «every word of it was
understood».

### 1.2 How a client learns the version

Three channels, and none of them is a tool call:

- **The refusal.** An unknown operation is refused by name and the refusal states the version
  this server speaks and, when it is known, the version that defines the operation asked for
  (§8, `UnknownOperation`). This is the only moment a client actually needs the number, and it
  gets it for free.
- **The session preamble.** The `apply_edits` tool description names the version in one clause
  (`operation set 1`). `tools/list` is paid once per session and already carries it.
- **The CLI.** `docsai edit --help` states it in its first line.

**The response of a successful call does not carry the version.** Four tokens on every successful
edit, forever, to answer a question only a *failing* client asks, and which the failure already
answers, is the wrong trade (§10).

A request **may** declare the version it was written against, as the optional request-level field
`ops` (§3). Absent means «whatever this server speaks», which is what a hand-written call and
every call in version 1 will look like. Present and greater than the server's is refused
(`UnsupportedOpsVersion`) rather than best-efforted. Present and *lower* than the server's is
accepted only where a future version declares itself compatible with it; version 1 has nothing
below it, so this rule exists to be inherited, not to be exercised.

### 1.3 Version 1 is the whole set

| operation | §  |
|---|---|
| `replace_text` | 6.1 |
| `set_props` | 6.2 |
| `insert_after` | 6.3 |
| `insert_before` | 6.3 |
| `delete` | 6.4 |
| `move` | 6.5 |
| `duplicate_slide` | 6.6 |
| `set_cell` | 6.7 |
| `set_notes` | 6.8 |
| `replace_image` | 6.9 |

**Ten names, nine entries in the plan.** `docs/development-plan-v2.md:316` — the authority on the
list — writes them as nine list items because it joins `insert_after`/`insert_before` with a
slash; [`kb/85-phase-17-plan.md`](../kb/85-phase-17-plan.md) then says «the nine operations» while
enumerating the slash-joined pair as two. There is no third operation hiding in the discrepancy
and none missing: the set is the ten names above, which is the plan's list read literally.

They are kept as **two names** rather than collapsed into one `insert` with a `where:
after|before` parameter. Collapsing would save nothing on the wire — an operation name and an
enum value cost the same one or two tokens — and would add a required parameter that every insert
pays, in exchange for departing from the list the plan fixed.

## 2. Document classes

The IR has three roots — `Document::Text`, `Document::Workbook`, `Document::Presentation` — and
Phase 17's first acceptance criterion speaks of **four document classes**. The fourth is
**DocMark itself**: a `.dmk.md` is a first-class input and output of `docsai edit`, it is the
serialization the operations are defined over, and every operation applies to it. So:

| class | IR root | formats |
|---|---|---|
| **text** | `Document::Text` | `.docx`, `.odt` |
| **workbook** | `Document::Workbook` | `.xlsx`, `.ods` |
| **presentation** | `Document::Presentation` | `.pptx` |
| **docmark** | any of the three | `.dmk.md` |

The criterion is «a round-trip test on all four document classes it applies to». Read against
this table it is checkable: §6 states per operation which of the four it applies to, and §11
turns that into the test matrix. Within **text** and **workbook** the criterion is satisfied only
when *both* packages are covered — `.docx` and `.odt`, `.xlsx` and `.ods` — because they are
different writers over the same tree, and an operation that works on one and not the other is a
defect the IR-level definition was supposed to prevent. `.doc` and `.xls` are read-only and
therefore outside every class here; `.pptm` is a `.pptx` for this purpose.

Whether the owner reads «four classes» the same way is scored in 17-H, not here. This document
states the reading it was written against so that a disagreement is about one sentence rather
than about the whole test matrix.

## 3. The request

```jsonc
{
  "path": "deck.pptx",          // the document to edit
  "ops": 1,                     // optional: the operation-set version the caller wrote against
  "operations": [ /* … */ ],    // in order; see §4
  "dry_run": false              // everything except the write; §5.6 and 17-D
}
```

`path`, the output destination, the `content_base64`/`filename` pair, `fidelity` and the asset
options are the door's business (17-E) and follow whatever the existing tools already do; they
are not part of the operation set and are not versioned by it. Only the two fields above are.

**Id policy.** An edit is only meaningful on a document that carries ids, because the response
names what it created and changed by id. A request whose output would carry no ids — `--ids
never`, or `--fidelity plain`, which is CommonMark and never carries ids — is refused with
`Unaddressable` rather than served with ids that the written file does not contain. Editing at
`standard` is likewise refused: `standard` is a lossy projection, and writing one back over a
package would discard everything the projection dropped, which is a far larger change than the
operation asked for. Version 1 edits at `full` and `agent` only.

## 4. Ordering, atomicity and validation

The operations of one request form an **ordered list**, applied in order to one in-memory
document. There is no parallelism and no reordering: operation *k* sees the document as
operations 1..*k*−1 left it. This matters because §5.3 makes path targets resolve against the
live document.

**Validation is a separate pass and it comes first.** Before any operation is applied, the whole
list is checked for everything that can be known without applying it: unknown operations, unknown
fields, missing required fields, malformed targets, an `ops` version the server does not speak.
A failure there refuses the **whole request** and the document is not touched. This is not the
transaction — that is 17-C, and it covers failures that only appear during application (a target
that does not exist, a wrong kind, a writer refusal) by rolling the whole request back. The
early pass exists because a request with a typo in it should cost nothing and should say so
before it has half-edited anything, even in a world where the rollback works perfectly.

**All-or-nothing** was 17-C's to implement and prove, and is
([`kb/89-phase-17-transaction.md`](../kb/89-phase-17-transaction.md)): the operations apply to an
in-memory document, the result is written to a temporary file in the destination's own directory,
and the original is replaced by rename. What this document fixes is the consequence
for the operation set: there is no partial success and therefore no per-operation status in the
response. Either every operation applied or none did.

## 5. Targets and ids

This is the section the document exists for.

### 5.1 What a target is

```
target ::= root ( "/" index )*
root   ::= node-id | "doc"
index  ::= 1..N          # 1-based position in the parent's child sequence
```

- A **node id** is the opaque token of DocMark §11.1: `n12`, or a hand-written `intro.title`. A
  leading `#` is accepted and ignored, so `#n12` and `n12` are the same target; the canonical
  form in a response is always without it.
- **`doc`** is the reserved root: the document's own top-level child sequence. `doc/3` is the
  third top-level block of a text document, the third sheet of a workbook, the third slide of a
  deck. It exists because a single-section text document has no addressable section (§11.1: a
  section is addressed only in a multi-section document) and would otherwise have no way to say
  «at the top».
  **In a multi-section text document `doc`'s children are its sections**, and the third block of
  the second section is `doc/2/3`. 17-B added this sentence, which 17-A left implicit: `doc` names
  what the document *shows*, so the single section that DocMark does not write is transparent and
  the sections it does write are not. The rule is the same one §11.1 already states about when a
  section is addressable at all, applied to the root.
  **`doc` is reserved**, so a document containing a node whose id is literally `doc` makes the
  root ambiguous; that is refused rather than shadowed, on the same grounds as `DuplicateId`.
- An **index** is 1-based, matching the `s4` selectors of Phase 11 and every other position in
  DocMark.

### 5.2 Why a *selector* is not a target

Plan task 2 says «targeting by id (`#s4.b2`) or relative path». `s4.b2` is a **selector** — the
specification is explicit that positional forms are selectors and never ids (§11.1) — and
version 1 does **not** accept selectors as edit targets. Three reasons, and the narrowing is
recorded as an open question for 17-H rather than hidden:

1. A selector is evaluated against a document *state*, and a batch has no single state. `s4` in
   operation 3 would mean something different from `s4` in operation 1 whenever operation 2
   inserted a block, and the caller wrote both at the same moment against the same reading.
2. Selectors union and match by type and text (`type:heading`, `text:revenue`). A term that
   matches four nodes is a perfectly good *reading* and an appalling *write*: «replace the text of
   every heading containing "revenue"» is a search-and-replace, not an addressed edit, and the
   set deliberately has no operation that applies to a set of nodes.
3. The caller does not need one. Every read primitive already returns ids — that is what Phase 10
   built them for — so a caller that ran `read --select s4` in the same loop has `n12` in hand and
   spends nothing to use it.

What survives of «or relative path» is §5.1's index path, which is a *positional* address with
the two problems above bounded: it names exactly one node, and it is rooted at an id (or at `doc`)
so only its own parent's sequence can move under it.

### 5.3 When a target is resolved

**At the moment its operation is applied**, against the live document, and this is normative:

- An **id** target is immune to every preceding operation in the batch. `n12` is the same node
  whatever was inserted before it, moved past it, or deleted around it — unless `n12` itself was
  deleted, which makes the later operation a plain `UnknownTarget` refusal.
- An **index path** target is not immune. `n7/3` after an `insert_before` on `n7/2` addresses
  what was `n7/2`.

This is the honest rule rather than the convenient one, and it is teachable in one line: *ids are
stable, positions are not.* A caller that wants immunity uses an id — and every operation that
creates a node returns its id (§5.4) precisely so that the next operation in the same batch can.

### 5.4 What each operation does to ids

The invariants, first, because every row below is one of them applied:

- **Ids are never renumbered.** Inserting, deleting or moving anything changes no other node's id.
  Phase 10 already guarantees this and Phase 17 must not break it.
- **A freed id is never reused.** Deleting the node holding `n7` does not lower the counter and
  `n7` is never handed out again, in this document or any later revision of it. The counter is
  monotonic (`Addressing::alloc`) and a reader raises it above every id it finds before allocating
  anything. This is a correctness property, not tidiness: an agent holding a stale `n7` from a
  previous read must get `UnknownTarget`, and would instead silently edit an unrelated node if
  `n7` had been reissued.
- **A created node always gets an id**, allocated from the document's own counter, even under
  `--ids preserve`. `preserve` describes what a *conversion* writes back; a node that was created
  by this request has no id to preserve, and the response has to be able to name it.

| operation | ids |
|---|---|
| `replace_text` | none created, none freed. The target keeps its id. |
| `set_props` | none created, none freed. |
| `insert_after` / `insert_before` | every addressable node in the inserted content that has no id is allocated one. Ids written into the content by the caller are honoured (§5.5). Nothing else in the document changes id. |
| `delete` | the target's id and every id beneath it are freed, and none of them is ever reused. |
| `move` | **every id is preserved, the subtree's included.** The node is the same node somewhere else, which is the entire point of having an identity separate from a position. |
| `duplicate_slide` | the copy and **every addressable node inside it** get fresh ids. Nothing is shared with the source: two nodes with one id is the failure mode ids exist to prevent. |
| `set_cell` | none. A cell is not an addressable node — a sheet is (§6.7). |
| `set_notes` | the notes' blocks are content of the slide and are not separately addressable, so none are allocated. |
| `replace_image` | none. The `ImageRef` keeps its id; only the asset it points at changes. |

### 5.5 Ids the caller writes

Inserted content is DocMark, so it may carry `{#n9}`.

- If the id is **free** in the document (no node has it, and the counter has not passed it in a
  way that could reissue it), it is honoured verbatim and the counter is raised above it, exactly
  as a reader does.
- If the id is **taken**, the request is refused with `DuplicateId` naming it. It is not
  renumbered. Silently renumbering a caller's id would hand back a document in which the id the
  caller wrote means a different node than the one it wrote it on — the same class of failure as
  reusing a freed id, arrived at from the other direction.

### 5.6 What the response says about ids

Fixed here because operations must be able to say it; the report's full shape is 17-D's.

```jsonc
{
  "created": [ { "id": "n41", "kind": "paragraph" } ],   // document order
  "deleted": [ "n8" ],                                    // targets removed
  "changed": [ { "id": "n12", "etag": "a3f19c" } ],       // new etags: 17-B/17-D
  "warnings": [ /* the existing ConversionReport warnings */ ]
}
```

`deleted` lists the **targets** and not their subtrees. A caller that deleted a slide knows every
id under it is gone — that is what deleting a slide means — and enumerating forty shape ids to
say so would cost more than the whole rest of the response (§10). The rule is stated instead:
*every id beneath a deleted target is deleted with it.*

`created` is what makes plan task 5's «no confirmation read is needed» true rather than claimed.
It is uniform across `insert_after`, `insert_before` and `duplicate_slide`: everything the request
made, with its kind, in document order. A duplicated slide therefore returns its shapes' ids too,
which costs roughly two tokens a node and is the reason a duplicate can be edited in the same
loop that made it.

## 6. The operations

Each entry states: its parameters, the node kinds it applies to, its meaning **against the IR**,
the document classes it applies to, what it returns, and what it refuses. Refusal names are
defined in §8.

Every operation additionally accepts the optional field **`if_match`** (§7).

### 6.1 `replace_text`

```jsonc
{ "op": "replace_text", "target": "n12", "text": "Q3 results" }
```

| | |
|---|---|
| kinds | `paragraph`, `heading`, `shape` whose kind is a placeholder or a text box |
| classes | text, presentation, docmark (a workbook's text is `set_cell`) |
| returns | `changed: [target]` |

**Meaning.** The target's text content becomes `text`, exactly. `text` is a **plain string**: no
Markdown, no DocMark, no inline markup is interpreted in it. A `*` is an asterisk. Structure is
changed by `insert_*`, `delete` and `move`, or by the escape hatch; letting structure in through a
string field would make every text replacement a parse with its own failure modes, and would make
`{#n9}` in a caller's prose an addressing accident.

**Against the IR.** For a `Paragraph` (and a `Heading`, which owns one), `content` becomes a
single `Inline::Text`. For a `Shape` with a text body, the body becomes a single `Paragraph`
holding a single `Inline::Text`; a multi-paragraph body is collapsed to one paragraph.

**Formatting.** The run properties of the **first** run of the target are applied to the
replacement, and the paragraph's own `ParaFormat` (its style, its direct paragraph properties) is
untouched. Every other run's properties are dropped, because there is no defensible mapping from
*n* runs onto a string of unrelated length. When the target had more than one run and they did not
all carry the same properties, the operation still applies and emits `Warning::Degraded` naming
the node and the number of distinct run styles it collapsed. When the target had one run, or
several with identical properties, nothing is lost and **nothing is warned** — a warning fired on
every retitle would be false in the ordinary case, which rule 5 forbids as firmly as a missing one.

**Refusals.** `WrongKind` for anything not in the kinds row. Notably: a `shape` whose kind is a
chart is refused and the message names §9 — a chart's title lives in `ppt/charts/chartN.xml` and
Phase 16 established that a preserved chart's *shape* is not editable, only its numbers
([`kb/84-phase-16-close.md`](../kb/84-phase-16-close.md) item 14). A `comment` is refused: §6.10.
A `table` or `row` is refused; the cell's paragraph is the target, reached by path.

### 6.2 `set_props`

```jsonc
{ "op": "set_props", "target": "n41", "props": { "bold": true, "color": "C00000" } }
```

| | |
|---|---|
| kinds | `paragraph`, `heading`, `shape` whose kind is a placeholder or a text box |
| classes | text, presentation, docmark |
| returns | `changed: [target]` |

**The property set is closed**, and it is these:

| key | type | IR |
|---|---|---|
| `bold`, `italic`, `strike`, `small_caps`, `caps` | bool | `FontProps` |
| `underline` | `none`, `single`, `double`, `thick`, `dotted`, `dashed`, `wave` | `FontProps::underline` |
| `color`, `highlight` | `RRGGBB` | `FontProps` |
| `size` | a DocMark length (`11pt`) | `FontProps::size` |
| `font` | string | `FontProps::name` |
| `align` | `left`, `center`, `right`, `justify` | `ParaProps::align` |
| `style` | a style id present in the document's catalogue | `ParaFormat::style` |

An unknown key is `UnknownField`, not an ignored key. A misspelled `bould: true` that returned
success would be a silent no-op reported as a change, which is worse than a refusal by exactly the
amount an agent trusts the response.

**Meaning.** `null` clears a property to whatever the cascade implies; an absent key leaves it
alone. That distinction is why the field is a map and not a struct of `Option`s on the wire.

A **font** property set on a paragraph or a heading applies to the paragraph's whole content: it
is written to `ParaFormat::run_direct` *and* the same property is cleared on every descendant run
that overrode it, so the paragraph reads uniformly. This is the operation's definition, not a
degradation, and it emits no warning: a caller that says «bold this paragraph» and gets a
paragraph with one stubbornly non-bold word has been failed, and a warning on every such call
would fire on the ordinary case. On a `shape`, the same applies to every paragraph of its body.

`style` is a **reference**. The style must already exist in the document's `StyleCatalog`;
`UnknownStyle` otherwise. Version 1 defines, creates and modifies no styles — see §7.2 of the
plan's own note and §8.2 below.

### 6.3 `insert_after`, `insert_before`

```jsonc
{ "op": "insert_after", "target": "n12", "content": "- Revenue up 12 %" }
```

| | |
|---|---|
| kinds | any node that is an element of a sequence: block-level nodes (`paragraph`, `heading`, `list`, `table`, `image`), `row`, `section`, `sheet`, `slide`, `shape`; plus the `doc` root and index paths |
| classes | all four |
| returns | `created: [every addressable node made]` |

**Meaning.** `content` is a **DocMark fragment**, parsed with the document's own DocMark version
and profile (a deck's fragment is DocMark-P). The parsed nodes are spliced into the target's
parent sequence, immediately after or before the target, in the order they were written. The
target's parent is not otherwise touched, and no sibling's serialization moves — which is Phase
17's second acceptance criterion, and is Phase 15-B's existing «nothing is written unless it
differs from what the part already says» restated, not a new property.

**The fragment's kind must match the sequence.** Blocks go into a block sequence, rows into a
table, slides into a deck, shapes onto a slide, sheets into a workbook. A paragraph inserted
after a slide is `ContentKindMismatch` naming both. There is no coercion: wrapping a stray
paragraph in a slide, or a stray slide in a text box, is the kind of helpfulness that produces a
document nobody asked for.

**Refusals.** `WrongKind` for a target that is not a sequence element — a `comment` and a
`footnote` are the two that matter, and both are refused with their reason: a comment's position
is its anchor and a footnote's is its reference, so «after» has no meaning for either, and the
set has no operation that creates one (§6.10). `ContentKindMismatch`. `DuplicateId` (§5.5). A
parse failure in the fragment is the parser's own typed error, carried through unchanged — which
is also what 17-F's `validate_docmark` exists to let a caller get *before* it spends an edit.

### 6.4 `delete`

```jsonc
{ "op": "delete", "target": "n8" }
```

| | |
|---|---|
| kinds | the sequence elements of §6.3, plus `comment` and `footnote` |
| classes | all four |
| returns | `deleted: [target]` |

**Meaning.** The node and its entire subtree leave their parent sequence. Deleting a `footnote`
removes both its reference and its text, because they are one node with two places in the
serialization. Deleting a `comment` removes the thread, its replies included; §6.10 argues why
that is in the set when creating one is not.

**Refusals.** `LastOfKind` when the deletion would leave a document that cannot be written: the
last slide of a deck, the last (or last visible) sheet of a workbook, the last section of a text
document. Each names what it refused and why, and each is a real constraint of the target format
rather than a nervous guard — a workbook with no visible sheet is rejected by Excel, and the pptx
writer has never been asked for a deck with no slides.

### 6.5 `move`

```jsonc
{ "op": "move", "target": "n8", "after": "n2" }
```

| | |
|---|---|
| kinds | the sequence elements of §6.3 |
| classes | all four |
| returns | `changed: [target]` |

Exactly one of `after` or `before` is required, and it is another target (§5.1). The node is
detached from its parent sequence and re-inserted adjacent to the destination in the
destination's parent sequence. Both may be the same sequence, which is a reorder.

**Ids are preserved, the whole subtree's.** §5.4.

**Refusals.** `WrongKind` when the destination's sequence does not accept the target's kind — a
slide cannot move into a table. `CycleRefused` when the destination is inside the target's own
subtree. `GridMismatch` when a `row` moves into a table whose column count differs from its own:
version 1 does not pad or truncate cells, because both are a guess about a document's meaning.
Moving a *slide* within a preserved deck reaches the writer's existing content-based pairing,
which can still guess when two slides are both edited and reordered
([`kb/84-phase-16-close.md`](../kb/84-phase-16-close.md) item 29); that is a known open item, it
already warns, and this operation does not make it better or worse.

### 6.6 `duplicate_slide`

```jsonc
{ "op": "duplicate_slide", "target": "n8" }
```

| | |
|---|---|
| kinds | `slide` |
| classes | presentation, docmark |
| returns | `created: [the new slide and every addressable node in it]` |

**Meaning.** A deep copy of the slide is inserted **immediately after** the source. There is no
destination parameter: a copy that belongs elsewhere is one `move` away, and a parameter every
caller pays to express the default position is a parameter the default should have absorbed
(§10).

**What the copy carries.** Every shape with its geometry, text and properties; the layout
reference; the notes page; the slide-level raw fragments (a transition, a timing tree), each
re-registered under a fresh `RawId` so the two slides do not share a sidecar payload.

**What the copy does not carry: comments.** A comment asserts that a named person wrote a
specific thing about a specific slide. Copying it fabricates that assertion on a slide that
person never saw, and the set has nothing that could later disown it (§6.10). When — and only
when — the source slide carries comments, the operation emits `Warning::Degraded` naming the
slide and how many were not copied. On a slide with none it says nothing, because there is
nothing true to say.

**Refusals.** `NotApplicableToClass` when the document is not a presentation — raised before the
target is even resolved, so `duplicate_slide` on a `.docx` says «this document is a text document
and `duplicate_slide` applies to presentations», not «no such node». `WrongKind { got: row, takes:
[slide] }` for a target inside a deck that is not a slide.

### 6.7 `set_cell`

```jsonc
{ "op": "set_cell", "target": "n3", "cell": "B7", "value": 1200 }
```

| | |
|---|---|
| kinds | `sheet` |
| classes | workbook, docmark |
| returns | `changed: [target]` — the sheet, because a cell is not an addressable node |

`cell` is an A1 reference. `value` is a string, a number, a boolean, or `null` to clear the cell.
A string beginning with `=` is a **formula**, stored verbatim in the sheet's own
`FormulaDialect`; docsai does not parse, validate or evaluate formulas, and version 1 does not
start.

**Against the IR.** `Sheet::cells` is a sparse `BTreeMap<CellRef, Cell>`. Setting a cell that
exists keeps its `num_fmt` and its `style` and replaces its `value` and `formula`; setting one
that does not creates a `Cell` with neither. Clearing removes the entry.

**A formula clears the cached value.** A spreadsheet part stores both the formula and the number
it last evaluated to; writing a new formula over an old cached value would display a number that
is simply wrong until something recalculates. So the cached value is cleared, and the workbook is
asked to recalculate on load. **17-E settled how, and the answer is that both writers can, so
there is no warning** ([`kb/90-phase-17-doors.md`](../kb/90-phase-17-doors.md)):

- **xlsx.** `xl/workbook.xml` gains `<calcPr calcId="0" fullCalcOnLoad="1"/>` — ECMA-376's own way
  to ask the application to recalculate before it displays anything. It is written **only** when
  the workbook actually contains a formula cell with no cached value, because forcing a
  recalculation of a workbook that already carries every cached value would change values nobody
  asked to change, `NOW()` first among them.
- **ods.** No flag exists in ODF and none is needed. ODF keeps the cached result in
  `office:value`, so a `table:formula` cell written without one has nothing to display and the
  application computes it. Measured, not assumed: LibreOffice Calc returns `300` for a cell
  written as `table:formula="of:=[.A1]*[.A2]"` with no `office:value` at all.

The operation still does not invent a number, and it no longer emits a `Warning::Degraded` for a
condition both writers now handle — a warning that survives the reason for it is a false warning
(§0.5).

**Refusals.** `WrongKind` for a target that is not a sheet — a paragraph, most obviously.
`NotApplicableToClass` when the document is not a workbook. `CoveredCell` when the address falls
inside a merged range but is not its top-left, naming the range: writing into a covered cell puts
a value where no application will ever show it. `AddressOutOfRange` beyond the target format's
limits.

**A table cell is not a `set_cell`.** A `.docx` or `.pptx` table cell is a sequence of blocks,
not a value, and it is edited by addressing the paragraph inside it — `replace_text` on
`n7/2/1`. Overloading `set_cell` onto both would give one name two return types and two failure
sets. This is the clearest case in the document of a wrong-kind refusal that a caller will hit
on purpose, so the refusal names the alternative.

### 6.8 `set_notes`

```jsonc
{ "op": "set_notes", "target": "n8", "content": "Ask about Q4." }
```

| | |
|---|---|
| kinds | `slide` |
| classes | presentation, docmark |
| returns | `changed: [target]` |

**Meaning.** The slide's entire notes page becomes `content`, a DocMark fragment of blocks (a
bare string is one paragraph). `null` or an empty string removes the notes. It replaces rather
than appends, and a slide has exactly one notes page — the property Phase 14's P4 gate found the
parser getting wrong, and the reason this operation is a *set* and not an *append*.

**Refusals.** `NotApplicableToClass` on a text document or a workbook, named as such rather than
as a missing node. `WrongKind { got, takes: [slide] }` inside a deck. `ContentKindMismatch` if
the fragment parses to something that is not blocks.

### 6.9 `replace_image`

```jsonc
{ "op": "replace_image", "target": "n19", "path": "logo.png", "alt": "The new logo" }
```

| | |
|---|---|
| kinds | `image`, `shape` whose kind is a picture |
| classes | all four |
| returns | `changed: [target]` |

The bytes come from `path`, or from `content_base64` plus `filename` — the same triple every read
tool already takes, so a client that can supply a document can supply a picture with no new
vocabulary. Exactly one of the two forms.

**Meaning.** The asset the `ImageRef` points at is replaced. The **geometry is kept**: display
size, crop, anchor, rotation, flip and border are the placement a person chose and the caller
replaced a picture, not a layout. Nothing is re-encoded — Phase 15-C's rule that images go into
the package as the bytes the asset store holds is not relaxed here.

When the new picture's aspect ratio differs from the drawn size by more than 1 %, the picture
will be visibly stretched, and `Warning::Degraded` names the node and both ratios. The behaviour
is specified rather than a failure, but a stretched logo is a degradation a caller wants to know
about, and the 1 % floor keeps the warning from firing on rounding.

**`alt` is optional and is the one extra parameter version 1 grants.** Replacing a picture
without being able to replace its description leaves a *false* description attached to it, which
is the same class of failure as a false warning. One optional key, five tokens when used, zero
when not.

**Refusals.** `WrongKind`. `ExternalImage` when the target has an `external_src`: a linked
picture has no bytes in the package to replace, and writing some would change what the document
*is*. A picture that lives inside an opaque object (an OLE snapshot) is not addressable at all —
it has no id — so it is an `UnknownTarget` and not a refusal of its own;
[`kb/84-phase-16-close.md`](../kb/84-phase-16-close.md) item 7 is the reason and is unchanged by
this phase.

### 6.10 Comments: `delete` only

`delete` accepts a `comment`; nothing creates one and nothing edits one. The asymmetry is
deliberate.

A comment is not document content in the way a paragraph is: it carries an **author**, initials
and a date, and it asserts that a named person said something. `docsai` cannot mint that
assertion, because there is no person on the other end of an `apply_edits` call — the agent is
not an author and inventing one would put a false name in a document. Editing an existing
comment's text is the same failure by a different route: it changes what a named person is
recorded as having said.

Deleting one is different in kind, and it is a normal act of reviewing a document: it removes an
assertion rather than fabricating one, and the deletion is the caller's own, made under the
caller's own authority over the file. So `delete` is in and everything else is out. A caller that
genuinely needs to add a comment has §9, where it writes the author's name itself and takes
responsibility for it.

## 7. Preconditions

Every operation accepts the optional field **`if_match`**, whose value is an etag
(`docsai_model::addressing::Etag`, six lowercase hex digits).

- **Absent** means no precondition. This is not a loophole: a caller that created a node earlier
  in the same batch has no etag for it, and one that has just read the document and is editing
  immediately has no concurrency to protect against. An unconditional edit is a normal edit.
- **Present** means the operation applies only if the target's etag equals it. A mismatch is
  `StaleEtag { target, expected, actual }` — a typed, loud failure carrying **both** values, and
  never an overwrite. Because there is no partial success (§4), one stale etag fails the whole
  request.

**The precondition is per operation, not per request.** A batch touches several nodes; a single
document-level etag would fail on any concurrent change anywhere in the file, including in a
region the caller neither read nor touched, which turns a precondition into a lock.

**Scope: an etag covers the node *and its subtree*.** This was reserved for 17-B and 17-B decided
it; the argument is in [`kb/87-phase-17-etags.md`](../kb/87-phase-17-etags.md). A slide's etag
moves when a shape on it changes, a table's when a cell does, a section's when any block inside
it does. The alternative — a node-only etag — would let `delete n4` and `move n4` guarded by an
`if_match` sail through a concurrent edit *inside* `n4` and destroy it, which is the one failure
the precondition exists to prevent.

**What an etag does not cover is appearance.** It hashes content and structure; formatting
properties are excluded, as they have been since Phase 10. The reason is that `agent` fidelity
drops appearance by design (spec §6), so an etag that hashed formatting would differ between the
same node read at `agent` and read at `full`, and §3 admits both for editing: a caller that read
one and wrote against the other would be refused for a change nobody made. A false refusal is as
forbidden as a missed one (§0.5). The consequence is stated rather than hidden: **a concurrent
`set_props` is not detected by `if_match` in version 1**, and a caller that must serialise
formatting edits has to do so above this layer.

## 8. Refusals

Every refusal is typed, carries the operation's index in the list, and names both sides of the
mismatch. The variants version 1 defines:

| refusal | raised when |
|---|---|
| `UnknownOperation { op, speaks, introduced_in }` | the `op` name is not in §1.3. Names the version this server speaks and, when known, the version that defines the name; otherwise says no version of the set defines it. |
| `UnsupportedOpsVersion { requested, speaks }` | the request's `ops` field asks for a version this server does not speak. |
| `UnknownField { op, field, known }` | a key inside an operation that version 1 does not define, including an unknown `set_props` property. |
| `MissingField { op, field }` | a required key absent, or the `after`/`before` pair of `move` given zero or two ways. |
| `MalformedField { op, field, reason }` | a key version 1 *does* define, holding a value of a shape it does not: `{"text": 5}`. Added in 17-E, when the first door turned the set into a JSON wire; §1.1's «a refusal made more specific» keeps the version at 1, because before the variant existed the case refused as an untyped deserialization error. |
| `MalformedTarget { target, reason }` | a target that is not §5.1's grammar. |
| `UnknownTarget { target }` | no such id, or an index path that ran off the end of a sequence; the message names the segment that failed. |
| `WrongKind { op, target, got, takes }` | the target resolves but its kind is not one the operation takes. |
| `NotApplicableToClass { op, class, applies_to }` | the *document* is of a class the operation does not apply to. Raised before target resolution. |
| `ContentKindMismatch { expected, got }` | inserted content whose kind the target's sequence does not hold. |
| `DuplicateId { id }` | inserted content carrying an id the document already uses. |
| `UnknownStyle { style }` | `set_props` naming a style that is not in the catalogue. |
| `CoveredCell { cell, merge }` | `set_cell` addressing a cell covered by a merge. |
| `AddressOutOfRange { cell }` | `set_cell` beyond the format's row/column limits. |
| `ExternalImage { target }` | `replace_image` on a linked picture. |
| `CycleRefused { target, destination }` | `move` into the target's own subtree. |
| `GridMismatch { target, destination, got, takes }` | `move` of a row into a table with a different column count. |
| `LastOfKind { kind }` | `delete` of the last slide, sheet or section. |
| `Unaddressable { reason }` | the request's fidelity or id policy would produce a document with no ids (§3). |
| `StaleEtag { target, expected, actual }` | `if_match` was given and the target's etag is not it (§7). Carries both values and never overwrites. |

### 8.1 The writer's own refusals

An operation the editor accepts may still be refused by the writer underneath it. Phase 15 and
Phase 16 already have those refusals and they are by name: a preserved chart's shape, a modern
comment part, a table whose grid moved, a hyperlink no relationship can state, a slide added to a
preserved deck that cannot be paired. Version 1 does not restate them and does not weaken them.

The rule that matters here is the **join**: a writer refusal fails the whole request, and 17-C's
transaction leaves the original file byte-identical. There is no path in which some operations
reach the package and one does not. This is why the two refusal layers can stay separate — the
operation layer refuses what it can know cheaply, the writer refuses what only it knows, and both
are total.

### 8.2 A refusal is not a warning

An operation that cannot be done refuses. An operation that is done but loses something warns.
The two are never swapped: refusing where a warning belongs makes the set unusable, and warning
where a refusal belongs is the silent degradation AGENTS.md §7 rule 3 forbids.

## 9. The escape hatch

Everything outside the set is done with **`read --select` and a rewrite**: read the region as
DocMark, edit the text, convert it back. The plan names this as the escape hatch and it is a
first-class part of the design, not an apology for one.

**Reach for it when:**

- The change has no name in §1.3: a table column added or removed (a column is not a node in the
  IR at all), a style defined or redefined, a chart's numbers or its shape, a comment created, a
  slide's layout changed, a document's metadata or page geometry, a header or a footer, a raw
  fragment's markup.
- The change is *structural over a region* — reorganising a section rather than moving one block.
  Past roughly a dozen operations the DocMark of the region costs less than the operations do
  (§10), and it is easier to check.
- The document is being edited at `plain` or `standard`, where §3 refuses an operation outright.

**What it costs, said plainly.** `read --select` gives a *partial* document (spec §2.1, `partial:
true`), which carries the ids and the counter of the document it came from and can be written back
node by node — but writing a partial document back *whole* would delete everything it does not
contain, which is why serialising one warns. And rewriting the whole document costs the document
twice on the wire, which is the cost this entire phase exists to remove. The escape hatch is a
boundary with a price on it, and the price is what makes the closed set worth having.

## 10. What the set costs a caller

Measured, not estimated, with the repository's own tokenizer
(`docsai_convert::tokens::count`, `o200k_base` — the same encoder as every other budget here),
over the framed JSON an MCP call actually carries.

| what | tokens |
|---|---|
| the call envelope: `tools/call`, the tool name, `path`, an empty `operations` list | **39** |
| `+ replace_text` | +18 |
| `+ insert_after` with a one-line fragment | +20 |
| `+ set_props { bold: true }` | +17 |
| `+ set_cell` | +21 |
| `+ set_notes` | +19 |
| `+ replace_image` with a `path` | +16 |
| `+ move` | +15 |
| `+ duplicate_slide` | +11 |
| `+ delete` | +10 |
| `+ if_match` on any of them | +9 |

So: **one operation costs a caller 57 request tokens**, of which 39 is the envelope every call
pays and 18 is the operation. A response naming one changed node and its new etag is **61**. One
edit, end to end, is **118 tokens**.

The number that matters to the phase is the batch. «Retitle slide 4, add a bullet, bold it» is
three operations in **one** call: 92 request tokens and 75 response tokens, **167 tokens and one
tool call**. Against the plan's 2 000-token, 3-tool-call budget for the whole loop — and against
the 1 238 tokens the read half of `deck-retitle` already spends
([`crates/docsai-mcp/tests/goldens/wire-cost.md`](../crates/docsai-mcp/tests/goldens/wire-cost.md))
— the return leg is not where that budget will be lost. 17-H measures it for real; this is what
the shapes above predict.

Three design decisions in this document were taken on those numbers, and are recorded so they are
not re-litigated by feel:

1. **`if_match` is optional.** It is 9 tokens, half the cost of a whole `replace_text`. A caller
   that needs a precondition pays for it; one editing a node it created two lines earlier does not.
2. **`duplicate_slide` has no destination parameter.** At 11 tokens it is the cheapest operation
   in the set, and a destination would roughly double it for a position that is the default
   almost every time.
3. **The response does not carry the operation-set version, and `deleted` does not enumerate
   subtrees.** Both would be paid on every call to state something the caller either already
   knows or is told for free when it is wrong.

## 11. The test matrix

Phase 17's first acceptance criterion — «every operation has a round-trip test on all four
document classes it applies to» — is this table, and 17-H scores it. A `—` is a
`NotApplicableToClass` or a `WrongKind`, and its refusal is itself the test.

| operation | text | workbook | presentation | docmark |
|---|---|---|---|---|
| `replace_text` | ✔ | — | ✔ | ✔ |
| `set_props` | ✔ | — | ✔ | ✔ |
| `insert_after` | ✔ | ✔ | ✔ | ✔ |
| `insert_before` | ✔ | ✔ | ✔ | ✔ |
| `delete` | ✔ | ✔ | ✔ | ✔ |
| `move` | ✔ | ✔ | ✔ | ✔ |
| `duplicate_slide` | — | — | ✔ | ✔ |
| `set_cell` | — | ✔ | — | ✔ |
| `set_notes` | — | — | ✔ | ✔ |
| `replace_image` | ✔ | ✔ | ✔ | ✔ |

**text** is satisfied only by both `.docx` and `.odt`; **workbook** only by both `.xlsx` and
`.ods` (§2).

## 12. What version 1 deliberately leaves out

Named individually, because a closed set is only defensible if its edges are argued. Every item
below is reachable through §9.

1. **Styles.** No `create_style`, no style redefinition. `set_props` may reference a style that
   exists. A style is a document-wide object and changing one changes every node that uses it,
   which is not an addressed edit; it is also the `read_styles` / `set_style` item the MCP wire-cost
   harness still lists as «not measured yet».
2. **Charts.** No operation touches a chart. Phase 16 landed charts as *data*, and Phase 16-C
   established that a preserved chart's shape is not editable — a series added or removed, its
   kind, a `c:rich` title and an emptied axis are each refused by the writer by name. Its
   **numbers** are edited by rewriting its DocMark table, which is §9 and is exactly how Phase 16
   demonstrated the criterion. Said plainly rather than left unstated: an operation cannot edit a
   chart in version 1.
3. **Comments, except `delete`.** §6.10.
4. **Slide thumbnails.** Not document content: a thumbnail is a render, produced by
   `render_slides`, and there is nothing in the document for an operation to change.
5. **Raw fragments and opaque objects.** SmartArt, OLE objects, transitions, timing trees and
   custom geometries are shapes with ids, so `delete` and `move` reach them as shapes. Their
   markup is not editable by any operation; it is a file in `assets/_raw/`, and editing a file is
   already the escape hatch.
6. **Document metadata, page geometry, headers and footers.** Front matter and section
   properties. They are a line each in DocMark and §9 rewrites them for nothing.
7. **Table columns.** A column is not a node in the IR — a table is rows of cells — so there is
   nothing to address. Rows are addressable in a complex table and by index path everywhere else.
8. **Slide layouts and masters.** No `set_layout`. A layout change re-resolves the entire
   placeholder cascade of a slide, which is a different document, not an edited node.
9. **Footnote and hyperlink creation, field insertion.** Inline structure that `replace_text`
   deliberately does not parse (§6.1); it arrives through an inserted DocMark fragment, which is
   `insert_*` or §9.
10. **Anything across two documents.** No copy, no merge. Ids are document-scoped and two
    documents' counters know nothing about each other.
11. **Set-valued targets.** No operation applies to more than one node. Search-and-replace is a
    reading followed by a rewrite, and §5.2 argues why.

## 13. Open questions this document hands on

Each names the increment that owns it.

- ~~**The scope of an etag** — node, or node and subtree.~~ Decided in **17-B**: node *and*
  subtree, §7.
- ~~**Whether a writer can request recalculation** after `set_cell` writes a formula, and the
  warning if it cannot.~~ Decided in **17-E**: both can, so there is no warning. xlsx writes
  `calcPr/@fullCalcOnLoad` when and only when a formula cell has no cached value; ods needs no
  flag, because an ODF formula cell without `office:value` is already a request to compute it,
  measured against LibreOffice Calc. §6.7.
- ~~**The full response shape**, of which §5.6 fixes only the id-bearing part.~~ Decided in
  **17-D**: §5.6's four lists and nothing else. `changed` names **the target in canonical form**,
  which for a node that carries an id is that id — and for a paragraph of ordinary prose, which
  Phase 10 deliberately gives no id, is the index path the caller wrote. §6.1's «returns
  `changed: [target]`» read literally.
- ~~**`dry_run`'s diff**: what «the applied diff» is, and whether it is DocMark or a structured
  list.~~ Decided in **17-D**: a structured list, and only of *identity and new state* — created
  ids with their kinds, deleted targets, changed targets with their new etags. Not the new
  content, which the caller wrote, and not a textual diff, whose size is a function of the
  document rather than of the request. Measured: a two-operation batch's report is **31 tokens**
  (`crates/docsai-convert/tests/edit_transaction.rs::the_report_for_a_realistic_batch_is_small_enough_to_be_free`).
- ~~**The `apply_edits` tool schema**, written for size from the first line: 16-G spent the
  wire-cost gate's 5 % inflation allowance once already, and a new tool costs preamble tokens
  before it costs anything else.~~ Decided in **17-E**: `operations` is published as an array of
  anything and the ten shapes are one line of prose, because the derived schema for the tagged
  enum is over a thousand tokens and every session would pay it. Measured: **249 tokens** on the
  session preamble, and the wire-cost suite moved 5 355 → 5 616 tokens, **+4.9 %**, inside the
  gate's 5 % allowance and without the hatch (`crates/docsai-mcp/tests/goldens/wire-cost.md`).
  §10.
- **Selectors as targets**, refused in §5.2 against a literal reading of plan task 2. If the owner
  reads the plan as requiring them, it is a version 2 decision and not a patch. **17-H**.
- **«Four document classes»**, read in §2 as text / workbook / presentation / docmark. **17-H**.
- **Risk P2.** `apply_edits` multiplies the number of ways bytes leave this project, and every one
  of them reaches a package as a splice into a preserved part — which is exactly the construction
  `docs/spikes/P2-powerpoint-gate.md` asks about, and it is still unsigned
  ([`kb/84-phase-16-close.md`](../kb/84-phase-16-close.md)). Owner action.
