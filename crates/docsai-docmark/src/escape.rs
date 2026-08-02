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
}

// A link label needs no context of its own: `[` and `]` are escaped
// everywhere, so a label escapes exactly like the text around it — and a label
// *inside* a table cell must still escape `|`, which a label-specific context
// would have got wrong.

/// Characters escaped everywhere: they open inline constructs.
///
/// `~` is here for the same reason as `*`: a literal one next to the markers
/// of a strike-through run would join them into a longer delimiter run, and
/// `~~~A~~` is not strike-through at all.
const ALWAYS: &[char] = &['\\', '`', '*', '_', '[', ']', '<', '~'];

/// True for the characters escaped wherever they appear.
///
/// Whoever asks what a run of text *renders* has to know: an escaped character
/// is written as a backslash and the character, so its first byte is `\`, not
/// the character itself.
pub fn is_always_escaped(c: char) -> bool {
    ALWAYS.contains(&c)
}

/// Escapes `text` for the given context, as if it started a line.
///
/// ```
/// use docsai_docmark::escape::{escape, TextContext};
/// assert_eq!(escape("a*b", TextContext::Block), r"a\*b");
/// assert_eq!(escape("a|b", TextContext::TableCell), r"a\|b");
/// assert_eq!(escape("a|b", TextContext::Block), "a|b");
/// ```
pub fn escape(text: &str, context: TextContext) -> String {
    escape_at(text, context, true)
}

/// Escapes `text`, told whether it lands at the start of a line.
///
/// The distinction matters because `#`, `>`, `-` and friends only open a block
/// when a line starts with them. The caller has to say so, rather than each run
/// assuming it begins a line, or the output would depend on how a paragraph
/// happens to be split into runs — and that is not information the format
/// carries: `[Text("a"), Text("#")]` and `[Text("a#")]` must write the same
/// bytes.
///
/// ```
/// use docsai_docmark::escape::{escape_at, TextContext};
/// assert_eq!(escape_at("# titulo", TextContext::Block, true), r"\# titulo");
/// assert_eq!(escape_at("# titulo", TextContext::Block, false), "# titulo");
/// ```
pub fn escape_at(text: &str, context: TextContext, starts_line: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at_line_start = starts_line;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        let needs_escape = ALWAYS.contains(&c)
            || (context == TextContext::TableCell && c == '|')
            || (at_line_start && matches!(c, '#' | '>' | '-' | '+' | '='))
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

/// Undoes [`escape`]: drops the backslash of every `\X` pair.
///
/// The inverse is exact because [`escape`] escapes `\` itself first, so a
/// backslash in the source always arrives doubled and can never be mistaken for
/// an escape marker.
///
/// ```
/// use docsai_docmark::escape::{escape, unescape, TextContext};
/// let original = r"a*b\c";
/// assert_eq!(unescape(&escape(original, TextContext::Block)), original);
/// ```
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(next) => out.push(next),
                // A trailing lone backslash is content, not a broken escape.
                None => out.push('\\'),
            },
            _ => out.push(c),
        }
    }
    out
}

/// Undoes [`escape_attr_value`].
pub fn unescape_attr_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some(next) => out.push(next),
                None => out.push('\\'),
            },
            _ => out.push(c),
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

/// CommonMark's left-flanking test: a delimiter run can *open* emphasis.
///
/// `before` and `after` are the characters either side of the run, `None` at
/// the start or end of the line.
pub fn left_flanking(before: Option<char>, after: Option<char>) -> bool {
    let Some(after) = after else { return false };
    if after.is_whitespace() {
        return false;
    }
    !is_markdown_punctuation(after)
        || before.is_none_or(|b| b.is_whitespace() || is_markdown_punctuation(b))
}

/// CommonMark's right-flanking test: a delimiter run can *close* emphasis.
pub fn right_flanking(before: Option<char>, after: Option<char>) -> bool {
    let Some(before) = before else { return false };
    if before.is_whitespace() {
        return false;
    }
    !is_markdown_punctuation(before)
        || after.is_none_or(|a| a.is_whitespace() || is_markdown_punctuation(a))
}

/// CommonMark punctuation: ASCII punctuation plus every Unicode symbol and
/// punctuation mark, which is what makes `**¡Hola!**` work.
pub fn is_markdown_punctuation(c: char) -> bool {
    c.is_ascii_punctuation() || (!c.is_alphanumeric() && !c.is_whitespace())
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
            escape(
                "*asterisco* _guion_ `code` [x] <y> ~z \\w",
                TextContext::Block
            ),
            // `>` only opens a block quote, and only at the start of a line.
            r"\*asterisco\* \_guion\_ \`code\` \[x\] \<y> \~z \\w"
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
    fn unescape_is_the_exact_inverse_of_escape() {
        // Every context, because the escape table differs between them and the
        // parser has only one `unescape` to undo all three.
        let samples = [
            r"*asterisco* _guion_ `code` [x] <y> \z",
            "# titulo",
            "1. uno",
            "a|b",
            "Tom & Jerry",
            "a&amp;b",
            r"barra final \",
            "linea\nsiguiente > cita",
        ];
        for sample in samples {
            for context in [TextContext::Block, TextContext::TableCell] {
                let escaped = escape(sample, context);
                assert_eq!(unescape(&escaped), sample, "`{sample}` in {context:?}");
            }
        }
    }

    #[test]
    fn attribute_values_round_trip() {
        for sample in [r#"a "b" \c"#, "una\nlinea", "Logo corporativo", ""] {
            assert_eq!(unescape_attr_value(&escape_attr_value(sample)), sample);
        }
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
