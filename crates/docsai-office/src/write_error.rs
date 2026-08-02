//! Writer errors.

/// Something went wrong writing an Office document.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("unsupported write format: {0}")]
    Unsupported(String),

    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    #[error("zip error: {0}")]
    Zip(String),

    #[error("asset error: {0}")]
    Asset(#[from] docsai_model::assets::AssetError),

    #[error("invalid document: {0}")]
    Invalid(String),
}

impl From<zip::result::ZipError> for WriteError {
    fn from(e: zip::result::ZipError) -> Self {
        WriteError::Zip(e.to_string())
    }
}
