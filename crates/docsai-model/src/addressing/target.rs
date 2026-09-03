//! Edit targets: from a target string to exactly one place in the IR
//! ([`docs/edit-operations.md`](../../../../../docs/edit-operations.md) §5.1),
//! and the etag precondition that guards a write to it (§7).
//!
//! Two things live here and they are deliberately together, because they are
//! the two halves of one sentence — *this node, and only if it still says what
//! I read*:
//!
//! - [`Target`] parses `("doc" | node-id) ("/" index)*` and [`resolve`] turns
//!   it into a [`Place`]: a borrowed cursor onto one node, one sequence element
//!   or the document root. Resolution is **total**. Every target either names
//!   exactly one place or fails with a [`TargetError`] that says what it got
//!   and what it expected; nothing is guessed and nothing resolves to a set.
//! - [`check_if_match`] compares the place's etag against the caller's. A
//!   mismatch is [`PreconditionError::StaleEtag`], carrying **both** values.
//!   There is no path through this module in which a stale etag becomes a
//!   warning, a log line, or an overwrite.
//!
//! # Etag scope: the node **and its subtree**
//!
//! This is Phase 17-B's decision, reserved by 17-A (§7) and argued in
//! `kb/87-phase-17-etags.md`. An etag covers the node *and everything beneath
//! it*: a slide's etag moves when a shape on it is retitled, a table's when a
//! cell changes, a section's when any block inside it does. The alternative —
//! a node-only etag — would mean that `delete n4` and `move n4` guarded by an
//! `if_match` sail straight through a concurrent edit *inside* `n4` and destroy
//! it, which is the exact failure the precondition exists to prevent. The
//! implementation is Phase 10's: every container's `hash_content` already feeds
//! its children in, so this is a decision confirmed and pinned by test, not a
//! mechanism invented here.
//!
//! What an etag does **not** cover is presentation. That too is Phase 10's
//! (`kb/14`), and 17-B keeps it: `agent` fidelity drops appearance, so an etag
//! that hashed formatting would differ between a node read at `agent` and the
//! same node read at `full`, and a caller that read one and edited against the
//! other would be refused for a change nobody made. A false refusal is as
//! forbidden as a missed one. The price is stated rather than hidden: a
//! concurrent `set_props` is **not** detected by `if_match` in version 1.

use std::fmt;

use crate::addressing::walk::{Addressable, NodeKind};
use crate::addressing::{Etag, NodeId};
use crate::image::ImageRef;
use crate::presentation::{Shape, ShapeKind, Slide};
use crate::sheet::Sheet;
use crate::text::{Block, Comment, Footnote, Inline, ListItem, Section, TableCell, TableRow};
use crate::Document;

/// The reserved root of §5.1: the document's own top-level child sequence.
pub const DOC_ROOT: &str = "doc";

/// The root of a target: the document itself, or one addressable node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetRoot {
    /// `doc` — the document's top-level child sequence.
    Doc,
    /// A node id (`n12`, `intro.title`).
    Id(NodeId),
}

/// A parsed edit target: `("doc" | node-id) ("/" index)*`.
///
/// ```
/// use docsai_model::addressing::{Target, TargetRoot};
///
/// let target = Target::parse("#n12/3").unwrap();
/// assert_eq!(target.root, TargetRoot::Id(docsai_model::NodeId::new("n12")));
/// assert_eq!(target.indices, vec![3]);
/// // The canonical form never carries the `#`.
/// assert_eq!(target.to_string(), "n12/3");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub root: TargetRoot,
    /// 1-based positions, outermost first. Empty for a bare root.
    pub indices: Vec<u32>,
}

/// Why a target string is not §5.1's grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MalformedReason {
    /// The whole target, or one of its segments, is empty.
    EmptySegment,
    /// The root is neither `doc` nor a token DocMark could write as `{#id}`.
    NotAnId,
    /// An index segment is not a decimal number.
    IndexNotANumber,
    /// An index segment is `0`, or carries a leading zero: indices are 1-based
    /// and written canonically.
    IndexNotOneBased,
    /// An index segment does not fit in a `u32`. A document with four billion
    /// siblings is not the case this is defending against; a caller pasting a
    /// hash where an index goes is.
    IndexTooLarge,
}

impl MalformedReason {
    pub fn as_str(self) -> &'static str {
        match self {
            MalformedReason::EmptySegment => "a target and each of its segments must be non-empty",
            MalformedReason::NotAnId => {
                "the root must be `doc` or a node id (letters, digits, `-`, `_`, `.`)"
            }
            MalformedReason::IndexNotANumber => "an index must be a decimal number",
            MalformedReason::IndexNotOneBased => {
                "an index is 1-based and written without a leading zero"
            }
            MalformedReason::IndexTooLarge => {
                "an index is too large to be a position in a document"
            }
        }
    }
}

impl fmt::Display for MalformedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Every way a target can fail to name exactly one place.
///
/// The variants are finer-grained than §8's caller-visible refusals on purpose:
/// §1.1 says a refusal made *more* specific costs no version bump, and the door
/// (17-E) maps these onto `MalformedTarget` and `UnknownTarget`.
/// [`TargetError::AmbiguousId`] maps onto neither, because it is not a mistake
/// the caller made — see its own note.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TargetError {
    /// Not §5.1's grammar. §8 `MalformedTarget { target, reason }`.
    #[error("malformed target `{target}`: {reason}")]
    Malformed {
        target: String,
        reason: MalformedReason,
    },
    /// No node in the document carries this id. §8 `UnknownTarget`.
    #[error("unknown target `{target}`: no node has the id `{id}`")]
    UnknownId { target: String, id: NodeId },
    /// An index ran off the end of a sequence. §8 `UnknownTarget`, and the
    /// message names the segment that failed rather than the whole target.
    #[error(
        "unknown target `{target}`: `{at}` is a {kind} with {len} children, so index {index} is out of range"
    )]
    IndexOutOfRange {
        target: String,
        /// The prefix of the target that did resolve.
        at: String,
        kind: &'static str,
        index: u32,
        len: usize,
    },
    /// An index into a node that has no indexable child sequence at all — a
    /// sheet, a paragraph, an image. Distinct from an out-of-range index
    /// because the fix is different: there is no index that would have worked.
    /// §8 `UnknownTarget`.
    #[error("unknown target `{target}`: `{at}` is a {kind}, which has no indexable children")]
    NotIndexable {
        target: String,
        at: String,
        kind: &'static str,
    },
    /// Two nodes carry the same id.
    ///
    /// Phase 10 guarantees this cannot happen: ids are allocated from a
    /// monotonic counter, never renumbered and never reused, and a reader
    /// raises the counter above every id it finds. The guarantee is asserted
    /// here rather than assumed, because the alternative to asserting it is
    /// resolving to whichever copy the walk reached first — an edit applied to
    /// a node the caller did not name, silently. It is a defect in the
    /// document (or in whatever wrote it), not in the request.
    #[error("ambiguous target `{target}`: {count} nodes carry the id `{id}`")]
    AmbiguousId {
        target: String,
        id: NodeId,
        count: usize,
    },
    /// The document contains a node whose id is the reserved root name `doc`,
    /// so `doc` would name two things. Refused rather than shadowed.
    #[error(
        "ambiguous target `doc`: the name is reserved for the document root, but a {kind} in this document carries it as its id"
    )]
    ReservedId { kind: &'static str },
}

impl Target {
    /// Parses §5.1's grammar. A single leading `#` is accepted and dropped.
    pub fn parse(target: &str) -> Result<Target, TargetError> {
        let bare = target.strip_prefix('#').unwrap_or(target);
        let malformed = |reason| TargetError::Malformed {
            target: target.to_string(),
            reason,
        };

        let mut segments = bare.split('/');
        // `split` always yields at least one item, so this cannot be `None`.
        let root_text = segments.next().unwrap_or_default();
        if root_text.is_empty() {
            return Err(malformed(MalformedReason::EmptySegment));
        }
        let root = if root_text == DOC_ROOT {
            TargetRoot::Doc
        } else {
            let id = NodeId::new(root_text);
            if !id.is_valid() {
                return Err(malformed(MalformedReason::NotAnId));
            }
            TargetRoot::Id(id)
        };

        let mut indices = Vec::new();
        for segment in segments {
            if segment.is_empty() {
                return Err(malformed(MalformedReason::EmptySegment));
            }
            if !segment.bytes().all(|b| b.is_ascii_digit()) {
                return Err(malformed(MalformedReason::IndexNotANumber));
            }
            if segment.starts_with('0') {
                return Err(malformed(MalformedReason::IndexNotOneBased));
            }
            let index: u32 = segment
                .parse()
                .map_err(|_| malformed(MalformedReason::IndexTooLarge))?;
            indices.push(index);
        }
        Ok(Target { root, indices })
    }

    /// The canonical form: no `#`, indices joined with `/`.
    pub fn canonical(&self) -> String {
        self.to_string()
    }

    /// The prefix of this target up to and including `depth` indices, which is
    /// what an error names when it says *which segment* failed.
    fn prefix(&self, depth: usize) -> String {
        let mut out = match &self.root {
            TargetRoot::Doc => DOC_ROOT.to_string(),
            TargetRoot::Id(id) => id.0.clone(),
        };
        for index in self.indices.iter().take(depth) {
            out.push('/');
            out.push_str(&index.to_string());
        }
        out
    }
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.prefix(self.indices.len()))
    }
}

impl std::str::FromStr for Target {
    type Err = TargetError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Target::parse(s)
    }
}

/// One place in the IR: a node, a sequence element, or the document root.
///
/// A place is *where an operation happens*. It is broader than an addressable
/// node because §5.1's index paths reach things that carry no id — a table
/// cell, a list item, the body of a text box — and §6.1 depends on that
/// (`a table or row is refused; the cell's paragraph is the target, reached by
/// path`). It is borrowed, and it is read-only: 17-B resolves and checks,
/// 17-C is the increment that mutates.
#[derive(Debug, Clone, Copy)]
pub enum Place<'a> {
    /// The reserved `doc` root.
    Doc(&'a Document),
    Section(&'a Section),
    Block(&'a Block),
    ListItem(&'a ListItem),
    Row(&'a TableRow),
    Cell(&'a TableCell),
    Sheet(&'a Sheet),
    Slide(&'a Slide),
    Shape(&'a Shape),
    Footnote(&'a Footnote),
    Comment(&'a Comment),
    /// An image reached as an inline, where there is no enclosing [`Block`].
    Image(&'a ImageRef),
}

impl<'a> Place<'a> {
    /// What this place is, in the words an error message uses.
    pub fn describe(&self) -> &'static str {
        match self {
            Place::Doc(_) => "document",
            Place::Section(_) => "section",
            Place::Block(block) => match block {
                Block::Paragraph(_) => "paragraph",
                Block::Heading(_) => "heading",
                Block::List(_) => "list",
                Block::Table(_) => "table",
                Block::Image(_) => "image",
                Block::TextBox(_) => "text box",
                Block::Raw(_) => "raw fragment",
            },
            Place::ListItem(_) => "list item",
            Place::Row(_) => "row",
            Place::Cell(_) => "cell",
            Place::Sheet(_) => "sheet",
            Place::Slide(_) => "slide",
            Place::Shape(shape) => match &shape.kind {
                ShapeKind::Placeholder(_) => "placeholder",
                ShapeKind::TextBox { .. } => "text box",
                ShapeKind::Picture(_) => "picture",
                ShapeKind::Table(_) => "table shape",
                ShapeKind::Chart(_) => "chart",
                ShapeKind::Group(_) => "group",
                ShapeKind::Raw(_) => "raw shape",
            },
            Place::Footnote(_) => "footnote",
            Place::Comment(_) => "comment",
            Place::Image(_) => "image",
        }
    }

    /// The node kind, where this place is one DocMark can address. `None` for
    /// the root and for the sequence elements that carry no id.
    pub fn kind(&self) -> Option<NodeKind> {
        match self {
            Place::Doc(_) | Place::ListItem(_) | Place::Cell(_) => None,
            Place::Section(_) => Some(NodeKind::Section),
            Place::Block(block) => match block {
                Block::Paragraph(_) => Some(NodeKind::Paragraph),
                Block::Heading(_) => Some(NodeKind::Heading),
                Block::List(_) => Some(NodeKind::List),
                Block::Table(_) => Some(NodeKind::Table),
                Block::Image(_) => Some(NodeKind::Image),
                Block::TextBox(_) | Block::Raw(_) => None,
            },
            Place::Row(_) => Some(NodeKind::TableRow),
            Place::Sheet(_) => Some(NodeKind::Sheet),
            Place::Slide(_) => Some(NodeKind::Slide),
            Place::Shape(_) => Some(NodeKind::Shape),
            Place::Footnote(_) => Some(NodeKind::Footnote),
            Place::Comment(_) => Some(NodeKind::Comment),
            Place::Image(_) => Some(NodeKind::Image),
        }
    }

    /// The id this place carries, if it carries one.
    pub fn id(&self) -> Option<&'a NodeId> {
        self.addressable().and_then(|node| node.node_id())
    }

    /// The etag of this place — the node **and its subtree** — or `None` where
    /// the place is not a node and therefore has no content hash of its own.
    pub fn etag(&self) -> Option<Etag> {
        self.addressable().map(|node| node.etag())
    }

    fn addressable(&self) -> Option<&'a dyn Addressable> {
        match self {
            Place::Doc(_) | Place::ListItem(_) | Place::Cell(_) => None,
            Place::Section(section) => Some(*section),
            Place::Block(block) => match block {
                Block::Paragraph(p) => Some(p),
                Block::Heading(h) => Some(h),
                Block::List(l) => Some(l),
                Block::Table(t) => Some(t),
                Block::Image(i) => Some(i),
                Block::TextBox(_) | Block::Raw(_) => None,
            },
            Place::Row(row) => Some(*row),
            Place::Sheet(sheet) => Some(*sheet),
            Place::Slide(slide) => Some(*slide),
            Place::Shape(shape) => Some(*shape),
            Place::Footnote(note) => Some(*note),
            Place::Comment(comment) => Some(*comment),
            Place::Image(image) => Some(*image),
        }
    }

    /// The child sequence an index addresses, in order.
    ///
    /// Empty means *this place has no indexable children at all*, which is a
    /// different failure from an index that ran off the end
    /// ([`TargetError::NotIndexable`] against [`TargetError::IndexOutOfRange`]).
    ///
    /// A **sheet** has none on purpose: its cells are a sparse map addressed in
    /// A1 notation by `set_cell` (§6.7), not a sequence, and inventing a
    /// positional order over a `BTreeMap` would give `sheet/3` a meaning that
    /// changes when an unrelated cell is filled in.
    pub fn children(&self) -> Vec<Place<'a>> {
        match self {
            Place::Doc(doc) => match doc {
                // A section is addressable only in a multi-section document
                // (spec §11.1), and `doc` exists precisely because of that
                // (§5.1). So `doc` names what the document *shows*: the single
                // section is transparent and `doc/3` is the third block, while
                // a multi-section document's own top-level sequence is its
                // sections.
                Document::Text(text) => match text.sections.as_slice() {
                    [only] => only.blocks.iter().map(Place::Block).collect(),
                    sections => sections.iter().map(Place::Section).collect(),
                },
                Document::Workbook(book) => book.sheets.iter().map(Place::Sheet).collect(),
                Document::Presentation(deck) => deck.slides.iter().map(Place::Slide).collect(),
            },
            Place::Section(section) => section.blocks.iter().map(Place::Block).collect(),
            Place::Block(block) => match block {
                Block::List(list) => list.items.iter().map(Place::ListItem).collect(),
                Block::Table(table) => table.rows.iter().map(Place::Row).collect(),
                Block::TextBox(text_box) => text_box.blocks.iter().map(Place::Block).collect(),
                Block::Paragraph(_) | Block::Heading(_) | Block::Image(_) | Block::Raw(_) => {
                    Vec::new()
                }
            },
            Place::ListItem(item) => item.blocks.iter().map(Place::Block).collect(),
            Place::Row(row) => row.cells.iter().map(Place::Cell).collect(),
            Place::Cell(cell) => cell.blocks.iter().map(Place::Block).collect(),
            Place::Sheet(_) | Place::Image(_) => Vec::new(),
            Place::Slide(slide) => slide.shapes.iter().map(Place::Shape).collect(),
            Place::Shape(shape) => match &shape.kind {
                ShapeKind::Placeholder(ph) => ph.body.iter().map(Place::Block).collect(),
                ShapeKind::TextBox { body } => body.iter().map(Place::Block).collect(),
                ShapeKind::Table(table) => table.rows.iter().map(Place::Row).collect(),
                ShapeKind::Group(children) => children.iter().map(Place::Shape).collect(),
                ShapeKind::Picture(_) | ShapeKind::Chart(_) | ShapeKind::Raw(_) => Vec::new(),
            },
            Place::Footnote(note) => note.blocks.iter().map(Place::Block).collect(),
            Place::Comment(comment) => comment.blocks.iter().map(Place::Block).collect(),
        }
    }
}

/// Resolves a target string against a document. Convenience over
/// [`Target::parse`] followed by [`resolve`].
pub fn resolve_str<'a>(doc: &'a Document, target: &str) -> Result<Place<'a>, TargetError> {
    resolve(doc, &Target::parse(target)?)
}

/// Resolves a parsed target to exactly one place.
///
/// Total: every input either yields one [`Place`] or a [`TargetError`] naming
/// what it got and what it expected.
pub fn resolve<'a>(doc: &'a Document, target: &Target) -> Result<Place<'a>, TargetError> {
    let canonical = target.canonical();
    let mut place = match &target.root {
        TargetRoot::Doc => {
            // `doc` is reserved, so a node holding it as an id would make the
            // root ambiguous. Refused, never shadowed: an edit that went to the
            // wrong one of two things called `doc` is the same class of failure
            // as a reused id.
            if let Some(found) = find_by_id(doc, &NodeId::new(DOC_ROOT)).first() {
                return Err(TargetError::ReservedId {
                    kind: found.describe(),
                });
            }
            Place::Doc(doc)
        }
        TargetRoot::Id(id) => {
            let found = find_by_id(doc, id);
            match found.len() {
                0 => {
                    return Err(TargetError::UnknownId {
                        target: canonical,
                        id: id.clone(),
                    })
                }
                1 => found[0],
                count => {
                    return Err(TargetError::AmbiguousId {
                        target: canonical,
                        id: id.clone(),
                        count,
                    })
                }
            }
        }
    };

    for (depth, index) in target.indices.iter().enumerate() {
        let children = place.children();
        if children.is_empty() {
            return Err(TargetError::NotIndexable {
                target: canonical,
                at: target.prefix(depth),
                kind: place.describe(),
            });
        }
        // Parsing guarantees `index >= 1`, so the subtraction is the 1-based
        // to 0-based step and nothing else.
        let position = usize::try_from(*index).unwrap_or(usize::MAX);
        match position.checked_sub(1).and_then(|at| children.get(at)) {
            Some(child) => place = *child,
            None => {
                return Err(TargetError::IndexOutOfRange {
                    target: canonical,
                    at: target.prefix(depth),
                    kind: place.describe(),
                    index: *index,
                    len: children.len(),
                })
            }
        }
    }
    Ok(place)
}

/// The etag of whatever `target` names.
pub fn etag_of(doc: &Document, target: &Target) -> Result<Etag, PreconditionError> {
    let place = resolve(doc, target).map_err(PreconditionError::Target)?;
    place.etag().ok_or_else(|| PreconditionError::NoEtag {
        target: target.canonical(),
        kind: place.describe(),
    })
}

/// Why an `if_match` precondition could not be satisfied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PreconditionError {
    /// The target does not name one place. Carried through rather than
    /// flattened: a precondition on a target that does not exist is a target
    /// failure, and saying so is more useful than saying the etag did not match.
    #[error(transparent)]
    Target(#[from] TargetError),
    /// §7/§8 `StaleEtag { target, expected, actual }`. Both values, always.
    #[error("stale etag for `{target}`: expected `{expected}`, the document says `{actual}`")]
    StaleEtag {
        target: String,
        /// What the caller sent in `if_match`.
        expected: Etag,
        /// What the document says now.
        actual: Etag,
    },
    /// An `if_match` on a place that has no etag — the `doc` root, a cell, a
    /// list item. Refused rather than ignored: a precondition that is silently
    /// not checked is worse than no precondition, because the caller believes
    /// it was.
    #[error("`{target}` is a {kind}, which has no etag to match against")]
    NoEtag { target: String, kind: &'static str },
}

/// Checks §7's `if_match` against a resolved place.
///
/// `None` means no precondition, which §7 is explicit is a normal edit and not
/// a loophole. `Some` means the operation applies only if the etag still
/// matches; otherwise this returns [`PreconditionError::StaleEtag`] and the
/// caller has nothing to overwrite with.
pub fn check_if_match(
    place: &Place<'_>,
    target: &Target,
    if_match: Option<&Etag>,
) -> Result<(), PreconditionError> {
    let Some(expected) = if_match else {
        return Ok(());
    };
    let target = target.canonical();
    let actual = place.etag().ok_or(PreconditionError::NoEtag {
        target: target.clone(),
        kind: place.describe(),
    })?;
    if &actual == expected {
        Ok(())
    } else {
        Err(PreconditionError::StaleEtag {
            target,
            expected: expected.clone(),
            actual,
        })
    }
}

/// Resolves `target` and checks `if_match` in one step: the whole of what an
/// operation does before it does anything.
pub fn resolve_checked<'a>(
    doc: &'a Document,
    target: &Target,
    if_match: Option<&Etag>,
) -> Result<Place<'a>, PreconditionError> {
    let place = resolve(doc, target)?;
    check_if_match(&place, target, if_match)?;
    Ok(place)
}

/// Every place in the document whose node carries `id`.
///
/// A `Vec` and not an `Option`, because the whole point is to be able to say
/// «two» ([`TargetError::AmbiguousId`]). The walk covers every node that can
/// *hold* an id, including the ones [`crate::addressing::for_each_addressable`]
/// skips because DocMark could not write their id — a single document's
/// section, a simple table's row. Assignment policy decides who is *given* an
/// id; a lookup has to find whoever *has* one, or a document that came in with
/// one would have an unreachable node.
fn find_by_id<'a>(doc: &'a Document, id: &NodeId) -> Vec<Place<'a>> {
    let mut found = Vec::new();
    for_each_place(doc, &mut |place| {
        if place.id() == Some(id) {
            found.push(place);
        }
    });
    found
}

/// Walks every id-carrying place of the document in document order.
fn for_each_place<'a>(doc: &'a Document, f: &mut dyn FnMut(Place<'a>)) {
    match doc {
        Document::Text(text) => {
            for section in &text.sections {
                f(Place::Section(section));
                for header in &section.headers {
                    each_blocks(&header.blocks, f);
                }
                for footer in &section.footers {
                    each_blocks(&footer.blocks, f);
                }
                each_blocks(&section.blocks, f);
            }
        }
        Document::Workbook(book) => {
            for sheet in &book.sheets {
                f(Place::Sheet(sheet));
                for image in &sheet.images {
                    f(Place::Image(image));
                }
            }
        }
        Document::Presentation(deck) => {
            for slide in &deck.slides {
                f(Place::Slide(slide));
                for shape in &slide.shapes {
                    each_shape(shape, f);
                }
                if let Some(notes) = &slide.notes {
                    each_blocks(notes, f);
                }
                for comment in &slide.comments {
                    each_comment(comment, f);
                }
            }
        }
    }
}

fn each_shape<'a>(shape: &'a Shape, f: &mut dyn FnMut(Place<'a>)) {
    f(Place::Shape(shape));
    match &shape.kind {
        ShapeKind::Placeholder(ph) => each_blocks(&ph.body, f),
        ShapeKind::TextBox { body } => each_blocks(body, f),
        ShapeKind::Picture(image) => f(Place::Image(image)),
        ShapeKind::Table(table) => each_table(table, f),
        ShapeKind::Group(children) => {
            for child in children {
                each_shape(child, f);
            }
        }
        ShapeKind::Chart(_) | ShapeKind::Raw(_) => {}
    }
}

fn each_table<'a>(table: &'a crate::text::Table, f: &mut dyn FnMut(Place<'a>)) {
    for row in &table.rows {
        f(Place::Row(row));
        for cell in &row.cells {
            each_blocks(&cell.blocks, f);
        }
    }
}

fn each_blocks<'a>(blocks: &'a [Block], f: &mut dyn FnMut(Place<'a>)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                f(Place::Block(block));
                each_inlines(&p.content, f);
            }
            Block::Heading(h) => {
                f(Place::Block(block));
                each_inlines(&h.paragraph.content, f);
            }
            Block::List(list) => {
                f(Place::Block(block));
                for item in &list.items {
                    each_blocks(&item.blocks, f);
                }
            }
            Block::Table(table) => {
                f(Place::Block(block));
                each_table(table, f);
            }
            Block::Image(_) => f(Place::Block(block)),
            Block::TextBox(text_box) => each_blocks(&text_box.blocks, f),
            Block::Raw(_) => {}
        }
    }
}

fn each_inlines<'a>(inlines: &'a [Inline], f: &mut dyn FnMut(Place<'a>)) {
    for inline in inlines {
        match inline {
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                each_inlines(content, f)
            }
            Inline::Footnote(note) => {
                f(Place::Footnote(note));
                each_blocks(&note.blocks, f);
            }
            Inline::Comment(comment) => each_comment(comment, f),
            Inline::Image(image) => f(Place::Image(image.as_ref())),
            _ => {}
        }
    }
}

fn each_comment<'a>(comment: &'a Comment, f: &mut dyn FnMut(Place<'a>)) {
    f(Place::Comment(comment));
    each_blocks(&comment.blocks, f);
    each_inlines(comment.covered(), f);
    for reply in &comment.replies {
        each_comment(reply, f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{Paragraph, TextDocument};

    fn doc_with(sections: Vec<Section>) -> Document {
        Document::Text(TextDocument {
            sections,
            ..Default::default()
        })
    }

    fn section(blocks: Vec<Block>) -> Section {
        Section {
            blocks,
            ..Default::default()
        }
    }

    fn para(id: &str, text: &str) -> Block {
        let mut p = Paragraph::text(text);
        p.id = Some(NodeId::new(id));
        Block::Paragraph(p)
    }

    #[test]
    fn the_hash_is_optional_and_the_canonical_form_drops_it() {
        assert_eq!(Target::parse("#n1").unwrap(), Target::parse("n1").unwrap());
        assert_eq!(Target::parse("#n1/2/3").unwrap().to_string(), "n1/2/3");
        // Only one `#` is a prefix; a second is part of the id and invalid.
        assert!(matches!(
            Target::parse("##n1"),
            Err(TargetError::Malformed {
                reason: MalformedReason::NotAnId,
                ..
            })
        ));
    }

    #[test]
    fn the_grammar_refuses_what_it_is_not() {
        let cases = [
            ("", MalformedReason::EmptySegment),
            ("#", MalformedReason::EmptySegment),
            ("n1//2", MalformedReason::EmptySegment),
            ("n1/", MalformedReason::EmptySegment),
            ("n1/x", MalformedReason::IndexNotANumber),
            ("n1/-2", MalformedReason::IndexNotANumber),
            ("n1/0", MalformedReason::IndexNotOneBased),
            ("n1/01", MalformedReason::IndexNotOneBased),
            ("n1/99999999999", MalformedReason::IndexTooLarge),
            ("a b", MalformedReason::NotAnId),
        ];
        for (input, expected) in cases {
            match Target::parse(input) {
                Err(TargetError::Malformed { reason, .. }) => {
                    assert_eq!(reason, expected, "for {input:?}");
                }
                other => panic!("{input:?} should be malformed, got {other:?}"),
            }
        }
    }

    #[test]
    fn doc_is_the_single_sections_blocks_and_the_sections_otherwise() {
        let one = doc_with(vec![section(vec![para("n1", "a"), para("n2", "b")])]);
        let place = resolve_str(&one, "doc/2").unwrap();
        assert_eq!(place.id(), Some(&NodeId::new("n2")));

        let many = doc_with(vec![
            section(vec![para("n1", "a")]),
            section(vec![para("n2", "b")]),
        ]);
        let place = resolve_str(&many, "doc/2").unwrap();
        assert_eq!(place.kind(), Some(NodeKind::Section));
        assert_eq!(
            resolve_str(&many, "doc/2/1").unwrap().id(),
            Some(&NodeId::new("n2"))
        );
    }

    #[test]
    fn a_reserved_id_is_refused_rather_than_shadowed() {
        let doc = doc_with(vec![section(vec![para("doc", "a")])]);
        assert!(matches!(
            resolve_str(&doc, "doc"),
            Err(TargetError::ReservedId { .. })
        ));
        // The node is still reachable by every other route.
        assert!(resolve_str(&doc, "#doc").is_err());
    }

    #[test]
    fn a_stale_etag_is_a_failure_carrying_both_values() {
        let doc = doc_with(vec![section(vec![para("n1", "hello")])]);
        let target = Target::parse("n1").unwrap();
        let place = resolve(&doc, &target).unwrap();
        let actual = place.etag().unwrap();

        assert!(check_if_match(&place, &target, None).is_ok());
        assert!(check_if_match(&place, &target, Some(&actual)).is_ok());

        let stale = Etag::new("000000");
        match check_if_match(&place, &target, Some(&stale)) {
            Err(PreconditionError::StaleEtag {
                target,
                expected,
                actual: got,
            }) => {
                assert_eq!(target, "n1");
                assert_eq!(expected, stale);
                assert_eq!(got, actual);
            }
            other => panic!("expected a stale etag, got {other:?}"),
        }
    }

    #[test]
    fn a_precondition_on_a_place_without_an_etag_is_refused() {
        let doc = doc_with(vec![section(vec![para("n1", "hello")])]);
        let target = Target::parse("doc").unwrap();
        let place = resolve(&doc, &target).unwrap();
        assert!(matches!(
            check_if_match(&place, &target, Some(&Etag::new("abc123"))),
            Err(PreconditionError::NoEtag { .. })
        ));
    }
}
