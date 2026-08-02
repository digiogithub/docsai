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
use crate::text::{Block, Inline, TextDocument};
use crate::Document;

/// A violated invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "error")]
pub enum ValidationError {
    /// A spreadsheet anchor was found in a text document.
    SheetAnchorInTextDocument { location: String, anchor: String },
    /// A document anchor was found on a sheet image.
    DocumentAnchorInSheet { sheet: String, anchor: String },
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

fn validate_text(doc: &TextDocument, errors: &mut Vec<ValidationError>) {
    for (si, section) in doc.sections.iter().enumerate() {
        let path = format!("section {si}");
        walk_blocks(&section.blocks, &path, errors);
        for h in &section.headers {
            walk_blocks(
                &h.blocks,
                &format!("{path} header {}", h.scope.as_str()),
                errors,
            );
        }
        for f in &section.footers {
            walk_blocks(
                &f.blocks,
                &format!("{path} footer {}", f.scope.as_str()),
                errors,
            );
        }
    }
}

fn walk_blocks(blocks: &[Block], path: &str, errors: &mut Vec<ValidationError>) {
    for (bi, block) in blocks.iter().enumerate() {
        let path = format!("{path}/block {bi}");
        match block {
            Block::Paragraph(p) => walk_inlines(&p.content, &path, errors),
            Block::Heading(h) => walk_inlines(&h.paragraph.content, &path, errors),
            Block::Image(image) => check_image(image, &path, errors),
            Block::List(list) => {
                for (ii, item) in list.items.iter().enumerate() {
                    walk_blocks(&item.blocks, &format!("{path}/item {ii}"), errors);
                }
            }
            Block::Table(table) => {
                let grid = table.width();
                for (ri, row) in table.rows.iter().enumerate() {
                    let width: usize = row.cells.iter().map(|c| c.colspan.max(1) as usize).sum();
                    if width > grid {
                        errors.push(ValidationError::RowWiderThanGrid {
                            table: path.clone(),
                            row: ri,
                            width,
                            grid,
                        });
                    }
                    for (ci, cell) in row.cells.iter().enumerate() {
                        walk_blocks(&cell.blocks, &format!("{path}/r{ri}c{ci}"), errors);
                    }
                }
            }
            Block::TextBox(tb) => walk_blocks(&tb.blocks, &path, errors),
            Block::Raw(_) => {}
        }
    }
}

fn walk_inlines(inlines: &[Inline], path: &str, errors: &mut Vec<ValidationError>) {
    for inline in inlines {
        match inline {
            Inline::Image(image) => check_image(image, path, errors),
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                walk_inlines(content, path, errors)
            }
            Inline::Footnote(blocks) => walk_blocks(blocks, &format!("{path}/footnote"), errors),
            _ => {}
        }
    }
}

fn check_image(image: &ImageRef, path: &str, errors: &mut Vec<ValidationError>) {
    if image.geometry.anchor.is_sheet() {
        errors.push(ValidationError::SheetAnchorInTextDocument {
            location: path.to_string(),
            anchor: image.geometry.anchor.keyword().into(),
        });
    }
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
    use crate::text::{Paragraph, Section};
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
                    vec![Block::Image(image_with(anchor))],
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
