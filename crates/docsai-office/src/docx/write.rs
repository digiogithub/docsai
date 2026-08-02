//! IR → `.docx` writer (Phase 2).

use std::collections::BTreeMap;
use std::io::{Seek, Write};

use docsai_model::assets::AssetStore;
use docsai_model::image::{
    AlignKeyword, Anchor, AxisPos, Flip, ImageRef, RelBase, WrapMode, WrapSide,
};
use docsai_model::list::{ListCatalog, ListDef, ListId, NumFormat};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::{
    Align, FontProps, LineHeight, ParaProps, StyleCatalog, StyleType, Underline, VertAlign,
};
use docsai_model::text::{
    Block, BreakKind, HeaderFooter, HeaderScope, Heading, Inline, List, ListItem, Orientation,
    Paragraph, RawFragment, RunProps, Section, Table, TableCell, TextDocument,
};
use docsai_model::Document;

use crate::package::Package;
use crate::write_error::WriteError;

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const NS_PKG_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const NS_CORE: &str = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_DCTERMS: &str = "http://purl.org/dc/terms/";
const NS_XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
const NS_EP: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";
const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
const NS_CUST: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";

const REL_OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// Writes a text document as a `.docx` package.
pub fn write_docx<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    let text = match document {
        Document::Text(t) => t,
        Document::Workbook(_) => {
            return Err(WriteError::Invalid(
                "cannot write a workbook as .docx".into(),
            ));
        }
    };
    let mut report = ConversionReport::new();
    let package = build_package(text, assets, &mut report)?;
    package.write_to(writer)?;
    Ok(report)
}

struct Rel {
    id: String,
    kind: String,
    target: String,
    external: bool,
}

struct WriterCtx<'a> {
    assets: &'a dyn AssetStore,
    report: &'a mut ConversionReport,
    /// document.xml relationships (styles, numbering, media, headers…).
    doc_rels: Vec<Rel>,
    next_rid: u32,
    /// asset id → (rId, part name)
    media: BTreeMap<String, (String, String)>,
    media_seq: u32,
    /// header/footer parts: part name → xml
    header_parts: BTreeMap<String, String>,
    footer_parts: BTreeMap<String, String>,
    hf_seq: u32,
    /// footnotes: id → body XML already rendered with full block fidelity
    footnotes: Vec<(i64, String)>,
    next_footnote: i64,
    /// list id string → numId
    list_nums: BTreeMap<String, i64>,
    /// drawing unique ids
    drawing_id: u32,
}

impl<'a> WriterCtx<'a> {
    fn new(assets: &'a dyn AssetStore, report: &'a mut ConversionReport) -> Self {
        Self {
            assets,
            report,
            doc_rels: Vec::new(),
            next_rid: 1,
            media: BTreeMap::new(),
            media_seq: 0,
            header_parts: BTreeMap::new(),
            footer_parts: BTreeMap::new(),
            hf_seq: 0,
            footnotes: Vec::new(),
            next_footnote: 1,
            list_nums: BTreeMap::new(),
            drawing_id: 1,
        }
    }

    fn add_rel(&mut self, kind_suffix: &str, target: &str, external: bool) -> String {
        let id = format!("rId{}", self.next_rid);
        self.next_rid += 1;
        self.doc_rels.push(Rel {
            id: id.clone(),
            kind: format!("{REL_OFFICE}/{kind_suffix}"),
            target: target.to_string(),
            external,
        });
        id
    }

    fn ensure_media(&mut self, image: &ImageRef) -> Option<(String, i64, i64)> {
        let asset_key = image.asset.as_str().to_string();
        let (rid, _) = if let Some(entry) = self.media.get(&asset_key) {
            entry.clone()
        } else {
            let _bytes = match self.assets.get(&image.asset) {
                Some(b) => b,
                None => {
                    self.report.warn(Warning::Degraded {
                        what: format!("image {}", image.asset),
                        why: "asset bytes missing".into(),
                    });
                    return None;
                }
            };
            let info = self.assets.info(&image.asset);
            let ext = info
                .map(|i| i.file_name.rsplit('.').next().unwrap_or("bin").to_string())
                .unwrap_or_else(|| "bin".into());
            self.media_seq += 1;
            let part = format!("media/image{}.{}", self.media_seq, ext);
            let rid = self.add_rel("image", &part, false);
            // store bytes later via package — keep mapping of rid→part; bytes via asset id
            self.media
                .insert(asset_key.clone(), (rid.clone(), part.clone()));
            // stash bytes under a side channel: part name is enough; package build pulls assets
            (rid, part)
        };
        let cx = image.geometry.display_size.width.emu().max(1);
        let cy = image.geometry.display_size.height.emu().max(1);
        Some((rid, cx, cy))
    }
}

fn build_package(
    doc: &TextDocument,
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Package, WriteError> {
    let mut ctx = WriterCtx::new(assets, report);

    // Pre-assign list numIds from catalog + any lists without defs
    assign_list_ids(doc, &mut ctx);

    // Always reference styles
    let _styles_rid = ctx.add_rel("styles", "styles.xml", false);
    let has_numbering = !ctx.list_nums.is_empty() || !doc.list_defs.is_empty();
    if has_numbering {
        let _ = ctx.add_rel("numbering", "numbering.xml", false);
    }

    // Build section content first so headers get registered
    let body_xml = write_body(doc, &mut ctx);

    let has_footnotes = !ctx.footnotes.is_empty();
    if has_footnotes {
        let _ = ctx.add_rel("footnotes", "footnotes.xml", false);
    }

    let mut package = Package::new();

    // Content types
    package.insert(
        "[Content_Types].xml",
        content_types_xml(&ctx, has_numbering, has_footnotes),
    );

    // Package relationships
    package.insert("_rels/.rels", package_rels_xml());

    // Core / app / custom props
    package.insert("docProps/core.xml", core_props_xml(&doc.meta));
    package.insert("docProps/app.xml", app_props_xml(&doc.meta));
    if !doc.meta.custom.is_empty() {
        package.insert("docProps/custom.xml", custom_props_xml(&doc.meta));
        // add relationship from package? custom is via app usually via Override only
    }

    // Document
    package.insert("word/document.xml", document_xml(&body_xml));
    package.insert("word/_rels/document.xml.rels", document_rels_xml(&ctx));

    // Styles + numbering
    package.insert("word/styles.xml", styles_xml(&doc.styles));
    if has_numbering {
        package.insert(
            "word/numbering.xml",
            numbering_xml(&doc.list_defs, &ctx.list_nums),
        );
    }

    // Footnotes
    if has_footnotes {
        package.insert("word/footnotes.xml", footnotes_xml(&ctx));
    }

    // Headers / footers
    for (name, xml) in &ctx.header_parts {
        package.insert(format!("word/{name}"), xml.clone());
    }
    for (name, xml) in &ctx.footer_parts {
        package.insert(format!("word/{name}"), xml.clone());
    }

    // Media
    for (asset_id, (_rid, part)) in &ctx.media {
        let id = docsai_model::assets::AssetId::new(asset_id.clone());
        if let Some(bytes) = assets.get(&id) {
            package.insert(format!("word/{part}"), bytes.to_vec());
        }
    }

    Ok(package)
}

fn assign_list_ids(doc: &TextDocument, ctx: &mut WriterCtx<'_>) {
    let mut next = 1i64;
    for id in doc.list_defs.defs.keys() {
        if !ctx.list_nums.contains_key(id.as_str()) {
            // Prefer numeric suffix of L{n}
            let n = id
                .as_str()
                .strip_prefix('L')
                .and_then(|s| s.parse().ok())
                .unwrap_or(next);
            ctx.list_nums.insert(id.as_str().to_string(), n);
            next = next.max(n + 1);
        }
    }
    // Walk lists that may lack catalog entries
    for section in &doc.sections {
        walk_blocks_for_lists(&section.blocks, ctx, &mut next);
        for h in section.headers.iter().chain(section.footers.iter()) {
            walk_blocks_for_lists(&h.blocks, ctx, &mut next);
        }
    }
}

fn walk_blocks_for_lists(blocks: &[Block], ctx: &mut WriterCtx<'_>, next: &mut i64) {
    for block in blocks {
        match block {
            Block::List(list) => {
                let key = list
                    .def
                    .as_ref()
                    .map(|d| d.as_str().to_string())
                    .unwrap_or_else(|| {
                        if list.ordered {
                            format!("__auto_ol_{}", list.level)
                        } else {
                            format!("__auto_ul_{}", list.level)
                        }
                    });
                if let std::collections::btree_map::Entry::Vacant(e) = ctx.list_nums.entry(key) {
                    e.insert(*next);
                    *next += 1;
                }
                for item in &list.items {
                    walk_blocks_for_lists(&item.blocks, ctx, next);
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        walk_blocks_for_lists(&cell.blocks, ctx, next);
                    }
                }
            }
            Block::TextBox(tb) => walk_blocks_for_lists(&tb.blocks, ctx, next),
            _ => {}
        }
    }
}

fn write_body(doc: &TextDocument, ctx: &mut WriterCtx<'_>) -> String {
    let mut out = String::new();
    let sections = if doc.sections.is_empty() {
        vec![Section::default()]
    } else {
        doc.sections.clone()
    };

    for (idx, section) in sections.iter().enumerate() {
        let last = idx + 1 == sections.len();
        for block in &section.blocks {
            write_block(block, ctx, &mut out, None);
        }
        // sectPr: for non-last sections it sits inside the last paragraph's pPr;
        // for the last section it trails the body. We always emit trailing sectPr
        // for the last section, and for earlier ones append an empty paragraph
        // carrying sectPr (Word accepts both).
        let sect = sect_pr_xml(section, ctx);
        if last {
            out.push_str(&sect);
        } else {
            out.push_str("<w:p><w:pPr>");
            // strip outer <w:sectPr>..</w:sectPr> already is sect
            out.push_str(&sect);
            out.push_str("</w:pPr></w:p>");
        }
    }
    out
}

fn write_block(
    block: &Block,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
    list_ctx: Option<(&str, u8)>,
) {
    match block {
        Block::Paragraph(p) => write_paragraph(p, ctx, out, list_ctx, None),
        Block::Heading(h) => write_heading(h, ctx, out),
        Block::List(list) => write_list(list, ctx, out),
        Block::Table(table) => write_table(table, ctx, out),
        Block::Image(img) => {
            // Block image as a paragraph containing the drawing
            out.push_str("<w:p>");
            write_image_run(img, ctx, out);
            out.push_str("</w:p>");
        }
        Block::TextBox(tb) => {
            ctx.report.warn(Warning::Degraded {
                what: "text-box".into(),
                why: "text boxes are flattened to paragraphs on write".into(),
            });
            for b in &tb.blocks {
                write_block(b, ctx, out, None);
            }
        }
        Block::Raw(raw) => write_raw_block(raw, ctx, out),
    }
}

fn write_heading(h: &Heading, ctx: &mut WriterCtx<'_>, out: &mut String) {
    let style = h
        .paragraph
        .format
        .style
        .as_ref()
        .map(|s| s.as_str().to_string())
        .unwrap_or_else(|| format!("Heading{}", h.level.min(9)));
    write_paragraph(&h.paragraph, ctx, out, None, Some(&style));
}

fn write_list(list: &List, ctx: &mut WriterCtx<'_>, out: &mut String) {
    let key = list
        .def
        .as_ref()
        .map(|d| d.as_str().to_string())
        .unwrap_or_else(|| {
            if list.ordered {
                format!("__auto_ol_{}", list.level)
            } else {
                format!("__auto_ul_{}", list.level)
            }
        });
    // Ensure num id
    if !ctx.list_nums.contains_key(&key) {
        let n = ctx.list_nums.values().copied().max().unwrap_or(0) + 1;
        ctx.list_nums.insert(key.clone(), n);
    }
    for item in &list.items {
        write_list_item(item, &key, list.level, ctx, out);
    }
}

fn write_list_item(
    item: &ListItem,
    list_key: &str,
    level: u8,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) {
    let mut first = true;
    for block in &item.blocks {
        match block {
            Block::List(nested) => write_list(nested, ctx, out),
            Block::Paragraph(p) if first => {
                write_paragraph(p, ctx, out, Some((list_key, level)), None);
                first = false;
            }
            other => {
                if first {
                    // list item starting with non-paragraph: empty marker para
                    let empty = Paragraph::default();
                    write_paragraph(&empty, ctx, out, Some((list_key, level)), None);
                    first = false;
                }
                write_block(other, ctx, out, None);
            }
        }
    }
    if first {
        let empty = Paragraph::default();
        write_paragraph(&empty, ctx, out, Some((list_key, level)), None);
    }
}

fn write_paragraph(
    p: &Paragraph,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
    list_ctx: Option<(&str, u8)>,
    style_override: Option<&str>,
) {
    out.push_str("<w:p>");
    let style = style_override
        .map(str::to_string)
        .or_else(|| p.format.style.as_ref().map(|s| s.as_str().to_string()));
    let needs_ppr = style.is_some()
        || !p.format.direct.is_empty()
        || !p.format.run_direct.is_empty()
        || list_ctx.is_some();
    if needs_ppr {
        out.push_str("<w:pPr>");
        if let Some(s) = &style {
            out.push_str(&format!(r#"<w:pStyle w:val="{}"/>"#, esc_attr(s)));
        }
        if let Some((key, level)) = list_ctx {
            if let Some(&num_id) = ctx.list_nums.get(key) {
                out.push_str(&format!(
                    "<w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{num_id}\"/></w:numPr>"
                ));
            }
        }
        write_para_props(&p.format.direct, out);
        if !p.format.run_direct.is_empty() {
            out.push_str("<w:rPr>");
            write_font_props(&p.format.run_direct, out);
            out.push_str("</w:rPr>");
        }
        out.push_str("</w:pPr>");
    }
    write_inlines(&p.content, ctx, out, &RunProps::default());
    out.push_str("</w:p>");
}

fn write_inlines(inlines: &[Inline], ctx: &mut WriterCtx<'_>, out: &mut String, base: &RunProps) {
    for inline in inlines {
        match inline {
            Inline::Text(t) => write_text_run(t, base, out),
            Inline::Styled { content, props } => {
                let merged = merge_run(base, props);
                write_inlines(content, ctx, out, &merged);
            }
            Inline::Link {
                target,
                content,
                props,
            } => {
                // Simple hyperlink
                let rid = ctx.add_rel("hyperlink", target, true);
                let merged = merge_run(base, props);
                out.push_str(&format!(r#"<w:hyperlink r:id="{}">"#, esc_attr(&rid)));
                write_inlines(content, ctx, out, &merged);
                out.push_str("</w:hyperlink>");
            }
            Inline::Footnote(blocks) => {
                let id = ctx.next_footnote;
                ctx.next_footnote += 1;
                let mut body = String::new();
                for block in blocks {
                    write_block(block, ctx, &mut body, None);
                }
                if body.is_empty() {
                    body.push_str("<w:p/>");
                }
                ctx.footnotes.push((id, body));
                out.push_str("<w:r><w:rPr><w:rStyle w:val=\"FootnoteReference\"/></w:rPr>");
                out.push_str(&format!(r#"<w:footnoteReference w:id="{id}"/>"#));
                out.push_str("</w:r>");
            }
            Inline::Field {
                kind,
                cached,
                instruction,
            } => {
                let instr = if instruction.is_empty() {
                    format!(" {} ", kind.as_str())
                } else {
                    instruction.clone()
                };
                out.push_str(r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#);
                out.push_str("<w:r><w:instrText xml:space=\"preserve\">");
                out.push_str(&esc_text(&instr));
                out.push_str("</w:instrText></w:r>");
                out.push_str(r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#);
                write_text_run(cached, base, out);
                out.push_str(r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#);
                let _ = kind;
            }
            Inline::Break(kind) => {
                let ty = match kind {
                    BreakKind::Line => None,
                    BreakKind::Page => Some("page"),
                    BreakKind::Column => Some("column"),
                };
                out.push_str("<w:r>");
                match ty {
                    Some(t) => out.push_str(&format!(r#"<w:br w:type="{t}"/>"#)),
                    None => out.push_str("<w:br/>"),
                }
                out.push_str("</w:r>");
            }
            Inline::Image(img) => write_image_run(img, ctx, out),
            Inline::Raw(raw) => {
                if raw.format == "ooxml" && looks_like_run_xml(&raw.content) {
                    out.push_str(&raw.content);
                } else {
                    ctx.report.warn(Warning::Degraded {
                        what: format!("raw {}", raw.id.0),
                        why: "inline raw fragment not re-injected".into(),
                    });
                }
            }
        }
    }
}

fn merge_run(base: &RunProps, over: &RunProps) -> RunProps {
    RunProps {
        style: over.style.clone().or_else(|| base.style.clone()),
        direct: over.direct.over(&base.direct),
    }
}

fn write_text_run(text: &str, props: &RunProps, out: &mut String) {
    if text.is_empty() {
        return;
    }
    // Split on newlines → soft breaks
    let parts: Vec<&str> = text.split('\n').collect();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push_str("<w:r>");
            write_rpr(props, out);
            out.push_str("<w:br/></w:r>");
        }
        if part.is_empty() {
            continue;
        }
        out.push_str("<w:r>");
        write_rpr(props, out);
        // Preserve spaces
        let preserve = part.starts_with(' ') || part.ends_with(' ') || part.contains("  ");
        if preserve {
            out.push_str(r#"<w:t xml:space="preserve">"#);
        } else {
            out.push_str("<w:t>");
        }
        out.push_str(&esc_text(part));
        out.push_str("</w:t></w:r>");
    }
}

fn write_rpr(props: &RunProps, out: &mut String) {
    if props.is_empty() {
        return;
    }
    out.push_str("<w:rPr>");
    if let Some(s) = &props.style {
        out.push_str(&format!(r#"<w:rStyle w:val="{}"/>"#, esc_attr(s.as_str())));
    }
    write_font_props(&props.direct, out);
    out.push_str("</w:rPr>");
}

fn write_font_props(font: &FontProps, out: &mut String) {
    if let Some(name) = &font.name {
        out.push_str(&format!(
            r#"<w:rFonts w:ascii="{0}" w:hAnsi="{0}" w:cs="{0}"/>"#,
            esc_attr(name)
        ));
    }
    if let Some(size) = font.size {
        let hp = size.half_points().max(1);
        out.push_str(&format!(r#"<w:sz w:val="{hp}"/><w:szCs w:val="{hp}"/>"#));
    }
    if font.bold == Some(true) {
        out.push_str("<w:b/><w:bCs/>");
    } else if font.bold == Some(false) {
        out.push_str(r#"<w:b w:val="0"/><w:bCs w:val="0"/>"#);
    }
    if font.italic == Some(true) {
        out.push_str("<w:i/><w:iCs/>");
    } else if font.italic == Some(false) {
        out.push_str(r#"<w:i w:val="0"/><w:iCs w:val="0"/>"#);
    }
    if font.strike == Some(true) {
        out.push_str("<w:strike/>");
    }
    if let Some(u) = font.underline {
        let val = match u {
            Underline::None => "none",
            Underline::Single => "single",
            Underline::Double => "double",
            Underline::Dotted => "dotted",
            Underline::Dashed => "dash",
            Underline::Wave => "wave",
            Underline::Thick => "thick",
        };
        out.push_str(&format!(r#"<w:u w:val="{val}"/>"#));
    }
    if let Some(color) = &font.color {
        let hex = color.trim_start_matches('#');
        out.push_str(&format!(r#"<w:color w:val="{}"/>"#, esc_attr(hex)));
    }
    if let Some(hl) = &font.highlight {
        out.push_str(&format!(r#"<w:highlight w:val="{}"/>"#, esc_attr(hl)));
    }
    if let Some(va) = font.vert_align {
        let val = match va {
            VertAlign::Baseline => "baseline",
            VertAlign::Superscript => "superscript",
            VertAlign::Subscript => "subscript",
        };
        out.push_str(&format!(r#"<w:vertAlign w:val="{val}"/>"#));
    }
    if font.small_caps == Some(true) {
        out.push_str("<w:smallCaps/>");
    }
    if font.caps == Some(true) {
        out.push_str("<w:caps/>");
    }
}

fn write_para_props(p: &ParaProps, out: &mut String) {
    if let Some(a) = p.align {
        let val = match a {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "both",
        };
        out.push_str(&format!(r#"<w:jc w:val="{val}"/>"#));
    }
    let has_ind = p.indent_left.is_some()
        || p.indent_right.is_some()
        || p.indent_first_line.is_some()
        || p.indent_hanging.is_some();
    if has_ind {
        out.push_str("<w:ind");
        if let Some(v) = p.indent_left {
            out.push_str(&format!(r#" w:left="{}""#, v.twips()));
        }
        if let Some(v) = p.indent_right {
            out.push_str(&format!(r#" w:right="{}""#, v.twips()));
        }
        if let Some(v) = p.indent_first_line {
            out.push_str(&format!(r#" w:firstLine="{}""#, v.twips()));
        }
        if let Some(v) = p.indent_hanging {
            out.push_str(&format!(r#" w:hanging="{}""#, v.twips()));
        }
        out.push_str("/>");
    }
    let has_sp = p.space_before.is_some() || p.space_after.is_some() || p.line_height.is_some();
    if has_sp {
        out.push_str("<w:spacing");
        if let Some(v) = p.space_before {
            out.push_str(&format!(r#" w:before="{}""#, v.twips()));
        }
        if let Some(v) = p.space_after {
            out.push_str(&format!(r#" w:after="{}""#, v.twips()));
        }
        if let Some(lh) = p.line_height {
            match lh {
                LineHeight::Multiple(thousandths) => {
                    // IR stores thousandths of a line; OOXML uses 240ths.
                    let ooxml = ((i64::from(thousandths) * 240 + 500) / 1000).max(1);
                    out.push_str(&format!(r#" w:line="{ooxml}" w:lineRule="auto""#));
                }
                LineHeight::Exact(l) => {
                    out.push_str(&format!(r#" w:line="{}" w:lineRule="exact""#, l.twips()));
                }
                LineHeight::AtLeast(l) => {
                    out.push_str(&format!(r#" w:line="{}" w:lineRule="atLeast""#, l.twips()));
                }
            }
        }
        out.push_str("/>");
    }
    if p.keep_with_next == Some(true) {
        out.push_str("<w:keepNext/>");
    }
    if p.page_break_before == Some(true) {
        out.push_str("<w:pageBreakBefore/>");
    }
    if let Some(bg) = &p.background {
        let hex = bg.trim_start_matches('#');
        out.push_str(&format!(
            r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
            esc_attr(hex)
        ));
    }
    if let Some(lvl) = p.outline_level {
        out.push_str(&format!(r#"<w:outlineLvl w:val="{lvl}"/>"#));
    }
}

fn write_image_run(img: &ImageRef, ctx: &mut WriterCtx<'_>, out: &mut String) {
    let Some((rid, cx, cy)) = ctx.ensure_media(img) else {
        return;
    };
    let doc_pr_id = ctx.drawing_id;
    ctx.drawing_id += 1;
    let name = img
        .name
        .clone()
        .unwrap_or_else(|| format!("Picture {doc_pr_id}"));
    let name_attr = esc_attr(&name);
    let alt = esc_attr(&img.alt);
    let title_attr = img
        .title
        .as_ref()
        .map(|t| format!(r#" title="{}""#, esc_attr(t)))
        .unwrap_or_default();

    let mut hlink = String::new();
    if let Some(link) = &img.link {
        let hrid = ctx.add_rel("hyperlink", link, true);
        hlink = format!(
            r#"<a:hlinkClick xmlns:a="{NS_A}" r:id="{}"/>"#,
            esc_attr(&hrid)
        );
    }

    let pic_xml = picture_xml(img, &rid, cx, cy, &name_attr);
    let doc_pr = format!(
        r#"<wp:docPr id="{doc_pr_id}" name="{name_attr}" descr="{alt}"{title_attr}>{hlink}</wp:docPr>"#
    );
    let frame_locks = format!(
        r#"<wp:cNvGraphicFramePr>
    <a:graphicFrameLocks xmlns:a="{NS_A}" noChangeAspect="1"/>
  </wp:cNvGraphicFramePr>"#
    );
    let graphic = format!(
        r#"<a:graphic xmlns:a="{NS_A}">
    <a:graphicData uri="{NS_PIC}">
{pic_xml}
    </a:graphicData>
  </a:graphic>"#
    );

    out.push_str("<w:r><w:drawing>");
    match &img.geometry.anchor {
        Anchor::Floating {
            relative_to_h,
            relative_to_v,
            position,
            wrap,
            wrap_side,
            behind_text,
        } => {
            let z = img.geometry.z_index.unwrap_or(0).max(0) as u32;
            let behind = if *behind_text { "1" } else { "0" };
            out.push_str(&format!(
                r#"<wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="{z}" behindDoc="{behind}" locked="0" layoutInCell="1" allowOverlap="1">
  <wp:simplePos x="0" y="0"/>
  {}
  {}
  <wp:extent cx="{cx}" cy="{cy}"/>
  <wp:effectExtent l="0" t="0" r="0" b="0"/>
  {}
  {doc_pr}
  {frame_locks}
  {graphic}
</wp:anchor>"#,
                position_axis_xml("positionH", *relative_to_h, position.h),
                position_axis_xml("positionV", *relative_to_v, position.v),
                wrap_xml(*wrap, *wrap_side),
            ));
        }
        Anchor::Inline => {
            out.push_str(&format!(
                r#"<wp:inline distT="0" distB="0" distL="0" distR="0">
  <wp:extent cx="{cx}" cy="{cy}"/>
  {doc_pr}
  {frame_locks}
  {graphic}
</wp:inline>"#
            ));
        }
        other => {
            // Sheet anchors do not belong in a .docx; fall back to inline.
            ctx.report.warn(Warning::Degraded {
                what: format!("image {}", img.asset),
                why: format!(
                    "anchor {} is not valid in docx; written as inline",
                    other.keyword()
                ),
            });
            out.push_str(&format!(
                r#"<wp:inline distT="0" distB="0" distL="0" distR="0">
  <wp:extent cx="{cx}" cy="{cy}"/>
  {doc_pr}
  {frame_locks}
  {graphic}
</wp:inline>"#
            ));
        }
    }
    out.push_str("</w:drawing></w:r>");
}

fn position_axis_xml(tag: &str, base: RelBase, pos: AxisPos) -> String {
    let relative = rel_base_ooxml(base);
    match pos {
        AxisPos::Offset(len) => format!(
            r#"<wp:{tag} relativeFrom="{relative}"><wp:posOffset>{}</wp:posOffset></wp:{tag}>"#,
            len.emu()
        ),
        AxisPos::Align(keyword) => format!(
            r#"<wp:{tag} relativeFrom="{relative}"><wp:align>{}</wp:align></wp:{tag}>"#,
            align_keyword_ooxml(keyword)
        ),
    }
}

fn rel_base_ooxml(base: RelBase) -> &'static str {
    match base {
        RelBase::Page => "page",
        RelBase::Margin => "margin",
        RelBase::Paragraph => "paragraph",
        RelBase::Character => "character",
        RelBase::Line => "line",
        RelBase::Column => "column",
    }
}

fn align_keyword_ooxml(keyword: AlignKeyword) -> &'static str {
    match keyword {
        AlignKeyword::Left => "left",
        AlignKeyword::Center => "center",
        AlignKeyword::Right => "right",
        AlignKeyword::Inside => "inside",
        AlignKeyword::Outside => "outside",
        AlignKeyword::Top => "top",
        AlignKeyword::Middle => "center",
        AlignKeyword::Bottom => "bottom",
    }
}

fn wrap_xml(mode: WrapMode, side: WrapSide) -> String {
    let wrap_text = match side {
        WrapSide::Both => "bothSides",
        WrapSide::Left => "left",
        WrapSide::Right => "right",
        WrapSide::Largest => "largest",
    };
    match mode {
        WrapMode::Square => format!(r#"<wp:wrapSquare wrapText="{wrap_text}"/>"#),
        WrapMode::Tight => format!(r#"<wp:wrapTight wrapText="{wrap_text}"/>"#),
        WrapMode::Through => format!(r#"<wp:wrapThrough wrapText="{wrap_text}"/>"#),
        WrapMode::TopBottom => "<wp:wrapTopAndBottom/>".into(),
        WrapMode::None => "<wp:wrapNone/>".into(),
    }
}

fn picture_xml(img: &ImageRef, rid: &str, cx: i64, cy: i64, name_attr: &str) -> String {
    let rot = if img.geometry.rotation_deg != 0.0 {
        let units = (img.geometry.rotation_deg * 60_000.0).round() as i64;
        format!(r#" rot="{units}""#)
    } else {
        String::new()
    };
    let (flip_h, flip_v) = match img.geometry.flip {
        Flip::None => ("", ""),
        Flip::H => (r#" flipH="1""#, ""),
        Flip::V => ("", r#" flipV="1""#),
        Flip::HV => (r#" flipH="1""#, r#" flipV="1""#),
    };

    let src_rect = img
        .geometry
        .crop
        .filter(|c| !c.is_empty())
        .map(|c| {
            let pct = |v: f32| (v * 1000.0).round() as i64;
            format!(
                r#"<a:srcRect l="{}" t="{}" r="{}" b="{}"/>"#,
                pct(c.left),
                pct(c.top),
                pct(c.right),
                pct(c.bottom)
            )
        })
        .unwrap_or_default();

    let border = img
        .geometry
        .border
        .as_ref()
        .map(|b| {
            let hex = b.color.trim_start_matches('#');
            let dash = match b.style.as_str() {
                "dashed" | "dash" => "dash",
                "dotted" | "dot" => "sysDot",
                "double" => "lgDash",
                _ => "solid",
            };
            format!(
                r#"<a:ln w="{}"><a:solidFill><a:srgbClr val="{}"/></a:solidFill><a:prstDash val="{dash}"/></a:ln>"#,
                b.width.emu().max(1),
                esc_attr(hex),
            )
        })
        .unwrap_or_default();

    format!(
        r#"      <pic:pic xmlns:pic="{NS_PIC}">
        <pic:nvPicPr>
          <pic:cNvPr id="0" name="{name_attr}"/>
          <pic:cNvPicPr/>
        </pic:nvPicPr>
        <pic:blipFill>
          <a:blip r:embed="{}"/>
          {src_rect}
          <a:stretch><a:fillRect/></a:stretch>
        </pic:blipFill>
        <pic:spPr>
          <a:xfrm{rot}{flip_h}{flip_v}>
            <a:off x="0" y="0"/>
            <a:ext cx="{cx}" cy="{cy}"/>
          </a:xfrm>
          <a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
          {border}
        </pic:spPr>
      </pic:pic>"#,
        esc_attr(rid),
    )
}

fn write_table(table: &Table, ctx: &mut WriterCtx<'_>, out: &mut String) {
    out.push_str("<w:tbl>");
    out.push_str("<w:tblPr>");
    if let Some(s) = &table.style {
        out.push_str(&format!(
            r#"<w:tblStyle w:val="{}"/>"#,
            esc_attr(s.as_str())
        ));
    }
    out.push_str(r#"<w:tblW w:w="0" w:type="auto"/>"#);
    out.push_str("</w:tblPr>");
    if !table.col_widths.is_empty() {
        out.push_str("<w:tblGrid>");
        for w in &table.col_widths {
            out.push_str(&format!(r#"<w:gridCol w:w="{}"/>"#, w.twips().max(1)));
        }
        out.push_str("</w:tblGrid>");
    }
    // Track vertical merges: col → remaining rowspan. Covered cells in the IR
    // mark continuations; we still emit `w:vMerge` without restart.
    let ncols = table.width().max(1);
    let mut vmerge_remain = vec![0u16; ncols];
    for row in &table.rows {
        out.push_str("<w:tr>");
        if row.is_header || table.header_row {
            out.push_str("<w:trPr><w:tblHeader/></w:trPr>");
        }
        let mut col = 0usize;
        for cell in &row.cells {
            if cell.covered {
                out.push_str(r#"<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>"#);
                if col < ncols && vmerge_remain[col] > 0 {
                    vmerge_remain[col] -= 1;
                }
                col += 1;
                continue;
            }
            while col < ncols && vmerge_remain[col] > 0 {
                out.push_str(r#"<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>"#);
                vmerge_remain[col] -= 1;
                col += 1;
            }
            write_cell(cell, ctx, out);
            let span = cell.colspan.max(1) as usize;
            if cell.rowspan > 1 {
                for slot in vmerge_remain
                    .iter_mut()
                    .skip(col)
                    .take(span.min(ncols.saturating_sub(col)))
                {
                    *slot = cell.rowspan - 1;
                }
            }
            col += span;
        }
        while col < ncols && vmerge_remain[col] > 0 {
            out.push_str(r#"<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>"#);
            vmerge_remain[col] -= 1;
            col += 1;
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
}

fn write_cell(cell: &TableCell, ctx: &mut WriterCtx<'_>, out: &mut String) {
    out.push_str("<w:tc>");
    out.push_str("<w:tcPr>");
    if let Some(w) = cell.width {
        out.push_str(&format!(
            r#"<w:tcW w:w="{}" w:type="dxa"/>"#,
            w.twips().max(1)
        ));
    }
    if cell.colspan > 1 {
        out.push_str(&format!(r#"<w:gridSpan w:val="{}"/>"#, cell.colspan));
    }
    if cell.rowspan > 1 {
        out.push_str(r#"<w:vMerge w:val="restart"/>"#);
    }
    if let Some(bg) = &cell.background {
        let hex = bg.trim_start_matches('#');
        out.push_str(&format!(
            r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
            esc_attr(hex)
        ));
    }
    out.push_str("</w:tcPr>");
    if cell.blocks.is_empty() {
        out.push_str("<w:p/>");
    } else {
        for b in &cell.blocks {
            write_block(b, ctx, out, None);
        }
    }
    out.push_str("</w:tc>");
}

fn write_raw_block(raw: &RawFragment, ctx: &mut WriterCtx<'_>, out: &mut String) {
    if raw.format == "ooxml" {
        // Re-inject if it looks like a block-level element
        let trimmed = raw.content.trim();
        if trimmed.starts_with('<') {
            out.push_str(trimmed);
            return;
        }
    }
    ctx.report.warn(Warning::Degraded {
        what: format!("raw {}", raw.id.0),
        why: format!("raw-block format={} not re-injected", raw.format),
    });
}

fn sect_pr_xml(section: &Section, ctx: &mut WriterCtx<'_>) -> String {
    let mut s = String::from("<w:sectPr>");
    // headers
    for h in &section.headers {
        let (part, typ) = register_hf(h, true, ctx);
        s.push_str(&format!(
            r#"<w:headerReference w:type="{}" r:id="{}"/>"#,
            typ,
            esc_attr(&part)
        ));
    }
    for f in &section.footers {
        let (part, typ) = register_hf(f, false, ctx);
        s.push_str(&format!(
            r#"<w:footerReference w:type="{}" r:id="{}"/>"#,
            typ,
            esc_attr(&part)
        ));
    }
    let page = &section.page;
    if !page.size.width.is_zero() && !page.size.height.is_zero() {
        let mut orient = "";
        if page.orientation == Orientation::Landscape {
            orient = r#" w:orient="landscape""#;
        }
        s.push_str(&format!(
            r#"<w:pgSz w:w="{}" w:h="{}"{orient}/>"#,
            page.size.width.twips().max(1),
            page.size.height.twips().max(1),
        ));
    }
    let m = &page.margins;
    if !(m.top.is_zero()
        && m.right.is_zero()
        && m.bottom.is_zero()
        && m.left.is_zero()
        && m.header.is_zero()
        && m.footer.is_zero())
    {
        s.push_str(&format!(
            r#"<w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" w:header="{}" w:footer="{}" w:gutter="0"/>"#,
            m.top.twips(),
            m.right.twips(),
            m.bottom.twips(),
            m.left.twips(),
            m.header.twips(),
            m.footer.twips(),
        ));
    }
    if page.columns > 1 {
        s.push_str(&format!(r#"<w:cols w:num="{}"/>"#, page.columns));
    }
    if page.title_page {
        s.push_str("<w:titlePg/>");
    }
    s.push_str("</w:sectPr>");
    s
}

fn register_hf(
    hf: &HeaderFooter,
    is_header: bool,
    ctx: &mut WriterCtx<'_>,
) -> (String, &'static str) {
    ctx.hf_seq += 1;
    let n = ctx.hf_seq;
    let file = if is_header {
        format!("header{n}.xml")
    } else {
        format!("footer{n}.xml")
    };
    let kind = if is_header { "header" } else { "footer" };
    let rid = ctx.add_rel(kind, &file, false);

    let mut body = String::new();
    for b in &hf.blocks {
        write_block(b, ctx, &mut body, None);
    }
    if body.is_empty() {
        body.push_str("<w:p/>");
    }
    let root = if is_header { "hdr" } else { "ftr" };
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:{root} xmlns:w="{NS_W}" xmlns:r="{NS_R}" xmlns:wp="{NS_WP}" xmlns:a="{NS_A}" xmlns:pic="{NS_PIC}">
{body}
</w:{root}>"#
    );
    if is_header {
        ctx.header_parts.insert(file, xml);
    } else {
        ctx.footer_parts.insert(file, xml);
    }
    let typ = match hf.scope {
        HeaderScope::Default => "default",
        HeaderScope::First => "first",
        HeaderScope::Even => "even",
    };
    (rid, typ)
}

fn document_xml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{NS_W}" xmlns:r="{NS_R}" xmlns:wp="{NS_WP}" xmlns:a="{NS_A}" xmlns:pic="{NS_PIC}">
  <w:body>
{body}
  </w:body>
</w:document>"#
    )
}

fn document_rels_xml(ctx: &WriterCtx<'_>) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_REL}">"#
    );
    for rel in &ctx.doc_rels {
        if rel.external {
            s.push_str(&format!(
                r#"<Relationship Id="{}" Type="{}" Target="{}" TargetMode="External"/>"#,
                esc_attr(&rel.id),
                esc_attr(&rel.kind),
                esc_attr(&rel.target),
            ));
        } else {
            s.push_str(&format!(
                r#"<Relationship Id="{}" Type="{}" Target="{}"/>"#,
                esc_attr(&rel.id),
                esc_attr(&rel.kind),
                esc_attr(&rel.target),
            ));
        }
    }
    s.push_str("</Relationships>");
    s
}

fn package_rels_xml() -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{NS_PKG_REL}">
  <Relationship Id="rId1" Type="{REL_PKG}/metadata/core-properties" Target="docProps/core.xml"/>
  <Relationship Id="rId2" Type="{REL_OFFICE}/extended-properties" Target="docProps/app.xml"/>
  <Relationship Id="rId3" Type="{REL_OFFICE}/officeDocument" Target="word/document.xml"/>
</Relationships>"#
    )
}

fn content_types_xml(ctx: &WriterCtx<'_>, numbering: bool, footnotes: bool) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="{NS_CT}">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Default Extension="png" ContentType="image/png"/>
  <Default Extension="jpg" ContentType="image/jpeg"/>
  <Default Extension="jpeg" ContentType="image/jpeg"/>
  <Default Extension="gif" ContentType="image/gif"/>
  <Default Extension="bmp" ContentType="image/bmp"/>
  <Default Extension="tif" ContentType="image/tiff"/>
  <Default Extension="tiff" ContentType="image/tiff"/>
  <Default Extension="emf" ContentType="image/x-emf"/>
  <Default Extension="wmf" ContentType="image/x-wmf"/>
  <Default Extension="bin" ContentType="application/octet-stream"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
  <Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
  <Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>
  <Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#
    );
    if numbering {
        s.push_str(r#"
  <Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>"#);
    }
    if footnotes {
        s.push_str(r#"
  <Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/>"#);
    }
    for name in ctx.header_parts.keys() {
        s.push_str(&format!(
            r#"
  <Override PartName="/word/{name}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>"#
        ));
    }
    for name in ctx.footer_parts.keys() {
        s.push_str(&format!(
            r#"
  <Override PartName="/word/{name}" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>"#
        ));
    }
    s.push_str("\n</Types>");
    s
}

fn core_props_xml(meta: &docsai_model::DocumentMeta) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="{NS_CORE}" xmlns:dc="{NS_DC}" xmlns:dcterms="{NS_DCTERMS}" xmlns:xsi="{NS_XSI}">"#
    );
    if let Some(t) = &meta.title {
        s.push_str(&format!("<dc:title>{}</dc:title>", esc_text(t)));
    }
    if let Some(a) = &meta.author {
        s.push_str(&format!("<dc:creator>{}</dc:creator>", esc_text(a)));
    }
    if let Some(v) = &meta.last_modified_by {
        s.push_str(&format!(
            "<cp:lastModifiedBy>{}</cp:lastModifiedBy>",
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.subject {
        s.push_str(&format!("<dc:subject>{}</dc:subject>", esc_text(v)));
    }
    if let Some(v) = &meta.keywords {
        s.push_str(&format!("<cp:keywords>{}</cp:keywords>", esc_text(v)));
    }
    if let Some(v) = &meta.description {
        s.push_str(&format!("<dc:description>{}</dc:description>", esc_text(v)));
    }
    if let Some(v) = &meta.language {
        s.push_str(&format!("<dc:language>{}</dc:language>", esc_text(v)));
    }
    if let Some(v) = &meta.created {
        s.push_str(&format!(
            r#"<dcterms:created xsi:type="dcterms:W3CDTF">{}</dcterms:created>"#,
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.modified {
        s.push_str(&format!(
            r#"<dcterms:modified xsi:type="dcterms:W3CDTF">{}</dcterms:modified>"#,
            esc_text(v)
        ));
    }
    s.push_str("</cp:coreProperties>");
    s
}

fn app_props_xml(meta: &docsai_model::DocumentMeta) -> String {
    let app = meta.application.clone().unwrap_or_else(|| "docsai".into());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="{NS_EP}" xmlns:vt="{NS_VT}">
  <Application>{}</Application>
</Properties>"#,
        esc_text(&app)
    )
}

fn custom_props_xml(meta: &docsai_model::DocumentMeta) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="{NS_CUST}" xmlns:vt="{NS_VT}">"#
    );
    for (i, (k, v)) in meta.custom.iter().enumerate() {
        s.push_str(&format!(
            r#"<property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="{}" name="{}"><vt:lpwstr>{}</vt:lpwstr></property>"#,
            i + 2,
            esc_attr(k),
            esc_text(v)
        ));
    }
    s.push_str("</Properties>");
    s
}

fn styles_xml(styles: &StyleCatalog) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{NS_W}">
  <w:docDefaults>
    <w:rPrDefault><w:rPr>"#
    );
    write_font_props(&styles.defaults.font, &mut s);
    s.push_str("</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>");
    write_para_props(&styles.defaults.paragraph, &mut s);
    s.push_str("</w:pPr></w:pPrDefault></w:docDefaults>");

    // Always ensure Normal exists
    let has_normal = styles.styles.values().any(|st| st.id.as_str() == "Normal");
    if !has_normal {
        s.push_str(
            r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#,
        );
    }
    // Heading styles are emitted only when present in the catalogue.

    for style in styles.styles.values() {
        let ty = match style.style_type {
            StyleType::Paragraph => "paragraph",
            StyleType::Character => "character",
            StyleType::Table => "table",
            StyleType::Numbering => "numbering",
        };
        s.push_str(&format!(
            r#"<w:style w:type="{ty}" w:styleId="{}""#,
            esc_attr(style.id.as_str())
        ));
        if style.is_default {
            s.push_str(r#" w:default="1""#);
        }
        s.push('>');
        s.push_str(&format!(r#"<w:name w:val="{}"/>"#, esc_attr(&style.name)));
        if let Some(b) = &style.based_on {
            s.push_str(&format!(r#"<w:basedOn w:val="{}"/>"#, esc_attr(b.as_str())));
        }
        if let Some(n) = &style.next {
            s.push_str(&format!(r#"<w:next w:val="{}"/>"#, esc_attr(n.as_str())));
        }
        if !style.paragraph.is_empty() {
            s.push_str("<w:pPr>");
            write_para_props(&style.paragraph, &mut s);
            s.push_str("</w:pPr>");
        }
        if !style.font.is_empty() {
            s.push_str("<w:rPr>");
            write_font_props(&style.font, &mut s);
            s.push_str("</w:rPr>");
        }
        s.push_str("</w:style>");
    }
    s.push_str("</w:styles>");
    s
}

fn numbering_xml(catalog: &ListCatalog, list_nums: &BTreeMap<String, i64>) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="{NS_W}">"#
    );

    // abstractNums
    let mut abstracts: Vec<(i64, ListDef, String)> = Vec::new();
    for (key, &num_id) in list_nums {
        let def = if let Some(id) = key.strip_prefix("__auto_") {
            // synthetic
            let ordered = id.starts_with("ol");
            synthetic_list_def(ordered)
        } else {
            catalog
                .get(&ListId::new(key))
                .cloned()
                .unwrap_or_else(|| synthetic_list_def(true))
        };
        abstracts.push((num_id, def, key.clone()));
    }
    abstracts.sort_by_key(|(id, _, _)| *id);

    for (num_id, def, _) in &abstracts {
        s.push_str(&format!(r#"<w:abstractNum w:abstractNumId="{num_id}">"#));
        if def.levels.is_empty() {
            write_lvl(&mut s, 0, &default_level(true));
        } else {
            for (ilvl, level) in def.levels.iter().enumerate() {
                write_lvl(&mut s, ilvl, level);
            }
        }
        s.push_str("</w:abstractNum>");
    }
    for (num_id, _, _) in &abstracts {
        s.push_str(&format!(
            r#"<w:num w:numId="{num_id}"><w:abstractNumId w:val="{num_id}"/></w:num>"#
        ));
    }
    s.push_str("</w:numbering>");
    s
}

fn synthetic_list_def(ordered: bool) -> ListDef {
    let mut def = ListDef::default();
    for i in 0..9 {
        def.levels.push(if ordered {
            docsai_model::list::ListLevel::new(NumFormat::Decimal, format!("%{}.", i + 1))
        } else {
            docsai_model::list::ListLevel::new(NumFormat::Bullet, "•")
        });
    }
    def
}

fn default_level(ordered: bool) -> docsai_model::list::ListLevel {
    if ordered {
        docsai_model::list::ListLevel::new(NumFormat::Decimal, "%1.")
    } else {
        docsai_model::list::ListLevel::new(NumFormat::Bullet, "•")
    }
}

fn write_lvl(s: &mut String, ilvl: usize, level: &docsai_model::list::ListLevel) {
    s.push_str(&format!(r#"<w:lvl w:ilvl="{ilvl}">"#));
    // Word defaults missing `w:start` to 1. Only emit when the IR carried an
    // explicit value so round-trip does not invent `start: 1` in DocMark.
    if let Some(start) = level.start {
        s.push_str(&format!(r#"<w:start w:val="{start}"/>"#));
    }
    s.push_str(&format!(
        r#"<w:numFmt w:val="{}"/>"#,
        esc_attr(level.format.as_str())
    ));
    s.push_str(&format!(
        r#"<w:lvlText w:val="{}"/>"#,
        esc_attr(&level.text)
    ));
    s.push_str(r#"<w:lvlJc w:val="left"/>"#);
    s.push_str("<w:pPr><w:ind");
    if let Some(ind) = level.indent {
        s.push_str(&format!(r#" w:left="{}""#, ind.twips()));
    } else {
        s.push_str(&format!(r#" w:left="{}""#, 720 * (ilvl as i64 + 1)));
    }
    if let Some(h) = level.hanging {
        s.push_str(&format!(r#" w:hanging="{}""#, h.twips()));
    } else {
        s.push_str(r#" w:hanging="360""#);
    }
    s.push_str("/></w:pPr>");
    s.push_str("</w:lvl>");
}

fn footnotes_xml(ctx: &WriterCtx<'_>) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:footnotes xmlns:w="{NS_W}" xmlns:r="{NS_R}" xmlns:wp="{NS_WP}" xmlns:a="{NS_A}" xmlns:pic="{NS_PIC}">
  <w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
  <w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"#
    );
    for (id, body) in &ctx.footnotes {
        s.push_str(&format!(r#"<w:footnote w:id="{id}">{body}</w:footnote>"#));
    }
    s.push_str("</w:footnotes>");
    s
}

fn looks_like_run_xml(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("<w:r") || t.starts_with("<w:hyperlink") || t.starts_with("<m:")
}

fn esc_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn esc_attr(s: &str) -> String {
    let mut out = esc_text(s);
    out = out.replace('"', "&quot;");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;
    use std::io::Cursor;

    #[test]
    fn write_then_read_preserves_basic_text() {
        let doc = Document::Text(TextDocument {
            meta: docsai_model::DocumentMeta {
                title: Some("Hola".into()),
                author: Some("docsai".into()),
                ..Default::default()
            },
            sections: vec![Section {
                page: docsai_model::text::PageGeometry {
                    size: docsai_model::units::Size::new(
                        docsai_model::units::Length::from_twips(11906),
                        docsai_model::units::Length::from_twips(16838),
                    ),
                    ..Default::default()
                },
                blocks: vec![
                    Block::Paragraph(Paragraph::text("Primer parrafo")),
                    Block::Heading(Heading {
                        level: 1,
                        paragraph: Paragraph::text("Titulo"),
                    }),
                    Block::Paragraph(Paragraph::text("Segundo")),
                ],
                ..Default::default()
            }],
            ..Default::default()
        });
        let assets = MemoryAssetStore::new();
        let mut buf = Cursor::new(Vec::new());
        write_docx(&doc, &assets, &mut buf).expect("write");
        buf.set_position(0);
        let mut assets2 = MemoryAssetStore::new();
        let (read_back, _) = crate::docx::read(buf, &mut assets2).expect("read");
        let text = match read_back {
            Document::Text(t) => t,
            _ => panic!("expected text"),
        };
        assert_eq!(text.meta.title.as_deref(), Some("Hola"));
        let plain: Vec<String> = text
            .blocks()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.plain_text()),
                Block::Heading(h) => Some(h.paragraph.plain_text()),
                _ => None,
            })
            .collect();
        assert!(plain.iter().any(|p| p.contains("Primer parrafo")));
        assert!(plain.iter().any(|p| p.contains("Titulo")));
        assert!(plain.iter().any(|p| p.contains("Segundo")));
    }

    #[test]
    fn write_then_read_preserves_floating_geometry_and_footnote_formatting() {
        use docsai_model::image::{
            Anchor, AxisPos, HVPos, ImageGeometry, ImageRef, RelBase, WrapMode, WrapSide,
        };
        use docsai_model::style::FontProps;
        use docsai_model::text::Inline;
        use docsai_model::units::{Length, Size};

        let png = {
            // 1x1 PNG
            vec![
                0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
                0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
                0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
                0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xFE,
                0xD4, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
            ]
        };
        let mut assets = MemoryAssetStore::new();
        let asset = assets.put(&png).expect("asset");
        let image = ImageRef {
            asset: asset.clone(),
            geometry: ImageGeometry {
                display_size: Size::new(Length::from_cm(3.5), Length::from_cm(2.6)),
                native_size_px: Some((1, 1)),
                dpi: None,
                anchor: Anchor::Floating {
                    relative_to_h: RelBase::Margin,
                    relative_to_v: RelBase::Paragraph,
                    position: HVPos {
                        h: AxisPos::Offset(Length::from_cm(1.2)),
                        v: AxisPos::Offset(Length::from_cm(0.5)),
                    },
                    wrap: WrapMode::Square,
                    wrap_side: WrapSide::Right,
                    behind_text: false,
                },
                rotation_deg: 0.0,
                flip: Default::default(),
                crop: None,
                border: None,
                z_index: Some(2),
            },
            alt: "Logo".into(),
            title: None,
            name: Some("Logo".into()),
            link: None,
            external_src: None,
            effects_raw: None,
        };

        let footnote_body = vec![Block::Paragraph(Paragraph {
            format: Default::default(),
            content: vec![
                Inline::Text("Nota con ".into()),
                Inline::Styled {
                    content: vec![Inline::Text("negrita".into())],
                    props: RunProps {
                        style: None,
                        direct: FontProps {
                            bold: Some(true),
                            ..Default::default()
                        },
                    },
                },
            ],
        })];

        let doc = Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![Block::Paragraph(Paragraph {
                    format: Default::default(),
                    content: vec![
                        Inline::Image(image),
                        Inline::Text(" cuerpo".into()),
                        Inline::Footnote(footnote_body),
                    ],
                })],
                ..Default::default()
            }],
            ..Default::default()
        });

        let mut buf = Cursor::new(Vec::new());
        let report = write_docx(&doc, &assets, &mut buf).expect("write");
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.message().contains("floating")),
            "floating must not degrade: {:?}",
            report.warnings
        );
        buf.set_position(0);
        let mut assets2 = MemoryAssetStore::new();
        let (read_back, read_report) = crate::docx::read(buf, &mut assets2).expect("read");
        assert_eq!(read_report.stats.footnotes, 1);
        let text = match read_back {
            Document::Text(t) => t,
            _ => panic!("expected text"),
        };
        let mut found_floating = false;
        let mut found_bold_note = false;
        fn walk(inlines: &[Inline], floating: &mut bool, bold_note: &mut bool) {
            for inline in inlines {
                match inline {
                    Inline::Image(img) => {
                        if matches!(img.geometry.anchor, Anchor::Floating { .. }) {
                            *floating = true;
                            assert_eq!(img.geometry.z_index, Some(2));
                        }
                    }
                    Inline::Footnote(blocks) => {
                        for b in blocks {
                            if let Block::Paragraph(p) = b {
                                for i in &p.content {
                                    if let Inline::Styled { props, content } = i {
                                        if props.direct.bold == Some(true) {
                                            let t: String = content
                                                .iter()
                                                .filter_map(|c| match c {
                                                    Inline::Text(s) => Some(s.as_str()),
                                                    _ => None,
                                                })
                                                .collect();
                                            if t.contains("negrita") {
                                                *bold_note = true;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Inline::Styled { content, .. } | Inline::Link { content, .. } => {
                        walk(content, floating, bold_note);
                    }
                    _ => {}
                }
            }
        }
        for block in text.blocks() {
            if let Block::Paragraph(p) = block {
                walk(&p.content, &mut found_floating, &mut found_bold_note);
            }
        }
        assert!(found_floating, "floating anchor lost on write/read");
        assert!(
            found_bold_note,
            "footnote bold formatting lost on write/read"
        );
    }
}
