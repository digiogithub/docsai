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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slides: Option<Vec<SlideSummary>>,
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

/// High-level slide facts for a presentation (plan v2 Phase 13-K).
///
/// What an agent needs to decide *where* to edit before it loads the deck: the
/// layout a slide hangs from, how much is on it, whether it carries speaker
/// notes, and whether it holds anything Markdown cannot express — SmartArt or an
/// embedded OLE object — which is exactly where a hand edit would destroy
/// something.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SlideSummary {
    /// Position in presentation order (`p:sldIdLst`), not file-name order.
    pub index: usize,
    /// The slide's stable address, when the read assigned ids.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// `p:cSld@name`, when the deck names its slides.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The title placeholder's text, when it holds any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The layout part the placeholders resolve against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    /// `p:cSld@name` of that layout — "Title and Content".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_name: Option<String>,
    /// Every shape on the slide, group children included: the question is how
    /// much is on the slide, not how many top-level entries `p:spTree` has.
    pub shapes: usize,
    pub placeholders: usize,
    pub pictures: usize,
    pub tables: usize,
    pub charts: usize,
    /// Shapes kept as a stub over raw markup — the ones an agent must not
    /// hand-edit.
    pub raw_shapes: usize,
    pub has_notes: bool,
    pub has_smart_art: bool,
    pub has_ole: bool,
    pub hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
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
    let (kind, sections, sheets, slides, stats) = match document {
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
                None,
                stats_from_workbook(book, document.styles().styles.len()),
            )
        }
        Document::Presentation(deck) => {
            for slide in &deck.slides {
                for block in slide.blocks() {
                    count_block_images(std::slice::from_ref(block), &mut ref_counts);
                }
                count_shape_images(&slide.shapes, &mut ref_counts);
            }
            let slides = deck
                .slides
                .iter()
                .enumerate()
                .map(|(index, slide)| summarize_slide(index, slide, &deck.layouts))
                .collect();
            (
                "presentation",
                None,
                None,
                Some(slides),
                stats_from_presentation(deck, document.styles().styles.len()),
            )
        }
    };

    // The preserved package is in the asset store, but it is not media: a deck
    // with no pictures must not report one asset that is the deck itself.
    let skeleton = match document {
        Document::Presentation(deck) => {
            deck.skeleton.as_ref().map(|s| s.asset.as_str().to_string())
        }
        _ => None,
    };
    let media = assets
        .ids()
        .into_iter()
        .filter(|id| Some(id.as_str()) != skeleton.as_deref())
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
        slides,
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

fn summarize_slide(
    index: usize,
    slide: &docsai_model::presentation::Slide,
    layouts: &docsai_model::presentation::LayoutCatalog,
) -> SlideSummary {
    let mut tally = ShapeTally::default();
    tally.walk(&slide.shapes);
    SlideSummary {
        index,
        id: slide.id.as_ref().map(|id| id.as_str().to_string()),
        name: slide.name.clone(),
        title: slide.title(),
        layout: slide.layout.as_ref().map(|id| id.as_str().to_string()),
        // A layout id is a part name; its `p:cSld@name` is what the deck's
        // author sees in PowerPoint, and only the catalogue knows it.
        layout_name: slide
            .layout
            .as_ref()
            .and_then(|id| layouts.layout(id))
            .map(|layout| layout.name.clone()),
        shapes: tally.shapes,
        placeholders: tally.placeholders,
        pictures: tally.pictures,
        tables: tally.tables,
        charts: tally.charts,
        raw_shapes: tally.raw_shapes,
        has_notes: slide.notes.is_some(),
        has_smart_art: tally.smart_art,
        has_ole: tally.ole,
        hidden: slide.hidden,
        section: slide.section.clone(),
    }
}

/// One walk of a slide's shape tree, counting everything the inventory reports.
#[derive(Default)]
struct ShapeTally {
    shapes: usize,
    placeholders: usize,
    pictures: usize,
    tables: usize,
    charts: usize,
    raw_shapes: usize,
    smart_art: bool,
    ole: bool,
}

impl ShapeTally {
    fn walk(&mut self, shapes: &[docsai_model::presentation::Shape]) {
        use docsai_model::presentation::{RawShapeKind, ShapeKind};
        for shape in shapes {
            self.shapes += 1;
            match &shape.kind {
                ShapeKind::Placeholder(_) => self.placeholders += 1,
                ShapeKind::Picture(_) => self.pictures += 1,
                ShapeKind::Table(_) => self.tables += 1,
                ShapeKind::Chart(_) => self.charts += 1,
                ShapeKind::Group(children) => self.walk(children),
                ShapeKind::Raw(raw) => {
                    self.raw_shapes += 1;
                    match raw.kind {
                        RawShapeKind::SmartArt => self.smart_art = true,
                        RawShapeKind::Ole => self.ole = true,
                        _ => {}
                    }
                }
                ShapeKind::TextBox { .. } => {}
            }
        }
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

    fn corpus_pptx(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/pptx")
            .join(name)
    }

    fn deck(name: &str) -> Vec<SlideSummary> {
        let report = inspect_path(&corpus_pptx(name), &ConvertOptions::default()).expect("inspect");
        assert_eq!(report.kind, "presentation");
        assert_eq!(report.source_format, "pptx");
        report.slides.expect("a deck reports its slides")
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

    // Presentations (plan v2 Phase 13-K).

    #[test]
    fn a_deck_is_inspected_slide_by_slide() {
        let slides = deck("basic-slides.pptx");
        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].index, 0);
        assert_eq!(slides[0].title.as_deref(), Some("Informe trimestral"));
        assert_eq!(
            slides[0].layout.as_deref(),
            Some("ppt/slideLayouts/slideLayout1.xml")
        );
        assert_eq!(
            slides[0].layout_name.as_deref(),
            Some("Titulo y objetos"),
            "the layout an agent recognises is `p:cSld@name`, not the part name"
        );
        assert_eq!(slides[0].shapes, 2);
        assert_eq!(slides[0].placeholders, 2);
        assert!(!slides[0].has_notes);
        assert!(!slides[0].hidden);
        assert_eq!(slides[1].index, 1);
        assert_eq!(slides[1].title.as_deref(), Some("Siguientes pasos"));
    }

    #[test]
    fn the_inventory_says_which_slides_carry_notes() {
        let slides = deck("notes-speaker.pptx");
        assert!(slides.iter().all(|s| s.has_notes));
        // The crossed fixture binds notes by relationship, not by numbering;
        // whichever slide ends up with them, the flag has to follow.
        let crossed = deck("notes-crossed.pptx");
        assert_eq!(crossed.iter().filter(|s| s.has_notes).count(), 2);
    }

    #[test]
    fn the_inventory_flags_what_must_not_be_hand_edited() {
        let slides = deck("smartart-fallback.pptx");
        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].raw_shapes, 1);
        assert!(
            slides[0].has_smart_art,
            "an agent deciding where to edit needs to know the diagram is there"
        );
        assert!(!slides[0].has_ole);

        let charts = deck("charts-embedded.pptx");
        assert_eq!(charts[0].charts, 1);
        let tables = deck("tables-simple.pptx");
        assert_eq!(tables[0].tables, 1);
    }

    #[test]
    fn a_group_counts_its_children_and_the_preserved_package_is_not_media() {
        let report = inspect_path(
            &corpus_pptx("images-anchored.pptx"),
            &ConvertOptions::default(),
        )
        .expect("inspect");
        let slides = report.slides.as_ref().expect("slides");
        assert_eq!(slides[0].pictures, 1);
        // One picture, one media asset: the skeleton is in the store too, and
        // reporting the deck itself as an image would be a lie by arithmetic.
        assert_eq!(report.media.len(), 1);
        assert_eq!(report.media[0].references, 1);
    }

    #[test]
    fn the_tally_walks_into_groups_and_names_an_ole_object() {
        use docsai_model::presentation::{RawShape, RawShapeKind, Shape, ShapeKind};

        fn raw(kind: RawShapeKind) -> Shape {
            Shape::new(
                0,
                ShapeKind::Raw(RawShape {
                    kind,
                    ..Default::default()
                }),
            )
        }

        // No corpus deck embeds an OLE object yet, so the flag is proved over
        // the IR instead of over a package — recorded as a gap, not hidden.
        let mut tally = ShapeTally::default();
        tally.walk(&[
            raw(RawShapeKind::Ole),
            Shape::new(
                1,
                ShapeKind::Group(vec![
                    raw(RawShapeKind::SmartArt),
                    Shape::new(0, ShapeKind::TextBox { body: Vec::new() }),
                ]),
            ),
        ]);
        assert!(tally.ole);
        assert!(tally.smart_art);
        assert_eq!(tally.raw_shapes, 2);
        assert_eq!(
            tally.shapes, 4,
            "the group and its two children, plus the OLE stub"
        );
    }
}
