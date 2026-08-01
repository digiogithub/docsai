//! Text-document side of the IR (`docx`, `odt`, `doc`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::image::{ImageRef, RawId};
use crate::list::{ListCatalog, ListId};
use crate::style::{FontProps, ParaProps, StyleCatalog, StyleId};
use crate::units::{Length, Size};

/// Document properties: `docProps/*` in OOXML, `meta.xml` in ODF.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct DocumentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_by: Option<String>,
    /// ISO-8601, verbatim from the source (no date library in the IR).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    /// Custom properties, kept as-is and ordered for determinism.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, String>,
}

/// Page margins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Margins {
    pub top: Length,
    pub right: Length,
    pub bottom: Length,
    pub left: Length,
    pub header: Length,
    pub footer: Length,
}

/// Page orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Orientation {
    #[default]
    Portrait,
    Landscape,
}

impl Orientation {
    pub fn as_str(self) -> &'static str {
        match self {
            Orientation::Portrait => "portrait",
            Orientation::Landscape => "landscape",
        }
    }
}

/// Page geometry of a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct PageGeometry {
    pub size: Size,
    pub margins: Margins,
    pub orientation: Orientation,
    /// Number of text columns.
    pub columns: u16,
    /// True when the first page uses its own header/footer.
    pub title_page: bool,
}

impl PageGeometry {
    /// The standard paper name matching this size, within 1 mm.
    ///
    /// ```
    /// # use docsai_model::text::PageGeometry;
    /// # use docsai_model::units::{Length, Size};
    /// let mut g = PageGeometry::default();
    /// g.size = Size::new(Length::from_twips(11906), Length::from_twips(16838));
    /// assert_eq!(g.paper_name(), Some("A4"));
    /// ```
    pub fn paper_name(&self) -> Option<&'static str> {
        const TOLERANCE_MM: f64 = 1.0;
        const PAPERS: &[(&str, f64, f64)] = &[
            ("A3", 297.0, 420.0),
            ("A4", 210.0, 297.0),
            ("A5", 148.0, 210.0),
            ("Letter", 215.9, 279.4),
            ("Legal", 215.9, 355.6),
            ("Tabloid", 279.4, 431.8),
        ];
        let (w, h) = (self.size.width.mm(), self.size.height.mm());
        let (short, long) = if w <= h { (w, h) } else { (h, w) };
        PAPERS
            .iter()
            .find(|(_, pw, ph)| {
                (short - pw).abs() <= TOLERANCE_MM && (long - ph).abs() <= TOLERANCE_MM
            })
            .map(|(name, _, _)| *name)
    }
}

/// Which pages a header or footer applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HeaderScope {
    Default,
    First,
    Even,
}

impl HeaderScope {
    pub fn as_str(self) -> &'static str {
        match self {
            HeaderScope::Default => "default",
            HeaderScope::First => "first",
            HeaderScope::Even => "even",
        }
    }
}

/// A header or footer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct HeaderFooter {
    pub scope: HeaderScope,
    pub blocks: Vec<Block>,
}

/// A document section: page geometry, its headers/footers and its content.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Section {
    pub page: PageGeometry,
    pub headers: Vec<HeaderFooter>,
    pub footers: Vec<HeaderFooter>,
    pub blocks: Vec<Block>,
}

/// A text document.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct TextDocument {
    pub meta: DocumentMeta,
    pub styles: StyleCatalog,
    pub list_defs: ListCatalog,
    pub sections: Vec<Section>,
}

impl TextDocument {
    /// Every block of every section, in document order.
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.sections.iter().flat_map(|s| s.blocks.iter())
    }
}

/// A block-level element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// Adjacently tagged: `Inline::Text(String)` cannot be internally tagged, and
// both enums stay consistent in the JSON that `inspect --json` emits.
#[serde(rename_all = "kebab-case", tag = "block", content = "data")]
pub enum Block {
    Paragraph(Paragraph),
    Heading(Heading),
    List(List),
    Table(Table),
    /// A picture that stands on its own line.
    Image(ImageRef),
    TextBox(TextBox),
    /// Fidelity hatch: source bytes with no DocMark representation (spec §7).
    Raw(RawFragment),
}

/// Paragraph formatting: a style reference plus direct deltas.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ParaFormat {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    /// Direct paragraph formatting, over and above the style.
    #[serde(skip_serializing_if = "ParaProps::is_empty")]
    pub direct: ParaProps,
    /// Direct run formatting applied to the whole paragraph mark.
    #[serde(skip_serializing_if = "FontProps::is_empty")]
    pub run_direct: FontProps,
}

impl ParaFormat {
    pub fn is_empty(&self) -> bool {
        self.style.is_none() && self.direct.is_empty() && self.run_direct.is_empty()
    }

    pub fn styled(style: impl Into<String>) -> Self {
        ParaFormat {
            style: Some(StyleId::new(style)),
            ..Default::default()
        }
    }
}

/// A paragraph.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Paragraph {
    pub format: ParaFormat,
    pub content: Vec<Inline>,
}

impl Paragraph {
    pub fn new(content: Vec<Inline>) -> Self {
        Paragraph {
            format: ParaFormat::default(),
            content,
        }
    }

    /// A paragraph holding a single text run.
    pub fn text(text: impl Into<String>) -> Self {
        Paragraph::new(vec![Inline::Text(text.into())])
    }

    /// True when the paragraph carries no content at all.
    pub fn is_empty(&self) -> bool {
        self.content.iter().all(|i| i.is_empty())
    }

    /// Concatenation of every text fragment, ignoring formatting.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for inline in &self.content {
            inline.push_plain_text(&mut out);
        }
        out
    }
}

/// A heading: an outline level plus a paragraph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Heading {
    /// 1-based heading depth, as `#` count in Markdown.
    pub level: u8,
    pub paragraph: Paragraph,
}

/// A rebuilt list tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct List {
    /// The definition in [`crate::list::ListCatalog`], when the
    /// list is not the document's default one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def: Option<ListId>,
    pub ordered: bool,
    /// Nesting depth, 0 for a top-level list.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub level: u8,
    pub items: Vec<ListItem>,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

/// One list item; nested lists appear as [`Block::List`] inside `blocks`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ListItem {
    pub blocks: Vec<Block>,
}

/// A table.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Table {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    /// Width of each grid column.
    pub col_widths: Vec<Length>,
    pub rows: Vec<TableRow>,
    /// True when the first row is a header row.
    pub header_row: bool,
}

impl Table {
    /// Number of grid columns, taking spans into account.
    pub fn width(&self) -> usize {
        self.rows
            .iter()
            .map(|r| r.cells.iter().map(|c| c.colspan.max(1) as usize).sum())
            .max()
            .unwrap_or(0)
    }

    /// True when the table needs the `complex=true` container of spec §3.4:
    /// a cell holding anything other than a single paragraph cannot live in a
    /// GFM table row.
    pub fn is_complex(&self) -> bool {
        self.rows.iter().flat_map(|r| r.cells.iter()).any(|c| {
            c.blocks.len() > 1
                || c.blocks
                    .iter()
                    .any(|b| !matches!(b, Block::Paragraph(_) | Block::Image(_)))
        })
    }
}

/// A table row.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
    /// True for rows repeated as a header on each page (`w:tblHeader`).
    pub is_header: bool,
}

/// A table cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct TableCell {
    pub blocks: Vec<Block>,
    /// Horizontal span; 1 for a normal cell.
    pub colspan: u16,
    /// Vertical span; 1 for a normal cell.
    pub rowspan: u16,
    /// True for a cell absorbed by a span above or to its left.
    pub covered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

impl Default for TableCell {
    fn default() -> Self {
        TableCell {
            blocks: Vec::new(),
            colspan: 1,
            rowspan: 1,
            covered: false,
            width: None,
            background: None,
        }
    }
}

impl TableCell {
    pub fn text(text: impl Into<String>) -> Self {
        TableCell {
            blocks: vec![Block::Paragraph(Paragraph::text(text))],
            ..Default::default()
        }
    }
}

/// A text box or frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct TextBox {
    pub blocks: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<Length>,
}

/// A verbatim fragment of the source, preserved for the reverse conversion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RawFragment {
    pub id: RawId,
    /// Source dialect (`ooxml`, `odf`). A writer for another dialect drops the
    /// fragment with a warning rather than emitting nonsense.
    pub format: String,
    /// Package part the fragment came from (`word/document.xml`).
    pub part: String,
    /// The original markup.
    pub content: String,
}

/// Character formatting of a run: style reference plus direct deltas.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct RunProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<StyleId>,
    #[serde(skip_serializing_if = "FontProps::is_empty")]
    pub direct: FontProps,
}

impl RunProps {
    pub fn is_empty(&self) -> bool {
        self.style.is_none() && self.direct.is_empty()
    }

    pub fn direct(direct: FontProps) -> Self {
        RunProps {
            style: None,
            direct,
        }
    }
}

/// A dynamic field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldKind {
    Page,
    NumPages,
    Date,
    Time,
    Toc,
    Ref,
    /// Any other field; the name is the first token of the instruction.
    Other(String),
}

impl FieldKind {
    /// Classifies a field instruction (`" PAGE "`, `" TOC \\o \"1-3\" "`).
    pub fn from_instruction(instr: &str) -> FieldKind {
        let name = instr.split_whitespace().next().unwrap_or("");
        match name.to_ascii_uppercase().as_str() {
            "PAGE" => FieldKind::Page,
            "NUMPAGES" => FieldKind::NumPages,
            "DATE" => FieldKind::Date,
            "TIME" => FieldKind::Time,
            "TOC" => FieldKind::Toc,
            "REF" => FieldKind::Ref,
            "" => FieldKind::Other(String::new()),
            other => FieldKind::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            FieldKind::Page => "PAGE",
            FieldKind::NumPages => "NUMPAGES",
            FieldKind::Date => "DATE",
            FieldKind::Time => "TIME",
            FieldKind::Toc => "TOC",
            FieldKind::Ref => "REF",
            FieldKind::Other(s) => s,
        }
    }
}

/// A manual break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BreakKind {
    Line,
    Page,
    Column,
}

impl BreakKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BreakKind::Line => "line",
            BreakKind::Page => "page",
            BreakKind::Column => "column",
        }
    }
}

/// An inline element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "inline", content = "data")]
pub enum Inline {
    Text(String),
    /// Text with formatting applied.
    Styled {
        content: Vec<Inline>,
        props: RunProps,
    },
    Link {
        target: String,
        content: Vec<Inline>,
        #[serde(default, skip_serializing_if = "RunProps::is_empty")]
        props: RunProps,
    },
    /// A footnote; its content is block-level.
    Footnote(Vec<Block>),
    Field {
        kind: FieldKind,
        /// Last known displayed value.
        cached: String,
        /// The full instruction, needed to regenerate the field.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        instruction: String,
    },
    Break(BreakKind),
    /// An image flowing with the text.
    Image(ImageRef),
    /// Inline-level fidelity hatch.
    Raw(RawFragment),
}

impl Inline {
    /// True when the element contributes nothing visible.
    pub fn is_empty(&self) -> bool {
        match self {
            Inline::Text(t) => t.is_empty(),
            Inline::Styled { content, .. } => content.iter().all(|i| i.is_empty()),
            _ => false,
        }
    }

    fn push_plain_text(&self, out: &mut String) {
        match self {
            Inline::Text(t) => out.push_str(t),
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                for i in content {
                    i.push_plain_text(out);
                }
            }
            Inline::Field { cached, .. } => out.push_str(cached),
            Inline::Break(BreakKind::Line) => out.push('\n'),
            _ => {}
        }
    }

    /// Wraps `content` in a [`Inline::Styled`], or returns it unchanged when
    /// there is no formatting to apply.
    pub fn styled(content: Vec<Inline>, props: RunProps) -> Vec<Inline> {
        if props.is_empty() {
            content
        } else {
            vec![Inline::Styled { content, props }]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Size;

    #[test]
    fn recognises_standard_paper_sizes() {
        let mut g = PageGeometry {
            size: Size::new(Length::from_twips(11906), Length::from_twips(16838)),
            ..Default::default()
        };
        assert_eq!(g.paper_name(), Some("A4"));
        g.size = Size::new(Length::from_twips(16838), Length::from_twips(11906));
        assert_eq!(g.paper_name(), Some("A4"), "landscape is the same paper");
        g.size = Size::new(Length::from_cm(10.0), Length::from_cm(10.0));
        assert_eq!(g.paper_name(), None);
    }

    #[test]
    fn field_instructions_are_classified() {
        assert_eq!(FieldKind::from_instruction(" PAGE "), FieldKind::Page);
        assert_eq!(
            FieldKind::from_instruction(r#" TOC \o "1-3" \h "#),
            FieldKind::Toc
        );
        assert_eq!(
            FieldKind::from_instruction(" HYPERLINK x "),
            FieldKind::Other("HYPERLINK".into())
        );
    }

    #[test]
    fn plain_text_walks_nested_inlines() {
        let p = Paragraph::new(vec![
            Inline::Text("Hola ".into()),
            Inline::Styled {
                content: vec![Inline::Text("mundo".into())],
                props: RunProps::default(),
            },
            Inline::Link {
                target: "https://example.com".into(),
                content: vec![Inline::Text(" enlace".into())],
                props: RunProps::default(),
            },
            Inline::Field {
                kind: FieldKind::Page,
                cached: " 1".into(),
                instruction: " PAGE ".into(),
            },
        ]);
        assert_eq!(p.plain_text(), "Hola mundo enlace 1");
    }

    #[test]
    fn table_width_accounts_for_colspans() {
        let table = Table {
            rows: vec![
                TableRow {
                    cells: vec![
                        TableCell::text("a"),
                        TableCell {
                            colspan: 2,
                            ..TableCell::text("b")
                        },
                    ],
                    is_header: true,
                },
                TableRow {
                    cells: vec![
                        TableCell::text("c"),
                        TableCell::text("d"),
                        TableCell::text("e"),
                    ],
                    is_header: false,
                },
            ],
            ..Default::default()
        };
        assert_eq!(table.width(), 3);
        assert!(!table.is_complex());
    }

    #[test]
    fn multi_block_cells_make_a_table_complex() {
        let table = Table {
            rows: vec![TableRow {
                cells: vec![TableCell {
                    blocks: vec![
                        Block::Paragraph(Paragraph::text("uno")),
                        Block::Paragraph(Paragraph::text("dos")),
                    ],
                    ..Default::default()
                }],
                is_header: false,
            }],
            ..Default::default()
        };
        assert!(table.is_complex());
    }

    #[test]
    fn styled_is_skipped_when_there_is_nothing_to_apply() {
        let content = vec![Inline::Text("x".into())];
        assert_eq!(
            Inline::styled(content.clone(), RunProps::default()),
            content,
            "no empty Styled wrappers in the IR"
        );
    }
}
