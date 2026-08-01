//! `word/styles.xml` → [`StyleCatalog`].

use docsai_model::style::{Style, StyleCatalog, StyleId, StyleType};

use super::format::{paragraph_props, run_props};
use crate::xml::Element;

/// Reads the style catalogue, including `w:docDefaults`.
pub fn read_styles(root: &Element) -> StyleCatalog {
    let mut catalog = StyleCatalog::default();

    if let Some(defaults) = root.child("docDefaults") {
        if let Some(rpr) = defaults.path(&["rPrDefault", "rPr"]) {
            catalog.defaults.font = run_props(rpr);
        }
        if let Some(ppr) = defaults.path(&["pPrDefault", "pPr"]) {
            catalog.defaults.paragraph = paragraph_props(ppr);
        }
    }

    for element in root.children_named("style") {
        let Some(id) = element.attr("styleId") else {
            continue;
        };
        let Some(style_type) = style_type(element.attr("type")) else {
            continue;
        };
        let mut style = Style::new(id, style_type);
        style.name = element
            .path(&["name"])
            .and_then(|e| e.attr("val"))
            .unwrap_or(id)
            .to_string();
        style.based_on = element
            .child("basedOn")
            .and_then(|e| e.attr("val"))
            .map(StyleId::new);
        style.next = element
            .child("next")
            .and_then(|e| e.attr("val"))
            .map(StyleId::new);
        style.is_default = element
            .attr("default")
            .is_some_and(|v| v != "0" && v != "false");
        if let Some(rpr) = element.child("rPr") {
            style.font = run_props(rpr);
        }
        if let Some(ppr) = element.child("pPr") {
            style.paragraph = paragraph_props(ppr);
        }
        catalog.insert(style);
    }

    catalog
}

fn style_type(value: Option<&str>) -> Option<StyleType> {
    match value {
        Some("paragraph") | None => Some(StyleType::Paragraph),
        Some("character") => Some(StyleType::Character),
        Some("table") => Some(StyleType::Table),
        Some("numbering") => Some(StyleType::Numbering),
        Some(_) => None,
    }
}

/// The heading depth a paragraph style implies, if any.
///
/// Word marks headings two ways and documents in the wild use both: the
/// outline level of the style's `pPr`, and the conventional style name
/// (`heading 1`) or id (`Heading1`).
pub fn heading_level(catalog: &StyleCatalog, id: &StyleId) -> Option<u8> {
    let style = catalog.get(id)?;
    // The outline level is inherited, so resolve the chain rather than reading
    // this style's own `pPr` only.
    if let Some(level) = catalog.resolve(Some(id)).paragraph.outline_level {
        if level <= 8 {
            return Some(level + 1);
        }
    }
    for candidate in [style.name.as_str(), id.as_str()] {
        let lowercase = candidate.to_ascii_lowercase();
        let Some(rest) = lowercase.strip_prefix("heading") else {
            continue;
        };
        if let Ok(level) = rest.trim().parse::<u8>() {
            if (1..=9).contains(&level) {
                return Some(level);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::units::Length;

    const STYLES: &str = r#"<w:styles xmlns:w="urn:w">
      <w:docDefaults>
        <w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri"/><w:sz w:val="22"/></w:rPr></w:rPrDefault>
        <w:pPrDefault><w:pPr><w:spacing w:after="160"/></w:pPr></w:pPrDefault>
      </w:docDefaults>
      <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
      <w:style w:type="paragraph" w:styleId="Heading1">
        <w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/>
        <w:pPr><w:outlineLvl w:val="0"/></w:pPr>
        <w:rPr><w:color w:val="2E74B5"/><w:sz w:val="32"/></w:rPr>
      </w:style>
      <w:style w:type="character" w:styleId="Enfatico"><w:name w:val="Enfatico"/>
        <w:rPr><w:i/></w:rPr></w:style>
      <w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/></w:style>
    </w:styles>"#;

    fn catalog() -> StyleCatalog {
        read_styles(&Element::parse("styles.xml", STYLES.as_bytes()).unwrap())
    }

    #[test]
    fn reads_doc_defaults_as_the_bottom_of_the_cascade() {
        let cat = catalog();
        assert_eq!(cat.defaults.font.name.as_deref(), Some("Calibri"));
        assert_eq!(cat.defaults.font.size, Some(Length::from_pt(11.0)));
        assert_eq!(
            cat.defaults.paragraph.space_after,
            Some(Length::from_twips(160))
        );
    }

    #[test]
    fn reads_every_style_kind_with_its_inheritance() {
        let cat = catalog();
        assert_eq!(cat.styles.len(), 4);
        let h1 = cat.get(&StyleId::new("Heading1")).unwrap();
        assert_eq!(h1.name, "heading 1");
        assert_eq!(h1.based_on, Some(StyleId::new("Normal")));
        assert_eq!(h1.next, Some(StyleId::new("Normal")));
        assert_eq!(h1.style_type, StyleType::Paragraph);
        assert!(cat.default_paragraph_style().is_some());
        assert_eq!(
            cat.get(&StyleId::new("Enfatico")).unwrap().style_type,
            StyleType::Character
        );
    }

    #[test]
    fn resolution_reaches_the_document_defaults() {
        let resolved = catalog().resolve(Some(&StyleId::new("Heading1")));
        assert_eq!(resolved.font.name.as_deref(), Some("Calibri"), "inherited");
        assert_eq!(
            resolved.font.size,
            Some(Length::from_pt(16.0)),
            "overridden"
        );
    }

    #[test]
    fn heading_level_comes_from_outline_level_or_the_name() {
        let cat = catalog();
        assert_eq!(heading_level(&cat, &StyleId::new("Heading1")), Some(1));
        assert_eq!(heading_level(&cat, &StyleId::new("Normal")), None);

        let xml = r#"<w:styles xmlns:w="urn:w">
            <w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/></w:style>
        </w:styles>"#;
        let named = read_styles(&Element::parse("s.xml", xml.as_bytes()).unwrap());
        assert_eq!(heading_level(&named, &StyleId::new("Heading3")), Some(3));
    }
}
