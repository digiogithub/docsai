//! Media storage abstraction (architecture §3.2).
//!
//! The IR never holds image bytes: an [`ImageRef`](crate::image::ImageRef)
//! points at an [`AssetId`] resolved by an [`AssetStore`]. `docsai-convert`
//! supplies the real implementations (a directory for the CLI, memory/base64
//! for MCP); [`MemoryAssetStore`] here is the reference implementation the
//! readers use in tests — it performs no I/O, so `docsai-model` stays
//! I/O-free.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Key of a stored asset. Derived from the content hash, so identical bytes
/// always yield the same id and the store deduplicates for free.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AssetId(pub String);

impl AssetId {
    pub fn new(id: impl Into<String>) -> Self {
        AssetId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What the store knows about one asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AssetInfo {
    pub id: AssetId,
    /// Deterministic name `img-<hash8>.<ext>`; the extension comes from
    /// sniffing the content, never from the source document's naming.
    pub file_name: String,
    pub content_type: String,
    pub byte_len: usize,
    /// Native pixel dimensions, when the format header exposes them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_size_px: Option<(u32, u32)>,
}

/// Errors an asset store can return.
#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset {0} not found")]
    NotFound(AssetId),
    #[error("asset is empty")]
    Empty,
    #[error("asset storage failed: {0}")]
    Io(String),
}

/// Content-addressed media storage.
pub trait AssetStore {
    /// Stores `bytes` and returns their id. Storing the same bytes twice must
    /// return the same id and must not duplicate storage.
    fn put(&mut self, bytes: &[u8]) -> Result<AssetId, AssetError>;

    /// The stored bytes, if present.
    fn get(&self, id: &AssetId) -> Option<&[u8]>;

    /// The manifest entry, if present.
    fn info(&self, id: &AssetId) -> Option<&AssetInfo>;

    /// Every id in the store, in deterministic order.
    fn ids(&self) -> Vec<AssetId>;

    /// Number of stored assets.
    fn len(&self) -> usize {
        self.ids().len()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An in-memory [`AssetStore`]; the reference implementation.
#[derive(Debug, Clone, Default)]
pub struct MemoryAssetStore {
    entries: BTreeMap<AssetId, (AssetInfo, Vec<u8>)>,
}

impl MemoryAssetStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The manifest, ordered by id.
    pub fn manifest(&self) -> Vec<&AssetInfo> {
        self.entries.values().map(|(info, _)| info).collect()
    }
}

impl AssetStore for MemoryAssetStore {
    fn put(&mut self, bytes: &[u8]) -> Result<AssetId, AssetError> {
        if bytes.is_empty() {
            return Err(AssetError::Empty);
        }
        let id = AssetId::new(content_hash(bytes));
        if !self.entries.contains_key(&id) {
            let info = describe(&id, bytes);
            self.entries.insert(id.clone(), (info, bytes.to_vec()));
        }
        Ok(id)
    }

    fn get(&self, id: &AssetId) -> Option<&[u8]> {
        self.entries.get(id).map(|(_, bytes)| bytes.as_slice())
    }

    fn info(&self, id: &AssetId) -> Option<&AssetInfo> {
        self.entries.get(id).map(|(info, _)| info)
    }

    fn ids(&self) -> Vec<AssetId> {
        self.entries.keys().cloned().collect()
    }
}

/// Builds the manifest entry for some bytes: id, deterministic file name,
/// sniffed content type and native dimensions.
pub fn describe(id: &AssetId, bytes: &[u8]) -> AssetInfo {
    let (content_type, ext) = sniff(bytes);
    AssetInfo {
        id: id.clone(),
        file_name: format!("img-{}.{}", &id.0[..id.0.len().min(8)], ext),
        content_type: content_type.to_string(),
        byte_len: bytes.len(),
        native_size_px: dimensions(bytes),
    }
}

/// FNV-1a 64-bit over the content, rendered as 16 lowercase hex digits.
///
/// A non-cryptographic hash is enough here: the id only has to be stable and
/// collision-free in practice for the media of one document, and avoiding a
/// crypto dependency keeps `docsai-model` free of heavy deps. The convert
/// layer is free to substitute a stronger hash without changing this trait.
pub fn content_hash(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // Mix in the length so that same-content-different-length is impossible.
    hash ^= bytes.len() as u64;
    hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    format!("{hash:016x}")
}

/// Guesses `(content-type, extension)` from the magic bytes.
///
/// Deliberately header-only and dependency-free: docsai never re-encodes a
/// bitmap, so it only needs to name and measure it.
pub fn sniff(bytes: &[u8]) -> (&'static str, &'static str) {
    match bytes {
        b if b.starts_with(b"\x89PNG\r\n\x1a\n") => ("image/png", "png"),
        b if b.starts_with(b"\xff\xd8\xff") => ("image/jpeg", "jpg"),
        b if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") => ("image/gif", "gif"),
        b if b.starts_with(b"BM") => ("image/bmp", "bmp"),
        b if b.starts_with(b"RIFF") && b.len() > 12 && &b[8..12] == b"WEBP" => {
            ("image/webp", "webp")
        }
        b if b.starts_with(b"II*\x00") || b.starts_with(b"MM\x00*") => ("image/tiff", "tiff"),
        b if is_emf(b) => ("image/x-emf", "emf"),
        [0xd7, 0xcd, 0xc6, 0x9a, ..] => ("image/x-wmf", "wmf"),
        b if b.starts_with(b"<?xml") || b.starts_with(b"<svg") => ("image/svg+xml", "svg"),
        _ => ("application/octet-stream", "bin"),
    }
}

fn is_emf(bytes: &[u8]) -> bool {
    bytes.len() >= 44
        && u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == 1
        && &bytes[40..44] == b" EMF"
}

/// Native pixel dimensions read straight from the format header.
///
/// Returns `None` for formats whose size is not a pixel count (EMF/WMF are
/// vector) or that this function does not parse.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    match sniff(bytes).1 {
        "png" if bytes.len() >= 24 => Some((be32(&bytes[16..20]), be32(&bytes[20..24]))),
        "gif" if bytes.len() >= 10 => Some((
            u16::from_le_bytes([bytes[6], bytes[7]]) as u32,
            u16::from_le_bytes([bytes[8], bytes[9]]) as u32,
        )),
        "bmp" if bytes.len() >= 26 => Some((
            u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]),
            u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]),
        )),
        "jpg" => jpeg_dimensions(bytes),
        _ => None,
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// Walks JPEG markers to the first frame header.
fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // SOF0..SOF15 except the non-frame markers DHT/JPG/DAC.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((w, h));
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes() -> Vec<u8> {
        let mut v = b"\x89PNG\r\n\x1a\n".to_vec();
        v.extend_from_slice(&[0, 0, 0, 13]);
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&120u32.to_be_bytes());
        v.extend_from_slice(&90u32.to_be_bytes());
        v.extend_from_slice(&[8, 2, 0, 0, 0]);
        v
    }

    #[test]
    fn identical_bytes_share_one_asset() {
        let mut store = MemoryAssetStore::new();
        let a = store.put(&png_bytes()).unwrap();
        let b = store.put(&png_bytes()).unwrap();
        assert_eq!(a, b);
        assert_eq!(store.len(), 1, "deduplicated by content");
    }

    #[test]
    fn different_bytes_get_different_ids() {
        let mut store = MemoryAssetStore::new();
        let a = store.put(&png_bytes()).unwrap();
        let b = store.put(b"GIF87a\x30\x00\x20\x00rest").unwrap();
        assert_ne!(a, b);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn empty_assets_are_rejected() {
        let mut store = MemoryAssetStore::new();
        assert!(matches!(store.put(&[]), Err(AssetError::Empty)));
    }

    #[test]
    fn file_names_are_deterministic_and_content_typed() {
        let mut store = MemoryAssetStore::new();
        let id = store.put(&png_bytes()).unwrap();
        let info = store.info(&id).unwrap();
        assert!(info.file_name.starts_with("img-"));
        assert!(info.file_name.ends_with(".png"));
        assert_eq!(info.content_type, "image/png");
        assert_eq!(info.native_size_px, Some((120, 90)));
    }

    #[test]
    fn sniffs_the_formats_the_readers_meet() {
        assert_eq!(sniff(&png_bytes()).1, "png");
        assert_eq!(sniff(b"GIF89a....").1, "gif");
        assert_eq!(sniff(b"\xff\xd8\xff\xe0").1, "jpg");
        assert_eq!(sniff(b"BM....").1, "bmp");
        assert_eq!(sniff(&[0xd7, 0xcd, 0xc6, 0x9a, 0]).1, "wmf");
        assert_eq!(sniff(b"nothing recognisable").1, "bin");
    }

    #[test]
    fn sniffs_emf_by_signature_not_extension() {
        let mut emf = vec![0u8; 44];
        emf[0] = 1;
        emf[40..44].copy_from_slice(b" EMF");
        assert_eq!(sniff(&emf), ("image/x-emf", "emf"));
        assert_eq!(dimensions(&emf), None, "vector formats have no pixel size");
    }

    #[test]
    fn gif_dimensions_are_little_endian() {
        let gif = b"GIF87a\x30\x00\x20\x00\xf0\x00";
        assert_eq!(dimensions(gif), Some((48, 32)));
    }
}
