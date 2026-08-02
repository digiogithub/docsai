//! IR → `.odt` writer (Phase 4).

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
    Block, BreakKind, DocumentMeta, FieldKind, HeaderFooter, Heading, Inline, List, Orientation,
    Paragraph, RawFragment, Table, TextDocument,
};
use docsai_model::units::Length;
use docsai_model::Document;

use crate::length::{format_cm, format_pt};
use crate::package::Package;
use crate::write_error::WriteError;

const MIME: &str = "application/vnd.oasis.opendocument.text";

const NS_OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const NS_STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const NS_TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const NS_TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const NS_DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const NS_FO: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const NS_SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const NS_XLINK: &str = "http://www.w3.org/1999/xlink";
const NS_DC: &str = "http://purl.org/dc/elements/1.1/";
const NS_META: &str = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0";
const NS_MANIFEST: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";

/// Writes a text document as an `.odt` package.
pub fn write_odt<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    let text = match document {
        Document::Text(t) => t,
        Document::Workbook(_) => {
            return Err(WriteError::Invalid(
                "cannot write a workbook as .odt".into(),
            ));
        }
    };
    let mut report = ConversionReport::new();
    let package = build_package(text, assets, &mut report)?;
    package.write_to(writer)?;
    Ok(report)
}

struct AutoStyles {
    para: BTreeMap<String, (Option<String>, ParaProps, FontProps)>,
    text: BTreeMap<String, (Option<String>, FontProps)>,
    graphic: BTreeMap<String, GraphicOut>,
    para_seq: u32,
    text_seq: u32,
    graphic_seq: u32,
}

#[derive(Clone)]
struct GraphicOut {
    wrap: String,
    run_through: Option<String>,
    h_pos: Option<String>,
    h_rel: Option<String>,
    v_pos: Option<String>,
    v_rel: Option<String>,
    mirror: Option<String>,
    rotation: Option<f32>,
}

impl AutoStyles {
    fn new() -> Self {
        Self {
            para: BTreeMap::new(),
            text: BTreeMap::new(),
            graphic: BTreeMap::new(),
            para_seq: 0,
            text_seq: 0,
            graphic_seq: 0,
        }
    }

    fn para_style(
        &mut self,
        parent: Option<&str>,
        para: &ParaProps,
        font: &FontProps,
    ) -> Option<String> {
        if parent.is_none() && para.is_empty() && font.is_empty() {
            return None;
        }
        if para.is_empty() && font.is_empty() {
            return parent.map(str::to_string);
        }
        for (name, (p, pp, ff)) in &self.para {
            if p.as_deref() == parent && pp == para && ff == font {
                return Some(name.clone());
            }
        }
        self.para_seq += 1;
        let name = format!("P{}", self.para_seq);
        self.para.insert(
            name.clone(),
            (parent.map(str::to_string), para.clone(), font.clone()),
        );
        Some(name)
    }

    fn text_style(&mut self, parent: Option<&str>, font: &FontProps) -> Option<String> {
        if parent.is_none() && font.is_empty() {
            return None;
        }
        if font.is_empty() {
            return parent.map(str::to_string);
        }
        for (name, (p, ff)) in &self.text {
            if p.as_deref() == parent && ff == font {
                return Some(name.clone());
            }
        }
        self.text_seq += 1;
        let name = format!("T{}", self.text_seq);
        self.text
            .insert(name.clone(), (parent.map(str::to_string), font.clone()));
        Some(name)
    }

    fn graphic_style(&mut self, g: GraphicOut) -> String {
        for (name, existing) in &self.graphic {
            if existing.wrap == g.wrap
                && existing.run_through == g.run_through
                && existing.h_pos == g.h_pos
                && existing.h_rel == g.h_rel
                && existing.v_pos == g.v_pos
                && existing.v_rel == g.v_rel
                && existing.mirror == g.mirror
                && existing.rotation == g.rotation
            {
                return name.clone();
            }
        }
        self.graphic_seq += 1;
        let name = format!("fr{}", self.graphic_seq);
        self.graphic.insert(name.clone(), g);
        name
    }
}

struct WriterCtx<'a> {
    assets: &'a dyn AssetStore,
    report: &'a mut ConversionReport,
    auto: AutoStyles,
    pictures: BTreeMap<String, Vec<u8>>, // path → bytes
    pic_seq: u32,
    note_seq: u32,
}

fn build_package(
    doc: &TextDocument,
    assets: &dyn AssetStore,
    report: &mut ConversionReport,
) -> Result<Package, WriteError> {
    let mut ctx = WriterCtx {
        assets,
        report,
        auto: AutoStyles::new(),
        pictures: BTreeMap::new(),
        pic_seq: 0,
        note_seq: 0,
    };

    let body_xml = write_body(doc, &mut ctx)?;
    let auto_xml = write_automatic_styles(&ctx.auto);
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-content xmlns:office="{NS_OFFICE}" xmlns:style="{NS_STYLE}" xmlns:text="{NS_TEXT}" xmlns:table="{NS_TABLE}" xmlns:draw="{NS_DRAW}" xmlns:fo="{NS_FO}" xmlns:svg="{NS_SVG}" xmlns:xlink="{NS_XLINK}" office:version="1.3">
{auto_xml}
 <office:body>
  <office:text>
{body_xml}  </office:text>
 </office:body>
</office:document-content>
"#
    );

    let styles_xml = write_styles_xml(doc, &mut ctx)?;
    let meta_xml = write_meta_xml(&doc.meta);
    let mut package = Package::new();
    package.insert("mimetype", MIME.as_bytes());
    package.insert("content.xml", content.into_bytes());
    package.insert("styles.xml", styles_xml.into_bytes());
    package.insert("meta.xml", meta_xml.into_bytes());

    for (path, bytes) in &ctx.pictures {
        package.insert(path, bytes.clone());
    }

    let mut entries = vec![
        ("/".into(), MIME.to_string()),
        ("content.xml".into(), "text/xml".into()),
        ("styles.xml".into(), "text/xml".into()),
        ("meta.xml".into(), "text/xml".into()),
    ];
    for path in ctx.pictures.keys() {
        let mime = guess_image_mime(path);
        entries.push((path.clone(), mime));
    }
    package.insert(
        "META-INF/manifest.xml",
        write_manifest(&entries).into_bytes(),
    );
    Ok(package)
}

fn write_body(doc: &TextDocument, ctx: &mut WriterCtx<'_>) -> Result<String, WriteError> {
    let mut out = String::new();
    for section in &doc.sections {
        write_blocks(&section.blocks, ctx, &mut out)?;
    }
    if doc.sections.is_empty() {
        out.push_str("   <text:p/>\n");
    }
    Ok(out)
}

fn write_blocks(
    blocks: &[Block],
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) -> Result<(), WriteError> {
    for block in blocks {
        write_block(block, ctx, out)?;
    }
    Ok(())
}

fn write_block(block: &Block, ctx: &mut WriterCtx<'_>, out: &mut String) -> Result<(), WriteError> {
    match block {
        Block::Paragraph(p) => write_paragraph("text:p", None, p, ctx, out)?,
        Block::Heading(h) => write_heading(h, ctx, out)?,
        Block::List(list) => write_list(list, ctx, out)?,
        Block::Table(t) => write_table(t, ctx, out)?,
        Block::Image(img) => {
            out.push_str("   <text:p>");
            write_image(img, ctx, out)?;
            out.push_str("</text:p>\n");
            ctx.report.stats.images = ctx.report.stats.images.saturating_add(1);
        }
        Block::TextBox(tb) => {
            ctx.report.warn(Warning::Degraded {
                what: "text-box".into(),
                why: "flattened to paragraphs in ODT writer".into(),
            });
            write_blocks(&tb.blocks, ctx, out)?;
        }
        Block::Raw(raw) => write_raw_block(raw, ctx, out),
    }
    Ok(())
}

fn write_heading(
    heading: &Heading,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) -> Result<(), WriteError> {
    let level = heading.level.clamp(1, 9);
    write_paragraph("text:h", Some(level), &heading.paragraph, ctx, out)?;
    ctx.report.stats.headings = ctx.report.stats.headings.saturating_add(1);
    Ok(())
}

fn write_paragraph(
    tag: &str,
    outline: Option<u8>,
    para: &Paragraph,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) -> Result<(), WriteError> {
    let parent = para.format.style.as_ref().map(|s| s.as_str());
    let style_name = ctx
        .auto
        .para_style(parent, &para.format.direct, &para.format.run_direct);
    out.push_str("   <");
    out.push_str(tag);
    if let Some(name) = &style_name {
        out.push_str(&format!(r#" text:style-name="{}""#, esc_attr(name)));
    }
    if let Some(level) = outline {
        out.push_str(&format!(r#" text:outline-level="{level}""#));
    }
    out.push('>');
    write_inlines(&para.content, ctx, out)?;
    out.push_str(&format!("</{tag}>\n"));
    ctx.report.stats.paragraphs = ctx.report.stats.paragraphs.saturating_add(1);
    Ok(())
}

fn write_inlines(
    inlines: &[Inline],
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) -> Result<(), WriteError> {
    for inline in inlines {
        write_inline(inline, ctx, out)?;
    }
    Ok(())
}

fn write_inline(
    inline: &Inline,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) -> Result<(), WriteError> {
    match inline {
        Inline::Text(t) => write_text_runs(t, out),
        Inline::Styled { content, props } => {
            let parent = props.style.as_ref().map(|s| s.as_str());
            let style = ctx.auto.text_style(parent, &props.direct);
            if let Some(name) = style {
                out.push_str(&format!(
                    r#"<text:span text:style-name="{}">"#,
                    esc_attr(&name)
                ));
                write_inlines(content, ctx, out)?;
                out.push_str("</text:span>");
            } else {
                write_inlines(content, ctx, out)?;
            }
        }
        Inline::Link {
            target, content, ..
        } => {
            out.push_str(&format!(
                r#"<text:a xlink:type="simple" xlink:href="{}">"#,
                esc_attr(target)
            ));
            write_inlines(content, ctx, out)?;
            out.push_str("</text:a>");
        }
        Inline::Break(BreakKind::Line) => out.push_str("<text:line-break/>"),
        Inline::Break(BreakKind::Page) => {
            // Page breaks are expressed via paragraph style break-before.
            out.push_str("<text:line-break/>");
        }
        Inline::Break(BreakKind::Column) => out.push_str("<text:line-break/>"),
        Inline::Image(img) => {
            write_image(img, ctx, out)?;
            ctx.report.stats.images = ctx.report.stats.images.saturating_add(1);
        }
        Inline::Footnote(blocks) => {
            ctx.note_seq += 1;
            let id = ctx.note_seq;
            out.push_str(&format!(
                r#"<text:note text:id="ftn{id}" text:note-class="footnote"><text:note-citation>{id}</text:note-citation><text:note-body>"#
            ));
            write_blocks(blocks, ctx, out)?;
            out.push_str("</text:note-body></text:note>");
            ctx.report.stats.footnotes = ctx.report.stats.footnotes.saturating_add(1);
        }
        Inline::Field { kind, cached, .. } => match kind {
            FieldKind::Page => {
                out.push_str(r#"<text:page-number text:select-page="current">"#);
                out.push_str(&esc_text(cached));
                out.push_str("</text:page-number>");
            }
            FieldKind::NumPages => {
                out.push_str("<text:page-count>");
                out.push_str(&esc_text(cached));
                out.push_str("</text:page-count>");
            }
            _ => write_text_runs(cached, out),
        },
        Inline::Raw(raw) => write_raw_inline(raw, ctx, out),
    }
    Ok(())
}

fn write_text_runs(text: &str, out: &mut String) {
    // Collapse runs of spaces into text:s where helpful; keep simple escaping.
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            ' ' => {
                let mut n = 1usize;
                while chars.peek() == Some(&' ') {
                    chars.next();
                    n += 1;
                }
                if n == 1 {
                    out.push(' ');
                } else {
                    out.push_str(&format!(r#"<text:s text:c="{n}"/>"#));
                }
            }
            '\t' => out.push_str("<text:tab/>"),
            _ => {
                let mut buf = String::new();
                buf.push(c);
                while let Some(&next) = chars.peek() {
                    if next == ' ' || next == '\t' {
                        break;
                    }
                    buf.push(next);
                    chars.next();
                }
                out.push_str(&esc_text(&buf));
            }
        }
    }
}

fn write_list(list: &List, ctx: &mut WriterCtx<'_>, out: &mut String) -> Result<(), WriteError> {
    out.push_str("   <text:list");
    if let Some(def) = &list.def {
        out.push_str(&format!(r#" text:style-name="{}""#, esc_attr(def.as_str())));
    }
    out.push_str(">\n");
    for item in &list.items {
        out.push_str("    <text:list-item>\n");
        write_blocks(&item.blocks, ctx, out)?;
        out.push_str("    </text:list-item>\n");
    }
    out.push_str("   </text:list>\n");
    ctx.report.stats.lists = ctx.report.stats.lists.saturating_add(1);
    Ok(())
}

fn write_table(table: &Table, ctx: &mut WriterCtx<'_>, out: &mut String) -> Result<(), WriteError> {
    out.push_str("   <table:table");
    if let Some(style) = &table.style {
        out.push_str(&format!(
            r#" table:style-name="{}""#,
            esc_attr(style.as_str())
        ));
    }
    out.push_str(">\n");
    let cols = table.width().max(1);
    for i in 0..cols {
        out.push_str("    <table:table-column");
        if let Some(w) = table.col_widths.get(i) {
            if w.emu() > 0 {
                // Column width is expressed via an automatic style; emit plain column.
            }
        }
        out.push_str("/>\n");
    }
    for row in &table.rows {
        out.push_str("    <table:table-row>\n");
        for cell in &row.cells {
            if cell.covered {
                out.push_str("     <table:covered-table-cell/>\n");
                continue;
            }
            out.push_str("     <table:table-cell");
            if cell.colspan > 1 {
                out.push_str(&format!(
                    r#" table:number-columns-spanned="{}""#,
                    cell.colspan
                ));
            }
            if cell.rowspan > 1 {
                out.push_str(&format!(r#" table:number-rows-spanned="{}""#, cell.rowspan));
            }
            out.push_str(">\n");
            if cell.blocks.is_empty() {
                out.push_str("      <text:p/>\n");
            } else {
                write_blocks(&cell.blocks, ctx, out)?;
            }
            out.push_str("     </table:table-cell>\n");
        }
        out.push_str("    </table:table-row>\n");
    }
    out.push_str("   </table:table>\n");
    ctx.report.stats.tables = ctx.report.stats.tables.saturating_add(1);
    Ok(())
}

fn write_image(
    img: &ImageRef,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) -> Result<(), WriteError> {
    if let Some(url) = &img.external_src {
        ctx.report
            .warn(Warning::ExternalImageNotFetched { url: url.clone() });
    }
    let href = if let Some(url) = &img.external_src {
        url.clone()
    } else {
        let bytes = ctx.assets.get(&img.asset).ok_or_else(|| {
            WriteError::Asset(docsai_model::assets::AssetError::NotFound(
                img.asset.clone(),
            ))
        })?;
        let ext = ctx
            .assets
            .info(&img.asset)
            .map(|i| i.file_name.rsplit('.').next().unwrap_or("png").to_string())
            .unwrap_or_else(|| "png".into());
        ctx.pic_seq += 1;
        let path = format!("Pictures/image{:04}.{}", ctx.pic_seq, ext);
        ctx.pictures.insert(path.clone(), bytes.to_vec());
        path
    };

    let w = format_cm(img.geometry.display_size.width);
    let h = format_cm(img.geometry.display_size.height);
    let (anchor_type, style_name, x, y) = frame_attrs(img, ctx);

    out.push_str("<draw:frame");
    if let Some(name) = img.name.as_ref().or(img.title.as_ref()) {
        out.push_str(&format!(r#" draw:name="{}""#, esc_attr(name)));
    }
    out.push_str(&format!(
        r#" draw:style-name="{}" text:anchor-type="{}" svg:width="{}" svg:height="{}""#,
        esc_attr(&style_name),
        esc_attr(anchor_type),
        esc_attr(&w),
        esc_attr(&h),
    ));
    if let Some(x) = x {
        out.push_str(&format!(r#" svg:x="{}""#, esc_attr(&format_cm(x))));
    }
    if let Some(y) = y {
        out.push_str(&format!(r#" svg:y="{}""#, esc_attr(&format_cm(y))));
    }
    if let Some(z) = img.geometry.z_index {
        out.push_str(&format!(r#" draw:z-index="{z}""#));
    }
    out.push('>');
    if !img.alt.is_empty() {
        out.push_str(&format!("<svg:desc>{}</svg:desc>", esc_text(&img.alt)));
    }
    if let Some(title) = &img.title {
        out.push_str(&format!("<svg:title>{}</svg:title>", esc_text(title)));
    }
    out.push_str(&format!(
        r#"<draw:image xlink:href="{}" xlink:type="simple" xlink:show="embed" xlink:actuate="onLoad"/>"#,
        esc_attr(&href)
    ));
    out.push_str("</draw:frame>");
    Ok(())
}

fn frame_attrs(
    img: &ImageRef,
    ctx: &mut WriterCtx<'_>,
) -> (&'static str, String, Option<Length>, Option<Length>) {
    match &img.geometry.anchor {
        Anchor::Inline => {
            let style = ctx.auto.graphic_style(GraphicOut {
                wrap: "none".into(),
                run_through: None,
                h_pos: None,
                h_rel: None,
                v_pos: None,
                v_rel: None,
                mirror: flip_mirror(img.geometry.flip),
                rotation: nonzero_rot(img.geometry.rotation_deg),
            });
            ("as-char", style, None, None)
        }
        Anchor::Floating {
            relative_to_h,
            relative_to_v,
            position,
            wrap,
            wrap_side,
            behind_text,
        } => {
            let (h_pos, x) = axis_out(&position.h, true);
            let (v_pos, y) = axis_out(&position.v, false);
            let wrap_s = wrap_out(*wrap, *wrap_side);
            let style = ctx.auto.graphic_style(GraphicOut {
                wrap: wrap_s,
                run_through: behind_text.then(|| "background".into()),
                h_pos,
                h_rel: Some(rel_out(*relative_to_h)),
                v_pos,
                v_rel: Some(rel_out(*relative_to_v)),
                mirror: flip_mirror(img.geometry.flip),
                rotation: nonzero_rot(img.geometry.rotation_deg),
            });
            let anchor_type = match relative_to_h {
                RelBase::Page => "page",
                RelBase::Character => "char",
                _ => "paragraph",
            };
            (anchor_type, style, x, y)
        }
        Anchor::SheetOneCell { .. }
        | Anchor::SheetTwoCell { .. }
        | Anchor::SheetAbsolute { .. } => {
            let style = ctx.auto.graphic_style(GraphicOut {
                wrap: "none".into(),
                run_through: None,
                h_pos: None,
                h_rel: None,
                v_pos: None,
                v_rel: None,
                mirror: flip_mirror(img.geometry.flip),
                rotation: nonzero_rot(img.geometry.rotation_deg),
            });
            ("paragraph", style, None, None)
        }
    }
}

fn axis_out(pos: &AxisPos, horizontal: bool) -> (Option<String>, Option<Length>) {
    match pos {
        AxisPos::Offset(l) => (
            Some(if horizontal {
                "from-left".into()
            } else {
                "from-top".into()
            }),
            Some(*l),
        ),
        AxisPos::Align(a) => {
            let s = match a {
                AlignKeyword::Left => "left",
                AlignKeyword::Center => "center",
                AlignKeyword::Right => "right",
                AlignKeyword::Top => "top",
                AlignKeyword::Middle => "middle",
                AlignKeyword::Bottom => "bottom",
                AlignKeyword::Inside => "inside",
                AlignKeyword::Outside => "outside",
            };
            (Some(s.into()), None)
        }
    }
}

fn rel_out(rel: RelBase) -> String {
    match rel {
        RelBase::Page => "page".into(),
        RelBase::Margin => "page-content".into(),
        RelBase::Paragraph => "paragraph".into(),
        RelBase::Character => "char".into(),
        RelBase::Line => "line".into(),
        RelBase::Column => "frame".into(),
    }
}

fn wrap_out(mode: WrapMode, side: WrapSide) -> String {
    match mode {
        WrapMode::Through => "run-through".into(),
        WrapMode::TopBottom | WrapMode::None => "none".into(),
        WrapMode::Square | WrapMode::Tight => match side {
            WrapSide::Left => "left".into(),
            WrapSide::Right => "right".into(),
            WrapSide::Largest => "biggest".into(),
            WrapSide::Both => "parallel".into(),
        },
    }
}

fn flip_mirror(flip: Flip) -> Option<String> {
    match flip {
        Flip::None => None,
        Flip::H => Some("horizontal".into()),
        Flip::V => Some("vertical".into()),
        Flip::HV => Some("horizontal vertical".into()),
    }
}

fn nonzero_rot(deg: f32) -> Option<f32> {
    if deg == 0.0 {
        None
    } else {
        Some(deg)
    }
}

fn write_raw_block(raw: &RawFragment, ctx: &mut WriterCtx<'_>, out: &mut String) {
    if raw.format == "odf" {
        out.push_str(&raw.content);
        if !raw.content.ends_with('\n') {
            out.push('\n');
        }
    } else {
        ctx.report.warn(Warning::RawBlockDropped {
            id: raw.id.as_str().to_string(),
            format: raw.format.clone(),
        });
    }
}

fn write_raw_inline(raw: &RawFragment, ctx: &mut WriterCtx<'_>, out: &mut String) {
    if raw.format == "odf" {
        out.push_str(&raw.content);
    } else {
        ctx.report.warn(Warning::RawBlockDropped {
            id: raw.id.as_str().to_string(),
            format: raw.format.clone(),
        });
    }
}

fn write_automatic_styles(auto: &AutoStyles) -> String {
    let mut s = String::from(" <office:automatic-styles>\n");
    for (name, (parent, para, font)) in &auto.para {
        s.push_str(&format!(
            r#"  <style:style style:name="{}" style:family="paragraph""#,
            esc_attr(name)
        ));
        if let Some(p) = parent {
            s.push_str(&format!(r#" style:parent-style-name="{}""#, esc_attr(p)));
        }
        s.push_str(">\n");
        if !para.is_empty() {
            s.push_str("   <style:paragraph-properties");
            write_para_attrs(para, &mut s);
            s.push_str("/>\n");
        }
        if !font.is_empty() {
            s.push_str("   <style:text-properties");
            write_font_attrs(font, &mut s);
            s.push_str("/>\n");
        }
        s.push_str("  </style:style>\n");
    }
    for (name, (parent, font)) in &auto.text {
        s.push_str(&format!(
            r#"  <style:style style:name="{}" style:family="text""#,
            esc_attr(name)
        ));
        if let Some(p) = parent {
            s.push_str(&format!(r#" style:parent-style-name="{}""#, esc_attr(p)));
        }
        s.push_str(">\n");
        if !font.is_empty() {
            s.push_str("   <style:text-properties");
            write_font_attrs(font, &mut s);
            s.push_str("/>\n");
        }
        s.push_str("  </style:style>\n");
    }
    for (name, g) in &auto.graphic {
        s.push_str(&format!(
            r#"  <style:style style:name="{}" style:family="graphic">
   <style:graphic-properties style:wrap="{}""#,
            esc_attr(name),
            esc_attr(&g.wrap)
        ));
        if let Some(rt) = &g.run_through {
            s.push_str(&format!(r#" style:run-through="{}""#, esc_attr(rt)));
        }
        if let Some(v) = &g.h_pos {
            s.push_str(&format!(r#" style:horizontal-pos="{}""#, esc_attr(v)));
        }
        if let Some(v) = &g.h_rel {
            s.push_str(&format!(r#" style:horizontal-rel="{}""#, esc_attr(v)));
        }
        if let Some(v) = &g.v_pos {
            s.push_str(&format!(r#" style:vertical-pos="{}""#, esc_attr(v)));
        }
        if let Some(v) = &g.v_rel {
            s.push_str(&format!(r#" style:vertical-rel="{}""#, esc_attr(v)));
        }
        if let Some(v) = &g.mirror {
            s.push_str(&format!(r#" style:mirror="{}""#, esc_attr(v)));
        }
        if let Some(a) = g.rotation {
            s.push_str(&format!(r#" draw:rotation-angle="{a}""#));
        }
        s.push_str("/>\n  </style:style>\n");
    }
    s.push_str(" </office:automatic-styles>\n");
    s
}

fn write_styles_xml(doc: &TextDocument, ctx: &mut WriterCtx<'_>) -> Result<String, WriteError> {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-styles xmlns:office="{NS_OFFICE}" xmlns:style="{NS_STYLE}" xmlns:text="{NS_TEXT}" xmlns:draw="{NS_DRAW}" xmlns:fo="{NS_FO}" xmlns:svg="{NS_SVG}" office:version="1.3">
 <office:styles>
"#
    );
    write_named_styles(&doc.styles, &mut s);
    write_list_styles(&doc.list_defs, &mut s);
    s.push_str(" </office:styles>\n");

    let page = doc.sections.first().map(|sec| sec.page);
    s.push_str(" <office:automatic-styles>\n");
    s.push_str(r#"  <style:page-layout style:name="pm1">"#);
    s.push('\n');
    if let Some(page) = page {
        s.push_str("   <style:page-layout-properties");
        s.push_str(&format!(
            r#" fo:page-width="{}" fo:page-height="{}" fo:margin-top="{}" fo:margin-bottom="{}" fo:margin-left="{}" fo:margin-right="{}" style:print-orientation="{}""#,
            esc_attr(&format_cm(page.size.width)),
            esc_attr(&format_cm(page.size.height)),
            esc_attr(&format_cm(page.margins.top)),
            esc_attr(&format_cm(page.margins.bottom)),
            esc_attr(&format_cm(page.margins.left)),
            esc_attr(&format_cm(page.margins.right)),
            match page.orientation {
                Orientation::Landscape => "landscape",
                Orientation::Portrait => "portrait",
            }
        ));
        s.push_str("/>\n");
    } else {
        s.push_str(
            r#"   <style:page-layout-properties fo:page-width="21cm" fo:page-height="29.7cm" fo:margin="2cm"/>"#,
        );
        s.push('\n');
    }
    s.push_str("  </style:page-layout>\n");
    s.push_str(" </office:automatic-styles>\n");

    s.push_str(
        r#" <office:master-styles>
  <style:master-page style:name="Standard" style:page-layout-name="pm1">
"#,
    );
    if let Some(section) = doc.sections.first() {
        if let Some(header) = section
            .headers
            .iter()
            .find(|h| matches!(h.scope, docsai_model::text::HeaderScope::Default))
            .or_else(|| section.headers.first())
        {
            s.push_str("   <style:header>\n");
            write_header_blocks(header, ctx, &mut s)?;
            s.push_str("   </style:header>\n");
        }
        if let Some(footer) = section
            .footers
            .iter()
            .find(|h| matches!(h.scope, docsai_model::text::HeaderScope::Default))
            .or_else(|| section.footers.first())
        {
            s.push_str("   <style:footer>\n");
            write_header_blocks(footer, ctx, &mut s)?;
            s.push_str("   </style:footer>\n");
        }
    }
    s.push_str("  </style:master-page>\n </office:master-styles>\n</office:document-styles>\n");
    Ok(s)
}

fn write_header_blocks(
    hf: &HeaderFooter,
    ctx: &mut WriterCtx<'_>,
    out: &mut String,
) -> Result<(), WriteError> {
    write_blocks(&hf.blocks, ctx, out)
}

fn write_named_styles(catalog: &StyleCatalog, out: &mut String) {
    if !catalog.defaults.is_empty() {
        out.push_str(r#"  <style:default-style style:family="paragraph">"#);
        out.push('\n');
        if !catalog.defaults.paragraph.is_empty() {
            out.push_str("   <style:paragraph-properties");
            write_para_attrs(&catalog.defaults.paragraph, out);
            out.push_str("/>\n");
        }
        if !catalog.defaults.font.is_empty() {
            out.push_str("   <style:text-properties");
            write_font_attrs(&catalog.defaults.font, out);
            out.push_str("/>\n");
        }
        out.push_str("  </style:default-style>\n");
    }

    let mut has_standard = false;
    for style in catalog.styles.values() {
        if style.id.as_str() == "Standard" {
            has_standard = true;
        }
        let family = match style.style_type {
            StyleType::Paragraph => "paragraph",
            StyleType::Character => "text",
            StyleType::Table => "table",
            StyleType::Numbering => continue,
        };
        out.push_str(&format!(
            r#"  <style:style style:name="{}" style:display-name="{}" style:family="{family}""#,
            esc_attr(style.id.as_str()),
            esc_attr(&style.name),
        ));
        if let Some(base) = &style.based_on {
            out.push_str(&format!(
                r#" style:parent-style-name="{}""#,
                esc_attr(base.as_str())
            ));
        }
        if let Some(next) = &style.next {
            out.push_str(&format!(
                r#" style:next-style-name="{}""#,
                esc_attr(next.as_str())
            ));
        }
        if let Some(level) = style.paragraph.outline_level {
            out.push_str(&format!(
                r#" style:default-outline-level="{}""#,
                level.saturating_add(1)
            ));
        }
        out.push_str(">\n");
        if !style.paragraph.is_empty() {
            out.push_str("   <style:paragraph-properties");
            write_para_attrs(&style.paragraph, out);
            out.push_str("/>\n");
        }
        if !style.font.is_empty() {
            out.push_str("   <style:text-properties");
            write_font_attrs(&style.font, out);
            out.push_str("/>\n");
        }
        out.push_str("  </style:style>\n");
    }
    if !has_standard {
        out.push_str(
            r#"  <style:style style:name="Standard" style:family="paragraph" style:class="text"/>
"#,
        );
    }
}

fn write_list_styles(lists: &ListCatalog, out: &mut String) {
    for (id, def) in &lists.defs {
        write_list_style(id, def, out);
    }
}

fn write_list_style(id: &ListId, def: &ListDef, out: &mut String) {
    out.push_str(&format!(
        r#"  <text:list-style style:name="{}">"#,
        esc_attr(id.as_str())
    ));
    out.push('\n');
    for (i, level) in def.levels.iter().enumerate() {
        let level_no = i + 1;
        match &level.format {
            NumFormat::Bullet | NumFormat::None => {
                out.push_str(&format!(
                    r#"   <text:list-level-style-bullet text:level="{level_no}" text:bullet-char="{}">"#,
                    esc_attr(&level.text)
                ));
            }
            other => {
                let num = match other {
                    NumFormat::LowerLetter => "a",
                    NumFormat::UpperLetter => "A",
                    NumFormat::LowerRoman => "i",
                    NumFormat::UpperRoman => "I",
                    _ => "1",
                };
                let (prefix, suffix) = split_num_template(&level.text, level_no);
                out.push_str(&format!(
                    r#"   <text:list-level-style-number text:level="{level_no}" style:num-format="{num}""#
                ));
                if !prefix.is_empty() {
                    out.push_str(&format!(r#" style:num-prefix="{}""#, esc_attr(&prefix)));
                }
                if !suffix.is_empty() {
                    out.push_str(&format!(r#" style:num-suffix="{}""#, esc_attr(&suffix)));
                }
                if let Some(start) = level.start {
                    out.push_str(&format!(r#" text:start-value="{start}""#));
                }
                out.push('>');
            }
        }
        out.push('\n');
        if level.indent.is_some() || level.hanging.is_some() {
            out.push_str("    <style:list-level-properties");
            if let Some(ind) = level.indent {
                out.push_str(&format!(
                    r#" text:space-before="{}""#,
                    esc_attr(&format_cm(ind))
                ));
            }
            if let Some(hang) = level.hanging {
                out.push_str(&format!(
                    r#" text:min-label-width="{}""#,
                    esc_attr(&format_cm(hang))
                ));
            }
            out.push_str("/>\n");
        }
        match &level.format {
            NumFormat::Bullet | NumFormat::None => {
                out.push_str("   </text:list-level-style-bullet>\n");
            }
            _ => out.push_str("   </text:list-level-style-number>\n"),
        }
    }
    out.push_str("  </text:list-style>\n");
}

fn split_num_template(text: &str, level: usize) -> (String, String) {
    let token = format!("%{level}");
    if let Some(idx) = text.find(&token) {
        let prefix = text[..idx].to_string();
        let suffix = text[idx + token.len()..].to_string();
        (prefix, suffix)
    } else if text.ends_with('.') {
        (String::new(), ".".into())
    } else {
        (String::new(), text.to_string())
    }
}

fn write_para_attrs(para: &ParaProps, out: &mut String) {
    if let Some(align) = para.align {
        let a = match align {
            Align::Left => "start",
            Align::Center => "center",
            Align::Right => "end",
            Align::Justify => "justify",
        };
        out.push_str(&format!(r#" fo:text-align="{a}""#));
    }
    if let Some(v) = para.indent_left {
        out.push_str(&format!(r#" fo:margin-left="{}""#, esc_attr(&format_cm(v))));
    }
    if let Some(v) = para.indent_right {
        out.push_str(&format!(
            r#" fo:margin-right="{}""#,
            esc_attr(&format_cm(v))
        ));
    }
    if let Some(v) = para.indent_first_line {
        out.push_str(&format!(r#" fo:text-indent="{}""#, esc_attr(&format_cm(v))));
    } else if let Some(v) = para.indent_hanging {
        let neg = Length::from_emu(-v.emu());
        out.push_str(&format!(
            r#" fo:text-indent="{}""#,
            esc_attr(&format_cm(neg))
        ));
    }
    if let Some(v) = para.space_before {
        out.push_str(&format!(r#" fo:margin-top="{}""#, esc_attr(&format_cm(v))));
    }
    if let Some(v) = para.space_after {
        out.push_str(&format!(
            r#" fo:margin-bottom="{}""#,
            esc_attr(&format_cm(v))
        ));
    }
    if let Some(lh) = para.line_height {
        match lh {
            LineHeight::Multiple(m) => {
                let pct = m as f64 / 10.0;
                let s = format!("{pct}%");
                out.push_str(&format!(r#" fo:line-height="{}""#, esc_attr(&s)));
            }
            LineHeight::Exact(l) | LineHeight::AtLeast(l) => {
                out.push_str(&format!(r#" fo:line-height="{}""#, esc_attr(&format_cm(l))));
            }
        }
    }
    if let Some(true) = para.keep_with_next {
        out.push_str(r#" fo:keep-with-next="always""#);
    }
    if let Some(true) = para.page_break_before {
        out.push_str(r#" fo:break-before="page""#);
    }
    if let Some(bg) = &para.background {
        out.push_str(&format!(r#" fo:background-color="{}""#, esc_attr(bg)));
    }
}

fn write_font_attrs(font: &FontProps, out: &mut String) {
    if let Some(name) = &font.name {
        out.push_str(&format!(r#" style:font-name="{}""#, esc_attr(name)));
    }
    if let Some(size) = font.size {
        out.push_str(&format!(
            r#" fo:font-size="{}""#,
            esc_attr(&format_pt(size))
        ));
    }
    if let Some(bold) = font.bold {
        out.push_str(if bold {
            r#" fo:font-weight="bold""#
        } else {
            r#" fo:font-weight="normal""#
        });
    }
    if let Some(italic) = font.italic {
        out.push_str(if italic {
            r#" fo:font-style="italic""#
        } else {
            r#" fo:font-style="normal""#
        });
    }
    if let Some(true) = font.strike {
        out.push_str(r#" style:text-line-through-style="solid""#);
    } else if let Some(false) = font.strike {
        out.push_str(r#" style:text-line-through-style="none""#);
    }
    if let Some(u) = font.underline {
        match u {
            Underline::None => out.push_str(r#" style:text-underline-style="none""#),
            Underline::Double => out.push_str(
                r#" style:text-underline-style="solid" style:text-underline-type="double""#,
            ),
            Underline::Dotted => out.push_str(r#" style:text-underline-style="dotted""#),
            Underline::Dashed => out.push_str(r#" style:text-underline-style="dash""#),
            Underline::Wave => out.push_str(r#" style:text-underline-style="wave""#),
            Underline::Single | Underline::Thick => {
                out.push_str(r#" style:text-underline-style="solid""#)
            }
        }
    }
    if let Some(color) = &font.color {
        out.push_str(&format!(r#" fo:color="{}""#, esc_attr(color)));
    }
    if let Some(bg) = &font.highlight {
        // highlight may be a name; only emit when it looks like a colour.
        if bg.starts_with('#') {
            out.push_str(&format!(r#" fo:background-color="{}""#, esc_attr(bg)));
        }
    }
    if let Some(va) = font.vert_align {
        match va {
            VertAlign::Superscript => out.push_str(r#" style:text-position="super 58%""#),
            VertAlign::Subscript => out.push_str(r#" style:text-position="sub 58%""#),
            VertAlign::Baseline => out.push_str(r#" style:text-position="0% 100%""#),
        }
    }
    if let Some(true) = font.small_caps {
        out.push_str(r#" fo:font-variant="small-caps""#);
    }
    if let Some(true) = font.caps {
        out.push_str(r#" fo:text-transform="uppercase""#);
    }
}

fn write_meta_xml(meta: &DocumentMeta) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<office:document-meta xmlns:office="{NS_OFFICE}" xmlns:dc="{NS_DC}" xmlns:meta="{NS_META}" office:version="1.3">
 <office:meta>
"#
    );
    if let Some(v) = &meta.title {
        s.push_str(&format!("  <dc:title>{}</dc:title>\n", esc_text(v)));
    }
    if let Some(v) = &meta.author {
        s.push_str(&format!(
            "  <meta:initial-creator>{}</meta:initial-creator>\n",
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.last_modified_by {
        s.push_str(&format!("  <dc:creator>{}</dc:creator>\n", esc_text(v)));
    }
    if let Some(v) = &meta.subject {
        s.push_str(&format!("  <dc:subject>{}</dc:subject>\n", esc_text(v)));
    }
    if let Some(v) = &meta.description {
        s.push_str(&format!(
            "  <dc:description>{}</dc:description>\n",
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.language {
        s.push_str(&format!("  <dc:language>{}</dc:language>\n", esc_text(v)));
    }
    if let Some(v) = &meta.created {
        s.push_str(&format!(
            "  <meta:creation-date>{}</meta:creation-date>\n",
            esc_text(v)
        ));
    }
    if let Some(v) = &meta.modified {
        s.push_str(&format!("  <dc:date>{}</dc:date>\n", esc_text(v)));
    }
    if let Some(v) = &meta.application {
        s.push_str(&format!(
            "  <meta:generator>{}</meta:generator>\n",
            esc_text(v)
        ));
    }
    if let Some(kws) = &meta.keywords {
        for kw in kws.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            s.push_str(&format!(
                "  <meta:keyword>{}</meta:keyword>\n",
                esc_text(kw)
            ));
        }
    }
    for (k, v) in &meta.custom {
        s.push_str(&format!(
            r#"  <meta:user-defined meta:name="{}">{}</meta:user-defined>
"#,
            esc_attr(k),
            esc_text(v)
        ));
    }
    s.push_str(" </office:meta>\n</office:document-meta>\n");
    s
}

fn write_manifest(entries: &[(String, String)]) -> String {
    let mut s = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<manifest:manifest xmlns:manifest="{NS_MANIFEST}" manifest:version="1.3">
"#
    );
    for (path, mime) in entries {
        if path == "/" {
            s.push_str(&format!(
                r#" <manifest:file-entry manifest:full-path="/" manifest:version="1.3" manifest:media-type="{}"/>
"#,
                esc_attr(mime)
            ));
        } else {
            s.push_str(&format!(
                r#" <manifest:file-entry manifest:full-path="{}" manifest:media-type="{}"/>
"#,
                esc_attr(path),
                esc_attr(mime)
            ));
        }
    }
    s.push_str("</manifest:manifest>\n");
    s
}

fn guess_image_mime(path: &str) -> String {
    match path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png".into(),
        "jpg" | "jpeg" => "image/jpeg".into(),
        "gif" => "image/gif".into(),
        "svg" => "image/svg+xml".into(),
        "bmp" => "image/bmp".into(),
        "tif" | "tiff" => "image/tiff".into(),
        _ => "application/octet-stream".into(),
    }
}

pub(crate) fn esc_text(s: &str) -> String {
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

pub(crate) fn esc_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::text::{Block, Paragraph, Section};
    use docsai_model::MemoryAssetStore;
    use std::io::Cursor;

    #[test]
    fn writes_and_rereads_simple_paragraph() {
        let doc = Document::Text(TextDocument {
            meta: DocumentMeta {
                title: Some("Hi".into()),
                ..Default::default()
            },
            sections: vec![Section {
                blocks: vec![Block::Paragraph(Paragraph::text("Hello world"))],
                ..Default::default()
            }],
            ..Default::default()
        });
        let assets = MemoryAssetStore::new();
        let mut buf = Cursor::new(Vec::new());
        write_odt(&doc, &assets, &mut buf).unwrap();
        let bytes = buf.into_inner();
        let mut assets2 = MemoryAssetStore::new();
        let (back, _) = crate::odt::read(Cursor::new(bytes), &mut assets2).unwrap();
        let Document::Text(text) = back else {
            panic!("expected text");
        };
        assert_eq!(text.meta.title.as_deref(), Some("Hi"));
        let plain: String = text
            .blocks()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.plain_text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        assert!(plain.contains("Hello world"), "{plain}");
    }
}
