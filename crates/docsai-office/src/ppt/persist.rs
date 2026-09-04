//! `Current User`, the `UserEditAtom` chain and the persist directory
//! ([MS-PPT] 2.1.2), which is how an offset in a `.ppt` becomes a container.
//!
//! The decision that separates this from the crate 19-C rejected: **there is no
//! scan fallback.** When `CurrentUserAtom.offsetToCurrentEdit` does not resolve
//! against the `PowerPoint Document` stream, the file is refused. Guessing at
//! the highest-offset `UserEditAtom` is what turns `corpus/ppt/truncated.ppt`
//! into a plausible one-slide deck of PowerPoint's placeholder prompt text, and
//! a caller has no way to tell that from a real presentation.

use std::collections::{BTreeMap, BTreeSet};

use crate::ReadError;

use super::records::{self, HEADER_LEN};
use super::window::Window;

/// The OLE2 stream holding the binary presentation ([MS-PPT] 2.1.1).
pub(super) const POWERPOINT_DOCUMENT: &str = "PowerPoint Document";
/// The OLE2 stream holding the `CurrentUserAtom` ([MS-PPT] 2.3.2).
pub(super) const CURRENT_USER: &str = "Current User";
/// The OLE2 stream holding the BLIPs ([MS-PPT] 2.1.1).
pub(super) const PICTURES: &str = "Pictures";

pub(super) const RT_USER_EDIT_ATOM: u16 = 0x0FF5;
pub(super) const RT_CURRENT_USER_ATOM: u16 = 0x0FF6;
pub(super) const RT_PERSIST_DIRECTORY_ATOM: u16 = 0x1772;

/// `CurrentUserAtom.headerToken` for a document that is **not** encrypted.
pub(super) const TOKEN_PLAIN: u32 = 0xE391_C05F;
/// `CurrentUserAtom.headerToken` for a document that **is** ([MS-PPT] 2.3.2).
pub(super) const TOKEN_ENCRYPTED: u32 = 0xF3D1_C4DF;

/// How many `UserEditAtom`s a chain may hold before the file is hostile rather
/// than well edited.
const MAX_EDIT_CHAIN: usize = 4096;

/// What the `Current User` stream says, read before any content.
#[derive(Debug, Clone, Copy)]
pub(super) struct CurrentUser {
    /// `headerToken`: [`TOKEN_PLAIN`], [`TOKEN_ENCRYPTED`], or something else.
    pub(super) header_token: u32,
    /// Where the most recent `UserEditAtom` claims to be.
    pub(super) offset_to_current_edit: u32,
}

impl CurrentUser {
    /// True when the container declares itself encrypted.
    pub(super) fn is_encrypted(&self) -> bool {
        self.header_token == TOKEN_ENCRYPTED
    }
}

/// Parses the `Current User` stream.
pub(super) fn read_current_user(bytes: &[u8]) -> Result<CurrentUser, ReadError> {
    let window = Window::stream(CURRENT_USER, bytes);
    let atom = records::find(window, RT_CURRENT_USER_ATOM, None)?.ok_or_else(|| {
        ReadError::WrongShape {
            part: CURRENT_USER.into(),
            expected: "a CurrentUserAtom (record type 0x0FF6)".into(),
        }
    })?;
    Ok(CurrentUser {
        header_token: atom.body.u32_at(4)?,
        offset_to_current_edit: atom.body.u32_at(8)?,
    })
}

/// One `UserEditAtom`, reduced to the three fields this reader uses.
#[derive(Debug, Clone, Copy)]
struct UserEdit {
    offset_last_edit: u32,
    offset_persist_directory: u32,
    doc_persist_id_ref: u32,
}

fn read_user_edit(window: &Window<'_>, at: usize) -> Result<UserEdit, ReadError> {
    let header = records::header_at(window, at)?;
    if header.rec_type != RT_USER_EDIT_ATOM {
        return Err(ReadError::WrongShape {
            part: format!("{POWERPOINT_DOCUMENT}@{}", window.base().saturating_add(at)),
            expected: format!(
                "a UserEditAtom (record type 0x0FF5), found record type {:#06X}",
                header.rec_type
            ),
        });
    }
    let body = window.sub(window.advance(at, HEADER_LEN)?, header.len as usize)?;
    Ok(UserEdit {
        offset_last_edit: body.u32_at(8)?,
        offset_persist_directory: body.u32_at(12)?,
        doc_persist_id_ref: body.u32_at(16)?,
    })
}

/// The persist directory: a persist id, and where its container starts.
#[derive(Debug)]
pub(super) struct PersistDirectory {
    entries: BTreeMap<u32, usize>,
    /// `docPersistIdRef` of the most recent edit — the `DocumentContainer`.
    document_id: u32,
}

impl PersistDirectory {
    /// The stream offset a persist id resolves to.
    pub(super) fn offset(&self, id: u32) -> Option<usize> {
        self.entries.get(&id).copied()
    }

    /// The persist id of the `DocumentContainer`.
    pub(super) fn document_id(&self) -> u32 {
        self.document_id
    }

    /// Every `(id, offset)` pair, in id order.
    pub(super) fn entries(&self) -> impl Iterator<Item = (u32, usize)> + '_ {
        self.entries.iter().map(|(id, off)| (*id, *off))
    }
}

/// Follows the edit chain and merges the persist directories, newest first.
///
/// Newest first is what «most recent edit wins» means: an id already present
/// came from a later edit and is not overwritten by an earlier one.
pub(super) fn read_persist_directory(
    window: &Window<'_>,
    current: &CurrentUser,
) -> Result<PersistDirectory, ReadError> {
    // The first bound, and the one `truncated.ppt` hits: 10 946 against a
    // 5 632-byte stream fails here, in `Window::sub`, with no fallback.
    let mut at = current.offset_to_current_edit as usize;
    let mut edits: Vec<UserEdit> = Vec::new();
    let mut seen: BTreeSet<usize> = BTreeSet::new();
    loop {
        if !seen.insert(at) {
            return Err(ReadError::WrongShape {
                part: POWERPOINT_DOCUMENT.into(),
                expected: format!("an acyclic UserEditAtom chain; offset {at} repeats"),
            });
        }
        if edits.len() >= MAX_EDIT_CHAIN {
            return Err(ReadError::TooLarge(format!(
                "`{POWERPOINT_DOCUMENT}` has more than {MAX_EDIT_CHAIN} UserEditAtoms"
            )));
        }
        let edit = read_user_edit(window, at)?;
        edits.push(edit);
        if edit.offset_last_edit == 0 {
            break;
        }
        at = edit.offset_last_edit as usize;
    }

    let document_id = edits.first().map(|e| e.doc_persist_id_ref).unwrap_or(0);
    let mut entries: BTreeMap<u32, usize> = BTreeMap::new();
    for edit in &edits {
        read_directory_atom(window, edit.offset_persist_directory as usize, &mut entries)?;
    }

    // Every offset the directory hands out must at least hold a record header.
    // Checking it once here is what lets the walks above treat a resolved
    // persist id as a real container rather than re-deciding per use.
    for (id, offset) in &entries {
        records::header_at(window, *offset).map_err(|error| match error {
            ReadError::Truncated { part, at, need, .. } => ReadError::Truncated {
                part: format!("{part} (persist id {id})"),
                at,
                need,
                have: window.len(),
            },
            other => other,
        })?;
    }

    Ok(PersistDirectory {
        entries,
        document_id,
    })
}

fn read_directory_atom(
    window: &Window<'_>,
    at: usize,
    entries: &mut BTreeMap<u32, usize>,
) -> Result<(), ReadError> {
    let header = records::header_at(window, at)?;
    if header.rec_type != RT_PERSIST_DIRECTORY_ATOM {
        return Err(ReadError::WrongShape {
            part: format!("{POWERPOINT_DOCUMENT}@{}", window.base().saturating_add(at)),
            expected: format!(
                "a PersistDirectoryAtom (record type 0x1772), found record type {:#06X}",
                header.rec_type
            ),
        });
    }
    let body = window.sub(window.advance(at, HEADER_LEN)?, header.len as usize)?;

    let mut off = 0usize;
    while off < body.len() {
        let packed = body.u32_at(off)?;
        let persist_id = packed & 0x000F_FFFF;
        let count = packed >> 20;
        off = body.advance(off, 4)?;
        for step in 0..count {
            let offset = body.u32_at(off)? as usize;
            off = body.advance(off, 4)?;
            let id = persist_id.saturating_add(step);
            // Newest edit first: an id already resolved stays resolved.
            entries.entry(id).or_insert(offset);
        }
    }
    Ok(())
}

#[cfg(test)]
// The module-wide `deny(indexing_slicing)` is about the reader, not about
// its tests: a panicking index in a test is a failing test, which is the
// point. Assertions below index freely.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn atom(rec_type: u16, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&rec_type.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    fn user_edit(offset_last_edit: u32, offset_dir: u32, doc_id: u32) -> Vec<u8> {
        let mut body = vec![0u8; 28];
        body.splice(8..12, offset_last_edit.to_le_bytes());
        body.splice(12..16, offset_dir.to_le_bytes());
        body.splice(16..20, doc_id.to_le_bytes());
        atom(RT_USER_EDIT_ATOM, &body)
    }

    #[test]
    fn a_cyclic_edit_chain_is_refused_rather_than_walked_forever() {
        // `offsetLastEdit == 0` ends a chain, so a cycle needs two atoms at
        // non-zero offsets pointing at each other.
        let mut bytes = vec![0u8; 8];
        let first = bytes.len() as u32;
        bytes.extend(user_edit(0, 0, 1));
        let second = bytes.len() as u32;
        bytes.extend(user_edit(first, 0, 1));
        // Close the loop: the first atom points forward at the second.
        let field = first as usize + HEADER_LEN + 8;
        bytes.splice(field..field + 4, second.to_le_bytes());

        let window = Window::stream(POWERPOINT_DOCUMENT, &bytes);
        let current = CurrentUser {
            header_token: TOKEN_PLAIN,
            offset_to_current_edit: first,
        };
        let error = read_persist_directory(&window, &current).unwrap_err();
        assert!(error.to_string().contains("acyclic"), "{error}");
    }

    #[test]
    fn the_newest_edit_wins_when_two_directories_name_the_same_id() {
        // Directory B (older, at the tail) says id 1 → 100; directory A
        // (newer) says id 1 → 200. The newest edit is read first.
        let dir_a = {
            let mut body = Vec::new();
            body.extend_from_slice(&((1u32 << 20) | 1).to_le_bytes());
            body.extend_from_slice(&0u32.to_le_bytes());
            atom(RT_PERSIST_DIRECTORY_ATOM, &body)
        };
        let dir_b = {
            let mut body = Vec::new();
            body.extend_from_slice(&((1u32 << 20) | 1).to_le_bytes());
            body.extend_from_slice(&8u32.to_le_bytes());
            atom(RT_PERSIST_DIRECTORY_ATOM, &body)
        };
        // Layout: [0] a header-sized filler, [8] another, then the atoms.
        let mut bytes = Vec::new();
        bytes.extend(atom(0x03E8, b"")); // 0: a container the ids point at
        bytes.extend(atom(0x03EE, b"")); // 8: another
        let dir_a_at = bytes.len() as u32;
        bytes.extend(dir_a);
        let dir_b_at = bytes.len() as u32;
        bytes.extend(dir_b);
        let newest_at = bytes.len() as u32;
        bytes.extend(user_edit(0, dir_b_at, 1)); // the older edit
        let older_at = newest_at;
        let newest_at = bytes.len() as u32;
        bytes.extend(user_edit(older_at, dir_a_at, 1));

        let window = Window::stream(POWERPOINT_DOCUMENT, &bytes);
        let current = CurrentUser {
            header_token: TOKEN_PLAIN,
            offset_to_current_edit: newest_at,
        };
        let dir = read_persist_directory(&window, &current).unwrap();
        assert_eq!(dir.offset(1), Some(0), "the newer directory wins");
        assert_eq!(dir.document_id(), 1);
    }

    #[test]
    fn a_directory_offset_that_leaves_the_stream_is_refused_at_build_time() {
        let dir = {
            let mut body = Vec::new();
            body.extend_from_slice(&((1u32 << 20) | 1).to_le_bytes());
            body.extend_from_slice(&9999u32.to_le_bytes());
            atom(RT_PERSIST_DIRECTORY_ATOM, &body)
        };
        let mut bytes = dir;
        let edit_at = bytes.len() as u32;
        bytes.extend(user_edit(0, 0, 1));
        let window = Window::stream(POWERPOINT_DOCUMENT, &bytes);
        let current = CurrentUser {
            header_token: TOKEN_PLAIN,
            offset_to_current_edit: edit_at,
        };
        let error = read_persist_directory(&window, &current).unwrap_err();
        assert!(error.to_string().contains("persist id 1"), "{error}");
    }
}
