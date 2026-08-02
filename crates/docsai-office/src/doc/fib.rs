//! File Information Block (FIB) for MS-DOC (Word 97–2003).
//!
//! Only the fields required by the degraded text extractor are decoded.
//! See [MS-DOC] 2.5.

use crate::error::ReadError;

/// Subset of the Word FIB used by the native `.doc` reader.
#[derive(Debug, Clone)]
pub(crate) struct Fib {
    /// `nFib` version stamp (informational).
    #[allow(dead_code)]
    pub n_fib: u16,
    /// When set, the document is encrypted or obfuscated — refuse to read.
    pub encrypted: bool,
    /// When set, the table stream is `1Table`; otherwise `0Table`.
    pub which_table_1: bool,
    /// Character count of the main document text (CCP).
    pub ccp_text: u32,
    /// Byte offset of the CLX in the table stream.
    pub fc_clx: u32,
    /// Byte size of the CLX in the table stream.
    pub lcb_clx: u32,
}

impl Fib {
    /// Parses a FIB from the start of the `WordDocument` stream.
    pub fn parse(word: &[u8]) -> Result<Self, ReadError> {
        if word.len() < 32 {
            return Err(ReadError::WrongShape {
                part: "WordDocument".into(),
                expected: "FIB of at least 32 bytes".into(),
            });
        }
        let w_ident = u16::from_le_bytes([word[0], word[1]]);
        if w_ident != 0xA5EC {
            return Err(ReadError::WrongShape {
                part: "WordDocument".into(),
                expected: format!("FIB wIdent 0xA5EC (got 0x{w_ident:04X})"),
            });
        }
        let n_fib = u16::from_le_bytes([word[2], word[3]]);
        let flags = u16::from_le_bytes([word[0x0A], word[0x0B]]);
        let f_encrypted = (flags & 0x0100) != 0;
        let f_which_tbl_stm = (flags & 0x0200) != 0;
        let f_obfuscated = (flags & 0x8000) != 0;

        // Layout after FibBase (32 bytes):
        //   csw:u16, FibRgW97 (csw * 2 bytes),
        //   cslw:u16, FibRgLw97 (cslw * 4 bytes),
        //   cbRgFcLcb:u16, FibRgFcLcbBlob (cbRgFcLcb * 8 bytes), ...
        let mut pos = 32usize;
        let csw = read_u16(word, pos)? as usize;
        pos += 2 + csw * 2;
        let cslw = read_u16(word, pos)? as usize;
        pos += 2;
        let rg_lw_start = pos;
        pos += cslw * 4;
        if cslw < 4 {
            return Err(ReadError::WrongShape {
                part: "WordDocument".into(),
                expected: "FibRgLw97 with ccpText".into(),
            });
        }
        // FibRgLw97: [0]=cbMac, [1]=reserved1, [2]=reserved2, [3]=ccpText
        let ccp_text = read_u32(word, rg_lw_start + 3 * 4)?;

        let cb_rg_fc_lcb = read_u16(word, pos)? as usize;
        pos += 2;
        let rg_fc_start = pos;
        // fcClx / lcbClx is pair index 32 in FibRgFcLcb97 (MS-DOC 2.5.6).
        const CLX_PAIR: usize = 32;
        if cb_rg_fc_lcb <= CLX_PAIR {
            return Err(ReadError::WrongShape {
                part: "WordDocument".into(),
                expected: format!(
                    "FibRgFcLcb with fcClx (need >{CLX_PAIR} pairs, got {cb_rg_fc_lcb})"
                ),
            });
        }
        let clx_off = rg_fc_start + CLX_PAIR * 8;
        let fc_clx = read_u32(word, clx_off)?;
        let lcb_clx = read_u32(word, clx_off + 4)?;

        Ok(Fib {
            n_fib,
            encrypted: f_encrypted || f_obfuscated,
            which_table_1: f_which_tbl_stm,
            ccp_text,
            fc_clx,
            lcb_clx,
        })
    }
}

fn read_u16(buf: &[u8], at: usize) -> Result<u16, ReadError> {
    let end = at.checked_add(2).ok_or_else(|| truncated(at))?;
    if end > buf.len() {
        return Err(truncated(at));
    }
    Ok(u16::from_le_bytes([buf[at], buf[at + 1]]))
}

fn read_u32(buf: &[u8], at: usize) -> Result<u32, ReadError> {
    let end = at.checked_add(4).ok_or_else(|| truncated(at))?;
    if end > buf.len() {
        return Err(truncated(at));
    }
    Ok(u32::from_le_bytes([
        buf[at],
        buf[at + 1],
        buf[at + 2],
        buf[at + 3],
    ]))
}

fn truncated(at: usize) -> ReadError {
    ReadError::WrongShape {
        part: "WordDocument".into(),
        expected: format!("FIB field at offset {at}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_ident() {
        let mut buf = vec![0u8; 64];
        buf[0] = 0x00;
        buf[1] = 0x00;
        assert!(Fib::parse(&buf).is_err());
    }
}
