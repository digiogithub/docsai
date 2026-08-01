//! Pandoc-style attribute blocks `{#id .class key="value"}`.
//!
//! The canonical order of spec §8 is enforced here, in one place: id first,
//! then classes alphabetically, then keys alphabetically. That ordering is
//! what makes the serialiser deterministic and the golden diffs stable.

use std::collections::BTreeMap;

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
        if self.is_empty() {
            return String::new();
        }
        let mut parts: Vec<String> = Vec::new();
        if let Some(id) = &self.id {
            parts.push(format!("#{id}"));
        }
        let mut classes = self.classes.clone();
        classes.sort();
        parts.extend(classes.into_iter().map(|c| format!(".{c}")));
        for (key, value) in &self.pairs {
            if is_bare_value(value) {
                parts.push(format!("{key}={value}"));
            } else {
                parts.push(format!("{key}=\"{}\"", escape_attr_value(value)));
            }
        }
        format!("{{{}}}", parts.join(" "))
    }

    /// Renders the block preceded by a space, for appending to a line.
    pub fn suffix(&self) -> String {
        if self.is_empty() {
            String::new()
        } else {
            format!(" {}", self.render())
        }
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
}
