//! IR invariant checking.
//!
//! The invariants a reader can violate but the writers rely on. Chief among
//! them (architecture §3.1): sheet anchors are only legal inside a
//! [`crate::sheet::Workbook`], and document anchors only inside a
//! [`crate::text::TextDocument`]. Cross-anchoring does not
//! exist, and the validator is what makes that a checked rule instead of a
//! comment.

use serde::{Deserialize, Serialize};

use crate::image::{Anchor, ImageRef};
use crate::presentation::{Presentation, Shape, ShapeKind};
use crate::text::{Block, Inline, Table, TextDocument};
use crate::Document;

/// A violated invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "error")]
pub enum ValidationError {
    /// A spreadsheet anchor was found in a text document.
    SheetAnchorInTextDocument { location: String, anchor: String },
    /// A document anchor was found on a sheet image.
    DocumentAnchorInSheet { sheet: String, anchor: String },
    /// A spreadsheet anchor was found on a slide. A shape is positioned on the
    /// canvas, never against a cell.
    SheetAnchorInPresentation { location: String, anchor: String },
    /// A table row has more cells than the table's grid.
    RowWiderThanGrid {
        table: String,
        row: usize,
        width: usize,
        grid: usize,
    },
    /// A `two-cell` anchor whose end is before its start.
    InvertedCellAnchor { location: String, range: String },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::SheetAnchorInTextDocument { location, anchor } => write!(
                f,
                "sheet anchor `{anchor}` on an image of a text document, at {location}"
            ),
            ValidationError::DocumentAnchorInSheet { sheet, anchor } => write!(
                f,
                "document anchor `{anchor}` on an image of sheet `{sheet}`"
            ),
            ValidationError::SheetAnchorInPresentation { location, anchor } => write!(
                f,
                "sheet anchor `{anchor}` on an image of a presentation, at {location}"
            ),
            ValidationError::RowWiderThanGrid {
                table,
                row,
                width,
                grid,
            } => write!(
                f,
                "table {table} row {row} spans {width} columns but the grid has {grid}"
            ),
            ValidationError::InvertedCellAnchor { location, range } => {
                write!(
                    f,
                    "two-cell anchor {range} ends before it starts, at {location}"
                )
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Checks every invariant, returning all violations rather than the first.
pub fn validate(doc: &Document) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    match doc {
        Document::Text(text) => validate_text(text, &mut errors),
        Document::Presentation(deck) => validate_presentation(deck, &mut errors),
        Document::Workbook(book) => {
            for sheet in &book.sheets {
                for image in &sheet.images {
                    if !image.geometry.anchor.is_sheet() {
                        errors.push(ValidationError::DocumentAnchorInSheet {
                            sheet: sheet.name.clone(),
                            anchor: image.geometry.anchor.keyword().into(),
                        });
                    }
                    check_two_cell(image, &format!("sheet {}", sheet.name), &mut errors);
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Which root the blocks being walked belong to. The anchor rules differ per
/// root (architecture §3.1) and so does the error that names the violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Root {
    Text,
    Presentation,
}

fn validate_presentation(deck: &Presentation, errors: &mut Vec<ValidationError>) {
    for (si, slide) in deck.slides.iter().enumerate() {
        let path = format!("slide {si}");
        for shape in &slide.shapes {
            walk_shape(shape, &path, errors);
        }
        if let Some(notes) = &slide.notes {
            walk_blocks(notes, &format!("{path}/notes"), Root::Presentation, errors);
        }
    }
}

fn walk_shape(shape: &Shape, path: &str, errors: &mut Vec<ValidationError>) {
    let path = format!("{path}/shape {}", shape.z_index);
    match &shape.kind {
        ShapeKind::Placeholder(ph) => walk_blocks(&ph.body, &path, Root::Presentation, errors),
        ShapeKind::TextBox { body } => walk_blocks(body, &path, Root::Presentation, errors),
        ShapeKind::Picture(image) => check_image(image, &path, Root::Presentation, errors),
        ShapeKind::Table(table) => check_table(table, &path, Root::Presentation, errors),
        ShapeKind::Group(children) => {
            for child in children {
                walk_shape(child, &path, errors);
            }
        }
        ShapeKind::Chart(_) | ShapeKind::Raw(_) => {}
    }
}

fn validate_text(doc: &TextDocument, errors: &mut Vec<ValidationError>) {
    for (si, section) in doc.sections.iter().enumerate() {
        let path = format!("section {si}");
        walk_blocks(&section.blocks, &path, Root::Text, errors);
        for h in &section.headers {
            walk_blocks(
                &h.blocks,
                &format!("{path} header {}", h.scope.as_str()),
                Root::Text,
                errors,
            );
        }
        for f in &section.footers {
            walk_blocks(
                &f.blocks,
                &format!("{path} footer {}", f.scope.as_str()),
                Root::Text,
                errors,
            );
        }
    }
}

fn walk_blocks(blocks: &[Block], path: &str, root: Root, errors: &mut Vec<ValidationError>) {
    for (bi, block) in blocks.iter().enumerate() {
        let path = format!("{path}/block {bi}");
        match block {
            Block::Paragraph(p) => walk_inlines(&p.content, &path, root, errors),
            Block::Heading(h) => walk_inlines(&h.paragraph.content, &path, root, errors),
            Block::Image(image) => check_image(image, &path, root, errors),
            Block::List(list) => {
                for (ii, item) in list.items.iter().enumerate() {
                    walk_blocks(&item.blocks, &format!("{path}/item {ii}"), root, errors);
                }
            }
            Block::Table(table) => check_table(table, &path, root, errors),
            Block::TextBox(tb) => walk_blocks(&tb.blocks, &path, root, errors),
            Block::Raw(_) => {}
        }
    }
}

fn check_table(table: &Table, path: &str, root: Root, errors: &mut Vec<ValidationError>) {
    let grid = table.width();
    for (ri, row) in table.rows.iter().enumerate() {
        let width: usize = row.cells.iter().map(|c| c.colspan.max(1) as usize).sum();
        if width > grid {
            errors.push(ValidationError::RowWiderThanGrid {
                table: path.to_string(),
                row: ri,
                width,
                grid,
            });
        }
        for (ci, cell) in row.cells.iter().enumerate() {
            walk_blocks(&cell.blocks, &format!("{path}/r{ri}c{ci}"), root, errors);
        }
    }
}

fn walk_inlines(inlines: &[Inline], path: &str, root: Root, errors: &mut Vec<ValidationError>) {
    for inline in inlines {
        match inline {
            Inline::Image(image) => check_image(image, path, root, errors),
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                walk_inlines(content, path, root, errors)
            }
            Inline::Footnote(note) => {
                walk_blocks(&note.blocks, &format!("{path}/footnote"), root, errors)
            }
            _ => {}
        }
    }
}

fn check_image(image: &ImageRef, path: &str, root: Root, errors: &mut Vec<ValidationError>) {
    if !image.geometry.anchor.is_sheet() {
        return;
    }
    let location = path.to_string();
    let anchor = image.geometry.anchor.keyword().into();
    errors.push(match root {
        Root::Text => ValidationError::SheetAnchorInTextDocument { location, anchor },
        Root::Presentation => ValidationError::SheetAnchorInPresentation { location, anchor },
    });
}

fn check_two_cell(image: &ImageRef, path: &str, errors: &mut Vec<ValidationError>) {
    if let Anchor::SheetTwoCell { from, to, .. } = &image.geometry.anchor {
        if to.cell.col < from.cell.col || to.cell.row < from.cell.row {
            errors.push(ValidationError::InvertedCellAnchor {
                location: path.to_string(),
                range: format!("{}:{}", from.cell.a1(), to.cell.a1()),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::AssetId;
    use crate::image::{CellAnchor, ImageGeometry};
    use crate::sheet::{CellRef, Sheet, Workbook};
    use crate::text::{Footnote, Paragraph, Section};
    use crate::units::{Length, Size};

    fn image_with(anchor: Anchor) -> ImageRef {
        let mut geometry = ImageGeometry::inline(Size::new(Length::ZERO, Length::ZERO));
        geometry.anchor = anchor;
        ImageRef::new(AssetId::new("a"), geometry)
    }

    #[test]
    fn a_plain_text_document_validates() {
        let doc = Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![Block::Paragraph(Paragraph::text("hola"))],
                ..Default::default()
            }],
            ..Default::default()
        });
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn sheet_anchors_are_rejected_in_text_documents() {
        let anchor = Anchor::SheetOneCell {
            from: CellAnchor::new(CellRef::new(0, 0), Length::ZERO, Length::ZERO),
        };
        let doc = Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![Block::Image(image_with(anchor))],
                ..Default::default()
            }],
            ..Default::default()
        });
        let errors = validate(&doc).unwrap_err();
        assert!(matches!(
            errors[0],
            ValidationError::SheetAnchorInTextDocument { .. }
        ));
    }

    #[test]
    fn nested_images_are_reached_too() {
        let anchor = Anchor::SheetAbsolute {
            pos: Default::default(),
        };
        let doc = Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![Block::Paragraph(Paragraph::new(vec![Inline::Footnote(
                    Footnote::new(vec![Block::Image(image_with(anchor))]),
                )]))],
                ..Default::default()
            }],
            ..Default::default()
        });
        assert_eq!(validate(&doc).unwrap_err().len(), 1);
    }

    #[test]
    fn document_anchors_are_rejected_on_sheets() {
        let mut sheet = Sheet::new("Hoja");
        sheet.images.push(image_with(Anchor::Inline));
        let doc = Document::Workbook(Workbook {
            sheets: vec![sheet],
            ..Default::default()
        });
        let errors = validate(&doc).unwrap_err();
        assert!(matches!(
            errors[0],
            ValidationError::DocumentAnchorInSheet { .. }
        ));
    }

    #[test]
    fn sheet_anchors_are_rejected_on_slides_too() {
        use crate::presentation::{Placeholder, Presentation, Shape, ShapeKind, Slide};
        let anchor = Anchor::SheetOneCell {
            from: CellAnchor::new(CellRef::new(0, 0), Length::ZERO, Length::ZERO),
        };
        let doc = Document::Presentation(Presentation {
            slides: vec![Slide {
                shapes: vec![
                    Shape::new(0, ShapeKind::Picture(image_with(anchor.clone()))),
                    Shape::new(
                        1,
                        ShapeKind::Placeholder(Placeholder {
                            body: vec![Block::Image(image_with(anchor))],
                            ..Default::default()
                        }),
                    ),
                ],
                ..Default::default()
            }],
            ..Default::default()
        });
        let errors = validate(&doc).unwrap_err();
        assert_eq!(errors.len(), 2, "both the shape and the block are checked");
        assert!(errors
            .iter()
            .all(|e| matches!(e, ValidationError::SheetAnchorInPresentation { .. })));
    }

    #[test]
    fn a_plain_presentation_validates() {
        use crate::presentation::{Presentation, Slide};
        let doc = Document::Presentation(Presentation {
            slides: vec![Slide::default()],
            ..Default::default()
        });
        assert!(validate(&doc).is_ok());
    }

    #[test]
    fn inverted_two_cell_anchors_are_caught() {
        let anchor = Anchor::SheetTwoCell {
            from: CellAnchor::new(CellRef::new(3, 8), Length::ZERO, Length::ZERO),
            to: CellAnchor::new(CellRef::new(1, 2), Length::ZERO, Length::ZERO),
            move_with_cells: true,
            size_with_cells: false,
        };
        let mut sheet = Sheet::new("Hoja");
        sheet.images.push(image_with(anchor));
        let doc = Document::Workbook(Workbook {
            sheets: vec![sheet],
            ..Default::default()
        });
        let errors = validate(&doc).unwrap_err();
        assert!(matches!(
            errors[0],
            ValidationError::InvertedCellAnchor { .. }
        ));
    }
}
