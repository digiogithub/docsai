//! Configurable style mapping ("publication" mode, DocMark spec §5).
//!
//! A style map projects source paragraph styles onto pure Markdown constructs
//! (`Heading1 → h1`, `SourceCode → code-block`). The transform is unidirectional
//! by definition: round-trip is not meaningful after it runs.

use std::collections::BTreeMap;
use std::path::Path;

use docsai_model::image::RawId;
use docsai_model::text::{Block, Heading, ListItem, Paragraph, RawFragment, TextDocument};
use docsai_model::{Document, Warning};

use crate::ConvertError;

/// Target construct for a mapped style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleTarget {
    /// Markdown heading level 1–6.
    Heading(u8),
    /// Keep as a paragraph but strip the style id.
    Paragraph,
    /// Drop the block entirely.
    Ignore,
    /// Emit a fenced code block (unidirectional; stored as a markdown raw fragment).
    CodeBlock,
}

impl StyleTarget {
    fn parse(value: &str) -> Option<Self> {
        let v = value.trim().to_ascii_lowercase();
        match v.as_str() {
            "p" | "paragraph" | "plain" => Some(StyleTarget::Paragraph),
            "ignore" | "skip" | "drop" => Some(StyleTarget::Ignore),
            "code" | "code-block" | "pre" => Some(StyleTarget::CodeBlock),
            "h1" | "heading1" | "title" => Some(StyleTarget::Heading(1)),
            "h2" | "heading2" => Some(StyleTarget::Heading(2)),
            "h3" | "heading3" => Some(StyleTarget::Heading(3)),
            "h4" | "heading4" => Some(StyleTarget::Heading(4)),
            "h5" | "heading5" => Some(StyleTarget::Heading(5)),
            "h6" | "heading6" => Some(StyleTarget::Heading(6)),
            _ => None,
        }
    }
}

/// Map of style id / style name → target construct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StyleMap {
    entries: BTreeMap<String, StyleTarget>,
}

impl StyleMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn insert(&mut self, key: impl Into<String>, target: StyleTarget) {
        self.entries.insert(normalize_key(&key.into()), target);
    }

    pub fn get(&self, style_id: &str, style_name: Option<&str>) -> Option<StyleTarget> {
        if let Some(t) = self.entries.get(&normalize_key(style_id)) {
            return Some(*t);
        }
        if let Some(name) = style_name {
            if let Some(t) = self.entries.get(&normalize_key(name)) {
                return Some(*t);
            }
        }
        None
    }

    /// Loads a style map from a YAML-ish file (flat `key: target` map).
    pub fn load_path(path: &Path) -> Result<Self, ConvertError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConvertError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::parse(&text).map_err(|message| {
            ConvertError::Invalid(format!("invalid style map `{}`: {message}", path.display()))
        })
    }

    /// Parses the style-map text format.
    ///
    /// Accepted shape (comments and blank lines allowed):
    ///
    /// ```text
    /// Heading1: h1
    /// "Heading 1": h1
    /// Title: h1
    /// SourceCode: code-block
    /// Comment: ignore
    /// ```
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut map = StyleMap::new();
        for (line_no, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let (key, value) = split_kv(line).ok_or_else(|| {
                format!(
                    "line {}: expected `StyleName: target` (h1–h6, p, ignore, code-block)",
                    line_no + 1
                )
            })?;
            let target = StyleTarget::parse(value).ok_or_else(|| {
                format!(
                    "line {}: unknown target `{value}`; use h1–h6, p, ignore or code-block",
                    line_no + 1
                )
            })?;
            map.insert(unquote(key), target);
        }
        Ok(map)
    }
}

/// Applies the style map to a text document in place.
///
/// Workbooks are left unchanged (style maps are a text-publication feature).
/// Returns warnings describing the unidirectional transform.
pub fn apply_style_map(document: &mut Document, style_map: &StyleMap) -> Vec<Warning> {
    if style_map.is_empty() {
        return Vec::new();
    }
    let mut warnings = vec![Warning::Degraded {
        what: "style-map".into(),
        why: "publication style map applied; output is unidirectional and not round-trip safe"
            .into(),
    }];

    match document {
        Document::Workbook(_) => {
            warnings.push(Warning::Degraded {
                what: "style-map".into(),
                why: "style maps apply to text documents only; workbook left unchanged".into(),
            });
        }
        Document::Text(text) => {
            let name_index = style_name_index(text);
            let mut code_seq = 0u32;
            for section in &mut text.sections {
                section.blocks = map_blocks(
                    std::mem::take(&mut section.blocks),
                    style_map,
                    &name_index,
                    &mut code_seq,
                );
                for header in &mut section.headers {
                    header.blocks = map_blocks(
                        std::mem::take(&mut header.blocks),
                        style_map,
                        &name_index,
                        &mut code_seq,
                    );
                }
                for footer in &mut section.footers {
                    footer.blocks = map_blocks(
                        std::mem::take(&mut footer.blocks),
                        style_map,
                        &name_index,
                        &mut code_seq,
                    );
                }
            }
        }
    }
    warnings
}

fn style_name_index(text: &TextDocument) -> BTreeMap<String, String> {
    text.styles
        .styles
        .values()
        .map(|s| (s.id.as_str().to_string(), s.name.clone()))
        .collect()
}

fn map_blocks(
    blocks: Vec<Block>,
    style_map: &StyleMap,
    names: &BTreeMap<String, String>,
    code_seq: &mut u32,
) -> Vec<Block> {
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                if let Some(mapped) = map_paragraph(paragraph, style_map, names, code_seq) {
                    out.push(mapped);
                }
            }
            Block::Heading(mut heading) => {
                // Allow mapping a heading's paragraph style as well.
                if let Some(style) = heading.paragraph.format.style.clone() {
                    let name = names.get(style.as_str()).map(String::as_str);
                    if let Some(target) = style_map.get(style.as_str(), name) {
                        match target {
                            StyleTarget::Ignore => continue,
                            StyleTarget::Heading(level) => {
                                heading.level = level;
                                heading.paragraph.format.style = None;
                                out.push(Block::Heading(heading));
                                continue;
                            }
                            StyleTarget::Paragraph => {
                                heading.paragraph.format.style = None;
                                out.push(Block::Paragraph(heading.paragraph));
                                continue;
                            }
                            StyleTarget::CodeBlock => {
                                if let Some(b) = paragraph_to_code(heading.paragraph, code_seq) {
                                    out.push(b);
                                }
                                continue;
                            }
                        }
                    }
                }
                out.push(Block::Heading(heading));
            }
            Block::List(mut list) => {
                list.items = list
                    .items
                    .into_iter()
                    .map(|item| ListItem {
                        blocks: map_blocks(item.blocks, style_map, names, code_seq),
                    })
                    .collect();
                out.push(Block::List(list));
            }
            Block::Table(mut table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        cell.blocks = map_blocks(
                            std::mem::take(&mut cell.blocks),
                            style_map,
                            names,
                            code_seq,
                        );
                    }
                }
                out.push(Block::Table(table));
            }
            Block::TextBox(mut tb) => {
                tb.blocks = map_blocks(std::mem::take(&mut tb.blocks), style_map, names, code_seq);
                out.push(Block::TextBox(tb));
            }
            other => out.push(other),
        }
    }
    out
}

fn map_paragraph(
    mut paragraph: Paragraph,
    style_map: &StyleMap,
    names: &BTreeMap<String, String>,
    code_seq: &mut u32,
) -> Option<Block> {
    let Some(style) = paragraph.format.style.clone() else {
        return Some(Block::Paragraph(paragraph));
    };
    let name = names.get(style.as_str()).map(String::as_str);
    let Some(target) = style_map.get(style.as_str(), name) else {
        return Some(Block::Paragraph(paragraph));
    };
    match target {
        StyleTarget::Ignore => None,
        StyleTarget::Paragraph => {
            paragraph.format.style = None;
            Some(Block::Paragraph(paragraph))
        }
        StyleTarget::Heading(level) => {
            paragraph.format.style = None;
            Some(Block::Heading(Heading { level, paragraph }))
        }
        StyleTarget::CodeBlock => paragraph_to_code(paragraph, code_seq),
    }
}

fn paragraph_to_code(paragraph: Paragraph, code_seq: &mut u32) -> Option<Block> {
    let text = paragraph.plain_text();
    *code_seq = code_seq.saturating_add(1);
    let id = format!("style-map-code-{code_seq:04}");
    Some(Block::Raw(RawFragment {
        id: RawId(id),
        format: "markdown".into(),
        part: "style-map".into(),
        content: format!("```\n{text}\n```"),
    }))
}

fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

fn strip_comment(line: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return &line[..i],
            _ => {}
        }
        i += 1;
    }
    line
}

fn split_kv(line: &str) -> Option<(&str, &str)> {
    // Find the first bare `:`.
    let mut in_single = false;
    let mut in_double = false;
    for (idx, c) in line.char_indices() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ':' if !in_single && !in_double => {
                let key = line[..idx].trim();
                let value = line[idx + 1..].trim();
                if key.is_empty() || value.is_empty() {
                    return None;
                }
                return Some((key, value));
            }
            _ => {}
        }
    }
    None
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[s.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'')
        {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::style::{Style, StyleCatalog, StyleType};
    use docsai_model::text::{Section, TextDocument};

    #[test]
    fn parses_flat_style_map() {
        let map = StyleMap::parse(
            r#"
# comment
Heading1: h1
"Source Code": code-block
Comment: ignore
BodyText: p
"#,
        )
        .unwrap();
        assert_eq!(map.get("Heading1", None), Some(StyleTarget::Heading(1)));
        assert_eq!(map.get("Source Code", None), Some(StyleTarget::CodeBlock));
        assert_eq!(map.get("Comment", None), Some(StyleTarget::Ignore));
        assert_eq!(map.get("BodyText", None), Some(StyleTarget::Paragraph));
    }

    #[test]
    fn maps_paragraph_styles_to_headings() {
        let mut styles = StyleCatalog::default();
        let mut heading = Style::new("Heading1", StyleType::Paragraph);
        heading.name = "heading 1".into();
        styles.insert(heading);
        let mut doc = Document::Text(TextDocument {
            styles,
            sections: vec![Section {
                blocks: vec![Block::Paragraph(Paragraph {
                    format: docsai_model::text::ParaFormat::styled("Heading1"),
                    content: vec![docsai_model::text::Inline::Text("Title".into())],
                })],
                ..Default::default()
            }],
            ..Default::default()
        });
        let map = StyleMap::parse("Heading1: h1").unwrap();
        let warnings = apply_style_map(&mut doc, &map);
        assert!(!warnings.is_empty());
        match &doc {
            Document::Text(text) => match &text.sections[0].blocks[0] {
                Block::Heading(h) => {
                    assert_eq!(h.level, 1);
                    assert!(h.paragraph.format.style.is_none());
                    assert_eq!(h.paragraph.plain_text(), "Title");
                }
                other => panic!("expected heading, got {other:?}"),
            },
            _ => panic!("text"),
        }
    }
}
