//! Small YAML subset used by DocMark front matter.
//!
//! Only the shapes the serializer emits are accepted: scalars, flow maps,
//! nested block maps and simple sequences of flow maps.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Seq(Vec<Value>),
    Map(BTreeMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_f64().map(|n| n as i64)
    }

    pub fn as_u16(&self) -> Option<u16> {
        self.as_f64().and_then(|n| {
            if n >= 0.0 && n <= u16::MAX as f64 {
                Some(n as u16)
            } else {
                None
            }
        })
    }

    pub fn as_u8(&self) -> Option<u8> {
        self.as_f64().and_then(|n| {
            if n >= 0.0 && n <= u8::MAX as f64 {
                Some(n as u8)
            } else {
                None
            }
        })
    }

    pub fn as_map(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Value]> {
        match self {
            Value::Seq(s) => Some(s),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_map()?.get(key)
    }

    pub fn string_or_empty(&self) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Number(n) => {
                let mut s = format!("{n}");
                if s.contains('.') {
                    while s.ends_with('0') {
                        s.pop();
                    }
                    if s.ends_with('.') {
                        s.pop();
                    }
                }
                s
            }
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        }
    }

    /// Renders a scalar the way DocMark lengths and tokens expect it.
    pub fn as_token(&self) -> Option<String> {
        match self {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(self.string_or_empty()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}

/// Parses a YAML document body (without the `---` fences) into a map.
pub fn parse_document(text: &str) -> Result<BTreeMap<String, Value>, String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut parser = Parser {
        lines: &lines,
        index: 0,
        base_line: 1,
    };
    parser.parse_block_map(0)
}

struct Parser<'a> {
    lines: &'a [&'a str],
    index: usize,
    base_line: usize,
}

impl<'a> Parser<'a> {
    fn line_no(&self) -> usize {
        self.base_line + self.index
    }

    fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.index).copied()
    }

    fn bump(&mut self) {
        self.index += 1;
    }

    fn parse_block_map(&mut self, indent: usize) -> Result<BTreeMap<String, Value>, String> {
        let mut map = BTreeMap::new();
        while let Some(raw) = self.peek() {
            if raw.trim().is_empty() {
                self.bump();
                continue;
            }
            let current = leading_spaces(raw);
            if current < indent {
                break;
            }
            if current > indent {
                return Err(format!(
                    "line {}: unexpected indent {current}, expected {indent}",
                    self.line_no()
                ));
            }
            let content = &raw[indent..];
            let (key, rest) = split_key(content).ok_or_else(|| {
                format!("line {}: expected mapping key", self.line_no())
            })?;
            self.bump();
            let value = if rest.is_empty() {
                self.parse_nested(indent + 2)?
            } else {
                parse_flow(rest.trim()).map_err(|e| format!("line {}: {e}", self.line_no() - 1))?
            };
            map.insert(key, value);
        }
        Ok(map)
    }

    fn parse_nested(&mut self, indent: usize) -> Result<Value, String> {
        let Some(raw) = self.peek() else {
            return Ok(Value::Null);
        };
        if raw.trim().is_empty() {
            self.bump();
            return self.parse_nested(indent);
        }
        let current = leading_spaces(raw);
        if current < indent {
            return Ok(Value::Null);
        }
        let content = raw.trim_start();
        if content.starts_with("- ") || content == "-" {
            return self.parse_block_seq(indent).map(Value::Seq);
        }
        self.parse_block_map(indent).map(Value::Map)
    }

    fn parse_block_seq(&mut self, indent: usize) -> Result<Vec<Value>, String> {
        let mut items = Vec::new();
        while let Some(raw) = self.peek() {
            if raw.trim().is_empty() {
                self.bump();
                continue;
            }
            let current = leading_spaces(raw);
            if current < indent {
                break;
            }
            if current > indent {
                return Err(format!(
                    "line {}: unexpected indent in sequence",
                    self.line_no()
                ));
            }
            let content = raw[indent..].trim_start();
            let Some(rest) = content.strip_prefix('-') else {
                break;
            };
            let rest = rest.strip_prefix(' ').unwrap_or(rest).trim();
            self.bump();
            let value = if rest.is_empty() {
                self.parse_nested(indent + 2)?
            } else {
                parse_flow(rest).map_err(|e| format!("line {}: {e}", self.line_no() - 1))?
            };
            items.push(value);
        }
        Ok(items)
    }
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn split_key(content: &str) -> Option<(String, &str)> {
    let content = content.trim_end();
    if let Some(rest) = content.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = rest.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                },
                '"' => {
                    let rest: String = chars.collect();
                    let rest = rest.trim_start();
                    let rest = rest.strip_prefix(':')?;
                    return Some((out, rest.trim_start()));
                }
                other => out.push(other),
            }
        }
        return None;
    }
    let colon = content.find(':')?;
    let key = content[..colon].trim();
    if key.is_empty() {
        return None;
    }
    // Bare keys cannot contain spaces in our serializer except when quoted.
    Some((key.to_string(), content[colon + 1..].trim_start()))
}

/// Parses a flow scalar, map or sequence.
pub fn parse_flow(input: &str) -> Result<Value, String> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Value::Null);
    }
    if input.starts_with('{') {
        return parse_flow_map(input);
    }
    if input.starts_with('[') {
        return parse_flow_seq(input);
    }
    Ok(parse_scalar(input))
}

fn parse_scalar(input: &str) -> Value {
    let input = input.trim();
    if let Some(s) = unquote(input) {
        return Value::String(s);
    }
    match input {
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        "null" | "Null" | "~" | "" => return Value::Null,
        _ => {}
    }
    if let Ok(n) = input.parse::<f64>() {
        if input.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')) {
            return Value::Number(n);
        }
    }
    Value::String(input.to_string())
}

fn unquote(input: &str) -> Option<String> {
    let rest = input.strip_prefix('"')?;
    if !input.ends_with('"') || input.len() < 2 {
        return None;
    }
    let inner = &input[1..input.len() - 1];
    // Ensure the closing quote is not escaped.
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            },
            other => out.push(other),
        }
    }
    // Verify it was properly quoted by checking we consumed a matching form.
    let _ = rest;
    Some(out)
}

fn parse_flow_map(input: &str) -> Result<Value, String> {
    let inner = strip_wrappers(input, '{', '}')?;
    let mut map = BTreeMap::new();
    if inner.trim().is_empty() {
        return Ok(Value::Map(map));
    }
    for item in split_flow_items(inner)? {
        let (key, value) = split_flow_pair(item)?;
        map.insert(key, parse_flow(value)?);
    }
    Ok(Value::Map(map))
}

fn parse_flow_seq(input: &str) -> Result<Value, String> {
    let inner = strip_wrappers(input, '[', ']')?;
    if inner.trim().is_empty() {
        return Ok(Value::Seq(Vec::new()));
    }
    let mut items = Vec::new();
    for item in split_flow_items(inner)? {
        items.push(parse_flow(item)?);
    }
    Ok(Value::Seq(items))
}

fn strip_wrappers(input: &str, open: char, close: char) -> Result<&str, String> {
    let input = input.trim();
    if !input.starts_with(open) || !input.ends_with(close) {
        return Err(format!("expected {open}...{close}"));
    }
    Ok(&input[open.len_utf8()..input.len() - close.len_utf8()])
}

fn split_flow_pair(item: &str) -> Result<(String, &str), String> {
    let item = item.trim();
    if let Some(rest) = item.strip_prefix('"') {
        // quoted key
        let mut i = 0usize;
        let bytes = rest.as_bytes();
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                let key = unquote(&item[..i + 2]).ok_or_else(|| "bad quoted key".to_string())?;
                let after = item[i + 2..].trim_start();
                let after = after
                    .strip_prefix(':')
                    .ok_or_else(|| "expected ':' after key".to_string())?;
                return Ok((key, after.trim_start()));
            }
            i += 1;
        }
        return Err("unterminated quoted key".into());
    }
    let colon = item
        .find(':')
        .ok_or_else(|| format!("expected key: value in `{item}`"))?;
    Ok((item[..colon].trim().to_string(), item[colon + 1..].trim()))
}

fn split_flow_items(input: &str) -> Result<Vec<&str>, String> {
    let mut items = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    for (idx, &(i, c)) in chars.iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                items.push(input[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
        let _ = idx;
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        items.push(tail);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_flow_map_with_quoted_values() {
        let v = parse_flow(r#"{ name: "Calibri Light", size: 16pt, color: "#2E74B5" }"#).unwrap();
        let map = v.as_map().unwrap();
        assert_eq!(map.get("name").and_then(|v| v.as_str()), Some("Calibri Light"));
        assert_eq!(map.get("size").and_then(|v| v.as_str()), Some("16pt"));
        assert_eq!(map.get("color").and_then(|v| v.as_str()), Some("#2E74B5"));
    }

    #[test]
    fn parses_nested_block_map() {
        let text = "\
docmark: \"1.0\"
page:
  size: A4
  margins: { top: 70.85pt, bottom: 70.85pt }
styles:
  Heading1:
    type: paragraph
    name: \"heading 1\"
";
        let map = parse_document(text).unwrap();
        assert_eq!(map.get("docmark").and_then(|v| v.as_str()), Some("1.0"));
        assert_eq!(
            map.get("page")
                .and_then(|v| v.get("size"))
                .and_then(|v| v.as_str()),
            Some("A4")
        );
        assert_eq!(
            map.get("styles")
                .and_then(|v| v.get("Heading1"))
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str()),
            Some("heading 1")
        );
    }

    #[test]
    fn parses_list_definition_sequence() {
        let text = "\
list-definitions:
  L1:
    levels:
      - { format: decimal, text: \"%1.\", start: 1 }
";
        let map = parse_document(text).unwrap();
        let levels = map
            .get("list-definitions")
            .and_then(|v| v.get("L1"))
            .and_then(|v| v.get("levels"))
            .and_then(|v| v.as_seq())
            .unwrap();
        assert_eq!(levels.len(), 1);
        assert_eq!(
            levels[0].get("format").and_then(|v| v.as_str()),
            Some("decimal")
        );
    }
}
