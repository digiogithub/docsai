//! Conversion orchestration: format detection, pipelines and asset management.
//!
//! This is the only crate that knows about more than one format; the readers
//! and writers never talk to each other (architecture §1).

#![forbid(unsafe_code)]

pub mod assets;
pub mod batch;
pub mod inspect;
pub mod loffice;
pub mod outline;
mod pipeline;
pub mod service;
pub mod style_map;
pub mod tokens;

pub use assets::DirAssetStore;
pub use batch::{convert_batch, BatchOutcome, BatchReport};
pub use docsai_docmark::{Fidelity, Options as DocMarkOptions, RawPolicy};
pub use inspect::{inspect_path, InspectReport};
pub use loffice::UseLoffice;
pub use outline::{outline, outline_path, Outline, OutlineNode};
pub use pipeline::{
    convert_bytes, convert_file, is_stdin_path, is_stdout_path, read_document,
    read_document_with_options, read_path, read_path_with_options, roundtrip_file, ConvertOptions,
    Outcome, RoundtripOutcome,
};
pub use service::{
    convert_from_markdown, convert_to_markdown, inspect_bytes, inspect_input, mime_type_for,
    parse_fidelity, validate_output_path, AssetBytes, AssetMode, FromMarkdownResult, SourceInput,
    ToMarkdownResult,
};
pub use style_map::{apply_style_map, StyleMap, StyleTarget};
pub use tokens::{token_report, token_report_path, NodeTokens, TokenReport};

use docsai_model::Format;

/// Errors the orchestration layer can return.
#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("{0}")]
    Read(#[from] docsai_office::ReadError),

    #[error("{0}")]
    Write(#[from] docsai_office::WriteError),

    #[error("{0}")]
    OdfRead(#[from] docsai_odf::ReadError),

    #[error("{0}")]
    OdfWrite(#[from] docsai_odf::WriteError),

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

    /// LibreOffice was required or failed while converting a legacy format.
    #[error("LibreOffice fallback: {message}")]
    Loffice { message: String },
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
        read: true,
        write: false,
        note: "Phase 5: native degraded text; full fidelity via --use-loffice when LibreOffice is installed",
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
        read: true,
        write: true,
        note: "Phase 4",
    },
    FormatSupport {
        format: Format::Ods,
        read: true,
        write: true,
        note: "Phase 4",
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
        // ahead of the readers/writers.
        for support in SUPPORT.iter().filter(|s| s.format != Format::DocMark) {
            let readable = docsai_office::READABLE.contains(&support.format)
                || docsai_odf::READABLE.contains(&support.format);
            let writable = docsai_office::WRITABLE.contains(&support.format)
                || docsai_odf::WRITABLE.contains(&support.format);
            assert_eq!(
                support.read, readable,
                "read support for {} is out of step with the format crates",
                support.format
            );
            assert_eq!(
                support.write, writable,
                "write support for {} is out of step with the format crates",
                support.format
            );
        }
        assert!(can_read(Format::Docx));
        assert!(can_write(Format::Docx));
        assert!(can_read(Format::Odt));
        assert!(can_write(Format::Odt));
        assert!(can_read(Format::Ods));
        assert!(can_write(Format::Ods));
        assert!(can_read(Format::DocMark));
        assert!(can_write(Format::DocMark));
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
