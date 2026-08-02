//! ODF styles: named styles + automatic-style de-automatization.

use std::collections::BTreeMap;

use docsai_model::list::{ListCatalog, ListDef, ListId, ListLevel, NumFormat};
use docsai_model::style::{FontProps, ParaProps, Style, StyleCatalog, StyleId, StyleType};
use docsai_model::units::Length;

use crate::format::{paragraph_properties, text_properties};
use crate::length::parse_length;
use crate::xml::Element;

/// Resolved view of an automatic style: parent reference + direct deltas.
#[derive(Debug, Clone, Default)]
pub struct AutoDelta {
    pub parent: Option<StyleId>,
    pub paragraph: ParaProps,
    pub font: FontProps,
    /// Outline level from `text:outline-level` on headings, or style default-outline-level.
    #[allow(dead_code)]
    pub outline_level: Option<u8>,
    /// List style name attached via `style:list-style-name`.
    #[allow(dead_code)]
    pub list_style: Option<String>,
}

/// Styles collected from `styles.xml` and `office:automatic-styles`.
#[derive(Debug, Default)]
pub struct OdfStyles {
    pub catalog: StyleCatalog,
    /// Automatic style name → deltas over the parent named style.
    pub automatic: BTreeMap<String, AutoDelta>,
    /// Graphic styles (wrap, etc.) keyed by name.
    pub graphic: BTreeMap<String, GraphicStyle>,
    pub lists: ListCatalog,
    /// List style name → list id in the catalog.
    pub list_names: BTreeMap<String, ListId>,
}

/// Subset of graphic-style properties the image model needs.
#[derive(Debug, Clone, Default)]
pub struct GraphicStyle {
    pub wrap: Option<String>,
    pub run_through: Option<String>,
    pub horizontal_pos: Option<String>,
    pub horizontal_rel: Option<String>,
    pub vertical_pos: Option<String>,
    pub vertical_rel: Option<String>,
    pub mirror: Option<String>,
    pub rotation_angle: Option<f32>,
    pub clip: Option<String>,
}

impl OdfStyles {
    /// Looks up a paragraph/text style reference and de-automatizes it.
    ///
    /// Named styles become a style reference with empty deltas. Automatic
    /// styles become `parent + properties` (the hard point of Phase 4).
    pub fn resolve_para_style(
        &self,
        name: Option<&str>,
    ) -> (Option<StyleId>, ParaProps, FontProps) {
        let Some(name) = name else {
            return (None, ParaProps::default(), FontProps::default());
        };
        if let Some(auto) = self.automatic.get(name) {
            return (
                auto.parent.clone(),
                auto.paragraph.clone(),
                auto.font.clone(),
            );
        }
        let id = StyleId::new(odf_style_id(name));
        if self.catalog.contains(&id) {
            (Some(id), ParaProps::default(), FontProps::default())
        } else {
            // Unknown style name: keep it as a reference so round-trip can
            // restore the attribute even if the definition was missing.
            (Some(id), ParaProps::default(), FontProps::default())
        }
    }

    pub fn resolve_text_style(&self, name: Option<&str>) -> (Option<StyleId>, FontProps) {
        let Some(name) = name else {
            return (None, FontProps::default());
        };
        if let Some(auto) = self.automatic.get(name) {
            return (auto.parent.clone(), auto.font.clone());
        }
        let id = StyleId::new(odf_style_id(name));
        (Some(id), FontProps::default())
    }

    pub fn graphic(&self, name: Option<&str>) -> Option<&GraphicStyle> {
        name.and_then(|n| self.graphic.get(n))
    }
}

/// Reads named styles from `styles.xml` (and optional `office:styles` in content).
pub fn read_named_styles(root: &Element, into: &mut OdfStyles) {
    // Default style (style:default-style).
    for def in root.children_named("default-style").chain(
        root.child("styles")
            .into_iter()
            .flat_map(|s| s.children_named("default-style")),
    ) {
        let family = def.attr("family").unwrap_or("paragraph");
        if family == "paragraph" {
            if let Some(p) = def.child("paragraph-properties") {
                into.catalog.defaults.paragraph = into
                    .catalog
                    .defaults
                    .paragraph
                    .over(&paragraph_properties(p));
            }
            if let Some(t) = def.child("text-properties") {
                into.catalog.defaults.font = into.catalog.defaults.font.over(&text_properties(t));
            }
        } else if family == "text" {
            if let Some(t) = def.child("text-properties") {
                into.catalog.defaults.font = into.catalog.defaults.font.over(&text_properties(t));
            }
        }
    }

    let style_elements: Vec<&Element> = root
        .children_named("style")
        .chain(
            root.child("styles")
                .into_iter()
                .flat_map(|s| s.children_named("style")),
        )
        .collect();

    for element in style_elements {
        read_one_style(element, into, false);
    }

    // List styles.
    for list_style in root.children_named("list-style").chain(
        root.child("styles")
            .into_iter()
            .flat_map(|s| s.children_named("list-style")),
    ) {
        read_list_style(list_style, into);
    }
}

/// Reads `office:automatic-styles` (from content.xml or styles.xml).
pub fn read_automatic_styles(container: &Element, into: &mut OdfStyles) {
    let Some(auto_root) = container.child("automatic-styles").or_else(|| {
        // When passed the automatic-styles element itself.
        if container.name == "automatic-styles" {
            Some(container)
        } else {
            None
        }
    }) else {
        return;
    };

    for element in auto_root.children_named("style") {
        read_one_style(element, into, true);
    }
    for list_style in auto_root.children_named("list-style") {
        read_list_style(list_style, into);
    }
    // Page layouts live here too; the ODT reader picks master-page separately.
}

fn read_one_style(element: &Element, into: &mut OdfStyles, automatic: bool) {
    let Some(name) = element.attr("name") else {
        return;
    };
    let family = element.attr("family").unwrap_or("paragraph");

    if family == "graphic" {
        let mut g = GraphicStyle::default();
        if let Some(gp) = element.child("graphic-properties") {
            g.wrap = gp.attr("wrap").map(str::to_string);
            g.run_through = gp.attr("run-through").map(str::to_string);
            g.horizontal_pos = gp.attr("horizontal-pos").map(str::to_string);
            g.horizontal_rel = gp.attr("horizontal-rel").map(str::to_string);
            g.vertical_pos = gp.attr("vertical-pos").map(str::to_string);
            g.vertical_rel = gp.attr("vertical-rel").map(str::to_string);
            g.mirror = gp.attr("mirror").map(str::to_string);
            if let Some(angle) = gp.attr("rotation-angle").and_then(|s| s.parse().ok()) {
                g.rotation_angle = Some(angle);
            }
            g.clip = gp.attr("clip").map(str::to_string);
        }
        into.graphic.insert(name.to_string(), g);
        return;
    }

    let style_type = match family {
        "paragraph" => StyleType::Paragraph,
        "text" => StyleType::Character,
        "table" | "table-column" | "table-row" | "table-cell" => StyleType::Table,
        _ => return,
    };

    let parent = element
        .attr("parent-style-name")
        .map(|p| StyleId::new(odf_style_id(p)));
    let mut para = element
        .child("paragraph-properties")
        .map(paragraph_properties)
        .unwrap_or_default();
    let font = element
        .child("text-properties")
        .map(text_properties)
        .unwrap_or_default();

    let outline = element
        .attr("default-outline-level")
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|l| (1..=9).contains(l))
        .map(|l| l - 1); // IR outline_level is 0-based
    if let Some(ol) = outline {
        para.outline_level = Some(ol);
    }

    let list_style = element.attr("list-style-name").map(str::to_string);

    if automatic {
        into.automatic.insert(
            name.to_string(),
            AutoDelta {
                parent,
                paragraph: para,
                font,
                outline_level: outline,
                list_style,
            },
        );
    } else {
        let id = StyleId::new(odf_style_id(name));
        let mut style = Style::new(id.as_str(), style_type);
        style.name = element.attr("display-name").unwrap_or(name).to_string();
        style.based_on = parent;
        style.next = element
            .attr("next-style-name")
            .map(|n| StyleId::new(odf_style_id(n)));
        style.font = font;
        style.paragraph = para;
        // Common ODF default paragraph style names.
        style.is_default =
            matches!(name, "Standard" | "Normal" | "Default") && style_type == StyleType::Paragraph;
        into.catalog.insert(style);
    }
}

fn read_list_style(element: &Element, into: &mut OdfStyles) {
    let Some(name) = element.attr("name") else {
        return;
    };
    let id = ListId::new(format!("L{}", into.lists.defs.len() + 1));
    let mut levels = Vec::new();

    // Levels can be list-level-style-bullet / number / image, ordered by level attr.
    let mut level_elems: Vec<&Element> = element
        .children()
        .filter(|e| e.name.starts_with("list-level-style-"))
        .collect();
    level_elems.sort_by_key(|e| e.attr_i64("level").unwrap_or(1));

    for lvl in level_elems {
        let level_num = lvl.attr_i64("level").unwrap_or(1).clamp(1, 9) as usize;
        while levels.len() + 1 < level_num {
            levels.push(ListLevel::new(NumFormat::Bullet, "•"));
        }
        let format = match lvl.name.as_str() {
            "list-level-style-bullet" => NumFormat::Bullet,
            "list-level-style-number" => match lvl.attr("num-format").unwrap_or("1") {
                "a" => NumFormat::LowerLetter,
                "A" => NumFormat::UpperLetter,
                "i" => NumFormat::LowerRoman,
                "I" => NumFormat::UpperRoman,
                _ => NumFormat::Decimal,
            },
            _ => NumFormat::Bullet,
        };
        let text = match &format {
            NumFormat::Bullet => lvl.attr("bullet-char").unwrap_or("•").to_string(),
            NumFormat::Decimal => {
                let suffix = lvl.attr("num-suffix").unwrap_or(".");
                let prefix = lvl.attr("num-prefix").unwrap_or("");
                format!("{prefix}%{level_num}{suffix}")
            }
            _ => {
                let suffix = lvl.attr("num-suffix").unwrap_or(".");
                format!("%{level_num}{suffix}")
            }
        };
        let start = lvl
            .attr("start-value")
            .and_then(|s| s.parse().ok())
            .filter(|&n| n != 1);
        let mut indent = None;
        let mut hanging = None;
        if let Some(pp) = lvl.child("list-level-properties") {
            if let Some(space) = pp.attr("space-before").and_then(parse_length) {
                indent = Some(space);
            }
            if let Some(min) = pp.attr("min-label-width").and_then(parse_length) {
                hanging = Some(min);
            }
        }
        let level = ListLevel {
            format,
            text,
            start,
            indent,
            hanging,
        };
        if levels.len() >= level_num {
            levels[level_num - 1] = level;
        } else {
            levels.push(level);
        }
    }
    if levels.is_empty() {
        levels.push(ListLevel::new(NumFormat::Bullet, "•"));
    }

    into.list_names.insert(name.to_string(), id.clone());
    into.lists.insert(id, ListDef { levels });
}

/// Converts an ODF style name (`Heading_20_1`) into a stable IR id (`Heading_20_1`).
///
/// Underscore-encoded spaces are preserved; the display name holds the pretty form.
pub fn odf_style_id(name: &str) -> String {
    name.to_string()
}

/// Pretty-prints an ODF style name for the IR `name` field.
#[allow(dead_code)]
pub fn odf_display_name(name: &str) -> String {
    // ODF encodes spaces as `_20_`, etc.
    let mut out = String::new();
    let bytes = name.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'_' && i + 3 < bytes.len() && bytes[i + 3] == b'_' {
            if let Ok(code) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(n) = u8::from_str_radix(code, 16) {
                    out.push(n as char);
                    i += 4;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Heading depth implied by a named style, if any.
pub fn heading_level_from_style(styles: &OdfStyles, id: &StyleId) -> Option<u8> {
    if let Some(style) = styles.catalog.get(id) {
        if let Some(level) = styles.catalog.resolve(Some(id)).paragraph.outline_level {
            if level <= 8 {
                return Some(level + 1);
            }
        }
        for candidate in [style.name.as_str(), id.as_str()] {
            let lower = candidate.to_ascii_lowercase().replace('_', " ");
            let lower = lower.replace("20 ", ""); // crude decode of Heading_20_1
                                                  // Better: use display decode
            let decoded = odf_display_name(candidate).to_ascii_lowercase();
            for text in [lower.as_str(), decoded.as_str()] {
                if let Some(rest) = text.strip_prefix("heading") {
                    if let Ok(level) = rest.trim().parse::<u8>() {
                        if (1..=9).contains(&level) {
                            return Some(level);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Ensures a minimum indent exists for hanging lists (helper for writers).
#[allow(dead_code)]
pub fn ensure_length(value: Option<Length>, fallback: Length) -> Length {
    value.unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::Element;

    #[test]
    fn deautomatizes_automatic_styles_to_parent_plus_delta() {
        let styles_xml = r#"
        <office:styles xmlns:office="urn:o" xmlns:style="urn:s" xmlns:fo="urn:f">
          <style:style style:name="Standard" style:family="paragraph">
            <style:text-properties fo:font-name="Liberation Serif" fo:font-size="12pt"/>
          </style:style>
          <style:style style:name="Emphasis" style:family="text">
            <style:text-properties fo:font-style="italic"/>
          </style:style>
        </office:styles>"#;
        let auto_xml = r#"
        <office:automatic-styles xmlns:office="urn:o" xmlns:style="urn:s" xmlns:fo="urn:f">
          <style:style style:name="P1" style:family="paragraph" style:parent-style-name="Standard">
            <style:paragraph-properties fo:text-align="center"/>
          </style:style>
          <style:style style:name="T1" style:family="text" style:parent-style-name="Emphasis">
            <style:text-properties fo:font-weight="bold"/>
          </style:style>
        </office:automatic-styles>"#;

        let mut styles = OdfStyles::default();
        read_named_styles(
            &Element::parse("s.xml", styles_xml.as_bytes()).unwrap(),
            &mut styles,
        );
        read_automatic_styles(
            &Element::parse("a.xml", auto_xml.as_bytes()).unwrap(),
            &mut styles,
        );

        let (parent, para, font) = styles.resolve_para_style(Some("P1"));
        assert_eq!(parent.unwrap().as_str(), "Standard");
        assert_eq!(para.align, Some(docsai_model::style::Align::Center));
        assert!(font.is_empty());

        let (tparent, tfont) = styles.resolve_text_style(Some("T1"));
        assert_eq!(tparent.unwrap().as_str(), "Emphasis");
        assert_eq!(tfont.bold, Some(true));
        assert!(tfont.italic.is_none(), "italic lives on the named style");
    }

    #[test]
    fn decodes_odf_display_names() {
        assert_eq!(odf_display_name("Heading_20_1"), "Heading 1");
    }
}
