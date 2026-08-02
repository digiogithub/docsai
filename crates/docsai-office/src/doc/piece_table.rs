//! Piece table (CLX / PlcPcd) → plain Unicode text for the main document.

use docsai_model::report::{ConversionReport, Warning};

use super::fib::Fib;
use crate::error::ReadError;

/// Extracts the main-document character text via the piece table.
pub(crate) fn extract_main_text(
    word: &[u8],
    table: &[u8],
    fib: &Fib,
    report: &mut ConversionReport,
) -> Result<String, ReadError> {
    if fib.lcb_clx == 0 {
        return Err(ReadError::WrongShape {
            part: "CLX".into(),
            expected: "non-empty piece table (lcbClx > 0)".into(),
        });
    }
    let start = fib.fc_clx as usize;
    let end = start
        .checked_add(fib.lcb_clx as usize)
        .ok_or_else(|| ReadError::WrongShape {
            part: "CLX".into(),
            expected: "fcClx+lcbClx within table stream".into(),
        })?;
    if end > table.len() {
        return Err(ReadError::WrongShape {
            part: "CLX".into(),
            expected: format!(
                "CLX range [{start}, {end}) inside table stream ({} bytes)",
                table.len()
            ),
        });
    }
    let clx = &table[start..end];
    let pieces = parse_clx(clx)?;
    if pieces.is_empty() {
        return Err(ReadError::WrongShape {
            part: "CLX".into(),
            expected: "at least one piece in PlcPcd".into(),
        });
    }

    // Main document text is the first `ccpText` characters across pieces.
    // Footnotes/headers live in later CP ranges and are ignored here.
    let mut out = String::new();
    let mut remaining = fib.ccp_text as usize;
    for piece in &pieces {
        if remaining == 0 {
            break;
        }
        let take = piece.char_count().min(remaining);
        let slice = decode_piece(word, piece, take, report);
        out.push_str(&slice);
        remaining = remaining.saturating_sub(take);
    }

    Ok(out)
}

#[derive(Debug, Clone)]
struct Piece {
    /// Absolute byte offset in the WordDocument stream.
    fc: u32,
    /// Number of characters in this piece.
    chars: u32,
    /// When true, text is stored as one byte per character (Windows-1252).
    compressed: bool,
}

impl Piece {
    fn char_count(&self) -> usize {
        self.chars as usize
    }
}

/// Parses the CLX structure into piece descriptors covering the main text.
fn parse_clx(clx: &[u8]) -> Result<Vec<Piece>, ReadError> {
    let mut pos = 0usize;
    // Skip any leading Prc (grpprl) blocks: clxt == 1.
    while pos < clx.len() {
        let clxt = clx[pos];
        if clxt == 1 {
            pos += 1;
            if pos + 2 > clx.len() {
                return Err(bad_clx("truncated Prc"));
            }
            let cb = u16::from_le_bytes([clx[pos], clx[pos + 1]]) as usize;
            pos += 2 + cb;
            continue;
        }
        if clxt == 2 {
            pos += 1;
            break;
        }
        return Err(bad_clx(format!("unknown clxt {clxt}")));
    }
    if pos + 4 > clx.len() {
        return Err(bad_clx("truncated Pcdt lcb"));
    }
    let lcb = u32::from_le_bytes(clx[pos..pos + 4].try_into().unwrap()) as usize;
    pos += 4;
    if pos + lcb > clx.len() {
        return Err(bad_clx("Pcdt lcb exceeds CLX"));
    }
    let plc = &clx[pos..pos + lcb];
    // PlcPcd: (n+1) CP u32s + n Pcd (8 bytes). 4(n+1) + 8n = 12n + 4 = lcb
    // => 12n = lcb - 4 => n = (lcb - 4) / 12
    if lcb < 16 || !(lcb - 4).is_multiple_of(12) {
        return Err(bad_clx(format!("PlcPcd size {lcb} is not 12n+4")));
    }
    let n = (lcb - 4) / 12;
    let mut cps = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let off = i * 4;
        let cp = u32::from_le_bytes(plc[off..off + 4].try_into().unwrap());
        cps.push(cp);
    }
    let pcd_base = (n + 1) * 4;
    let mut pieces = Vec::with_capacity(n);
    for i in 0..n {
        let off = pcd_base + i * 8;
        let pcd = &plc[off..off + 8];
        // Pcd: 2 bytes flags, 4 bytes FcCompressed, 2 bytes prm
        let fc_raw = u32::from_le_bytes(pcd[2..6].try_into().unwrap());
        let compressed = (fc_raw & 0x4000_0000) != 0;
        let fc = if compressed {
            (fc_raw & 0x3FFF_FFFF) / 2
        } else {
            fc_raw & 0x3FFF_FFFF
        };
        let cp_start = cps[i];
        let cp_end = cps[i + 1];
        if cp_end < cp_start {
            return Err(bad_clx("CP array is not non-decreasing"));
        }
        let chars = cp_end - cp_start;
        pieces.push(Piece {
            fc,
            chars,
            compressed,
        });
    }
    Ok(pieces)
}

fn decode_piece(
    word: &[u8],
    piece: &Piece,
    max_chars: usize,
    report: &mut ConversionReport,
) -> String {
    let n = max_chars.min(piece.char_count());
    if n == 0 {
        return String::new();
    }
    let fc = piece.fc as usize;
    if piece.compressed {
        let end = fc.saturating_add(n);
        if fc >= word.len() {
            report.warn(Warning::Degraded {
                what: "piece".into(),
                why: format!("compressed piece fc={fc} outside WordDocument"),
            });
            return String::new();
        }
        let end = end.min(word.len());
        // Word's "compressed" encoding is effectively the ANSI code page of the
        // document (usually Windows-1252). Map bytes as Windows-1252.
        word[fc..end].iter().map(|&b| cp1252_char(b)).collect()
    } else {
        let byte_len = n.saturating_mul(2);
        let end = fc.saturating_add(byte_len);
        if fc >= word.len() {
            report.warn(Warning::Degraded {
                what: "piece".into(),
                why: format!("unicode piece fc={fc} outside WordDocument"),
            });
            return String::new();
        }
        let end = end.min(word.len());
        let slice = &word[fc..end];
        let mut u16s = Vec::with_capacity(slice.len() / 2);
        let mut i = 0;
        while i + 1 < slice.len() {
            u16s.push(u16::from_le_bytes([slice[i], slice[i + 1]]));
            i += 2;
        }
        String::from_utf16_lossy(&u16s)
    }
}

fn cp1252_char(b: u8) -> char {
    // Windows-1252: 0x00-0x7F and most high bytes map like Unicode Latin-1
    // with a few exceptions in 0x80-0x9F.
    match b {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8A => 'Š',
        0x8B => '‹',
        0x8C => 'Œ',
        0x8E => 'Ž',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '˜',
        0x99 => '™',
        0x9A => 'š',
        0x9B => '›',
        0x9C => 'œ',
        0x9E => 'ž',
        0x9F => 'Ÿ',
        0x81 | 0x8D | 0x8F | 0x90 | 0x9D => '\u{FFFD}',
        other => other as char,
    }
}

fn bad_clx(msg: impl Into<String>) -> ReadError {
    ReadError::WrongShape {
        part: "CLX".into(),
        expected: msg.into(),
    }
}
