//! Reader errors.
//!
//! Phase 1 acceptance criterion: *no panics on corrupt input, always `Err`*.
//! Every failure path in this crate ends in one of these variants.

/// Something went wrong reading an Office document.
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

    /// An encrypted or protected document, refused **in front of** whichever
    /// route would have read it (Phase 19-E).
    ///
    /// Distinct from [`Encrypted`](ReadError::Encrypted), which states the fact
    /// and nothing else. This one carries the evidence the refusal rests on and
    /// what the reader of the message can do about it, because it is raised
    /// before a route is chosen — there is no later stage that would get a
    /// chance to explain it, and «unsupported» would send a user looking for a
    /// fault in a file that has none.
    #[error("{what} is encrypted or password-protected ({evidence}); {remedy}")]
    EncryptedInput {
        /// What the file is, in the user's terms rather than the format's.
        what: String,
        /// Where the refusal comes from: the field, its value and its stream.
        evidence: String,
        /// What can be done instead, in one sentence.
        remedy: String,
    },

    /// A format this build can recognise and route, but not parse itself.
    ///
    /// Introduced in Phase 19-B for legacy `.ppt`, whose only reader then lived
    /// outside the process (LibreOffice); 19-D gave `.ppt` a native reader and
    /// **nothing constructs this variant today**. It is kept rather than
    /// removed because it is public API and because its shape — name what is
    /// absent and what to do about it, instead of saying "unsupported" and
    /// sending a user looking for a fault in a file that has none — is the one
    /// the next legacy format routed before it is parsed will want. See
    /// `kb/106-phase-19-close.md`.
    #[error("no native reader for `{format}`: {missing}")]
    NoNativeRoute { format: String, missing: String },

    /// A structure inside a binary container claimed bytes the stream does not
    /// have.
    ///
    /// Phase 19 introduces it for legacy `.ppt`, where every offset a file
    /// declares is one this project must check itself: `cfb` opens
    /// `corpus/ppt/truncated.ppt` without a word. The four numbers are the
    /// diagnostic — [`WrongShape`](ReadError::WrongShape) would flatten them
    /// into prose, and «malformed» sends a reader looking in the wrong place.
    #[error(
        "`{part}` is truncated: {need} byte(s) wanted at offset {at}, but the stream holds {have}"
    )]
    Truncated {
        /// The OLE2 stream, and which structure inside it was being read.
        part: String,
        /// Absolute offset in that stream where the read started.
        at: usize,
        /// How many bytes the structure declared it needed there.
        need: usize,
        /// How many bytes the stream actually holds.
        have: usize,
    },

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
