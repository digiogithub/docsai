//! Reader errors.
//!
//! Parsers never panic on corrupt input: every failure path ends in one of these
//! variants (AGENTS.md §6).

/// Something went wrong reading an OpenDocument package.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("not a ZIP container: {0}")]
    NotAZip(String),

    #[error("required part `{0}` is missing")]
    MissingPart(String),

    #[error("malformed XML in `{part}`: {source}")]
    Xml {
        part: String,
        #[source]
        source: quick_xml::Error,
    },

    #[error("`{part}` is not valid UTF-8")]
    Encoding { part: String },

    #[error("`{part}` is not a {expected} document")]
    WrongShape { part: String, expected: String },

    #[error("document exceeds the safety limit: {0}")]
    TooLarge(String),

    #[error("document is encrypted or password-protected")]
    Encrypted,

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("asset storage: {0}")]
    Asset(#[from] docsai_model::assets::AssetError),
}

impl From<zip::result::ZipError> for ReadError {
    fn from(e: zip::result::ZipError) -> Self {
        match e {
            zip::result::ZipError::Io(io) => ReadError::Io(io),
            other => ReadError::NotAZip(other.to_string()),
        }
    }
}
