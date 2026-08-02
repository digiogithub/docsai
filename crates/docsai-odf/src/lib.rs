//! OpenDocument readers and writers (`.odt`, `.ods`).
//!
//! Phase 4 of `docs/development-plan.md`. This crate depends on `docsai-model`
//! and on nothing else in the workspace (`AGENTS.md` §3).
//!
//! ```no_run
//! use docsai_model::MemoryAssetStore;
//! let file = std::fs::File::open("informe.odt")?;
//! let mut assets = MemoryAssetStore::new();
//! let (document, report) = docsai_odf::read_odt(file, &mut assets)?;
//! println!("{} warnings", report.warnings.len());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod draw;
mod error;
mod format;
mod length;
mod ods;
mod odt;
mod package;
mod styles;
mod write_error;
mod xml;

pub use error::ReadError;
pub use write_error::WriteError;

use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format};
use std::io::{Read, Seek, Write};

/// Formats this crate can read.
pub const READABLE: &[Format] = &[Format::Odt, Format::Ods];

/// Formats this crate can write.
pub const WRITABLE: &[Format] = &[Format::Odt, Format::Ods];

/// Formats this crate handles.
pub const FORMATS: &[Format] = &[Format::Odt, Format::Ods];

/// Reads an `.odt` document.
pub fn read_odt<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    odt::read(reader, assets)
}

/// Reads an `.ods` workbook.
pub fn read_ods<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    ods::read(reader, assets)
}

/// Reads any ODF document this crate supports.
pub fn read<R: Read + Seek>(
    format: Format,
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    match format {
        Format::Odt => read_odt(reader, assets),
        Format::Ods => read_ods(reader, assets),
        other => Err(ReadError::WrongShape {
            part: other.to_string(),
            expected: "a format supported by docsai-odf (odt, ods)".into(),
        }),
    }
}

/// Writes an `.odt` document.
pub fn write_odt<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    odt::write::write_odt(document, assets, writer)
}

/// Writes an `.ods` workbook.
pub fn write_ods<W: Write + Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    ods::write::write_ods(document, assets, writer)
}

/// Writes any ODF document this crate supports.
pub fn write<W: Write + Seek>(
    format: Format,
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    match format {
        Format::Odt => write_odt(document, assets, writer),
        Format::Ods => write_ods(document, assets, writer),
        other => Err(WriteError::Unsupported(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_are_the_odf_pair() {
        assert_eq!(super::FORMATS.len(), 2);
        assert_eq!(super::READABLE, super::WRITABLE);
    }
}
