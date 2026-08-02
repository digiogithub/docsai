//! DocMark block syntax → IR (spec §3.1, §3.3, §3.4, §3.6, §7).
//!
//! Line-oriented and recursive: a container (`::: {…}`) is located by matching
//! its closing fence, its body is handed back to the same routine, and the
//! class on the opener decides what the result becomes.

use std::collections::BTreeMap;

use docsai_model::assets::AssetId;
use docsai_model::image::RawId;
use docsai_model::list::ListId;
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::StyleId;
use docsai_model::text::{
    Block, HeaderFooter, HeaderScope, Heading, Inline, List, ListItem, PageGeometry, ParaFormat,
    Paragraph, RawFragment, Section, Table, TableCell, TableRow, TextBox,
};
use docsai_model::units::Size;

use super::front::{parse_align, FrontMatter};
use super::inline::Inliner;
use crate::attrs::{split_trailing, Attrs};
use crate::escape::unescape;
use crate::units::parse_len;

/// Turns the body of a DocMark file into sections.
pub struct BlockParser<'a> {
    pub assets: &'a BTreeMap<String, AssetId>,
    pub footnotes: BTreeMap<String, Vec<Block>>,
    pub report: &'a mut ConversionReport,
    /// The page geometry the front matter declared, inherited by every section.
    pub page: PageGeometry,
    /// How deep inside list items the walk currently is. Markdown indentation
    /// is the only place a list's level is written, so it is read from there.
    depth: u8,
}

impl<'a> BlockParser<'a> {
    pub fn new(
        assets: &'a BTreeMap<String, AssetId>,
        report: &'a mut ConversionReport,
        front: &FrontMatter,
    ) -> Self {
        BlockParser {
            assets,
            footnotes: BTreeMap::new(),
            report,
            page: front.page,
            depth: 0,
        }
    }

    fn inliner(&mut self) -> Inliner<'_> {
        Inliner {
            assets: self.assets,
            footnotes: &self.footnotes,
            report: self.report,
        }
    }

    /// Collects the `[^name]: …` definitions so that references can resolve.
    ///
    /// Two passes are needed because a reference always precedes its
    /// definition, and the IR stores a footnote's blocks at the reference.
    pub fn collect_footnotes(&mut self, lines: &[&str]) {
        let mut index = 0usize;
        while index < lines.len() {
            let Some((name, first)) = footnote_definition(lines[index]) else {
                index += 1;
                continue;
            };
            let mut body: Vec<String> = vec![first.to_string()];
            index += 1;
            // Continuation lines are indented by four spaces (writer) or blank.
            while let Some(line) = lines.get(index) {
                if line.trim().is_empty() {
                    // A blank line only continues the note if indented content
                    // follows it.
                    match lines.get(index + 1) {
                        Some(next) if next.starts_with("    ") => {
                            body.push(String::new());
                            index += 1;
                        }
                        _ => break,
                    }
                } else if let Some(rest) = line.strip_prefix("    ") {
                    body.push(rest.to_string());
                    index += 1;
                } else {
                    break;
                }
            }
            let refs: Vec<&str> = body.iter().map(String::as_str).collect();
            let blocks = self.blocks(&refs);
            self.footnotes.insert(name.to_string(), blocks);
        }
    }

    /// Splits the top level into sections, intercepting headers and footers.
    pub fn document(&mut self, lines: &[&str]) -> Vec<Section> {
        let mut sections: Vec<Section> = Vec::new();
        let mut headers: Vec<HeaderFooter> = Vec::new();
        let mut footers: Vec<HeaderFooter> = Vec::new();
        let mut loose: Vec<Block> = Vec::new();
        let mut segment_start = 0usize;
        let mut index = 0usize;

        while index < lines.len() {
            let Some(attrs) = container_open(lines[index]) else {
                index += 1;
                continue;
            };
            let is_structural = attrs.has_class("header")
                || attrs.has_class("footer")
                || attrs.has_class("section");
            let end = container_end(lines, index);
            if !is_structural {
                index = end + 1;
                continue;
            }

            loose.extend(self.blocks(&lines[segment_start..index]));
            let body = &lines[index + 1..end];
            if attrs.has_class("section") {
                let blocks: Vec<Block> = loose.drain(..).chain(self.blocks(body)).collect();
                sections.push(Section {
                    page: self.section_page(&attrs),
                    headers: std::mem::take(&mut headers),
                    footers: std::mem::take(&mut footers),
                    blocks,
                });
            } else {
                let part = HeaderFooter {
                    scope: match attrs.get("scope") {
                        Some("first") => HeaderScope::First,
                        Some("even") => HeaderScope::Even,
                        _ => HeaderScope::Default,
                    },
                    blocks: self.blocks(body),
                };
                if attrs.has_class("header") {
                    headers.push(part);
                } else {
                    footers.push(part);
                }
            }
            index = end + 1;
            segment_start = index;
        }

        loose.extend(self.blocks(&lines[segment_start..]));
        if !loose.is_empty() || sections.is_empty() || !headers.is_empty() || !footers.is_empty() {
            sections.push(Section {
                page: self.page,
                headers,
                footers,
                blocks: loose,
            });
        }
        sections
    }

    /// A section container carries only what spec §3.6 defines; the rest of the
    /// geometry is inherited from the front matter.
    fn section_page(&self, attrs: &Attrs) -> PageGeometry {
        let mut page = self.page;
        if let Some(columns) = attrs.get("columns").and_then(|c| c.parse().ok()) {
            page.columns = columns;
        }
        if attrs.get("orientation") == Some("landscape") {
            page.orientation = docsai_model::text::Orientation::Landscape;
            if page.size.width < page.size.height {
                page.size = Size::new(page.size.height, page.size.width);
            }
        }
        page
    }

    /// Parses a run of lines into blocks.
    pub fn blocks(&mut self, lines: &[&str]) -> Vec<Block> {
        let mut out = Vec::new();
        let mut index = 0usize;
        while index < lines.len() {
            let line = lines[index];
            if line.trim().is_empty() {
                index += 1;
                continue;
            }
            if container_open(line).is_some() {
                let end = container_end(lines, index);
                out.extend(self.container(line, &lines[index + 1..end]));
                index = end + 1;
            } else if let Some((level, rest)) = heading(line) {
                out.push(Block::Heading(Heading {
                    level,
                    paragraph: self.paragraph(rest, None),
                }));
                index += 1;
            } else if table_starts(lines, index) {
                let end = run_end(lines, index, |l| l.trim_start().starts_with('|'));
                if let Some(table) = self.gfm_table(&lines[index..end], &Attrs::new()) {
                    out.push(Block::Table(table));
                }
                index = end;
            } else if list_marker(line).is_some() {
                let (list, next) = self.list(lines, index);
                out.push(Block::List(list));
                index = next;
            } else if footnote_definition(line).is_some() {
                // Already collected in the first pass.
                index += 1;
                while lines
                    .get(index)
                    .is_some_and(|l| l.starts_with("    ") || l.trim().is_empty())
                {
                    index += 1;
                }
            } else {
                let mut end = index + 1;
                while end < lines.len()
                    && !lines[end].trim().is_empty()
                    && container_open(lines[end]).is_none()
                    && heading(lines[end]).is_none()
                    && !table_starts(lines, end)
                    && list_marker(lines[end]).is_none()
                    && footnote_definition(lines[end]).is_none()
                {
                    end += 1;
                }
                out.extend(self.paragraph_block(&lines[index..end]));
                index = end;
            }
        }
        out
    }

    /// A paragraph made of one or more lines.
    fn paragraph_block(&mut self, lines: &[&str]) -> Vec<Block> {
        let text = lines.join("\n");
        let paragraph = self.paragraph(&text, None);
        vec![as_block(paragraph)]
    }

    /// Builds a paragraph from its text, honouring a trailing attribute block.
    ///
    /// `list_out` receives the `list=` pair when the caller is a list item, so
    /// that it never lands in the paragraph's own formatting.
    fn paragraph(&mut self, text: &str, mut list_out: Option<&mut Option<ListId>>) -> Paragraph {
        let (body, attrs) = split_trailing(text);
        let mut format = ParaFormat::default();
        if let Some(mut attrs) = attrs {
            if let Some(out) = list_out.as_mut() {
                **out = attrs.take("list").map(ListId::new);
            }
            format = self.para_format(attrs);
        }
        let content = self.inliner().parse(body);
        Paragraph { format, content }
    }

    /// Undoes `writer::paragraph_attrs`.
    fn para_format(&mut self, mut attrs: Attrs) -> ParaFormat {
        let mut format = ParaFormat::default();
        let direct = &mut format.direct;
        direct.align = attrs.take("align").as_deref().and_then(parse_align);
        direct.indent_left = attrs.take("indent-left").as_deref().and_then(parse_len);
        direct.indent_right = attrs.take("indent-right").as_deref().and_then(parse_len);
        direct.indent_first_line = attrs
            .take("indent-first-line")
            .as_deref()
            .and_then(parse_len);
        direct.indent_hanging = attrs.take("indent-hanging").as_deref().and_then(parse_len);
        direct.space_before = attrs.take("space-before").as_deref().and_then(parse_len);
        direct.space_after = attrs.take("space-after").as_deref().and_then(parse_len);
        direct.background = attrs.take("background");
        if attrs.flag("keep-with-next") == Some(true) {
            direct.keep_with_next = Some(true);
        }
        if attrs.flag("page-break-before") == Some(true) {
            direct.page_break_before = Some(true);
        }
        direct.outline_level = attrs.take("outline-level").and_then(|v| v.parse().ok());
        format.style = attrs.classes().first().cloned().map(StyleId::new);
        format
    }

    // ----------------------------------------------------------------------
    // Containers
    // ----------------------------------------------------------------------

    fn container(&mut self, opener: &str, body: &[&str]) -> Vec<Block> {
        let mut attrs = container_open(opener).unwrap_or_default();
        if attrs.take_class("raw") {
            return vec![self.raw_block(attrs, body)];
        }
        if attrs.take_class("table") {
            return match self.table_container(&attrs, body) {
                Some(table) => vec![Block::Table(table)],
                None => Vec::new(),
            };
        }
        if attrs.take_class("textbox") {
            return vec![Block::TextBox(TextBox {
                blocks: self.blocks(body),
                size: match (
                    attrs.get("width").and_then(parse_len),
                    attrs.get("height").and_then(parse_len),
                ) {
                    (Some(width), Some(height)) => Some(Size::new(width, height)),
                    _ => None,
                },
                x: attrs.get("x").and_then(parse_len),
                y: attrs.get("y").and_then(parse_len),
            })];
        }
        // A header, footer or section met somewhere it cannot be honoured, or
        // a class from a future version: keep the content, say what happened.
        let classes = attrs.classes().join(".");
        if !classes.is_empty() {
            self.report.warn(Warning::Degraded {
                what: format!("container .{classes}"),
                why: "not a container DocMark 1.0 defines here; its content was kept".into(),
            });
        }
        self.blocks(body)
    }

    fn raw_block(&mut self, attrs: Attrs, body: &[&str]) -> Block {
        // The body is a fenced code block; everything between the fences is the
        // original markup, byte for byte.
        let mut content: Vec<&str> = Vec::new();
        let mut inside = false;
        for line in body {
            if line.trim_start().starts_with("```") {
                if inside {
                    break;
                }
                inside = true;
                continue;
            }
            if inside {
                content.push(line);
            }
        }
        Block::Raw(RawFragment {
            id: RawId::new(attrs.get_id().unwrap_or_default()),
            format: attrs.get("format").unwrap_or("ooxml").to_string(),
            part: attrs.get("part").unwrap_or_default().to_string(),
            content: content.join("\n"),
        })
    }

    fn table_container(&mut self, attrs: &Attrs, body: &[&str]) -> Option<Table> {
        if attrs.flag("complex") == Some(true) {
            return Some(self.complex_table(attrs, body));
        }
        let rows: Vec<&str> = body
            .iter()
            .copied()
            .filter(|l| l.trim_start().starts_with('|'))
            .collect();
        self.gfm_table(&rows, attrs)
    }

    /// A GFM table plus the metadata its container carries.
    fn gfm_table(&mut self, lines: &[&str], attrs: &Attrs) -> Option<Table> {
        let mut rows: Vec<TableRow> = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            // Row 1 is the delimiter; it carries no content.
            if index == 1 && is_delimiter_row(line) {
                continue;
            }
            rows.push(TableRow {
                cells: self.table_cells(line),
                is_header: false,
            });
        }
        if rows.is_empty() {
            return None;
        }

        let header_row = attrs.flag("header-row") != Some(false);
        if !header_row {
            // The writer inserted an empty row because GFM demands one.
            rows.remove(0);
        } else if let Some(first) = rows.first_mut() {
            first.is_header = true;
        }

        let mut table = Table {
            style: attrs.get("style").map(StyleId::new),
            col_widths: attrs
                .get("col-widths")
                .map(|w| w.split(',').filter_map(parse_len).collect())
                .unwrap_or_default(),
            rows,
            header_row,
        };
        mark_covered_cells(&mut table);
        Some(table)
    }

    /// Splits one `| a | b |` row, dropping the fillers a colspan left behind.
    fn table_cells(&mut self, line: &str) -> Vec<TableCell> {
        let mut cells: Vec<TableCell> = Vec::new();
        for raw in split_row(line) {
            let (text, attrs) = split_trailing(&raw);
            let mut cell = TableCell {
                blocks: Vec::new(),
                ..Default::default()
            };
            if let Some(attrs) = attrs {
                cell.colspan = attrs
                    .get("colspan")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                cell.rowspan = attrs
                    .get("rowspan")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                cell.background = attrs.get("background").map(str::to_string);
            }
            let text = text.trim();
            if !text.is_empty() {
                let content = self.inliner().parse(text);
                cell.blocks = vec![Block::Paragraph(Paragraph::new(content))];
            }
            cells.push(cell);
        }

        // A cell spanning N columns was written followed by N-1 empty ones so
        // that the GFM grid stayed rectangular; they are not cells.
        let mut out: Vec<TableCell> = Vec::new();
        let mut skip = 0usize;
        for cell in cells {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            skip = cell.colspan.max(1) as usize - 1;
            out.push(cell);
        }
        out
    }

    /// `::: {.table complex=true}` with `.row` and `.cell` sub-containers.
    fn complex_table(&mut self, attrs: &Attrs, body: &[&str]) -> Table {
        let mut rows: Vec<TableRow> = Vec::new();
        let mut index = 0usize;
        while index < body.len() {
            let Some(row_attrs) = container_open(body[index]) else {
                index += 1;
                continue;
            };
            let row_end = container_end(body, index);
            if !row_attrs.has_class("row") {
                index = row_end + 1;
                continue;
            }
            let inner = &body[index + 1..row_end];
            let mut cells: Vec<TableCell> = Vec::new();
            let mut cursor = 0usize;
            while cursor < inner.len() {
                let Some(cell_attrs) = container_open(inner[cursor]) else {
                    cursor += 1;
                    continue;
                };
                let cell_end = container_end(inner, cursor);
                if cell_attrs.has_class("cell") {
                    cells.push(TableCell {
                        blocks: self.blocks(&inner[cursor + 1..cell_end]),
                        colspan: cell_attrs
                            .get("colspan")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1),
                        rowspan: cell_attrs
                            .get("rowspan")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(1),
                        ..Default::default()
                    });
                }
                cursor = cell_end + 1;
            }
            rows.push(TableRow {
                cells,
                is_header: false,
            });
            index = row_end + 1;
        }

        let header_row = attrs.flag("header-row") == Some(true);
        if header_row {
            if let Some(first) = rows.first_mut() {
                first.is_header = true;
            }
        }
        let mut table = Table {
            style: attrs.get("style").map(StyleId::new),
            col_widths: attrs
                .get("col-widths")
                .map(|w| w.split(',').filter_map(parse_len).collect())
                .unwrap_or_default(),
            rows,
            header_row,
        };
        mark_covered_cells(&mut table);
        table
    }

    // ----------------------------------------------------------------------
    // Lists
    // ----------------------------------------------------------------------

    /// Reads one list, starting at `start`, and returns the line after it.
    fn list(&mut self, lines: &[&str], start: usize) -> (List, usize) {
        let (ordered, _) = list_marker(lines[start]).unwrap_or((false, 2));
        let mut items: Vec<ListItem> = Vec::new();
        let mut def: Option<ListId> = None;
        let mut index = start;

        while index < lines.len() {
            let Some((item_ordered, marker_len)) = list_marker(lines[index]) else {
                break;
            };
            if item_ordered != ordered {
                break;
            }
            // The first line of the item is whatever follows its marker.
            let mut body: Vec<String> = vec![lines[index][marker_len..].to_string()];
            index += 1;
            while let Some(line) = lines.get(index) {
                if line.trim().is_empty() {
                    // Only a continuation if indented content follows.
                    match lines.get(index + 1) {
                        Some(next) if next.len() > marker_len && next.starts_with(' ') => {
                            body.push(String::new());
                            index += 1;
                        }
                        _ => break,
                    }
                } else if line.len() > marker_len && line.starts_with(&" ".repeat(marker_len)) {
                    body.push(line[marker_len..].to_string());
                    index += 1;
                } else {
                    break;
                }
            }

            let refs: Vec<&str> = body.iter().map(String::as_str).collect();
            let mut item_def: Option<ListId> = None;
            let blocks = self.item_blocks(&refs, &mut item_def);
            if def.is_none() {
                def = item_def;
            }
            items.push(ListItem { blocks });
        }

        (
            List {
                def,
                ordered,
                level: self.depth,
                items,
            },
            index,
        )
    }

    /// An item's blocks, pulling `list=` out of the leading paragraph.
    fn item_blocks(&mut self, lines: &[&str], def: &mut Option<ListId>) -> Vec<Block> {
        let mut out: Vec<Block> = Vec::new();
        let mut first_end = if lines.is_empty() { 0 } else { 1 };
        while first_end < lines.len()
            && !lines[first_end].trim().is_empty()
            && container_open(lines[first_end]).is_none()
            && heading(lines[first_end]).is_none()
            && !table_starts(lines, first_end)
            && list_marker(lines[first_end]).is_none()
        {
            first_end += 1;
        }
        if first_end > 0 {
            let text = lines[..first_end].join("\n");
            // Deliberately *not* collapsed to a block-level image: the list
            // definition is named inside this paragraph's attribute block, and
            // an image has no room for it.
            out.push(Block::Paragraph(self.paragraph(&text, Some(def))));
        }
        self.depth = self.depth.saturating_add(1);
        out.extend(self.blocks(&lines[first_end..]));
        self.depth -= 1;
        out
    }
}

// --------------------------------------------------------------------------
// Line classification
// --------------------------------------------------------------------------

/// A paragraph as the block it really is: one holding nothing but a picture
/// writes exactly what a block-level image writes, so that is what it is.
fn as_block(paragraph: Paragraph) -> Block {
    match paragraph.content.as_slice() {
        [Inline::Image(image)] if paragraph.format.is_empty() => Block::Image(image.clone()),
        _ => Block::Paragraph(paragraph),
    }
}

/// The attributes of a container opener, or `None` for any other line.
fn container_open(line: &str) -> Option<Attrs> {
    let rest = line.trim().strip_prefix(":::")?.trim();
    if rest.is_empty() {
        return None; // this is a closing fence
    }
    Attrs::parse(rest).or_else(|| Some(Attrs::new()))
}

fn is_container_close(line: &str) -> bool {
    line.trim() == ":::"
}

/// Index of the fence closing the container that opens at `start`.
///
/// Fenced code inside a raw-block is skipped, so a `:::` that is part of the
/// preserved markup cannot close the container around it.
fn container_end(lines: &[&str], start: usize) -> usize {
    let mut depth = 0usize;
    let mut index = start;
    let mut fence: Option<usize> = None;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        let backticks = trimmed.len() - trimmed.trim_start_matches('`').len();
        match fence {
            Some(open) if backticks >= open => fence = None,
            Some(_) => {}
            None if backticks >= 3 => fence = Some(backticks),
            None if container_open(lines[index]).is_some() => depth += 1,
            None if is_container_close(lines[index]) => {
                depth -= 1;
                if depth == 0 {
                    return index;
                }
            }
            None => {}
        }
        index += 1;
    }
    // Unterminated: everything to the end belongs to it.
    lines.len()
}

/// `# Título` → `(1, "Título")`.
fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = line[hashes..].strip_prefix(' ')?;
    Some((hashes as u8, rest))
}

/// `- ` or `N. ` → `(ordered, marker length)`.
fn list_marker(line: &str) -> Option<(bool, usize)> {
    if let Some(rest) = line.strip_prefix("- ") {
        let _ = rest;
        return Some((false, 2));
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits == 0 {
        return None;
    }
    if line[digits..].starts_with(". ") {
        return Some((true, digits + 2));
    }
    None
}

/// `[^1]: texto` → `("1", "texto")`.
fn footnote_definition(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("[^")?;
    let close = rest.find("]: ")?;
    Some((&rest[..close], &rest[close + 3..]))
}

/// True when a GFM table opens here: a pipe row whose next line is the
/// delimiter. Without that second line a leading `|` is ordinary text, and a
/// paragraph that happens to wrap onto one must not be cut in two.
fn table_starts(lines: &[&str], index: usize) -> bool {
    lines
        .get(index)
        .is_some_and(|l| l.trim_start().starts_with('|'))
        && lines.get(index + 1).is_some_and(|l| is_delimiter_row(l))
}

fn is_delimiter_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
        && trimmed.contains('-')
}

/// Splits a `| a | b |` row on its unescaped pipes.
fn split_row(line: &str) -> Vec<String> {
    let trimmed = line.trim();
    let inner = trimmed
        .strip_prefix('|')
        .unwrap_or(trimmed)
        .strip_suffix('|')
        .unwrap_or(trimmed);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                current.push(c);
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            '|' => cells.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    cells.push(current);
    cells
}

/// End of the run of lines from `start` for which `keep` holds.
fn run_end(lines: &[&str], start: usize, keep: impl Fn(&str) -> bool) -> usize {
    let mut index = start;
    // The first line is taken on the caller's word.
    if index < lines.len() {
        index += 1;
    }
    while index < lines.len() && keep(lines[index]) {
        index += 1;
    }
    index
}

/// Restores `covered` from the spans, which is where the flag lives in the IR.
///
/// The serialiser writes a covered cell as an empty one, because that is what
/// a GFM grid can hold; the geometry is what says which cells those are.
fn mark_covered_cells(table: &mut Table) {
    let width = table.width().max(1);
    let rows = table.rows.len();
    let mut covered = vec![vec![false; width]; rows];
    for (row_index, row) in table.rows.iter().enumerate() {
        let mut column = 0usize;
        for cell in &row.cells {
            let colspan = cell.colspan.max(1) as usize;
            let rowspan = cell.rowspan.max(1) as usize;
            let last_row = (row_index + rowspan).min(rows);
            for (r, row_flags) in covered
                .iter_mut()
                .enumerate()
                .take(last_row)
                .skip(row_index)
            {
                for (c, flag) in row_flags
                    .iter_mut()
                    .enumerate()
                    .take((column + colspan).min(width))
                    .skip(column)
                {
                    // The cell that opens the area is not covered by itself.
                    if r != row_index || c != column {
                        *flag = true;
                    }
                }
            }
            column += colspan;
        }
    }
    for (row_index, row) in table.rows.iter_mut().enumerate() {
        let mut column = 0usize;
        for cell in &mut row.cells {
            cell.covered = covered
                .get(row_index)
                .and_then(|r| r.get(column))
                .copied()
                .unwrap_or(false);
            column += cell.colspan.max(1) as usize;
        }
    }
}

/// Unescapes a plain-text fragment; used where no inline markup is expected.
#[allow(dead_code)]
fn plain(text: &str) -> String {
    unescape(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::text::Orientation;

    fn parse(source: &str) -> (Vec<Section>, ConversionReport) {
        let assets = BTreeMap::new();
        let mut report = ConversionReport::new();
        let sections = {
            let mut parser = BlockParser::new(&assets, &mut report, &FrontMatter::default());
            let lines: Vec<&str> = source.lines().collect();
            parser.collect_footnotes(&lines);
            parser.document(&lines)
        };
        (sections, report)
    }

    fn blocks(source: &str) -> Vec<Block> {
        let (sections, _) = parse(source);
        sections.into_iter().flat_map(|s| s.blocks).collect()
    }

    #[test]
    fn headings_carry_their_level_and_style() {
        let blocks = blocks("# Titulo de nivel 1 {.Heading1}\n\n## Nivel 2 {.Heading2}\n");
        let [Block::Heading(first), Block::Heading(second)] = blocks.as_slice() else {
            panic!("expected two headings, got {blocks:?}");
        };
        assert_eq!(first.level, 1);
        assert_eq!(first.paragraph.plain_text(), "Titulo de nivel 1");
        assert_eq!(
            first.paragraph.format.style.as_ref().unwrap().as_str(),
            "Heading1"
        );
        assert_eq!(second.level, 2);
    }

    #[test]
    fn paragraph_attributes_read_back() {
        let blocks =
            blocks("Parrafo centrado. {align=center indent-first-line=24px space-after=6pt}\n");
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected a paragraph");
        };
        assert_eq!(
            p.format.direct.align,
            Some(docsai_model::style::Align::Center)
        );
        assert_eq!(
            p.format.direct.indent_first_line,
            Some(docsai_model::units::Length::from_px(24.0))
        );
    }

    #[test]
    fn an_empty_paragraph_survives_as_an_empty_paragraph() {
        let blocks = blocks("Antes.\n\n[]{.empty}\n\nDespues.\n");
        assert_eq!(blocks.len(), 3);
        let [_, Block::Paragraph(empty), _] = blocks.as_slice() else {
            panic!("expected the empty paragraph in the middle");
        };
        assert!(empty.content.is_empty());
        assert!(empty.format.is_empty());
    }

    #[test]
    fn a_standalone_image_is_a_block_not_a_paragraph() {
        let blocks = blocks("![x](assets/img-1.png){width=1cm height=1cm}\n");
        assert!(matches!(blocks.as_slice(), [Block::Image(_)]));
    }

    #[test]
    fn an_image_followed_by_text_stays_inside_its_paragraph() {
        // Floating images are written glued to the text that flows round them.
        let blocks = blocks("![x](assets/img-1.png){anchor=floating width=1cm height=1cm}Texto.\n");
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected a paragraph, got {blocks:?}");
        };
        assert_eq!(p.content.len(), 2);
        assert!(matches!(p.content[0], Inline::Image(_)));
    }

    #[test]
    fn nested_lists_rebuild_their_tree() {
        let source = "1. Primer punto {.ListParagraph list=L1}\n   1. Sub-punto a {.ListParagraph}\n   2. Sub-punto b\n2. Segundo punto\n";
        let blocks = blocks(source);
        let [Block::List(list)] = blocks.as_slice() else {
            panic!("expected one list, got {blocks:?}");
        };
        assert!(list.ordered);
        assert_eq!(list.def.as_ref().unwrap().as_str(), "L1");
        assert_eq!(list.items.len(), 2);

        let [Block::Paragraph(head), Block::List(nested)] = list.items[0].blocks.as_slice() else {
            panic!("expected a paragraph and a nested list");
        };
        assert_eq!(head.plain_text(), "Primer punto");
        assert_eq!(
            head.format.style.as_ref().unwrap().as_str(),
            "ListParagraph",
            "`list=` must not survive as a style"
        );
        assert_eq!(nested.items.len(), 2);
    }

    #[test]
    fn bullet_and_ordered_lists_do_not_merge() {
        let blocks = blocks("- uno\n- dos\n\n1. tres\n");
        assert_eq!(blocks.len(), 2);
        let [Block::List(bullets), Block::List(ordered)] = blocks.as_slice() else {
            panic!("expected two lists, got {blocks:?}");
        };
        assert!(!bullets.ordered);
        assert!(ordered.ordered);
    }

    #[test]
    fn a_gfm_table_rebuilds_its_spans_and_covered_cells() {
        let source = concat!(
            "::: {.table col-widths=\"125pt,125pt\" header-row=false style=TableGrid}\n",
            "|                   |                        |     |       |\n",
            "| ----------------- | ---------------------- | --- | ----- |\n",
            "| Region            | Trimestres {colspan=2} |     | Total |\n",
            "| Norte {rowspan=2} | 100                    | 200 | 300   |\n",
            "|                   | 150                    | 250 | 400   |\n",
            ":::\n"
        );
        let blocks = blocks(source);
        let [Block::Table(table)] = blocks.as_slice() else {
            panic!("expected a table, got {blocks:?}");
        };
        assert!(!table.header_row, "the empty header row was the writer's");
        assert_eq!(table.rows.len(), 3);
        assert_eq!(table.style.as_ref().unwrap().as_str(), "TableGrid");
        assert_eq!(table.width(), 4);

        // Row 0: Region | Trimestres(colspan=2) | Total — three cells, not four.
        assert_eq!(table.rows[0].cells.len(), 3);
        assert_eq!(table.rows[0].cells[1].colspan, 2);

        assert_eq!(table.rows[1].cells[0].rowspan, 2);
        assert!(!table.rows[1].cells[0].covered);
        assert!(
            table.rows[2].cells[0].covered,
            "the cell under a rowspan is covered"
        );
    }

    #[test]
    fn a_complex_table_reads_its_row_and_cell_containers() {
        let source = concat!(
            "::: {.table complex=true style=TableGrid}\n",
            "::: {.row}\n",
            "::: {.cell rowspan=2}\n",
            "Primer parrafo.\n",
            "\n",
            "Segundo parrafo.\n",
            ":::\n",
            "::: {.cell}\n",
            "Otra celda.\n",
            ":::\n",
            ":::\n",
            ":::\n"
        );
        let blocks = blocks(source);
        let [Block::Table(table)] = blocks.as_slice() else {
            panic!("expected a table, got {blocks:?}");
        };
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.rows[0].cells.len(), 2);
        assert_eq!(table.rows[0].cells[0].blocks.len(), 2);
        assert_eq!(table.rows[0].cells[0].rowspan, 2);
        assert!(table.is_complex());
    }

    #[test]
    fn headers_and_footers_attach_to_their_section() {
        let source = concat!(
            "::: {.header scope=default}\n",
            "Cabecera\n",
            ":::\n",
            "\n",
            "::: {.footer scope=first}\n",
            "Pie\n",
            ":::\n",
            "\n",
            "Cuerpo.\n"
        );
        let (sections, _) = parse(source);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].headers.len(), 1);
        assert_eq!(sections[0].headers[0].scope, HeaderScope::Default);
        assert_eq!(sections[0].footers[0].scope, HeaderScope::First);
        assert_eq!(sections[0].blocks.len(), 1);
    }

    #[test]
    fn section_containers_become_sections() {
        let source = concat!(
            "::: {.section columns=2 orientation=landscape page-size=A4}\n",
            "Primera.\n",
            ":::\n",
            "\n",
            "::: {.section orientation=portrait page-size=A4}\n",
            "Segunda.\n",
            ":::\n"
        );
        let (sections, _) = parse(source);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].page.columns, 2);
        assert_eq!(sections[0].page.orientation, Orientation::Landscape);
        assert_eq!(sections[1].page.orientation, Orientation::Portrait);
    }

    #[test]
    fn footnote_definitions_reach_their_reference() {
        let source = "Texto con nota[^1] y sigue.\n\n[^1]: Primera nota al pie.\n";
        let blocks = blocks(source);
        let [Block::Paragraph(p)] = blocks.as_slice() else {
            panic!("expected one paragraph, got {blocks:?}");
        };
        let footnote = p
            .content
            .iter()
            .find_map(|i| match i {
                Inline::Footnote(blocks) => Some(blocks),
                _ => None,
            })
            .expect("a footnote");
        let [Block::Paragraph(note)] = footnote.as_slice() else {
            panic!("expected the note's paragraph");
        };
        assert_eq!(note.plain_text(), "Primera nota al pie.");
    }

    #[test]
    fn a_raw_block_keeps_its_bytes_exactly() {
        let source = concat!(
            "::: {#raw-0001 .raw format=ooxml part=\"word/document.xml\"}\n",
            "```xml\n",
            "<w:sdt><w:sdtPr/><w:sdtContent>::: no es un cierre</w:sdtContent></w:sdt>\n",
            "```\n",
            ":::\n",
            "\n",
            "Despues.\n"
        );
        let blocks = blocks(source);
        let [Block::Raw(raw), Block::Paragraph(_)] = blocks.as_slice() else {
            panic!("expected a raw block then a paragraph, got {blocks:?}");
        };
        assert_eq!(raw.id.as_str(), "raw-0001");
        assert_eq!(raw.format, "ooxml");
        assert_eq!(raw.part, "word/document.xml");
        assert!(
            raw.content.contains("::: no es un cierre"),
            "a `:::` inside the fence must not close the container"
        );
    }

    #[test]
    fn an_unknown_container_keeps_its_content_and_says_so() {
        let (sections, report) = parse("::: {.invento}\nTexto que no se pierde.\n:::\n");
        let blocks: Vec<&Block> = sections.iter().flat_map(|s| s.blocks.iter()).collect();
        assert_eq!(blocks.len(), 1);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.message().contains("invento")));
    }
}
