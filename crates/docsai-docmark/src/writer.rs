//! IR → DocMark (spec §3, §7, §8).
//!
//! The serializer is written by hand rather than delegated to a Markdown
//! library so that every byte of the output is under our control: the
//! idempotence test of Phase 2 (`serialize(parse(md)) == md`) has no room for a
//! library's own formatting opinions.

use docsai_model::assets::AssetStore;
use docsai_model::image::{Anchor, AxisPos, Flip, ImageRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::{FontProps, ResolvedStyle, StyleCatalog, StyleId, Underline, VertAlign};
use docsai_model::text::{
    Block, BreakKind, HeaderFooter, Inline, List, Paragraph, RawFragment, RunProps, Section, Table,
    TableCell, TextDocument,
};
use docsai_model::units::Length;

use docsai_model::addressing::{list_is_addressable, paragraph_is_container, NodeKind};

use crate::attrs::Attrs;
use crate::dict::AttrDict;
use crate::escape::{escape, TextContext};
use crate::ids::IdSource;
use crate::units::{geometry, len, number, percent, pt};
use crate::{Fidelity, Options, RawPolicy};

/// Serialises a text document body (front matter excluded).
pub struct Writer<'a> {
    out: String,
    options: &'a Options,
    styles: &'a StyleCatalog,
    assets: &'a dyn AssetStore,
    report: ConversionReport,
    /// Rendered footnote definitions, emitted at the very end.
    footnotes: Vec<String>,
    /// Node ids for this run (spec §11.1).
    ids: &'a mut IdSource,
    /// The attribute-set dictionary (spec §3.7). Shared rather than owned: the
    /// first pass only counts through it, so it needs no mutable access.
    dict: &'a AttrDict,
    /// The formatting the paragraph being rendered already implies. A run
    /// inherits from its paragraph's style before it inherits from the
    /// document (the middle of the OOXML cascade), so this is the base every
    /// run delta is measured against.
    para_style: ResolvedStyle,
}

impl<'a> Writer<'a> {
    pub fn new(
        options: &'a Options,
        styles: &'a StyleCatalog,
        assets: &'a dyn AssetStore,
        ids: &'a mut IdSource,
        dict: &'a AttrDict,
    ) -> Self {
        Writer {
            out: String::new(),
            options,
            styles,
            assets,
            report: ConversionReport::new(),
            footnotes: Vec::new(),
            ids,
            dict,
            para_style: styles.resolve(None),
        }
    }

    fn full(&self) -> bool {
        self.options.fidelity == Fidelity::Full
    }

    fn plain(&self) -> bool {
        self.options.fidelity == Fidelity::Plain
    }

    /// Whether appearance reaches the output. `agent` says no: it keeps the
    /// text, the structure and the addresses, and drops every measurement.
    fn formatting(&self) -> bool {
        self.options.fidelity.formatting()
    }

    /// Decimals a readable unit may use (spec §2).
    fn precision(&self) -> u8 {
        self.options.precision
    }

    /// A drawing length: image sizes and anchor offsets, in pixels.
    fn geometry(&self, value: Length) -> String {
        geometry(value, self.precision())
    }

    /// Writes the whole document body and returns the text and the report.
    pub fn finish(mut self, doc: &TextDocument) -> (String, ConversionReport) {
        let multi_section = doc.sections.len() > 1;
        for section in &doc.sections {
            self.write_section(section, multi_section);
        }
        if !self.footnotes.is_empty() {
            let definitions = std::mem::take(&mut self.footnotes);
            for definition in definitions {
                self.block(&definition);
            }
        }
        // A document always ends with exactly one newline.
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
        (self.out, self.report)
    }

    // ----------------------------------------------------------------------
    // The presentation profile (spec §11.2)
    //
    // A deck is written by `deck_writer`, which owns the slide layout of the
    // output and borrows this writer for the parts a slide shares with a text
    // document: paragraphs, lists, tables and images render exactly as they do
    // in a `.docx`, and a second renderer for them is how two spellings of the
    // same list appear.
    // ----------------------------------------------------------------------

    /// The ids of this run, for the nodes `deck_writer` addresses itself.
    pub(crate) fn ids(&mut self) -> &mut IdSource {
        self.ids
    }

    /// The report this run is accumulating.
    pub(crate) fn report_mut(&mut self) -> &mut ConversionReport {
        &mut self.report
    }

    /// The report, once the caller has finished writing through this writer.
    pub(crate) fn into_report(self) -> ConversionReport {
        self.report
    }

    /// The attribute-set dictionary, for the containers `deck_writer` renders.
    pub(crate) fn dict(&self) -> &AttrDict {
        self.dict
    }

    /// A drawing length — a shape's position or size — in the unit spec §2
    /// gives drawings, with `emu` as the exact fallback.
    pub(crate) fn drawing_length(&self, value: Length) -> String {
        self.geometry(value)
    }

    /// Renders a run of blocks at slide level: the layout's primary body
    /// placeholder, written as ordinary Markdown (spec §11.2 rule 2).
    pub(crate) fn render_slide_blocks(&mut self, blocks: &[Block]) -> String {
        self.render_blocks(blocks, 0)
    }

    /// Renders the slide heading, which *is* the title placeholder (spec §11.2
    /// rule 1): no container repeats it.
    ///
    /// `seed` carries what the slide knows and the paragraph does not — the
    /// slide's id, `.slide`, its layout and its section — and wins over the
    /// paragraph's own attributes. A title with no text writes an empty
    /// heading, which the spec accepts as ugly and bounded.
    pub(crate) fn render_slide_heading(
        &mut self,
        title: &[Block],
        seed: Attrs,
        location: &str,
    ) -> String {
        let mut attrs = Attrs::new();
        let mut parts: Vec<String> = Vec::new();
        for (index, block) in title.iter().enumerate() {
            let paragraph = match block {
                Block::Paragraph(p) => p,
                Block::Heading(h) => &h.paragraph,
                // A picture or a table inside a title placeholder has no
                // heading to be rendered into. It is not silently dropped:
                // rule 8's stub is `deck_writer`'s job, and until then this
                // says so.
                other => {
                    self.report.warn(Warning::UnsupportedElement {
                        kind: block_kind(other).into(),
                        location: location.into(),
                        action: "skipped: a slide title holds text".into(),
                    });
                    continue;
                }
            };
            if index > 0 {
                // Every paragraph of the title reaches the heading, because a
                // heading is one line and losing the second one would lose
                // text. The paragraph break is what goes, and it is warned.
                self.report.warn(Warning::UnsupportedElement {
                    kind: "paragraph".into(),
                    location: location.into(),
                    action: "joined into the slide heading".into(),
                });
            }
            self.report.stats.paragraphs += 1;
            let outer = std::mem::replace(
                &mut self.para_style,
                self.styles.resolve(paragraph.format.style.as_ref()),
            );
            let content = self.render_inlines(&paragraph.content, TextContext::Block);
            self.para_style = outer;
            if index == 0 && !self.plain() {
                attrs = self.paragraph_attrs(paragraph, None);
            }
            let content = content.trim().to_string();
            if !content.is_empty() {
                parts.push(content);
            }
        }
        if !self.plain() {
            attrs.merge(&seed);
        }
        self.report.stats.headings += 1;
        let body = parts.join(" ");
        if body.is_empty() {
            // `##` and nothing else: no trailing space, so the line is stable
            // under `serialize(parse(md)) == md`.
            format!("##{}", attrs.suffix_with(self.dict))
        } else {
            format!("## {body}{}", attrs.suffix_with(self.dict))
        }
    }

    /// Appends a block, separated from the previous one by a blank line.
    fn block(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            if !self.out.ends_with('\n') {
                self.out.push('\n');
            }
            self.out.push('\n');
        }
        self.out.push_str(text.trim_end_matches('\n'));
        self.out.push('\n');
    }

    // ----------------------------------------------------------------------
    // Sections, headers and footers
    // ----------------------------------------------------------------------

    fn write_section(&mut self, section: &Section, wrap: bool) {
        if !self.plain() {
            for header in &section.headers {
                self.write_header_footer(header, "header");
            }
            for footer in &section.footers {
                self.write_header_footer(footer, "footer");
            }
        }

        // A single-section document has no `::: {.section}` container to hold
        // an id, so it does not take one (see `for_each_addressable`).
        let mark = self.ids.mark();
        let id = wrap.then(|| self.ids.take(section)).flatten();
        let body = self.render_blocks(&section.blocks, 0);
        if wrap && !self.plain() {
            let mut attrs = Attrs::new();
            attrs.class("section");
            if let Some(id) = id.clone() {
                attrs.id(id);
            }
            if self.formatting() {
                if section.page.columns > 1 {
                    attrs.set("columns", section.page.columns.to_string());
                }
                if let Some(paper) = section.page.paper_name() {
                    attrs.set("page-size", paper);
                }
                attrs.set("orientation", section.page.orientation.as_str());
            }
            let rendered = format!(
                "::: {}\n{}\n:::",
                attrs.render_with(self.dict),
                body.trim_end()
            );
            self.ids
                .record(id.as_deref(), NodeKind::Section, &rendered, mark);
            self.block(&rendered);
        } else {
            self.block(&body);
        }
    }

    fn write_header_footer(&mut self, part: &HeaderFooter, kind: &str) {
        let body = self.render_blocks(&part.blocks, 0);
        if body.trim().is_empty() {
            return;
        }
        let mut attrs = Attrs::new();
        attrs.class(kind).set("scope", part.scope.as_str());
        let rendered = format!(
            "::: {}\n{}\n:::",
            attrs.render_with(self.dict),
            body.trim_end()
        );
        self.block(&rendered);
    }

    // ----------------------------------------------------------------------
    // Blocks
    // ----------------------------------------------------------------------

    /// Renders a run of blocks into one string, blank-line separated.
    ///
    /// One exception: a list directly after a paragraph is joined with a
    /// single newline, which is what keeps a nested list *tight* in
    /// CommonMark instead of wrapping every item in a `<p>`.
    fn render_blocks(&mut self, blocks: &[Block], depth: usize) -> String {
        let mut out = String::new();
        let mut previous: Option<&Block> = None;
        for block in blocks {
            let text = self.render_block(block, depth);
            if text.trim().is_empty() {
                continue;
            }
            if !out.is_empty() {
                // Only *inside* a list item: at the top level a blank line is
                // what separates a paragraph from the list that follows it.
                let tight = depth > 0
                    && matches!(previous, Some(Block::Paragraph(_)))
                    && matches!(block, Block::List(_));
                out.push_str(if tight { "\n" } else { "\n\n" });
            }
            out.push_str(&text);
            previous = Some(block);
        }
        out
    }

    fn render_block(&mut self, block: &Block, depth: usize) -> String {
        match block {
            Block::Paragraph(p) => {
                let mark = self.ids.mark();
                let id = paragraph_is_container(p)
                    .then(|| self.ids.take(p))
                    .flatten();
                let rendered = self.render_paragraph(p, None, &[], id.clone());
                self.ids
                    .record(id.as_deref(), NodeKind::Paragraph, &rendered, mark);
                rendered
            }
            Block::Heading(h) => {
                let hashes = "#".repeat(h.level.clamp(1, 6) as usize);
                let mark = self.ids.mark();
                let id = self.ids.take(h);
                let rendered = self.render_paragraph(&h.paragraph, Some(&hashes), &[], id.clone());
                self.ids
                    .record(id.as_deref(), NodeKind::Heading, &rendered, mark);
                rendered
            }
            Block::List(list) => self.render_list(list, depth),
            Block::Table(table) => self.render_table(table),
            Block::Image(image) => self.render_image(image),
            Block::TextBox(text_box) => {
                let body = self.render_blocks(&text_box.blocks, depth);
                if self.plain() {
                    return body;
                }
                let mut attrs = Attrs::new();
                attrs.class("textbox");
                if self.formatting() {
                    if let Some(size) = text_box.size {
                        attrs
                            .set("width", self.geometry(size.width))
                            .set("height", self.geometry(size.height));
                    }
                    if let Some(x) = text_box.x {
                        attrs.set("x", self.geometry(x));
                    }
                    if let Some(y) = text_box.y {
                        attrs.set("y", self.geometry(y));
                    }
                }
                format!(
                    "::: {}\n{}\n:::",
                    attrs.render_with(self.dict),
                    body.trim_end()
                )
            }
            Block::Raw(raw) => self.render_raw(raw),
        }
    }

    /// A raw-block (spec §7). Dropped in `standard` and `plain`, which is a
    /// documented loss and therefore a warning. `agent` keeps it, always as a
    /// stub: an agent has to *see* that something is there — it addresses the
    /// node — but the bytes are the definition of what it cannot edit.
    fn render_raw(&mut self, raw: &RawFragment) -> String {
        if !self.options.fidelity.addresses() {
            self.report.warn(Warning::RawBlockDropped {
                id: raw.id.as_str().to_string(),
                format: raw.format.clone(),
            });
            return String::new();
        }
        self.report.raw_blocks_emitted += 1;
        let mut attrs = Attrs::new();
        attrs
            .class("raw")
            .set("format", &raw.format)
            .set("part", &raw.part)
            .id(raw.id.as_str());
        if self.options.raw == RawPolicy::Sidecar || !self.full() {
            // The bytes are in a file of their own; the body keeps the stub so
            // a reader sees that something is there without paying for it.
            attrs.set(
                "src",
                crate::raw::sidecar_path(&self.options.assets_dir, raw),
            );
            return format!("::: {}\n:::", attrs.render_with(self.dict));
        }
        // The fence must be longer than the longest backtick run inside.
        let longest = raw
            .content
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat(longest.max(2) + 1);
        format!(
            "::: {}\n{fence}xml\n{}\n{fence}\n:::",
            attrs.render_with(self.dict),
            raw.content.trim_end()
        )
    }

    /// Renders a paragraph.
    ///
    /// `prefix` turns it into a heading; `extra` merges more pairs into its
    /// attribute block, which is how a list names its definition and its id
    /// without growing a second `{...}` (spec §3.3); `id` is the node's own
    /// address.
    fn render_paragraph(
        &mut self,
        paragraph: &Paragraph,
        prefix: Option<&str>,
        extra: &[(&str, String)],
        id: Option<String>,
    ) -> String {
        self.report.stats.paragraphs += 1;
        // A footnote body is a paragraph inside a paragraph, so the context is
        // saved and put back rather than simply overwritten.
        let outer = std::mem::replace(
            &mut self.para_style,
            self.styles.resolve(paragraph.format.style.as_ref()),
        );
        let content = self.render_inlines(&paragraph.content, TextContext::Block);
        self.para_style = outer;
        let mut attrs = if self.plain() {
            Attrs::new()
        } else {
            self.paragraph_attrs(paragraph, prefix.map(|hashes| hashes.len() as u8))
        };
        if !self.plain() {
            if let Some(id) = id {
                attrs.id(id);
            }
            for (key, value) in extra {
                attrs.set(*key, value);
            }
        }

        let body = content.trim_end_matches(' ');
        match prefix {
            Some(hashes) => format!("{hashes} {body}{}", attrs.suffix_with(self.dict)),
            // An empty paragraph is real content in a word processor (it is
            // how people space a document out), and a blank line cannot carry
            // it: Markdown would merge it away. Full fidelity marks it with an
            // empty span so the reverse conversion can put it back; the lossy
            // modes drop it, which is exactly what they promise (spec §6).
            None if body.is_empty() && attrs.is_empty() => {
                if self.full() {
                    "[]{.empty}".to_string()
                } else {
                    String::new()
                }
            }
            None if body.is_empty() => format!("[]{{.empty}}{}", attrs.suffix_with(self.dict)),
            None => format!("{body}{}", attrs.suffix_with(self.dict)),
        }
    }

    /// True when naming the style would say what the document already says
    /// (spec §3.1, the economy rule).
    ///
    /// Two cases, and both have to be reversible or the class cannot go: the
    /// default paragraph style, which applies wherever nothing else does, and
    /// the style a heading of this level names by being that heading.
    fn style_is_implied(&self, style: &StyleId, heading_level: Option<u8>) -> bool {
        match heading_level {
            Some(level) => self.styles.heading_style(level) == Some(style),
            None => self.styles.default_paragraph_style().map(|s| &s.id) == Some(style),
        }
    }

    /// The attribute block of a paragraph: its style plus whatever direct
    /// formatting the style does not already imply (the economy rule, §3.1).
    fn paragraph_attrs(&self, paragraph: &Paragraph, heading_level: Option<u8>) -> Attrs {
        let mut attrs = Attrs::new();
        let style = paragraph.format.style.as_ref();
        if let Some(style) = style {
            if !self.style_is_implied(style, heading_level) {
                attrs.class(style.as_str());
            }
        }
        if !self.formatting() {
            // The style *name* survives — it is what the paragraph is — but
            // its indents and spacing are not something an agent edits.
            return attrs;
        }
        let resolved = self.styles.resolve(style);
        let direct = paragraph.format.direct.minus(&resolved.paragraph);

        if let Some(align) = direct.align {
            attrs.set("align", align.as_str());
        }
        push_len(
            &mut attrs,
            "indent-left",
            direct.indent_left,
            self.precision(),
        );
        push_len(
            &mut attrs,
            "indent-right",
            direct.indent_right,
            self.precision(),
        );
        push_len(
            &mut attrs,
            "indent-first-line",
            direct.indent_first_line,
            self.precision(),
        );
        push_len(
            &mut attrs,
            "indent-hanging",
            direct.indent_hanging,
            self.precision(),
        );
        if let Some(value) = direct.space_before {
            attrs.set("space-before", pt(value));
        }
        if let Some(value) = direct.space_after {
            attrs.set("space-after", pt(value));
        }
        if let Some(value) = direct.background {
            attrs.set("background", value);
        }
        if direct.keep_with_next == Some(true) {
            attrs.set("keep-with-next", "true");
        }
        if direct.page_break_before == Some(true) {
            attrs.set("page-break-before", "true");
        }
        // The `#` count already carries the heading depth.
        if heading_level.is_none() {
            if let Some(level) = direct.outline_level {
                attrs.set("outline-level", level.to_string());
            }
        }
        attrs
    }

    fn render_list(&mut self, list: &List, depth: usize) -> String {
        self.report.stats.lists += 1;
        let list_mark = self.ids.mark();
        let mut list_id = list_is_addressable(list)
            .then(|| self.ids.take(list))
            .flatten();
        let traced_id = list_id.clone();
        let mut out = String::new();
        for (index, item) in list.items.iter().enumerate() {
            let marker = if list.ordered {
                format!("{}. ", index + 1)
            } else {
                "- ".to_string()
            };
            let indent = " ".repeat(marker.len());
            // The list definition is named once, on the first item, inside
            // that item's own attribute block.
            let names_definition = index == 0 && !self.plain();
            let mut pairs: Vec<(&str, String)> = Vec::new();
            if names_definition {
                if let Some(def) = list.def.as_ref() {
                    pairs.push(("list", def.as_str().to_string()));
                }
                if let Some(id) = list_id.take() {
                    // The list's own address rides on its first item, the only
                    // attribute block a Markdown list offers.
                    pairs.push(("list-id", id));
                }
            }
            let body = match (pairs.is_empty(), item.blocks.split_first()) {
                (false, Some((Block::Paragraph(first), rest))) => {
                    let first_mark = self.ids.mark();
                    let first_id = paragraph_is_container(first)
                        .then(|| self.ids.take(first))
                        .flatten();
                    let head = self.render_paragraph(first, None, &pairs, first_id.clone());
                    self.ids
                        .record(first_id.as_deref(), NodeKind::Paragraph, &head, first_mark);
                    let tail = self.render_blocks(rest, depth + 1);
                    match rest.first() {
                        None => head,
                        Some(Block::List(_)) => format!("{head}\n{tail}"),
                        Some(_) => format!("{head}\n\n{tail}"),
                    }
                }
                _ => self.render_blocks(&item.blocks, depth + 1),
            };

            let mut lines = body.lines();
            let first = lines.next().unwrap_or("");
            out.push_str(&format!("{marker}{first}\n"));
            for line in lines {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str(&format!("{indent}{line}\n"));
                }
            }
        }
        let out = out.trim_end().to_string();
        self.ids
            .record(traced_id.as_deref(), NodeKind::List, &out, list_mark);
        out
    }

    // ----------------------------------------------------------------------
    // Tables
    // ----------------------------------------------------------------------

    fn render_table(&mut self, table: &Table) -> String {
        self.report.stats.tables += 1;
        if table.rows.is_empty() {
            return String::new();
        }
        let mark = self.ids.mark();
        let id = self.ids.take(table);
        let rendered = self.render_table_body(table, id.clone());
        self.ids
            .record(id.as_deref(), NodeKind::Table, &rendered, mark);
        rendered
    }

    fn render_table_body(&mut self, table: &Table, id: Option<String>) -> String {
        if table.is_complex() && !self.plain() {
            return self.render_complex_table(table, id);
        }

        let width = table.width().max(1);
        let mut grid: Vec<Vec<String>> = Vec::new();
        for row in &table.rows {
            let mut cells: Vec<String> = Vec::new();
            for cell in &row.cells {
                if cell.covered {
                    // Occupies one grid slot under a rowspan above; keep the
                    // GFM rectangle rectangular without re-emitting content.
                    cells.push(String::new());
                    continue;
                }
                cells.push(self.render_table_cell(cell));
                // A horizontal span leaves the absorbed columns empty so that
                // the GFM grid stays rectangular.
                for _ in 1..cell.colspan.max(1) {
                    cells.push(String::new());
                }
            }
            cells.resize(width, String::new());
            grid.push(cells);
        }

        // A GFM table always has a header row; when the source had none we
        // keep an empty one and record the fact in the container.
        let has_header = table.header_row;
        if !has_header {
            grid.insert(0, vec![String::new(); width]);
        }

        let widths = column_widths(&grid, width);
        let mut out = String::new();
        for (index, row) in grid.iter().enumerate() {
            out.push_str(&render_row(row, &widths));
            if index == 0 {
                out.push_str(&render_delimiter(&widths));
            }
        }

        if self.plain() {
            return out.trim_end().to_string();
        }
        let mut attrs = Attrs::new();
        if let Some(style) = &table.style {
            attrs.set("style", style.as_str());
        }
        if !table.col_widths.is_empty() && self.formatting() {
            let widths: Vec<String> = table
                .col_widths
                .iter()
                .map(|w| len(*w, self.precision()))
                .collect();
            attrs.set("col-widths", widths.join(","));
        }
        if !has_header {
            attrs.set("header-row", "false");
        }
        if let Some(id) = id {
            attrs.id(id);
        }
        if attrs.is_empty() {
            return out.trim_end().to_string();
        }
        attrs.class("table");
        format!(
            "::: {}\n{}\n:::",
            attrs.render_with(self.dict),
            out.trim_end()
        )
    }

    /// Cell content plus its span attributes, on one line.
    fn render_table_cell(&mut self, cell: &TableCell) -> String {
        let mut text = match cell.blocks.as_slice() {
            [] => String::new(),
            [Block::Paragraph(p)] => {
                // A one-paragraph cell is rendered inline, so it has to carry
                // its own style context the way `render_paragraph` would.
                let outer = std::mem::replace(
                    &mut self.para_style,
                    self.styles.resolve(p.format.style.as_ref()),
                );
                let text = self.render_inlines(&p.content, TextContext::TableCell);
                self.para_style = outer;
                text
            }
            blocks => {
                let rendered = self.render_blocks(blocks, 0);
                rendered.replace('\n', " ")
            }
        };
        text = text.trim().to_string();

        if self.plain() {
            return text;
        }
        let mut attrs = Attrs::new();
        if cell.colspan > 1 {
            attrs.set("colspan", cell.colspan.to_string());
        }
        if cell.rowspan > 1 {
            attrs.set("rowspan", cell.rowspan.to_string());
        }
        if let Some(background) = &cell.background {
            if self.formatting() {
                attrs.set("background", background);
            }
        }
        format!("{text}{}", attrs.suffix_with(self.dict))
            .trim()
            .to_string()
    }

    /// Tables GFM cannot hold (multi-paragraph or nested content, spec §3.4).
    fn render_complex_table(&mut self, table: &Table, id: Option<String>) -> String {
        let mut attrs = Attrs::new();
        attrs.class("table").set("complex", "true");
        if let Some(style) = &table.style {
            attrs.set("style", style.as_str());
        }
        if let Some(id) = id {
            attrs.id(id);
        }
        let mut out = format!("::: {}\n", attrs.render_with(self.dict));
        for row in &table.rows {
            let mut row_attrs = Attrs::new();
            row_attrs.class("row");
            let row_mark = self.ids.mark();
            let row_id = self.ids.take(row);
            if let Some(id) = row_id.clone() {
                row_attrs.id(id);
            }
            let row_start = out.len();
            out.push_str(&format!("::: {}\n", row_attrs.render_with(self.dict)));
            for cell in &row.cells {
                let mut cell_attrs = Attrs::new();
                cell_attrs.class("cell");
                if cell.colspan > 1 {
                    cell_attrs.set("colspan", cell.colspan.to_string());
                }
                if cell.rowspan > 1 {
                    cell_attrs.set("rowspan", cell.rowspan.to_string());
                }
                let body = self.render_blocks(&cell.blocks, 0);
                out.push_str(&format!(
                    "::: {}\n{}\n:::\n",
                    cell_attrs.render_with(self.dict),
                    body.trim_end()
                ));
            }
            out.push_str(":::\n");
            let row_markdown = out[row_start..].to_string();
            self.ids.record(
                row_id.as_deref(),
                NodeKind::TableRow,
                &row_markdown,
                row_mark,
            );
        }
        out.push_str(":::");
        out
    }

    // ----------------------------------------------------------------------
    // Inlines
    // ----------------------------------------------------------------------

    fn render_inlines(&mut self, inlines: &[Inline], context: TextContext) -> String {
        let mut out = String::new();
        for inline in inlines {
            out.push_str(&self.render_inline(inline, context));
        }
        out
    }

    fn render_inline(&mut self, inline: &Inline, context: TextContext) -> String {
        match inline {
            Inline::Text(text) => escape(text, context),
            Inline::Styled { content, props } => {
                let inner = self.render_inlines(content, context);
                self.wrap_styled(inner, props)
            }
            Inline::Link {
                target,
                content,
                props,
            } => {
                let content = strip_redundant_style(content, props);
                let label = self.render_inlines(&content, TextContext::LinkLabel);
                let link = format!("[{label}]({})", escape_link_target(target));
                if self.plain() || props.is_empty() {
                    link
                } else {
                    let mut attrs = Attrs::new();
                    self.style_attrs(&mut attrs, props);
                    format!("{link}{}", attrs.render_with(self.dict))
                }
            }
            Inline::Footnote(note) => {
                self.report.stats.footnotes += 1;
                let mark = self.ids.mark();
                let index = self.footnotes.len() + 1;
                let body = self.render_blocks(&note.blocks, 0);
                let mut lines = body.lines();
                // The source separates its own marker from the text with a
                // space; DocMark draws the marker itself.
                let first = lines.next().unwrap_or("").trim_start();
                let mut definition = format!("[^{index}]: {first}");
                for line in lines {
                    definition.push_str(&format!("\n    {line}"));
                }
                let mut attrs = Attrs::new();
                let id = self.ids.take(note);
                if let Some(id) = id.clone() {
                    attrs.id(id);
                }
                // A footnote is addressed on its reference but *costs* its
                // definition, which is what the reader ends up paying for.
                self.ids
                    .record(id.as_deref(), NodeKind::Footnote, &definition, mark);
                self.footnotes.push(definition);
                format!("[^{index}]{}", attrs.render_with(self.dict))
            }
            Inline::Field {
                kind,
                cached,
                instruction,
            } => {
                let text = escape(cached, context);
                if self.plain() {
                    return text;
                }
                let mut attrs = Attrs::new();
                attrs.class("field").set("field", kind.as_str());
                if self.full() && !instruction.is_empty() && instruction != kind.as_str() {
                    attrs.set("instr", instruction);
                }
                format!("[{text}]{}", attrs.render_with(self.dict))
            }
            Inline::Break(BreakKind::Line) => "  \n".to_string(),
            Inline::Break(kind) => {
                if self.plain() {
                    "\n".to_string()
                } else {
                    let mut attrs = Attrs::new();
                    attrs.class("break").set("kind", kind.as_str());
                    format!("[]{}", attrs.render_with(self.dict))
                }
            }
            Inline::Image(image) => self.render_image(image),
            Inline::Raw(raw) => {
                if self.options.fidelity.addresses() {
                    // An inline raw fragment still travels as a block-level
                    // raw-block; the spec has no inline form (§7).
                    let rendered = self.render_raw(raw);
                    format!("\n\n{rendered}\n\n")
                } else {
                    self.report.warn(Warning::RawBlockDropped {
                        id: raw.id.as_str().to_string(),
                        format: raw.format.clone(),
                    });
                    String::new()
                }
            }
        }
    }

    /// Wraps rendered text in the emphasis markers and the span its formatting
    /// needs. Order is fixed: strike inside italic inside bold inside the span.
    fn wrap_styled(&mut self, inner: String, props: &RunProps) -> String {
        if inner.is_empty() {
            return inner;
        }
        let resolved = self
            .styles
            .resolve_over(props.style.as_ref(), &self.para_style);
        let font = props.direct.minus(&resolved.font);

        let mut text = inner;
        if font.strike == Some(true) {
            text = format!("~~{text}~~");
        }
        if font.italic == Some(true) {
            text = format!("*{text}*");
        }
        if font.bold == Some(true) {
            text = format!("**{text}**");
        }
        if self.plain() {
            return text;
        }

        let mut attrs = Attrs::new();
        self.style_attrs(&mut attrs, props);
        if attrs.is_empty() {
            text
        } else {
            format!("[{text}]{}", attrs.render_with(self.dict))
        }
    }

    /// Everything about a run that the emphasis markers cannot express.
    fn style_attrs(&self, attrs: &mut Attrs, props: &RunProps) {
        if let Some(style) = &props.style {
            attrs.class(style.as_str());
        }
        let resolved: ResolvedStyle = self
            .styles
            .resolve_over(props.style.as_ref(), &self.para_style);
        let font: FontProps = props.direct.minus(&resolved.font);

        // Classes first: they name what the run *is*, and `agent` keeps them —
        // superscript is not decoration, it changes what the text says.
        if !matches!(font.underline, Some(Underline::None) | None) {
            attrs.class("underline");
        }
        match font.vert_align {
            Some(VertAlign::Superscript) => {
                attrs.class("sup");
            }
            Some(VertAlign::Subscript) => {
                attrs.class("sub");
            }
            _ => {}
        }
        if font.small_caps == Some(true) {
            attrs.class("small-caps");
        }
        if font.caps == Some(true) {
            attrs.class("caps");
        }
        if !self.formatting() {
            return;
        }

        if let Some(other) = font.underline {
            if !matches!(other, Underline::None | Underline::Single) {
                attrs.set("underline", underline_name(other));
            }
        }
        if font.bold == Some(false) {
            attrs.set("bold", "false");
        }
        if font.italic == Some(false) {
            attrs.set("italic", "false");
        }
        attrs.set_opt("color", font.color);
        attrs.set_opt("highlight", font.highlight);
        attrs.set_opt("font", font.name);
        if let Some(size) = font.size {
            attrs.set("size", pt(size));
        }
    }

    // ----------------------------------------------------------------------
    // Images
    // ----------------------------------------------------------------------

    fn render_image(&mut self, image: &ImageRef) -> String {
        let mark = self.ids.mark();
        let (rendered, id) = self.render_image_body(image);
        self.ids
            .record(id.as_deref(), NodeKind::Image, &rendered, mark);
        rendered
    }

    /// The image and the id it was written with, which is not necessarily the
    /// one the IR carries: `assign` hands out fresh ids at write time.
    fn render_image_body(&mut self, image: &ImageRef) -> (String, Option<String>) {
        self.report.stats.images += 1;
        let alt = escape(&image.alt, TextContext::LinkLabel);
        let path = match self.assets.info(&image.asset) {
            Some(info) => format!("{}/{}", self.options.assets_dir, info.file_name),
            None => image
                .external_src
                .clone()
                .unwrap_or_else(|| format!("{}/missing", self.options.assets_dir)),
        };

        if self.plain() {
            return (format!("![{alt}]({path})"), None);
        }

        let mut attrs = Attrs::new();
        let id = self.ids.take(image);
        if let Some(id) = id.clone() {
            attrs.id(id);
        }
        let geometry = &image.geometry;

        if !self.formatting() {
            // An image is a leaf an agent can move, delete or re-caption, and
            // its geometry is exactly the part it cannot write. The address,
            // the alt text and the link survive; the EMUs do not.
            attrs.set_opt("title", image.title.clone());
            attrs.set_opt("link", image.link.clone());
            return (
                format!("![{alt}]({path}){}", attrs.render_with(self.dict)),
                id,
            );
        }

        // `width`/`height` are mandatory except on two-cell sheet anchors,
        // where the grid defines the size (spec §4.1).
        let two_cell = matches!(geometry.anchor, Anchor::SheetTwoCell { .. });
        if !two_cell {
            attrs
                .set("width", self.geometry(geometry.display_size.width))
                .set("height", self.geometry(geometry.display_size.height));
        }
        if let Some((w, h)) = geometry.native_size_px {
            attrs.set("native-size", format!("{w}x{h}"));
        }
        if let Some(dpi) = geometry.dpi.filter(|d| *d != 96) {
            attrs.set("dpi", dpi.to_string());
        }
        self.anchor_attrs(&mut attrs, &geometry.anchor);

        if geometry.rotation_deg != 0.0 {
            attrs.set("rotation", number(geometry.rotation_deg));
        }
        if geometry.flip != Flip::None {
            attrs.set("flip", geometry.flip.as_str());
        }
        if let Some(crop) = geometry.crop.filter(|c| !c.is_empty()) {
            attrs.set(
                "crop",
                format!(
                    "{},{},{},{}",
                    percent(crop.left),
                    percent(crop.top),
                    percent(crop.right),
                    percent(crop.bottom)
                ),
            );
        }
        if let Some(border) = &geometry.border {
            attrs.set(
                "border",
                format!("{} {} {}", pt(border.width), border.style, border.color),
            );
        }
        if let Some(z) = geometry.z_index {
            attrs.set("z-index", z.to_string());
        }

        attrs.set_opt("name", image.name.clone());
        attrs.set_opt("title", image.title.clone());
        attrs.set_opt("link", image.link.clone());
        attrs.set_opt("external-src", image.external_src.clone());
        if let Some(raw) = &image.effects_raw {
            attrs.set("effects-raw", raw.as_str());
        }
        // A hint for viewers: these are preserved but not displayable.
        if self
            .assets
            .info(&image.asset)
            .is_some_and(|i| matches!(i.content_type.as_str(), "image/x-emf" | "image/x-wmf"))
        {
            attrs.set("render", "unsupported");
        }

        (
            format!("![{alt}]({path}){}", attrs.render_with(self.dict)),
            id,
        )
    }

    fn anchor_attrs(&self, attrs: &mut Attrs, anchor: &Anchor) {
        match anchor {
            Anchor::Inline => {}
            Anchor::Floating {
                relative_to_h,
                relative_to_v,
                position,
                wrap,
                wrap_side,
                behind_text,
            } => {
                attrs.set("anchor", if *behind_text { "behind" } else { "floating" });
                attrs.set("relative-to", relative_to_h.as_str());
                if relative_to_v != relative_to_h {
                    attrs.set("relative-to-v", relative_to_v.as_str());
                }
                match position.h {
                    AxisPos::Offset(x) => attrs.set("x", self.geometry(x)),
                    AxisPos::Align(keyword) => attrs.set("align-h", keyword.as_str()),
                };
                match position.v {
                    AxisPos::Offset(y) => attrs.set("y", self.geometry(y)),
                    AxisPos::Align(keyword) => attrs.set("align-v", keyword.as_str()),
                };
                attrs.set("wrap", wrap.as_str());
                if *wrap_side != docsai_model::image::WrapSide::Both {
                    attrs.set("wrap-side", wrap_side.as_str());
                }
            }
            Anchor::SheetTwoCell {
                from,
                to,
                move_with_cells,
                size_with_cells,
            } => {
                attrs
                    .set("anchor", "two-cell")
                    .set("from", from.cell.a1())
                    .set(
                        "from-offset",
                        offset(from.offset_x, from.offset_y, self.precision()),
                    )
                    .set("to", to.cell.a1())
                    .set(
                        "to-offset",
                        offset(to.offset_x, to.offset_y, self.precision()),
                    )
                    .set("move-with-cells", move_with_cells.to_string())
                    .set("size-with-cells", size_with_cells.to_string());
            }
            Anchor::SheetOneCell { from } => {
                attrs
                    .set("anchor", "one-cell")
                    .set("from", from.cell.a1())
                    .set(
                        "from-offset",
                        offset(from.offset_x, from.offset_y, self.precision()),
                    );
            }
            Anchor::SheetAbsolute { pos } => {
                attrs
                    .set("anchor", "absolute")
                    .set("x", self.geometry(pos.x))
                    .set("y", self.geometry(pos.y));
            }
        }
    }
}

/// Removes the link's own character style from its child runs, so that
/// `[texto](url){.Hyperlink}` does not also carry `{.Hyperlink}` inside the
/// label.
/// What a block is, as a warning names it.
pub(crate) fn block_kind(block: &Block) -> &'static str {
    match block {
        Block::Paragraph(_) => "paragraph",
        Block::Heading(_) => "heading",
        Block::List(_) => "list",
        Block::Table(_) => "table",
        Block::Image(_) => "image",
        Block::TextBox(_) => "textbox",
        Block::Raw(_) => "raw",
    }
}

fn strip_redundant_style(content: &[Inline], props: &RunProps) -> Vec<Inline> {
    let Some(style) = &props.style else {
        return content.to_vec();
    };
    content
        .iter()
        .map(|inline| match inline {
            Inline::Styled {
                content: inner,
                props: inner_props,
            } if inner_props.style.as_ref() == Some(style) => {
                let stripped = RunProps {
                    style: None,
                    direct: inner_props.direct.clone(),
                };
                if stripped.is_empty() {
                    // Nothing else to say about the run: unwrap it entirely.
                    Inline::Styled {
                        content: inner.clone(),
                        props: RunProps::default(),
                    }
                } else {
                    Inline::Styled {
                        content: inner.clone(),
                        props: stripped,
                    }
                }
            }
            other => other.clone(),
        })
        .collect()
}

fn offset(x: Length, y: Length, precision: u8) -> String {
    format!("{},{}", geometry(x, precision), geometry(y, precision))
}

fn push_len(attrs: &mut Attrs, key: &str, value: Option<Length>, precision: u8) {
    if let Some(value) = value {
        attrs.set(key, len(value, precision));
    }
}

fn underline_name(underline: Underline) -> &'static str {
    match underline {
        Underline::None => "none",
        Underline::Single => "single",
        Underline::Double => "double",
        Underline::Thick => "thick",
        Underline::Dotted => "dotted",
        Underline::Dashed => "dashed",
        Underline::Wave => "wave",
    }
}

/// A link destination needs `<>` when it holds spaces or parentheses.
fn escape_link_target(target: &str) -> String {
    if target.contains(|c: char| c.is_whitespace() || c == '(' || c == ')') {
        format!("<{}>", target.replace('>', "%3E"))
    } else {
        target.to_string()
    }
}

/// Column widths for a padded GFM table (spec §8: padded below 120 columns).
fn column_widths(grid: &[Vec<String>], columns: usize) -> Vec<usize> {
    const MAX_PADDED_COLUMNS: usize = 120;
    if columns > MAX_PADDED_COLUMNS {
        return vec![0; columns];
    }
    let mut widths = vec![3usize; columns];
    for row in grid {
        for (index, cell) in row.iter().enumerate() {
            if index < columns {
                widths[index] = widths[index].max(cell.chars().count());
            }
        }
    }
    widths
}

fn render_row(row: &[String], widths: &[usize]) -> String {
    let cells: Vec<String> = row
        .iter()
        .zip(widths)
        .map(|(cell, width)| {
            let pad = width.saturating_sub(cell.chars().count());
            format!("{cell}{}", " ".repeat(pad))
        })
        .collect();
    format!("| {} |\n", cells.join(" | "))
}

fn render_delimiter(widths: &[usize]) -> String {
    let cells: Vec<String> = widths.iter().map(|w| "-".repeat((*w).max(3))).collect();
    format!("| {} |\n", cells.join(" | "))
}
