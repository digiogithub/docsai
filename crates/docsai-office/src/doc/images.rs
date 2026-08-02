//! Embedded image extraction from MS-DOC (OfficeArt / Escher BLIPs).
//!
//! The degraded path does not reconstruct wrap or floating geometry. BLIP
//! payloads are stored in the [`AssetStore`] and emitted as inline images with
//! an [`ImageGeometryDegraded`](docsai_model::Warning::ImageGeometryDegraded)
//! warning.

use docsai_model::assets::AssetStore;
use docsai_model::image::ImageRef;
use docsai_model::report::ConversionReport;

use super::image_ref_from_asset;

/// Pulls embedded bitmaps out of the WordDocument and optional Data streams.
pub(crate) fn extract_embedded_images(
    word: &[u8],
    data: Option<&[u8]>,
    assets: &mut dyn AssetStore,
    report: &mut ConversionReport,
) -> Vec<ImageRef> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for stream in std::iter::once(word).chain(data) {
        for payload in find_image_payloads(stream) {
            // Dedup by content hash via AssetStore; also skip identical raw slices.
            let key = (payload.len(), simple_hash(&payload));
            if !seen.insert(key) {
                continue;
            }
            match assets.put(&payload) {
                Ok(id) => {
                    report.stats.images = report.stats.images.saturating_add(1);
                    out.push(image_ref_from_asset(assets, id, report));
                }
                Err(err) => {
                    report.warn(docsai_model::Warning::AssetIssue {
                        asset: "doc-blip".into(),
                        why: err.to_string(),
                    });
                }
            }
        }
    }
    out
}

fn simple_hash(bytes: &[u8]) -> u64 {
    // Same FNV-1a 64 spirit as the asset store; only used for de-dup in-scan.
    let mut hash = 0xcbf29ce484222325u64;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// Finds PNG / JPEG / GIF / BMP payloads, including those wrapped in OfficeArt
/// BLIP records.
fn find_image_payloads(stream: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();

    // 1) OfficeArt BLIP records (record types 0xF018..=0xF117).
    out.extend(scan_office_art_blips(stream));

    // 2) Raw signature scan as a safety net (skips payloads already captured).
    for sig in [
        Signature::Png,
        Signature::Jpeg,
        Signature::Gif,
        Signature::Bmp,
    ] {
        let mut start = 0usize;
        while let Some(rel) = find_bytes(&stream[start..], sig.magic()) {
            let at = start + rel;
            if let Some(payload) = sig.extract(stream, at) {
                if !out.iter().any(|p| p == &payload) {
                    out.push(payload);
                }
            }
            start = at + sig.magic().len();
        }
    }
    out
}

#[derive(Clone, Copy)]
enum Signature {
    Png,
    Jpeg,
    Gif,
    Bmp,
}

impl Signature {
    fn magic(self) -> &'static [u8] {
        match self {
            Signature::Png => b"\x89PNG\r\n\x1a\n",
            Signature::Jpeg => b"\xff\xd8\xff",
            Signature::Gif => b"GIF8",
            Signature::Bmp => b"BM",
        }
    }

    fn extract(self, stream: &[u8], at: usize) -> Option<Vec<u8>> {
        match self {
            Signature::Png => extract_png(stream, at),
            Signature::Jpeg => extract_jpeg(stream, at),
            Signature::Gif => extract_gif(stream, at),
            Signature::Bmp => extract_bmp(stream, at),
        }
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// OfficeArt record header is 8 bytes: ver/instance (2), type (2), len (4).
fn scan_office_art_blips(stream: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 8 <= stream.len() {
        let rec_type = u16::from_le_bytes([stream[i + 2], stream[i + 3]]);
        let rec_len =
            u32::from_le_bytes([stream[i + 4], stream[i + 5], stream[i + 6], stream[i + 7]])
                as usize;
        // BLIP records live in 0xF018..=0xF117 (MS-ODRAW).
        if (0xF018..=0xF117).contains(&rec_type) && rec_len > 16 && i + 8 + rec_len <= stream.len()
        {
            let body = &stream[i + 8..i + 8 + rec_len];
            // BLIP body: optional metafile header, then 16-byte RGBUid (+ optional
            // second UID), then tag byte, then bits. Try signature scan inside body.
            if let Some(payload) = first_image_in(body) {
                out.push(payload);
            }
            i += 8 + rec_len;
            continue;
        }
        // Not a BLIP (or bogus length): advance one byte to resync.
        i += 1;
    }
    out
}

fn first_image_in(bytes: &[u8]) -> Option<Vec<u8>> {
    for sig in [
        Signature::Png,
        Signature::Jpeg,
        Signature::Gif,
        Signature::Bmp,
    ] {
        if let Some(rel) = find_bytes(bytes, sig.magic()) {
            if let Some(payload) = sig.extract(bytes, rel) {
                return Some(payload);
            }
        }
    }
    None
}

fn extract_png(data: &[u8], at: usize) -> Option<Vec<u8>> {
    // Walk chunks until IEND.
    let mut pos = at + 8; // past signature
    if pos > data.len() {
        return None;
    }
    loop {
        if pos + 12 > data.len() {
            return None;
        }
        let len = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
        let ctype = &data[pos + 4..pos + 8];
        let next = pos.checked_add(12 + len)?;
        if next > data.len() {
            return None;
        }
        if ctype == b"IEND" {
            return Some(data[at..next].to_vec());
        }
        // Guard against runaway.
        if len > 64 * 1024 * 1024 {
            return None;
        }
        pos = next;
    }
}

fn extract_jpeg(data: &[u8], at: usize) -> Option<Vec<u8>> {
    // Find EOI 0xFFD9.
    let mut i = at + 3;
    while i + 1 < data.len() {
        if data[i] == 0xFF && data[i + 1] == 0xD9 {
            return Some(data[at..=i + 1].to_vec());
        }
        i += 1;
        // Cap scan to 32 MiB from start.
        if i - at > 32 * 1024 * 1024 {
            break;
        }
    }
    None
}

fn extract_gif(data: &[u8], at: usize) -> Option<Vec<u8>> {
    // GIF ends with trailer 0x3B; take until last 0x3B within a bound.
    if at + 6 > data.len() {
        return None;
    }
    let bound = (at + 16 * 1024 * 1024).min(data.len());
    let slice = &data[at..bound];
    let trailer = slice.iter().rposition(|&b| b == 0x3B)?;
    Some(data[at..=at + trailer].to_vec())
}

fn extract_bmp(data: &[u8], at: usize) -> Option<Vec<u8>> {
    if at + 6 > data.len() {
        return None;
    }
    let size = u32::from_le_bytes(data[at + 2..at + 6].try_into().ok()?) as usize;
    if !(14..=64 * 1024 * 1024).contains(&size) {
        return None;
    }
    let end = at.checked_add(size)?;
    if end > data.len() {
        return None;
    }
    Some(data[at..end].to_vec())
}
