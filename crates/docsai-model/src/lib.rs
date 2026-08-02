//! The docsai intermediate document model (IR).
//!
//! Every converter reads *into* this model and writes *from* it; no converter
//! ever talks to another converter (architecture §1). The crate is
//! deliberately free of I/O and of heavy dependencies: it knows nothing about
//! ZIP, XML or the filesystem, and media live behind the
//! [`AssetStore`] trait.
//!
//! Design rules the types encode:
//!
//! * **Style = reference + delta**, never flattened formatting ([`style`]).
//! * **Lengths are normalised to EMU** with a newtype ([`units::Length`]).
//! * **One image model for every format** ([`image`]), whose invariants are
//!   checked by [`validate`].
//! * **Everything is `serde`-serialisable**, which gives `inspect --json` and
//!   round-trip diffing for free.
//!
//! ```
//! use docsai_model::{Document, text::{TextDocument, Section, Block, Paragraph}};
//!
//! let doc = Document::Text(TextDocument {
//!     sections: vec![Section {
//!         blocks: vec![Block::Paragraph(Paragraph::text("Hola"))],
//!         ..Default::default()
//!     }],
//!     ..Default::default()
//! });
//! assert!(docsai_model::validate::validate(&doc).is_ok());
//! ```

pub mod assets;
pub mod image;
pub mod list;
pub mod report;
pub mod sheet;
pub mod style;
pub mod text;
pub mod units;
pub mod validate;

use serde::{Deserialize, Serialize};

pub use assets::{AssetId, AssetInfo, AssetStore, MemoryAssetStore};
pub use report::{ConversionReport, ConversionStats, Severity, Warning};
pub use sheet::Workbook;
pub use style::{StyleCatalog, StyleId};
pub use text::{DocumentMeta, TextDocument};
pub use units::Length;

/// Version of the DocMark specification this model targets.
pub const DOCMARK_VERSION: &str = "1.0";

/// The two shapes a document can take.
///
/// Externally tagged on purpose: an internally tagged enum makes serde buffer
/// the payload through `Content`, which rewrites every map key as a string and
/// would break the integer-keyed maps inside [`Workbook`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Document {
    Text(TextDocument),
    Workbook(Workbook),
}

impl Document {
    /// Document properties, whichever shape it is.
    pub fn meta(&self) -> &DocumentMeta {
        match self {
            Document::Text(d) => &d.meta,
            Document::Workbook(w) => &w.meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut DocumentMeta {
        match self {
            Document::Text(d) => &mut d.meta,
            Document::Workbook(w) => &mut w.meta,
        }
    }

    /// The style catalogue, whichever shape it is.
    pub fn styles(&self) -> &StyleCatalog {
        match self {
            Document::Text(d) => &d.styles,
            Document::Workbook(w) => &w.styles,
        }
    }

    pub fn is_text(&self) -> bool {
        matches!(self, Document::Text(_))
    }

    pub fn is_workbook(&self) -> bool {
        matches!(self, Document::Workbook(_))
    }
}

/// The source formats docsai knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Docx,
    Doc,
    Xlsx,
    Xls,
    Odt,
    Ods,
    /// The DocMark pivot itself.
    DocMark,
}

impl Format {
    /// The name used in the DocMark front matter and on the CLI.
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Docx => "docx",
            Format::Doc => "doc",
            Format::Xlsx => "xlsx",
            Format::Xls => "xls",
            Format::Odt => "odt",
            Format::Ods => "ods",
            Format::DocMark => "docmark",
        }
    }

    /// Parses a format name; also accepts `md`/`markdown` for DocMark.
    pub fn parse(name: &str) -> Option<Format> {
        match name
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "docx" | "docm" => Some(Format::Docx),
            "doc" => Some(Format::Doc),
            "xlsx" | "xlsm" => Some(Format::Xlsx),
            "xls" => Some(Format::Xls),
            "odt" => Some(Format::Odt),
            "ods" => Some(Format::Ods),
            "docmark" | "dmk" | "md" | "markdown" => Some(Format::DocMark),
            _ => None,
        }
    }

    /// True when documents of this format become a [`Document::Workbook`].
    pub fn is_spreadsheet(self) -> bool {
        matches!(self, Format::Xlsx | Format::Xls | Format::Ods)
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_names_round_trip() {
        for f in [
            Format::Docx,
            Format::Doc,
            Format::Xlsx,
            Format::Xls,
            Format::Odt,
            Format::Ods,
            Format::DocMark,
        ] {
            assert_eq!(Format::parse(f.as_str()), Some(f));
        }
    }

    #[test]
    fn format_parsing_is_forgiving_about_shape() {
        assert_eq!(Format::parse(".DOCX"), Some(Format::Docx));
        assert_eq!(Format::parse("md"), Some(Format::DocMark));
        assert_eq!(
            Format::parse("docm"),
            Some(Format::Docx),
            "macro-enabled files are read as their macro-free equivalent"
        );
        assert_eq!(Format::parse("pdf"), None);
    }

    #[test]
    fn document_exposes_meta_for_both_shapes() {
        let mut text = Document::Text(TextDocument::default());
        text.meta_mut().title = Some("T".into());
        assert_eq!(text.meta().title.as_deref(), Some("T"));
        assert!(text.is_text());

        let book = Document::Workbook(Workbook::default());
        assert!(book.is_workbook());
        assert_eq!(book.meta().title, None);
    }
}
