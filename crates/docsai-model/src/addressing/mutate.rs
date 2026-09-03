//! Writing where a [`Target`](crate::addressing::Target) points.
//!
//! # Why this module exists instead of a second resolver
//!
//! 17-B's [`resolve`](crate::addressing::resolve) hands back a **shared**
//! borrow, on purpose: an increment that could not mutate could not break a
//! document. 17-C has to mutate, and there were three ways to get a mutable
//! place:
//!
//! 1. **Resolve again, mutably.** A `resolve_mut` returning a `PlaceMut` means
//!    a second copy of every rule in §5 — what `doc` means, which walk finds an
//!    id, which sequences an index addresses, which failure each miss is. Two
//!    copies of a resolution policy drift, and the day they drift the read path
//!    says `n12` is one node and the write path edits another. That is the
//!    exact failure ids exist to prevent, arrived at through the back door.
//! 2. **Rebuild the document functionally**, mapping the tree to a new tree.
//!    It costs a full clone of every document on every operation, and it throws
//!    away the identity of everything it copies — the `Addressing` counter and
//!    the `RawId`/`AssetId` links survive only because somebody remembered to
//!    carry them.
//! 3. **Record the route and replay it.** Resolution stays the single
//!    implementation it was; it additionally reports the [`Step`] path it walked
//!    ([`resolve_path`](crate::addressing::resolve_path)), and this module
//!    replays that path against `&mut Document`.
//!
//! **Chosen: (3).** The cost is honest and bounded: one `Vec<Step>` per
//! resolution — a dozen machine words for a target six levels deep — and one
//! `match` arm here per `(cursor, step)` pair the read walk can produce. The
//! *policy* has exactly one home, and the mutable half carries none: it cannot
//! decide that `doc/3` means something, it can only walk where it was told.
//! When a step and a cursor do not fit, the answer is `None` and the caller
//! refuses; nothing is guessed.
//!
//! The two shapes a write needs are both here, because operations need both:
//!
//! - [`with_place_mut`] — the node itself, for the operations that change what
//!   a node *says* (`replace_text`, `set_props`, `set_cell`, `set_notes`,
//!   `replace_image`).
//! - [`with_seq_mut`] — the parent sequence and the position in it, for the
//!   operations that change what a document *contains* (`insert_after`,
//!   `insert_before`, `delete`, `move`, `duplicate_slide`).
//!
//! Both are continuation-passing. A function returning `PlaceMut<'a>` would
//! have to name the borrow of the document it came from and thread it through
//! every caller; a closure gets the same borrow for exactly as long as it needs
//! it, and the recursion that produces it stays four lines long.

use crate::addressing::target::Step;
use crate::image::ImageRef;
use crate::presentation::{Shape, ShapeKind, Slide};
use crate::sheet::Sheet;
use crate::text::{Block, Comment, Footnote, Inline, ListItem, Section, TableCell, TableRow};
use crate::Document;

/// The mutable twin of [`Place`](crate::addressing::Place).
///
/// It has no `Inline` variant for the same reason `Place` has none: a target
/// never resolves to a bare inline. An inline is walked *through* — to a
/// footnote, a comment or an image — and that intermediate cursor is private.
#[derive(Debug)]
pub enum PlaceMut<'a> {
    Doc(&'a mut Document),
    Section(&'a mut Section),
    Block(&'a mut Block),
    ListItem(&'a mut ListItem),
    Row(&'a mut TableRow),
    Cell(&'a mut TableCell),
    Sheet(&'a mut Sheet),
    Slide(&'a mut Slide),
    Shape(&'a mut Shape),
    Footnote(&'a mut Footnote),
    Comment(&'a mut Comment),
    Image(&'a mut ImageRef),
}

/// A child sequence of the IR, mutably, with the kind it holds.
///
/// `insert_after` and friends need to know what a sequence *takes* before they
/// parse a fragment for it (§6.3: «the fragment's kind must match the
/// sequence»), which is why the kind is a value and not a comment.
#[derive(Debug)]
pub enum SeqMut<'a> {
    Sections(&'a mut Vec<Section>),
    Blocks(&'a mut Vec<Block>),
    Inlines(&'a mut Vec<Inline>),
    ListItems(&'a mut Vec<ListItem>),
    Rows(&'a mut Vec<TableRow>),
    Cells(&'a mut Vec<TableCell>),
    Sheets(&'a mut Vec<Sheet>),
    Slides(&'a mut Vec<Slide>),
    Shapes(&'a mut Vec<Shape>),
    Comments(&'a mut Vec<Comment>),
    Images(&'a mut Vec<ImageRef>),
}

/// What a [`SeqMut`] holds, without borrowing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqKind {
    Sections,
    Blocks,
    Inlines,
    ListItems,
    Rows,
    Cells,
    Sheets,
    Slides,
    Shapes,
    Comments,
    Images,
}

impl SeqKind {
    /// The word an error message uses for what this sequence holds.
    pub fn as_str(self) -> &'static str {
        match self {
            SeqKind::Sections => "sections",
            SeqKind::Blocks => "blocks",
            SeqKind::Inlines => "inline content",
            SeqKind::ListItems => "list items",
            SeqKind::Rows => "rows",
            SeqKind::Cells => "cells",
            SeqKind::Sheets => "sheets",
            SeqKind::Slides => "slides",
            SeqKind::Shapes => "shapes",
            SeqKind::Comments => "comments",
            SeqKind::Images => "images",
        }
    }
}

impl std::fmt::Display for SeqKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl SeqMut<'_> {
    pub fn kind(&self) -> SeqKind {
        match self {
            SeqMut::Sections(_) => SeqKind::Sections,
            SeqMut::Blocks(_) => SeqKind::Blocks,
            SeqMut::Inlines(_) => SeqKind::Inlines,
            SeqMut::ListItems(_) => SeqKind::ListItems,
            SeqMut::Rows(_) => SeqKind::Rows,
            SeqMut::Cells(_) => SeqKind::Cells,
            SeqMut::Sheets(_) => SeqKind::Sheets,
            SeqMut::Slides(_) => SeqKind::Slides,
            SeqMut::Shapes(_) => SeqKind::Shapes,
            SeqMut::Comments(_) => SeqKind::Comments,
            SeqMut::Images(_) => SeqKind::Images,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            SeqMut::Sections(v) => v.len(),
            SeqMut::Blocks(v) => v.len(),
            SeqMut::Inlines(v) => v.len(),
            SeqMut::ListItems(v) => v.len(),
            SeqMut::Rows(v) => v.len(),
            SeqMut::Cells(v) => v.len(),
            SeqMut::Sheets(v) => v.len(),
            SeqMut::Slides(v) => v.len(),
            SeqMut::Shapes(v) => v.len(),
            SeqMut::Comments(v) => v.len(),
            SeqMut::Images(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A cursor mid-walk: a place, or an inline on the way to one.
enum Cursor<'a> {
    Place(PlaceMut<'a>),
    Inline(&'a mut Inline),
}

/// Runs `f` on the place `path` names, or returns `None` when the path does not
/// fit the document — which, for a path that came out of `resolve_path` against
/// this same document, cannot happen.
pub fn with_place_mut<R>(
    doc: &mut Document,
    path: &[Step],
    f: &mut dyn FnMut(PlaceMut<'_>) -> R,
) -> Option<R> {
    walk(
        Cursor::Place(PlaceMut::Doc(doc)),
        path,
        &mut |cursor| match cursor {
            Cursor::Place(place) => Some(f(place)),
            Cursor::Inline(_) => None,
        },
    )
}

/// Runs `f` on the sequence that *contains* what `path` names, with the
/// position in it.
///
/// `None` for the empty path (the document root is in no sequence) and for a
/// path whose last step is not a position in a sequence — the three `Into*`
/// steps, which step from an inline to the thing it holds and move nothing.
pub fn with_seq_mut<R>(
    doc: &mut Document,
    path: &[Step],
    f: &mut dyn FnMut(SeqMut<'_>, usize) -> R,
) -> Option<R> {
    let (last, parent) = path.split_last()?;
    let last = *last;
    walk(Cursor::Place(PlaceMut::Doc(doc)), parent, &mut |cursor| {
        let (seq, index) = seq_of(cursor, last)?;
        Some(f(seq, index))
    })
}

fn walk<'d, R>(
    cursor: Cursor<'d>,
    path: &[Step],
    f: &mut dyn FnMut(Cursor<'_>) -> Option<R>,
) -> Option<R> {
    match path.split_first() {
        None => f(cursor),
        Some((step, rest)) => walk(step_into(cursor, *step)?, rest, f),
    }
}

/// One hop. Every arm mirrors an arm of the read walk in `target.rs`; a pair
/// the read walk cannot produce is `None` rather than a panic, because this
/// function is also reachable with a hand-built path.
fn step_into<'d>(cursor: Cursor<'d>, step: Step) -> Option<Cursor<'d>> {
    let place = match cursor {
        Cursor::Inline(inline) => {
            return Some(match (inline, step) {
                (Inline::Styled { content, .. }, Step::Inline(k))
                | (Inline::Link { content, .. }, Step::Inline(k)) => {
                    Cursor::Inline(content.get_mut(k)?)
                }
                (Inline::Footnote(note), Step::IntoFootnote) => {
                    Cursor::Place(PlaceMut::Footnote(note))
                }
                (Inline::Comment(comment), Step::IntoComment) => {
                    Cursor::Place(PlaceMut::Comment(comment.as_mut()))
                }
                (Inline::Image(image), Step::IntoImage) => {
                    Cursor::Place(PlaceMut::Image(image.as_mut()))
                }
                _ => return None,
            })
        }
        Cursor::Place(place) => place,
    };
    Some(match place {
        PlaceMut::Doc(doc) => match (doc, step) {
            (Document::Text(text), Step::Section(i)) => {
                Cursor::Place(PlaceMut::Section(text.sections.get_mut(i)?))
            }
            (Document::Workbook(book), Step::Sheet(i)) => {
                Cursor::Place(PlaceMut::Sheet(book.sheets.get_mut(i)?))
            }
            (Document::Presentation(deck), Step::Slide(i)) => {
                Cursor::Place(PlaceMut::Slide(deck.slides.get_mut(i)?))
            }
            _ => return None,
        },
        PlaceMut::Section(section) => match step {
            Step::Block(k) => Cursor::Place(PlaceMut::Block(section.blocks.get_mut(k)?)),
            Step::HeaderBlock(j, k) => Cursor::Place(PlaceMut::Block(
                section.headers.get_mut(j)?.blocks.get_mut(k)?,
            )),
            Step::FooterBlock(j, k) => Cursor::Place(PlaceMut::Block(
                section.footers.get_mut(j)?.blocks.get_mut(k)?,
            )),
            _ => return None,
        },
        PlaceMut::Block(block) => match (block, step) {
            (Block::Paragraph(p), Step::Inline(k)) => Cursor::Inline(p.content.get_mut(k)?),
            (Block::Heading(h), Step::Inline(k)) => Cursor::Inline(h.paragraph.content.get_mut(k)?),
            (Block::List(list), Step::ListItem(k)) => {
                Cursor::Place(PlaceMut::ListItem(list.items.get_mut(k)?))
            }
            (Block::Table(table), Step::Row(k)) => {
                Cursor::Place(PlaceMut::Row(table.rows.get_mut(k)?))
            }
            (Block::TextBox(text_box), Step::Block(k)) => {
                Cursor::Place(PlaceMut::Block(text_box.blocks.get_mut(k)?))
            }
            _ => return None,
        },
        PlaceMut::ListItem(item) => match step {
            Step::Block(k) => Cursor::Place(PlaceMut::Block(item.blocks.get_mut(k)?)),
            _ => return None,
        },
        PlaceMut::Row(row) => match step {
            Step::Cell(k) => Cursor::Place(PlaceMut::Cell(row.cells.get_mut(k)?)),
            _ => return None,
        },
        PlaceMut::Cell(cell) => match step {
            Step::Block(k) => Cursor::Place(PlaceMut::Block(cell.blocks.get_mut(k)?)),
            _ => return None,
        },
        PlaceMut::Sheet(sheet) => match step {
            Step::SheetImage(k) => Cursor::Place(PlaceMut::Image(sheet.images.get_mut(k)?)),
            _ => return None,
        },
        PlaceMut::Slide(slide) => match step {
            Step::Shape(k) => Cursor::Place(PlaceMut::Shape(slide.shapes.get_mut(k)?)),
            Step::NotesBlock(k) => {
                Cursor::Place(PlaceMut::Block(slide.notes.as_mut()?.get_mut(k)?))
            }
            Step::Comment(k) => Cursor::Place(PlaceMut::Comment(slide.comments.get_mut(k)?)),
            _ => return None,
        },
        PlaceMut::Shape(shape) => match (&mut shape.kind, step) {
            (ShapeKind::Placeholder(ph), Step::Block(k)) => {
                Cursor::Place(PlaceMut::Block(ph.body.get_mut(k)?))
            }
            (ShapeKind::TextBox { body }, Step::Block(k)) => {
                Cursor::Place(PlaceMut::Block(body.get_mut(k)?))
            }
            (ShapeKind::Table(table), Step::Row(k)) => {
                Cursor::Place(PlaceMut::Row(table.rows.get_mut(k)?))
            }
            (ShapeKind::Group(children), Step::Shape(k)) => {
                Cursor::Place(PlaceMut::Shape(children.get_mut(k)?))
            }
            (ShapeKind::Picture(image), Step::PictureImage) => {
                Cursor::Place(PlaceMut::Image(image))
            }
            _ => return None,
        },
        PlaceMut::Footnote(note) => match step {
            Step::Block(k) => Cursor::Place(PlaceMut::Block(note.blocks.get_mut(k)?)),
            _ => return None,
        },
        PlaceMut::Comment(comment) => match step {
            Step::Block(k) => Cursor::Place(PlaceMut::Block(comment.blocks.get_mut(k)?)),
            Step::CommentCovered(k) => Cursor::Inline(comment.covered_mut()?.get_mut(k)?),
            Step::Comment(k) => Cursor::Place(PlaceMut::Comment(comment.replies.get_mut(k)?)),
            _ => return None,
        },
        PlaceMut::Image(_) => return None,
    })
}

/// The sequence a final step indexes into, and the position.
fn seq_of<'d>(cursor: Cursor<'d>, step: Step) -> Option<(SeqMut<'d>, usize)> {
    let place = match cursor {
        Cursor::Inline(inline) => {
            return match (inline, step) {
                (Inline::Styled { content, .. }, Step::Inline(k))
                | (Inline::Link { content, .. }, Step::Inline(k)) => {
                    Some((SeqMut::Inlines(content), k))
                }
                _ => None,
            }
        }
        Cursor::Place(place) => place,
    };
    Some(match place {
        PlaceMut::Doc(doc) => match (doc, step) {
            (Document::Text(text), Step::Section(i)) => (SeqMut::Sections(&mut text.sections), i),
            (Document::Workbook(book), Step::Sheet(i)) => (SeqMut::Sheets(&mut book.sheets), i),
            (Document::Presentation(deck), Step::Slide(i)) => (SeqMut::Slides(&mut deck.slides), i),
            _ => return None,
        },
        PlaceMut::Section(section) => match step {
            Step::Block(k) => (SeqMut::Blocks(&mut section.blocks), k),
            Step::HeaderBlock(j, k) => (SeqMut::Blocks(&mut section.headers.get_mut(j)?.blocks), k),
            Step::FooterBlock(j, k) => (SeqMut::Blocks(&mut section.footers.get_mut(j)?.blocks), k),
            _ => return None,
        },
        PlaceMut::Block(block) => match (block, step) {
            (Block::Paragraph(p), Step::Inline(k)) => (SeqMut::Inlines(&mut p.content), k),
            (Block::Heading(h), Step::Inline(k)) => (SeqMut::Inlines(&mut h.paragraph.content), k),
            (Block::List(list), Step::ListItem(k)) => (SeqMut::ListItems(&mut list.items), k),
            (Block::Table(table), Step::Row(k)) => (SeqMut::Rows(&mut table.rows), k),
            (Block::TextBox(text_box), Step::Block(k)) => (SeqMut::Blocks(&mut text_box.blocks), k),
            _ => return None,
        },
        PlaceMut::ListItem(item) => match step {
            Step::Block(k) => (SeqMut::Blocks(&mut item.blocks), k),
            _ => return None,
        },
        PlaceMut::Row(row) => match step {
            Step::Cell(k) => (SeqMut::Cells(&mut row.cells), k),
            _ => return None,
        },
        PlaceMut::Cell(cell) => match step {
            Step::Block(k) => (SeqMut::Blocks(&mut cell.blocks), k),
            _ => return None,
        },
        PlaceMut::Sheet(sheet) => match step {
            Step::SheetImage(k) => (SeqMut::Images(&mut sheet.images), k),
            _ => return None,
        },
        PlaceMut::Slide(slide) => match step {
            Step::Shape(k) => (SeqMut::Shapes(&mut slide.shapes), k),
            Step::NotesBlock(k) => (SeqMut::Blocks(slide.notes.as_mut()?), k),
            Step::Comment(k) => (SeqMut::Comments(&mut slide.comments), k),
            _ => return None,
        },
        PlaceMut::Shape(shape) => match (&mut shape.kind, step) {
            (ShapeKind::Placeholder(ph), Step::Block(k)) => (SeqMut::Blocks(&mut ph.body), k),
            (ShapeKind::TextBox { body }, Step::Block(k)) => (SeqMut::Blocks(body), k),
            (ShapeKind::Table(table), Step::Row(k)) => (SeqMut::Rows(&mut table.rows), k),
            (ShapeKind::Group(children), Step::Shape(k)) => (SeqMut::Shapes(children), k),
            _ => return None,
        },
        PlaceMut::Footnote(note) => match step {
            Step::Block(k) => (SeqMut::Blocks(&mut note.blocks), k),
            _ => return None,
        },
        PlaceMut::Comment(comment) => match step {
            Step::Block(k) => (SeqMut::Blocks(&mut comment.blocks), k),
            Step::CommentCovered(k) => (SeqMut::Inlines(comment.covered_mut()?), k),
            Step::Comment(k) => (SeqMut::Comments(&mut comment.replies), k),
            _ => return None,
        },
        PlaceMut::Image(_) => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressing::{resolve_path, Target};
    use crate::text::{Paragraph, Section, TextDocument};

    fn doc() -> Document {
        let mut first = Paragraph::text("uno");
        first.id = Some(crate::NodeId::new("n1"));
        let mut second = Paragraph::text("dos");
        second.id = Some(crate::NodeId::new("n2"));
        Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![Block::Paragraph(first), Block::Paragraph(second)],
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    #[test]
    fn the_route_resolution_recorded_reaches_the_same_node_mutably() {
        let mut document = doc();
        let target = Target::parse("n2").expect("a target");
        let path = {
            let (place, path) = resolve_path(&document, &target).expect("resolves");
            assert_eq!(place.id().map(|i| i.0.clone()), Some("n2".to_string()));
            path
        };
        let changed = with_place_mut(&mut document, &path, &mut |place| match place {
            PlaceMut::Block(Block::Paragraph(p)) => {
                p.content = vec![Inline::Text("DOS".into())];
                true
            }
            _ => false,
        });
        assert_eq!(changed, Some(true));
        let (place, _) = resolve_path(&document, &target).expect("resolves");
        assert_eq!(place.describe(), "paragraph");
        match place {
            crate::addressing::Place::Block(Block::Paragraph(p)) => {
                assert_eq!(p.plain_text(), "DOS")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_parent_sequence_of_a_block_is_the_sections_blocks() {
        let mut document = doc();
        let (_, path) =
            resolve_path(&document, &Target::parse("n1").expect("target")).expect("resolves");
        let taken = with_seq_mut(&mut document, &path, &mut |seq, index| {
            assert_eq!(index, 0);
            assert_eq!(seq.kind(), SeqKind::Blocks);
            seq.len()
        });
        assert_eq!(taken, Some(2));
    }

    #[test]
    fn the_document_root_is_in_no_sequence() {
        let mut document = doc();
        assert!(with_seq_mut(&mut document, &[], &mut |_, _| ()).is_none());
    }
}
