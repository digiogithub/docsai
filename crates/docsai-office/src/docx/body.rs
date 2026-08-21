//! `word/document.xml` (and headers, footers and footnotes) → IR blocks.

use std::collections::BTreeMap;

use docsai_model::addressing::NodeId;
use docsai_model::assets::AssetStore;
use docsai_model::image::RawId;
use docsai_model::list::ListId;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::{StyleCatalog, StyleId};
use docsai_model::text::{
    Block, BreakKind, FieldKind, Footnote, Heading, Inline, List, ListItem, ParaFormat, Paragraph,
    RawFragment, RunProps, Table, TableCell, TableRow,
};
use docsai_model::units::Length;

use super::drawing::{read_drawing, read_vml_picture, DrawingContext};
use super::format::{hex_color, paragraph_props, run_props};
use super::numbering::Numbering;
use super::styles::heading_level;
use crate::package::{Package, Relationships};
use crate::xml::Element;

/// Everything a body walk needs to read, but never mutates.
pub struct Ctx<'a> {
    pub package: &'a Package,
    pub rels: &'a Relationships,
    /// Part being read, used for warning locations and raw-block provenance.
    pub part: &'a str,
    /// The part's text, so raw-blocks can quote the original bytes.
    pub source: &'a str,
    pub styles: &'a StyleCatalog,
    pub numbering: &'a Numbering,
    /// Footnote id → its blocks. Empty while reading `footnotes.xml` itself.
    pub footnotes: &'a BTreeMap<i64, Vec<Block>>,
}

impl Ctx<'_> {
    fn drawing(&self) -> DrawingContext<'_> {
        DrawingContext {
            package: self.package,
            rels: self.rels,
            part: self.part,
        }
    }
}

/// Everything a body walk mutates.
pub struct State<'a> {
    pub assets: &'a mut dyn AssetStore,
    pub report: &'a mut ConversionReport,
    /// Sequence behind `raw-0001`, `raw-0002`… Shared across the document so
    /// that ids are unique and stable.
    pub raw_seq: u32,
    /// Accepted tracked changes, reported once at the end.
    pub revisions: u32,
}

impl State<'_> {
    /// Preserves an unrecognised element verbatim (spec §7).
    fn raw(&mut self, element: &Element, ctx: &Ctx<'_>) -> RawFragment {
        self.raw_seq += 1;
        self.report.raw_block(
            qualified(element),
            format!("{}:{}", ctx.part, element.span.start),
        );
        RawFragment {
            id: RawId::new(format!("raw-{:04}", self.raw_seq)),
            format: "ooxml".into(),
            part: ctx.part.to_string(),
            content: element.raw(ctx.source).to_string(),
        }
    }
}

fn qualified(element: &Element) -> String {
    if element.prefix.is_empty() {
        element.name.clone()
    } else {
        format!("{}:{}", element.prefix, element.name)
    }
}

/// Elements that carry no content and need neither a block nor a warning.
const IGNORED: &[&str] = &[
    "bookmarkStart",
    "bookmarkEnd",
    "proofErr",
    "commentRangeStart",
    "commentRangeEnd",
    "permStart",
    "permEnd",
    "lastRenderedPageBreak",
    "sectPr",
    "tblPr",
    "tblGrid",
    "trPr",
    "tcPr",
    "pPr",
    "rPr",
];

// --------------------------------------------------------------------------
// Blocks
// --------------------------------------------------------------------------

/// One item of the flat body stream, before lists are rebuilt into a tree.
enum Flat {
    Block(Block),
    /// A paragraph carrying `w:numPr`, still flat as OOXML stores it.
    ListParagraph {
        num_id: i64,
        ilvl: u8,
        paragraph: Paragraph,
    },
}

/// Reads the block children of an element (a body, a table cell, a header…).
pub fn read_blocks(parent: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Vec<Block> {
    read_blocks_of(parent.children(), ctx, st)
}

/// Reads an explicit sequence of block elements.
///
/// Sections split the body at the paragraph that carries `w:sectPr`, so the
/// document reader needs to hand over slices rather than a whole parent.
pub fn read_blocks_of<'e>(
    children: impl IntoIterator<Item = &'e Element>,
    ctx: &Ctx<'_>,
    st: &mut State<'_>,
) -> Vec<Block> {
    let mut flat = Vec::new();
    collect_blocks(children, ctx, st, &mut flat);
    rebuild_lists(flat, ctx, st)
}

fn collect_blocks<'e>(
    children: impl IntoIterator<Item = &'e Element>,
    ctx: &Ctx<'_>,
    st: &mut State<'_>,
    out: &mut Vec<Flat>,
) {
    for child in children {
        match child.name.as_str() {
            "p" => {
                let (paragraph, numbering) = read_paragraph(child, ctx, st);
                match numbering {
                    Some((num_id, ilvl)) => out.push(Flat::ListParagraph {
                        num_id,
                        ilvl,
                        paragraph,
                    }),
                    None => {
                        st.report.stats.paragraphs += 1;
                        out.push(Flat::Block(as_heading_or_paragraph(paragraph, ctx, st)));
                    }
                }
            }
            "tbl" => {
                st.report.stats.tables += 1;
                out.push(Flat::Block(Block::Table(read_table(child, ctx, st))));
            }
            "sdt" => {
                // A content control has no DocMark representation, but its
                // text does: flatten to the content and say so, rather than
                // hiding readable text inside an opaque raw-block.
                st.report.warn(Warning::Degraded {
                    what: "w:sdt".into(),
                    why: "structured document tag flattened to its content".into(),
                });
                if let Some(content) = child.child("sdtContent") {
                    collect_blocks(content.children(), ctx, st, out);
                }
            }
            "customXml" => collect_blocks(child.children(), ctx, st, out),
            "ins" => {
                st.revisions += 1;
                collect_blocks(child.children(), ctx, st, out);
            }
            "del" => {
                st.revisions += 1;
            }
            name if IGNORED.contains(&name) => {}
            _ => {
                let fragment = st.raw(child, ctx);
                out.push(Flat::Block(Block::Raw(fragment)));
            }
        }
    }
}

fn as_heading_or_paragraph(paragraph: Paragraph, ctx: &Ctx<'_>, st: &mut State<'_>) -> Block {
    let level = paragraph
        .format
        .direct
        .outline_level
        .map(|l| l + 1)
        .or_else(|| {
            paragraph
                .format
                .style
                .as_ref()
                .and_then(|id| heading_level(ctx.styles, id))
        });
    match level {
        Some(level) if (1..=9).contains(&level) && !paragraph.is_empty() => {
            st.report.stats.headings += 1;
            // The bookmark belongs to the heading, the addressable node here
            // — not to the `Paragraph` nested inside it, which never takes
            // its own id (see `Paragraph::id`'s doc comment).
            let id = paragraph.id.clone();
            let paragraph = Paragraph {
                id: None,
                ..paragraph
            };
            Block::Heading(Heading {
                id,
                level,
                paragraph,
            })
        }
        _ => Block::Paragraph(paragraph),
    }
}

/// Rebuilds the list tree from the flat `(numId, ilvl)` pairs.
fn rebuild_lists(flat: Vec<Flat>, ctx: &Ctx<'_>, st: &mut State<'_>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut run: Vec<(i64, u8, Paragraph)> = Vec::new();

    for item in flat {
        match item {
            Flat::ListParagraph {
                num_id,
                ilvl,
                paragraph,
            } => {
                // A different list definition starts a different list.
                if run.first().is_some_and(|(id, _, _)| *id != num_id) {
                    flush_list(&mut run, ctx, st, &mut out);
                }
                run.push((num_id, ilvl, paragraph));
            }
            Flat::Block(block) => {
                flush_list(&mut run, ctx, st, &mut out);
                out.push(block);
            }
        }
    }
    flush_list(&mut run, ctx, st, &mut out);
    out
}

fn flush_list(
    run: &mut Vec<(i64, u8, Paragraph)>,
    ctx: &Ctx<'_>,
    st: &mut State<'_>,
    out: &mut Vec<Block>,
) {
    if run.is_empty() {
        return;
    }
    st.report.stats.lists += 1;
    st.report.stats.paragraphs += run.len() as u32;
    let items = std::mem::take(run);
    out.push(build_list(&items, ctx));
}

/// Turns a run of same-list paragraphs into a nested [`List`].
fn build_list(items: &[(i64, u8, Paragraph)], ctx: &Ctx<'_>) -> Block {
    let (num_id, base_level, _) = &items[0];
    let base = *base_level;
    let def: Option<ListId> = ctx.numbering.list_id(*num_id).cloned();
    let mut list = List {
        id: None,
        def,
        ordered: ctx.numbering.is_ordered(*num_id, base as usize),
        level: base,
        items: Vec::new(),
    };

    let mut i = 0;
    while i < items.len() {
        if items[i].1 <= base {
            list.items.push(ListItem {
                blocks: vec![Block::Paragraph(items[i].2.clone())],
            });
            i += 1;
        } else {
            let start = i;
            while i < items.len() && items[i].1 > base {
                i += 1;
            }
            let nested = build_list(&items[start..i], ctx);
            match list.items.last_mut() {
                // A deeper item with no parent above it (a malformed document)
                // still becomes a list, just an orphan one.
                Some(parent) => parent.blocks.push(nested),
                None => list.items.push(ListItem {
                    blocks: vec![nested],
                }),
            }
        }
    }
    Block::List(list)
}

// --------------------------------------------------------------------------
// Paragraphs
// --------------------------------------------------------------------------

/// Reads a `w:p`, returning its `(numId, ilvl)` when it is a list item.
fn read_paragraph(
    p: &Element,
    ctx: &Ctx<'_>,
    st: &mut State<'_>,
) -> (Paragraph, Option<(i64, u8)>) {
    let ppr = p.child("pPr");
    let mut format = ParaFormat::default();
    let mut numbering = None;

    if let Some(ppr) = ppr {
        format.style = ppr
            .child("pStyle")
            .and_then(|e| e.attr("val"))
            .map(StyleId::new);
        format.direct = paragraph_props(ppr);
        if let Some(rpr) = ppr.child("rPr") {
            format.run_direct = run_props(rpr);
        }
        if let Some(numpr) = ppr.child("numPr") {
            let num_id = numpr.child("numId").and_then(|e| e.attr_i64("val"));
            let ilvl = numpr
                .child("ilvl")
                .and_then(|e| e.attr_i64("val"))
                .unwrap_or(0)
                .clamp(0, 8) as u8;
            // `numId=0` explicitly means "no numbering".
            numbering = num_id.filter(|id| *id > 0).map(|id| (id, ilvl));
        }
    }

    let (content, bookmarks) = read_inlines_with_bookmarks(p, ctx, st);
    let id = bookmark_node_id(&bookmarks);
    (
        Paragraph {
            id,
            format,
            content,
        },
        numbering,
    )
}

/// Picks a bookmark to carry forward as the paragraph's (or, for a heading,
/// the heading's) DocMark id, so internal links that address it by that name
/// — a TOC's hyperlinks and `PAGEREF` fields, chiefly — still resolve after a
/// round trip. A paragraph can open more than one bookmark; the first
/// DocMark-legal name wins, since ordering in the source reflects nothing
/// meaningful here.
fn bookmark_node_id(bookmarks: &[String]) -> Option<NodeId> {
    bookmarks
        .iter()
        .map(|name| NodeId::new(name.clone()))
        .find(NodeId::is_valid)
}

/// A run may emit content *or* field-control markers; the paragraph-level loop
/// assembles the markers into fields.
enum Piece {
    // Boxed: `Inline` dwarfs the field-control markers, and a paragraph builds
    // a whole vector of these.
    Inline(Box<Inline>),
    FieldBegin,
    FieldSeparate,
    FieldEnd,
    Instruction(String),
    /// A `w:bookmarkStart` name, carried out so the paragraph it opens in can
    /// take it as its DocMark id. Internal hyperlinks and TOC/PAGEREF fields
    /// address headings by this name (e.g. `_Toc234329254`); dropping it
    /// silently, as `IGNORED` used to, leaves those links pointing nowhere.
    Bookmark(String),
}

impl Piece {
    fn inline(inline: Inline) -> Piece {
        Piece::Inline(Box::new(inline))
    }
}

struct FieldBuilder {
    instruction: String,
    result: Vec<Inline>,
    in_result: bool,
}

/// Reads the inline children of a paragraph (or of a hyperlink, or of an
/// inserted run range).
fn read_inlines(parent: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Vec<Inline> {
    read_inlines_with_bookmarks(parent, ctx, st).0
}

/// Same, plus the names of any `w:bookmarkStart` elements found directly in
/// `parent` (not inside nested hyperlinks or fields, where they don't occur
/// in practice).
fn read_inlines_with_bookmarks(
    parent: &Element,
    ctx: &Ctx<'_>,
    st: &mut State<'_>,
) -> (Vec<Inline>, Vec<String>) {
    let mut pieces = Vec::new();
    collect_pieces(parent, ctx, st, &mut pieces);

    let mut out: Vec<Inline> = Vec::new();
    let mut bookmarks: Vec<String> = Vec::new();
    let mut fields: Vec<FieldBuilder> = Vec::new();

    for piece in pieces {
        match piece {
            Piece::Bookmark(name) => bookmarks.push(name),
            Piece::FieldBegin => fields.push(FieldBuilder {
                instruction: String::new(),
                result: Vec::new(),
                in_result: false,
            }),
            Piece::Instruction(text) => {
                if let Some(field) = fields.last_mut() {
                    field.instruction.push_str(&text);
                }
            }
            Piece::FieldSeparate => {
                if let Some(field) = fields.last_mut() {
                    field.in_result = true;
                }
            }
            Piece::FieldEnd => {
                let Some(field) = fields.pop() else { continue };
                let inline = build_field(field);
                match fields.last_mut() {
                    Some(parent) => parent.result.push(inline),
                    None => out.push(inline),
                }
            }
            Piece::Inline(inline) => match fields.last_mut() {
                Some(field) if field.in_result => field.result.push(*inline),
                // Content between `begin` and `separate` is the instruction
                // rendering, which the field itself already carries.
                Some(_) => {}
                None => out.push(*inline),
            },
        }
    }

    // An unterminated field still has to surface its result text.
    for field in fields {
        out.push(build_field(field));
    }
    (out, bookmarks)
}

fn build_field(field: FieldBuilder) -> Inline {
    let cached = Paragraph::new(field.result).plain_text();
    Inline::Field {
        kind: FieldKind::from_instruction(&field.instruction),
        cached,
        instruction: field.instruction.trim().to_string(),
    }
}

fn collect_pieces(parent: &Element, ctx: &Ctx<'_>, st: &mut State<'_>, out: &mut Vec<Piece>) {
    for child in parent.children() {
        match child.name.as_str() {
            "r" => read_run(child, ctx, st, out),
            "hyperlink" => {
                let target = hyperlink_target(child, ctx);
                let content = read_inlines(child, ctx, st);
                let props = RunProps {
                    style: child
                        .path(&["r", "rPr", "rStyle"])
                        .and_then(|e| e.attr("val"))
                        .map(StyleId::new),
                    ..Default::default()
                };
                match target {
                    Some(target) => out.push(Piece::inline(Inline::Link {
                        target,
                        content,
                        props,
                    })),
                    // A hyperlink with no resolvable target is still text.
                    None => out.extend(content.into_iter().map(Piece::inline)),
                }
            }
            "fldSimple" => {
                let instruction = child.attr("instr").unwrap_or_default().trim().to_string();
                let cached = Paragraph::new(read_inlines(child, ctx, st)).plain_text();
                out.push(Piece::inline(Inline::Field {
                    kind: FieldKind::from_instruction(&instruction),
                    cached,
                    instruction,
                }));
            }
            "ins" | "smartTag" => {
                if child.name == "ins" {
                    st.revisions += 1;
                }
                collect_pieces(child, ctx, st, out);
            }
            "del" => {
                st.revisions += 1;
            }
            "sdt" => {
                if let Some(content) = child.child("sdtContent") {
                    collect_pieces(content, ctx, st, out);
                }
            }
            "commentReference" => st.report.warn(Warning::Degraded {
                what: "w:commentReference".into(),
                why: "comments are out of scope in v1".into(),
            }),
            "bookmarkStart" => {
                if let Some(name) = child.attr("name") {
                    out.push(Piece::Bookmark(name.to_string()));
                }
            }
            name if IGNORED.contains(&name) => {}
            _ => {
                let fragment = st.raw(child, ctx);
                out.push(Piece::inline(Inline::Raw(fragment)));
            }
        }
    }
}

fn hyperlink_target(link: &Element, ctx: &Ctx<'_>) -> Option<String> {
    if let Some(id) = link.attr_qualified("r:id") {
        if let Some(rel) = ctx.rels.get(id) {
            let anchor = link.attr("anchor").unwrap_or_default();
            return Some(if anchor.is_empty() {
                rel.target.clone()
            } else {
                format!("{}#{anchor}", rel.target)
            });
        }
    }
    link.attr("anchor").map(|a| format!("#{a}"))
}

fn read_run(r: &Element, ctx: &Ctx<'_>, st: &mut State<'_>, out: &mut Vec<Piece>) {
    let props = r
        .child("rPr")
        .map(|rpr| RunProps {
            style: rpr
                .child("rStyle")
                .and_then(|e| e.attr("val"))
                .map(StyleId::new),
            direct: run_props(rpr),
        })
        .unwrap_or_default();

    // Content of this run, buffered so that formatting wraps it in one span.
    let mut buffer: Vec<Inline> = Vec::new();
    macro_rules! flush {
        () => {
            if !buffer.is_empty() {
                let content = std::mem::take(&mut buffer);
                out.extend(
                    Inline::styled(content, props.clone())
                        .into_iter()
                        .map(Piece::inline),
                );
            }
        };
    }

    for child in r.children() {
        match child.name.as_str() {
            "t" => buffer.push(Inline::Text(child.text())),
            "tab" => buffer.push(Inline::Text("\t".into())),
            "noBreakHyphen" => buffer.push(Inline::Text("\u{2011}".into())),
            "softHyphen" => {}
            "br" => buffer.push(Inline::Break(match child.attr("type") {
                Some("page") => BreakKind::Page,
                Some("column") => BreakKind::Column,
                _ => BreakKind::Line,
            })),
            "cr" => buffer.push(Inline::Break(BreakKind::Line)),
            "sym" => {
                // A symbol font character: the code point is meaningless
                // without the font, so preserve it verbatim.
                let fragment = st.raw(child, ctx);
                buffer.push(Inline::Raw(fragment));
            }
            "drawing" => match read_drawing(child, &ctx.drawing(), st.assets, st.report) {
                Ok(Some(image)) => buffer.push(Inline::Image(Box::new(image))),
                Ok(None) => {
                    let fragment = st.raw(child, ctx);
                    buffer.push(Inline::Raw(fragment));
                }
                Err(e) => st.report.warn(Warning::AssetIssue {
                    asset: ctx.part.to_string(),
                    why: e.to_string(),
                }),
            },
            "pict" | "object" => {
                match read_vml_picture(child, &ctx.drawing(), st.assets, st.report) {
                    Ok(Some(image)) => buffer.push(Inline::Image(Box::new(image))),
                    Ok(None) => {
                        let fragment = st.raw(child, ctx);
                        buffer.push(Inline::Raw(fragment));
                    }
                    Err(e) => st.report.warn(Warning::AssetIssue {
                        asset: ctx.part.to_string(),
                        why: e.to_string(),
                    }),
                }
            }
            "footnoteReference" | "endnoteReference" => {
                let id = child.attr_i64("id").unwrap_or(-1);
                match ctx.footnotes.get(&id) {
                    Some(blocks) => {
                        st.report.stats.footnotes += 1;
                        // The reference sits *outside* the run's character
                        // style: `FootnoteReference` only makes the marker
                        // superscript, and DocMark draws its own marker.
                        flush!();
                        out.push(Piece::inline(Inline::Footnote(Footnote::new(
                            blocks.clone(),
                        ))));
                    }
                    None => st.report.warn(Warning::Degraded {
                        what: format!("footnote {id}"),
                        why: "referenced note not found in the package".into(),
                    }),
                }
            }
            "footnoteRef" | "endnoteRef" | "separator" | "continuationSeparator" => {}
            "fldChar" => {
                flush!();
                match child.attr("fldCharType") {
                    Some("begin") => out.push(Piece::FieldBegin),
                    Some("separate") => out.push(Piece::FieldSeparate),
                    Some("end") => out.push(Piece::FieldEnd),
                    _ => {}
                }
            }
            "instrText" => {
                flush!();
                out.push(Piece::Instruction(child.text()));
            }
            name if IGNORED.contains(&name) => {}
            _ => {
                let fragment = st.raw(child, ctx);
                buffer.push(Inline::Raw(fragment));
            }
        }
    }
    flush!();
}

// --------------------------------------------------------------------------
// Tables
// --------------------------------------------------------------------------

fn read_table(tbl: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Table {
    let mut table = Table {
        style: tbl
            .path(&["tblPr", "tblStyle"])
            .and_then(|e| e.attr("val"))
            .map(StyleId::new),
        col_widths: tbl
            .child("tblGrid")
            .map(|grid| {
                grid.children_named("gridCol")
                    .map(|c| Length::from_twips(c.attr_i64("w").unwrap_or(0)))
                    .collect()
            })
            .unwrap_or_default(),
        ..Default::default()
    };

    // `vMerge` continuations point back at the restart cell above them, so
    // rowspans are resolved after the grid is known.
    let mut open_merges: BTreeMap<usize, (usize, usize)> = BTreeMap::new();

    for tr in tbl.children_named("tr") {
        let mut row = TableRow {
            is_header: tr.path(&["trPr", "tblHeader"]).is_some(),
            ..Default::default()
        };
        let mut grid_col = 0usize;

        for tc in tr.children_named("tc") {
            let tcpr = tc.child("tcPr");
            let colspan = tcpr
                .and_then(|p| p.child("gridSpan"))
                .and_then(|e| e.attr_i64("val"))
                .unwrap_or(1)
                .clamp(1, 1024) as u16;
            let vmerge = tcpr.and_then(|p| p.child("vMerge"));
            let continues = vmerge.is_some_and(|e| !matches!(e.attr("val"), Some("restart")));

            let mut cell = TableCell {
                blocks: read_blocks(tc, ctx, st),
                colspan,
                rowspan: 1,
                covered: false,
                width: tcpr
                    .and_then(|p| p.child("tcW"))
                    .filter(|w| w.attr("type") != Some("auto"))
                    .and_then(|w| w.attr_i64("w"))
                    .map(Length::from_twips),
                background: tcpr
                    .and_then(|p| p.child("shd"))
                    .and_then(|s| s.attr("fill"))
                    .and_then(hex_color),
            };

            if vmerge.is_some() {
                if continues {
                    if let Some((row_index, cell_index)) = open_merges.get(&grid_col).copied() {
                        if let Some(target) = table
                            .rows
                            .get_mut(row_index)
                            .and_then(|r| r.cells.get_mut(cell_index))
                        {
                            target.rowspan = target.rowspan.saturating_add(1);
                        }
                    }
                    cell.covered = true;
                    cell.blocks.clear();
                } else {
                    open_merges.insert(grid_col, (table.rows.len(), row.cells.len()));
                }
            } else {
                open_merges.remove(&grid_col);
            }

            grid_col += colspan as usize;
            row.cells.push(cell);
        }
        table.rows.push(row);
    }

    table.header_row = table.rows.first().is_some_and(|r| r.is_header);
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    #[derive(Default)]
    struct Fixture {
        package: Package,
        styles: StyleCatalog,
        numbering: Numbering,
        footnotes: BTreeMap<i64, Vec<Block>>,
    }

    fn read(xml: &str, fixture: &Fixture) -> (Vec<Block>, ConversionReport) {
        let root = Element::parse("word/document.xml", xml.as_bytes()).unwrap();
        let rels = Relationships::default();
        let ctx = Ctx {
            package: &fixture.package,
            rels: &rels,
            part: "word/document.xml",
            source: xml,
            styles: &fixture.styles,
            numbering: &fixture.numbering,
            footnotes: &fixture.footnotes,
        };
        let mut assets = MemoryAssetStore::new();
        let mut report = ConversionReport::new();
        let mut st = State {
            assets: &mut assets,
            report: &mut report,
            raw_seq: 0,
            revisions: 0,
        };
        let blocks = read_blocks(&root, &ctx, &mut st);
        (blocks, report)
    }

    const NS: &str = r#"xmlns:w="urn:w" xmlns:r="urn:r""#;

    #[test]
    fn reads_runs_and_their_formatting() {
        let xml = format!(
            r#"<w:body {NS}><w:p>
                 <w:r><w:t xml:space="preserve">normal </w:t></w:r>
                 <w:r><w:rPr><w:b/></w:rPr><w:t>negrita</w:t></w:r>
               </w:p></w:body>"#
        );
        let (blocks, _) = read(&xml, &Fixture::default());
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("expected a paragraph, got {:?}", blocks[0])
        };
        assert_eq!(p.plain_text(), "normal negrita");
        assert!(matches!(p.content[1], Inline::Styled { .. }));
    }

    #[test]
    fn breaks_and_tabs_survive() {
        let xml = format!(
            r#"<w:body {NS}><w:p><w:r><w:t>a</w:t><w:br/><w:t>b</w:t><w:tab/>
               <w:br w:type="page"/></w:r></w:p></w:body>"#
        );
        let (blocks, _) = read(&xml, &Fixture::default());
        let Block::Paragraph(p) = &blocks[0] else {
            unreachable!()
        };
        assert!(p
            .content
            .iter()
            .any(|i| matches!(i, Inline::Break(BreakKind::Line))));
        assert!(p
            .content
            .iter()
            .any(|i| matches!(i, Inline::Break(BreakKind::Page))));
        assert!(p.plain_text().contains('\t'));
    }

    #[test]
    fn rebuilds_a_nested_list_from_flat_numbering() {
        let mut fixture = Fixture::default();
        let numbering_xml = r#"<w:numbering xmlns:w="urn:w">
            <w:abstractNum w:abstractNumId="0">
              <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
              <w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/></w:lvl>
            </w:abstractNum>
            <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#;
        let mut discard = ConversionReport::new();
        fixture.numbering = super::super::numbering::read_numbering(
            &Element::parse("n.xml", numbering_xml.as_bytes()).unwrap(),
            &mut discard,
        );

        let item = |text: &str, lvl: u8| {
            format!(
                r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{lvl}"/><w:numId w:val="1"/></w:numPr></w:pPr>
                   <w:r><w:t>{text}</w:t></w:r></w:p>"#
            )
        };
        let xml = format!(
            "<w:body {NS}>{}{}{}{}</w:body>",
            item("uno", 0),
            item("uno.a", 1),
            item("uno.b", 1),
            item("dos", 0)
        );
        let (blocks, _) = read(&xml, &fixture);
        assert_eq!(blocks.len(), 1, "one list, not four paragraphs");
        let Block::List(list) = &blocks[0] else {
            panic!("expected a list")
        };
        assert!(list.ordered);
        assert_eq!(list.items.len(), 2);
        let nested = list.items[0].blocks.iter().find_map(|b| match b {
            Block::List(l) => Some(l),
            _ => None,
        });
        assert_eq!(nested.expect("nested list").items.len(), 2);
    }

    #[test]
    fn num_id_zero_is_not_a_list() {
        let xml = format!(
            r#"<w:body {NS}><w:p><w:pPr><w:numPr><w:numId w:val="0"/></w:numPr></w:pPr>
               <w:r><w:t>x</w:t></w:r></w:p></w:body>"#
        );
        let (blocks, _) = read(&xml, &Fixture::default());
        assert!(matches!(blocks[0], Block::Paragraph(_)));
    }

    #[test]
    fn resolves_vertical_and_horizontal_merges() {
        let xml = format!(
            r#"<w:body {NS}><w:tbl>
              <w:tblGrid><w:gridCol w:w="2500"/><w:gridCol w:w="2500"/><w:gridCol w:w="2000"/></w:tblGrid>
              <w:tr><w:trPr><w:tblHeader/></w:trPr>
                <w:tc><w:p><w:r><w:t>Region</w:t></w:r></w:p></w:tc>
                <w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Trimestres</w:t></w:r></w:p></w:tc>
              </w:tr>
              <w:tr>
                <w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>Norte</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>100</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>200</w:t></w:r></w:p></w:tc>
              </w:tr>
              <w:tr>
                <w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>
                <w:tc><w:p><w:r><w:t>150</w:t></w:r></w:p></w:tc>
                <w:tc><w:p><w:r><w:t>250</w:t></w:r></w:p></w:tc>
              </w:tr>
            </w:tbl></w:body>"#
        );
        let (blocks, _) = read(&xml, &Fixture::default());
        let Block::Table(table) = &blocks[0] else {
            panic!("expected a table")
        };
        assert_eq!(table.col_widths.len(), 3);
        assert!(table.header_row);
        assert_eq!(table.rows[0].cells[1].colspan, 2);
        assert_eq!(table.rows[1].cells[0].rowspan, 2, "vMerge became a rowspan");
        assert!(table.rows[2].cells[0].covered);
        assert_eq!(table.width(), 3);
    }

    #[test]
    fn assembles_complex_fields_from_their_run_sequence() {
        let xml = format!(
            r#"<w:body {NS}><w:p>
                 <w:r><w:fldChar w:fldCharType="begin"/></w:r>
                 <w:r><w:instrText xml:space="preserve"> PAGE </w:instrText></w:r>
                 <w:r><w:fldChar w:fldCharType="separate"/></w:r>
                 <w:r><w:t>7</w:t></w:r>
                 <w:r><w:fldChar w:fldCharType="end"/></w:r>
               </w:p></w:body>"#
        );
        let (blocks, _) = read(&xml, &Fixture::default());
        let Block::Paragraph(p) = &blocks[0] else {
            unreachable!()
        };
        match &p.content[0] {
            Inline::Field {
                kind,
                cached,
                instruction,
            } => {
                assert_eq!(*kind, FieldKind::Page);
                assert_eq!(cached, "7");
                assert_eq!(instruction, "PAGE");
            }
            other => panic!("expected a field, got {other:?}"),
        }
    }

    #[test]
    fn simple_fields_keep_their_instruction() {
        let xml = format!(
            r#"<w:body {NS}><w:p><w:fldSimple w:instr=" DATE \@ &quot;dd/MM/yyyy&quot; ">
               <w:r><w:t>01/01/2026</w:t></w:r></w:fldSimple></w:p></w:body>"#
        );
        let (blocks, _) = read(&xml, &Fixture::default());
        let Block::Paragraph(p) = &blocks[0] else {
            unreachable!()
        };
        let Inline::Field {
            kind,
            cached,
            instruction,
        } = &p.content[0]
        else {
            panic!("expected a field")
        };
        assert_eq!(*kind, FieldKind::Date);
        assert_eq!(cached, "01/01/2026");
        assert!(instruction.contains("dd/MM/yyyy"));
    }

    #[test]
    fn unknown_elements_become_raw_blocks_with_their_bytes() {
        let xml = format!(r#"<w:body {NS}><w:weirdThing a="1"><w:x/></w:weirdThing></w:body>"#);
        let (blocks, report) = read(&xml, &Fixture::default());
        let Block::Raw(raw) = &blocks[0] else {
            panic!("expected a raw block")
        };
        assert_eq!(raw.format, "ooxml");
        assert!(raw.content.starts_with("<w:weirdThing"));
        assert!(raw.content.ends_with("</w:weirdThing>"));
        assert_eq!(report.raw_blocks_emitted, 1);
    }

    #[test]
    fn content_controls_keep_their_text_visible() {
        let xml = format!(
            r#"<w:body {NS}><w:sdt><w:sdtPr/><w:sdtContent>
               <w:p><w:r><w:t>dentro</w:t></w:r></w:p></w:sdtContent></w:sdt></w:body>"#
        );
        let (blocks, report) = read(&xml, &Fixture::default());
        assert_eq!(blocks.len(), 1);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("sdt content should be flattened to blocks")
        };
        assert_eq!(p.plain_text(), "dentro");
        assert!(report
            .warnings
            .iter()
            .any(|w| w.message().contains("w:sdt")));
    }

    #[test]
    fn tracked_changes_are_taken_as_accepted() {
        let xml = format!(
            r#"<w:body {NS}><w:p>
                 <w:ins><w:r><w:t>añadido </w:t></w:r></w:ins>
                 <w:del><w:r><w:delText>borrado</w:delText></w:r></w:del>
                 <w:r><w:t>final</w:t></w:r>
               </w:p></w:body>"#
        );
        let (blocks, _) = read(&xml, &Fixture::default());
        let Block::Paragraph(p) = &blocks[0] else {
            unreachable!()
        };
        assert_eq!(p.plain_text(), "añadido final");
    }

    #[test]
    fn headings_come_from_the_style_catalogue() {
        let mut fixture = Fixture::default();
        let styles_xml = r#"<w:styles xmlns:w="urn:w">
            <w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/>
              <w:pPr><w:outlineLvl w:val="1"/></w:pPr></w:style></w:styles>"#;
        fixture.styles = super::super::styles::read_styles(
            &Element::parse("s.xml", styles_xml.as_bytes()).unwrap(),
        );
        let xml = format!(
            r#"<w:body {NS}><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr>
               <w:r><w:t>Titulo</w:t></w:r></w:p></w:body>"#
        );
        let (blocks, _) = read(&xml, &fixture);
        let Block::Heading(h) = &blocks[0] else {
            panic!("expected a heading")
        };
        assert_eq!(h.level, 2);
        assert_eq!(h.paragraph.format.style, Some(StyleId::new("Heading2")));
    }
}
