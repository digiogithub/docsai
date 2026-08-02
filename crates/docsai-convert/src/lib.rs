//! Conversion orchestration: format detection, pipelines and asset management.
//!
//! This is the only crate that knows about more than one format; the readers
//! and writers never talk to each other (architecture §1).

#![forbid(unsafe_code)]

pub mod assets;
mod pipeline;

pub use assets::DirAssetStore;
pub use docsai_docmark::{Fidelity, Options as DocMarkOptions};
pub use pipeline::{
    convert_file, read_document, read_path, roundtrip_file, ConvertOptions, Outcome,
    RoundtripOutcome,
};

use docsai_model::Format;

/// Errors the orchestration layer can return.
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("{0}")]
    Read(#[from] docsai_office::ReadError),

    #[error("{0}")]
    Write(#[from] docsai_office::WriteError),

    #[error("{0}")]
    Parse(#[from] docsai_docmark::ParseError),

    #[error("i/o error on `{path}`: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("unsupported conversion: {from} -> {to}")]
    Unsupported { from: Format, to: Format },

    #[error("could not tell what kind of document `{0}` is")]
    UnknownFormat(String),

    #[error("the produced document breaks an IR invariant: {0}")]
    Invalid(String),
}

/// One row of the support matrix shown by `docsai formats`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatSupport {
    pub format: Format,
    pub read: bool,
    pub write: bool,
    /// Phase of `docs/development-plan.md` that lands the missing direction.
    pub note: &'static str,
}

/// The support matrix as of this build.
pub const SUPPORT: &[FormatSupport] = &[
    FormatSupport {
        format: Format::Docx,
        read: true,
        write: true,
        note: "Phase 2",
    },
    FormatSupport {
        format: Format::Doc,
        read: false,
        write: false,
        note: "reading arrives in Phase 5; writing is out of scope",
    },
    FormatSupport {
        format: Format::Xlsx,
        read: true,
        write: true,
        note: "Phase 3",
    },
    FormatSupport {
        format: Format::Xls,
        read: true,
        write: false,
        note: "Phase 3 read-only; writing is out of scope",
    },
    FormatSupport {
        format: Format::Odt,
        read: false,
        write: false,
        note: "arrives in Phase 4",
    },
    FormatSupport {
        format: Format::Ods,
        read: false,
        write: false,
        note: "arrives in Phase 4",
    },
    FormatSupport {
        format: Format::DocMark,
        read: true,
        write: true,
        note: "Phase 2",
    },
];

/// Whether this build can read a format.
pub fn can_read(format: Format) -> bool {
    SUPPORT.iter().any(|s| s.format == format && s.read)
}

/// Whether this build can write a format.
pub fn can_write(format: Format) -> bool {
    SUPPORT.iter().any(|s| s.format == format && s.write)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_matrix_matches_what_the_crates_actually_do() {
        // The matrix is what `docsai formats` prints, so it must not drift
        // ahead of the readers.
        for support in SUPPORT.iter().filter(|s| s.format != Format::DocMark) {
            assert_eq!(
                support.read,
                docsai_office::READABLE.contains(&support.format),
                "read support for {} is out of step with the office crate",
                support.format
            );
        }
        assert!(can_read(Format::Docx));
        assert!(can_write(Format::Docx));
        assert!(can_read(Format::DocMark));
        assert!(can_write(Format::DocMark));
        assert_eq!(
            can_write(Format::Docx),
            docsai_office::WRITABLE.contains(&Format::Docx)
        );
    }

    #[test]
    fn every_format_appears_exactly_once() {
        let mut seen: Vec<Format> = SUPPORT.iter().map(|s| s.format).collect();
        let total = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), total);
        assert_eq!(total, 7);
    }
}
