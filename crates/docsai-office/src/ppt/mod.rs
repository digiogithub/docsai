//! Legacy binary PowerPoint (`.ppt`) — read-only (Phase 19).
//!
//! A sibling of [`crate::doc`] and [`crate::xls`], because that is where the
//! OLE2 binary formats live. The module exists as a **seam**: increment 19-B
//! gives `.ppt` the LibreOffice route (`soffice --convert-to pptx`, then the
//! Phase 13 reader unchanged), and increment 19-D fills this module with the
//! native degraded reader that [[99-phase-19-parser-choice]] specifies —
//! `Window`, `records`, `persist`, `text`, `images`.
//!
//! Until then `read` is honest about what it does not have. It does not return
//! an empty presentation, and it does not pretend the file was unreadable: the
//! file is fine, this build simply has no native route for it yet, and the
//! route it does have is the one `--use-loffice never` just declined.
//!
//! Writing `.ppt` is out of scope for the whole of Phase 19, and there is no
//! `write` here to grow into one.

use std::io::{Read, Seek};

use docsai_model::assets::AssetStore;
use docsai_model::{ConversionReport, Document};

use crate::ReadError;

/// Reads a legacy `.ppt` natively.
///
/// Not implemented in this build. See the module documentation: today the only
/// route into a `.ppt` is the LibreOffice pre-conversion in `docsai-convert`,
/// and reaching this function means the caller declined it.
pub(crate) fn read<R: Read + Seek>(
    _reader: R,
    _assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    Err(ReadError::NoNativeRoute {
        format: "ppt".into(),
        missing: "the native degraded .ppt reader is Phase 19-D and is not in this build. \
                  Today a legacy .ppt is read by pre-converting it to .pptx with LibreOffice, \
                  which `--use-loffice never` forbids. Re-run with `--use-loffice auto` or \
                  `--use-loffice require`, and install LibreOffice (or point DOCSAI_LIBREOFFICE \
                  at its `soffice` binary) if it is not on this machine"
            .into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;
    use std::io::Cursor;

    #[test]
    fn the_seam_says_what_is_missing_rather_than_failing_vaguely() {
        let mut assets = MemoryAssetStore::new();
        let err = read(Cursor::new(Vec::new()), &mut assets).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("19-D"), "{message}");
        assert!(message.contains("--use-loffice"), "{message}");
        assert!(message.contains("LibreOffice"), "{message}");
    }
}
