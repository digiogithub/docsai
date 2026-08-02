//! A reader for the YAML subset DocMark's front matter uses (spec §2).
//!
//! The emitter in [`crate::frontmatter`] is written by hand and produces a
//! small, fully known shape: block mappings, flow mappings, sequences of flow
//! mappings, and double-quoted or bare scalars. This reads exactly that back,
//! which keeps an unmaintained YAML crate out of the dependency tree and keeps
//! both halves of the round-trip under one roof.
//!
//! What it deliberately does *not* do: anchors, tags, multi-line scalars, flow
//! sequences. None of them can appear in a document docsai wrote; a document
//! that has them reports the line rather than guessing.

use std::collections::BTreeMap;

use super::ParseError;

/// A YAML value, in the three shapes the front matter uses.
#[derive(Debug, Clone, PartialEq)]
pub enum Yaml {
    Scalar(String),
    Map(BTreeMap<String, Yaml>),
    Seq(Vec<Yaml>),
}

impl Yaml {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Yaml::Scalar(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, Yaml>> {
        match self {
            Yaml::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Yaml]> {
        match self {
            Yaml::Seq(items) => Some(items),
            _ => None,
        }
    }

    /// A child of a mapping.
    pub fn get(&self, key: &str) -> Option<&Yaml> {
        self.as_map()?.get(key)
    }

    /// A child scalar.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    /// A child scalar, owned.
    pub fn string(&self, key: &str) -> Option<String> {
        self.str(key).map(str::to_string)
    }

    /// A child boolean; anything other than `true` reads as `false`.
    pub fn bool(&self, key: &str) -> Option<bool> {
        self.str(key).map(|v| v == "true")
    }
}

/// One line, already stripped of its indentation.
struct Line<'a> {
    indent: usize,
    text: &'a str,
    number: usize,
}

/// Parses a front-matter body (delimiters excluded) into a mapping.
pub fn parse(source: &str) -> Result<Yaml, ParseError> {
    let lines: Vec<Line> = source
        .lines()
        .enumerate()
        .map(|(index, raw)| Line {
            indent: raw.len() - raw.trim_start_matches(' ').len(),
            text: raw.trim_end(),
            number: index + 1,
        })
        // Blank lines and comments carry no structure.
        .filter(|l| !l.text.trim().is_empty() && !l.text.trim_start().starts_with('#'))
        .collect();

    let mut cursor = 0usize;
    let map = parse_block(&lines, &mut cursor, 0)?;
    Ok(map)
}

/// Parses every line at `indent` or deeper into one mapping or sequence.
fn parse_block(lines: &[Line], cursor: &mut usize, indent: usize) -> Result<Yaml, ParseError> {
    // A block starting with `- ` is a sequence, not a mapping.
    if lines
        .get(*cursor)
        .is_some_and(|l| l.indent >= indent && l.text.trim_start().starts_with("- "))
    {
        return parse_seq(lines, cursor, indent);
    }

    let mut map = BTreeMap::new();
    while let Some(line) = lines.get(*cursor) {
        if line.indent < indent {
            break;
        }
        let content = line.text.trim_start();
        let (key, rest) = split_key(content).ok_or_else(|| ParseError::FrontMatter {
            line: line.number,
            message: format!("expected `key: value`, found `{content}`"),
        })?;
        let key = read_scalar(key);
        *cursor += 1;

        let value = if rest.is_empty() {
            // The value is the indented block that follows, if there is one.
            match lines.get(*cursor).filter(|l| l.indent > line.indent) {
                Some(next) => {
                    let child_indent = next.indent;
                    parse_block(lines, cursor, child_indent)?
                }
                None => Yaml::Scalar(String::new()),
            }
        } else {
            parse_inline(rest, line.number)?
        };
        map.insert(key, value);
    }
    Ok(Yaml::Map(map))
}

fn parse_seq(lines: &[Line], cursor: &mut usize, indent: usize) -> Result<Yaml, ParseError> {
    let mut items = Vec::new();
    while let Some(line) = lines.get(*cursor) {
        if line.indent < indent {
            break;
        }
        let Some(rest) = line.text.trim_start().strip_prefix("- ") else {
            break;
        };
        *cursor += 1;
        items.push(parse_inline(rest.trim(), line.number)?);
    }
    Ok(Yaml::Seq(items))
}

/// Parses a value written on the same line as its key.
fn parse_inline(text: &str, line: usize) -> Result<Yaml, ParseError> {
    let text = text.trim();
    if let Some(body) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
        let mut map = BTreeMap::new();
        for entry in split_flow(body) {
            let Some((key, value)) = split_key(&entry) else {
                return Err(ParseError::FrontMatter {
                    line,
                    message: format!("expected `key: value` inside `{{ … }}`, found `{entry}`"),
                });
            };
            map.insert(read_scalar(key), Yaml::Scalar(read_scalar(value)));
        }
        return Ok(Yaml::Map(map));
    }
    Ok(Yaml::Scalar(read_scalar(text)))
}

/// Splits `key: value`, honouring quotes so that a `:` inside a quoted key or
/// value (an ISO timestamp, a URL) does not split it.
fn split_key(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut in_quotes = false;
    let mut depth = 0usize;
    for index in 0..bytes.len() {
        match bytes[index] {
            b'"' if index == 0 || bytes[index - 1] != b'\\' => in_quotes = !in_quotes,
            b'{' if !in_quotes => depth += 1,
            b'}' if !in_quotes => depth = depth.saturating_sub(1),
            // Only a colon *followed by a space or end of line* separates a
            // key: `10:00:00Z` inside a timestamp must survive.
            b':' if !in_quotes && depth == 0 && bytes.get(index + 1).is_none_or(|c| *c == b' ') => {
                return Some((&text[..index], text[index + 1..].trim()));
            }
            _ => {}
        }
    }
    None
}

/// Splits a flow mapping's body on commas, honouring quotes and nesting.
fn split_flow(body: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut depth = 0usize;
    for c in body.chars() {
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
            '{' if !in_quotes => {
                depth += 1;
                current.push(c);
            }
            '}' if !in_quotes => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if !in_quotes && depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    parts
}

/// Unwraps a scalar: double-quoted values are unescaped, bare ones trimmed.
fn read_scalar(text: &str) -> String {
    let text = text.trim();
    match text.strip_prefix('"').and_then(|t| t.strip_suffix('"')) {
        Some(quoted) => {
            let mut out = String::with_capacity(quoted.len());
            let mut chars = quoted.chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some('n') => out.push('\n'),
                        Some('r') => out.push('\r'),
                        Some('t') => out.push('\t'),
                        Some(other) => out.push(other),
                        None => out.push('\\'),
                    },
                    _ => out.push(c),
                }
            }
            out
        }
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_scalars_and_the_nesting() {
        let yaml = parse(
            r#"docmark: "1.0"
source-format: docx
created: 2026-01-01T00:00:00Z
custom-properties:
  Departamento: "Ventas"
  "clave rara": "x"
page:
  size: A4
  margins: { top: 2.5cm, bottom: 2.5cm }
  orientation: portrait
"#,
        )
        .expect("parses");

        assert_eq!(yaml.str("docmark"), Some("1.0"));
        assert_eq!(
            yaml.str("created"),
            Some("2026-01-01T00:00:00Z"),
            "a timestamp's colons are not key separators"
        );
        let custom = yaml.get("custom-properties").expect("custom");
        assert_eq!(custom.str("Departamento"), Some("Ventas"));
        assert_eq!(custom.str("clave rara"), Some("x"));
        let page = yaml.get("page").expect("page");
        assert_eq!(page.str("size"), Some("A4"));
        assert_eq!(
            page.get("margins").and_then(|m| m.str("top")),
            Some("2.5cm")
        );
    }

    #[test]
    fn reads_sequences_of_flow_mappings() {
        let yaml = parse(
            r#"list-definitions:
  L1:
    levels:
      - { format: decimal, text: "%1.", start: 1, indent: 48px }
      - { format: lowerLetter, text: "%2)" }
"#,
        )
        .expect("parses");
        let levels = yaml
            .get("list-definitions")
            .and_then(|d| d.get("L1"))
            .and_then(|l| l.get("levels"))
            .and_then(Yaml::as_seq)
            .expect("levels");
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0].str("format"), Some("decimal"));
        assert_eq!(levels[0].str("text"), Some("%1."));
        assert_eq!(levels[1].str("text"), Some("%2)"));
    }

    #[test]
    fn quoted_values_keep_their_commas_and_braces() {
        let yaml = parse(r##"style: { name: "Uno, dos", color: "#2E74B5" }"##).expect("parses");
        let style = yaml.get("style").expect("style");
        assert_eq!(style.str("name"), Some("Uno, dos"));
        assert_eq!(style.str("color"), Some("#2E74B5"));
    }

    #[test]
    fn escapes_inside_quoted_scalars_are_undone() {
        let yaml = parse(r#"title: "Informe \"Anual\"""#).expect("parses");
        assert_eq!(yaml.str("title"), Some(r#"Informe "Anual""#));
    }

    #[test]
    fn a_line_that_is_not_a_mapping_reports_where() {
        let error = parse("bien: 1\nesto no es yaml\n").unwrap_err();
        let ParseError::FrontMatter { line, .. } = error else {
            panic!("expected a front-matter error, got {error:?}");
        };
        assert_eq!(line, 2);
    }

    #[test]
    fn blank_lines_and_comments_are_ignored() {
        let yaml = parse("a: 1\n\n# un comentario\nb: 2\n").expect("parses");
        assert_eq!(yaml.str("a"), Some("1"));
        assert_eq!(yaml.str("b"), Some("2"));
    }
}
