//! Front matter → IR catalogues and metadata.

use std::collections::BTreeMap;

use docsai_model::list::{ListCatalog, ListDef, ListId, ListLevel, NumFormat};
use docsai_model::style::{
    Align, DocDefaults, FontProps, LineHeight, ParaProps, Style, StyleCatalog, StyleId, StyleType,
    Underline, VertAlign,
};
use docsai_model::text::{DocumentMeta, Margins, Orientation, PageGeometry};
use docsai_model::units::{Length, Size};
use docsai_model::{Format, DOCMARK_VERSION};

use crate::error::ParseError;
use crate::yaml::{self, Value};

#[derive(Debug)]
pub struct FrontMatter {
    pub source_format: Format,
    pub meta: DocumentMeta,
    pub page: Option<PageGeometry>,
    pub styles: StyleCatalog,
    pub list_defs: ListCatalog,
}

impl Default for FrontMatter {
    fn default() -> Self {
        FrontMatter {
            source_format: Format::DocMark,
            meta: DocumentMeta::default(),
            page: None,
            styles: StyleCatalog::default(),
            list_defs: ListCatalog::default(),
        }
    }
}

/// Parses the text between the opening and closing `---` fences.
pub fn parse(text: &str, start_line: usize) -> Result<FrontMatter, ParseError> {
    let map = yaml::parse_document(text).map_err(|message| {
        // yaml errors already include line numbers relative to the body;
        // shift them if needed.
        let line = message
            .strip_prefix("line ")
            .and_then(|s| s.split(':').next())
            .and_then(|n| n.parse::<usize>().ok())
            .map(|n| start_line + n.saturating_sub(1))
            .unwrap_or(start_line);
        ParseError::front_matter(line, message)
    })?;

    let mut fm = FrontMatter {
        source_format: Format::Docx,
        ..Default::default()
    };

    if let Some(v) = map.get("docmark") {
        let version = v.as_str().unwrap_or("").trim();
        if !version.is_empty() && version != DOCMARK_VERSION {
            // Accept with a soft approach: still parse; future versions may
            // need migration, but v1.0 is the only supported contract today.
            if !version.starts_with('1') {
                return Err(ParseError::front_matter(
                    start_line,
                    format!("unsupported docmark version `{version}`"),
                ));
            }
        }
    }

    if let Some(v) = map.get("source-format").and_then(|v| v.as_str()) {
        fm.source_format = Format::parse(v).unwrap_or(Format::Docx);
    }

    fm.meta = read_meta(&map);
    if let Some(page) = map.get("page") {
        fm.page = Some(read_page(page, start_line)?);
    }
    if let Some(defaults) = map.get("style-defaults") {
        fm.styles.defaults = read_defaults(defaults);
    }
    if let Some(styles) = map.get("styles").and_then(|v| v.as_map()) {
        for (id, value) in styles {
            if let Some(style) = read_style(id, value) {
                fm.styles.insert(style);
            }
        }
    }
    if let Some(lists) = map.get("list-definitions").and_then(|v| v.as_map()) {
        for (id, value) in lists {
            if let Some(def) = read_list_def(value) {
                fm.list_defs.insert(ListId::new(id.clone()), def);
            }
        }
    }

    Ok(fm)
}

fn read_meta(map: &BTreeMap<String, Value>) -> DocumentMeta {
    let text = |key: &str| map.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let mut meta = DocumentMeta {
        title: text("title"),
        author: text("author"),
        last_modified_by: text("last-modified-by"),
        subject: text("subject"),
        keywords: text("keywords"),
        description: text("description"),
        created: map
            .get("created")
            .map(|v| v.string_or_empty())
            .filter(|s| !s.is_empty()),
        modified: map
            .get("modified")
            .map(|v| v.string_or_empty())
            .filter(|s| !s.is_empty()),
        language: text("language"),
        application: text("application"),
        custom: BTreeMap::new(),
    };
    if let Some(custom) = map.get("custom-properties").and_then(|v| v.as_map()) {
        for (k, v) in custom {
            meta.custom.insert(k.clone(), v.string_or_empty());
        }
    }
    meta
}

fn read_page(value: &Value, line: usize) -> Result<PageGeometry, ParseError> {
    let map = value
        .as_map()
        .ok_or_else(|| ParseError::front_matter(line, "page must be a mapping"))?;
    let mut page = PageGeometry {
        columns: 1,
        ..Default::default()
    };

    if let Some(size) = map.get("size") {
        if let Some(name) = size.as_str() {
            page.size = paper_size(name).ok_or_else(|| {
                ParseError::front_matter(line, format!("unknown paper size `{name}`"))
            })?;
        } else if let Some(m) = size.as_map() {
            page.size = Size::new(
                len_value(m.get("width")).unwrap_or(Length::ZERO),
                len_value(m.get("height")).unwrap_or(Length::ZERO),
            );
        }
    }

    if let Some(margins) = map.get("margins").and_then(|v| v.as_map()) {
        page.margins = Margins {
            top: len_value(margins.get("top")).unwrap_or(Length::ZERO),
            bottom: len_value(margins.get("bottom")).unwrap_or(Length::ZERO),
            left: len_value(margins.get("left")).unwrap_or(Length::ZERO),
            right: len_value(margins.get("right")).unwrap_or(Length::ZERO),
            header: len_value(margins.get("header")).unwrap_or(Length::ZERO),
            footer: len_value(margins.get("footer")).unwrap_or(Length::ZERO),
        };
    }

    if let Some(orient) = map.get("orientation").and_then(|v| v.as_str()) {
        page.orientation = match orient {
            "landscape" => Orientation::Landscape,
            _ => Orientation::Portrait,
        };
    }
    if let Some(cols) = map.get("columns").and_then(|v| v.as_u16()) {
        page.columns = cols.max(1);
    }
    if map.get("title-page").and_then(|v| v.as_bool()) == Some(true) {
        page.title_page = true;
    }
    Ok(page)
}

fn paper_size(name: &str) -> Option<Size> {
    // Portrait short × long in mm, matching PageGeometry::paper_name.
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

fn read_defaults(value: &Value) -> DocDefaults {
    let mut defaults = DocDefaults::default();
    if let Some(font) = value.get("font") {
        defaults.font = read_font(font);
    }
    if let Some(para) = value.get("paragraph") {
        defaults.paragraph = read_para(para);
    }
    defaults
}

fn read_style(id: &str, value: &Value) -> Option<Style> {
    let map = value.as_map()?;
    let style_type = match map
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("paragraph")
    {
        "character" => StyleType::Character,
        "table" => StyleType::Table,
        "numbering" => StyleType::Numbering,
        _ => StyleType::Paragraph,
    };
    let mut style = Style::new(id, style_type);
    if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
        style.name = name.to_string();
    }
    if let Some(based) = map.get("based-on").and_then(|v| v.as_str()) {
        style.based_on = Some(StyleId::new(based));
    }
    if let Some(next) = map.get("next").and_then(|v| v.as_str()) {
        style.next = Some(StyleId::new(next));
    }
    if map.get("default").and_then(|v| v.as_bool()) == Some(true) {
        style.is_default = true;
    }
    if let Some(font) = map.get("font") {
        style.font = read_font(font);
    }
    if let Some(para) = map.get("paragraph") {
        style.paragraph = read_para(para);
    }
    Some(style)
}

fn read_font(value: &Value) -> FontProps {
    let mut font = FontProps::default();
    let Some(map) = value.as_map() else {
        return font;
    };
    if let Some(name) = map.get("name").and_then(|v| v.as_str()) {
        font.name = Some(name.to_string());
    }
    if let Some(size) = map.get("size").and_then(len_value_ref) {
        font.size = Some(size);
    }
    if let Some(color) = map.get("color").and_then(|v| v.as_str()) {
        font.color = Some(color.to_string());
    }
    if let Some(b) = map.get("bold").and_then(|v| v.as_bool()) {
        font.bold = Some(b);
    }
    if let Some(i) = map.get("italic").and_then(|v| v.as_bool()) {
        font.italic = Some(i);
    }
    if let Some(s) = map.get("strike").and_then(|v| v.as_bool()) {
        font.strike = Some(s);
    }
    if let Some(u) = map.get("underline").and_then(|v| v.as_str()) {
        font.underline = Some(parse_underline(u));
    }
    if let Some(h) = map.get("highlight").and_then(|v| v.as_str()) {
        font.highlight = Some(h.to_string());
    }
    if let Some(v) = map.get("vertical-align").and_then(|v| v.as_str()) {
        font.vert_align = Some(match v {
            "superscript" => VertAlign::Superscript,
            "subscript" => VertAlign::Subscript,
            _ => VertAlign::Baseline,
        });
    }
    if let Some(b) = map.get("small-caps").and_then(|v| v.as_bool()) {
        font.small_caps = Some(b);
    }
    if let Some(b) = map.get("caps").and_then(|v| v.as_bool()) {
        font.caps = Some(b);
    }
    font
}

fn read_para(value: &Value) -> ParaProps {
    let mut para = ParaProps::default();
    let Some(map) = value.as_map() else {
        return para;
    };
    if let Some(a) = map.get("align").and_then(|v| v.as_str()) {
        para.align = Some(match a {
            "center" => Align::Center,
            "right" => Align::Right,
            "justify" => Align::Justify,
            _ => Align::Left,
        });
    }
    para.indent_left = map.get("indent-left").and_then(len_value_ref);
    para.indent_right = map.get("indent-right").and_then(len_value_ref);
    para.indent_first_line = map.get("indent-first-line").and_then(len_value_ref);
    para.indent_hanging = map.get("indent-hanging").and_then(len_value_ref);
    para.space_before = map.get("space-before").and_then(len_value_ref);
    para.space_after = map.get("space-after").and_then(len_value_ref);
    if let Some(lh) = map.get("line-height") {
        para.line_height = parse_line_height(lh);
    }
    if let Some(b) = map.get("keep-with-next").and_then(|v| v.as_bool()) {
        para.keep_with_next = Some(b);
    }
    if let Some(b) = map.get("page-break-before").and_then(|v| v.as_bool()) {
        para.page_break_before = Some(b);
    }
    if let Some(bg) = map.get("background").and_then(|v| v.as_str()) {
        para.background = Some(bg.to_string());
    }
    if let Some(level) = map.get("outline-level").and_then(|v| v.as_u8()) {
        para.outline_level = Some(level);
    }
    para
}

fn parse_line_height(value: &Value) -> Option<LineHeight> {
    if let Some(n) = value.as_f64() {
        // Multiple stored in thousandths.
        return Some(LineHeight::Multiple((n * 1000.0).round() as i32));
    }
    let text = value.as_str()?;
    if let Some(rest) = text.strip_prefix("exact ") {
        return len_str(rest).map(LineHeight::Exact);
    }
    if let Some(rest) = text.strip_prefix("at-least ") {
        return len_str(rest).map(LineHeight::AtLeast);
    }
    text.parse::<f64>()
        .ok()
        .map(|n| LineHeight::Multiple((n * 1000.0).round() as i32))
}

fn read_list_def(value: &Value) -> Option<ListDef> {
    let levels = value.get("levels")?.as_seq()?;
    let mut def = ListDef::default();
    for level in levels {
        let map = level.as_map()?;
        let format = match map
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("decimal")
        {
            "decimal" => NumFormat::Decimal,
            "lowerLetter" => NumFormat::LowerLetter,
            "upperLetter" => NumFormat::UpperLetter,
            "lowerRoman" => NumFormat::LowerRoman,
            "upperRoman" => NumFormat::UpperRoman,
            "bullet" => NumFormat::Bullet,
            "none" => NumFormat::None,
            other => NumFormat::Other(other.to_string()),
        };
        let text = map
            .get("text")
            .map(|v| v.string_or_empty())
            .unwrap_or_default();
        let mut item = ListLevel::new(format, text);
        if let Some(start) = map.get("start").and_then(|v| v.as_i64()) {
            item.start = Some(start as i32);
        }
        item.indent = map.get("indent").and_then(len_value_ref);
        item.hanging = map.get("hanging").and_then(len_value_ref);
        def.levels.push(item);
    }
    Some(def)
}

fn len_value(value: Option<&Value>) -> Option<Length> {
    value.and_then(len_value_ref)
}

fn len_value_ref(value: &Value) -> Option<Length> {
    match value {
        Value::String(s) => len_str(s),
        Value::Number(n) => Length::parse(&format!("{n}px")),
        _ => None,
    }
}

fn len_str(text: &str) -> Option<Length> {
    // Typographic measures in front matter may be bare `11pt`.
    Length::parse(text)
}

fn parse_underline(value: &str) -> Underline {
    match value {
        "none" => Underline::None,
        "double" => Underline::Double,
        "thick" => Underline::Thick,
        "dotted" => Underline::Dotted,
        "dashed" => Underline::Dashed,
        "wave" => Underline::Wave,
        _ => Underline::Single,
    }
}
