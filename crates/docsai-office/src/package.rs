//! OPC package handling: the ZIP container and its relationship graph.

use std::collections::BTreeMap;
use std::io::{Read, Seek};

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
