//! A minimal XML tree over `quick-xml`.
//!
//! OOXML parts are small enough to hold in memory, and a tree makes the
//! readers far easier to review than a hand-rolled event state machine. Each
//! node also records the **byte span** it occupies in the source, which is what
//! makes the raw-block fidelity hatch (spec §7) exact: an unrecognised element
//! is preserved as the original bytes, not as a re-serialisation.
//!
//! Elements are matched by *local* name. OOXML prefixes are conventional but
//! not guaranteed, and every lookup in this crate is contextual (children of a
//! known parent), so local names are unambiguous while being robust to a
//! document that renames its prefixes.

use quick_xml::events::Event;
use quick_xml::Reader;
use std::ops::Range;

use crate::error::ReadError;

/// An XML element.
#[derive(Debug, Clone)]
pub struct Element {
    /// Local name, without prefix (`p` for `w:p`).
    pub name: String,
    /// Namespace prefix as written (`w`), empty when there was none.
    pub prefix: String,
    /// Attributes, in document order, with their qualified names.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
    /// Byte range of the whole element in the source part.
    pub span: Range<usize>,
}

/// A child of an element.
#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
}

/// Depth limit; a document nested deeper than this is pathological and would
/// otherwise let a crafted file exhaust memory.
const MAX_DEPTH: usize = 256;

impl Element {
    /// Parses a whole part into a tree rooted at its single top element.
    pub fn parse(part: &str, source: &[u8]) -> Result<Element, ReadError> {
        let text = std::str::from_utf8(source).map_err(|_| ReadError::Encoding {
            part: part.to_string(),
        })?;
        let mut reader = Reader::from_str(text);
        let config = reader.config_mut();
        config.trim_text(false);
        config.expand_empty_elements = false;
        config.check_end_names = false;
        // A stray closing tag should not sink a document that is otherwise
        // readable; a *missing* one still does, because a truncated part means
        // truncated content and the caller must be told.
        config.allow_unmatched_ends = true;

        let mut stack: Vec<Element> = Vec::new();
        let mut root: Option<Element> = None;
        let mut start = 0usize;

        loop {
            let event = reader.read_event().map_err(|source| ReadError::Xml {
                part: part.to_string(),
                source,
            })?;
            let end = reader.buffer_position() as usize;
            match event {
                Event::Start(e) => {
                    if stack.len() >= MAX_DEPTH {
                        return Err(ReadError::TooLarge(format!(
                            "`{part}` nests deeper than {MAX_DEPTH} elements"
                        )));
                    }
                    stack.push(new_element(e.name().as_ref(), e.attributes(), start, part)?);
                }
                Event::Empty(e) => {
                    let element = new_element(e.name().as_ref(), e.attributes(), start, part)?;
                    let element = Element {
                        span: start..end,
                        ..element
                    };
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(Node::Element(element)),
                        None if root.is_none() => root = Some(element),
                        None => {}
                    }
                }
                Event::End(_) => {
                    let Some(mut element) = stack.pop() else {
                        // Stray closing tag: ignore rather than fail; the
                        // surrounding content is still usable.
                        start = end;
                        continue;
                    };
                    element.span = element.span.start..end;
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(Node::Element(element)),
                        None if root.is_none() => root = Some(element),
                        None => {}
                    }
                }
                Event::Text(t) => {
                    if let Some(parent) = stack.last_mut() {
                        let text = t.unescape().map_err(|source| ReadError::Xml {
                            part: part.to_string(),
                            source,
                        })?;
                        if !text.is_empty() {
                            parent.children.push(Node::Text(text.into_owned()));
                        }
                    }
                }
                Event::CData(t) => {
                    if let Some(parent) = stack.last_mut() {
                        let text = String::from_utf8_lossy(t.as_ref()).into_owned();
                        parent.children.push(Node::Text(text));
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            start = end;
        }

        root.ok_or_else(|| ReadError::WrongShape {
            part: part.to_string(),
            expected: "XML with a root element".into(),
        })
    }

    /// The value of an attribute, matched on its local name.
    ///
    /// `w:val` and `val` both match a lookup for `"val"`, which keeps the
    /// readers independent of prefix conventions.
    pub fn attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| local_of(name) == local)
            .map(|(_, value)| value.as_str())
    }

    /// The value of an attribute, matched on its *qualified* name. Needed when
    /// two namespaces put different meanings on the same local name, as
    /// `r:embed` and `r:link` do next to `a:blip`'s own attributes.
    pub fn attr_qualified(&self, qualified: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(name, _)| name == qualified)
            .map(|(_, value)| value.as_str())
    }

    /// An OOXML boolean attribute/element: absent value means true, and
    /// `0`/`false`/`off` mean false.
    pub fn ooxml_flag(&self) -> bool {
        !matches!(self.attr("val"), Some("0") | Some("false") | Some("off"))
    }

    /// The first child element with this local name.
    pub fn child(&self, local: &str) -> Option<&Element> {
        self.children().find(|e| e.name == local)
    }

    /// Every child element with this local name.
    pub fn children_named<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children().filter(move |e| e.name == local)
    }

    /// Every child element.
    pub fn children(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// A descendant reached by a path of local names.
    pub fn path(&self, path: &[&str]) -> Option<&Element> {
        let mut cursor = self;
        for step in path {
            cursor = cursor.child(step)?;
        }
        Some(cursor)
    }

    /// Concatenation of the direct text children.
    pub fn text(&self) -> String {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::Text(t) => Some(t.as_str()),
                Node::Element(_) => None,
            })
            .collect()
    }

    /// Concatenation of every descendant text node.
    #[cfg(test)]
    pub fn deep_text(&self) -> String {
        let mut out = String::new();
        self.push_deep_text(&mut out);
        out
    }

    #[cfg(test)]
    fn push_deep_text(&self, out: &mut String) {
        for node in &self.children {
            match node {
                Node::Text(t) => out.push_str(t),
                Node::Element(e) => e.push_deep_text(out),
            }
        }
    }

    /// The original bytes of this element, for raw-block preservation.
    pub fn raw<'a>(&self, source: &'a str) -> &'a str {
        source
            .get(self.span.clone())
            .unwrap_or_default()
            .trim_matches(|c: char| c == '\n' || c == '\r')
    }

    /// An integer attribute, or `None` when absent or unparsable.
    pub fn attr_i64(&self, local: &str) -> Option<i64> {
        self.attr(local)?.trim().parse().ok()
    }
}

fn new_element(
    raw_name: &[u8],
    attributes: quick_xml::events::attributes::Attributes<'_>,
    start: usize,
    part: &str,
) -> Result<Element, ReadError> {
    let qualified = String::from_utf8_lossy(raw_name).into_owned();
    let (prefix, name) = match qualified.split_once(':') {
        Some((p, n)) => (p.to_string(), n.to_string()),
        None => (String::new(), qualified.clone()),
    };

    let mut attrs = Vec::new();
    for attr in attributes {
        let attr = attr.map_err(|e| ReadError::Xml {
            part: part.to_string(),
            source: quick_xml::Error::InvalidAttr(e),
        })?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let value = attr
            .unescape_value()
            .map(|v| v.into_owned())
            .unwrap_or_else(|_| String::from_utf8_lossy(&attr.value).into_owned());
        attrs.push((key, value));
    }

    Ok(Element {
        name,
        prefix,
        attrs,
        children: Vec::new(),
        span: start..start,
    })
}

fn local_of(qualified: &str) -> &str {
    qualified.split_once(':').map_or(qualified, |(_, n)| n)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"<?xml version="1.0"?>
<w:document xmlns:w="urn:w"><w:body>
  <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t xml:space="preserve">Hola </w:t></w:r><w:r><w:t>mundo</w:t></w:r></w:p>
  <w:tbl><w:tr><w:tc><w:p><w:r><w:t>celda</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#;

    #[test]
    fn parses_the_tree_and_matches_on_local_names() {
        let root = Element::parse("test.xml", DOC.as_bytes()).unwrap();
        assert_eq!(root.name, "document");
        assert_eq!(root.prefix, "w");
        let body = root.child("body").unwrap();
        assert_eq!(body.children_named("p").count(), 1);
        let p = body.child("p").unwrap();
        assert_eq!(
            p.path(&["pPr", "pStyle"]).and_then(|e| e.attr("val")),
            Some("Heading1")
        );
        assert_eq!(p.deep_text(), "Hola mundo");
    }

    #[test]
    fn spans_point_at_the_original_bytes() {
        let root = Element::parse("test.xml", DOC.as_bytes()).unwrap();
        let tbl = root.path(&["body", "tbl"]).unwrap();
        let raw = tbl.raw(DOC);
        assert!(raw.starts_with("<w:tbl>"), "got {raw}");
        assert!(raw.ends_with("</w:tbl>"));
        assert!(raw.contains("celda"));
    }

    #[test]
    fn empty_elements_get_a_span_too() {
        let root = Element::parse("test.xml", DOC.as_bytes()).unwrap();
        let style = root.path(&["body", "p", "pPr", "pStyle"]).unwrap();
        assert_eq!(style.raw(DOC), r#"<w:pStyle w:val="Heading1"/>"#);
    }

    #[test]
    fn ooxml_flags_default_to_true() {
        let xml =
            r#"<w:rPr xmlns:w="urn:w"><w:b/><w:i w:val="0"/><w:strike w:val="true"/></w:rPr>"#;
        let root = Element::parse("t.xml", xml.as_bytes()).unwrap();
        assert!(root.child("b").unwrap().ooxml_flag());
        assert!(!root.child("i").unwrap().ooxml_flag());
        assert!(root.child("strike").unwrap().ooxml_flag());
    }

    #[test]
    fn qualified_attribute_lookup_distinguishes_namespaces() {
        let xml = r#"<a:blip xmlns:a="urn:a" xmlns:r="urn:r" r:embed="rId1" r:link="rId2"/>"#;
        let root = Element::parse("t.xml", xml.as_bytes()).unwrap();
        assert_eq!(root.attr_qualified("r:embed"), Some("rId1"));
        assert_eq!(root.attr_qualified("r:link"), Some("rId2"));
    }

    #[test]
    fn entities_are_unescaped_in_text_and_attributes() {
        let xml = r#"<w:t xmlns:w="urn:w" w:val="a &lt; b">x &amp; y</w:t>"#;
        let root = Element::parse("t.xml", xml.as_bytes()).unwrap();
        assert_eq!(root.text(), "x & y");
        assert_eq!(root.attr("val"), Some("a < b"));
    }

    #[test]
    fn malformed_xml_is_an_error_not_a_panic() {
        assert!(
            Element::parse("t.xml", b"<w:p><w:r>").is_err(),
            "a truncated part is an error, not partial content"
        );
        assert!(Element::parse("t.xml", b"").is_err());
        assert!(Element::parse("t.xml", &[0xff, 0xfe, 0x00]).is_err());
    }

    #[test]
    fn stray_closing_tags_do_not_underflow() {
        let root = Element::parse("t.xml", b"<a></b><c/></a>").unwrap();
        assert_eq!(root.name, "a", "the first complete root wins");
    }
}
