//! Walking the IR to assign ids and to derive etags.
//!
//! Two passes, and the order matters: every id already in the document is
//! *observed* first, so the counter dominates them, and only then are the
//! missing ones filled in. A single pass would let a hand-written `n7` collide
//! with a freshly allocated `n7` further down the document.
//!
//! Etags are **derived, never stored**: recomputing one from the node is always
//! correct, whereas a stored etag can go stale behind an edit. The IR therefore
//! carries ids only.

use crate::addressing::{Addressing, Etag, EtagHasher, IdPolicy, NodeId};
use crate::image::ImageRef;
use crate::sheet::Sheet;
use crate::text::{Block, Footnote, Heading, Inline, List, Paragraph, Section, Table, TableRow};
use crate::Document;

/// The kinds of node that carry a stable id (spec §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeKind {
    Section,
    Heading,
    Paragraph,
    List,
    Table,
    TableRow,
    Image,
    Footnote,
    Sheet,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            NodeKind::Section => "section",
            NodeKind::Heading => "heading",
            NodeKind::Paragraph => "paragraph",
            NodeKind::List => "list",
            NodeKind::Table => "table",
            NodeKind::TableRow => "row",
            NodeKind::Image => "image",
            NodeKind::Footnote => "footnote",
            NodeKind::Sheet => "sheet",
        }
    }
}

impl std::fmt::Display for NodeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A node that can be addressed by id.
///
/// Raw fragments are addressable too, but they already own a
/// [`RawId`](crate::image::RawId) allocated by the reader, and reusing it keeps
/// the sidecar file names of Phase 11 stable.
pub trait Addressable {
    fn node_kind(&self) -> NodeKind;
    fn node_id(&self) -> Option<&NodeId>;
    fn set_node_id(&mut self, id: NodeId);
    fn clear_node_id(&mut self);

    /// Feeds the node's normalised content into `hasher`.
    fn hash_content(&self, hasher: &mut EtagHasher);

    /// The etag of this node, derived from its current content.
    fn etag(&self) -> Etag {
        let mut hasher = EtagHasher::new(self.node_kind().as_str());
        self.hash_content(&mut hasher);
        hasher.finish()
    }
}

macro_rules! addressable {
    ($ty:ty, $kind:expr, $hash:expr) => {
        impl Addressable for $ty {
            fn node_kind(&self) -> NodeKind {
                $kind
            }
            fn node_id(&self) -> Option<&NodeId> {
                self.id.as_ref()
            }
            fn set_node_id(&mut self, id: NodeId) {
                self.id = Some(id);
            }
            fn clear_node_id(&mut self) {
                self.id = None;
            }
            fn hash_content(&self, hasher: &mut EtagHasher) {
                #[allow(clippy::redundant_closure_call)]
                ($hash)(self, hasher)
            }
        }
    };
}

addressable!(
    Section,
    NodeKind::Section,
    |s: &Section, h: &mut EtagHasher| { hash_blocks(&s.blocks, h) }
);
addressable!(
    Heading,
    NodeKind::Heading,
    |x: &Heading, h: &mut EtagHasher| {
        h.number(u64::from(x.level));
        hash_inlines(&x.paragraph.content, h);
    }
);
addressable!(
    Paragraph,
    NodeKind::Paragraph,
    |p: &Paragraph, h: &mut EtagHasher| { hash_inlines(&p.content, h) }
);
addressable!(List, NodeKind::List, |l: &List, h: &mut EtagHasher| {
    h.token(if l.ordered { "ordered" } else { "bullet" });
    for item in &l.items {
        h.token("item");
        hash_blocks(&item.blocks, h);
    }
});
addressable!(Table, NodeKind::Table, |t: &Table, h: &mut EtagHasher| {
    for row in &t.rows {
        h.token("row");
        row.hash_content(h);
    }
});
addressable!(
    TableRow,
    NodeKind::TableRow,
    |r: &TableRow, h: &mut EtagHasher| {
        for cell in &r.cells {
            h.token("cell");
            h.number(u64::from(cell.colspan));
            h.number(u64::from(cell.rowspan));
            hash_blocks(&cell.blocks, h);
        }
    }
);
addressable!(
    ImageRef,
    NodeKind::Image,
    |i: &ImageRef, h: &mut EtagHasher| {
        h.token(i.asset.as_str());
        h.text(&i.alt);
    }
);
addressable!(
    Footnote,
    NodeKind::Footnote,
    |f: &Footnote, h: &mut EtagHasher| { hash_blocks(&f.blocks, h) }
);
addressable!(Sheet, NodeKind::Sheet, |s: &Sheet, h: &mut EtagHasher| {
    h.text(&s.name);
    for (at, cell) in &s.cells {
        h.token(&at.a1());
        hash_cell_value(&cell.value, h);
        if let Some(formula) = &cell.formula {
            h.token(&formula.text);
        }
    }
});

/// Assigns ids across a whole document according to `policy`.
///
/// Existing ids are always preserved and always observed, whatever the policy;
/// only [`IdPolicy::Assign`] hands out new ones. The traversal is document
/// order, so the same document always produces the same numbering.
pub fn assign_ids(doc: &mut Document, policy: IdPolicy) {
    let mut addressing = std::mem::take(doc.addressing_mut());
    visit(doc, &mut |node: &mut dyn Addressable| {
        if let Some(id) = node.node_id() {
            addressing.observe(id);
        }
    });
    if policy.assigns() {
        visit(doc, &mut |node: &mut dyn Addressable| {
            if node.node_id().is_none() {
                let id = addressing.alloc();
                node.set_node_id(id);
            }
        });
    }
    *doc.addressing_mut() = addressing;
}

/// Raises the document counter above every id already present, without
/// assigning anything. Readers of DocMark call this after parsing.
pub fn observe_ids(doc: &mut Document) {
    assign_ids(doc, IdPolicy::Preserve);
}

/// Strips every id in the document and resets the counter, which is what
/// `--ids never` needs before serialising.
pub fn clear_ids(doc: &mut Document) {
    visit(doc, &mut |node: &mut dyn Addressable| {
        node.clear_node_id();
    });
    *doc.addressing_mut() = Addressing::default();
}

/// Walks every addressable node of the document in document order, read-only.
///
/// This is what `outline` and any id lookup use; the mutable twin below is
/// reserved for assignment. The two traversals must stay in the same order.
///
/// **A node is addressable only where DocMark can carry its id.** A section is
/// addressed only in a multi-section document, where the serializer emits a
/// `::: {.section}` container, and a table row only inside a complex table,
/// which is the only shape with a `::: {.row}` of its own. Assigning an id that
/// cannot be written would make it change on every round trip, which is worse
/// than not having one.
pub fn for_each_addressable(doc: &Document, f: &mut dyn FnMut(&dyn Addressable)) {
    match doc {
        Document::Text(text) => {
            let addressable_sections = text.sections.len() > 1;
            for section in &text.sections {
                if addressable_sections {
                    f(section);
                }
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
                f(sheet);
                for image in &sheet.images {
                    f(image);
                }
            }
        }
    }
}

/// Every id present in the document, in document order.
pub fn node_ids(doc: &Document) -> Vec<NodeId> {
    let mut ids = Vec::new();
    for_each_addressable(doc, &mut |node| {
        if let Some(id) = node.node_id() {
            ids.push(id.clone());
        }
    });
    ids
}

fn each_blocks(blocks: &[Block], f: &mut dyn FnMut(&dyn Addressable)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                if paragraph_is_container(p) {
                    f(p);
                }
                each_inlines(&p.content, f);
            }
            Block::Heading(h) => {
                f(h);
                each_inlines(&h.paragraph.content, f);
            }
            Block::List(list) => {
                if list_is_addressable(list) {
                    f(list);
                }
                for item in &list.items {
                    each_blocks(&item.blocks, f);
                }
            }
            Block::Table(table) => {
                f(table);
                let addressable_rows = table.is_complex();
                for row in &table.rows {
                    if addressable_rows {
                        f(row);
                    }
                    for cell in &row.cells {
                        each_blocks(&cell.blocks, f);
                    }
                }
            }
            Block::Image(image) => f(image),
            Block::TextBox(text_box) => each_blocks(&text_box.blocks, f),
            Block::Raw(_) => {}
        }
    }
}

fn each_inlines(inlines: &[Inline], f: &mut dyn FnMut(&dyn Addressable)) {
    for inline in inlines {
        match inline {
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                each_inlines(content, f)
            }
            Inline::Footnote(note) => {
                f(note);
                each_blocks(&note.blocks, f);
            }
            // An inline picture is as addressable as a block-level one: it is
            // the same `ImageRef`, and the serializer gives it the same
            // attribute block.
            Inline::Image(image) => f(image.as_ref()),
            _ => {}
        }
    }
}

/// Walks every addressable node of the document in document order.
fn visit(doc: &mut Document, f: &mut dyn FnMut(&mut dyn Addressable)) {
    match doc {
        Document::Text(text) => {
            let addressable_sections = text.sections.len() > 1;
            for section in &mut text.sections {
                if addressable_sections {
                    f(section);
                }
                for header in &mut section.headers {
                    visit_blocks(&mut header.blocks, f);
                }
                for footer in &mut section.footers {
                    visit_blocks(&mut footer.blocks, f);
                }
                visit_blocks(&mut section.blocks, f);
            }
        }
        Document::Workbook(book) => {
            for sheet in &mut book.sheets {
                f(sheet);
                for image in &mut sheet.images {
                    f(image);
                }
            }
        }
    }
}

fn visit_blocks(blocks: &mut [Block], f: &mut dyn FnMut(&mut dyn Addressable)) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                if paragraph_is_container(p) {
                    f(p);
                }
                visit_inlines(&mut p.content, f);
            }
            Block::Heading(h) => {
                f(h);
                visit_inlines(&mut h.paragraph.content, f);
            }
            Block::List(list) => {
                if list_is_addressable(list) {
                    f(list);
                }
                for item in &mut list.items {
                    visit_blocks(&mut item.blocks, f);
                }
            }
            Block::Table(table) => {
                f(table);
                let addressable_rows = table.is_complex();
                for row in &mut table.rows {
                    if addressable_rows {
                        f(row);
                    }
                    for cell in &mut row.cells {
                        visit_blocks(&mut cell.blocks, f);
                    }
                }
            }
            Block::Image(image) => f(image),
            Block::TextBox(text_box) => visit_blocks(&mut text_box.blocks, f),
            Block::Raw(_) => {}
        }
    }
}

fn visit_inlines(inlines: &mut [Inline], f: &mut dyn FnMut(&mut dyn Addressable)) {
    for inline in inlines {
        match inline {
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                visit_inlines(content, f)
            }
            Inline::Footnote(note) => {
                f(note);
                visit_blocks(&mut note.blocks, f);
            }
            Inline::Image(image) => f(image.as_mut()),
            _ => {}
        }
    }
}

/// A list carries its id in the attribute block of its first item, so it needs
/// that item to start with a paragraph — the same condition that already
/// governs where the list definition is named (spec §3.3).
pub fn list_is_addressable(list: &List) -> bool {
    matches!(
        list.items.first().and_then(|item| item.blocks.first()),
        Some(Block::Paragraph(_))
    )
}

/// A paragraph is addressable only when it *contains* something an agent can
/// point at on its own (plan v2 Phase 10.1); a plain run of text is reached by
/// relative path instead, which keeps the id noise out of ordinary prose.
pub fn paragraph_is_container(paragraph: &Paragraph) -> bool {
    fn contains_container(inlines: &[Inline]) -> bool {
        inlines.iter().any(|inline| match inline {
            Inline::Footnote(_) | Inline::Image(_) | Inline::Raw(_) => true,
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                contains_container(content)
            }
            _ => false,
        })
    }
    contains_container(&paragraph.content)
}

fn hash_cell_value(value: &crate::sheet::CellValue, hasher: &mut EtagHasher) {
    use crate::sheet::CellValue;
    match value {
        CellValue::Empty => hasher.token("empty"),
        CellValue::Number(n) => hasher.token(&n.to_string()),
        CellValue::Text(t) => hasher.text(t),
        CellValue::Bool(b) => hasher.token(if *b { "true" } else { "false" }),
        CellValue::DateTime(d) | CellValue::Error(d) => hasher.token(d),
    }
}

fn hash_blocks(blocks: &[Block], hasher: &mut EtagHasher) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                hasher.token("p");
                hash_inlines(&p.content, hasher);
            }
            Block::Heading(h) => {
                hasher.token("h");
                hasher.number(u64::from(h.level));
                hash_inlines(&h.paragraph.content, hasher);
            }
            Block::List(list) => {
                hasher.token("list");
                list.hash_content(hasher);
            }
            Block::Table(table) => {
                hasher.token("table");
                table.hash_content(hasher);
            }
            Block::Image(image) => {
                hasher.token("img");
                image.hash_content(hasher);
            }
            Block::TextBox(text_box) => {
                hasher.token("textbox");
                hash_blocks(&text_box.blocks, hasher);
            }
            Block::Raw(raw) => {
                hasher.token("raw");
                hasher.token(raw.id.as_str());
            }
        }
    }
}

fn hash_inlines(inlines: &[Inline], hasher: &mut EtagHasher) {
    for inline in inlines {
        match inline {
            Inline::Text(text) => hasher.text(text),
            // Formatting is not content: the *shape* of the run tree is hashed,
            // not the properties, so restyling does not churn the etag.
            Inline::Styled { content, .. } => {
                hasher.token("run");
                hash_inlines(content, hasher);
            }
            Inline::Link {
                target, content, ..
            } => {
                hasher.token("link");
                hasher.token(target);
                hash_inlines(content, hasher);
            }
            Inline::Footnote(note) => {
                hasher.token("fn");
                note.hash_content(hasher);
            }
            Inline::Field { kind, cached, .. } => {
                hasher.token("field");
                hasher.token(kind.as_str());
                hasher.text(cached);
            }
            Inline::Break(kind) => hasher.token(kind.as_str()),
            Inline::Image(image) => {
                hasher.token("img");
                image.hash_content(hasher);
            }
            Inline::Raw(raw) => {
                hasher.token("raw");
                hasher.token(raw.id.as_str());
            }
        }
    }
}
