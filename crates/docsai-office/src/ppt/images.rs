//! The `Pictures` stream's BLIPs, and the `OfficeArtBStoreContainer` that says
//! which shape shows which one.
//!
//! [[99-phase-19-parser-choice]] assumed a recovered picture would be «an asset
//! with no geometry», and warned accordingly. It has geometry: the shape's
//! `pib` property ([MS-ODRAW] 0x0104) indexes the store, the store entry's
//! `foDelay` is the BLIP's offset in this stream, and the shape's client anchor
//! is where it sits. So the picture is placed, and — AGENTS.md §7 rule 3's
//! second half — nothing warns that it was not. A BLIP **no** shape points at
//! is the case that really has no geometry, and only that case warns.

use std::collections::BTreeMap;

use crate::ReadError;

use super::records::{self, RecordIter, MAX_RECORD_DEPTH};
use super::window::Window;

pub(super) const OFFICE_ART_BSTORE_CONTAINER: u16 = 0xF001;
pub(super) const OFFICE_ART_FBSE: u16 = 0xF007;

/// `foDelay` sits 28 bytes into an `OfficeArtFBSE` ([MS-ODRAW] 2.2.32):
/// `btWin32`, `btMacOS`, `rgbUid`, `tag`, `size`, `cRef`.
const FBSE_FO_DELAY: usize = 28;

/// A BLIP recovered from the `Pictures` stream.
pub(super) struct Blip {
    /// Offset of the BLIP record in the stream — the key `foDelay` uses.
    pub(super) at: usize,
    /// The image bytes, ready for the asset store.
    pub(super) payload: Vec<u8>,
}

/// A BLIP this reader recognises but cannot hand on as a file.
pub(super) struct UndecodedBlip {
    pub(super) at: usize,
    pub(super) kind: &'static str,
}

/// What the `Pictures` stream held.
#[derive(Default)]
pub(super) struct Pictures {
    pub(super) blips: Vec<Blip>,
    pub(super) undecoded: Vec<UndecodedBlip>,
}

/// Walks the `Pictures` stream ([MS-ODRAW] 2.2.23 `OfficeArtBlip`).
///
/// Every BLIP starts with one `rgbUid`, and a second when the record instance
/// is odd. A bitmap then has a one-byte `tag` and its file bytes; a metafile
/// has a 34-byte header and *deflate-compressed* bytes, which is why the two
/// are separated rather than both called "recovered".
pub(super) fn read_pictures(bytes: &[u8]) -> Result<Pictures, ReadError> {
    let window = Window::stream(super::persist::PICTURES, bytes);
    let mut out = Pictures::default();
    for record in RecordIter::new(window) {
        let record = record?;
        let uids = 1 + usize::from(record.instance() & 1);
        let header = record.body.advance(0, uids.saturating_mul(16))?;
        let kind = match record.rec_type() {
            0xF01D => Some("jpeg"),
            0xF01E => Some("png"),
            0xF029 => Some("tiff"),
            0xF01A => None, // EMF
            0xF01B => None, // WMF
            0xF01C => None, // PICT
            0xF01F => None, // DIB
            _ => continue,
        };
        match kind {
            Some(_) => {
                // One `tag` byte, then the file exactly as it would be on disk.
                let at = record.body.advance(header, 1)?;
                let payload = record
                    .body
                    .slice(at, record.body.len().saturating_sub(at))?
                    .to_vec();
                out.blips.push(Blip {
                    at: record.at,
                    payload,
                });
            }
            None => out.undecoded.push(UndecodedBlip {
                at: record.at,
                kind: match record.rec_type() {
                    0xF01A => "EMF",
                    0xF01B => "WMF",
                    0xF01C => "PICT",
                    _ => "DIB",
                },
            }),
        }
    }
    Ok(out)
}

/// The `foDelay` of every entry in the drawing group's BLIP store, in order.
///
/// The `pib` property is a **one-based** index into this list.
pub(super) fn blip_store(drawing_group: Window<'_>) -> Result<Vec<usize>, ReadError> {
    let Some(store) =
        records::find_deep(drawing_group, OFFICE_ART_BSTORE_CONTAINER, MAX_RECORD_DEPTH)?
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in store.children() {
        let entry = entry?;
        if entry.rec_type() != OFFICE_ART_FBSE {
            continue;
        }
        out.push(entry.body.u32_at(FBSE_FO_DELAY)? as usize);
    }
    Ok(out)
}

/// Indexes the recovered BLIPs by their stream offset.
pub(super) fn by_offset(pictures: &Pictures) -> BTreeMap<usize, &[u8]> {
    pictures
        .blips
        .iter()
        .map(|blip| (blip.at, blip.payload.as_slice()))
        .collect()
}

#[cfg(test)]
// The module-wide `deny(indexing_slicing)` is about the reader, not about
// its tests: a panicking index in a test is a failing test, which is the
// point. Assertions below index freely.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn blip(rec_type: u16, instance: u16, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(instance << 4).to_le_bytes());
        out.extend_from_slice(&rec_type.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_png_blip_gives_back_the_file_bytes() {
        let mut body = vec![0xAA; 16]; // one rgbUid
        body.push(0xFF); // tag
        body.extend_from_slice(b"\x89PNG\r\n\x1a\n");
        let bytes = blip(0xF01E, 0x6E0, &body);
        let pictures = read_pictures(&bytes).unwrap();
        assert_eq!(pictures.blips.len(), 1);
        assert_eq!(pictures.blips[0].at, 0);
        assert_eq!(pictures.blips[0].payload, b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn an_odd_instance_means_a_second_uid() {
        let mut body = vec![0xAA; 32]; // two rgbUids
        body.push(0xFF);
        body.extend_from_slice(b"\x89PNG");
        let bytes = blip(0xF01E, 0x6E1, &body);
        let pictures = read_pictures(&bytes).unwrap();
        assert_eq!(pictures.blips[0].payload, b"\x89PNG");
    }

    #[test]
    fn a_metafile_blip_is_recorded_as_undecoded_rather_than_handed_on_compressed() {
        let body = vec![0u8; 16 + 34 + 8];
        let bytes = blip(0xF01B, 0x216, &body);
        let pictures = read_pictures(&bytes).unwrap();
        assert!(pictures.blips.is_empty());
        assert_eq!(pictures.undecoded.len(), 1);
        assert_eq!(pictures.undecoded[0].kind, "WMF");
    }

    #[test]
    fn a_blip_shorter_than_its_uids_is_refused() {
        let bytes = blip(0xF01E, 0x6E0, &[0u8; 4]);
        assert!(read_pictures(&bytes).is_err());
    }
}
