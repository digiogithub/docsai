//! Legacy Microsoft Word `.doc` (MS-DOC / Word 97–2003) reading (Phase 5).
//!
//! Two-level strategy (technical-analysis §1.3):
//!
//! 1. **LibreOffice headless** (optional, orchestrated by `docsai-convert`) converts
//!    to `.docx` and re-enters the Phase 1 pipeline for full fidelity.
//! 2. **Native degraded extractor** (this module): `cfb` + FIB + piece table →
//!    paragraphs and basic properties; embedded BLIPs extracted without fine
//!    geometry (`ImageGeometryDegraded`).
//!
//! Write support is out of scope.

mod fib;
mod images;
mod piece_table;
/// Synthetic `.doc` builders for tests and the corpus generator.
#[doc(hidden)]
pub mod test_fixture;

#[cfg(test)]
use std::io::Cursor;
use std::io::{Read, Seek, SeekFrom};

use docsai_model::assets::AssetStore;
use docsai_model::image::{ImageGeometry, ImageRef};
use docsai_model::report::{ConversionReport, Warning};
use docsai_model::text::{Block, DocumentMeta, Inline, Paragraph, Section, TextDocument};
use docsai_model::units::{Length, Size};
use docsai_model::Document;

use crate::error::ReadError;

use fib::Fib;
use piece_table::extract_main_text;

/// OLE stream that holds the Word binary document.
pub(crate) const WORD_DOCUMENT: &str = "WordDocument";
/// Table stream selected when FIB flag `fWhichTblStm` is clear.
pub(crate) const TABLE_0: &str = "0Table";
/// Table stream selected when FIB flag `fWhichTblStm` is set.
pub(crate) const TABLE_1: &str = "1Table";
/// Optional data stream (Escher/OfficeArt BLIPs often live here).
pub(crate) const DATA_STREAM: &str = "Data";
/// Excel 97–2003 workbook stream (used by format detection).
pub(crate) const XLS_WORKBOOK: &str = "Workbook";
/// Very old Excel stream name.
pub(crate) const XLS_BOOK: &str = "Book";

/// Reads a legacy `.doc` into the IR using the degraded native path.
///
/// Always emits a [`Warning::Degraded`] so callers know the result is not a
/// full-fidelity conversion. Prefer the LibreOffice path when available.
pub fn read<R: Read + Seek>(
    reader: R,
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    let mut report = ConversionReport::new();
    report.warn(Warning::Degraded {
        what: "doc".into(),
        why: "native MS-DOC path is text + basic structure only; install LibreOffice \
              and use --use-loffice auto|require for full fidelity via docx"
            .into(),
    });

    let mut comp = open_cfb(reader)?;
    if !comp.is_stream(WORD_DOCUMENT) {
        return Err(ReadError::WrongShape {
            part: WORD_DOCUMENT.into(),
            expected: "a Word 97–2003 .doc (WordDocument stream)".into(),
        });
    }

    let word = read_stream(&mut comp, WORD_DOCUMENT)?;
    let fib = Fib::parse(&word)?;
    if fib.encrypted {
        return Err(ReadError::Encrypted);
    }

    let table_name = if fib.which_table_1 { TABLE_1 } else { TABLE_0 };
    if !comp.is_stream(table_name) {
        return Err(ReadError::MissingPart(table_name.into()));
    }
    let table = read_stream(&mut comp, table_name)?;

    let text = extract_main_text(&word, &table, &fib, &mut report)?;
    let paragraphs = split_paragraphs(&text);

    let mut blocks: Vec<Block> = paragraphs
        .into_iter()
        .map(|p| {
            report.stats.paragraphs = report.stats.paragraphs.saturating_add(1);
            Block::Paragraph(p)
        })
        .collect();

    // Embedded bitmaps: scan WordDocument + Data for OfficeArt BLIPs / raw images.
    let data = if comp.is_stream(DATA_STREAM) {
        read_stream(&mut comp, DATA_STREAM).ok()
    } else {
        None
    };
    let images = images::extract_embedded_images(&word, data.as_deref(), assets, &mut report);
    for image in images {
        blocks.push(Block::Image(image));
    }

    let meta = read_summary_meta(&mut comp);

    let doc = TextDocument {
        meta,
        styles: Default::default(),
        list_defs: Default::default(),
        sections: vec![Section {
            blocks,
            ..Default::default()
        }],
    };
    Ok((Document::Text(doc), report))
}

/// Opens a CFB container, mapping format errors to [`ReadError`].
pub(crate) fn open_cfb<R: Read + Seek>(reader: R) -> Result<cfb::CompoundFile<R>, ReadError> {
    cfb::OpenOptions::new()
        .open_with(reader)
        .map_err(|e| ReadError::WrongShape {
            part: "cfb".into(),
            expected: format!("OLE2/CFB compound file ({e})"),
        })
}

fn read_stream<R: Read + Seek>(
    comp: &mut cfb::CompoundFile<R>,
    name: &str,
) -> Result<Vec<u8>, ReadError> {
    let mut stream = comp
        .open_stream(name)
        .map_err(|e| ReadError::MissingPart(format!("{name}: {e}")))?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf)?;
    // Soft cap: a single stream larger than 256 MiB is treated as hostile.
    if buf.len() > 256 * 1024 * 1024 {
        return Err(ReadError::TooLarge(format!(
            "stream `{name}` is {} bytes",
            buf.len()
        )));
    }
    Ok(buf)
}

/// Splits plain document text on Word paragraph marks (`\\r`) into IR paragraphs.
///
/// Soft line breaks (`\\x0B`) become newlines inside a paragraph. Cell/row marks
/// (`\\x07`) and page breaks (`\\x0C`) become paragraph boundaries.
fn split_paragraphs(text: &str) -> Vec<Paragraph> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();

    let flush = |current: &mut String, paragraphs: &mut Vec<Paragraph>| {
        // Preserve empty paragraphs (a lone \\r) as empty blocks, matching docx.
        let content = std::mem::take(current);
        // Trailing paragraph mark at end of document often leaves one empty
        // paragraph; keep interior empties, drop a single trailing empty when
        // we already have content.
        paragraphs.push(Paragraph::new(if content.is_empty() {
            Vec::new()
        } else {
            vec![Inline::Text(content)]
        }));
    };

    for ch in text.chars() {
        match ch {
            '\r' | '\u{000c}' | '\u{0007}' => flush(&mut current, &mut paragraphs),
            '\u{000b}' => current.push('\n'),
            '\n' => {
                // Odd lone LF: treat as break inside the paragraph.
                current.push('\n');
            }
            // Strip other C0 controls except tab.
            c if c.is_control() && c != '\t' => {}
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        flush(&mut current, &mut paragraphs);
    }
    // Word documents always end with a final paragraph mark; the piece table
    // usually includes it. If that produced a trailing empty paragraph after
    // real content, keep it — DocMark round-trips empty paragraphs.
    if paragraphs.is_empty() {
        paragraphs.push(Paragraph::default());
    }
    paragraphs
}

/// Best-effort OLE SummaryInformation parse (title/author only).
fn read_summary_meta<R: Read + Seek>(comp: &mut cfb::CompoundFile<R>) -> DocumentMeta {
    let mut meta = DocumentMeta::default();
    // The stream name starts with the 0x05 property-set prefix.
    let name = "\u{0005}SummaryInformation";
    if !comp.is_stream(name) {
        return meta;
    }
    let Ok(mut stream) = comp.open_stream(name) else {
        return meta;
    };
    let mut bytes = Vec::new();
    if stream.read_to_end(&mut bytes).is_err() || bytes.len() < 48 {
        return meta;
    }
    // Very small subset of the OLE property set format: look for length-prefixed
    // CodePage strings for PID_TITLE (2) and PID_AUTHOR (4) by scanning. This is
    // intentionally best-effort; failure leaves meta empty.
    if let Some(title) = find_ole_string_prop(&bytes, 2) {
        meta.title = Some(title);
    }
    if let Some(author) = find_ole_string_prop(&bytes, 4) {
        meta.author = Some(author);
    }
    meta
}

fn find_ole_string_prop(bytes: &[u8], prop_id: u32) -> Option<String> {
    // Property set section starts after the header; scan for the property id
    // as a little-endian u32 followed by a type tag VT_LPSTR (0x001E) or
    // VT_LPWSTR (0x001F).
    let id_bytes = prop_id.to_le_bytes();
    let mut i = 0;
    while i + 12 < bytes.len() {
        if bytes[i..i + 4] == id_bytes {
            let ty = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?);
            if ty == 0x001E {
                // VT_LPSTR: size (u32) + bytes
                let size = u32::from_le_bytes(bytes[i + 8..i + 12].try_into().ok()?) as usize;
                let start = i + 12;
                let end = start.checked_add(size)?.min(bytes.len());
                if start >= end {
                    return None;
                }
                let raw = &bytes[start..end];
                // Trim trailing NULs.
                let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                let s = String::from_utf8_lossy(&raw[..end]).into_owned();
                if !s.is_empty() {
                    return Some(s);
                }
            } else if ty == 0x001F {
                let size = u32::from_le_bytes(bytes[i + 8..i + 12].try_into().ok()?) as usize;
                // size is count of UTF-16 code units including NUL.
                let byte_len = size.saturating_mul(2);
                let start = i + 12;
                let end = start.checked_add(byte_len)?.min(bytes.len());
                if start >= end {
                    return None;
                }
                let mut u16s = Vec::new();
                let mut j = start;
                while j + 1 < end {
                    let cu = u16::from_le_bytes([bytes[j], bytes[j + 1]]);
                    j += 2;
                    if cu == 0 {
                        break;
                    }
                    u16s.push(cu);
                }
                let s = String::from_utf16_lossy(&u16s);
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        i += 1;
    }
    None
}

/// Builds an inline [`ImageRef`] from stored asset bytes (native size when known).
pub(crate) fn image_ref_from_asset(
    assets: &dyn AssetStore,
    asset: docsai_model::assets::AssetId,
    report: &mut ConversionReport,
) -> ImageRef {
    let (w, h) = assets
        .info(&asset)
        .and_then(|i| i.native_size_px)
        .unwrap_or((64, 64));
    report.warn(Warning::ImageGeometryDegraded {
        what: "doc-escher-blip".into(),
        why: "native .doc path emits embedded images as inline with native pixel size only".into(),
    });
    let geometry = ImageGeometry::inline(Size::new(
        Length::from_px(w as f64),
        Length::from_px(h as f64),
    ));
    let mut image = ImageRef::new(asset, geometry);
    if let Some(info) = assets.info(&image.asset) {
        image.geometry.native_size_px = info.native_size_px;
    }
    image
}

/// Peeks an OLE2 container and returns whether it looks like `.doc` or `.xls`.
///
/// Used by [`crate::detect`]. The reader is rewound to the start on return.
pub fn classify_ole2<R: Read + Seek>(reader: &mut R) -> Result<docsai_model::Format, ReadError> {
    struct IoRef<'a, R>(&'a mut R);
    impl<R: Read> Read for IoRef<'_, R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }
    impl<R: Seek> Seek for IoRef<'_, R> {
        fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(pos)
        }
    }

    let format = {
        let comp = open_cfb(IoRef(reader))?;
        if comp.is_stream(WORD_DOCUMENT) {
            docsai_model::Format::Doc
        } else if comp.is_stream(XLS_WORKBOOK) || comp.is_stream(XLS_BOOK) {
            docsai_model::Format::Xls
        } else {
            // Unknown OLE2 payload; leave as Doc and let the reader fail clearly.
            docsai_model::Format::Doc
        }
    };
    let _ = reader.seek(SeekFrom::Start(0));
    Ok(format)
}

/// Reads an in-memory `.doc` (helper for tests and convert after LO fallback is skipped).
#[cfg(test)]
pub fn read_bytes(
    bytes: &[u8],
    assets: &mut dyn AssetStore,
) -> Result<(Document, ConversionReport), ReadError> {
    read(Cursor::new(bytes.to_vec()), assets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::MemoryAssetStore;

    #[test]
    fn minimal_fixture_yields_paragraphs() {
        let bytes = test_fixture::minimal_doc("Hello from Phase 5\rSecond paragraph\r");
        let mut assets = MemoryAssetStore::new();
        let (doc, report) = read_bytes(&bytes, &mut assets).expect("read ok");
        let Document::Text(text) = doc else {
            panic!("expected text document");
        };
        let plain: Vec<String> = text
            .sections
            .iter()
            .flat_map(|s| s.blocks.iter())
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.plain_text()),
                _ => None,
            })
            .filter(|s| !s.is_empty())
            .collect();
        assert!(
            plain.iter().any(|p| p.contains("Hello from Phase 5")),
            "got {plain:?}"
        );
        assert!(
            plain.iter().any(|p| p.contains("Second paragraph")),
            "got {plain:?}"
        );
        assert!(report
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::Degraded { what, .. } if what == "doc")));
    }

    #[test]
    fn encrypted_fib_is_rejected() {
        let bytes = test_fixture::encrypted_doc();
        let mut assets = MemoryAssetStore::new();
        let err = read_bytes(&bytes, &mut assets).expect_err("must reject");
        assert!(matches!(err, ReadError::Encrypted), "got {err:?}");
    }

    #[test]
    fn classify_ole2_sees_word_document() {
        let bytes = test_fixture::minimal_doc("x\r");
        let mut cur = Cursor::new(bytes);
        assert_eq!(classify_ole2(&mut cur).unwrap(), docsai_model::Format::Doc);
        assert_eq!(cur.position(), 0);
    }

    #[test]
    fn garbage_is_not_a_panic() {
        let mut assets = MemoryAssetStore::new();
        let _ = read_bytes(b"not a cfb file at all", &mut assets);
        let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        ole.extend_from_slice(&[0u8; 512]);
        let _ = read_bytes(&ole, &mut assets);
    }
}
