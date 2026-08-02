//! DocMark → IR body parser.
//!
//! Hand-written to mirror `writer.rs`, so `serialize(parse(md))` stays stable
//! for the shapes our serializer emits.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use docsai_model::assets::AssetStore;
use docsai_model::image::RawId;
use docsai_model::image::{
    AlignKeyword, Anchor, AxisPos, CropRect, Flip, HVPos, ImageGeometry, ImageRef, RelBase,
    SimpleBorder, WrapMode, WrapSide,
};
use docsai_model::list::ListId;
use docsai_model::report::ConversionReport;
use docsai_model::style::{Align, FontProps, StyleId, Underline, VertAlign};
use docsai_model::text::{
    Block, BreakKind, FieldKind, HeaderFooter, HeaderScope, Heading, Inline, List, ListItem,
    Orientation, ParaFormat, Paragraph, RawFragment, RunProps, Section, Table, TableCell, TableRow,
    TextBox, TextDocument,
};
use docsai_model::units::{Length, Size};
use docsai_model::{ConversionReport as Report, Document};

use crate::attrs::Attrs;
use crate::error::ParseError;
use crate::escape::unescape;
use crate::frontmatter_parse::{self, FrontMatter};

/// Parses DocMark markdown into the IR.
pub fn parse(
    markdown: &str,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ParseError> {
    parse_with_base(markdown, None, assets)
}

/// Parses DocMark, resolving relative image paths against `base_dir`.
pub fn parse_with_base(
    markdown: &str,
    base_dir: Option<&Path>,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ParseError> {
    let markdown = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let mut report = ConversionReport::new();
    let (fm, body, body_line) = split_front_matter(&markdown)?;
    let fm = match fm {
        Some(text) => frontmatter_parse::parse(text, 2)?,
        None => FrontMatter::default(),
    };

    if crate::sheet_parser::looks_like_workbook(&fm, body) {
        let doc = crate::sheet_parser::parse_workbook(
            body,
            body_line,
            fm,
            base_dir,
            assets,
            &mut report,
        )?;
        return Ok((doc, report));
    }

    let mut parser = BodyParser {
        base_dir: base_dir.map(Path::to_path_buf),
        assets,
        report: &mut report,
        footnotes: BTreeMap::new(),
        _styles: fm.styles.clone(),
    };

    let blocks = parser.parse_blocks(body, body_line)?;
    let (headers, footers, body_blocks, nested_sections) = partition_structure(blocks);

    let mut sections = Vec::new();
    if nested_sections.is_empty() {
        let mut section = Section {
            page: fm.page.unwrap_or_default(),
            headers,
            footers,
            blocks: body_blocks,
        };
        if section.page.columns == 0 {
            section.page.columns = 1;
        }
        sections.push(section);
    } else {
        // Multi-section documents: headers/footers before the first section
        // apply to section 0; each `.section` container is its own section.
        for (index, (attrs, blocks)) in nested_sections.into_iter().enumerate() {
            let mut section = Section {
                page: fm.page.unwrap_or_default(),
                blocks,
                ..Default::default()
            };
            if index == 0 {
                section.headers = headers.clone();
                section.footers = footers.clone();
            }
            apply_section_attrs(&mut section, &attrs);
            sections.push(section);
        }
        if !body_blocks.is_empty() && sections.is_empty() {
            sections.push(Section {
                page: fm.page.unwrap_or_default(),
                headers,
                footers,
                blocks: body_blocks,
            });
        }
    }

    // Attach footnote definitions collected from the body.
    attach_footnotes(&mut sections, &parser.footnotes);

    let doc = Document::Text(TextDocument {
        meta: fm.meta,
        styles: fm.styles,
        list_defs: fm.list_defs,
        sections,
    });
    report.stats.styles = match &doc {
        Document::Text(t) => t.styles.styles.len() as u32,
        Document::Workbook(w) => w.styles.styles.len() as u32,
    };
    Ok((doc, report))
}

fn split_front_matter(markdown: &str) -> Result<(Option<&str>, &str, usize), ParseError> {
    let text = markdown;
    if !text.starts_with("---") {
        return Ok((None, text, 1));
    }
    // Opening fence must be a whole line.
    let after_open = if let Some(rest) = text.strip_prefix("---\n") {
        rest
    } else if text == "---" {
        return Err(ParseError::front_matter(1, "unclosed front matter"));
    } else {
        // Not a line-bounded opening fence (e.g. `---foo`).
        return Ok((None, text, 1));
    };

    let mut search = after_open;
    let mut offset = 0usize;
    loop {
        if let Some(idx) = search.find("\n---") {
            let after = &search[idx + 1..]; // starts at ---
            if after == "---" || after.starts_with("---\n") || after.starts_with("---\r") {
                let yaml = &after_open[..offset + idx];
                let body = if let Some(rest) = after.strip_prefix("---\n") {
                    rest
                } else if after == "---" {
                    ""
                } else if let Some(rest) = after.strip_prefix("---\r\n") {
                    rest
                } else if let Some(rest) = after.strip_prefix("---\r") {
                    rest
                } else {
                    after.get(3..).unwrap_or("")
                };
                // body starts after the closing fence line
                let start_line = 2 + yaml.bytes().filter(|b| *b == b'\n').count() + 1;
                // skip optional blank line after closing fence
                let body = body.strip_prefix('\n').unwrap_or(body);
                let start_line = start_line + 1;
                return Ok((Some(yaml), body, start_line));
            }
            offset += idx + 1;
            search = &after_open[offset..];
        } else {
            return Err(ParseError::front_matter(1, "unclosed front matter"));
        }
    }
}

#[allow(clippy::type_complexity)]
fn partition_structure(
    blocks: Vec<ParsedBlock>,
) -> (
    Vec<HeaderFooter>,
    Vec<HeaderFooter>,
    Vec<Block>,
    Vec<(Attrs, Vec<Block>)>,
) {
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    let mut body = Vec::new();
    let mut sections = Vec::new();
    for block in blocks {
        match block {
            ParsedBlock::Header(h) => headers.push(h),
            ParsedBlock::Footer(f) => footers.push(f),
            ParsedBlock::Section { attrs, blocks } => sections.push((attrs, blocks)),
            ParsedBlock::Normal(b) => body.push(b),
        }
    }
    (headers, footers, body, sections)
}

fn apply_section_attrs(section: &mut Section, attrs: &Attrs) {
    if let Some(cols) = attrs.get("columns").and_then(|v| v.parse().ok()) {
        section.page.columns = cols;
    }
    if let Some(orient) = attrs.get("orientation") {
        section.page.orientation = match orient {
            "landscape" => Orientation::Landscape,
            _ => Orientation::Portrait,
        };
    }
    if let Some(name) = attrs.get("page-size") {
        if let Some(size) = paper_size(name) {
            section.page.size = size;
        }
    }
}

fn paper_size(name: &str) -> Option<Size> {
    let (w_mm, h_mm) = match name {
        "A3" => (297.0, 420.0),
        "A4" => (210.0, 297.0),
        "A5" => (148.0, 210.0),
        "Letter" => (215.9, 279.4),
        "Legal" => (215.9, 355.6),
        "Tabloid" => (279.4, 431.8),
        _ => return None,
    };
    Some(Size::new(Length::from_mm(w_mm), Length::from_mm(h_mm)))
}

fn attach_footnotes(sections: &mut [Section], footnotes: &BTreeMap<u32, Vec<Block>>) {
    if footnotes.is_empty() {
        return;
    }
    for section in sections.iter_mut() {
        rewrite_blocks(&mut section.blocks, footnotes);
        for h in &mut section.headers {
            rewrite_blocks(&mut h.blocks, footnotes);
        }
        for f in &mut section.footers {
            rewrite_blocks(&mut f.blocks, footnotes);
        }
    }
}

fn rewrite_blocks(blocks: &mut [Block], footnotes: &BTreeMap<u32, Vec<Block>>) {
    for block in blocks.iter_mut() {
        match block {
            Block::Paragraph(p) => rewrite_inlines(&mut p.content, footnotes),
            Block::Heading(h) => rewrite_inlines(&mut h.paragraph.content, footnotes),
            Block::List(list) => {
                for item in &mut list.items {
                    rewrite_blocks(&mut item.blocks, footnotes);
                }
            }
            Block::Table(t) => {
                for row in &mut t.rows {
                    for cell in &mut row.cells {
                        rewrite_blocks(&mut cell.blocks, footnotes);
                    }
                }
            }
            Block::TextBox(tb) => rewrite_blocks(&mut tb.blocks, footnotes),
            _ => {}
        }
    }
}

fn rewrite_inlines(inlines: &mut [Inline], footnotes: &BTreeMap<u32, Vec<Block>>) {
    for inline in inlines.iter_mut() {
        match inline {
            Inline::Footnote(blocks) => {
                // Placeholder: single empty paragraph tagged with index in plain text?
                // We store index via a temporary Text("\u{0}N") convention during parse.
                if let Some(Block::Paragraph(p)) = blocks.first() {
                    if let Some(Inline::Text(t)) = p.content.first() {
                        if let Some(rest) = t.strip_prefix('\u{1}') {
                            if let Ok(n) = rest.parse::<u32>() {
                                if let Some(real) = footnotes.get(&n) {
                                    *blocks = real.clone();
                                }
                            }
                        }
                    }
                }
            }
            Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                rewrite_inlines(content, footnotes);
            }
            _ => {}
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum ParsedBlock {
    Normal(Block),
    Header(HeaderFooter),
    Footer(HeaderFooter),
    Section { attrs: Attrs, blocks: Vec<Block> },
}

struct BodyParser<'a> {
    base_dir: Option<PathBuf>,
    assets: &'a mut dyn AssetStore,
    report: &'a mut Report,
    footnotes: BTreeMap<u32, Vec<Block>>,
    _styles: docsai_model::StyleCatalog,
}

impl<'a> BodyParser<'a> {
    fn parse_blocks(
        &mut self,
        text: &str,
        start_line: usize,
    ) -> Result<Vec<ParsedBlock>, ParseError> {
        let chunks = split_top_level(text);
        let mut out = Vec::new();
        let mut line = start_line;
        for chunk in chunks {
            let chunk_line = line;
            line += chunk.bytes().filter(|b| *b == b'\n').count() + 2; // rough
            if chunk.trim().is_empty() {
                continue;
            }
            // Footnote definition
            if let Some((n, body)) = parse_footnote_def(chunk) {
                let blocks = self.parse_normal_blocks(body, chunk_line)?;
                self.footnotes.insert(n, blocks);
                continue;
            }
            if let Some(parsed) = self.parse_container_or_block(chunk, chunk_line)? {
                out.push(parsed);
            }
        }
        // Re-parse more carefully with list grouping
        let mut regrouped = Vec::new();
        let list_buf: Vec<(usize, bool, String, usize)> = Vec::new();
        // Actually lists are inside chunks already if blank-line separated.
        // Nested lists are in the same chunk. Good.
        for item in out {
            regrouped.push(item);
        }
        let _ = list_buf;
        Ok(regrouped)
    }

    fn parse_normal_blocks(
        &mut self,
        text: &str,
        start_line: usize,
    ) -> Result<Vec<Block>, ParseError> {
        let parsed = self.parse_blocks(text, start_line)?;
        Ok(parsed
            .into_iter()
            .filter_map(|b| match b {
                ParsedBlock::Normal(b) => Some(b),
                _ => None,
            })
            .collect())
    }

    fn parse_container_or_block(
        &mut self,
        chunk: &str,
        line: usize,
    ) -> Result<Option<ParsedBlock>, ParseError> {
        let trimmed = chunk.trim_start_matches('\n');
        if trimmed.starts_with(":::") {
            return self.parse_fence(trimmed, line).map(Some);
        }
        // Table?
        if looks_like_table(trimmed) {
            let table = self.parse_gfm_table(trimmed, line)?;
            self.report.stats.tables += 1;
            return Ok(Some(ParsedBlock::Normal(Block::Table(table))));
        }
        // List?
        if looks_like_list(trimmed) {
            let list = self.parse_list(trimmed, line, 0)?;
            self.report.stats.lists += 1;
            return Ok(Some(ParsedBlock::Normal(Block::List(list))));
        }
        // Heading
        if let Some(rest) = strip_heading(trimmed) {
            let (level, content) = rest;
            let (para, _) = self.parse_paragraph_line(content, true)?;
            self.report.stats.headings += 1;
            self.report.stats.paragraphs += 1;
            return Ok(Some(ParsedBlock::Normal(Block::Heading(Heading {
                level,
                paragraph: para,
            }))));
        }
        // Image-only block
        if let Some(img) = self.try_parse_block_image(trimmed)? {
            self.report.stats.images += 1;
            return Ok(Some(ParsedBlock::Normal(Block::Image(img))));
        }
        // Paragraph (possibly multi-line with hard breaks)
        let para = self.parse_paragraph_block(trimmed)?;
        self.report.stats.paragraphs += 1;
        Ok(Some(ParsedBlock::Normal(Block::Paragraph(para))))
    }

    fn parse_fence(&mut self, text: &str, line: usize) -> Result<ParsedBlock, ParseError> {
        let mut lines = text.lines();
        let first = lines.next().unwrap_or("");
        let attr_src = first.trim_start_matches(':').trim();
        let attrs = Attrs::parse(attr_src).unwrap_or_default();
        let mut body_lines: Vec<&str> = Vec::new();
        let mut depth = 1i32;
        for l in lines {
            let t = l.trim();
            if t.starts_with(":::") && t.trim_start_matches(':').trim().is_empty() {
                // closing or opening bare :::
                if t == ":::" || t.chars().all(|c| c == ':') {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    body_lines.push(l);
                    continue;
                }
            }
            if t.starts_with(":::") {
                // nested open
                depth += 1;
            }
            body_lines.push(l);
        }
        let body = body_lines.join("\n");

        if attrs.has_class("header") {
            let blocks = self.parse_normal_blocks(&body, line + 1)?;
            let scope = parse_scope(attrs.get("scope"));
            return Ok(ParsedBlock::Header(HeaderFooter { scope, blocks }));
        }
        if attrs.has_class("footer") {
            let blocks = self.parse_normal_blocks(&body, line + 1)?;
            let scope = parse_scope(attrs.get("scope"));
            return Ok(ParsedBlock::Footer(HeaderFooter { scope, blocks }));
        }
        if attrs.has_class("section") {
            let blocks = self.parse_normal_blocks(&body, line + 1)?;
            return Ok(ParsedBlock::Section { attrs, blocks });
        }
        if attrs.has_class("raw") {
            let raw = parse_raw_block(&attrs, &body)?;
            self.report.raw_blocks_emitted += 1;
            return Ok(ParsedBlock::Normal(Block::Raw(raw)));
        }
        if attrs.has_class("textbox") {
            let blocks = self.parse_normal_blocks(&body, line + 1)?;
            let tb = TextBox {
                blocks,
                size: match (attrs.get("width"), attrs.get("height")) {
                    (Some(w), Some(h)) => Some(Size::new(
                        Length::parse(w).unwrap_or(Length::ZERO),
                        Length::parse(h).unwrap_or(Length::ZERO),
                    )),
                    _ => None,
                },
                x: attrs.get("x").and_then(Length::parse),
                y: attrs.get("y").and_then(Length::parse),
            };
            return Ok(ParsedBlock::Normal(Block::TextBox(tb)));
        }
        if attrs.has_class("table") {
            let table = if attrs.get("complex") == Some("true") {
                self.parse_complex_table(&body, line + 1, &attrs)?
            } else {
                let mut table = self.parse_gfm_table(body.trim(), line + 1)?;
                apply_table_attrs(&mut table, &attrs);
                table
            };
            self.report.stats.tables += 1;
            return Ok(ParsedBlock::Normal(Block::Table(table)));
        }

        // Unknown container: parse body as normal blocks and ignore wrapper.
        let blocks = self.parse_normal_blocks(&body, line + 1)?;
        if blocks.len() == 1 {
            Ok(ParsedBlock::Normal(blocks.into_iter().next().unwrap()))
        } else {
            // Flatten
            Err(ParseError::unexpected(
                line,
                format!("unknown container class {:?}", attrs.classes()),
            ))
        }
    }

    fn parse_complex_table(
        &mut self,
        body: &str,
        line: usize,
        attrs: &Attrs,
    ) -> Result<Table, ParseError> {
        let mut table = Table {
            header_row: false,
            ..Default::default()
        };
        apply_table_attrs(&mut table, attrs);
        // Parse nested ::: {.row} / ::: {.cell}
        let mut rest = body.trim();
        while !rest.is_empty() {
            rest = rest.trim_start_matches('\n').trim_start();
            if rest.is_empty() {
                break;
            }
            if !rest.starts_with(":::") {
                break;
            }
            let (inner_attrs, inner_body, next) = split_one_fence(rest, line)?;
            rest = next;
            if !inner_attrs.has_class("row") {
                continue;
            }
            let mut row = TableRow::default();
            let mut cell_src = inner_body.trim();
            while !cell_src.is_empty() {
                cell_src = cell_src.trim_start_matches('\n').trim_start();
                if !cell_src.starts_with(":::") {
                    break;
                }
                let (cell_attrs, cell_body, next_cell) = split_one_fence(cell_src, line)?;
                cell_src = next_cell;
                if !cell_attrs.has_class("cell") {
                    continue;
                }
                let blocks = self.parse_normal_blocks(cell_body, line)?;
                let mut cell = TableCell {
                    blocks,
                    ..Default::default()
                };
                if let Some(c) = cell_attrs.get("colspan").and_then(|v| v.parse().ok()) {
                    cell.colspan = c;
                }
                if let Some(r) = cell_attrs.get("rowspan").and_then(|v| v.parse().ok()) {
                    cell.rowspan = r;
                }
                if let Some(bg) = cell_attrs.get("background") {
                    cell.background = Some(bg.to_string());
                }
                row.cells.push(cell);
            }
            table.rows.push(row);
        }
        Ok(table)
    }

    fn parse_gfm_table(&mut self, text: &str, _line: usize) -> Result<Table, ParseError> {
        let mut rows_raw: Vec<Vec<String>> = Vec::new();
        let mut delimiter_at = None;
        for (idx, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if !line.contains('|') {
                continue;
            }
            let cells = split_table_row(line);
            if cells
                .iter()
                .all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' '))
                && cells.iter().any(|c| c.contains('-'))
            {
                delimiter_at = Some(idx);
                continue;
            }
            rows_raw.push(cells);
        }
        let mut table = Table::default();
        let has_header = delimiter_at.is_some();
        table.header_row = has_header;
        for (i, cells) in rows_raw.into_iter().enumerate() {
            let mut row = TableRow {
                is_header: has_header && i == 0,
                ..Default::default()
            };
            for cell_text in cells {
                row.cells.push(self.parse_table_cell(&cell_text)?);
            }
            table.rows.push(row);
        }
        // Writer inserts an empty header when header_row is false.
        if !has_header {
            // Our rows already are body rows; header_row stays false.
        } else if let Some(first) = table.rows.first() {
            // Drop synthetic empty header if all empty — keep as-is.
            let _ = first;
        }
        // When writer had no header it inserted empty header + delimiter.
        // Parser sees empty header row first. Detect and drop it.
        if has_header {
            if let Some(first) = table.rows.first() {
                let empty = first.cells.iter().all(|c| {
                    c.blocks.is_empty()
                        || matches!(
                            c.blocks.as_slice(),
                            [Block::Paragraph(p)] if p.is_empty()
                        )
                });
                if empty && table.rows.len() > 1 {
                    table.rows.remove(0);
                    table.header_row = false;
                    for r in &mut table.rows {
                        r.is_header = false;
                    }
                }
            }
        }
        Ok(table)
    }

    fn parse_table_cell(&mut self, text: &str) -> Result<TableCell, ParseError> {
        let text = text.trim();
        let (content, attrs) = split_trailing_attrs(text);
        let content = unescape(&content.replace("\\|", "|"));
        let inlines = self.parse_inlines(&content)?;
        let mut cell = TableCell {
            blocks: vec![Block::Paragraph(Paragraph::new(inlines))],
            ..Default::default()
        };
        if let Some(attrs) = attrs {
            if let Some(c) = attrs.get("colspan").and_then(|v| v.parse().ok()) {
                cell.colspan = c;
            }
            if let Some(r) = attrs.get("rowspan").and_then(|v| v.parse().ok()) {
                cell.rowspan = r;
            }
            if let Some(bg) = attrs.get("background") {
                cell.background = Some(bg.to_string());
            }
        }
        Ok(cell)
    }

    fn parse_list(&mut self, text: &str, line: usize, depth: u8) -> Result<List, ParseError> {
        let items = collect_list_items(text);
        if items.is_empty() {
            return Err(ParseError::unexpected(line, "empty list"));
        }
        let ordered = items[0].ordered;
        let mut list = List {
            def: None,
            ordered,
            level: depth,
            items: Vec::new(),
        };
        for item in items {
            let mut blocks = Vec::new();
            // First line is paragraph content (may include attrs)
            let first = item.lines.first().map(String::as_str).unwrap_or("");
            let (para, list_id) = self.parse_paragraph_line(first, false)?;
            if list.def.is_none() {
                if let Some(id) = list_id {
                    list.def = Some(ListId::new(id));
                }
            }
            // Nested content
            let rest = if item.lines.len() > 1 {
                item.lines[1..].join("\n")
            } else {
                String::new()
            };
            let rest = strip_list_indent(&rest, item.marker_len);
            blocks.push(Block::Paragraph(para));
            if !rest.trim().is_empty() {
                // May contain nested list and/or more paragraphs
                let nested = self.parse_normal_blocks(&rest, line)?;
                // If first nested is list, keep tight
                blocks.extend(nested);
            }
            // Remove empty trailing?
            list.items.push(ListItem { blocks });
        }
        Ok(list)
    }

    fn parse_paragraph_block(&mut self, text: &str) -> Result<Paragraph, ParseError> {
        // Hard breaks: line ending with two spaces
        let mut joined = String::new();
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if i + 1 < lines.len() && line.ends_with("  ") {
                joined.push_str(line.trim_end_matches(' '));
                joined.push_str("  \n");
            } else if i + 1 < lines.len() {
                joined.push_str(line);
                joined.push('\n');
            } else {
                joined.push_str(line);
            }
        }
        let (para, _) = self.parse_paragraph_line(&joined, false)?;
        Ok(para)
    }

    fn parse_paragraph_line(
        &mut self,
        text: &str,
        is_heading: bool,
    ) -> Result<(Paragraph, Option<String>), ParseError> {
        let text = text.trim_end();
        // empty marker
        if text == "[]{.empty}" || text.starts_with("[]{.empty}") {
            let rest = text.trim_start_matches("[]{.empty}").trim_start();
            let mut format = ParaFormat::default();
            let mut list_id = None;
            if let Some(attrs) = Attrs::parse(rest) {
                list_id = attrs.get("list").map(str::to_string);
                format = paragraph_format_from_attrs(&attrs, is_heading);
            }
            return Ok((
                Paragraph {
                    format,
                    content: Vec::new(),
                },
                list_id,
            ));
        }

        let (content, attrs) = split_trailing_attrs(text);
        let mut list_id = None;
        let mut format = ParaFormat::default();
        if let Some(attrs) = attrs {
            list_id = attrs.get("list").map(str::to_string);
            format = paragraph_format_from_attrs(&attrs, is_heading);
        }
        // Content may still end with `{.empty}` form already handled.
        if content.trim() == "[]" {
            // shouldn't happen
        }
        let inlines = self.parse_inlines(content.trim_end())?;
        Ok((
            Paragraph {
                format,
                content: inlines,
            },
            list_id,
        ))
    }

    fn try_parse_block_image(&mut self, text: &str) -> Result<Option<ImageRef>, ParseError> {
        let text = text.trim();
        if !text.starts_with("![") {
            return Ok(None);
        }
        // Only a single image and optional attrs
        if text.lines().count() > 1 {
            return Ok(None);
        }
        let inlines = self.parse_inlines(text)?;
        match inlines.as_slice() {
            [Inline::Image(img)] => Ok(Some(img.clone())),
            _ => Ok(None),
        }
    }

    fn parse_inlines(&mut self, text: &str) -> Result<Vec<Inline>, ParseError> {
        parse_inlines_inner(text, self)
    }

    fn load_image(
        &mut self,
        path: &str,
        attrs: Attrs,
        alt: String,
    ) -> Result<ImageRef, ParseError> {
        let mut asset_id = None;
        if let Some(base) = &self.base_dir {
            let full = base.join(path);
            if full.is_file() {
                let bytes =
                    std::fs::read(&full).map_err(|e| ParseError::io(Some(full.clone()), e))?;
                asset_id = Some(self.assets.put(&bytes)?);
            }
        }
        // Also try path as-is relative to cwd when no base
        if asset_id.is_none() {
            let p = Path::new(path);
            if p.is_file() {
                let bytes =
                    std::fs::read(p).map_err(|e| ParseError::io(Some(p.to_path_buf()), e))?;
                asset_id = Some(self.assets.put(&bytes)?);
            }
        }
        // Assets already present in the store (round-trip via MemoryAssetStore).
        if asset_id.is_none() {
            let file_name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path);
            for id in self.assets.ids() {
                if self
                    .assets
                    .info(&id)
                    .is_some_and(|info| info.file_name == file_name)
                {
                    asset_id = Some(id);
                    break;
                }
            }
        }

        let width = attrs
            .get("width")
            .and_then(Length::parse)
            .unwrap_or(Length::ZERO);
        let height = attrs
            .get("height")
            .and_then(Length::parse)
            .unwrap_or(Length::ZERO);
        let mut geometry = ImageGeometry::inline(Size::new(width, height));
        if let Some(ns) = attrs.get("native-size") {
            if let Some((w, h)) = ns.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse(), h.parse()) {
                    geometry.native_size_px = Some((w, h));
                }
            }
        }
        if let Some(dpi) = attrs.get("dpi").and_then(|v| v.parse().ok()) {
            geometry.dpi = Some(dpi);
        }
        geometry.anchor = parse_anchor(&attrs);
        if let Some(rot) = attrs.get("rotation").and_then(|v| v.parse().ok()) {
            geometry.rotation_deg = rot;
        }
        if let Some(flip) = attrs.get("flip") {
            geometry.flip = match flip {
                "h" => Flip::H,
                "v" => Flip::V,
                "hv" => Flip::HV,
                _ => Flip::None,
            };
        }
        if let Some(crop) = attrs.get("crop") {
            let parts: Vec<_> = crop.split(',').collect();
            if parts.len() == 4 {
                let parse_pct =
                    |s: &str| s.trim().trim_end_matches('%').parse::<f32>().unwrap_or(0.0);
                geometry.crop = Some(CropRect {
                    left: parse_pct(parts[0]),
                    top: parse_pct(parts[1]),
                    right: parse_pct(parts[2]),
                    bottom: parse_pct(parts[3]),
                });
            }
        }
        if let Some(border) = attrs.get("border") {
            let parts: Vec<_> = border.split_whitespace().collect();
            if parts.len() >= 3 {
                geometry.border = Some(SimpleBorder {
                    width: Length::parse(parts[0]).unwrap_or(Length::ZERO),
                    style: parts[1].to_string(),
                    color: parts[2].to_string(),
                });
            }
        }
        if let Some(z) = attrs.get("z-index").and_then(|v| v.parse().ok()) {
            geometry.z_index = Some(z);
        }

        let id = asset_id.unwrap_or_else(|| {
            // Placeholder id from path hash when bytes unavailable.
            docsai_model::assets::AssetId::new(format!("missing-{}", path.replace('/', "_")))
        });

        let mut image = ImageRef::new(id, geometry);
        image.alt = alt;
        image.title = attrs.get("title").map(str::to_string);
        image.name = attrs.get("name").map(str::to_string);
        image.link = attrs.get("link").map(str::to_string);
        image.external_src = attrs.get("external-src").map(str::to_string);
        if let Some(raw) = attrs.get("effects-raw") {
            image.effects_raw = Some(RawId::new(raw));
        }
        Ok(image)
    }
}

// ---------------------------------------------------------------------------
// Inline parser
// ---------------------------------------------------------------------------

fn parse_inlines_inner(text: &str, ctx: &mut BodyParser<'_>) -> Result<Vec<Inline>, ParseError> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut text_buf = String::new();

    let flush_text = |buf: &mut String, out: &mut Vec<Inline>| {
        if !buf.is_empty() {
            out.push(Inline::Text(unescape(buf)));
            buf.clear();
        }
    };

    while i < chars.len() {
        let c = chars[i];
        // Hard break: two spaces before newline
        if c == ' ' && i + 2 < chars.len() && chars[i + 1] == ' ' && chars[i + 2] == '\n' {
            flush_text(&mut text_buf, &mut out);
            out.push(Inline::Break(BreakKind::Line));
            i += 3;
            continue;
        }
        if c == '\\' && i + 1 < chars.len() {
            text_buf.push('\\');
            text_buf.push(chars[i + 1]);
            i += 2;
            continue;
        }
        // Footnote ref [^n]
        if c == '[' && i + 2 < chars.len() && chars[i + 1] == '^' {
            if let Some(end) = find_closing_bracket(&chars, i + 2) {
                let inner: String = chars[i + 2..end].iter().collect();
                if inner.chars().all(|ch| ch.is_ascii_digit()) {
                    flush_text(&mut text_buf, &mut out);
                    let n: u32 = inner.parse().unwrap_or(0);
                    // Placeholder footnote; filled later.
                    out.push(Inline::Footnote(vec![Block::Paragraph(Paragraph::new(
                        vec![Inline::Text(format!("\u{1}{n}"))],
                    ))]));
                    ctx.report.stats.footnotes += 1;
                    i = end + 1;
                    continue;
                }
            }
        }
        // Image ![alt](path){attrs}
        if c == '!' && i + 1 < chars.len() && chars[i + 1] == '[' {
            if let Some((img, next)) = try_parse_image(&chars, i, ctx)? {
                flush_text(&mut text_buf, &mut out);
                out.push(Inline::Image(img));
                i = next;
                continue;
            }
        }
        // Link or span [text](url) or [text]{attrs} or []{attrs}
        if c == '[' {
            if let Some((inline, next)) = try_parse_bracket(&chars, i, ctx)? {
                flush_text(&mut text_buf, &mut out);
                out.push(inline);
                i = next;
                continue;
            }
        }
        // Bold **
        if c == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some(end) = find_closing_delim(&chars, i + 2, "**") {
                flush_text(&mut text_buf, &mut out);
                let inner: String = chars[i + 2..end].iter().collect();
                let content = parse_inlines_inner(&inner, ctx)?;
                out.push(Inline::Styled {
                    content,
                    props: RunProps::direct(FontProps {
                        bold: Some(true),
                        ..Default::default()
                    }),
                });
                i = end + 2;
                continue;
            }
        }
        // Strike ~~
        if c == '~' && i + 1 < chars.len() && chars[i + 1] == '~' {
            if let Some(end) = find_closing_delim(&chars, i + 2, "~~") {
                flush_text(&mut text_buf, &mut out);
                let inner: String = chars[i + 2..end].iter().collect();
                let content = parse_inlines_inner(&inner, ctx)?;
                out.push(Inline::Styled {
                    content,
                    props: RunProps::direct(FontProps {
                        strike: Some(true),
                        ..Default::default()
                    }),
                });
                i = end + 2;
                continue;
            }
        }
        // Italic *
        if c == '*' {
            if let Some(end) = find_closing_delim(&chars, i + 1, "*") {
                // avoid matching **
                if end > i + 1 {
                    flush_text(&mut text_buf, &mut out);
                    let inner: String = chars[i + 1..end].iter().collect();
                    let content = parse_inlines_inner(&inner, ctx)?;
                    out.push(Inline::Styled {
                        content,
                        props: RunProps::direct(FontProps {
                            italic: Some(true),
                            ..Default::default()
                        }),
                    });
                    i = end + 1;
                    continue;
                }
            }
        }
        text_buf.push(c);
        i += 1;
    }
    flush_text(&mut text_buf, &mut out);
    Ok(merge_styled(out))
}

fn merge_styled(inlines: Vec<Inline>) -> Vec<Inline> {
    // Flatten nested emphasis that the writer would emit as stacked markers
    // inside a span: already fine as nested Styled.
    inlines
}

fn try_parse_image(
    chars: &[char],
    start: usize,
    ctx: &mut BodyParser<'_>,
) -> Result<Option<(ImageRef, usize)>, ParseError> {
    // ![alt](path){attrs}?
    if start + 1 >= chars.len() || chars[start] != '!' || chars[start + 1] != '[' {
        return Ok(None);
    }
    let alt_end = match find_closing_bracket(chars, start + 2) {
        Some(e) => e,
        None => return Ok(None),
    };
    let alt: String = chars[start + 2..alt_end].iter().collect();
    let alt = unescape(&alt);
    let mut i = alt_end + 1;
    if i >= chars.len() || chars[i] != '(' {
        return Ok(None);
    }
    i += 1;
    let (path, next) = read_link_dest(chars, i)?;
    i = next;
    let mut attrs = Attrs::new();
    if i < chars.len() && chars[i] == '{' {
        if let Some(end) = find_closing_brace(chars, i + 1) {
            let raw: String = chars[i..=end].iter().collect();
            attrs = Attrs::parse(&raw).unwrap_or_default();
            i = end + 1;
        }
    }
    let img = ctx.load_image(&path, attrs, alt)?;
    Ok(Some((img, i)))
}

fn try_parse_bracket(
    chars: &[char],
    start: usize,
    ctx: &mut BodyParser<'_>,
) -> Result<Option<(Inline, usize)>, ParseError> {
    if chars[start] != '[' {
        return Ok(None);
    }
    let end = match find_closing_bracket(chars, start + 1) {
        Some(e) => e,
        None => return Ok(None),
    };
    let inner: String = chars[start + 1..end].iter().collect();
    let mut i = end + 1;
    // Link [text](url){attrs}?
    if i < chars.len() && chars[i] == '(' {
        i += 1;
        let (url, next) = read_link_dest(chars, i)?;
        i = next;
        let content = parse_inlines_inner(&inner, ctx)?;
        let mut props = RunProps::default();
        if i < chars.len() && chars[i] == '{' {
            if let Some(aend) = find_closing_brace(chars, i + 1) {
                let raw: String = chars[i..=aend].iter().collect();
                if let Some(attrs) = Attrs::parse(&raw) {
                    props = run_props_from_attrs(&attrs);
                }
                i = aend + 1;
            }
        }
        return Ok(Some((
            Inline::Link {
                target: url,
                content,
                props,
            },
            i,
        )));
    }
    // Span [text]{attrs} or []{attrs}
    if i < chars.len() && chars[i] == '{' {
        if let Some(aend) = find_closing_brace(chars, i + 1) {
            let raw: String = chars[i..=aend].iter().collect();
            let attrs = Attrs::parse(&raw).unwrap_or_default();
            i = aend + 1;
            // breaks
            if attrs.has_class("break") {
                let kind = match attrs.get("kind").unwrap_or("line") {
                    "page" => BreakKind::Page,
                    "column" => BreakKind::Column,
                    _ => BreakKind::Line,
                };
                return Ok(Some((Inline::Break(kind), i)));
            }
            // empty paragraph marker handled at block level
            if attrs.has_class("empty") && inner.is_empty() {
                return Ok(None);
            }
            // field
            if attrs.has_class("field") {
                let kind = match attrs.get("field").unwrap_or("") {
                    "PAGE" => FieldKind::Page,
                    "NUMPAGES" => FieldKind::NumPages,
                    "DATE" => FieldKind::Date,
                    "TIME" => FieldKind::Time,
                    "TOC" => FieldKind::Toc,
                    "REF" => FieldKind::Ref,
                    other => FieldKind::Other(other.to_string()),
                };
                let instruction = attrs.get("instr").unwrap_or(kind.as_str()).to_string();
                return Ok(Some((
                    Inline::Field {
                        kind,
                        cached: unescape(&inner),
                        instruction,
                    },
                    i,
                )));
            }
            let content = if inner.is_empty() {
                Vec::new()
            } else {
                parse_inlines_inner(&inner, ctx)?
            };
            // Strip emphasis markers that are also represented as attrs? Writer
            // nests markers then wraps span. Content already has Styled nodes.
            let props = run_props_from_attrs(&attrs);
            if props.is_empty() {
                return Ok(Some((
                    Inline::Styled {
                        content,
                        props: RunProps::default(),
                    },
                    i,
                )));
            }
            return Ok(Some((Inline::Styled { content, props }, i)));
        }
    }
    Ok(None)
}

fn read_link_dest(chars: &[char], mut i: usize) -> Result<(String, usize), ParseError> {
    if i < chars.len() && chars[i] == '<' {
        i += 1;
        let start = i;
        while i < chars.len() && chars[i] != '>' {
            i += 1;
        }
        let path: String = chars[start..i].iter().collect();
        if i < chars.len() {
            i += 1; // >
        }
        if i < chars.len() && chars[i] == ')' {
            i += 1;
        }
        return Ok((path.replace("%3E", ">"), i));
    }
    let start = i;
    while i < chars.len() && chars[i] != ')' {
        i += 1;
    }
    let path: String = chars[start..i].iter().collect();
    if i < chars.len() {
        i += 1; // )
    }
    Ok((path, i))
}

fn find_closing_bracket(chars: &[char], mut i: usize) -> Option<usize> {
    let mut depth = 1i32;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '[' => {
                depth += 1;
                i += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

fn find_closing_brace(chars: &[char], mut i: usize) -> Option<usize> {
    let mut depth = 1i32;
    let mut in_str = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                i += 1;
            }
            '{' => {
                depth += 1;
                i += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

fn find_closing_delim(chars: &[char], start: usize, delim: &str) -> Option<usize> {
    let d: Vec<char> = delim.chars().collect();
    let mut i = start;
    while i + d.len() <= chars.len() {
        if chars[i] == '\\' {
            i += 2;
            continue;
        }
        if chars[i..].starts_with(&d) {
            // For single *, don't match the first * of **
            if delim == "*" && i + 1 < chars.len() && chars[i + 1] == '*' {
                i += 1;
                continue;
            }
            return Some(i);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// Attr → IR helpers
// ---------------------------------------------------------------------------

fn paragraph_format_from_attrs(attrs: &Attrs, is_heading: bool) -> ParaFormat {
    let mut format = ParaFormat::default();
    // Style = first class that is not a known structural class
    for class in attrs.classes() {
        if matches!(
            class.as_str(),
            "empty"
                | "table"
                | "row"
                | "cell"
                | "header"
                | "footer"
                | "section"
                | "raw"
                | "textbox"
        ) {
            continue;
        }
        format.style = Some(StyleId::new(class.clone()));
        break;
    }
    let mut direct = docsai_model::style::ParaProps::default();
    if let Some(a) = attrs.get("align") {
        direct.align = Some(match a {
            "center" => Align::Center,
            "right" => Align::Right,
            "justify" => Align::Justify,
            _ => Align::Left,
        });
    }
    direct.indent_left = attrs.get("indent-left").and_then(Length::parse);
    direct.indent_right = attrs.get("indent-right").and_then(Length::parse);
    direct.indent_first_line = attrs.get("indent-first-line").and_then(Length::parse);
    direct.indent_hanging = attrs.get("indent-hanging").and_then(Length::parse);
    direct.space_before = attrs.get("space-before").and_then(Length::parse);
    direct.space_after = attrs.get("space-after").and_then(Length::parse);
    if let Some(bg) = attrs.get("background") {
        direct.background = Some(bg.to_string());
    }
    if attrs.get("keep-with-next") == Some("true") {
        direct.keep_with_next = Some(true);
    }
    if attrs.get("page-break-before") == Some("true") {
        direct.page_break_before = Some(true);
    }
    if !is_heading {
        if let Some(level) = attrs.get("outline-level").and_then(|v| v.parse().ok()) {
            direct.outline_level = Some(level);
        }
    }
    format.direct = direct;
    format
}

fn run_props_from_attrs(attrs: &Attrs) -> RunProps {
    let mut props = RunProps::default();
    let mut font = FontProps::default();
    for class in attrs.classes() {
        match class.as_str() {
            "underline" => {
                font.underline = Some(match attrs.get("underline") {
                    Some("double") => Underline::Double,
                    Some("thick") => Underline::Thick,
                    Some("dotted") => Underline::Dotted,
                    Some("dashed") => Underline::Dashed,
                    Some("wave") => Underline::Wave,
                    Some("none") => Underline::None,
                    _ => Underline::Single,
                });
            }
            "sup" => font.vert_align = Some(VertAlign::Superscript),
            "sub" => font.vert_align = Some(VertAlign::Subscript),
            "small-caps" => font.small_caps = Some(true),
            "caps" => font.caps = Some(true),
            "field" | "break" | "empty" => {}
            other => {
                if props.style.is_none() {
                    props.style = Some(StyleId::new(other));
                }
            }
        }
    }
    if attrs.get("bold") == Some("false") {
        font.bold = Some(false);
    }
    if attrs.get("italic") == Some("false") {
        font.italic = Some(false);
    }
    if let Some(c) = attrs.get("color") {
        font.color = Some(c.to_string());
    }
    if let Some(h) = attrs.get("highlight") {
        font.highlight = Some(h.to_string());
    }
    if let Some(f) = attrs.get("font") {
        font.name = Some(f.to_string());
    }
    if let Some(s) = attrs.get("size").and_then(Length::parse) {
        font.size = Some(s);
    }
    props.direct = font;
    props
}

fn parse_anchor(attrs: &Attrs) -> Anchor {
    match attrs.get("anchor").unwrap_or("inline") {
        "floating" | "behind" => {
            let behind = attrs.get("anchor") == Some("behind");
            let rel_h = parse_rel(attrs.get("relative-to").unwrap_or("margin"));
            let rel_v = attrs.get("relative-to-v").map(parse_rel).unwrap_or(rel_h);
            let h = if let Some(a) = attrs.get("align-h") {
                AxisPos::Align(parse_align_kw(a))
            } else {
                AxisPos::Offset(
                    attrs
                        .get("x")
                        .and_then(Length::parse)
                        .unwrap_or(Length::ZERO),
                )
            };
            let v = if let Some(a) = attrs.get("align-v") {
                AxisPos::Align(parse_align_kw(a))
            } else {
                AxisPos::Offset(
                    attrs
                        .get("y")
                        .and_then(Length::parse)
                        .unwrap_or(Length::ZERO),
                )
            };
            let wrap = match attrs.get("wrap").unwrap_or("square") {
                "tight" => WrapMode::Tight,
                "through" => WrapMode::Through,
                "top-bottom" => WrapMode::TopBottom,
                "none" => WrapMode::None,
                _ => WrapMode::Square,
            };
            let wrap_side = match attrs.get("wrap-side").unwrap_or("both") {
                "left" => WrapSide::Left,
                "right" => WrapSide::Right,
                "largest" => WrapSide::Largest,
                _ => WrapSide::Both,
            };
            Anchor::Floating {
                relative_to_h: rel_h,
                relative_to_v: rel_v,
                position: HVPos { h, v },
                wrap,
                wrap_side,
                behind_text: behind,
            }
        }
        "absolute" => Anchor::SheetAbsolute {
            pos: docsai_model::units::Point::new(
                attrs
                    .get("x")
                    .and_then(Length::parse)
                    .unwrap_or(Length::ZERO),
                attrs
                    .get("y")
                    .and_then(Length::parse)
                    .unwrap_or(Length::ZERO),
            ),
        },
        "two-cell" => {
            let from = parse_cell_anchor_attr(attrs.get("from"), attrs.get("from-offset"));
            let to = parse_cell_anchor_attr(attrs.get("to"), attrs.get("to-offset"));
            Anchor::SheetTwoCell {
                from,
                to,
                move_with_cells: attrs.get("move-with-cells") != Some("false"),
                size_with_cells: attrs.get("size-with-cells") == Some("true"),
            }
        }
        "one-cell" => Anchor::SheetOneCell {
            from: parse_cell_anchor_attr(attrs.get("from"), attrs.get("from-offset")),
        },
        _ => Anchor::Inline,
    }
}

fn parse_cell_anchor_attr(
    cell: Option<&str>,
    offset: Option<&str>,
) -> docsai_model::image::CellAnchor {
    use docsai_model::image::CellAnchor;
    use docsai_model::sheet::CellRef;
    let cell = cell
        .and_then(CellRef::parse_a1)
        .unwrap_or(CellRef::new(0, 0));
    let (ox, oy) = match offset {
        Some(o) => {
            let mut parts = o.split(',');
            let x = parts.next().and_then(Length::parse).unwrap_or(Length::ZERO);
            let y = parts.next().and_then(Length::parse).unwrap_or(Length::ZERO);
            (x, y)
        }
        None => (Length::ZERO, Length::ZERO),
    };
    CellAnchor::new(cell, ox, oy)
}

fn parse_rel(v: &str) -> RelBase {
    match v {
        "page" => RelBase::Page,
        "paragraph" => RelBase::Paragraph,
        "character" => RelBase::Character,
        "line" => RelBase::Line,
        "column" => RelBase::Column,
        _ => RelBase::Margin,
    }
}

fn parse_align_kw(v: &str) -> AlignKeyword {
    match v {
        "center" => AlignKeyword::Center,
        "right" => AlignKeyword::Right,
        "inside" => AlignKeyword::Inside,
        "outside" => AlignKeyword::Outside,
        "top" => AlignKeyword::Top,
        "middle" => AlignKeyword::Middle,
        "bottom" => AlignKeyword::Bottom,
        _ => AlignKeyword::Left,
    }
}

fn parse_scope(v: Option<&str>) -> HeaderScope {
    match v.unwrap_or("default") {
        "first" => HeaderScope::First,
        "even" => HeaderScope::Even,
        _ => HeaderScope::Default,
    }
}

fn apply_table_attrs(table: &mut Table, attrs: &Attrs) {
    if let Some(style) = attrs.get("style") {
        table.style = Some(StyleId::new(style));
    }
    if let Some(widths) = attrs.get("col-widths") {
        table.col_widths = widths
            .split(',')
            .filter_map(|w| Length::parse(w.trim()))
            .collect();
    }
    if attrs.get("header-row") == Some("false") {
        table.header_row = false;
    }
}

fn parse_raw_block(attrs: &Attrs, body: &str) -> Result<RawFragment, ParseError> {
    let format = attrs.get("format").unwrap_or("ooxml").to_string();
    let part = attrs.get("part").unwrap_or("").to_string();
    let id = attrs
        .id_ref()
        .map(RawId::new)
        .unwrap_or_else(|| RawId::new("raw"));
    // Body is a fenced code block
    let mut lines = body.lines();
    let first = lines.next().unwrap_or("");
    let fence = first.chars().take_while(|c| *c == '`').count();
    let content = if fence >= 3 {
        let mut content_lines: Vec<&str> = Vec::new();
        for l in lines {
            if l.chars().take_while(|c| *c == '`').count() >= fence
                && l.trim().chars().all(|c| c == '`')
            {
                break;
            }
            content_lines.push(l);
        }
        content_lines.join("\n")
    } else {
        body.trim().to_string()
    };
    Ok(RawFragment {
        id,
        format,
        part,
        content,
    })
}

// ---------------------------------------------------------------------------
// Chunk / list / table utilities
// ---------------------------------------------------------------------------

fn split_top_level(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0usize;
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut fence_depth = 0i32;
    while i < bytes.len() {
        if bytes[i] == b'\n' {
            // Check blank line
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] == b'\r' {
                j += 1;
            }
            if j < bytes.len() && bytes[j] == b'\n' {
                // blank line at j
                if fence_depth == 0 {
                    let chunk = &text[start..i];
                    if !chunk.trim().is_empty() {
                        chunks.push(chunk);
                    }
                    start = j + 1;
                    i = j + 1;
                    continue;
                }
            }
            // track fences on this line
            let line_start = text[..i].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let line = &text[line_start..i];
            let t = line.trim();
            if t.starts_with(":::") {
                let rest = t.trim_start_matches(':').trim();
                if rest.is_empty() {
                    fence_depth = (fence_depth - 1).max(0);
                } else {
                    fence_depth += 1;
                }
            }
        }
        i += 1;
    }
    if start < text.len() {
        let chunk = &text[start..];
        if !chunk.trim().is_empty() {
            chunks.push(chunk);
        }
    }
    chunks
}

fn split_one_fence(text: &str, line: usize) -> Result<(Attrs, &str, &str), ParseError> {
    let text = text.trim_start_matches('\n');
    let mut lines = text.lines();
    let first = lines
        .next()
        .ok_or_else(|| ParseError::unexpected(line, "expected fenced container"))?;
    if !first.trim_start().starts_with(":::") {
        return Err(ParseError::unexpected(line, "expected :::"));
    }
    let attr_src = first.trim_start().trim_start_matches(':').trim();
    let attrs = Attrs::parse(attr_src).unwrap_or_default();
    let after_first = first.len() + 1; // + newline; careful with end
    let body_start = if text.len() > first.len() {
        // account for \n
        first.len() + 1
    } else {
        first.len()
    };
    let mut depth = 1i32;
    let mut consumed = body_start;
    let mut body_end = body_start;
    for l in text[body_start..].lines() {
        let t = l.trim();
        let line_len = l.len() + 1; // include \n
        if t.starts_with(":::") {
            let rest = t.trim_start_matches(':').trim();
            if rest.is_empty() {
                depth -= 1;
                if depth == 0 {
                    let body = &text[body_start..consumed];
                    let _next = if consumed + l.len() < text.len() {
                        text[consumed + l.len()..].trim_start_matches('\n')
                    } else {
                        ""
                    };
                    // fix next offset
                    let next_start = consumed + l.len();
                    let next = if next_start < text.len() {
                        let n = &text[next_start..];
                        n.strip_prefix('\n').unwrap_or(n)
                    } else {
                        ""
                    };
                    let _ = after_first;
                    let _ = body_end;
                    return Ok((attrs, body.trim_end_matches('\n'), next));
                }
            } else {
                depth += 1;
            }
        }
        consumed += line_len;
        body_end = consumed;
    }
    Err(ParseError::unexpected(line, "unclosed fenced container"))
}

fn looks_like_table(text: &str) -> bool {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    if !first.contains('|') {
        return false;
    }
    let Some(second) = lines.next() else {
        return false;
    };
    second.contains('|') && second.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn looks_like_list(text: &str) -> bool {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let t = first.trim_start();
    t.starts_with("- ")
        || t.starts_with("* ")
        || t.chars().next().is_some_and(|c| c.is_ascii_digit())
            && t.split_whitespace().next().is_some_and(|w| {
                w.ends_with('.') && w[..w.len() - 1].chars().all(|c| c.is_ascii_digit())
            })
}

struct ListItemRaw {
    ordered: bool,
    marker_len: usize,
    lines: Vec<String>,
}

fn collect_list_items(text: &str) -> Vec<ListItemRaw> {
    let mut items = Vec::new();
    let mut current: Option<ListItemRaw> = None;
    for line in text.lines() {
        let indent = line.chars().take_while(|c| *c == ' ').count();
        let trimmed = &line[indent..];
        if let Some((ordered, marker_len, content)) = match_list_marker(trimmed) {
            if indent == 0 {
                if let Some(item) = current.take() {
                    items.push(item);
                }
                current = Some(ListItemRaw {
                    ordered,
                    marker_len,
                    lines: vec![content.to_string()],
                });
                continue;
            }
        }
        if let Some(item) = current.as_mut() {
            item.lines.push(line.to_string());
        }
    }
    if let Some(item) = current {
        items.push(item);
    }
    items
}

fn match_list_marker(trimmed: &str) -> Option<(bool, usize, &str)> {
    if let Some(rest) = trimmed.strip_prefix("- ") {
        return Some((false, 2, rest));
    }
    if let Some(rest) = trimmed.strip_prefix("* ") {
        return Some((false, 2, rest));
    }
    let mut i = 0usize;
    while i < trimmed.len() && trimmed.as_bytes()[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && trimmed[i..].starts_with(". ") {
        return Some((true, i + 2, &trimmed[i + 2..]));
    }
    None
}

fn strip_list_indent(text: &str, marker_len: usize) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let stripped =
            if line.len() >= marker_len && line.chars().take(marker_len).all(|c| c == ' ') {
                &line[marker_len..]
            } else {
                line.trim_start()
            };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(stripped);
    }
    out
}

fn strip_heading(text: &str) -> Option<(u8, &str)> {
    let mut n = 0usize;
    let bytes = text.as_bytes();
    while n < bytes.len() && bytes[n] == b'#' && n < 6 {
        n += 1;
    }
    if n == 0 || n > 6 {
        return None;
    }
    if n < bytes.len() && bytes[n] == b' ' {
        Some((n as u8, text[n + 1..].trim_end()))
    } else {
        None
    }
}

fn split_trailing_attrs(text: &str) -> (&str, Option<Attrs>) {
    let text = text.trim_end();
    // Paragraph-level attrs are always emitted with a leading space (`Attrs::suffix`).
    // Span/link/image attrs attach tightly: `[text]{.underline}`, `![](p){width=1}`.
    // Only peel a trailing `{...}` when whitespace precedes it (or it is the whole line).
    let bytes = text.as_bytes();
    if !bytes.last().is_some_and(|b| *b == b'}') {
        return (text, None);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut i = chars.len() - 1;
    let mut depth = 0i32;
    let mut in_str = false;
    loop {
        let c = chars[i];
        if in_str {
            if c == '"' && i > 0 && chars[i - 1] != '\\' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '}' => depth += 1,
                '{' => {
                    depth -= 1;
                    if depth == 0 {
                        // Require whitespace before `{`, or attrs-only line.
                        if i > 0 && !chars[i - 1].is_whitespace() {
                            return (text, None);
                        }
                        let attr_str: String = chars[i..].iter().collect();
                        if let Some(attrs) = Attrs::parse(&attr_str) {
                            let mut end = i;
                            while end > 0 && chars[end - 1].is_whitespace() {
                                end -= 1;
                            }
                            let byte_end = text
                                .char_indices()
                                .nth(end)
                                .map(|(b, _)| b)
                                .unwrap_or(text.len());
                            return (&text[..byte_end], Some(attrs));
                        }
                        return (text, None);
                    }
                }
                _ => {}
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    (text, None)
}

fn split_table_row(line: &str) -> Vec<String> {
    let line = line.trim().trim_matches('|');
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(n) = chars.next() {
                cur.push('\\');
                cur.push(n);
            } else {
                cur.push('\\');
            }
            continue;
        }
        if c == '|' {
            cells.push(cur.trim().to_string());
            cur.clear();
            continue;
        }
        cur.push(c);
    }
    cells.push(cur.trim().to_string());
    cells
}

fn parse_footnote_def(chunk: &str) -> Option<(u32, &str)> {
    let t = chunk.trim_start();
    let rest = t.strip_prefix("[^")?;
    let end = rest.find("]:")?;
    let n: u32 = rest[..end].parse().ok()?;
    let body = rest[end + 2..].trim_start();
    Some((n, body))
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    #[test]
    fn parses_basic_styles_golden() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/docx/basic-styles.expected.dmk.md"
        );
        let md = std::fs::read_to_string(path).unwrap();
        let mut assets = MemoryAssetStore::new();
        let (doc, _report) = parse(&md, &mut assets).expect("parse");
        let Document::Text(text) = doc else {
            panic!("expected text");
        };
        assert!(text.blocks().any(|b| matches!(b, Block::Heading(_))));
        let plain: String = text
            .blocks()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.plain_text()),
                Block::Heading(h) => Some(h.paragraph.plain_text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(plain.contains("negrita"));
        assert!(plain.contains("Titulo de nivel 1"));
    }

    #[test]
    fn round_trip_basic_text() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/docx/basic-text.expected.dmk.md"
        );
        let md = std::fs::read_to_string(path).unwrap();
        let mut assets = MemoryAssetStore::new();
        let (doc, _) = parse(&md, &mut assets).expect("parse");
        let (out, _) = crate::serialize(
            &doc,
            &assets,
            &crate::Options {
                fidelity: crate::Fidelity::Full,
                assets_dir: "assets".into(),
                source_format: docsai_model::Format::Docx,
            },
        );
        assert_eq!(out, md, "serialize(parse(md)) must equal md");
    }

    fn round_trip_golden(name: &str) {
        let path = format!(
            "{}/../../corpus/docx/{name}.expected.dmk.md",
            env!("CARGO_MANIFEST_DIR")
        );
        let md = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let mut assets = MemoryAssetStore::new();
        let base = std::path::Path::new(&path).parent();
        let (doc, _) =
            parse_with_base(&md, base, &mut assets).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (out, _) = crate::serialize(
            &doc,
            &assets,
            &crate::Options {
                fidelity: crate::Fidelity::Full,
                assets_dir: "assets".into(),
                source_format: docsai_model::Format::Docx,
            },
        );
        assert_eq!(out, md, "{name}: serialize(parse(md)) must equal md");
    }

    #[test]
    fn round_trip_basic_styles() {
        round_trip_golden("basic-styles");
    }

    #[test]
    fn round_trip_nested_lists() {
        round_trip_golden("nested-lists");
    }

    #[test]
    fn parses_nested_lists_golden() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../corpus/docx/nested-lists.expected.dmk.md"
        );
        let md = std::fs::read_to_string(path).unwrap();
        let mut assets = MemoryAssetStore::new();
        let (doc, _) = parse(&md, &mut assets).expect("parse nested-lists");
        let Document::Text(text) = doc else {
            panic!("expected text");
        };
        assert!(
            text.blocks().any(|b| matches!(b, Block::List(_))),
            "expected at least one list"
        );
    }
}
