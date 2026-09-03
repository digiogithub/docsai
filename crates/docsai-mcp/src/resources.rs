//! The contextual cheat-sheet, served as an MCP **resource** (Phase 17-G).
//!
//! Why a resource and not a tool. A tool costs every session: its schema and
//! its prose are published in `tools/list` before the session has done any
//! work, and this server already pays 2 477 tokens for that list. A resource
//! costs a session only the four bytes of `"resources":{}` in the server's
//! declared capabilities — `resources/templates/list` and `resources/read` are
//! fetched on request, by a client that wants them. A feature whose whole
//! reason to exist is a smaller token bill has no business inflating the one
//! response nobody can decline.
//!
//! The URI is a template, because the sheet is a function of a document:
//! `docmark://cheatsheet/<path>`. Reading it returns the DocMark syntax that
//! document actually uses and nothing else — see
//! [`docsai_convert::cheatsheet`] for how it is derived and what keeps it
//! honest.

use docsai_convert::cheatsheet::cheatsheet_input;
use docsai_convert::ConvertError;

use crate::config::McpConfig;
use crate::input::{resolve_document_input, ResolvedInput};

/// The URI scheme and path prefix of the cheat-sheet template.
pub const CHEATSHEET_PREFIX: &str = "docmark://cheatsheet/";

/// The RFC 6570 template `resources/templates/list` publishes.
pub const CHEATSHEET_TEMPLATE: &str = "docmark://cheatsheet/{path}";

/// The document path a cheat-sheet URI names, or `None` for any other URI.
///
/// Percent-escapes are decoded, so a path with a space reaches the reader as
/// the path the caller meant. Anything that is not valid UTF-8 after decoding
/// is refused rather than lossily replaced: a path is opened, not displayed.
pub fn cheatsheet_target(uri: &str) -> Option<Result<String, ConvertError>> {
    let rest = uri.strip_prefix(CHEATSHEET_PREFIX)?;
    Some(percent_decode(rest))
}

fn percent_decode(value: &str) -> Result<String, ConvertError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes.get(index + 1..index + 3).ok_or_else(|| {
                ConvertError::Invalid(format!("truncated percent-escape in `{value}`"))
            })?;
            let hex = std::str::from_utf8(hex).map_err(|_| {
                ConvertError::Invalid(format!("invalid percent-escape in `{value}`"))
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| {
                ConvertError::Invalid(format!("invalid percent-escape `%{hex}` in `{value}`"))
            })?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| ConvertError::Invalid(format!("`{value}` is not a UTF-8 path")))
}

/// Builds the cheat-sheet for the document a URI names.
pub fn cheatsheet(uri: &str, config: &McpConfig) -> Result<String, ConvertError> {
    let path = cheatsheet_target(uri).ok_or_else(|| {
        ConvertError::Invalid(format!(
            "unknown resource `{uri}`; this server serves `{CHEATSHEET_TEMPLATE}`"
        ))
    })??;
    if path.is_empty() {
        return Err(ConvertError::Invalid(format!(
            "`{CHEATSHEET_TEMPLATE}` needs a document path after the prefix"
        )));
    }
    // The same input resolution and the same read cache the tools use: a
    // resource must not be a second door onto the filesystem with its own
    // rules.
    let resolved = resolve_document_input(Some(&path), None, None, config)?;
    let ResolvedInput::Path(_) = &resolved else {
        return Err(ConvertError::Invalid(
            "a cheat-sheet URI names a path".into(),
        ));
    };
    let options = config.convert_options();
    cheatsheet_input(resolved.as_source(), &options)
}

/// The cheat-sheet resource, shared with the handler that publishes it.
pub fn template_description() -> &'static str {
    "DocMark syntax for one document, limited to the constructs it actually uses. \
     Path goes after the prefix: docmark://cheatsheet/report.docx"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uri_names_the_document_it_is_about() {
        assert_eq!(
            cheatsheet_target("docmark://cheatsheet/corpus/docx/basic-text.docx")
                .expect("prefix")
                .expect("decodes"),
            "corpus/docx/basic-text.docx"
        );
        assert_eq!(
            cheatsheet_target("docmark://cheatsheet/my%20report.docx")
                .expect("prefix")
                .expect("decodes"),
            "my report.docx"
        );
        assert!(cheatsheet_target("file:///etc/passwd").is_none());
    }

    #[test]
    fn a_broken_escape_is_refused_rather_than_guessed() {
        let error = cheatsheet_target("docmark://cheatsheet/a%zz.docx")
            .expect("prefix")
            .expect_err("invalid escape");
        assert!(error.to_string().contains("percent-escape"), "{error}");
    }

    #[test]
    fn an_unknown_uri_names_what_this_server_does_serve() {
        let error = cheatsheet(
            "docmark://outline/report.docx",
            &crate::config::McpConfig::default(),
        )
        .expect_err("refused");
        assert!(error.to_string().contains(CHEATSHEET_TEMPLATE), "{error}");
    }
}
