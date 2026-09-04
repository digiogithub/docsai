//! `TextHeaderAtom`, `TextCharsAtom`, `TextBytesAtom` and the part of
//! `StyleTextPropAtom` that says which outline level a paragraph sits at.
//!
//! The level is read rather than flattened, and that is deliberate:
//! [[99-phase-19-parser-choice]] rejected `office_oxide` partly because its IR
//! cannot carry an indent level, which makes a warning about the loss
//! impossible to state truthfully. Reading the atom is what lets this module
//! rebuild the nesting **and** stay silent about a loss that did not happen.

use docsai_model::text::{Block, BreakKind, Inline, Paragraph};

use crate::ReadError;

use super::records::{Record, RecordIter};
use super::window::Window;

pub(super) const RT_TEXT_HEADER_ATOM: u16 = 0x0F9F;
pub(super) const RT_TEXT_CHARS_ATOM: u16 = 0x0FA0;
pub(super) const RT_STYLE_TEXT_PROP_ATOM: u16 = 0x0FA1;
pub(super) const RT_TEXT_BYTES_ATOM: u16 = 0x0FA8;

/// `TextTypeEnum` ([MS-PPT] 2.13.33).
pub(super) mod text_type {
    pub(in crate::ppt) const TITLE: u32 = 0;
    pub(in crate::ppt) const BODY: u32 = 1;
    pub(in crate::ppt) const NOTES: u32 = 2;
    pub(in crate::ppt) const OTHER: u32 = 4;
    pub(in crate::ppt) const CENTER_BODY: u32 = 5;
    pub(in crate::ppt) const CENTER_TITLE: u32 = 6;
    pub(in crate::ppt) const HALF_BODY: u32 = 7;
    pub(in crate::ppt) const QUARTER_BODY: u32 = 8;
}

/// A shape's text, as the binary format holds it.
pub(super) struct ShapeText {
    /// `TextHeaderAtom.textType`, which is what says title from body here.
    pub(super) text_type: u32,
    /// The text as blocks, with the outline levels rebuilt into nested lists.
    pub(super) blocks: Vec<Block>,
    /// How many paragraphs the text held.
    pub(super) paragraphs: u32,
}

/// One paragraph before the list tree is rebuilt.
struct Para {
    text: Vec<u16>,
    level: u8,
    bulleted: Option<bool>,
}

/// Reads an `OfficeArtClientTextbox` ([MS-ODRAW] 0xF00D) into blocks.
///
/// `Ok(None)` when the box declares no `TextHeaderAtom` at all — which is a
/// shape with no text, not a text loss.
pub(super) fn read_client_textbox(textbox: &Record<'_>) -> Result<Option<ShapeText>, ReadError> {
    let mut text_type: Option<u32> = None;
    let mut units: Option<Vec<u16>> = None;
    let mut style: Option<Window<'_>> = None;

    for child in RecordIter::new(textbox.body) {
        let child = child?;
        match child.rec_type() {
            RT_TEXT_HEADER_ATOM => text_type = Some(child.body.u32_at(0)?),
            RT_TEXT_CHARS_ATOM => units = Some(utf16_units(&child.body)?),
            RT_TEXT_BYTES_ATOM => units = Some(latin1_units(&child.body)?),
            RT_STYLE_TEXT_PROP_ATOM => style = Some(child.body),
            _ => {}
        }
    }

    let Some(text_type) = text_type else {
        return Ok(None);
    };
    let units = units.unwrap_or_default();
    let props = match style {
        Some(style) => paragraph_props(&style, units.len())?,
        None => Vec::new(),
    };
    let paragraphs = split_paragraphs(&units, &props);
    let count = paragraphs.len() as u32;

    Ok(Some(ShapeText {
        text_type,
        blocks: rebuild(paragraphs),
        paragraphs: count,
    }))
}

/// `TextCharsAtom`: UTF-16LE, no terminator.
fn utf16_units(body: &Window<'_>) -> Result<Vec<u16>, ReadError> {
    if !body.len().is_multiple_of(2) {
        // Half a code unit is a stream that stops mid-character. Refused
        // rather than rounded down, which is the whole rule of this module.
        return Err(body.truncated(body.len().saturating_sub(1), 2));
    }
    let mut out = Vec::with_capacity(body.len() / 2);
    let mut off = 0usize;
    while body.advance(off, 2)? <= body.len() {
        out.push(body.u16_at(off)?);
        off = body.advance(off, 2)?;
    }
    Ok(out)
}

/// `TextBytesAtom`: one byte per character, high byte implicitly zero.
fn latin1_units(body: &Window<'_>) -> Result<Vec<u16>, ReadError> {
    let mut out = Vec::with_capacity(body.len());
    for off in 0..body.len() {
        out.push(u16::from(body.u8_at(off)?));
    }
    Ok(out)
}

/// The outline level and bullet state of every character position.
///
/// Expanded per character rather than kept as runs because a run count is a
/// number the file declares: expanding it against the real text length is what
/// stops a claimed count of four billion from being believed.
fn paragraph_props(
    style: &Window<'_>,
    text_len: usize,
) -> Result<Vec<(u8, Option<bool>)>, ReadError> {
    // [MS-PPT] 2.9.42: the runs cover the text plus its final paragraph mark.
    let total = text_len.saturating_add(1);
    let mut out: Vec<(u8, Option<bool>)> = Vec::with_capacity(total);
    let mut off = 0usize;
    while out.len() < total && style.advance(off, 10)? <= style.len() {
        let count = style.u32_at(off)? as usize;
        let level = style.i16_at(style.advance(off, 4)?)?;
        let masks = style.u32_at(style.advance(off, 6)?)?;
        let (bulleted, next) = pf_exception(style, style.advance(off, 10)?, masks)?;
        let level = level.clamp(0, 8) as u8;
        let take = count.min(total.saturating_sub(out.len()));
        out.extend(std::iter::repeat_n((level, bulleted), take));
        if next <= off {
            // A run that does not advance would spin the loop; the file is
            // malformed rather than merely old.
            break;
        }
        off = next;
        if count == 0 {
            break;
        }
    }
    Ok(out)
}

/// Skips a `TextPFException` ([MS-PPT] 2.9.44), returning its bullet state and
/// the offset just past it.
///
/// Every field is optional and its presence is a bit in `masks`, so the only
/// way to reach the next run is to walk them all in order.
fn pf_exception(
    style: &Window<'_>,
    at: usize,
    masks: u32,
) -> Result<(Option<bool>, usize), ReadError> {
    // PFMasks ([MS-PPT] 2.9.13), in the order the fields follow the masks.
    const HAS_BULLET: u32 = 1 << 0;
    const BULLET_FLAGS: u32 = 0b1111; // hasBullet | hasFont | hasColor | hasSize
    const BULLET_FONT: u32 = 1 << 4;
    const BULLET_COLOR: u32 = 1 << 5;
    const BULLET_SIZE: u32 = 1 << 6;
    const BULLET_CHAR: u32 = 1 << 7;
    const LEFT_MARGIN: u32 = 1 << 8;
    const INDENT: u32 = 1 << 10;
    const ALIGN: u32 = 1 << 11;
    const LINE_SPACING: u32 = 1 << 12;
    const SPACE_BEFORE: u32 = 1 << 13;
    const SPACE_AFTER: u32 = 1 << 14;
    const DEFAULT_TAB_SIZE: u32 = 1 << 15;
    const FONT_ALIGN: u32 = 1 << 16;
    const WRAP: u32 = (1 << 17) | (1 << 18) | (1 << 19);
    const TAB_STOPS: u32 = 1 << 20;
    const TEXT_DIRECTION: u32 = 1 << 21;

    let mut off = at;
    let mut bulleted = None;
    if masks & BULLET_FLAGS != 0 {
        let flags = style.u16_at(off)?;
        // Only the mask bit says the flag is *stated*; without it the value is
        // inherited and this reader must not claim to know it.
        if masks & HAS_BULLET != 0 {
            bulleted = Some(flags & 0x0001 != 0);
        }
        off = style.advance(off, 2)?;
    }
    for (mask, size) in [
        (BULLET_CHAR, 2usize),
        (BULLET_FONT, 2),
        (BULLET_SIZE, 2),
        (BULLET_COLOR, 4),
        (ALIGN, 2),
        (LINE_SPACING, 2),
        (SPACE_BEFORE, 2),
        (SPACE_AFTER, 2),
        (LEFT_MARGIN, 2),
        (INDENT, 2),
        (DEFAULT_TAB_SIZE, 2),
    ] {
        if masks & mask != 0 {
            off = style.advance(off, size)?;
        }
    }
    if masks & TAB_STOPS != 0 {
        let count = usize::from(style.u16_at(off)?);
        off = style.advance(off, 2)?;
        off = style.advance(off, count.saturating_mul(4))?;
    }
    for (mask, size) in [(FONT_ALIGN, 2usize), (WRAP, 2), (TEXT_DIRECTION, 2)] {
        if masks & mask != 0 {
            off = style.advance(off, size)?;
        }
    }
    // The walk must stay inside the atom even when nothing was read from the
    // last field: `advance` only checks arithmetic, this checks the window.
    if off > style.len() {
        return Err(style.truncated(at, off.saturating_sub(at)));
    }
    Ok((bulleted, off))
}

/// Splits the code units on `\r`, PowerPoint's paragraph mark.
fn split_paragraphs(units: &[u16], props: &[(u8, Option<bool>)]) -> Vec<Para> {
    let mut out: Vec<Para> = Vec::new();
    let mut start = 0usize;
    let push = |from: usize, to: usize, out: &mut Vec<Para>| {
        let (level, bulleted) = props.get(from).copied().unwrap_or((0, None));
        out.push(Para {
            text: units.get(from..to).unwrap_or_default().to_vec(),
            level,
            bulleted,
        });
    };
    for (index, unit) in units.iter().enumerate() {
        if *unit == 0x000D {
            push(start, index, &mut out);
            start = index.saturating_add(1);
        }
    }
    if start < units.len() {
        push(start, units.len(), &mut out);
    }
    out
}

/// Turns the paragraphs into blocks, nesting the outline levels into lists the
/// way `pptx` nests `a:pPr@lvl` — the same algorithm, because the tree shape is
/// the IR's rather than either format's.
fn rebuild(paragraphs: Vec<Para>) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    let mut run: Vec<(bool, u8, Paragraph)> = Vec::new();
    for para in paragraphs {
        let paragraph = to_paragraph(&para.text);
        let bulleted = match para.bulleted {
            _ if paragraph.is_empty() => false,
            // An indented paragraph is an item even where the bullet is
            // inherited: its level means nothing outside a list.
            _ if para.level > 0 => true,
            Some(stated) => stated,
            None => false,
        };
        if bulleted {
            run.push((false, para.level, paragraph));
        } else {
            flush(&mut run, &mut out);
            out.push(Block::Paragraph(paragraph));
        }
    }
    flush(&mut run, &mut out);
    out
}

fn flush(run: &mut Vec<(bool, u8, Paragraph)>, out: &mut Vec<Block>) {
    if run.is_empty() {
        return;
    }
    let items = std::mem::take(run);
    out.push(crate::pptx::text::build_list(&items));
}

/// One paragraph's code units, with `\v` kept as the line break it is.
fn to_paragraph(units: &[u16]) -> Paragraph {
    let mut content: Vec<Inline> = Vec::new();
    let mut buffer = String::new();
    for chunk in String::from_utf16_lossy(units).chars() {
        match chunk {
            '\u{000B}' => {
                if !buffer.is_empty() {
                    content.push(Inline::Text(std::mem::take(&mut buffer)));
                }
                content.push(Inline::Break(BreakKind::Line));
            }
            // 0x0D is the paragraph mark and never reaches here; the rest of
            // the C0 block is PowerPoint's internal furniture.
            c if c.is_control() && c != '\t' => {}
            c => buffer.push(c),
        }
    }
    if !buffer.is_empty() {
        content.push(Inline::Text(buffer));
    }
    Paragraph::new(content)
}

#[cfg(test)]
// The module-wide `deny(indexing_slicing)` is about the reader, not about
// its tests: a panicking index in a test is a failing test, which is the
// point. Assertions below index freely.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn window(bytes: &[u8]) -> Window<'_> {
        Window::stream("PowerPoint Document", bytes)
    }

    #[test]
    fn a_style_atom_run_yields_its_outline_level() {
        // The two runs of `corpus/ppt/mixed-media.ppt` slide 2, verbatim: one
        // paragraph at level 0 and one at level 1.
        let bytes = hex(
            "0b0000000000ff710a000f00b7f006006400000000fe6400ffffffffd8000100\
             0a0000000100ff750a000f00b7f006006400000000fe6400ffffffffb001d80001\
             00",
        );
        let props = paragraph_props(&window(&bytes), 20).unwrap();
        assert_eq!(props.len(), 21);
        assert_eq!(props.first().copied(), Some((0, Some(true))));
        assert_eq!(props.get(11).copied(), Some((1, Some(true))));
    }

    #[test]
    fn a_title_run_states_that_it_has_no_bullet() {
        // `corpus/ppt/basic-slides.ppt` slide 1's title.
        let bytes = hex("12000000000001780a00000000006400ffffffff010012000000");
        let props = paragraph_props(&window(&bytes), 17).unwrap();
        assert_eq!(props.first().copied(), Some((0, Some(false))));
    }

    #[test]
    fn a_run_count_far_larger_than_the_text_expands_no_further_than_the_text() {
        let mut bytes = hex("00000000000000000000");
        bytes.splice(0..4, u32::MAX.to_le_bytes());
        let props = paragraph_props(&window(&bytes), 3).unwrap();
        assert_eq!(props.len(), 4, "text length plus the paragraph mark");
    }

    #[test]
    fn paragraphs_split_on_the_carriage_return_and_keep_their_level() {
        let units: Vec<u16> = "uno\rdos".encode_utf16().collect();
        let props = vec![(0u8, Some(true)); 8];
        let paras = split_paragraphs(&units, &props);
        assert_eq!(paras.len(), 2);
        assert_eq!(String::from_utf16_lossy(&paras[1].text), "dos");
    }

    #[test]
    fn a_vertical_tab_is_a_line_break_inside_the_paragraph() {
        let units: Vec<u16> = "uno\u{000B}dos".encode_utf16().collect();
        let paragraph = to_paragraph(&units);
        assert!(matches!(
            paragraph.content.get(1),
            Some(Inline::Break(BreakKind::Line))
        ));
    }

    fn hex(text: &str) -> Vec<u8> {
        let clean: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        clean
            .as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
}
