//! Escaping rules (spec §8).
//!
//! Normative and deliberately small: only characters that would change the
//! meaning of the surrounding CommonMark are escaped, with a fixed decision
//! table so that two serialisations of the same IR are byte-identical.

/// Where a run of text is being written, which decides what must be escaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextContext {
    /// Ordinary block content.
    Block,
    /// Inside a GFM table cell, where `|` also ends the cell.
    TableCell,
    /// Inside a link label, where `[` and `]` also close it early.
    LinkLabel,
}

/// Characters escaped everywhere: they open inline constructs.
const ALWAYS: &[char] = &['\\', '`', '*', '_', '[', ']', '<'];

/// Escapes `text` for the given context.
///
/// ```
/// use docsai_docmark::escape::{escape, TextContext};
/// assert_eq!(escape("a*b", TextContext::Block), r"a\*b");
/// assert_eq!(escape("a|b", TextContext::TableCell), r"a\|b");
/// assert_eq!(escape("a|b", TextContext::Block), "a|b");
/// ```
pub fn escape(text: &str, context: TextContext) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_line_start = true;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        let needs_escape = ALWAYS.contains(&c)
            || (context == TextContext::TableCell && c == '|')
            || (at_line_start && matches!(c, '#' | '>' | '-' | '+' | '=' | '~'))
            // A digit that starts a line and is followed by `.` or `)` would
            // become an ordered list marker.
            || (at_line_start
                && c.is_ascii_digit()
                && chars.peek().is_some_and(|n| matches!(n, '.' | ')')))
            // `&` only matters when it could open an entity reference.
            || (c == '&' && chars.peek().is_some_and(|n| n.is_ascii_alphanumeric() || *n == '#'));

        if needs_escape {
            out.push('\\');
        }
        out.push(c);

        at_line_start = c == '\n';
        // A leading run of spaces still counts as the line start.
        if c == ' ' && at_line_start {
            at_line_start = true;
        }
    }
    out
}

/// Escapes a value for use inside a double-quoted attribute.
pub fn escape_attr_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out
}

/// True when a value can be written without quotes: a number or a simple
/// identifier (spec §8).
pub fn is_bare_value(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '%' | '#' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_inline_markers_but_not_ordinary_punctuation() {
        assert_eq!(
            escape("*asterisco* _guion_ `code` [x] <y> \\z", TextContext::Block),
            // `>` only opens a block quote, and only at the start of a line.
            r"\*asterisco\* \_guion\_ \`code\` \[x\] \<y> \\z"
        );
        assert_eq!(
            escape("coma, punto. dos: puntos; ¿eñe?", TextContext::Block),
            "coma, punto. dos: puntos; ¿eñe?"
        );
    }

    #[test]
    fn escapes_block_markers_only_at_the_start_of_a_line() {
        assert_eq!(escape("# titulo", TextContext::Block), r"\# titulo");
        assert_eq!(escape("a # b", TextContext::Block), "a # b");
        assert_eq!(escape("- item", TextContext::Block), r"\- item");
        assert_eq!(escape("1. uno", TextContext::Block), r"\1. uno");
        assert_eq!(escape("1 uno", TextContext::Block), "1 uno", "not a marker");
        assert_eq!(escape("x\n> cita", TextContext::Block), "x\n\\> cita");
    }

    #[test]
    fn pipes_are_escaped_only_inside_table_cells() {
        assert_eq!(escape("a|b", TextContext::TableCell), r"a\|b");
        assert_eq!(escape("a|b", TextContext::Block), "a|b");
    }

    #[test]
    fn ampersands_are_escaped_only_when_they_could_be_entities() {
        assert_eq!(escape("Tom & Jerry", TextContext::Block), "Tom & Jerry");
        assert_eq!(escape("a&amp;b", TextContext::Block), r"a\&amp;b");
        assert_eq!(escape("a&#38;b", TextContext::Block), r"a\&#38;b");
    }

    #[test]
    fn attribute_values_choose_bare_or_quoted() {
        assert!(is_bare_value("450px"));
        assert!(is_bare_value("#2E74B5"));
        assert!(is_bare_value("top-bottom"));
        assert!(!is_bare_value("Logo corporativo"));
        assert!(!is_bare_value(""));
        assert_eq!(escape_attr_value(r#"a "b" \c"#), r#"a \"b\" \\c"#);
    }

    #[test]
    fn escaping_is_idempotent_in_shape() {
        // Escaping twice must not double the backslashes of the *first* pass
        // in a way the parser cannot undo: `\*` becomes `\\\*`, which unescapes
        // back to `\*`. This is the property the round-trip relies on.
        let once = escape("a*b", TextContext::Block);
        let twice = escape(&once, TextContext::Block);
        assert_eq!(twice, r"a\\\*b");
    }
}
