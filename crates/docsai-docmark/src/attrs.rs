//! Pandoc-style attribute blocks `{#id .class key="value"}`.
//!
//! The canonical order of spec §8 is enforced here, in one place: id first,
//! then classes alphabetically, then keys alphabetically. That ordering is
//! what makes the serialiser deterministic and the golden diffs stable.

use std::collections::BTreeMap;

use crate::escape::{escape_attr_value, is_bare_value, unescape_attr_value};

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

    // ----------------------------------------------------------------------
    // Reading (Fase 2)
    // ----------------------------------------------------------------------

    /// Parses a block, braces included.
    ///
    /// Unknown keys and classes are kept as they come: the caller decides what
    /// it recognises, which is what keeps the format forward-compatible
    /// (spec §2).
    ///
    /// ```
    /// use docsai_docmark::attrs::Attrs;
    /// let a = Attrs::parse(r#"{#img-1 .Quote title="Figura 1" width=450px}"#).unwrap();
    /// assert_eq!(a.get_id(), Some("img-1"));
    /// assert!(a.has_class("Quote"));
    /// assert_eq!(a.get("title"), Some("Figura 1"));
    /// ```
    pub fn parse(block: &str) -> Option<Attrs> {
        let inner = block.strip_prefix('{')?.strip_suffix('}')?;
        let mut attrs = Attrs::new();
        for token in split_tokens(inner) {
            match token.split_once('=') {
                Some((key, value)) if !key.is_empty() => {
                    attrs.set(key, read_value(value));
                }
                _ => match token.chars().next() {
                    Some('#') => {
                        attrs.id(&token[1..]);
                    }
                    Some('.') => {
                        attrs.class(&token[1..]);
                    }
                    // A bare word is not something the serialiser writes; keep
                    // it as a class, which is how Pandoc reads it too.
                    Some(_) => {
                        attrs.class(token.as_str());
                    }
                    None => {}
                },
            }
        }
        Some(attrs)
    }

    pub fn get_id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs.get(key).map(String::as_str)
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    /// Classes in the order they were read.
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// Removes a class, reporting whether it was there.
    pub fn take_class(&mut self, class: &str) -> bool {
        let before = self.classes.len();
        self.classes.retain(|c| c != class);
        self.classes.len() != before
    }

    /// Removes a key and returns its value.
    pub fn take(&mut self, key: &str) -> Option<String> {
        self.pairs.remove(key)
    }

    /// A boolean pair; anything other than `true` reads as `false`.
    pub fn flag(&self, key: &str) -> Option<bool> {
        self.get(key).map(|v| v == "true")
    }
}

/// Splits an attribute block's body on spaces, honouring quoted values.
fn split_tokens(inner: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    for c in inner.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_quotes => {
                current.push(c);
                escaped = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                current.push(c);
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Unwraps a value: quoted values are unescaped, bare ones taken verbatim.
fn read_value(value: &str) -> String {
    match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(quoted) => unescape_attr_value(quoted),
        None => value.to_string(),
    }
}

/// Finds the attribute block that ends `line`, returning `(text, block)`.
///
/// Only a block preceded by a space counts: the serialiser always writes a
/// paragraph's attributes as `" {…}"`, and never puts a space between the `]`
/// of a span or the `)` of a link and its own block. That single rule is what
/// tells `Pagina [1]{.field field=PAGE}` (a paragraph ending in a span) apart
/// from `Parrafo con estilo. {.Destacado}` (a paragraph with attributes).
pub fn split_trailing(line: &str) -> (&str, Option<Attrs>) {
    let trimmed = line.trim_end();
    if !trimmed.ends_with('}') {
        return (line, None);
    }
    let Some(open) = matching_open_brace(trimmed) else {
        return (line, None);
    };
    if open == 0 || !trimmed[..open].ends_with(' ') {
        return (line, None);
    }
    match Attrs::parse(&trimmed[open..]) {
        Some(attrs) => (trimmed[..open].trim_end(), Some(attrs)),
        None => (line, None),
    }
}

/// Byte index of the `{` opening the brace group that closes `text`.
fn matching_open_brace(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut in_quotes = false;
    for index in (0..bytes.len()).rev() {
        match bytes[index] {
            b'"' if index == 0 || bytes[index - 1] != b'\\' => in_quotes = !in_quotes,
            b'}' if !in_quotes => depth += 1,
            b'{' if !in_quotes => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
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
    fn parsing_is_the_inverse_of_rendering() {
        let mut original = Attrs::new();
        original
            .id("img-1")
            .class("Quote")
            .class("underline")
            .set("width", "450px")
            .set("name", "Logo corporativo")
            .set("border", "1pt solid #000000")
            .set("title", r#"Con "comillas" y \barra"#);
        let text = original.render();
        let parsed = Attrs::parse(&text).expect("parses");
        assert_eq!(parsed.render(), text, "re-rendering gives the same bytes");
        assert_eq!(parsed.get("name"), Some("Logo corporativo"));
        assert_eq!(parsed.get("title"), Some(r#"Con "comillas" y \barra"#));
        assert!(parsed.has_class("Quote"));
        assert_eq!(parsed.get_id(), Some("img-1"));
    }

    #[test]
    fn a_trailing_block_needs_a_space_before_it() {
        // The discriminator between a paragraph's attributes and a span that
        // happens to end the paragraph.
        let (text, attrs) = split_trailing("Parrafo con estilo. {.Destacado align=center}");
        assert_eq!(text, "Parrafo con estilo.");
        assert!(attrs.expect("has attributes").has_class("Destacado"));

        let line = "Pagina [1]{.field field=PAGE} de [3]{.field field=NUMPAGES}";
        let (text, attrs) = split_trailing(line);
        assert_eq!(text, line, "a span is not a paragraph attribute block");
        assert!(attrs.is_none());

        let (text, attrs) = split_trailing("[x]{.sup} {.Quote}");
        assert_eq!(text, "[x]{.sup}");
        assert!(attrs.expect("has attributes").has_class("Quote"));
    }

    #[test]
    fn braces_inside_quoted_values_do_not_confuse_the_split() {
        let (text, attrs) = split_trailing(r#"Texto {title="a } b"}"#);
        assert_eq!(text, "Texto");
        assert_eq!(attrs.expect("has attributes").get("title"), Some("a } b"));
    }

    #[test]
    fn a_line_without_a_block_is_returned_untouched() {
        assert_eq!(split_trailing("solo texto").0, "solo texto");
        assert!(split_trailing("solo texto").1.is_none());
        assert!(split_trailing("desbalanceado }").1.is_none());
    }
}
