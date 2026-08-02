//! Office readers and writers for docsai.
//!
//! Phase 1 lands the `.docx` reader; Phase 2 the docx writer; Phase 3 adds
//! `.xlsx` read/write and `.xls` read; Phase 5 adds degraded `.doc` reading.
//! The crate depends on `docsai-model` and on nothing else in the workspace
//! (`AGENTS.md` §3).
//!
//! ```no_run
//! use docsai_model::MemoryAssetStore;
//! let file = std::fs::File::open("informe.docx")?;
//! let mut assets = MemoryAssetStore::new();
//! let (document, report) = docsai_office::read_docx(file, &mut assets)?;
//! println!("{} warnings", report.warnings.len());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

pub mod detect;
mod doc;
mod docx;
mod error;
mod package;
mod write_error;
mod xls;
mod xlsx;
mod xml;

pub use detect::{detect, DetectScore};
pub use error::ReadError;
pub use write_error::WriteError;

use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format};
use std::io::{Read, Seek};

/// Formats this crate can read today.
pub const READABLE: &[Format] = &[Format::Docx, Format::Doc, Format::Xlsx, Format::Xls];

/// Formats this crate can write today.
pub const WRITABLE: &[Format] = &[Format::Docx, Format::Xlsx];

/// Reads a `.docx` document.
pub fn read_docx<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    docx::read(reader, assets)
}

/// Reads a legacy `.doc` document (degraded native path).
///
/// For full fidelity, prefer the LibreOffice fallback in `docsai-convert`
/// (`--use-loffice`).
pub fn read_doc<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    doc::read(reader, assets)
}

/// Reads a `.xlsx` workbook.
pub fn read_xlsx<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    xlsx::read(reader, assets)
}

/// Reads a legacy `.xls` workbook (values and formulas only).
pub fn read_xls<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    xls::read(reader, assets)
}

/// Reads any Office document this crate supports.
pub fn read<R: Read + Seek>(
    format: Format,
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    match format {
        Format::Docx => read_docx(reader, assets),
        Format::Doc => read_doc(reader, assets),
        Format::Xlsx => read_xlsx(reader, assets),
        Format::Xls => read_xls(reader, assets),
        other => Err(ReadError::WrongShape {
            part: other.to_string(),
            expected: "a format supported in this phase (docx, doc, xlsx, xls)".into(),
        }),
    }
}

/// Hidden test helpers (synthetic `.doc` fixtures).
#[doc(hidden)]
pub mod test_support {
    pub use crate::doc::classify_ole2;
    pub use crate::doc::test_fixture;
}

/// Writes a `.docx` document.
pub fn write_docx<W: std::io::Write + std::io::Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    docx::write::write_docx(document, assets, writer)
}

/// Writes a `.xlsx` workbook.
pub fn write_xlsx<W: std::io::Write + std::io::Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    xlsx::write::write_xlsx(document, assets, writer)
}

/// Writes any Office document this crate supports.
pub fn write<W: std::io::Write + std::io::Seek>(
    format: Format,
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    match format {
        Format::Docx => write_docx(document, assets, writer),
        Format::Xlsx => write_xlsx(document, assets, writer),
        other => Err(WriteError::Unsupported(other.to_string())),
    }
}
