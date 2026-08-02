//! Parser errors for DocMark → IR.

use std::io;
use std::path::PathBuf;

/// Something went wrong parsing a DocMark document.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid front matter at line {line}: {message}")]
    InvalidFrontMatter { line: usize, message: String },

    #[error("unexpected content at line {line}: {message}")]
    Unexpected { line: usize, message: String },

    #[error("unsupported: {what}")]
    Unsupported { what: String },

    #[error("i/o error{path}: {source}", path = path_suffix(.path))]
    Io {
        path: Option<PathBuf>,
        #[source]
        source: io::Error,
    },

    #[error("asset error: {0}")]
    Asset(#[from] docsai_model::assets::AssetError),
}

fn path_suffix(path: &Option<PathBuf>) -> String {
    match path {
        Some(p) => format!(" on `{}`", p.display()),
        None => String::new(),
    }
}

impl ParseError {
    pub fn front_matter(line: usize, message: impl Into<String>) -> Self {
        ParseError::InvalidFrontMatter {
            line,
            message: message.into(),
        }
    }

    pub fn unexpected(line: usize, message: impl Into<String>) -> Self {
        ParseError::Unexpected {
            line,
            message: message.into(),
        }
    }

    pub fn io(path: Option<PathBuf>, source: io::Error) -> Self {
        ParseError::Io { path, source }
    }
}
