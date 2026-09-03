//! The contextual cheat-sheet: DocMark syntax for the document at hand.
//!
//! An agent that is about to edit a document needs the syntax of the
//! constructs that document contains, and nothing else. A deck with no charts
//! and no comments should not pay for their syntax, and a plain report should
//! not pay for the presentation profile at all. So the sheet is **derived**:
//! the IR is walked, the constructs actually present are collected as
//! [`Feature`]s, and only their entries are written.
//!
//! Three properties keep it honest, and each is a test rather than an
//! intention:
//!
//! * **Every entry parses.** The snippet of every feature is validated as
//!   DocMark in a wrapper of its own document class, so an entry that drifts
//!   from the specification fails the build rather than misleading an agent
//!   (`the_examples_parse`).
//! * **The specification cannot grow past it silently.** Every heading of
//!   `docs/docmark-specification.md` is either claimed by a feature or listed
//!   with a reason in the map the test carries; a new section is red until
//!   somebody decides which it is (`the_specification_is_fully_accounted_for`,
//!   in `tests/cheatsheet.rs`).
//! * **No entry is dead.** Every feature is produced by at least one document
//!   of the corpus, so an entry nothing can trigger is caught rather than
//!   being paid for by every reader (`every_feature_is_reachable`, same file).
//!
//! The sheet describes what `full` and `agent` write. `standard` and `plain`
//! are subsets of it (spec §6), so an agent reading one of those is told more
//! than it needs and never less.

use std::collections::BTreeSet;

use docsai_model::addressing::implicit_shapes;
use docsai_model::presentation::{Presentation, RawShapeKind, Shape, ShapeKind};
use docsai_model::sheet::Workbook;
use docsai_model::style::FontProps;
use docsai_model::text::{Block, Inline, Paragraph, Table, TextDocument};
use docsai_model::Document;

use crate::service::with_scratch_document;
use crate::{ConvertError, ConvertOptions, SourceInput};
use docsai_model::Format;

/// Which document class a feature's example belongs to.
///
/// The examples are validated inside a wrapper, and a slide container does not
/// parse in a report any more than a `cell-meta` block does on a slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Valid in any document class.
    Any,
    Text,
    Workbook,
    Deck,
}

/// A DocMark construct the cheat-sheet can document.
///
/// The order of the variants is the order the sheet writes them: the front
/// matter first, then text, then the spreadsheet profile, then the
/// presentation profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    FrontMatter,
    Heading,
    ParagraphAttributes,
    Emphasis,
    SpanAttributes,
    EmptyParagraph,
    Link,
    List,
    Table,
    ComplexTable,
    Image,
    Footnote,
    Field,
    Break,
    TextBox,
    Section,
    HeaderFooter,
    AttributeSets,
    RawBlock,
    Comment,
    Sheet,
    CellMeta,
    SheetImages,
    Deck,
    Placeholder,
    ShapeContainer,
    Group,
    Chart,
    ChartData,
    Stub,
    Notes,
}

/// Every feature, in sheet order. The single list the tests measure against.
pub const FEATURES: &[Feature] = &[
    Feature::FrontMatter,
    Feature::Heading,
    Feature::ParagraphAttributes,
    Feature::Emphasis,
    Feature::SpanAttributes,
    Feature::EmptyParagraph,
    Feature::Link,
    Feature::List,
    Feature::Table,
    Feature::ComplexTable,
    Feature::Image,
    Feature::Footnote,
    Feature::Field,
    Feature::Break,
    Feature::TextBox,
    Feature::Section,
    Feature::HeaderFooter,
    Feature::AttributeSets,
    Feature::RawBlock,
    Feature::Comment,
    Feature::Sheet,
    Feature::CellMeta,
    Feature::SheetImages,
    Feature::Deck,
    Feature::Placeholder,
    Feature::ShapeContainer,
    Feature::Group,
    Feature::Chart,
    Feature::ChartData,
    Feature::Stub,
    Feature::Notes,
];

impl Feature {
    /// The name the sheet writes.
    pub fn key(self) -> &'static str {
        match self {
            Feature::FrontMatter => "front matter",
            Feature::Heading => "heading",
            Feature::ParagraphAttributes => "paragraph attributes",
            Feature::Emphasis => "emphasis",
            Feature::SpanAttributes => "styled span",
            Feature::EmptyParagraph => "empty paragraph",
            Feature::Link => "link",
            Feature::List => "list",
            Feature::Table => "table",
            Feature::ComplexTable => "table container",
            Feature::Image => "image",
            Feature::Footnote => "footnote",
            Feature::Field => "field",
            Feature::Break => "break",
            Feature::TextBox => "text box",
            Feature::Section => "section",
            Feature::HeaderFooter => "header / footer",
            Feature::AttributeSets => "attribute sets",
            Feature::RawBlock => "raw block",
            Feature::Comment => "comment",
            Feature::Sheet => "sheet",
            Feature::CellMeta => "cell metadata",
            Feature::SheetImages => "sheet images",
            Feature::Deck => "slide",
            Feature::Placeholder => "placeholder",
            Feature::ShapeContainer => "shape",
            Feature::Group => "group",
            Feature::Chart => "chart",
            Feature::ChartData => "chart data",
            Feature::Stub => "object stub",
            Feature::Notes => "speaker notes",
        }
    }

    /// The section of `docs/docmark-specification.md` this projects.
    pub fn spec(self) -> &'static str {
        match self {
            Feature::FrontMatter => "2, 11.1",
            Feature::Heading | Feature::ParagraphAttributes | Feature::EmptyParagraph => "3.1",
            Feature::Emphasis | Feature::SpanAttributes | Feature::Link | Feature::Field => "3.2",
            Feature::Break => "3.2",
            Feature::List => "3.3",
            Feature::Table | Feature::ComplexTable => "3.4",
            Feature::Image => "3.5",
            Feature::Footnote => "3.2",
            Feature::TextBox | Feature::Section | Feature::HeaderFooter => "3.6",
            Feature::AttributeSets => "3.7",
            Feature::RawBlock => "7",
            Feature::Comment => "11.3",
            Feature::Sheet | Feature::CellMeta => "4",
            Feature::SheetImages => "4.1",
            Feature::Deck => "11.2",
            Feature::Placeholder | Feature::ShapeContainer | Feature::Group => "11.2",
            Feature::Chart | Feature::Stub => "11.2",
            Feature::ChartData => "11.2",
            Feature::Notes => "11.2",
        }
    }

    /// The document class the example belongs to.
    pub fn class(self) -> Class {
        match self {
            Feature::Sheet | Feature::CellMeta | Feature::SheetImages => Class::Workbook,
            Feature::Deck
            | Feature::Placeholder
            | Feature::ShapeContainer
            | Feature::Group
            | Feature::Chart
            | Feature::ChartData
            | Feature::Stub
            | Feature::Notes => Class::Deck,
            Feature::Footnote
            | Feature::Field
            | Feature::Break
            | Feature::TextBox
            | Feature::Section
            | Feature::HeaderFooter
            | Feature::Heading => Class::Text,
            _ => Class::Any,
        }
    }

    /// The DocMark the entry shows. Valid DocMark, asserted by test.
    pub fn snippet(self) -> &'static str {
        match self {
            // The rendered sheet substitutes the document's own version and
            // source format here; `front_matter` is the same text and the two
            // are asserted equal.
            Feature::FrontMatter => "---\ndocmark: \"1.1\"\nsource-format: docx\nnext-id: 42\n---",
            Feature::Heading => "# Chapter title {#n1 .Heading1}",
            Feature::ParagraphAttributes => "Body text. {#n2 .Quote align=center space-after=12pt}",
            Feature::Emphasis => "**bold**, *italic*, ~~struck~~",
            Feature::SpanAttributes => {
                "[text]{.underline color=\"#FF0000\" size=14pt}, [x]{.sup}, [y]{.Emphatic}"
            }
            Feature::EmptyParagraph => "[]{.empty}",
            Feature::Link => "[text](https://example.com){.Hyperlink}",
            Feature::List => "1. First {.ListParagraph list=L1 list-id=n3}\n2. Second",
            Feature::Table => "| Concept | T1 |\n|---|---|\n| Sales | 100 |",
            Feature::ComplexTable => {
                "::: {#n4 .table style=TableGrid col-widths=\"3cm,5cm\"}\n\
                 | Concept | T1 |\n|---|---|\n| Sales {rowspan=2} | 100 |\n| | 300 |\n:::"
            }
            Feature::Image => {
                "![alt text](assets/img-3f2a91.png){#n5 width=450px height=300px anchor=floating \
                 x=1.2cm y=0.5cm wrap=square}"
            }
            Feature::Footnote => "A claim.[^1]{#n6}\n\n[^1]: The note.",
            Feature::Field => "page [7]{.field field=PAGE}",
            Feature::Break => "[]{.break kind=page}",
            Feature::TextBox => "::: {.textbox x=5cm y=2cm width=6cm}\nBoxed text.\n:::",
            Feature::Section => {
                "::: {#n7 .section columns=2 page-size=A4 orientation=landscape}\nContent.\n:::"
            }
            Feature::HeaderFooter => "::: {.header scope=default}\nHeader text.\n:::",
            Feature::AttributeSets => "[convert]{.g1} runs the pipeline.",
            Feature::RawBlock => {
                "::: {#r7 .raw format=ooxml part=\"word/document.xml\" \
                 src=\"assets/_raw/r7.xml\"}\n:::"
            }
            Feature::Comment => {
                "Ship it by [Friday]{.comment ref=c1}.\n\n\
                 ::: {#n9 .comment author=\"Ada Lovelace\" date=\"2026-05-04T09:12:00Z\" \
                 initials=AL ref=c1}\nThursday is the real deadline.\n\n\
                 ::: {#n10 .comment author=\"Grace Hopper\"}\nAgreed.\n:::\n:::"
            }
            Feature::Sheet => {
                "# Sales {#n1 .sheet cols=\"A:C\" col-widths=\"12,9,9\" frozen=\"A2\"}\n\n\
                 | | A | B | C |\n|---|---|---|---|\n\
                 | **1** | Product | T1 | Total |\n| **2** | Widgets | 100 | 100 |"
            }
            Feature::CellMeta => {
                "::: {.cell-meta}\n- C2: formula=\"SUM(B2:B2)\" num-fmt=\"#,##0\"\n\
                 - A1:C1: style=HeaderRow\n- A5:C5: merge=true\n:::"
            }
            Feature::SheetImages => {
                "::: {.sheet-images}\n![Logo](assets/img-9c04b7.png){anchor=two-cell from=\"B2\" \
                 to=\"D8\" move-with-cells=true}\n:::"
            }
            Feature::Deck => "## Q3 results {#n1 .slide layout=L1 section=\"Results\"}",
            Feature::Placeholder => {
                "::: {#n4 .ph idx=2 type=body pos=\"88px,120px\" size=\"784px,300px\"}\n\
                 - A bullet\n:::"
            }
            Feature::ShapeContainer => {
                "::: {#n5 .shape geom=rightArrow name=\"Arrow 1\" pos=\"3200000emu,2200000emu\" \
                 size=\"1400000emu,500000emu\" raw=r7}\nLabel\n:::"
            }
            Feature::Group => {
                "::: {#n6 .group}\n::: {.shape geom=ellipse}\n:::\n::: {.shape geom=line}\n:::\n:::"
            }
            Feature::Chart => {
                "::: {#n2 .chart kind=barChart data=\"assets/asset-48b54984.xlsx\" raw=r3}\n:::"
            }
            Feature::ChartData => {
                "::: {#n2 .chart kind=barChart}\nSales by region\n\n\
                 | | North {format=\"#,##0\"} | South |\n|---|---|---|\n\
                 | Q1 | 1200 | 800 |\n| Q2 | 980 | |\n:::"
            }
            Feature::Stub => "::: {#n8 .smartart raw=r9}\nProcess diagram\n:::",
            Feature::Notes => "::: {.notes}\nOpen with the churn number.\n:::",
        }
    }

    /// The one thing about the entry that the snippet does not show.
    pub fn note(self) -> &'static str {
        match self {
            Feature::FrontMatter => {
                "`{#n7}` addresses a node; ids are never renumbered or reused, and `next-id` \
                 is the counter they come from. `full` also writes the document's metadata and \
                 its catalogues (`page:`, `style-defaults:`, `styles:`, `list-definitions:`); \
                 `agent` drops them and keeps the ids."
            }
            Feature::Heading => {
                "The `#` count is the outline level; the class is the style, omitted when the \
                 level already names it."
            }
            Feature::ParagraphAttributes => {
                "Trailing block. Only what the style cascade does not already imply is written."
            }
            Feature::Emphasis => "Native GFM; no attribute block needed.",
            Feature::SpanAttributes => {
                "`.underline`, `.sub`, `.sup`, `.small-caps`, `.caps`; `bold=false` turns a \
                 style's formatting off."
            }
            Feature::EmptyParagraph => "An empty paragraph is content; a blank line is not.",
            Feature::Link => "Attributes only when the link carries a character style.",
            Feature::List => {
                "`list=` and `list-id=` ride in the first item's own block — an item never \
                 carries two."
            }
            Feature::Table => "GFM while the table is regular: one paragraph per cell, no spans.",
            Feature::ComplexTable => {
                "The container carries `style`, `col-widths`, `header-row=false`; spans go on \
                 the cell that opens the area and absorbed cells are empty. A cell holding more \
                 than one block needs `complex=true` with `::: {.row}` / `::: {.cell}` inside."
            }
            Feature::Image => {
                "`width`/`height` are always written. Alt text is the `![…]` field, never an \
                 attribute."
            }
            Feature::Footnote => {
                "The id is on the reference, which is where the note sits in the flow; the \
                 definition goes at the foot of the document."
            }
            Feature::Field => "The visible text is the last known value; `instr=\"…\"` at `full`.",
            Feature::Break => {
                "`kind=page` or `kind=column`; a line break is a trailing double space."
            }
            Feature::TextBox => "A floating frame with its own position.",
            Feature::Section => "Only written when the document has more than one section.",
            Feature::HeaderFooter => "`scope` is `default`, `first` or `even`.",
            Feature::AttributeSets => {
                "`attribute-sets:` in the front matter names a repeated attribute payload; a \
                 reader expands the class into its pairs, and a pair written on the node wins."
            }
            Feature::RawBlock => {
                "What DocMark cannot represent, kept verbatim. Do not edit it; deleting it \
                 loses exactly that element. A missing sidecar is an error, not a warning."
            }
            Feature::Comment => {
                "The container is where the comment lives; `ref=` binds it to its span and is \
                 document-local. A reply is a nested container and carries no `ref=`. A slide \
                 comment has `pos=` and no span."
            }
            Feature::Sheet => {
                "One `#` section per sheet. The first row and column are generated coordinates; \
                 the cell shows the value."
            }
            Feature::CellMeta => {
                "Formula (without `=`, in the source dialect), `num-fmt`, `type` \
                 (`number|text|bool|date|error`), `style` and `merge` travel here, not in the \
                 grid. Ranges are expanded by the parser."
            }
            Feature::SheetImages => {
                "`anchor` is `two-cell`, `one-cell` or `absolute`; `two-cell` writes no size."
            }
            Feature::Deck => {
                "An `##` carrying `.slide`; the heading *is* the title placeholder, and the \
                 layout's body placeholder is written at slide level as ordinary Markdown."
            }
            Feature::Placeholder => {
                "Every placeholder that is not the implicit title or body. `idx=` is what \
                 matches it to the layout."
            }
            Feature::ShapeContainer => {
                "A free shape, text box or `.connector`. `geom=` is identity, not measurement."
            }
            Feature::Group => "A box around shapes that stay addressable in their own right.",
            Feature::Chart => {
                "A stub: present so it is not deleted, opaque so it is not hand-edited."
            }
            Feature::ChartData => {
                "Categories are the first column, one column per series, the first header cell \
                 empty. An empty cell is a gap, not a zero."
            }
            Feature::Stub => {
                "`.smartart`, `.ole`, `.media`, `.object`; the body is the text the object \
                 shows, if any."
            }
            Feature::Notes => "After the slide's shapes. A blockquote at `standard`.",
        }
    }
}

/// The constructs `document` actually contains.
pub fn features(document: &Document) -> BTreeSet<Feature> {
    let mut found = BTreeSet::new();
    found.insert(Feature::FrontMatter);
    let mut direct_runs = 0usize;
    match document {
        Document::Text(text) => text_features(text, &mut found, &mut direct_runs),
        Document::Workbook(book) => workbook_features(book, &mut found, &mut direct_runs),
        Document::Presentation(deck) => deck_features(deck, &mut found, &mut direct_runs),
    }
    // §3.7 interns a pattern that repeats at least three times. The dictionary
    // is a function of the serialised text, so this is the IR's closest
    // honest question: is there enough hand formatting for one to appear?
    // Over-reporting costs an entry; under-reporting would leave `{.g1}`
    // undocumented in a document that writes it.
    if direct_runs >= 3 {
        found.insert(Feature::AttributeSets);
    }
    found
}

fn text_features(text: &TextDocument, found: &mut BTreeSet<Feature>, direct: &mut usize) {
    if text.sections.len() > 1 {
        found.insert(Feature::Section);
    }
    for section in &text.sections {
        if !section.headers.is_empty() || !section.footers.is_empty() {
            found.insert(Feature::HeaderFooter);
        }
        for hf in section.headers.iter().chain(section.footers.iter()) {
            walk_blocks(&hf.blocks, found, direct);
        }
        walk_blocks(&section.blocks, found, direct);
    }
}

fn workbook_features(book: &Workbook, found: &mut BTreeSet<Feature>, direct: &mut usize) {
    found.insert(Feature::Sheet);
    for sheet in &book.sheets {
        if !sheet.images.is_empty() {
            found.insert(Feature::SheetImages);
        }
        if !sheet.raw.is_empty() {
            found.insert(Feature::RawBlock);
        }
        if !sheet.merges.is_empty() {
            found.insert(Feature::CellMeta);
        }
        for cell in sheet.cells.values() {
            if cell.formula.is_some()
                || cell.num_fmt.is_some()
                || cell.style.is_some()
                || !matches!(
                    cell.value,
                    docsai_model::sheet::CellValue::Text(_)
                        | docsai_model::sheet::CellValue::Number(_)
                        | docsai_model::sheet::CellValue::Empty
                )
            {
                found.insert(Feature::CellMeta);
                break;
            }
        }
        let _ = direct;
    }
}

fn deck_features(deck: &Presentation, found: &mut BTreeSet<Feature>, direct: &mut usize) {
    found.insert(Feature::Deck);
    for slide in &deck.slides {
        if slide.notes.is_some() {
            found.insert(Feature::Notes);
        }
        if !slide.comments.is_empty() {
            found.insert(Feature::Comment);
        }
        if !slide.raw.is_empty() {
            found.insert(Feature::RawBlock);
        }
        for comment in &slide.comments {
            walk_blocks(&comment.blocks, found, direct);
        }
        // Asking the same function the deck writer asks is what keeps "this
        // shape needs a `.ph` container" from drifting from what is actually
        // written (spec §11.2 rules 1 and 2).
        let implicit = implicit_shapes(slide, &deck.layouts);
        walk_shapes(&slide.shapes, &implicit, found, direct);
    }
}

fn walk_shapes(
    shapes: &[Shape],
    implicit: &[usize],
    found: &mut BTreeSet<Feature>,
    direct: &mut usize,
) {
    for (index, shape) in shapes.iter().enumerate() {
        match &shape.kind {
            ShapeKind::Placeholder(ph) => {
                if !implicit.contains(&index) {
                    found.insert(Feature::Placeholder);
                }
                walk_blocks(&ph.body, found, direct);
            }
            ShapeKind::TextBox { body } => {
                found.insert(Feature::ShapeContainer);
                walk_blocks(body, found, direct);
            }
            ShapeKind::Picture(_) => {
                found.insert(Feature::Image);
            }
            ShapeKind::Table(table) => {
                table_features(table, found, direct);
            }
            ShapeKind::Chart(chart) => {
                found.insert(Feature::Chart);
                if !chart.series.is_empty() {
                    found.insert(Feature::ChartData);
                }
                if chart.raw.is_some() {
                    found.insert(Feature::RawBlock);
                }
            }
            ShapeKind::Group(children) => {
                found.insert(Feature::Group);
                // Nothing inside a group is implicit: rules 1 and 2 name two
                // shapes of the slide's own tree, never of a group's.
                walk_shapes(children, &[], found, direct);
            }
            ShapeKind::Raw(raw) => {
                match raw.kind {
                    RawShapeKind::Shape | RawShapeKind::Connector => {
                        found.insert(Feature::ShapeContainer);
                    }
                    RawShapeKind::SmartArt
                    | RawShapeKind::Ole
                    | RawShapeKind::Media
                    | RawShapeKind::Other => {
                        found.insert(Feature::Stub);
                    }
                }
                if raw.raw.is_some() {
                    found.insert(Feature::RawBlock);
                }
            }
        }
    }
}

fn walk_blocks(blocks: &[Block], found: &mut BTreeSet<Feature>, direct: &mut usize) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => paragraph_features(paragraph, found, direct),
            Block::Heading(heading) => {
                found.insert(Feature::Heading);
                paragraph_features(&heading.paragraph, found, direct);
            }
            Block::List(list) => {
                found.insert(Feature::List);
                for item in &list.items {
                    walk_blocks(&item.blocks, found, direct);
                }
            }
            Block::Table(table) => table_features(table, found, direct),
            Block::Image(_) => {
                found.insert(Feature::Image);
            }
            Block::TextBox(text_box) => {
                found.insert(Feature::TextBox);
                walk_blocks(&text_box.blocks, found, direct);
            }
            Block::Raw(_) => {
                found.insert(Feature::RawBlock);
            }
        }
    }
}

fn table_features(table: &Table, found: &mut BTreeSet<Feature>, direct: &mut usize) {
    found.insert(Feature::Table);
    let spanned = table
        .rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .any(|cell| cell.colspan > 1 || cell.rowspan > 1);
    if spanned || table.is_complex() || table.style.is_some() || !table.col_widths.is_empty() {
        found.insert(Feature::ComplexTable);
    }
    for cell in table.rows.iter().flat_map(|row| row.cells.iter()) {
        walk_blocks(&cell.blocks, found, direct);
    }
}

fn paragraph_features(paragraph: &Paragraph, found: &mut BTreeSet<Feature>, direct: &mut usize) {
    if paragraph.content.iter().all(Inline::is_empty) {
        found.insert(Feature::EmptyParagraph);
    }
    if !paragraph.format.direct.is_empty() || !paragraph.format.run_direct.is_empty() {
        *direct += 1;
    }
    if !paragraph.format.is_empty() {
        found.insert(Feature::ParagraphAttributes);
    }
    walk_inlines(&paragraph.content, found, direct);
}

fn walk_inlines(inlines: &[Inline], found: &mut BTreeSet<Feature>, direct: &mut usize) {
    for inline in inlines {
        match inline {
            Inline::Text(_) => {}
            Inline::Styled { content, props } => {
                if !props.direct.is_empty() {
                    *direct += 1;
                }
                if is_emphasis(&props.direct) {
                    found.insert(Feature::Emphasis);
                }
                if props.style.is_some() || beyond_emphasis(&props.direct) {
                    found.insert(Feature::SpanAttributes);
                }
                walk_inlines(content, found, direct);
            }
            Inline::Link { content, .. } => {
                found.insert(Feature::Link);
                walk_inlines(content, found, direct);
            }
            Inline::Footnote(footnote) => {
                found.insert(Feature::Footnote);
                walk_blocks(&footnote.blocks, found, direct);
            }
            Inline::Comment(comment) => {
                found.insert(Feature::Comment);
                walk_inlines(comment.covered(), found, direct);
                walk_blocks(&comment.blocks, found, direct);
                for reply in &comment.replies {
                    walk_blocks(&reply.blocks, found, direct);
                }
            }
            Inline::Field { .. } => {
                found.insert(Feature::Field);
            }
            Inline::Break(_) => {
                found.insert(Feature::Break);
            }
            Inline::Image(_) => {
                found.insert(Feature::Image);
            }
            Inline::Raw(_) => {
                found.insert(Feature::RawBlock);
            }
        }
    }
}

/// The three GFM has native syntax for.
fn is_emphasis(props: &FontProps) -> bool {
    props.bold == Some(true) || props.italic == Some(true) || props.strike == Some(true)
}

/// Anything that needs an attribute block rather than `**`, `*` or `~~`.
fn beyond_emphasis(props: &FontProps) -> bool {
    props.name.is_some()
        || props.size.is_some()
        || props.underline.is_some()
        || props.color.is_some()
        || props.highlight.is_some()
        || props.vert_align.is_some()
        || props.small_caps.is_some()
        || props.caps.is_some()
        || props.bold == Some(false)
        || props.italic == Some(false)
        || props.strike == Some(false)
}

/// The DocMark version a document with these features declares (spec §11).
fn declared_version(found: &BTreeSet<Feature>) -> &'static str {
    if found.contains(&Feature::Comment) {
        docsai_model::DOCMARK_VERSION_COMMENTS
    } else if found.contains(&Feature::Deck) {
        docsai_model::DOCMARK_VERSION_PRESENTATION
    } else {
        docsai_model::DOCMARK_VERSION_ADDRESSED
    }
}

/// The front-matter example, which is the one entry written from the document
/// rather than from a constant: a sheet that showed `source-format: docx` over
/// a deck would contradict itself on its first line.
fn front_matter(version: &str, source_format: &str) -> String {
    format!("---\ndocmark: \"{version}\"\nsource-format: {source_format}\nnext-id: 42\n---")
}

/// Writes the sheet for a set of features.
pub fn render(found: &BTreeSet<Feature>, label: Option<&str>, source_format: Format) -> String {
    let version = declared_version(found);
    let format_name = source_format.as_str();
    let mut out = String::new();
    match label {
        Some(label) => out.push_str(&format!("# DocMark syntax used by `{label}`\n\n")),
        None => out.push_str("# DocMark syntax used by this document\n\n"),
    }
    out.push_str(
        "Projection of `docs/docmark-specification.md` (§ numbers below) limited to what this \
         document contains, at `full`/`agent`.\n",
    );
    for feature in FEATURES.iter().filter(|f| found.contains(f)) {
        let snippet = match feature {
            Feature::FrontMatter => front_matter(version, format_name),
            other => other.snippet().to_string(),
        };
        out.push('\n');
        out.push_str(feature.key());
        out.push_str(" §");
        out.push_str(feature.spec());
        out.push('\n');
        for line in snippet.lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
        out.push_str("  — ");
        out.push_str(feature.note());
        out.push('\n');
    }
    out
}

/// The cheat-sheet for a document, read through the same door as every other
/// read-only primitive (so the same cache serves it).
pub fn cheatsheet_input(
    source: SourceInput<'_>,
    options: &ConvertOptions,
) -> Result<String, ConvertError> {
    let ((found, source_format), label) =
        with_scratch_document(source, options, false, |document, _, docmark| {
            // Reading a `.dmk.md` detects `docmark` as the format, which is
            // true of the file and useless in the example: a document's own
            // `source-format:` names the package it came from. Fall back to
            // the class, which is what that key can only be.
            let format = match docmark.source_format {
                Format::DocMark => match document {
                    Document::Text(_) => Format::Docx,
                    Document::Workbook(_) => Format::Xlsx,
                    Document::Presentation(_) => Format::Pptx,
                },
                other => other,
            };
            (features(document), format)
        })?;
    Ok(render(&found, label.as_deref(), source_format))
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_docmark::validate::{validate_at, Validation};

    /// Wraps one entry's snippet in the smallest document of its class.
    fn probe(feature: Feature) -> String {
        if feature == Feature::FrontMatter {
            return format!("{}\n\nA paragraph.\n", feature.snippet());
        }
        let (front, prelude) = match feature.class() {
            Class::Workbook => (
                "---\ndocmark: \"1.1\"\nsource-format: xlsx\nnext-id: 20\n---\n",
                if feature == Feature::Sheet {
                    String::new()
                } else {
                    format!("\n{}\n", Feature::Sheet.snippet())
                },
            ),
            Class::Deck => (
                "---\ndocmark: \"1.2\"\nsource-format: pptx\nnext-id: 20\nlayouts:\n  L1: \
                 { name: \"Title and Content\", master: M1, title: 0, body: 1 }\n---\n",
                if feature == Feature::Deck {
                    String::new()
                } else {
                    format!("\n{}\n", Feature::Deck.snippet())
                },
            ),
            Class::Any | Class::Text => (
                "---\ndocmark: \"1.3\"\nsource-format: docx\nnext-id: 20\nattribute-sets:\n  \
                 g1: \"color=#1F4E79 font=Consolas size=12pt\"\n---\n",
                String::new(),
            ),
        };
        format!("{front}{prelude}\n{}\n", feature.snippet())
    }

    #[test]
    fn the_examples_parse() {
        // A sidecar raw-block resolves `src=` against the document's own
        // directory (spec §7), so the probe is validated as a file on disk
        // with the sidecar beside it rather than as bare text.
        let dir = tempfile::tempdir().expect("scratch");
        std::fs::create_dir_all(dir.path().join("assets/_raw")).expect("raw dir");
        std::fs::write(dir.path().join("assets/_raw/r7.xml"), "<w:sdt/>").expect("sidecar");

        for &feature in FEATURES {
            let text = probe(feature);
            let result: Validation = validate_at(&text, Some(dir.path()));
            assert!(
                result.parses(),
                "{:?} does not parse: {:#?}\n---\n{text}",
                feature,
                result.diagnostics
            );
        }
    }

    #[test]
    fn the_rendered_front_matter_is_the_example_it_documents() {
        assert_eq!(
            front_matter("1.1", "docx"),
            Feature::FrontMatter.snippet(),
            "the substituted form and the validated constant must not drift"
        );
    }

    #[test]
    fn every_feature_has_prose_and_a_snippet() {
        for &feature in FEATURES {
            assert!(!feature.key().is_empty());
            assert!(!feature.spec().is_empty());
            assert!(!feature.snippet().is_empty());
            assert!(!feature.note().is_empty(), "{feature:?} has no note");
        }
    }

    #[test]
    fn a_plain_document_pays_for_almost_nothing() {
        let mut doc = TextDocument::default();
        let mut section = docsai_model::text::Section::default();
        section
            .blocks
            .push(Block::Paragraph(Paragraph::text("Hello.")));
        doc.sections.push(section);
        let found = features(&Document::Text(doc));
        assert_eq!(
            found,
            BTreeSet::from([Feature::FrontMatter]),
            "a bare paragraph documents the front matter and nothing else"
        );
    }
}
