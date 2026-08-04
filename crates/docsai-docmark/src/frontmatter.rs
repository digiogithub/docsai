//! The YAML front matter (spec §2).
//!
//! Written by hand rather than through a YAML library: the spec demands byte
//! determinism, and the schema is small and fully known. Doing it here also
//! keeps a YAML serializer out of the dependency tree.

use docsai_model::list::{ListCatalog, NumFormat};
use docsai_model::sheet::Workbook;
use docsai_model::style::{DocDefaults, FontProps, ParaProps, Style, StyleCatalog, Underline};
use docsai_model::text::{DocumentMeta, PageGeometry};
use docsai_model::units::Length;
use docsai_model::{Document, DOCMARK_VERSION, DOCMARK_VERSION_ADDRESSED};

use crate::dict::AttrDict;
use crate::units::{len, pt};
use crate::{Fidelity, Options};

/// Writes the front matter block, delimiters included, ending in a blank line.
///
/// `next_id` is `Some` when the body carried node ids, which is what makes the
/// document DocMark 1.1 rather than 1.0 (spec §11.1). The declared version
/// tracks what was actually written, so a document without ids keeps parsing
/// under a 1.0 reader.
pub fn write(
    out: &mut String,
    doc: &Document,
    options: &Options,
    next_id: Option<u64>,
    dict: &AttrDict,
) {
    let fidelity = options.fidelity;
    if fidelity == Fidelity::Plain {
        return; // plain output is CommonMark only (spec §6)
    }

    out.push_str("---\n");
    let version = match next_id {
        Some(_) => DOCMARK_VERSION_ADDRESSED,
        None => DOCMARK_VERSION,
    };
    scalar(out, "docmark", &quoted(version));
    scalar(out, "source-format", options.source_format.as_str());
    if fidelity == Fidelity::Agent {
        // A projection says so: whoever reads this must know the document it
        // came from carries more, and that writing this back whole would lose
        // it (spec §6.1).
        scalar(out, "fidelity", "agent");
    }
    if let Some(next_id) = next_id {
        scalar(out, "next-id", &next_id.to_string());
    }
    write_meta(out, doc.meta());

    match doc {
        Document::Text(text) => {
            if let Some(section) = text.sections.first() {
                if fidelity.formatting() {
                    write_page(out, &section.page, options.precision);
                }
            }
            if fidelity == Fidelity::Full {
                write_styles(out, &text.styles, options.precision);
                write_lists(out, &text.list_defs, options.precision);
            }
        }
        Document::Workbook(book) => {
            write_workbook(out, book);
            if fidelity == Fidelity::Full {
                write_styles(out, &book.styles, options.precision);
            }
        }
    }
    write_attr_sets(out, dict);

    out.push_str("---\n\n");
}

/// The attribute-set dictionary (spec §3.7).
///
/// The value is the attribute-block payload verbatim, quoted, so the reader
/// gives it to the same parser that reads a block in the body — one syntax,
/// one implementation, and no second escaping scheme to get wrong.
fn write_attr_sets(out: &mut String, dict: &AttrDict) {
    if dict.is_empty() {
        return;
    }
    out.push_str("attribute-sets:\n");
    for (name, pattern) in dict.entries() {
        out.push_str(&format!("  {name}: {}\n", quoted(pattern)));
    }
}

fn write_meta(out: &mut String, meta: &DocumentMeta) {
    let text = |out: &mut String, key: &str, value: &Option<String>| {
        if let Some(value) = value {
            scalar(out, key, &quoted(value));
        }
    };
    text(out, "title", &meta.title);
    text(out, "author", &meta.author);
    text(out, "last-modified-by", &meta.last_modified_by);
    text(out, "subject", &meta.subject);
    text(out, "keywords", &meta.keywords);
    text(out, "description", &meta.description);
    // Timestamps are ISO-8601 and safe unquoted.
    if let Some(created) = &meta.created {
        scalar(out, "created", created);
    }
    if let Some(modified) = &meta.modified {
        scalar(out, "modified", modified);
    }
    text(out, "language", &meta.language);
    text(out, "application", &meta.application);

    if !meta.custom.is_empty() {
        out.push_str("custom-properties:\n");
        for (key, value) in &meta.custom {
            out.push_str(&format!("  {}: {}\n", yaml_key(key), quoted(value)));
        }
    }
}

fn write_page(out: &mut String, page: &PageGeometry, precision: u8) {
    if page.size.width.is_zero() && page.size.height.is_zero() {
        return;
    }
    out.push_str("page:\n");
    match page.paper_name() {
        Some(name) => out.push_str(&format!("  size: {name}\n")),
        None => out.push_str(&format!(
            "  size: {{ width: {}, height: {} }}\n",
            len(page.size.width, precision),
            len(page.size.height, precision)
        )),
    }
    let m = &page.margins;
    out.push_str(&format!(
        "  margins: {{ top: {}, bottom: {}, left: {}, right: {}, header: {}, footer: {} }}\n",
        len(m.top, precision),
        len(m.bottom, precision),
        len(m.left, precision),
        len(m.right, precision),
        len(m.header, precision),
        len(m.footer, precision)
    ));
    out.push_str(&format!("  orientation: {}\n", page.orientation.as_str()));
    if page.columns > 1 {
        out.push_str(&format!("  columns: {}\n", page.columns));
    }
    if page.title_page {
        out.push_str("  title-page: true\n");
    }
}

fn write_workbook(out: &mut String, book: &Workbook) {
    if book.active_sheet.is_none() && book.defined_names.is_empty() {
        return;
    }
    out.push_str("workbook:\n");
    if let Some(active) = &book.active_sheet {
        out.push_str(&format!("  active-sheet: {}\n", quoted(active)));
    }
    if !book.defined_names.is_empty() {
        out.push_str("  defined-names:\n");
        for name in &book.defined_names {
            out.push_str(&format!(
                "    {}: {}\n",
                yaml_key(&name.name),
                quoted(&name.refers_to)
            ));
        }
    }
}

fn write_styles(out: &mut String, catalog: &StyleCatalog, precision: u8) {
    if !catalog.defaults.is_empty() {
        write_defaults(out, &catalog.defaults, precision);
    }
    if catalog.styles.is_empty() {
        return;
    }
    out.push_str("styles:\n");
    for style in catalog.styles.values() {
        write_style(out, style, catalog, precision);
    }
}

fn write_defaults(out: &mut String, defaults: &DocDefaults, precision: u8) {
    out.push_str("style-defaults:\n");
    if let Some(font) = font_flow(&defaults.font) {
        out.push_str(&format!("  font: {font}\n"));
    }
    if let Some(para) = para_flow(&defaults.paragraph, precision) {
        out.push_str(&format!("  paragraph: {para}\n"));
    }
}

fn write_style(out: &mut String, style: &Style, catalog: &StyleCatalog, precision: u8) {
    out.push_str(&format!("  {}:\n", yaml_key(style.id.as_str())));
    out.push_str(&format!("    type: {}\n", style.style_type.as_str()));
    if style.name != style.id.as_str() {
        out.push_str(&format!("    name: {}\n", quoted(&style.name)));
    }
    if let Some(based_on) = &style.based_on {
        out.push_str(&format!("    based-on: {}\n", yaml_key(based_on.as_str())));
    }
    if let Some(next) = &style.next {
        out.push_str(&format!("    next: {}\n", yaml_key(next.as_str())));
    }
    if style.is_default {
        out.push_str("    default: true\n");
    }
    // Only what the chain does not already say (spec §3.1). Readers hand us
    // deltas when the source format stores deltas, but not every one does —
    // `.doc` and `.xls` resolve as they go — and the parser rebuilds the same
    // catalogue either way, since dropping a value equal to the inherited one
    // cannot change what it resolves to.
    let inherited = catalog.resolve(style.based_on.as_ref());
    if let Some(font) = font_flow(&style.font.minus(&inherited.font)) {
        out.push_str(&format!("    font: {font}\n"));
    }
    if let Some(para) = para_flow(&style.paragraph.minus(&inherited.paragraph), precision) {
        out.push_str(&format!("    paragraph: {para}\n"));
    }
}

/// A `FontProps` as a YAML flow mapping, or `None` when it carries nothing.
///
/// Field order is fixed by this function, not alphabetical: it mirrors the
/// spec's example and stays stable across releases.
pub fn font_flow(font: &FontProps) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(name) = &font.name {
        parts.push(format!("name: {}", quoted(name)));
    }
    if let Some(size) = font.size {
        parts.push(format!("size: {}", pt(size)));
    }
    if let Some(color) = &font.color {
        parts.push(format!("color: {}", quoted(color)));
    }
    push_flag(&mut parts, "bold", font.bold);
    push_flag(&mut parts, "italic", font.italic);
    push_flag(&mut parts, "strike", font.strike);
    if let Some(underline) = font.underline {
        parts.push(format!("underline: {}", underline_name(underline)));
    }
    if let Some(highlight) = &font.highlight {
        parts.push(format!("highlight: {}", quoted(highlight)));
    }
    if let Some(vert) = font.vert_align {
        parts.push(format!(
            "vertical-align: {}",
            match vert {
                docsai_model::style::VertAlign::Superscript => "superscript",
                docsai_model::style::VertAlign::Subscript => "subscript",
                docsai_model::style::VertAlign::Baseline => "baseline",
            }
        ));
    }
    push_flag(&mut parts, "small-caps", font.small_caps);
    push_flag(&mut parts, "caps", font.caps);
    flow(parts)
}

/// A `ParaProps` as a YAML flow mapping, or `None` when it carries nothing.
pub fn para_flow(para: &ParaProps, precision: u8) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(align) = para.align {
        parts.push(format!("align: {}", align.as_str()));
    }
    push_len(&mut parts, "indent-left", para.indent_left, precision);
    push_len(&mut parts, "indent-right", para.indent_right, precision);
    push_len(
        &mut parts,
        "indent-first-line",
        para.indent_first_line,
        precision,
    );
    push_len(&mut parts, "indent-hanging", para.indent_hanging, precision);
    if let Some(value) = para.space_before {
        parts.push(format!("space-before: {}", pt(value)));
    }
    if let Some(value) = para.space_after {
        parts.push(format!("space-after: {}", pt(value)));
    }
    if let Some(line) = para.line_height {
        parts.push(format!("line-height: {}", line_height(line)));
    }
    push_flag(&mut parts, "keep-with-next", para.keep_with_next);
    push_flag(&mut parts, "page-break-before", para.page_break_before);
    if let Some(background) = &para.background {
        parts.push(format!("background: {}", quoted(background)));
    }
    if let Some(level) = para.outline_level {
        parts.push(format!("outline-level: {level}"));
    }
    flow(parts)
}

fn line_height(value: docsai_model::style::LineHeight) -> String {
    use docsai_model::style::LineHeight::*;
    match value {
        Multiple(thousandths) => docsai_model::units::trim_float(thousandths as f64 / 1000.0, 3),
        Exact(length) => format!("exact {}", pt(length)),
        AtLeast(length) => format!("at-least {}", pt(length)),
    }
}

fn write_lists(out: &mut String, catalog: &ListCatalog, precision: u8) {
    if catalog.is_empty() {
        return;
    }
    out.push_str("list-definitions:\n");
    for (id, def) in &catalog.defs {
        out.push_str(&format!("  {}:\n    levels:\n", yaml_key(id.as_str())));
        for level in &def.levels {
            let mut parts = vec![format!("format: {}", num_format(&level.format))];
            if !level.text.is_empty() {
                parts.push(format!("text: {}", quoted(&level.text)));
            }
            if let Some(start) = level.start {
                parts.push(format!("start: {start}"));
            }
            push_len(&mut parts, "indent", level.indent, precision);
            push_len(&mut parts, "hanging", level.hanging, precision);
            out.push_str(&format!("      - {{ {} }}\n", parts.join(", ")));
        }
    }
}

fn num_format(format: &NumFormat) -> String {
    let name = format.as_str();
    if name.is_empty() {
        "none".into()
    } else {
        name.to_string()
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

fn push_flag(parts: &mut Vec<String>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        parts.push(format!("{key}: {value}"));
    }
}

fn push_len(parts: &mut Vec<String>, key: &str, value: Option<Length>, precision: u8) {
    if let Some(value) = value {
        parts.push(format!("{key}: {}", len(value, precision)));
    }
}

fn flow(parts: Vec<String>) -> Option<String> {
    if parts.is_empty() {
        None
    } else {
        Some(format!("{{ {} }}", parts.join(", ")))
    }
}

fn scalar(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("{key}: {value}\n"));
}

/// A YAML double-quoted scalar.
fn quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A mapping key: bare when it is a plain identifier, quoted otherwise.
fn yaml_key(key: &str) -> String {
    let plain = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        && !key.chars().next().is_some_and(|c| c.is_ascii_digit());
    if plain {
        key.to_string()
    } else {
        quoted(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::style::{Align, StyleId, StyleType};
    use docsai_model::text::{Margins, Orientation, Section, TextDocument};
    use docsai_model::units::Size;

    fn render(doc: &Document) -> String {
        let mut out = String::new();
        write(&mut out, doc, &Options::default(), None, &AttrDict::off());
        out
    }

    #[test]
    fn writes_the_mandatory_header_and_the_metadata() {
        let doc = Document::Text(TextDocument {
            meta: DocumentMeta {
                title: Some("Informe \"Anual\"".into()),
                author: Some("Ana Pérez".into()),
                created: Some("2026-03-01T10:00:00Z".into()),
                language: Some("es-ES".into()),
                custom: [("Departamento".to_string(), "Ventas".to_string())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        });
        let out = render(&doc);
        assert!(out.starts_with("---\ndocmark: \"1.0\"\nsource-format: docx\n"));
        assert!(out.contains(r#"title: "Informe \"Anual\"""#));
        assert!(out.contains("created: 2026-03-01T10:00:00Z\n"));
        assert!(out.contains("custom-properties:\n  Departamento: \"Ventas\"\n"));
        assert!(out.ends_with("---\n\n"));
    }

    #[test]
    fn recognises_the_paper_size_and_writes_the_margins() {
        let doc = Document::Text(TextDocument {
            sections: vec![Section {
                page: PageGeometry {
                    size: Size::new(Length::from_twips(11906), Length::from_twips(16838)),
                    margins: Margins {
                        top: Length::from_cm(2.5),
                        bottom: Length::from_cm(2.5),
                        left: Length::from_cm(3.0),
                        right: Length::from_cm(3.0),
                        header: Length::from_cm(1.25),
                        footer: Length::from_cm(1.25),
                    },
                    orientation: Orientation::Landscape,
                    columns: 2,
                    title_page: true,
                },
                ..Default::default()
            }],
            ..Default::default()
        });
        let out = render(&doc);
        assert!(out.contains("page:\n  size: A4\n"));
        assert!(out.contains("margins: { top: 2.5cm, bottom: 2.5cm, left: 3cm, right: 3cm"));
        assert!(out.contains("orientation: landscape\n"));
        assert!(out.contains("columns: 2\n"));
        assert!(out.contains("title-page: true\n"));
    }

    #[test]
    fn writes_the_style_catalogue_with_its_inheritance() {
        let mut styles = StyleCatalog::default();
        styles.defaults.font.size = Some(Length::from_pt(11.0));
        let mut heading = Style::new("Heading1", StyleType::Paragraph);
        heading.name = "heading 1".into();
        heading.based_on = Some(StyleId::new("Normal"));
        heading.font.name = Some("Calibri Light".into());
        heading.font.size = Some(Length::from_pt(16.0));
        heading.font.color = Some("#2E74B5".into());
        heading.paragraph.align = Some(Align::Center);
        heading.paragraph.outline_level = Some(0);
        styles.insert(heading);

        let doc = Document::Text(TextDocument {
            styles,
            ..Default::default()
        });
        let out = render(&doc);
        assert!(out.contains("style-defaults:\n  font: { size: 11pt }\n"));
        assert!(out.contains("styles:\n  Heading1:\n    type: paragraph\n"));
        assert!(out.contains("    name: \"heading 1\"\n"));
        assert!(out.contains("    based-on: Normal\n"));
        assert!(
            out.contains("    font: { name: \"Calibri Light\", size: 16pt, color: \"#2E74B5\" }")
        );
        assert!(out.contains("    paragraph: { align: center, outline-level: 0 }\n"));
    }

    #[test]
    fn plain_fidelity_has_no_front_matter() {
        let doc = Document::Text(TextDocument::default());
        let mut out = String::new();
        write(
            &mut out,
            &doc,
            &Options {
                fidelity: Fidelity::Plain,
                ..Options::default()
            },
            None,
            &AttrDict::off(),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn standard_fidelity_drops_the_catalogues() {
        let mut styles = StyleCatalog::default();
        styles.insert(Style::new("Heading1", StyleType::Paragraph));
        let doc = Document::Text(TextDocument {
            styles,
            ..Default::default()
        });
        let mut out = String::new();
        write(
            &mut out,
            &doc,
            &Options {
                fidelity: Fidelity::Standard,
                ..Options::default()
            },
            None,
            &AttrDict::off(),
        );
        assert!(!out.contains("styles:"));
        assert!(out.contains("docmark:"));
    }

    #[test]
    fn keys_that_are_not_identifiers_get_quoted() {
        assert_eq!(yaml_key("Heading1"), "Heading1");
        assert_eq!(yaml_key("mi estilo"), "\"mi estilo\"");
        assert_eq!(yaml_key("2col"), "\"2col\"");
    }
}
