//! Office readers and writers for docsai.
//!
//! Phase 1 lands the `.docx` reader; `.xlsx`/`.xls` arrive in Phase 3 and `.doc`
//! in Phase 5. The crate depends on `docsai-model` and on nothing else in the
//! workspace (`AGENTS.md` §3).
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
mod docx;
mod error;
mod package;
mod xml;

pub use detect::{detect, DetectScore};
pub use error::ReadError;

use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document, Format};
use std::io::{Read, Seek};

/// Formats this crate can read today.
pub const READABLE: &[Format] = &[Format::Docx];

/// Formats this crate can write today.
pub const WRITABLE: &[Format] = &[];

/// Reads a `.docx` document.
pub fn read_docx<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    docx::read(reader, assets)
}

/// Reads any Office document this crate supports.
pub fn read<R: Read + Seek>(
    format: Format,
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    match format {
        Format::Docx => read_docx(reader, assets),
        other => Err(ReadError::WrongShape {
            part: other.to_string(),
            expected: "a format supported in this phase (docx)".into(),
        }),
    }
}
