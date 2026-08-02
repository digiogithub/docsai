//! Format detection **by content**, not by extension (architecture §4).
//!
//! A file called `informe.txt` that is really a `.docx` must still convert,
//! and a `.docx` extension on a ZIP of holiday photos must not.

use docsai_model::Format;
use std::io::{Read, Seek, SeekFrom};

/// How confident detection is about a format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DetectScore {
    /// The content does not look like this format at all.
    No,
    /// The container matches but the payload was not inspected.
    Maybe,
    /// The defining part of the format was found.
    Certain,
}

/// The OLE2/CFB compound file signature, shared by `.doc` and `.xls`.
const OLE2_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// Local file header of a ZIP archive.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// Detects the format of a document.
///
/// `hint` is the file name, used only to break ties the content cannot: `.doc`
/// and `.xls` share a container, and so do `.odt` and `.ods` when the
/// `mimetype` entry is missing.
pub fn detect<R: Read + Seek>(mut reader: R, hint: Option<&str>) -> (Format, DetectScore) {
    let mut magic = [0u8; 8];
    let read = read_head(&mut reader, &mut magic);
    let _ = reader.seek(SeekFrom::Start(0));

    if read >= 8 && magic == OLE2_MAGIC {
        // Both legacy formats are the same container; only the name tells them
        // apart without parsing the directory (Phase 5 does that properly).
        let format = match extension(hint).as_deref() {
            Some("xls") | Some("xlt") => Format::Xls,
            _ => Format::Doc,
        };
        return (format, DetectScore::Maybe);
    }

    if read >= 4 && magic[..4] == *ZIP_MAGIC {
        if let Ok(zip) = zip::ZipArchive::new(&mut reader) {
            let names: Vec<String> = zip.file_names().map(str::to_string).collect();
            let has = |name: &str| names.iter().any(|n| n == name);
            let _ = reader.seek(SeekFrom::Start(0));

            if has(crate::docx::DOCUMENT_PART) {
                return (Format::Docx, DetectScore::Certain);
            }
            if has("xl/workbook.xml") {
                return (Format::Xlsx, DetectScore::Certain);
            }
            if has("content.xml") && has("mimetype") {
                let format = match extension(hint).as_deref() {
                    Some("ods") => Format::Ods,
                    _ => Format::Odt,
                };
                return (format, DetectScore::Maybe);
            }
        }
        let _ = reader.seek(SeekFrom::Start(0));
        return (Format::Docx, DetectScore::No);
    }

    // DocMark is text; the front matter is what identifies it.
    if looks_like_docmark(&magic[..read]) {
        return (Format::DocMark, DetectScore::Maybe);
    }
    match extension(hint).and_then(|e| Format::parse(&e)) {
        Some(format) => (format, DetectScore::Maybe),
        None => (Format::DocMark, DetectScore::No),
    }
}

fn read_head<R: Read>(reader: &mut R, buffer: &mut [u8; 8]) -> usize {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => break,
        }
    }
    filled
}

fn looks_like_docmark(head: &[u8]) -> bool {
    if !head.starts_with(b"---") {
        return false;
    }
    // Prefer a stronger signal when the probe buffer already holds it.
    if head.windows(8).any(|w| w == b"docmark:") {
        return true;
    }
    // `---` alone is still a maybe for DocMark (Phase 2).
    true
}

fn extension(hint: Option<&str>) -> Option<String> {
    let name = hint?;
    let ext = name.rsplit('.').next()?;
    if ext == name {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn corpus(name: &str) -> Cursor<Vec<u8>> {
        let path = format!("{}/../../corpus/{name}", env!("CARGO_MANIFEST_DIR"));
        Cursor::new(std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}")))
    }

    #[test]
    fn recognises_docx_and_xlsx_by_their_defining_part() {
        assert_eq!(
            detect(corpus("docx/basic-text.docx"), Some("basic-text.docx")),
            (Format::Docx, DetectScore::Certain)
        );
        assert_eq!(
            detect(corpus("xlsx/values-types.xlsx"), Some("values-types.xlsx")),
            (Format::Xlsx, DetectScore::Certain)
        );
    }

    #[test]
    fn the_extension_does_not_override_the_content() {
        let (format, score) = detect(corpus("docx/basic-text.docx"), Some("notas.txt"));
        assert_eq!((format, score), (Format::Docx, DetectScore::Certain));
    }

    #[test]
    fn a_plain_zip_is_not_an_office_document() {
        let mut buffer = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
            zip.start_file::<_, ()>("photo.jpg", Default::default())
                .unwrap();
            zip.finish().unwrap();
        }
        assert_eq!(
            detect(Cursor::new(buffer), Some("x.docx")).1,
            DetectScore::No
        );
    }

    #[test]
    fn legacy_containers_are_told_apart_by_name() {
        let mut ole = OLE2_MAGIC.to_vec();
        ole.extend_from_slice(&[0u8; 32]);
        assert_eq!(
            detect(Cursor::new(ole.clone()), Some("a.doc")).0,
            Format::Doc
        );
        assert_eq!(detect(Cursor::new(ole), Some("a.xls")).0, Format::Xls);
    }

    #[test]
    fn docmark_is_recognised_by_its_front_matter() {
        let text = b"---\ndocmark: \"1.0\"\n---\n# Hola\n".to_vec();
        assert_eq!(detect(Cursor::new(text), None).0, Format::DocMark);
    }

    #[test]
    fn an_empty_file_is_not_a_panic() {
        assert_eq!(detect(Cursor::new(Vec::new()), None).1, DetectScore::No);
    }
}
