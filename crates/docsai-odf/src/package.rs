//! ODF package handling: the ZIP container (`mimetype`, `content.xml`, …).

use std::collections::BTreeMap;
use std::io::{Read, Seek, Write};

use crate::error::ReadError;
use crate::write_error::WriteError;
use crate::xml::Element;

/// Cap on the total uncompressed size of a package.
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// Cap on a single part.
const MAX_PART_BYTES: u64 = 128 * 1024 * 1024;

/// An ODF package held in memory, keyed by part name.
#[derive(Debug, Default)]
pub struct Package {
    parts: BTreeMap<String, Vec<u8>>,
}

impl Package {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.parts.insert(name.into(), bytes.into());
    }

    /// Writes the package as a ZIP archive.
    ///
    /// ODF requires `mimetype` to be the first entry and stored uncompressed
    /// (no extra field) so tools can sniff the package without decompressing.
    pub fn write_to<W: Write + Seek>(&self, writer: W) -> Result<(), WriteError> {
        let mut zip = zip::ZipWriter::new(writer);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(
                zip::DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0)
                    .unwrap_or_else(|_| zip::DateTime::default_for_write()),
            );
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(
                zip::DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0)
                    .unwrap_or_else(|_| zip::DateTime::default_for_write()),
            );

        if let Some(bytes) = self.parts.get("mimetype") {
            zip.start_file("mimetype", stored)?;
            zip.write_all(bytes)?;
        }

        for (name, bytes) in &self.parts {
            if name == "mimetype" {
                continue;
            }
            zip.start_file(name.as_str(), deflated)?;
            zip.write_all(bytes)?;
        }
        zip.finish()?;
        Ok(())
    }

    /// Reads every part of the ZIP container.
    pub fn open<R: Read + Seek>(reader: R) -> Result<Package, ReadError> {
        let mut zip = zip::ZipArchive::new(reader)?;
        let mut parts = BTreeMap::new();
        let mut total: u64 = 0;

        for i in 0..zip.len() {
            let mut entry = match zip.by_index(i) {
                Ok(entry) => entry,
                Err(zip::result::ZipError::Io(e)) => return Err(ReadError::Io(e)),
                Err(_) => continue,
            };
            if entry.is_dir() {
                continue;
            }
            if entry.encrypted() {
                return Err(ReadError::Encrypted);
            }
            let size = entry.size();
            if size > MAX_PART_BYTES {
                return Err(ReadError::TooLarge(format!(
                    "part `{}` declares {size} bytes",
                    entry.name()
                )));
            }
            total = total.saturating_add(size);
            if total > MAX_TOTAL_BYTES {
                return Err(ReadError::TooLarge(format!(
                    "package expands to more than {MAX_TOTAL_BYTES} bytes"
                )));
            }
            let Some(name) = normalise_part_name(entry.name()) else {
                continue;
            };
            let mut bytes = Vec::with_capacity(size.min(1 << 20) as usize);
            entry.read_to_end(&mut bytes)?;
            parts.insert(name, bytes);
        }

        if parts.is_empty() {
            return Err(ReadError::NotAZip("archive has no readable parts".into()));
        }
        Ok(Package { parts })
    }

    pub fn part(&self, name: &str) -> Option<&[u8]> {
        self.parts.get(name).map(|v| v.as_slice())
    }

    pub fn has_part(&self, name: &str) -> bool {
        self.parts.contains_key(name)
    }

    #[allow(dead_code)]
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts.keys().map(|s| s.as_str())
    }

    pub fn text(&self, name: &str) -> Result<&str, ReadError> {
        let bytes = self
            .part(name)
            .ok_or_else(|| ReadError::MissingPart(name.to_string()))?;
        std::str::from_utf8(bytes).map_err(|_| ReadError::Encoding {
            part: name.to_string(),
        })
    }

    pub fn optional_xml(&self, name: &str) -> Result<Option<Element>, ReadError> {
        match self.part(name) {
            Some(bytes) => Element::parse(name, bytes).map(Some),
            None => Ok(None),
        }
    }

    /// Resolves a package-relative `xlink:href` against a base part directory.
    pub fn resolve_href(&self, base_part: &str, href: &str) -> Option<String> {
        if href.starts_with("http://") || href.starts_with("https://") || href.starts_with("file:")
        {
            return None;
        }
        let base = parent_dir(base_part);
        resolve_target(&base, href)
    }
}

fn normalise_part_name(raw: &str) -> Option<String> {
    let raw = raw.replace('\\', "/");
    if raw.starts_with('/') || raw.contains(':') {
        return None;
    }
    let mut out: Vec<&str> = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return None,
            s => out.push(s),
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("/"))
    }
}

fn parent_dir(part: &str) -> String {
    match part.rfind('/') {
        Some(i) => part[..i].to_string(),
        None => String::new(),
    }
}

fn resolve_target(base: &str, target: &str) -> Option<String> {
    let target = target.replace('\\', "/");
    let joined = if let Some(stripped) = target.strip_prefix('/') {
        stripped.to_string()
    } else if base.is_empty() {
        target
    } else {
        format!("{base}/{target}")
    };

    let mut out: Vec<&str> = Vec::new();
    for segment in joined.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                out.pop()?;
            }
            s => out.push(s),
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn mimetype_is_written_first_and_stored() {
        let mut package = Package::new();
        package.insert(
            "mimetype",
            b"application/vnd.oasis.opendocument.text".as_slice(),
        );
        package.insert("content.xml", b"<office:document-content/>".as_slice());
        let mut buf = Cursor::new(Vec::new());
        package.write_to(&mut buf).unwrap();
        let bytes = buf.into_inner();
        // Local file header signature + version + flags + method (0 = stored)
        // then the name length; mimetype must appear early in the archive.
        assert!(bytes.windows(8).any(|w| w == b"mimetype"));
        let reopened = Package::open(Cursor::new(bytes)).unwrap();
        assert_eq!(
            reopened.part("mimetype"),
            Some(b"application/vnd.oasis.opendocument.text".as_slice())
        );
    }

    #[test]
    fn member_names_are_sanitised() {
        assert_eq!(
            normalise_part_name("Pictures//./logo.png").as_deref(),
            Some("Pictures/logo.png")
        );
        assert_eq!(normalise_part_name("../secret"), None);
    }
}
