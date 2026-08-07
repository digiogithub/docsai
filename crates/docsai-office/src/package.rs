//! OPC package handling: the ZIP container and its relationship graph.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

use docsai_model::text::DocumentMeta;

use crate::error::ReadError;
use crate::xml::Element;

/// Cap on the total uncompressed size of a package.
///
/// A first line of defence against decompression bombs; Phase 8 hardens this
/// further with a dedicated adversarial suite.
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// Cap on a single part.
const MAX_PART_BYTES: u64 = 128 * 1024 * 1024;

/// An OPC package held in memory, keyed by part name.
#[derive(Debug, Default)]
pub struct Package {
    parts: BTreeMap<String, Vec<u8>>,
}

impl Package {
    /// Builds an empty package.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces a part.
    pub fn insert(&mut self, name: impl Into<String>, bytes: impl Into<Vec<u8>>) {
        self.parts.insert(name.into(), bytes.into());
    }

    /// Writes the package as a ZIP archive with deterministic part order.
    pub fn write_to<W: std::io::Write + std::io::Seek>(
        &self,
        writer: W,
    ) -> Result<(), crate::write_error::WriteError> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(writer);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(
                zip::DateTime::from_date_and_time(2020, 1, 1, 0, 0, 0)
                    .unwrap_or_else(|_| zip::DateTime::default_for_write()),
            );
        for (name, bytes) in &self.parts {
            zip.start_file(name.as_str(), options)?;
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
                // A single unreadable entry must not sink the document.
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
                // Absolute or traversing member name: never trust it.
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

    /// Part names, in deterministic order.
    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts.keys().map(|s| s.as_str())
    }

    /// A required part, as UTF-8 text.
    pub fn text(&self, name: &str) -> Result<&str, ReadError> {
        let bytes = self
            .part(name)
            .ok_or_else(|| ReadError::MissingPart(name.to_string()))?;
        std::str::from_utf8(bytes).map_err(|_| ReadError::Encoding {
            part: name.to_string(),
        })
    }

    /// Parses a part into an XML tree, or `None` if the part is absent.
    pub fn optional_xml(&self, name: &str) -> Result<Option<Element>, ReadError> {
        match self.part(name) {
            Some(bytes) => Element::parse(name, bytes).map(Some),
            None => Ok(None),
        }
    }

    /// The relationships declared for `part` (`<dir>/_rels/<file>.rels`).
    pub fn relationships(&self, part: &str) -> Relationships {
        let Some(rels_name) = rels_part_name(part) else {
            return Relationships::default();
        };
        let Some(bytes) = self.part(&rels_name) else {
            return Relationships::default();
        };
        let Ok(root) = Element::parse(&rels_name, bytes) else {
            return Relationships::default();
        };

        let base = parent_dir(part);
        let mut map = BTreeMap::new();
        for rel in root.children_named("Relationship") {
            let (Some(id), Some(kind), Some(target)) =
                (rel.attr("Id"), rel.attr("Type"), rel.attr("Target"))
            else {
                continue;
            };
            let external = rel.attr("TargetMode") == Some("External");
            let resolved = if external {
                target.to_string()
            } else {
                match resolve_target(&base, target) {
                    Some(path) => path,
                    None => continue,
                }
            };
            map.insert(
                id.to_string(),
                Relationship {
                    kind: kind.rsplit('/').next().unwrap_or(kind).to_string(),
                    target: resolved,
                    external,
                },
            );
        }
        Relationships { map }
    }
}

/// One OPC relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Last path segment of the relationship type (`image`, `hyperlink`…).
    pub kind: String,
    /// Package-absolute part name, or the URL for external targets.
    pub target: String,
    pub external: bool,
}

/// The relationships of one part.
#[derive(Debug, Default, Clone)]
pub struct Relationships {
    map: BTreeMap<String, Relationship>,
}

impl Relationships {
    pub fn get(&self, id: &str) -> Option<&Relationship> {
        self.map.get(id)
    }

    /// The first relationship of a given kind, if any.
    pub fn first_of_kind(&self, kind: &str) -> Option<&Relationship> {
        self.map.values().find(|r| r.kind == kind)
    }

    /// All relationships, in deterministic id order.
    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Relationship)> {
        self.map.iter().map(|(id, rel)| (id.as_str(), rel))
    }

    /// Every relationship of a given kind.
    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Relationship> + 'a {
        self.map.values().filter(move |r| r.kind == kind)
    }
}

/// `[Content_Types].xml`: what every part of the package *is*.
///
/// OPC part names are conventional, not normative. `ppt/slides/slide1.xml` is
/// where PowerPoint puts a slide, but the only thing that makes a part a slide
/// is its content type, and spike P3 made this the rule for the pptx reader:
/// parts are found through their type, never through their name.
#[derive(Debug, Default, Clone)]
pub struct ContentTypes {
    /// Extension (lowercase, no dot) to content type.
    defaults: BTreeMap<String, String>,
    /// Part name (package-absolute, no leading slash) to content type.
    overrides: BTreeMap<String, String>,
}

impl ContentTypes {
    /// Reads the package's content types.
    ///
    /// A package with no readable `[Content_Types].xml` is malformed, but it is
    /// not this function's job to say so: it returns an empty map and the
    /// caller decides whether the parts it needs are still identifiable.
    pub fn read(package: &Package) -> ContentTypes {
        let Ok(Some(root)) = package.optional_xml("[Content_Types].xml") else {
            return ContentTypes::default();
        };
        let mut types = ContentTypes::default();
        for child in root.children() {
            match child.name.as_str() {
                "Default" => {
                    if let (Some(ext), Some(kind)) =
                        (child.attr("Extension"), child.attr("ContentType"))
                    {
                        types
                            .defaults
                            .insert(ext.to_ascii_lowercase(), kind.trim().to_string());
                    }
                }
                "Override" => {
                    if let (Some(name), Some(kind)) =
                        (child.attr("PartName"), child.attr("ContentType"))
                    {
                        // `PartName` is package-absolute (`/ppt/slides/slide1.xml`);
                        // part names in the ZIP are not.
                        if let Some(name) = normalise_part_name(name.trim_start_matches('/')) {
                            types.overrides.insert(name, kind.trim().to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        types
    }

    /// The content type of a part: its override, or the default for its
    /// extension.
    pub fn of(&self, part: &str) -> Option<&str> {
        if let Some(kind) = self.overrides.get(part) {
            return Some(kind.as_str());
        }
        let ext = part.rsplit('.').next()?.to_ascii_lowercase();
        self.defaults.get(&ext).map(|s| s.as_str())
    }

    /// True when the part is declared as `kind`. An **undeclared** part is not
    /// rejected: a package whose content types are missing is degraded, and
    /// refusing to read it would be worse than reading it.
    pub fn is(&self, part: &str, kind: &str) -> bool {
        match self.of(part) {
            Some(declared) => declared == kind,
            None => true,
        }
    }

    /// Every part explicitly declared as `kind`, in part-name order.
    pub fn parts_of<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.overrides
            .iter()
            .filter(move |(_, declared)| declared.as_str() == kind)
            .map(|(name, _)| name.as_str())
    }
}

/// The `docProps/*` metadata every OOXML package carries.
///
/// Shared by the docx, xlsx and pptx readers: the parts are identical across
/// the three formats, and three copies of this function is three places for
/// them to drift.
pub fn read_meta(package: &Package) -> DocumentMeta {
    let mut meta = DocumentMeta::default();

    if let Ok(Some(core)) = package.optional_xml("docProps/core.xml") {
        let text = |name: &str| core.child(name).map(|e| e.text()).filter(|t| !t.is_empty());
        meta.title = text("title");
        meta.author = text("creator");
        meta.last_modified_by = text("lastModifiedBy");
        meta.created = text("created");
        meta.modified = text("modified");
        meta.language = text("language");
        meta.subject = text("subject");
        meta.keywords = text("keywords");
        meta.description = text("description");
    }

    if let Ok(Some(app)) = package.optional_xml("docProps/app.xml") {
        meta.application = app
            .child("Application")
            .map(|e| e.text())
            .filter(|t| !t.is_empty());
    }

    if let Ok(Some(custom)) = package.optional_xml("docProps/custom.xml") {
        for property in custom.children_named("property") {
            let Some(name) = property.attr("name") else {
                continue;
            };
            let value = property.children().map(|v| v.text()).collect::<String>();
            meta.custom.insert(name.to_string(), value);
        }
    }

    meta
}

/// Normalises a ZIP member name, rejecting anything that escapes the package.
///
/// Media file names inside a document are attacker-controlled, so a member
/// called `../../etc/passwd` must never become a part name (architecture §3.2).
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

fn rels_part_name(part: &str) -> Option<String> {
    let dir = parent_dir(part);
    let file = part.rsplit('/').next()?;
    Some(if dir.is_empty() {
        format!("_rels/{file}.rels")
    } else {
        format!("{dir}/_rels/{file}.rels")
    })
}

/// Resolves a relationship target against the part's directory.
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

    #[test]
    fn targets_resolve_relative_to_their_part() {
        assert_eq!(
            resolve_target("word", "media/image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            resolve_target("xl/worksheets", "../drawings/drawing1.xml").as_deref(),
            Some("xl/drawings/drawing1.xml")
        );
        assert_eq!(
            resolve_target("word", "/word/styles.xml").as_deref(),
            Some("word/styles.xml")
        );
    }

    #[test]
    fn targets_cannot_escape_the_package() {
        assert_eq!(resolve_target("word", "../../etc/passwd"), None);
        assert_eq!(resolve_target("", ".."), None);
    }

    #[test]
    fn member_names_are_sanitised() {
        assert_eq!(
            normalise_part_name("word//media/./image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            normalise_part_name("word\\media\\image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(normalise_part_name("../secret"), None);
        assert_eq!(normalise_part_name("/abs/path"), None);
        assert_eq!(normalise_part_name("C:/win"), None);
    }

    #[test]
    fn rels_part_names_follow_the_opc_convention() {
        assert_eq!(
            rels_part_name("word/document.xml").as_deref(),
            Some("word/_rels/document.xml.rels")
        );
        assert_eq!(
            rels_part_name("[Content_Types].xml").as_deref(),
            Some("_rels/[Content_Types].xml.rels")
        );
    }
}
