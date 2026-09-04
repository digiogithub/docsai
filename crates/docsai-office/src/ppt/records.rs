//! The [MS-PPT] / [MS-ODRAW] record header, and a walk that refuses before it
//! seeks.
//!
//! Every record is `recVer` (4 bits), `recInstance` (12 bits), `recType`
//! (16 bits), `recLen` (32 bits), then `recLen` bytes of body. A container has
//! `recVer == 0xF` and its body is more records.
//!
//! The rule this file exists for: **`recLen` is validated before it is used**.
//! `corpus/ppt/truncated.ppt` has a header at offset 5 300 declaring a body
//! that ends at 7 164 in a 5 632-byte stream, so [`RecordIter`] yields an
//! error there **from the declared length alone, with no seek and no read**.

use crate::ReadError;

use super::window::Window;

/// Bytes of a record header.
pub(super) const HEADER_LEN: usize = 8;

/// How deep a container walk may go. [MS-PPT] nests four or five levels; a file
/// that nests more is hostile, not old.
pub(super) const MAX_RECORD_DEPTH: u8 = 24;

/// A record header, decoded.
#[derive(Debug, Clone, Copy)]
pub(super) struct RecordHeader {
    pub(super) version: u8,
    pub(super) instance: u16,
    pub(super) rec_type: u16,
    pub(super) len: u32,
}

impl RecordHeader {
    /// True when the body is a sequence of records rather than a payload.
    pub(super) fn is_container(&self) -> bool {
        self.version == 0x0F
    }
}

/// A record: its header, its body, and where in the stream it started.
#[derive(Clone, Copy, Debug)]
pub(super) struct Record<'a> {
    pub(super) header: RecordHeader,
    pub(super) body: Window<'a>,
    /// Absolute offset of the record header inside its stream.
    pub(super) at: usize,
}

impl<'a> Record<'a> {
    pub(super) fn rec_type(&self) -> u16 {
        self.header.rec_type
    }

    pub(super) fn instance(&self) -> u16 {
        self.header.instance
    }

    /// The records directly inside this one. Empty for an atom, which is a
    /// statement about the file rather than a silent skip: an atom's body is a
    /// payload and is read by the caller that knows the type.
    pub(super) fn children(&self) -> RecordIter<'a> {
        RecordIter::new(self.body)
    }
}

/// Reads a record header at `off`, or says what it needed.
pub(super) fn header_at(window: &Window<'_>, off: usize) -> Result<RecordHeader, ReadError> {
    let head = window.array::<HEADER_LEN>(off)?;
    let ver_inst = u16::from_le_bytes([head[0], head[1]]);
    Ok(RecordHeader {
        version: (ver_inst & 0x000F) as u8,
        instance: ver_inst >> 4,
        rec_type: u16::from_le_bytes([head[2], head[3]]),
        len: u32::from_le_bytes([head[4], head[5], head[6], head[7]]),
    })
}

/// Walks the records of one window, refusing rather than clamping.
///
/// Three properties a hostile file needs it to have:
///
/// * a record whose declared length leaves the window is an `Err`, not a short
///   read (the `truncated.ppt` trap);
/// * a record of length zero still advances by [`HEADER_LEN`], so no walk
///   stands still;
/// * a record can never contain itself, because a child's body is a strict
///   sub-window that starts eight bytes further in.
pub(super) struct RecordIter<'a> {
    window: Window<'a>,
    at: usize,
    done: bool,
}

impl<'a> RecordIter<'a> {
    pub(super) fn new(window: Window<'a>) -> Self {
        RecordIter {
            window,
            at: 0,
            done: false,
        }
    }

    fn fail(&mut self, error: ReadError) -> Option<Result<Record<'a>, ReadError>> {
        self.done = true;
        Some(Err(error))
    }
}

impl<'a> Iterator for RecordIter<'a> {
    type Item = Result<Record<'a>, ReadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        // `at` never exceeds the window: it only moves to the end of a body
        // that `Window::sub` already accepted.
        let remaining = self.window.len().saturating_sub(self.at);
        if remaining == 0 {
            self.done = true;
            return None;
        }
        if remaining < HEADER_LEN {
            // Trailing bytes too short to be a header are not padding to skip;
            // they are a stream that stops mid-record.
            let error = self.window.truncated(self.at, HEADER_LEN);
            return self.fail(error);
        }
        let at = self.at;
        let header = match header_at(&self.window, at) {
            Ok(header) => header,
            Err(error) => return self.fail(error),
        };
        let body_at = match self.window.advance(at, HEADER_LEN) {
            Ok(off) => off,
            Err(error) => return self.fail(error),
        };
        // The check that matters, and it happens here rather than at the first
        // read inside the body.
        let body = match self.window.sub(body_at, header.len as usize) {
            Ok(body) => body,
            Err(error) => return self.fail(error),
        };
        self.at = match self.window.advance(body_at, header.len as usize) {
            Ok(off) => off,
            Err(error) => return self.fail(error),
        };
        Some(Ok(Record {
            header,
            body,
            at: self.window.base().saturating_add(at),
        }))
    }
}

/// The first direct child of `window` with this type, and optionally this
/// instance.
///
/// An error anywhere in the walk is an error of the whole search: a corrupt
/// sibling is not something to step over quietly.
pub(super) fn find<'a>(
    window: Window<'a>,
    rec_type: u16,
    instance: Option<u16>,
) -> Result<Option<Record<'a>>, ReadError> {
    for record in RecordIter::new(window) {
        let record = record?;
        if record.rec_type() == rec_type && instance.is_none_or(|i| record.instance() == i) {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

/// The first descendant with this type, at most `depth` container levels down.
pub(super) fn find_deep<'a>(
    window: Window<'a>,
    rec_type: u16,
    depth: u8,
) -> Result<Option<Record<'a>>, ReadError> {
    if depth == 0 {
        return Ok(None);
    }
    for record in RecordIter::new(window) {
        let record = record?;
        if record.rec_type() == rec_type {
            return Ok(Some(record));
        }
        if record.header.is_container() {
            if let Some(found) = find_deep(record.body, rec_type, depth.saturating_sub(1))? {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
// The module-wide `deny(indexing_slicing)` is about the reader, not about
// its tests: a panicking index in a test is a failing test, which is the
// point. Assertions below index freely.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// `recVer|recInstance`, `recType`, `recLen`, then the body.
    fn record(ver_inst: u16, rec_type: u16, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&ver_inst.to_le_bytes());
        out.extend_from_slice(&rec_type.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_declared_length_that_leaves_the_window_is_refused_before_any_seek() {
        let mut bytes = record(0x0000, 0x0FA8, b"hola");
        // Restate the length as far more than the stream holds.
        bytes.splice(4..8, 4096u32.to_le_bytes());
        let window = Window::stream("PowerPoint Document", &bytes);
        let error = RecordIter::new(window).next().unwrap().unwrap_err();
        match error {
            ReadError::Truncated { at, need, have, .. } => {
                assert_eq!((at, need, have), (8, 4096, 12));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_zero_length_record_still_advances() {
        let mut bytes = record(0x0000, 0x03EA, b"");
        bytes.extend(record(0x0000, 0x0FA8, b"hi"));
        let window = Window::stream("PowerPoint Document", &bytes);
        let types: Vec<u16> = RecordIter::new(window)
            .map(|r| r.unwrap().rec_type())
            .collect();
        assert_eq!(types, vec![0x03EA, 0x0FA8]);
    }

    #[test]
    fn trailing_bytes_too_short_for_a_header_are_an_error_not_padding() {
        let mut bytes = record(0x0000, 0x0FA8, b"hi");
        bytes.extend_from_slice(&[0, 0, 0]);
        let window = Window::stream("PowerPoint Document", &bytes);
        let mut iter = RecordIter::new(window);
        assert!(iter.next().unwrap().is_ok());
        assert!(iter.next().unwrap().is_err());
        assert!(iter.next().is_none(), "the walk stops after it refuses");
    }

    #[test]
    fn a_container_body_is_a_strict_sub_window() {
        let inner = record(0x0000, 0x0FA8, b"hola");
        let outer = record(0x000F, 0x03EE, &inner);
        let window = Window::stream("PowerPoint Document", &outer);
        let slide = find(window, 0x03EE, None).unwrap().unwrap();
        assert!(slide.header.is_container());
        assert_eq!(slide.body.base(), HEADER_LEN);
        let child = slide.children().next().unwrap().unwrap();
        assert_eq!(child.at, HEADER_LEN);
        assert_eq!(child.body.base(), 2 * HEADER_LEN);
    }

    #[test]
    fn find_deep_stops_at_the_depth_budget() {
        let leaf = record(0x0000, 0x0FA8, b"x");
        let l3 = record(0x000F, 0x0003, &leaf);
        let l2 = record(0x000F, 0x0002, &l3);
        let l1 = record(0x000F, 0x0001, &l2);
        let window = Window::stream("PowerPoint Document", &l1);
        assert!(find_deep(window, 0x0FA8, 4).unwrap().is_some());
        assert!(find_deep(window, 0x0FA8, 3).unwrap().is_none());
    }
}
