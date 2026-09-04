//! The only way an offset becomes bytes.
//!
//! [[99-phase-19-parser-choice]] makes bounds checking *structural* rather than
//! sprinkled: one type owns the `&[u8]`, every accessor returns `Result`, every
//! offset is combined with `checked_add`, and — the decision that separates
//! this reader from the crate 19-C rejected — **there is no clamping accessor
//! at all**. The `.doc` piece table's `.min(bytes.len())` (`piece_table.rs:237`
//! and `:253`) is the pattern this file exists not to repeat: a clamp turns
//! `corpus/ppt/truncated.ppt`'s 1 532-byte overrun into a silently short read,
//! which is the silent degradation AGENTS.md §7 rule 3 forbids.
//!
//! A `Window` is a view *into* a stream. It remembers the stream it came from
//! and where in that stream it starts, so every error can name an absolute
//! offset a reader can go and look at with a hex editor.

use crate::ReadError;

/// A bounds-checked view over part of an OLE2 stream.
#[derive(Clone, Copy, Debug)]
pub(super) struct Window<'a> {
    /// The bytes of this view, and nothing outside it.
    bytes: &'a [u8],
    /// Absolute offset of `bytes[0]` inside `part`.
    base: usize,
    /// Total length of the stream, for the diagnostic.
    stream_len: usize,
    /// The OLE2 stream name, for the diagnostic.
    part: &'static str,
}

impl<'a> Window<'a> {
    /// A window over a whole stream.
    pub(super) fn stream(part: &'static str, bytes: &'a [u8]) -> Self {
        Window {
            bytes,
            base: 0,
            stream_len: bytes.len(),
            part,
        }
    }

    /// Length of this view.
    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Absolute offset of this view inside its stream.
    pub(super) fn base(&self) -> usize {
        self.base
    }

    /// The error a read past the end produces, naming where and how much.
    pub(super) fn truncated(&self, off: usize, need: usize) -> ReadError {
        ReadError::Truncated {
            part: self.part.to_string(),
            at: self.base.saturating_add(off),
            need,
            have: self.stream_len,
        }
    }

    /// `len` bytes at `off`, or an error naming the shortfall.
    ///
    /// The one place a range is taken. `checked_add` first, so an `off + len`
    /// that overflows `usize` is refused rather than wrapping into a read that
    /// looks in bounds.
    pub(super) fn slice(&self, off: usize, len: usize) -> Result<&'a [u8], ReadError> {
        let end = off
            .checked_add(len)
            .ok_or_else(|| self.truncated(off, len))?;
        self.bytes
            .get(off..end)
            .ok_or_else(|| self.truncated(off, len))
    }

    /// A sub-view, which is how a record body is reached.
    pub(super) fn sub(&self, off: usize, len: usize) -> Result<Window<'a>, ReadError> {
        Ok(Window {
            bytes: self.slice(off, len)?,
            base: self.base.saturating_add(off),
            stream_len: self.stream_len,
            part: self.part,
        })
    }

    /// `N` bytes at `off` as an array, so the decoders below index a fixed-size
    /// array with a constant rather than slicing.
    pub(super) fn array<const N: usize>(&self, off: usize) -> Result<[u8; N], ReadError> {
        let slice = self.slice(off, N)?;
        <[u8; N]>::try_from(slice).map_err(|_| self.truncated(off, N))
    }

    pub(super) fn u8_at(&self, off: usize) -> Result<u8, ReadError> {
        let [b] = self.array::<1>(off)?;
        Ok(b)
    }

    pub(super) fn u16_at(&self, off: usize) -> Result<u16, ReadError> {
        Ok(u16::from_le_bytes(self.array::<2>(off)?))
    }

    pub(super) fn i16_at(&self, off: usize) -> Result<i16, ReadError> {
        Ok(i16::from_le_bytes(self.array::<2>(off)?))
    }

    pub(super) fn u32_at(&self, off: usize) -> Result<u32, ReadError> {
        Ok(u32::from_le_bytes(self.array::<4>(off)?))
    }

    pub(super) fn i32_at(&self, off: usize) -> Result<i32, ReadError> {
        Ok(i32::from_le_bytes(self.array::<4>(off)?))
    }

    /// `a + b`, refused rather than wrapped.
    ///
    /// Every offset walked forward inside a record goes through here, so a
    /// declared length near `usize::MAX` cannot fold back into the window.
    pub(super) fn advance(&self, at: usize, by: usize) -> Result<usize, ReadError> {
        at.checked_add(by).ok_or_else(|| self.truncated(at, by))
    }
}

#[cfg(test)]
// The module-wide `deny(indexing_slicing)` is about the reader, not about
// its tests: a panicking index in a test is a failing test, which is the
// point. Assertions below index freely.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_read_past_the_end_names_where_and_how_much() {
        let window = Window::stream("PowerPoint Document", &[0u8; 16]);
        let err = window.slice(12, 8).unwrap_err();
        match err {
            ReadError::Truncated {
                part,
                at,
                need,
                have,
            } => {
                assert_eq!(part, "PowerPoint Document");
                assert_eq!((at, need, have), (12, 8, 16));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_offset_plus_length_that_overflows_usize_is_refused() {
        let window = Window::stream("Current User", &[0u8; 16]);
        // The classic wrap: `off + len` folds to a small number and the range
        // looks in bounds. `checked_add` is what stops it.
        assert!(window.slice(usize::MAX, 8).is_err());
        assert!(window.slice(8, usize::MAX).is_err());
        assert!(window.sub(usize::MAX - 1, 4).is_err());
        assert!(window.advance(usize::MAX, 1).is_err());
    }

    #[test]
    fn a_sub_window_keeps_the_absolute_offset_of_the_stream() {
        let bytes: Vec<u8> = (0..64u8).collect();
        let window = Window::stream("PowerPoint Document", &bytes);
        let inner = window.sub(40, 8).unwrap();
        assert_eq!(inner.base(), 40);
        assert_eq!(inner.len(), 8);
        assert_eq!(inner.u8_at(0).unwrap(), 40);
        // The error still speaks in stream coordinates, not window ones.
        match inner.slice(4, 8).unwrap_err() {
            ReadError::Truncated { at, have, .. } => assert_eq!((at, have), (44, 64)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn there_is_no_accessor_that_shortens_a_read() {
        // Not a test of behaviour but of surface: every accessor is fallible,
        // so a caller cannot ask for "as much as you have".
        let window = Window::stream("Pictures", &[1, 2, 3]);
        assert!(window.u32_at(0).is_err());
        assert!(window.u16_at(2).is_err());
        assert_eq!(window.u16_at(0).unwrap(), 0x0201);
    }
}
