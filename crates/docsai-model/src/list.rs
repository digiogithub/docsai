//! List definitions.
//!
//! OOXML lists are not nested syntactically: paragraphs carry flat
//! `(numId, ilvl)` pairs and the tree has to be rebuilt. The IR stores the
//! *rebuilt tree* in [`crate::text::List`] and the *typographic definition*
//! here, so DocMark can keep clean Markdown lists and still regenerate
//! `numbering.xml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::units::Length;

/// Identifier of a list definition (`L1`, `L2`… as written to the front matter).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ListId(pub String);

impl ListId {
    pub fn new(id: impl Into<String>) -> Self {
        ListId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ListId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Numbering style of a list level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumFormat {
    Decimal,
    LowerLetter,
    UpperLetter,
    LowerRoman,
    UpperRoman,
    Bullet,
    /// The level shows no marker at all.
    None,
    /// Any other format, kept verbatim so the writer can restore it.
    Other(String),
}

impl NumFormat {
    /// True for formats that produce an ordered Markdown list.
    pub fn is_ordered(&self) -> bool {
        !matches!(self, NumFormat::Bullet | NumFormat::None)
    }

    pub fn as_str(&self) -> &str {
        match self {
            NumFormat::Decimal => "decimal",
            NumFormat::LowerLetter => "lowerLetter",
            NumFormat::UpperLetter => "upperLetter",
            NumFormat::LowerRoman => "lowerRoman",
            NumFormat::UpperRoman => "upperRoman",
            NumFormat::Bullet => "bullet",
            NumFormat::None => "none",
            NumFormat::Other(s) => s,
        }
    }

    /// Maps an OOXML `w:numFmt` value.
    pub fn from_ooxml(value: &str) -> NumFormat {
        match value {
            "decimal" => NumFormat::Decimal,
            "lowerLetter" => NumFormat::LowerLetter,
            "upperLetter" => NumFormat::UpperLetter,
            "lowerRoman" => NumFormat::LowerRoman,
            "upperRoman" => NumFormat::UpperRoman,
            "bullet" => NumFormat::Bullet,
            "none" => NumFormat::None,
            other => NumFormat::Other(other.to_string()),
        }
    }
}

/// One level of a list definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ListLevel {
    pub format: NumFormat,
    /// Marker template as the source writes it (`%1.`, `%2)`, `•`).
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent: Option<Length>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hanging: Option<Length>,
}

impl ListLevel {
    pub fn new(format: NumFormat, text: impl Into<String>) -> Self {
        ListLevel {
            format,
            text: text.into(),
            start: None,
            indent: None,
            hanging: None,
        }
    }
}

/// A whole list definition.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ListDef {
    pub levels: Vec<ListLevel>,
}

impl ListDef {
    pub fn level(&self, ilvl: usize) -> Option<&ListLevel> {
        self.levels.get(ilvl)
    }

    /// Whether a level renders as an ordered list; unknown levels default to
    /// ordered, matching Word's behaviour for out-of-range `ilvl`.
    pub fn is_ordered_at(&self, ilvl: usize) -> bool {
        self.level(ilvl).is_none_or(|l| l.format.is_ordered())
    }
}

/// The document's list definitions, ordered by id for deterministic output.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", transparent)]
pub struct ListCatalog {
    pub defs: BTreeMap<ListId, ListDef>,
}

impl ListCatalog {
    pub fn insert(&mut self, id: ListId, def: ListDef) {
        self.defs.insert(id, def);
    }

    pub fn get(&self, id: &ListId) -> Option<&ListDef> {
        self.defs.get(id)
    }

    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bullets_are_unordered_everything_else_is_ordered() {
        assert!(NumFormat::Decimal.is_ordered());
        assert!(NumFormat::LowerRoman.is_ordered());
        assert!(!NumFormat::Bullet.is_ordered());
        assert!(!NumFormat::None.is_ordered());
        assert!(NumFormat::Other("chicago".into()).is_ordered());
    }

    #[test]
    fn unknown_ooxml_formats_survive_verbatim() {
        let f = NumFormat::from_ooxml("ideographDigital");
        assert_eq!(f, NumFormat::Other("ideographDigital".into()));
        assert_eq!(f.as_str(), "ideographDigital");
    }

    #[test]
    fn missing_levels_default_to_ordered() {
        let def = ListDef {
            levels: vec![ListLevel::new(NumFormat::Bullet, "•")],
        };
        assert!(!def.is_ordered_at(0));
        assert!(def.is_ordered_at(5));
    }
}
