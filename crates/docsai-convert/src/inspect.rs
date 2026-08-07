//! Document inspection without a full conversion.
//!
//! Builds the structure JSON shared by `docsai inspect --json` and the future
//! MCP `inspect_document` tool (architecture §5–§6).

use docsai_model::assets::AssetStore;
use docsai_model::image::ImageRef;
use docsai_model::text::{Block, DocumentMeta, HeaderFooter, Section, TextDocument};
use docsai_model::{ConversionReport, ConversionStats, Document, Format, Warning};
use serde::Serialize;

use crate::pipeline::{read_path_with_options, ConvertOptions};
use crate::ConvertError;
use std::path::Path;

/// Structured view of a document for humans and machines.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct InspectReport {
    /// Input path when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub source_format: String,
    /// `"text"` or `"workbook"`.
    pub kind: &'static str,
    pub meta: DocumentMeta,
    pub styles: Vec<StyleSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<Vec<SectionSummary>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheets: Option<Vec<SheetSummary>>,
    pub media: Vec<MediaSummary>,
    pub stats: ConversionStats,
    pub warnings: Vec<Warning>,
}

/// One named style from the catalogue.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct StyleSummary {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub style_type: String,
    pub is_default: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<String>,
}

/// High-level section facts for a text document.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SectionSummary {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper: Option<String>,
    pub orientation: String,
    pub columns: u16,
    pub headers: usize,
    pub footers: usize,
    pub blocks: usize,
}

/// High-level sheet facts for a workbook.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SheetSummary {
    pub name: String,
    pub cells: usize,
    pub formulas: usize,
    pub merges: usize,
    pub images: usize,
    pub hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used_range: Option<String>,
}

/// One media asset present in the package.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct MediaSummary {
    pub id: String,
    pub file_name: String,
    pub content_type: String,
    pub byte_len: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub native_size_px: Option<(u32, u32)>,
    /// How many times the asset is referenced from the body.
    pub references: usize,
}

/// Reads `input` and returns an inspection report without writing DocMark.
pub fn inspect_path(input: &Path, options: &ConvertOptions) -> Result<InspectReport, ConvertError> {
    let mut store = docsai_model::MemoryAssetStore::new();
    let (document, format, report) = read_path_with_options(input, &mut store, options)?;
    Ok(build_report(
        Some(input.display().to_string()),
        format,
        &document,
        &store,
        report,
    ))
}

/// Builds an [`InspectReport`] from an already-loaded document.
pub fn build_report(
    path: Option<String>,
    source_format: Format,
    document: &Document,
    assets: &dyn AssetStore,
    report: ConversionReport,
) -> InspectReport {
    let styles = document
        .styles()
        .styles
        .values()
        .map(|s| StyleSummary {
            id: s.id.as_str().to_string(),
            name: s.name.clone(),
            style_type: s.style_type.as_str().to_string(),
            is_default: s.is_default,
            based_on: s.based_on.as_ref().map(|b| b.as_str().to_string()),
        })
        .collect();

    let mut ref_counts = std::collections::BTreeMap::<String, usize>::new();
    let (kind, sections, sheets, stats) = match document {
        Document::Text(text) => {
            count_text_image_refs(text, &mut ref_counts);
            let sections = text
                .sections
                .iter()
                .enumerate()
                .map(|(index, section)| summarize_section(index, section))
                .collect();
            (
                "text",
                Some(sections),
                None,
                stats_from_text(text, document.styles().styles.len()),
            )
        }
        Document::Workbook(book) => {
            for sheet in &book.sheets {
                for image in &sheet.images {
                    *ref_counts
                        .entry(image.asset.as_str().to_string())
                        .or_default() += 1;
                }
            }
            let sheets: Vec<SheetSummary> = book
                .sheets
                .iter()
                .map(|sheet| {
                    let formulas = sheet.cells.values().filter(|c| c.formula.is_some()).count();
                    SheetSummary {
                        name: sheet.name.clone(),
                        cells: sheet.cells.len(),
                        formulas,
                        merges: sheet.merges.len(),
                        images: sheet.images.len(),
                        hidden: sheet.hidden,
                        used_range: sheet.used_range().map(|r| r.a1()),
                    }
                })
                .collect();
            (
                "workbook",
                None,
                Some(sheets),
                stats_from_workbook(book, document.styles().styles.len()),
            )
        }
        // The slide inventory an agent needs — layout, shape count, notes —
        // lands with the reader that can fill it (plan v2 Phase 13 task 11).
        Document::Presentation(deck) => {
            for slide in &deck.slides {
                for block in slide.blocks() {
                    count_block_images(std::slice::from_ref(block), &mut ref_counts);
                }
                count_shape_images(&slide.shapes, &mut ref_counts);
            }
            (
                "presentation",
                None,
                None,
                stats_from_presentation(deck, document.styles().styles.len()),
            )
        }
    };

    let media = assets
        .ids()
        .into_iter()
        .filter_map(|id| {
            let info = assets.info(&id)?;
            Some(MediaSummary {
                id: info.id.as_str().to_string(),
                file_name: info.file_name.clone(),
                content_type: info.content_type.clone(),
                byte_len: info.byte_len,
                native_size_px: info.native_size_px,
                references: *ref_counts.get(info.id.as_str()).unwrap_or(&0),
            })
        })
        .collect();

    // Prefer live counts from the IR; keep read-time warnings.
    let mut stats = stats;
    if report.stats.styles > stats.styles {
        stats.styles = report.stats.styles;
    }

    InspectReport {
        path,
        source_format: source_format.as_str().to_string(),
        kind,
        meta: document.meta().clone(),
        styles,
        sections,
        sheets,
        media,
        stats,
        warnings: report.warnings,
    }
}

fn summarize_section(index: usize, section: &Section) -> SectionSummary {
    SectionSummary {
        index,
        paper: section.page.paper_name().map(str::to_string),
        orientation: section.page.orientation.as_str().to_string(),
        columns: section.page.columns.max(1),
        headers: section.headers.len(),
        footers: section.footers.len(),
        blocks: section.blocks.len(),
    }
}

fn stats_from_text(text: &TextDocument, style_count: usize) -> ConversionStats {
    let mut stats = ConversionStats {
        styles: style_count as u32,
        ..Default::default()
    };
    for section in &text.sections {
        tally_blocks(&section.blocks, &mut stats);
        for part in section.headers.iter().chain(section.footers.iter()) {
            tally_header_footer(part, &mut stats);
        }
    }
    stats
}

fn stats_from_presentation(
    deck: &docsai_model::Presentation,
    style_count: usize,
) -> ConversionStats {
    let mut stats = ConversionStats {
        styles: style_count as u32,
        slides: deck.slides.len() as u32,
        ..Default::default()
    };
    for slide in &deck.slides {
        for block in slide.blocks() {
            tally_blocks(std::slice::from_ref(block), &mut stats);
        }
        stats.images = stats
            .images
            .saturating_add(count_slide_pictures(&slide.shapes));
    }
    stats
}

/// Pictures are shapes, not blocks, so neither the block tally nor the
/// reference count sees them.
fn count_slide_pictures(shapes: &[docsai_model::presentation::Shape]) -> u32 {
    use docsai_model::presentation::ShapeKind;
    shapes
        .iter()
        .map(|shape| match &shape.kind {
            ShapeKind::Picture(_) => 1,
            ShapeKind::Group(children) => count_slide_pictures(children),
            _ => 0,
        })
        .sum()
}

fn count_shape_images(
    shapes: &[docsai_model::presentation::Shape],
    counts: &mut std::collections::BTreeMap<String, usize>,
) {
    use docsai_model::presentation::ShapeKind;
    for shape in shapes {
        match &shape.kind {
            ShapeKind::Picture(image) => bump_image(image, counts),
            ShapeKind::Group(children) => count_shape_images(children, counts),
            _ => {}
        }
    }
}

fn stats_from_workbook(book: &docsai_model::Workbook, style_count: usize) -> ConversionStats {
    let mut stats = ConversionStats {
        styles: style_count as u32,
        sheets: book.sheets.len() as u32,
        ..Default::default()
    };
    for sheet in &book.sheets {
        stats.cells = stats.cells.saturating_add(sheet.cells.len() as u32);
        stats.images = stats.images.saturating_add(sheet.images.len() as u32);
        let formulas = sheet.cells.values().filter(|c| c.formula.is_some()).count() as u32;
        stats.formulas = stats.formulas.saturating_add(formulas);
    }
    stats
}

fn tally_header_footer(part: &HeaderFooter, stats: &mut ConversionStats) {
    tally_blocks(&part.blocks, stats);
}

fn tally_blocks(blocks: &[Block], stats: &mut ConversionStats) {
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                stats.paragraphs = stats.paragraphs.saturating_add(1);
                tally_inline_stats(&p.content, stats);
            }
            Block::Heading(h) => {
                stats.headings = stats.headings.saturating_add(1);
                stats.paragraphs = stats.paragraphs.saturating_add(1);
                tally_inline_stats(&h.paragraph.content, stats);
            }
            Block::List(list) => {
                stats.lists = stats.lists.saturating_add(1);
                for item in &list.items {
                    tally_blocks(&item.blocks, stats);
                }
            }
            Block::Table(table) => {
                stats.tables = stats.tables.saturating_add(1);
                for row in &table.rows {
                    for cell in &row.cells {
                        tally_blocks(&cell.blocks, stats);
                    }
                }
            }
            Block::Image(_) => stats.images = stats.images.saturating_add(1),
            Block::TextBox(tb) => tally_blocks(&tb.blocks, stats),
            Block::Raw(_) => {}
        }
    }
}

fn tally_inline_stats(inlines: &[docsai_model::text::Inline], stats: &mut ConversionStats) {
    use docsai_model::text::Inline;
    for inline in inlines {
        match inline {
            Inline::Image(_) => stats.images = stats.images.saturating_add(1),
            Inline::Link { content, .. } | Inline::Styled { content, .. } => {
                tally_inline_stats(content, stats);
            }
            Inline::Footnote(note) => {
                stats.footnotes = stats.footnotes.saturating_add(1);
                tally_blocks(&note.blocks, stats);
            }
            _ => {}
        }
    }
}

fn count_text_image_refs(
    text: &TextDocument,
    counts: &mut std::collections::BTreeMap<String, usize>,
) {
    for section in &text.sections {
        count_block_images(&section.blocks, counts);
        for part in section.headers.iter().chain(section.footers.iter()) {
            count_block_images(&part.blocks, counts);
        }
    }
}

fn count_block_images(blocks: &[Block], counts: &mut std::collections::BTreeMap<String, usize>) {
    for block in blocks {
        match block {
            Block::Image(image) => bump_image(image, counts),
            Block::Paragraph(p) => count_inline_images(&p.content, counts),
            Block::Heading(h) => count_inline_images(&h.paragraph.content, counts),
            Block::List(list) => {
                for item in &list.items {
                    count_block_images(&item.blocks, counts);
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        count_block_images(&cell.blocks, counts);
                    }
                }
            }
            Block::TextBox(tb) => count_block_images(&tb.blocks, counts),
            Block::Raw(_) => {}
        }
    }
}

fn count_inline_images(
    inlines: &[docsai_model::text::Inline],
    counts: &mut std::collections::BTreeMap<String, usize>,
) {
    use docsai_model::text::Inline;
    for inline in inlines {
        match inline {
            Inline::Image(image) => bump_image(image, counts),
            Inline::Link { content, .. } | Inline::Styled { content, .. } => {
                count_inline_images(content, counts);
            }
            Inline::Footnote(note) => count_block_images(&note.blocks, counts),
            _ => {}
        }
    }
}

fn bump_image(image: &ImageRef, counts: &mut std::collections::BTreeMap<String, usize>) {
    *counts.entry(image.asset.as_str().to_string()).or_default() += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn corpus_docx(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/docx")
            .join(name)
    }

    #[test]
    fn inspects_a_basic_docx() {
        let report = inspect_path(&corpus_docx("basic-text.docx"), &ConvertOptions::default())
            .expect("inspect");
        assert_eq!(report.source_format, "docx");
        assert_eq!(report.kind, "text");
        assert!(report.sections.as_ref().is_some_and(|s| !s.is_empty()));
        assert!(report.stats.paragraphs > 0 || report.stats.headings > 0);
    }

    #[test]
    fn inspects_styles_and_media() {
        let report = inspect_path(
            &corpus_docx("images-inline.docx"),
            &ConvertOptions::default(),
        )
        .expect("inspect");
        assert_eq!(report.media.len(), 3);
        assert!(report.stats.images >= 3);
    }
}
