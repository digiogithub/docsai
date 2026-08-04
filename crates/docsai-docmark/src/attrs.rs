//! Pandoc-style attribute blocks `{#id .class key="value"}`.
//!
//! The canonical order of spec §8 is enforced here, in one place: id first,
//! then classes alphabetically, then keys alphabetically. That ordering is
//! what makes the serializer deterministic and the golden diffs stable.

use std::collections::BTreeMap;

use crate::dict::AttrDict;
use crate::escape::{escape_attr_value, is_bare_value};

/// A set of attributes being built for one node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs {
    id: Option<String>,
    classes: Vec<String>,
    pairs: BTreeMap<String, String>,
}

impl Attrs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn id(&mut self, id: impl Into<String>) -> &mut Self {
        self.id = Some(id.into());
        self
    }

    /// Adds a class. Empty names are ignored so callers can pass through
    /// optional style ids without branching.
    pub fn class(&mut self, class: impl Into<String>) -> &mut Self {
        let class = class.into();
        if !class.is_empty() && !self.classes.contains(&class) {
            self.classes.push(class);
        }
        self
    }

    /// Adds a key/value pair.
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.pairs.insert(key.into(), value.into());
        self
    }

    /// Adds a pair only when the value is `Some`.
    pub fn set_opt(&mut self, key: &str, value: Option<impl Into<String>>) -> &mut Self {
        if let Some(value) = value {
            self.set(key, value);
        }
        self
    }

    /// Adds a boolean pair only when it is `true`.
    pub fn set_flag(&mut self, key: &str, value: bool) -> &mut Self {
        if value {
            self.set(key, "true");
        }
        self
    }

    pub fn is_empty(&self) -> bool {
        self.id.is_none() && self.classes.is_empty() && self.pairs.is_empty()
    }

    /// Renders the block, or the empty string when there is nothing to render.
    ///
    /// ```
    /// use docsai_docmark::attrs::Attrs;
    /// let mut a = Attrs::new();
    /// a.set("width", "450px").class("Quote").id("img-1").set("title", "Figura 1");
    /// assert_eq!(a.render(), r#"{#img-1 .Quote title="Figura 1" width=450px}"#);
    /// ```
    pub fn render(&self) -> String {
        self.render_with(&AttrDict::off())
    }

    /// Renders the block, interning its pairs through `dict` (spec §3.7).
    ///
    /// With a dictionary that is counting, or that has no name for this
    /// pattern, the output is exactly [`Attrs::render`]'s — the first pass and
    /// a document with nothing to intern write the same bytes.
    pub fn render_with(&self, dict: &AttrDict) -> String {
        if self.is_empty() {
            return String::new();
        }
        let interned = match self.pattern() {
            Some(pattern) => {
                dict.observe(&pattern);
                dict.name_for(&pattern)
            }
            None => None,
        };

        let mut parts: Vec<String> = Vec::new();
        if let Some(id) = &self.id {
            parts.push(format!("#{id}"));
        }
        let mut classes = self.classes.clone();
        if let Some(name) = interned {
            classes.push(name.to_string());
        }
        classes.sort();
        parts.extend(classes.into_iter().map(|c| format!(".{c}")));
        if interned.is_none() && !self.pairs.is_empty() {
            parts.push(self.pairs_rendered());
        }
        format!("{{{}}}", parts.join(" "))
    }

    /// The pairs as they render, which is what the dictionary interns.
    ///
    /// `None` when there is nothing to intern: no pairs, or a raw-block, whose
    /// `src=` is scanned out of the serialised text by
    /// [`crate::raw::raw_sidecars`] and must stay where it is written.
    fn pattern(&self) -> Option<String> {
        if self.pairs.is_empty() || self.has_class("raw") {
            return None;
        }
        Some(self.pairs_rendered())
    }

    fn pairs_rendered(&self) -> String {
        self.pairs
            .iter()
            .map(|(key, value)| {
                if is_bare_value(value) {
                    format!("{key}={value}")
                } else {
                    format!("{key}=\"{}\"", escape_attr_value(value))
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Renders the block preceded by a space, for appending to a line.
    pub fn suffix(&self) -> String {
        self.suffix_with(&AttrDict::off())
    }

    /// [`Attrs::suffix`], through a dictionary.
    pub fn suffix_with(&self, dict: &AttrDict) -> String {
        if self.is_empty() {
            String::new()
        } else {
            format!(" {}", self.render_with(dict))
        }
    }

    /// Replaces every dictionary class by the pairs it stands for (spec §3.7).
    ///
    /// A pair written on the node itself wins: the dictionary is a default the
    /// node may override, so an expanded block never loses what it said
    /// explicitly.
    pub fn expand(&mut self, sets: &BTreeMap<String, Attrs>) {
        if sets.is_empty() || self.classes.is_empty() {
            return;
        }
        let classes = std::mem::take(&mut self.classes);
        let mut kept = Vec::new();
        for class in classes {
            match sets.get(&class) {
                Some(set) => {
                    for (key, value) in &set.pairs {
                        self.pairs
                            .entry(key.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
                None => kept.push(class),
            }
        }
        self.classes = kept;
    }

    /// The id, if any (`#id`).
    pub fn id_ref(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// Classes in insertion order.
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// True when `class` is present.
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    /// First class, commonly the style id.
    pub fn first_class(&self) -> Option<&str> {
        self.classes.first().map(String::as_str)
    }

    /// Value of `key`, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.get(key).map(String::as_str)
    }

    /// Removes and returns the value of `key`.
    pub fn take(&mut self, key: &str) -> Option<String> {
        self.pairs.remove(key)
    }

    /// All key/value pairs in sorted order.
    pub fn pairs(&self) -> &BTreeMap<String, String> {
        &self.pairs
    }

    /// Parses a Pandoc-style attribute block, with or without surrounding `{}`.
    ///
    /// ```
    /// use docsai_docmark::attrs::Attrs;
    /// let a = Attrs::parse(r#"{#img-1 .Quote title="Figura 1" width=450px}"#).unwrap();
    /// assert_eq!(a.id_ref(), Some("img-1"));
    /// assert!(a.has_class("Quote"));
    /// assert_eq!(a.get("width"), Some("450px"));
    /// assert_eq!(a.get("title"), Some("Figura 1"));
    /// ```
    pub fn parse(input: &str) -> Option<Attrs> {
        let input = input.trim();
        let inner = if input.starts_with('{') && input.ends_with('}') && input.len() >= 2 {
            &input[1..input.len() - 1]
        } else {
            input
        };
        let mut attrs = Attrs::new();
        let mut rest = inner.trim();
        while !rest.is_empty() {
            rest = rest.trim_start();
            if rest.is_empty() {
                break;
            }
            if let Some(stripped) = rest.strip_prefix('#') {
                let (id, next) = take_token(stripped);
                if id.is_empty() {
                    return None;
                }
                attrs.id = Some(id.to_string());
                rest = next;
                continue;
            }
            if let Some(stripped) = rest.strip_prefix('.') {
                let (class, next) = take_token(stripped);
                if class.is_empty() {
                    return None;
                }
                attrs.class(class);
                rest = next;
                continue;
            }
            let (key, after_key) = take_ident(rest);
            if key.is_empty() {
                return None;
            }
            let after_key = after_key.trim_start();
            let after_eq = after_key.strip_prefix('=')?;
            let after_eq = after_eq.trim_start();
            let (value, next) = take_value(after_eq)?;
            attrs.set(key, value);
            rest = next;
        }
        Some(attrs)
    }
}

/// Identifier or class/id token: letters, digits, `_`, `-`, `.` (for class names).
fn take_token(input: &str) -> (&str, &str) {
    let end = input
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
        .map(|(i, _)| i)
        .unwrap_or(input.len());
    (&input[..end], &input[end..])
}

fn take_ident(input: &str) -> (&str, &str) {
    let end = input
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
        .map(|(i, _)| i)
        .unwrap_or(input.len());
    (&input[..end], &input[end..])
}

fn take_value(input: &str) -> Option<(String, &str)> {
    if let Some(rest) = input.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, '"')) => out.push('"'),
                    Some((_, '\\')) => out.push('\\'),
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, other)) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                },
                '"' => return Some((out, &rest[i + 1..])),
                other => out.push(other),
            }
        }
        None
    } else {
        let end = input
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(input.len());
        if end == 0 {
            return None;
        }
        Some((input[..end].to_string(), &input[end..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_in_the_canonical_order() {
        let mut a = Attrs::new();
        a.set("z", "1")
            .class("Beta")
            .set("a", "2")
            .class("Alpha")
            .id("x");
        assert_eq!(a.render(), "{#x .Alpha .Beta a=2 z=1}");
    }

    #[test]
    fn quotes_only_what_needs_quoting() {
        let mut a = Attrs::new();
        a.set("width", "450px")
            .set("color", "#FF0000")
            .set("name", "Logo corporativo")
            .set("link", "https://example.com/a b");
        assert_eq!(
            a.render(),
            r#"{color=#FF0000 link="https://example.com/a b" name="Logo corporativo" width=450px}"#
        );
    }

    #[test]
    fn is_empty_when_nothing_was_added() {
        assert!(Attrs::new().is_empty());
        assert_eq!(Attrs::new().render(), "");
        assert_eq!(Attrs::new().suffix(), "");
    }

    #[test]
    fn empty_classes_and_absent_options_are_skipped() {
        let mut a = Attrs::new();
        a.class("")
            .set_opt("missing", None::<String>)
            .set_flag("off", false)
            .set_flag("on", true);
        assert_eq!(a.render(), "{on=true}");
    }

    #[test]
    fn later_values_replace_earlier_ones() {
        let mut a = Attrs::new();
        a.set("k", "1").set("k", "2");
        assert_eq!(a.render(), "{k=2}");
    }

    #[test]
    fn parse_round_trips_with_render() {
        let samples = [
            r#"{#img-1 .Quote title="Figura 1" width=450px}"#,
            "{#x .Alpha .Beta a=2 z=1}",
            r#"{color=#FF0000 link="https://example.com/a b" name="Logo corporativo" width=450px}"#,
            "{on=true}",
            "{.Heading1}",
            "{align=center indent-first-line=24px}",
        ];
        for sample in samples {
            let parsed = Attrs::parse(sample).unwrap_or_else(|| panic!("parse {sample}"));
            assert_eq!(parsed.render(), sample, "render mismatch for {sample}");
        }
    }

    #[test]
    fn parse_getters_and_take() {
        let mut a = Attrs::parse(r#"{#id .A .B k=v}"#).unwrap();
        assert_eq!(a.id_ref(), Some("id"));
        assert_eq!(a.classes(), &["A".to_string(), "B".to_string()]);
        assert_eq!(a.get("k"), Some("v"));
        assert_eq!(a.take("k"), Some("v".into()));
        assert_eq!(a.get("k"), None);
    }
}
