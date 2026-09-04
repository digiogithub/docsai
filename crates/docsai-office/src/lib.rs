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
mod drawingml;
mod error;
mod package;
mod ppt;
mod pptx;
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
pub const READABLE: &[Format] = &[
    Format::Docx,
    Format::Doc,
    Format::Xlsx,
    Format::Xls,
    Format::Pptx,
    // Phase 19: read two ways, like `.doc` — the degraded native reader in
    // `ppt/` (19-D) and the LibreOffice pre-conversion in `docsai-convert`
    // (19-B), chosen by `--use-loffice`. Never written.
    Format::Ppt,
];

/// Formats this crate can write today.
pub const WRITABLE: &[Format] = &[Format::Docx, Format::Xlsx, Format::Pptx];

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

/// Reads a `.pptx` presentation.
///
/// In [`READABLE`] since Phase 13-K, which is the increment that gave a deck a
/// destination: `inspect` reports the slide inventory. Reading is *not* the same
/// as converting — DocMark-P is Phase 14, and until it exists a deck read into
/// the IR still refuses to serialise rather than writing an empty document
/// (`docsai-convert`).
pub fn read_pptx<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    pptx::read(reader, assets)
}

/// Reads a legacy `.ppt` presentation natively.
///
/// Read-only by design (Phase 19 writes no `.ppt`, ever). Degraded: it recovers
/// slides, shape text with its outline levels and the `Pictures` stream's
/// BLIPs, and says so in the report. For full fidelity prefer the LibreOffice
/// fallback in `docsai-convert` (`--use-loffice`).
pub fn read_ppt<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    ppt::read(reader, assets)
}

/// Reads a legacy `.xls` workbook (values and formulas only).
pub fn read_xls<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    xls::read(reader, assets)
}

/// Refuses an encrypted or password-protected document **before** anything
/// decides how to read it (Phase 19-E).
///
/// Legacy binary formats are reached two ways — this crate's own degraded
/// reader, or a LibreOffice pre-conversion in `docsai-convert` — and only the
/// first of those is under this project's control. LibreOffice does not consult
/// `CurrentUserAtom.headerToken`, so it reads an encrypted `.ppt` and hands back
/// a deck; a refusal placed inside a reader would therefore be silently skipped
/// on the other route. This function is the shared pre-flight both routes pass
/// through, and it is the only place the refusal is worded.
///
/// It is cheap and conservative: it reads one field of one small OLE2 stream,
/// and answers `Ok(())` both for a format with no such flag and for a file it
/// cannot make sense of — refusing a *broken* file is the reader's job, which
/// can say where and why.
///
/// Covers `.ppt` today. `.doc` is **not** covered: its `fEncrypted` flag is
/// checked by the native reader (`FibBase`), so `--use-loffice never` refuses
/// an encrypted `.doc` and the LibreOffice route does not reach that check —
/// see `kb/106-phase-19-close.md`.
pub fn refuse_if_encrypted<R: Read + Seek>(format: Format, reader: R) -> Result<(), ReadError> {
    match format {
        Format::Ppt => ppt::refuse_if_encrypted(reader),
        _ => Ok(()),
    }
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
        Format::Pptx => read_pptx(reader, assets),
        Format::Ppt => read_ppt(reader, assets),
        other => Err(ReadError::WrongShape {
            part: other.to_string(),
            expected: "a format supported in this phase (docx, doc, xlsx, xls, pptx, ppt)".into(),
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

/// Writes a `.pptx` presentation.
///
/// Phase 15-A re-emits the preserved package unchanged; the slides start coming
/// from the model in 15-B, which is why `docsai-convert`'s `SUPPORT` still
/// answers `write: false` for `pptx`.
pub fn write_pptx<W: std::io::Write + std::io::Seek>(
    document: &Document,
    assets: &dyn AssetStore,
    writer: W,
) -> Result<ConversionReport, WriteError> {
    pptx::write::write_pptx(document, assets, writer)
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
        Format::Pptx => write_pptx(document, assets, writer),
        other => Err(WriteError::Unsupported(other.to_string())),
    }
}
