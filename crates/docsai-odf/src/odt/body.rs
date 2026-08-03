//! ODT `office:text` → IR blocks.

use docsai_model::assets::AssetStore;
use docsai_model::image::RawId;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::{FontProps, ParaProps, StyleId};
use docsai_model::text::{
    Block, BreakKind, Footnote, Heading, Inline, List, ListItem, ParaFormat, Paragraph,
    RawFragment, RunProps, Table, TableCell, TableRow,
};
use docsai_model::units::Length;

use crate::draw;
use crate::package::Package;
use crate::styles::{heading_level_from_style, OdfStyles};
use crate::xml::{Element, Node};

pub struct Ctx<'a> {
    pub package: &'a Package,
    pub part: &'a str,
    pub source: &'a str,
    pub styles: &'a OdfStyles,
}

pub struct State<'a> {
    pub assets: &'a mut dyn AssetStore,
    pub report: &'a mut ConversionReport,
    pub raw_seq: u32,
}

impl State<'_> {
    fn raw(&mut self, element: &Element, ctx: &Ctx<'_>) -> RawFragment {
        self.raw_seq += 1;
        self.report.raw_block(
            qualified(element),
            format!("{}:{}", ctx.part, element.span.start),
        );
        RawFragment {
            id: RawId::new(format!("raw-{:04}", self.raw_seq)),
            format: "odf".into(),
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

/// Reads block children of an office:text, table-cell, list-item, etc.
pub fn read_blocks(parent: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Vec<Block> {
    let mut blocks = Vec::new();
    for child in parent.children() {
        if let Some(block) = read_block(child, ctx, st) {
            blocks.push(block);
        }
    }
    blocks
}

fn read_block(element: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Option<Block> {
    match element.name.as_str() {
        "p" => Some(Block::Paragraph(read_paragraph(element, ctx, st))),
        "h" => Some(read_heading(element, ctx, st)),
        "list" => Some(Block::List(read_list(element, ctx, st, 0))),
        "table" => Some(Block::Table(read_table(element, ctx, st))),
        "frame" => {
            if let Some(img) = draw::read_frame(
                element,
                ctx.package,
                ctx.part,
                ctx.styles,
                st.assets,
                st.report,
            ) {
                Some(Block::Image(img))
            } else {
                Some(Block::Raw(st.raw(element, ctx)))
            }
        }
        "section" => {
            let mut out = Vec::new();
            for child in element.children() {
                if let Some(b) = read_block(child, ctx, st) {
                    out.push(b);
                }
            }
            if out.len() == 1 {
                out.pop()
            } else if out.is_empty() {
                None
            } else {
                // Preserve full markup rather than silently dropping siblings.
                Some(Block::Raw(st.raw(element, ctx)))
            }
        }
        "soft-page-break"
        | "sequence-decls"
        | "variable-decls"
        | "user-field-decls"
        | "dde-connection-decls"
        | "forms" => None,
        "table-of-content" | "illustration-index" | "table-index" | "object-index"
        | "user-index" | "alphabetical-index" | "bibliography" => {
            Some(Block::Raw(st.raw(element, ctx)))
        }
        other if other.ends_with("-decls") || other.ends_with("-decl") => None,
        _ => Some(Block::Raw(st.raw(element, ctx))),
    }
}

fn read_heading(element: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Block {
    let level = element
        .attr("outline-level")
        .or_else(|| element.attr_qualified("text:outline-level"))
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|l| (1..=9).contains(l))
        .unwrap_or(1);
    let paragraph = read_paragraph(element, ctx, st);
    st.report.stats.headings += 1;
    Block::Heading(Heading {
        id: None,
        level,
        paragraph,
    })
}

fn read_paragraph(element: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Paragraph {
    let style_name = element
        .attr("style-name")
        .or_else(|| element.attr_qualified("text:style-name"));
    let (style, mut direct, run_direct) = ctx.styles.resolve_para_style(style_name);
    if let Some(ref sid) = style {
        if direct.outline_level.is_none() {
            if let Some(lvl) = heading_level_from_style(ctx.styles, sid) {
                direct.outline_level = Some(lvl.saturating_sub(1));
            }
        }
    }
    let content = read_inlines(element, ctx, st);
    st.report.stats.paragraphs += 1;
    Paragraph {
        id: None,
        format: ParaFormat {
            style,
            direct,
            run_direct,
        },
        content,
    }
}

fn read_inlines(parent: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Vec<Inline> {
    let mut out = Vec::new();
    for node in &parent.children {
        match node {
            Node::Text(t) => {
                if !t.is_empty() {
                    out.push(Inline::Text(t.clone()));
                }
            }
            Node::Element(e) => match e.name.as_str() {
                "span" => {
                    let style_name = e
                        .attr("style-name")
                        .or_else(|| e.attr_qualified("text:style-name"));
                    let (style, direct) = ctx.styles.resolve_text_style(style_name);
                    let content = read_inlines(e, ctx, st);
                    let props = RunProps { style, direct };
                    if props.is_empty() {
                        out.extend(content);
                    } else {
                        out.push(Inline::Styled { content, props });
                    }
                }
                "a" => {
                    let target = e
                        .attr_qualified("xlink:href")
                        .or_else(|| e.attr("href"))
                        .unwrap_or("")
                        .to_string();
                    let content = read_inlines(e, ctx, st);
                    out.push(Inline::Link {
                        target,
                        content,
                        props: RunProps::default(),
                    });
                }
                "line-break" => out.push(Inline::Break(BreakKind::Line)),
                "s" => {
                    let n = e
                        .attr("c")
                        .or_else(|| e.attr_qualified("text:c"))
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(1)
                        .min(1024);
                    out.push(Inline::Text(" ".repeat(n)));
                }
                "tab" => out.push(Inline::Text("\t".into())),
                "frame" => {
                    if let Some(img) =
                        draw::read_frame(e, ctx.package, ctx.part, ctx.styles, st.assets, st.report)
                    {
                        out.push(Inline::Image(Box::new(img)));
                    } else {
                        out.push(Inline::Raw(st.raw(e, ctx)));
                    }
                }
                "bookmark-start"
                | "bookmark-end"
                | "bookmark"
                | "reference-mark"
                | "reference-mark-start"
                | "reference-mark-end"
                | "toc-mark"
                | "toc-mark-start"
                | "toc-mark-end"
                | "soft-page-break"
                | "change"
                | "change-start"
                | "change-end" => {}
                "note" => {
                    let body = e
                        .child("note-body")
                        .map(|b| read_blocks(b, ctx, st))
                        .unwrap_or_default();
                    st.report.stats.footnotes += 1;
                    out.push(Inline::Footnote(Footnote::new(body)));
                }
                "page-number" => {
                    out.push(Inline::Field {
                        kind: docsai_model::text::FieldKind::Page,
                        cached: e.deep_text(),
                        instruction: "PAGE".into(),
                    });
                }
                "page-count" => {
                    out.push(Inline::Field {
                        kind: docsai_model::text::FieldKind::NumPages,
                        cached: e.deep_text(),
                        instruction: "NUMPAGES".into(),
                    });
                }
                "ruby" => out.extend(read_inlines(e, ctx, st)),
                _ => {
                    if matches!(e.name.as_str(), "p" | "h" | "list" | "table") {
                        out.push(Inline::Raw(st.raw(e, ctx)));
                    } else {
                        let text = e.deep_text();
                        if text.is_empty() {
                            out.push(Inline::Raw(st.raw(e, ctx)));
                        } else {
                            st.report.warn(Warning::Degraded {
                                what: qualified(e),
                                why: "inline element flattened to text".into(),
                            });
                            out.push(Inline::Text(text));
                        }
                    }
                }
            },
        }
    }
    out
}

fn read_list(element: &Element, ctx: &Ctx<'_>, st: &mut State<'_>, level: u8) -> List {
    let style_name = element
        .attr("style-name")
        .or_else(|| element.attr_qualified("text:style-name"));
    let def = style_name
        .and_then(|n| ctx.styles.list_names.get(n))
        .cloned();
    let ordered = def
        .as_ref()
        .and_then(|id| ctx.styles.lists.get(id))
        .map(|d| d.is_ordered_at(level as usize))
        .unwrap_or(false);

    let mut items = Vec::new();
    for item_el in element.children_named("list-item") {
        let mut blocks = Vec::new();
        for child in item_el.children() {
            match child.name.as_str() {
                "list" => {
                    blocks.push(Block::List(read_list(
                        child,
                        ctx,
                        st,
                        level.saturating_add(1),
                    )));
                }
                _ => {
                    if let Some(b) = read_block(child, ctx, st) {
                        blocks.push(b);
                    }
                }
            }
        }
        items.push(ListItem { blocks });
    }
    st.report.stats.lists += 1;
    List {
        id: None,
        def,
        ordered,
        level,
        items,
    }
}

fn read_table(element: &Element, ctx: &Ctx<'_>, st: &mut State<'_>) -> Table {
    let style = element
        .attr("style-name")
        .or_else(|| element.attr_qualified("table:style-name"))
        .map(|n| StyleId::new(crate::styles::odf_style_id(n)));

    let mut col_widths = Vec::new();
    for col in element.children_named("table-column") {
        let repeat = col
            .attr("number-columns-repeated")
            .or_else(|| col.attr_qualified("table:number-columns-repeated"))
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(1)
            .min(256);
        let w = Length::ZERO;
        for _ in 0..repeat {
            col_widths.push(w);
        }
    }

    let mut rows = Vec::new();
    let mut header_row = false;
    for child in element.children() {
        match child.name.as_str() {
            "table-header-rows" => {
                header_row = true;
                for row in child.children_named("table-row") {
                    rows.push(read_row(row, ctx, st, true));
                }
            }
            "table-row" => rows.push(read_row(child, ctx, st, false)),
            "table-rows" => {
                for row in child.children_named("table-row") {
                    rows.push(read_row(row, ctx, st, false));
                }
            }
            _ => {}
        }
    }

    st.report.stats.tables += 1;
    Table {
        id: None,
        style,
        col_widths,
        rows,
        header_row,
    }
}

fn read_row(element: &Element, ctx: &Ctx<'_>, st: &mut State<'_>, is_header: bool) -> TableRow {
    let mut cells = Vec::new();
    for child in element.children() {
        match child.name.as_str() {
            "table-cell" => {
                let colspan = child
                    .attr("number-columns-spanned")
                    .or_else(|| child.attr_qualified("table:number-columns-spanned"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1u16)
                    .max(1);
                let rowspan = child
                    .attr("number-rows-spanned")
                    .or_else(|| child.attr_qualified("table:number-rows-spanned"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1u16)
                    .max(1);
                let repeat = child
                    .attr("number-columns-repeated")
                    .or_else(|| child.attr_qualified("table:number-columns-repeated"))
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1)
                    .min(256);
                let blocks = read_blocks(child, ctx, st);
                let cell = TableCell {
                    blocks,
                    colspan,
                    rowspan,
                    covered: false,
                    width: None,
                    background: None,
                };
                for _ in 0..repeat {
                    cells.push(cell.clone());
                }
            }
            "covered-table-cell" => {
                let repeat = child
                    .attr("number-columns-repeated")
                    .or_else(|| child.attr_qualified("table:number-columns-repeated"))
                    .and_then(|s| s.parse::<usize>().ok())
                    .unwrap_or(1)
                    .min(256);
                for _ in 0..repeat {
                    cells.push(TableCell {
                        blocks: Vec::new(),
                        colspan: 1,
                        rowspan: 1,
                        covered: true,
                        width: None,
                        background: None,
                    });
                }
            }
            _ => {}
        }
    }
    TableRow {
        id: None,
        cells,
        is_header,
    }
}

#[allow(dead_code)]
pub fn empty_para() -> (Option<StyleId>, ParaProps, FontProps) {
    (None, ParaProps::default(), FontProps::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles::{read_automatic_styles, read_named_styles};
    use crate::xml::Element;
    use docsai_model::MemoryAssetStore;

    #[test]
    fn reads_paragraph_with_span_and_break() {
        let content = r#"
        <office:text xmlns:text="urn:t" xmlns:office="urn:o">
          <text:p text:style-name="Standard">Hello <text:span text:style-name="T1">world</text:span><text:line-break/>next</text:p>
        </office:text>"#;
        let auto = r#"
        <office:automatic-styles xmlns:office="urn:o" xmlns:style="urn:s" xmlns:fo="urn:f">
          <style:style style:name="T1" style:family="text">
            <style:text-properties fo:font-weight="bold"/>
          </style:style>
        </office:automatic-styles>"#;
        let mut styles = OdfStyles::default();
        read_named_styles(
            &Element::parse(
                "s.xml",
                br#"<office:styles xmlns:style="urn:s">
                  <style:style style:name="Standard" style:family="paragraph"/>
                </office:styles>"#,
            )
            .unwrap(),
            &mut styles,
        );
        read_automatic_styles(
            &Element::parse("a.xml", auto.as_bytes()).unwrap(),
            &mut styles,
        );
        let root = Element::parse("c.xml", content.as_bytes()).unwrap();
        let package = Package::new();
        let ctx = Ctx {
            package: &package,
            part: "content.xml",
            source: content,
            styles: &styles,
        };
        let mut assets = MemoryAssetStore::new();
        let mut report = ConversionReport::new();
        let mut st = State {
            assets: &mut assets,
            report: &mut report,
            raw_seq: 0,
        };
        let blocks = read_blocks(&root, &ctx, &mut st);
        assert_eq!(blocks.len(), 1);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("{:?}", blocks[0]);
        };
        assert!(p.plain_text().contains("Hello"));
        assert!(p.plain_text().contains("world"));
        assert!(p.content.iter().any(|i| matches!(i, Inline::Break(_))));
        assert!(matches!(
            p.content.iter().find(|i| matches!(i, Inline::Styled { .. })),
            Some(Inline::Styled { props, .. }) if props.direct.bold == Some(true)
        ));
    }
}
