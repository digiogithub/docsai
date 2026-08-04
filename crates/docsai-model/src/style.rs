//! Named styles with inheritance.
//!
//! The cardinal rule (analysis §5.2): the IR stores **a reference plus a
//! delta**, never flattened formatting. Every field of [`FontProps`] and
//! [`ParaProps`] is an `Option`; `None` means "inherit", not "off".

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::units::Length;

/// Identifier of a named style, as it appears in the source document.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StyleId(pub String);

impl StyleId {
    pub fn new(id: impl Into<String>) -> Self {
        StyleId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StyleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a style can be applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StyleType {
    Paragraph,
    Character,
    Table,
    Numbering,
}

impl StyleType {
    pub fn as_str(self) -> &'static str {
        match self {
            StyleType::Paragraph => "paragraph",
            StyleType::Character => "character",
            StyleType::Table => "table",
            StyleType::Numbering => "numbering",
        }
    }
}

/// Underline styles that DocMark can express; anything else degrades to
/// [`Underline::Single`] with a warning from the reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Underline {
    None,
    Single,
    Double,
    Thick,
    Dotted,
    Dashed,
    Wave,
}

/// Sub/superscript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VertAlign {
    Baseline,
    Superscript,
    Subscript,
}

/// Character-level formatting. Every field is a delta over what is inherited.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct FontProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<Underline>,
    /// `#RRGGBB`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Highlight colour name as the source names it (`yellow`, `cyan`…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vert_align: Option<VertAlign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub small_caps: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caps: Option<bool>,
}

impl FontProps {
    /// True when the delta carries no information.
    pub fn is_empty(&self) -> bool {
        *self == FontProps::default()
    }

    /// Layers `self` on top of `base`: fields set in `self` win, the rest are
    /// inherited. This is the merge step of the four-level OOXML cascade.
    pub fn over(&self, base: &FontProps) -> FontProps {
        macro_rules! pick {
            ($($f:ident),+ $(,)?) => {
                FontProps { $($f: self.$f.clone().or_else(|| base.$f.clone())),+ }
            };
        }
        pick!(
            name, size, bold, italic, strike, underline, color, highlight, vert_align, small_caps,
            caps
        )
    }

    /// The subset of `self` that is not already implied by `base`.
    ///
    /// Used by the serializer to honour the "economy rule" of the spec (§3.1):
    /// formatting identical to the applied style is not emitted.
    pub fn minus(&self, base: &FontProps) -> FontProps {
        macro_rules! drop_same {
            ($($f:ident),+ $(,)?) => {
                FontProps {
                    $($f: match (&self.$f, &base.$f) {
                        (Some(a), Some(b)) if a == b => None,
                        (a, _) => a.clone(),
                    }),+
                }
            };
        }
        drop_same!(
            name, size, bold, italic, strike, underline, color, highlight, vert_align, small_caps,
            caps
        )
    }
}

/// Horizontal alignment of a paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    pub fn as_str(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }
}

/// Line spacing, kept in the source's own terms so the round-trip is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineHeight {
    /// A multiple of the single line height, in 1/1000 (240 → 1.0 in OOXML).
    Multiple(i32),
    /// An exact height.
    Exact(Length),
    /// A minimum height.
    AtLeast(Length),
}

/// Paragraph-level formatting. Every field is a delta over what is inherited.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct ParaProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_left: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_right: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_first_line: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indent_hanging: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after: Option<Length>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_height: Option<LineHeight>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_break_before: Option<bool>,
    /// Shading colour, `#RRGGBB`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Outline level, 0-based: the heading depth the paragraph belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_level: Option<u8>,
}

impl ParaProps {
    pub fn is_empty(&self) -> bool {
        *self == ParaProps::default()
    }

    /// Layers `self` on top of `base`; see [`FontProps::over`].
    pub fn over(&self, base: &ParaProps) -> ParaProps {
        macro_rules! pick {
            ($($f:ident),+ $(,)?) => {
                ParaProps { $($f: self.$f.clone().or_else(|| base.$f.clone())),+ }
            };
        }
        pick!(
            align,
            indent_left,
            indent_right,
            indent_first_line,
            indent_hanging,
            space_before,
            space_after,
            line_height,
            keep_with_next,
            page_break_before,
            background,
            outline_level,
        )
    }

    /// See [`FontProps::minus`].
    pub fn minus(&self, base: &ParaProps) -> ParaProps {
        macro_rules! drop_same {
            ($($f:ident),+ $(,)?) => {
                ParaProps {
                    $($f: match (&self.$f, &base.$f) {
                        (Some(a), Some(b)) if a == b => None,
                        (a, _) => a.clone(),
                    }),+
                }
            };
        }
        drop_same!(
            align,
            indent_left,
            indent_right,
            indent_first_line,
            indent_hanging,
            space_before,
            space_after,
            line_height,
            keep_with_next,
            page_break_before,
            background,
            outline_level,
        )
    }
}

/// A named style as declared by the source document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Style {
    pub id: StyleId,
    /// Human-readable name (`w:name`), which may differ from the id.
    pub name: String,
    #[serde(rename = "type")]
    pub style_type: StyleType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub based_on: Option<StyleId>,
    /// Style applied to the next paragraph (`w:next`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<StyleId>,
    #[serde(skip_serializing_if = "FontProps::is_empty", default)]
    pub font: FontProps,
    #[serde(skip_serializing_if = "ParaProps::is_empty", default)]
    pub paragraph: ParaProps,
    /// True for the document's default style of its kind.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub is_default: bool,
}

impl Style {
    pub fn new(id: impl Into<String>, style_type: StyleType) -> Self {
        let id = StyleId::new(id);
        Style {
            name: id.0.clone(),
            id,
            style_type,
            based_on: None,
            next: None,
            font: FontProps::default(),
            paragraph: ParaProps::default(),
            is_default: false,
        }
    }
}

/// Document-wide defaults (`w:docDefaults`), the bottom of the cascade.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct DocDefaults {
    #[serde(skip_serializing_if = "FontProps::is_empty")]
    pub font: FontProps,
    #[serde(skip_serializing_if = "ParaProps::is_empty")]
    pub paragraph: ParaProps,
}

/// Formatting with the whole cascade already applied.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedStyle {
    pub font: FontProps,
    pub paragraph: ParaProps,
}

/// The document's style catalogue.
///
/// Kept ordered by id so that serialisation is deterministic.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct StyleCatalog {
    #[serde(skip_serializing_if = "DocDefaults::is_empty")]
    pub defaults: DocDefaults,
    pub styles: BTreeMap<StyleId, Style>,
}

impl DocDefaults {
    pub fn is_empty(&self) -> bool {
        self.font.is_empty() && self.paragraph.is_empty()
    }
}

/// Depth limit for `based-on` chains. Word itself refuses to nest this deep;
/// the limit also breaks cycles in malformed documents without recursion.
const MAX_INHERITANCE_DEPTH: usize = 32;

impl StyleCatalog {
    pub fn insert(&mut self, style: Style) {
        self.styles.insert(style.id.clone(), style);
    }

    pub fn get(&self, id: &StyleId) -> Option<&Style> {
        self.styles.get(id)
    }

    pub fn contains(&self, id: &StyleId) -> bool {
        self.styles.contains_key(id)
    }

    /// The default paragraph style, if the document declared one.
    pub fn default_paragraph_style(&self) -> Option<&Style> {
        self.styles
            .values()
            .find(|s| s.is_default && s.style_type == StyleType::Paragraph)
    }

    /// The paragraph style a heading of `level` (1-based) already names by
    /// being a heading: the one style whose *resolved* outline level is
    /// `level - 1`.
    ///
    /// `None` when the document has no such style, or has more than one — an
    /// ambiguous chain implies nothing, and a serializer that guessed would
    /// have to guess the same way on the way back.
    pub fn heading_style(&self, level: u8) -> Option<&StyleId> {
        if level == 0 {
            return None;
        }
        let wanted = level - 1;
        let mut found = None;
        for (id, style) in &self.styles {
            if style.style_type != StyleType::Paragraph {
                continue;
            }
            // The style's *own* level, not the resolved one: a style that
            // inherits its outline level from a heading is a different style,
            // and counting it would make every document with a `TOC Heading`
            // ambiguous.
            if style.paragraph.outline_level != Some(wanted) {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(id);
        }
        found
    }

    /// Walks the `based_on` chain from the document defaults up to `id` and
    /// merges every level, producing fully resolved formatting.
    ///
    /// Cycles are broken by a depth limit and by a visited set, so
    /// a malformed `styles.xml` cannot hang or overflow the stack.
    ///
    /// ```
    /// # use docsai_model::style::*;
    /// let mut cat = StyleCatalog::default();
    /// let mut base = Style::new("Normal", StyleType::Paragraph);
    /// base.font.size = Some(docsai_model::units::Length::from_pt(11.0));
    /// let mut child = Style::new("Quote", StyleType::Paragraph);
    /// child.based_on = Some(StyleId::new("Normal"));
    /// child.font.italic = Some(true);
    /// cat.insert(base);
    /// cat.insert(child);
    /// let r = cat.resolve(Some(&StyleId::new("Quote")));
    /// assert_eq!(r.font.italic, Some(true));
    /// assert_eq!(r.font.size, Some(docsai_model::units::Length::from_pt(11.0)));
    /// ```
    pub fn resolve(&self, id: Option<&StyleId>) -> ResolvedStyle {
        let defaults = ResolvedStyle {
            font: self.defaults.font.clone(),
            paragraph: self.defaults.paragraph.clone(),
        };
        self.resolve_over(id, &defaults)
    }

    /// Resolves `id` on top of `base` rather than on the document defaults.
    ///
    /// This is the middle of the OOXML cascade: a run's formatting inherits
    /// from its *paragraph's* style before it inherits from the document, so a
    /// character style resolved on the defaults alone would report as "direct"
    /// everything the paragraph style already said.
    pub fn resolve_over(&self, id: Option<&StyleId>, base: &ResolvedStyle) -> ResolvedStyle {
        let mut chain: Vec<&Style> = Vec::new();
        let mut visited: Vec<&StyleId> = Vec::new();
        let mut cursor = id;
        while let Some(current) = cursor {
            if visited.contains(&current) || chain.len() >= MAX_INHERITANCE_DEPTH {
                break;
            }
            visited.push(current);
            match self.styles.get(current) {
                Some(style) => {
                    chain.push(style);
                    cursor = style.based_on.as_ref();
                }
                None => break,
            }
        }

        let mut resolved = base.clone();
        // `chain` runs child → ancestor; apply ancestors first.
        for style in chain.iter().rev() {
            resolved.font = style.font.over(&resolved.font);
            resolved.paragraph = style.paragraph.over(&resolved.paragraph);
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Length;

    fn catalog() -> StyleCatalog {
        let mut cat = StyleCatalog::default();
        cat.defaults.font.name = Some("Calibri".into());
        cat.defaults.font.size = Some(Length::from_pt(11.0));

        let mut normal = Style::new("Normal", StyleType::Paragraph);
        normal.is_default = true;
        cat.insert(normal);

        let mut destacado = Style::new("Destacado", StyleType::Paragraph);
        destacado.based_on = Some(StyleId::new("Normal"));
        destacado.font.italic = Some(true);
        destacado.font.color = Some("#C00000".into());
        destacado.paragraph.align = Some(Align::Center);
        cat.insert(destacado);

        let mut fuerte = Style::new("DestacadoFuerte", StyleType::Paragraph);
        fuerte.based_on = Some(StyleId::new("Destacado"));
        fuerte.font.bold = Some(true);
        cat.insert(fuerte);
        cat
    }

    #[test]
    fn resolves_through_the_whole_chain() {
        let r = catalog().resolve(Some(&StyleId::new("DestacadoFuerte")));
        assert_eq!(r.font.bold, Some(true)); // own
        assert_eq!(r.font.italic, Some(true)); // parent
        assert_eq!(r.font.name.as_deref(), Some("Calibri")); // docDefaults
        assert_eq!(r.paragraph.align, Some(Align::Center));
    }

    #[test]
    fn child_wins_over_ancestor() {
        let mut cat = catalog();
        let mut fuerte = cat.get(&StyleId::new("DestacadoFuerte")).unwrap().clone();
        fuerte.font.color = Some("#000000".into());
        cat.insert(fuerte);
        let r = cat.resolve(Some(&StyleId::new("DestacadoFuerte")));
        assert_eq!(r.font.color.as_deref(), Some("#000000"));
    }

    #[test]
    fn unknown_style_falls_back_to_defaults() {
        let r = catalog().resolve(Some(&StyleId::new("NoExiste")));
        assert_eq!(r.font.name.as_deref(), Some("Calibri"));
        assert_eq!(r.font.italic, None);
    }

    #[test]
    fn inheritance_cycle_terminates() {
        let mut cat = StyleCatalog::default();
        let mut a = Style::new("A", StyleType::Paragraph);
        a.based_on = Some(StyleId::new("B"));
        a.font.bold = Some(true);
        let mut b = Style::new("B", StyleType::Paragraph);
        b.based_on = Some(StyleId::new("A"));
        b.font.italic = Some(true);
        cat.insert(a);
        cat.insert(b);
        let r = cat.resolve(Some(&StyleId::new("A")));
        assert_eq!(r.font.bold, Some(true));
        assert_eq!(r.font.italic, Some(true));
    }

    #[test]
    fn minus_drops_redundant_formatting() {
        let base = FontProps {
            italic: Some(true),
            color: Some("#C00000".into()),
            ..Default::default()
        };
        let direct = FontProps {
            italic: Some(true),
            bold: Some(true),
            color: Some("#000000".into()),
            ..Default::default()
        };
        let delta = direct.minus(&base);
        assert_eq!(delta.italic, None, "identical to the style, not emitted");
        assert_eq!(delta.bold, Some(true));
        assert_eq!(delta.color.as_deref(), Some("#000000"), "differs, kept");
    }
}
