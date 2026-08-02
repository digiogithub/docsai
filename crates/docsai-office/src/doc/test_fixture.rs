//! Minimal synthetic `.doc` fixtures for unit and corpus tests.
//!
//! Builds a Word 97-shaped CFB with a single Unicode piece so the native
//! reader can be exercised without LibreOffice or Word installed.

use std::io::{Cursor, Write};

/// Builds a minimal `.doc` whose main text is `text` (use `\\r` for paragraph marks).
pub fn minimal_doc(text: &str) -> Vec<u8> {
    build_doc(text, false)
}

/// Same as [`minimal_doc`] but with the FIB encryption flag set.
pub fn encrypted_doc() -> Vec<u8> {
    build_doc("secret\r", true)
}

fn build_doc(text: &str, encrypted: bool) -> Vec<u8> {
    // Encode text as UTF-16LE and ensure a trailing paragraph mark.
    let mut chars: Vec<u16> = text.encode_utf16().collect();
    if chars.last().copied() != Some(0x000D) {
        chars.push(0x000D);
    }
    let ccp_text = chars.len() as u32;
    let text_bytes = u16s_to_le(&chars);

    // FIB sizes for Word 97 (nFib 0x00C1):
    // FibBase 32 + csw 2 + 14*2 + cslw 2 + 22*4 + cbRgFcLcb 2 + 0x5D*8 + cswNew 2
    const CSW: u16 = 0x000E;
    const CSLW: u16 = 0x0016;
    const CB_RG_FC_LCB: u16 = 0x005D; // 93 pairs
    let fib_len =
        32 + 2 + (CSW as usize) * 2 + 2 + (CSLW as usize) * 4 + 2 + (CB_RG_FC_LCB as usize) * 8 + 2;

    // Place text immediately after the FIB in WordDocument.
    let text_fc = fib_len as u32;
    // Align text_fc to even (Unicode pieces require even fc).
    let text_fc = text_fc + (text_fc & 1);
    let word_stream_len = text_fc as usize + text_bytes.len();

    // CLX with one piece covering [0, ccp_text].
    let mut clx = Vec::new();
    clx.push(2u8); // clxt = Pcdt
                   // PlcPcd size: 4*(1+1) + 8*1 = 16
    let plc_len: u32 = 16;
    clx.extend_from_slice(&plc_len.to_le_bytes());
    // CPs
    clx.extend_from_slice(&0u32.to_le_bytes());
    clx.extend_from_slice(&ccp_text.to_le_bytes());
    // Pcd: flags(2)=0, FcCompressed=text_fc (uncompressed), prm(2)=0
    clx.extend_from_slice(&0u16.to_le_bytes());
    clx.extend_from_slice(&text_fc.to_le_bytes());
    clx.extend_from_slice(&0u16.to_le_bytes());

    let table = clx;

    // Build FIB.
    let mut fib = vec![0u8; fib_len];
    // wIdent, nFib
    fib[0..2].copy_from_slice(&0xA5ECu16.to_le_bytes());
    fib[2..4].copy_from_slice(&0x00C1u16.to_le_bytes());
    // lid en-US
    fib[6..8].copy_from_slice(&0x0409u16.to_le_bytes());
    // flags: fWhichTblStm (0x0200) → use 1Table; optional fEncrypted
    let mut flags: u16 = 0x0200;
    if encrypted {
        flags |= 0x0100;
    }
    // fExtChar
    flags |= 0x1000;
    fib[0x0A..0x0C].copy_from_slice(&flags.to_le_bytes());
    // nFibBack
    fib[0x0C..0x0E].copy_from_slice(&0x00BFu16.to_le_bytes());

    let mut pos = 32usize;
    fib[pos..pos + 2].copy_from_slice(&CSW.to_le_bytes());
    pos += 2 + (CSW as usize) * 2;
    fib[pos..pos + 2].copy_from_slice(&CSLW.to_le_bytes());
    pos += 2;
    // FibRgLw97[0] = cbMac ≈ end of WordDocument
    let cb_mac = word_stream_len as u32;
    fib[pos..pos + 4].copy_from_slice(&cb_mac.to_le_bytes());
    // FibRgLw97[3] = ccpText
    fib[pos + 12..pos + 16].copy_from_slice(&ccp_text.to_le_bytes());
    pos += (CSLW as usize) * 4;
    fib[pos..pos + 2].copy_from_slice(&CB_RG_FC_LCB.to_le_bytes());
    pos += 2;
    // fcClx / lcbClx at pair index 32
    let clx_pair_off = pos + 32 * 8;
    fib[clx_pair_off..clx_pair_off + 4].copy_from_slice(&0u32.to_le_bytes()); // fcClx = 0
    fib[clx_pair_off + 4..clx_pair_off + 8].copy_from_slice(&(table.len() as u32).to_le_bytes());

    // Assemble WordDocument stream: FIB + pad + text
    let mut word = vec![0u8; word_stream_len];
    word[..fib.len()].copy_from_slice(&fib);
    word[text_fc as usize..text_fc as usize + text_bytes.len()].copy_from_slice(&text_bytes);

    // CFB package (V3, typical for .doc).
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut comp = cfb::CompoundFile::create_with_version(cfb::Version::V3, &mut cursor)
            .expect("create cfb");
        {
            let mut s = comp.create_stream("WordDocument").expect("WordDocument");
            s.write_all(&word).unwrap();
        }
        {
            let mut s = comp.create_stream("1Table").expect("1Table");
            s.write_all(&table).unwrap();
        }
        comp.flush().unwrap();
    }
    cursor.into_inner()
}

fn u16s_to_le(units: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(units.len() * 2);
    for &u in units {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_ole2() {
        let bytes = minimal_doc("Hi\r");
        assert_eq!(
            &bytes[..8],
            &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
        );
    }
}
