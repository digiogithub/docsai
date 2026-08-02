//! Front matter → IR (spec §2).
//!
//! The exact inverse of [`crate::frontmatter`]. Where that module decides how a
//! field is written, this one decides how it is read; the two are meant to be
//! edited together, and the idempotence test over the goldens is what catches
//! it when they are not.

use docsai_model::list::{ListCatalog, ListDef, ListId, ListLevel, NumFormat};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::style::{
    Align, DocDefaults, FontProps, LineHeight, ParaProps, Style, StyleCatalog, StyleId, StyleType,
    Underline, VertAlign,
};
use docsai_model::text::{DocumentMeta, Margins, Orientation, PageGeometry};
use docsai_model::units::{Length, Size};
use docsai_model::Format;

use super::yaml::Yaml;
use crate::units::parse_len;

/// Everything the front matter contributes to the document.
#[derive(Debug, Default)]
pub struct FrontMatter {
    pub meta: DocumentMeta,
    pub page: PageGeometry,
    pub styles: StyleCatalog,
    pub lists: ListCatalog,
    pub source_format: Option<Format>,
    /// Version declared by the file, for the caller to check.
    pub version: Option<String>,
}

/// Keys this reader understands. Anything else is reported rather than
/// silently ignored: the format is forward-compatible (spec §2), but a key we
/// drop is still information the user should hear about.
const KNOWN_KEYS: &[&str] = &[
    "docmark",
    "source-format",
    "title",
    "author",
    "last-modified-by",
    "subject",
    "keywords",
    "description",
    "created",
    "modified",
    "language",
    "application",
    "custom-properties",
    "page",
    "workbook",
    "style-defaults",
    "styles",
    "list-definitions",
];

/// Reads a parsed front matter into its IR pieces.
pub fn read(yaml: &Yaml, report: &mut ConversionReport) -> FrontMatter {
    let mut front = FrontMatter {
        version: yaml.string("docmark"),
        source_format: yaml.str("source-format").and_then(Format::parse),
        ..Default::default()
    };

    if let Some(map) = yaml.as_map() {
        for key in map.keys() {
            if !KNOWN_KEYS.contains(&key.as_str()) {
                report.warn(Warning::Degraded {
                    what: format!("front matter key `{key}`"),
                    why: "not part of DocMark 1.0; kept out of the document".into(),
                });
            }
        }
    }

    read_meta(yaml, &mut front.meta);
    if let Some(page) = yaml.get("page") {
        front.page = read_page(page);
    }
    read_styles(yaml, &mut front.styles);
    read_lists(yaml, &mut front.lists);
    front
}

fn read_meta(yaml: &Yaml, meta: &mut DocumentMeta) {
    meta.title = yaml.string("title");
    meta.author = yaml.string("author");
    meta.last_modified_by = yaml.string("last-modified-by");
    meta.subject = yaml.string("subject");
    meta.keywords = yaml.string("keywords");
    meta.description = yaml.string("description");
    meta.created = yaml.string("created");
    meta.modified = yaml.string("modified");
    meta.language = yaml.string("language");
    meta.application = yaml.string("application");
    if let Some(custom) = yaml.get("custom-properties").and_then(Yaml::as_map) {
        for (key, value) in custom {
            if let Some(value) = value.as_str() {
                meta.custom.insert(key.clone(), value.to_string());
            }
        }
    }
}

/// Standard paper sizes, so `size: A4` reads back to the same EMU the writer
/// recognised it from. Values in millimetres, portrait.
const PAPERS: &[(&str, f64, f64)] = &[
    ("A3", 297.0, 420.0),
    ("A4", 210.0, 297.0),
    ("A5", 148.0, 210.0),
    ("Letter", 215.9, 279.4),
    ("Legal", 215.9, 355.6),
    ("Tabloid", 279.4, 431.8),
];

pub fn read_page(yaml: &Yaml) -> PageGeometry {
    let mut page = PageGeometry {
        orientation: match yaml.str("orientation") {
            Some("landscape") => Orientation::Landscape,
            _ => Orientation::Portrait,
        },
        columns: yaml
            .str("columns")
            .and_then(|c| c.parse().ok())
            .unwrap_or(1),
        title_page: yaml.bool("title-page").unwrap_or(false),
        ..Default::default()
    };

    match yaml.get("size") {
        Some(Yaml::Scalar(name)) => {
            if let Some((_, w, h)) = PAPERS.iter().find(|(n, _, _)| n == name) {
                let (short, long) = (Length::from_mm(*w), Length::from_mm(*h));
                page.size = match page.orientation {
                    Orientation::Portrait => Size::new(short, long),
                    Orientation::Landscape => Size::new(long, short),
                };
            }
        }
        Some(size) => {
            page.size = Size::new(
                size.str("width").and_then(parse_len).unwrap_or_default(),
                size.str("height").and_then(parse_len).unwrap_or_default(),
            );
        }
        None => {}
    }

    if let Some(margins) = yaml.get("margins") {
        let get = |key: &str| margins.str(key).and_then(parse_len).unwrap_or_default();
        page.margins = Margins {
            top: get("top"),
            right: get("right"),
            bottom: get("bottom"),
            left: get("left"),
            header: get("header"),
            footer: get("footer"),
        };
    }
    page
}

fn read_styles(yaml: &Yaml, catalog: &mut StyleCatalog) {
    if let Some(defaults) = yaml.get("style-defaults") {
        catalog.defaults = DocDefaults {
            font: defaults.get("font").map(read_font).unwrap_or_default(),
            paragraph: defaults.get("paragraph").map(read_para).unwrap_or_default(),
        };
    }
    let Some(styles) = yaml.get("styles").and_then(Yaml::as_map) else {
        return;
    };
    for (id, body) in styles {
        let mut style = Style::new(id.clone(), read_style_type(body.str("type")));
        if let Some(name) = body.string("name") {
            style.name = name;
        }
        style.based_on = body.string("based-on").map(StyleId::new);
        style.next = body.string("next").map(StyleId::new);
        style.is_default = body.bool("default").unwrap_or(false);
        style.font = body.get("font").map(read_font).unwrap_or_default();
        style.paragraph = body.get("paragraph").map(read_para).unwrap_or_default();
        catalog.insert(style);
    }
}

fn read_style_type(value: Option<&str>) -> StyleType {
    match value {
        Some("character") => StyleType::Character,
        Some("table") => StyleType::Table,
        Some("numbering") => StyleType::Numbering,
        // `paragraph` is both the written default and the sane fallback.
        _ => StyleType::Paragraph,
    }
}

/// Reads a `FontProps` flow mapping.
pub fn read_font(yaml: &Yaml) -> FontProps {
    FontProps {
        name: yaml.string("name"),
        size: yaml.str("size").and_then(parse_len),
        bold: yaml.bool("bold"),
        italic: yaml.bool("italic"),
        strike: yaml.bool("strike"),
        underline: yaml.str("underline").and_then(parse_underline),
        color: yaml.string("color"),
        highlight: yaml.string("highlight"),
        vert_align: yaml.str("vertical-align").and_then(|v| match v {
            "superscript" => Some(VertAlign::Superscript),
            "subscript" => Some(VertAlign::Subscript),
            "baseline" => Some(VertAlign::Baseline),
            _ => None,
        }),
        small_caps: yaml.bool("small-caps"),
        caps: yaml.bool("caps"),
    }
}

pub fn parse_underline(value: &str) -> Option<Underline> {
    Some(match value {
        "none" => Underline::None,
        "single" => Underline::Single,
        "double" => Underline::Double,
        "thick" => Underline::Thick,
        "dotted" => Underline::Dotted,
        "dashed" => Underline::Dashed,
        "wave" => Underline::Wave,
        _ => return None,
    })
}

pub fn parse_align(value: &str) -> Option<Align> {
    Some(match value {
        "left" => Align::Left,
        "center" => Align::Center,
        "right" => Align::Right,
        "justify" => Align::Justify,
        _ => return None,
    })
}

/// Reads a `ParaProps` flow mapping.
pub fn read_para(yaml: &Yaml) -> ParaProps {
    ParaProps {
        align: yaml.str("align").and_then(parse_align),
        indent_left: yaml.str("indent-left").and_then(parse_len),
        indent_right: yaml.str("indent-right").and_then(parse_len),
        indent_first_line: yaml.str("indent-first-line").and_then(parse_len),
        indent_hanging: yaml.str("indent-hanging").and_then(parse_len),
        space_before: yaml.str("space-before").and_then(parse_len),
        space_after: yaml.str("space-after").and_then(parse_len),
        line_height: yaml.str("line-height").and_then(parse_line_height),
        keep_with_next: yaml.bool("keep-with-next"),
        page_break_before: yaml.bool("page-break-before"),
        background: yaml.string("background"),
        outline_level: yaml.str("outline-level").and_then(|v| v.parse().ok()),
    }
}

/// `1.079`, `exact 12pt` or `at-least 14pt`.
fn parse_line_height(value: &str) -> Option<LineHeight> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("exact ") {
        return parse_len(rest).map(LineHeight::Exact);
    }
    if let Some(rest) = value.strip_prefix("at-least ") {
        return parse_len(rest).map(LineHeight::AtLeast);
    }
    let multiple: f64 = value.parse().ok()?;
    Some(LineHeight::Multiple((multiple * 1000.0).round() as i32))
}

fn read_lists(yaml: &Yaml, catalog: &mut ListCatalog) {
    let Some(defs) = yaml.get("list-definitions").and_then(Yaml::as_map) else {
        return;
    };
    for (id, body) in defs {
        let mut def = ListDef::default();
        for level in body.get("levels").and_then(Yaml::as_seq).unwrap_or(&[]) {
            def.levels.push(ListLevel {
                format: level
                    .str("format")
                    .map(NumFormat::from_ooxml)
                    .unwrap_or(NumFormat::None),
                text: level.string("text").unwrap_or_default(),
                start: level.str("start").and_then(|s| s.parse().ok()),
                indent: level.str("indent").and_then(parse_len),
                hanging: level.str("hanging").and_then(parse_len),
            });
        }
        catalog.insert(ListId::new(id.clone()), def);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::yaml;

    fn front(source: &str) -> (FrontMatter, ConversionReport) {
        let mut report = ConversionReport::new();
        let yaml = yaml::parse(source).expect("parses");
        (read(&yaml, &mut report), report)
    }

    #[test]
    fn reads_the_metadata_and_the_declared_format() {
        let (front, _) = front(
            r#"docmark: "1.0"
source-format: docx
title: "Informe \"Anual\""
author: "Ana Perez"
created: 2026-03-01T10:00:00Z
language: es-ES
custom-properties:
  Departamento: "Ventas"
"#,
        );
        assert_eq!(front.version.as_deref(), Some("1.0"));
        assert_eq!(front.source_format, Some(Format::Docx));
        assert_eq!(front.meta.title.as_deref(), Some(r#"Informe "Anual""#));
        assert_eq!(front.meta.created.as_deref(), Some("2026-03-01T10:00:00Z"));
        assert_eq!(front.meta.custom.get("Departamento").unwrap(), "Ventas");
    }

    #[test]
    fn a_paper_name_reads_back_to_the_size_it_stood_for() {
        let (portrait, _) = front("page:\n  size: A4\n  orientation: portrait\n");
        assert_eq!(portrait.page.paper_name(), Some("A4"));

        let (landscape, _) = front("page:\n  size: A4\n  orientation: landscape\n");
        assert!(
            landscape.page.size.width > landscape.page.size.height,
            "landscape swaps the axes, and the paper is still A4"
        );
        assert_eq!(landscape.page.paper_name(), Some("A4"));
    }

    #[test]
    fn margins_keep_their_units() {
        let (front, _) = front(
            "page:\n  margins: { top: 70.85pt, bottom: 2.5cm, left: 48px, right: 0px, header: 0px, footer: 0px }\n",
        );
        assert_eq!(front.page.margins.top, Length::from_twips(1417));
        assert_eq!(front.page.margins.bottom, Length::from_cm(2.5));
        assert_eq!(front.page.margins.left, Length::from_px(48.0));
    }

    #[test]
    fn reads_the_style_catalogue_with_its_inheritance() {
        let (front, _) = front(
            r##"style-defaults:
  font: { name: "Calibri", size: 11pt }
  paragraph: { space-after: 8pt, line-height: 1.079 }
styles:
  Heading1:
    type: paragraph
    name: "heading 1"
    based-on: Normal
    font: { name: "Calibri Light", size: 16pt, color: "#2E74B5" }
    paragraph: { align: center, keep-with-next: true, outline-level: 0 }
  Normal:
    type: paragraph
    default: true
  Enfatico:
    type: character
    font: { italic: true, underline: double }
"##,
        );
        assert_eq!(front.styles.defaults.font.size, Some(Length::from_pt(11.0)));
        assert_eq!(
            front.styles.defaults.paragraph.line_height,
            Some(LineHeight::Multiple(1079))
        );

        let heading = front
            .styles
            .get(&StyleId::new("Heading1"))
            .expect("Heading1");
        assert_eq!(heading.name, "heading 1");
        assert_eq!(heading.based_on.as_ref().unwrap().as_str(), "Normal");
        assert_eq!(heading.font.color.as_deref(), Some("#2E74B5"));
        assert_eq!(heading.paragraph.align, Some(Align::Center));
        assert_eq!(heading.paragraph.outline_level, Some(0));

        assert!(front.styles.default_paragraph_style().is_some());
        let enfatico = front
            .styles
            .get(&StyleId::new("Enfatico"))
            .expect("Enfatico");
        assert_eq!(enfatico.style_type, StyleType::Character);
        assert_eq!(enfatico.font.underline, Some(Underline::Double));
    }

    #[test]
    fn a_style_without_a_name_keeps_its_id() {
        // The writer omits `name` when it equals the id, so reading must not
        // leave the name empty.
        let (front, _) = front("styles:\n  Normal:\n    type: paragraph\n");
        assert_eq!(
            front.styles.get(&StyleId::new("Normal")).unwrap().name,
            "Normal"
        );
    }

    #[test]
    fn reads_the_list_definitions() {
        let (front, _) = front(
            r#"list-definitions:
  L1:
    levels:
      - { format: decimal, text: "%1.", start: 1, indent: 48px, hanging: 24px }
      - { format: lowerLetter, text: "%2)" }
  L2:
    levels:
      - { format: bullet, text: "•" }
"#,
        );
        let l1 = front.lists.get(&ListId::new("L1")).expect("L1");
        assert_eq!(l1.levels.len(), 2);
        assert_eq!(l1.levels[0].format, NumFormat::Decimal);
        assert_eq!(l1.levels[0].start, Some(1));
        assert_eq!(l1.levels[0].indent, Some(Length::from_px(48.0)));
        assert!(l1.is_ordered_at(0));
        assert!(!front
            .lists
            .get(&ListId::new("L2"))
            .unwrap()
            .is_ordered_at(0));
    }

    #[test]
    fn line_heights_read_in_all_three_shapes() {
        assert_eq!(parse_line_height("1.079"), Some(LineHeight::Multiple(1079)));
        assert_eq!(
            parse_line_height("exact 12pt"),
            Some(LineHeight::Exact(Length::from_pt(12.0)))
        );
        assert_eq!(
            parse_line_height("at-least 14pt"),
            Some(LineHeight::AtLeast(Length::from_pt(14.0)))
        );
        assert_eq!(parse_line_height("alto"), None);
    }

    #[test]
    fn an_unknown_key_is_reported_rather_than_dropped_in_silence() {
        let (_, report) = front("docmark: \"1.0\"\ninvento-futuro: 3\n");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.message().contains("invento-futuro")),
            "an unrecognised key must reach the report: {:?}",
            report.warnings
        );
    }
}
