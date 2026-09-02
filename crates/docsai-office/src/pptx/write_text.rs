//! DrawingML text on the way out: IR blocks into `a:p` (Phase 15, 15-B).
//!
//! The exact mirror of [`super::text`], and it has to be exact in one specific
//! sense: what this module writes, that reader must read back as the blocks it
//! was given. Everything else about the round trip is measured against that
//! identity, so a rule bent here shows up as a diff there.
//!
//! Two of the reader's judgement calls are inverted here rather than copied:
//!
//! * **The bullet.** A body placeholder bullets its paragraphs by inheritance,
//!   so a list item inside one writes no `a:buChar` at all, and it is a *plain*
//!   paragraph in that shape that needs the explicit `a:buNone`. Outside a body
//!   placeholder it is the other way round. A nested item needs neither: `lvl`
//!   greater than zero already means «item» on the way in.
//! * **The delta.** A run stores what it adds to the cascade, and the cascade
//!   is in the layout and the master, which this phase never rewrites. So the
//!   delta is written back as direct properties and subtracted again on the next
//!   read, arriving at the same value it started as.
//!
//! What this module refuses is as load-bearing as what it writes. An inline it
//! cannot put back — an image, a footnote, a field needing an id nothing
//! recorded — is an `Err` naming it, and the caller keeps the original shape
//! rather than writing a shape with a hole in it (`AGENTS.md` §7 rule 3).
//!
//! A hyperlink is the one inline that needs something outside the part: an
//! `r:id` that exists in the slide's own `_rels`. Since 15-C the caller supplies
//! a [`Links`] that hands one over — reusing the relationship the deck already
//! had whenever the target is unchanged, so a link that was read comes back as
//! the bytes it was read from — and a target it cannot make a relationship for
//! is still a refusal rather than a run with the link quietly removed.

use docsai_model::style::{Align, FontProps, LineHeight, Underline, VertAlign};
use docsai_model::text::{Block, BreakKind, Inline, Paragraph, RunProps};

/// Why a body could not be written, in words a warning can carry.
pub(super) type Unsupported = String;

/// The part's relationships, seen from the text writer.
///
/// DrawingML states a hyperlink as an `r:id` on the run, so the text cannot be
/// written without the part that holds the ids. The trait keeps that dependency
/// one call wide: this module asks for an id and knows nothing about how the
/// relationship part is stored, spliced or numbered.
pub(super) trait Links {
    /// The relationship id for a hyperlink target, allocating one if the part
    /// does not already have it, or `None` when it cannot be expressed.
    fn hyperlink(&mut self, target: &str) -> Option<String>;
}

/// Writes the `a:p` sequence of one text body.
///
/// `bulleted` is what the shape inherits: true for a body placeholder, whose
/// paragraphs are bullets unless they say otherwise.
pub(super) fn write_body(
    blocks: &[Block],
    bulleted: bool,
    lang: Option<&str>,
    links: &mut dyn Links,
) -> Result<String, Unsupported> {
    let mut flat = Vec::new();
    flatten(blocks, None, &mut flat)?;

    let mut out = String::new();
    for entry in &flat {
        write_paragraph(entry, bulleted, lang, links, &mut out)?;
    }
    Ok(out)
}

/// A paragraph as the format stores it: flat, with its own level and bullet.
struct Flat<'a> {
    paragraph: &'a Paragraph,
    /// `None` for a paragraph that is not a list item.
    item: Option<(bool, u8)>,
}

/// Undoes [`super::text`]'s list rebuilding: a nested list is a run of
/// paragraphs whose only difference is the `lvl` they carry.
fn flatten<'a>(
    blocks: &'a [Block],
    item: Option<(bool, u8)>,
    out: &mut Vec<Flat<'a>>,
) -> Result<(), Unsupported> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => out.push(Flat { paragraph, item }),
            Block::List(list) => {
                for entry in &list.items {
                    flatten(&entry.blocks, Some((list.ordered, list.level)), out)?;
                }
            }
            other => return Err(format!("a {} in a text body", block_name(other))),
        }
    }
    Ok(())
}

fn block_name(block: &Block) -> &'static str {
    match block {
        Block::Paragraph(_) => "paragraph",
        Block::Heading(_) => "heading",
        Block::List(_) => "list",
        Block::Table(_) => "table",
        Block::Image(_) => "image",
        Block::Raw(_) => "raw block",
        _ => "block this writer has no DrawingML for",
    }
}

fn write_paragraph(
    entry: &Flat<'_>,
    bulleted: bool,
    lang: Option<&str>,
    links: &mut dyn Links,
    out: &mut String,
) -> Result<(), Unsupported> {
    let paragraph = entry.paragraph;
    let mut body = String::new();
    write_ppr(entry, bulleted, &mut body);
    for inline in &paragraph.content {
        write_inline(inline, &RunProps::default(), lang, None, links, &mut body)?;
    }
    // An empty paragraph carries an `a:endParaRPr`: it is where the properties
    // of the text that would be typed there live, and every producer writes
    // one. A paragraph with runs only needs it when it holds something.
    if !paragraph.format.run_direct.is_empty() || paragraph.is_empty() {
        body.push_str("<a:endParaRPr");
        write_font_attrs(&paragraph.format.run_direct, &mut body);
        write_font_children(
            &paragraph.format.run_direct,
            None,
            &mut body,
            "a:endParaRPr",
        );
    }
    if body.is_empty() {
        out.push_str("<a:p/>");
    } else {
        out.push_str("<a:p>");
        out.push_str(&body);
        out.push_str("</a:p>");
    }
    Ok(())
}

/// `a:pPr`: the outline level, the bullet control and the direct paragraph
/// properties, in the order the schema fixes for `CT_TextParagraphProperties`.
fn write_ppr(entry: &Flat<'_>, bulleted: bool, out: &mut String) {
    let props = &entry.paragraph.format.direct;
    let level = entry.item.map(|(_, level)| level).unwrap_or(0);
    let bullet = bullet_element(entry, bulleted);

    if level == 0 && bullet.is_none() && props.is_empty() {
        return;
    }

    out.push_str("<a:pPr");
    if let Some(indent) = props.indent_left {
        out.push_str(&format!(" marL=\"{}\"", indent.emu()));
    }
    if let Some(indent) = props.indent_right {
        out.push_str(&format!(" marR=\"{}\"", indent.emu()));
    }
    if level > 0 {
        out.push_str(&format!(" lvl=\"{level}\""));
    }
    // DrawingML has one `indent`: negative is hanging, positive first-line.
    if let Some(hanging) = props.indent_hanging {
        out.push_str(&format!(" indent=\"{}\"", -hanging.emu()));
    } else if let Some(first) = props.indent_first_line {
        out.push_str(&format!(" indent=\"{}\"", first.emu()));
    }
    if let Some(align) = props.align {
        out.push_str(&format!(" algn=\"{}\"", align_keyword(align)));
    }
    let mut children = String::new();

    if let Some(height) = props.line_height {
        children.push_str("<a:lnSpc>");
        match height {
            // Thousandths of a line in the IR, thousandths of a percent here.
            LineHeight::Multiple(per_mille) => {
                children.push_str(&format!("<a:spcPct val=\"{}\"/>", per_mille * 100));
            }
            LineHeight::Exact(length) | LineHeight::AtLeast(length) => {
                children.push_str(&format!(
                    "<a:spcPts val=\"{}\"/>",
                    hundredths_of_a_point(length)
                ));
            }
        }
        children.push_str("</a:lnSpc>");
    }
    if let Some(space) = props.space_before {
        children.push_str(&format!(
            "<a:spcBef><a:spcPts val=\"{}\"/></a:spcBef>",
            hundredths_of_a_point(space)
        ));
    }
    if let Some(space) = props.space_after {
        children.push_str(&format!(
            "<a:spcAft><a:spcPts val=\"{}\"/></a:spcAft>",
            hundredths_of_a_point(space)
        ));
    }
    if let Some(bullet) = bullet {
        children.push_str(bullet);
    }
    if children.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        out.push_str(&children);
        out.push_str("</a:pPr>");
    }
}

/// The bullet element the paragraph needs, given what its shape inherits.
///
/// Nothing at all is the common case, and it is not an omission: a bulleted
/// item inside a body placeholder that states its bullet would be stating what
/// the master already said, which is the direct property the reader would then
/// have to subtract again.
fn bullet_element(entry: &Flat<'_>, bulleted: bool) -> Option<&'static str> {
    match entry.item {
        Some((true, _)) => Some("<a:buAutoNum type=\"arabicPeriod\"/>"),
        // A nested item is an item by its `lvl` alone, whatever it inherits.
        Some((false, level)) => (!bulleted && level == 0).then_some("<a:buChar char=\"•\"/>"),
        // An empty paragraph never carried a bullet on the way in, so it needs
        // nothing to come back as a paragraph.
        None if bulleted && !entry.paragraph.is_empty() => Some("<a:buNone/>"),
        None => None,
    }
}

fn write_inline(
    inline: &Inline,
    base: &RunProps,
    lang: Option<&str>,
    link: Option<&str>,
    links: &mut dyn Links,
    out: &mut String,
) -> Result<(), Unsupported> {
    match inline {
        Inline::Text(text) => {
            out.push_str("<a:r>");
            write_rpr(base, lang, link, out);
            out.push_str("<a:t>");
            out.push_str(&esc_text(text));
            out.push_str("</a:t></a:r>");
            Ok(())
        }
        Inline::Styled { content, props } => {
            let merged = merge(base, props);
            for inline in content {
                write_inline(inline, &merged, lang, link, links, out)?;
            }
            Ok(())
        }
        Inline::Break(BreakKind::Line) => {
            out.push_str("<a:br/>");
            Ok(())
        }
        Inline::Break(other) => Err(format!("a {other:?} break")),
        // A hyperlink is not an element of its own here: it is `a:hlinkClick`
        // on the properties of every run it covers, which is why the id travels
        // down rather than wrapping anything. A nested link wins over the one
        // around it, exactly as the reader reads it back.
        Inline::Link {
            target,
            content,
            props,
        } => {
            let Some(id) = links.hyperlink(target) else {
                return Err(format!("the hyperlink to `{target}`"));
            };
            let merged = merge(base, props);
            for inline in content {
                write_inline(inline, &merged, lang, Some(&id), links, out)?;
            }
            Ok(())
        }
        // `a:fld` is identified by a GUID the IR does not record, and inventing
        // one changes the field's identity in the file. Putting a field back
        // needs the *model* to keep that GUID, which is a `docsai-model` change
        // no increment of Phase 15 takes: 15-E, which the write-side table of
        // 15-B pointed at, rewrites the deck's own parts and not its runs.
        Inline::Field { instruction, .. } => Err(format!("the `{instruction}` field")),
        Inline::Image(_) => Err("an inline image".into()),
        Inline::Footnote(_) => Err("a footnote".into()),
        Inline::Raw(raw) => Err(format!("the raw fragment `{}`", raw.id.as_str())),
    }
}

/// `self` over `base`, the same layering the cascade uses.
fn merge(base: &RunProps, over: &RunProps) -> RunProps {
    RunProps {
        style: over.style.clone().or_else(|| base.style.clone()),
        direct: over.direct.over(&base.direct),
    }
}

fn write_rpr(props: &RunProps, lang: Option<&str>, link: Option<&str>, out: &mut String) {
    if props.direct.is_empty() && lang.is_none() && link.is_none() {
        return;
    }
    out.push_str("<a:rPr");
    // The editing language is not an IR property — it is spell-check metadata —
    // but it is on every run PowerPoint has ever written, and a rewritten slide
    // that dropped it would quietly change what the file says about itself. The
    // slide's own language travels through, and a run written from nothing gets
    // the one its neighbours use.
    if let Some(lang) = lang {
        out.push_str(&format!(" lang=\"{}\"", esc_attr(lang)));
    }
    write_font_attrs(&props.direct, out);
    write_font_children(&props.direct, link, out, "a:rPr");
}

/// The attribute half of `CT_TextCharacterProperties`, and the opening `>` or
/// `/>` is left to [`write_font_children`].
fn write_font_attrs(font: &FontProps, out: &mut String) {
    if let Some(size) = font.size {
        out.push_str(&format!(" sz=\"{}\"", hundredths_of_a_point(size)));
    }
    if let Some(bold) = font.bold {
        out.push_str(&format!(" b=\"{}\"", bit(bold)));
    }
    if let Some(italic) = font.italic {
        out.push_str(&format!(" i=\"{}\"", bit(italic)));
    }
    if let Some(underline) = font.underline {
        out.push_str(&format!(" u=\"{}\"", underline_keyword(underline)));
    }
    if let Some(strike) = font.strike {
        out.push_str(&format!(
            " strike=\"{}\"",
            if strike { "sngStrike" } else { "noStrike" }
        ));
    }
    if let Some(caps) = caps_keyword(font) {
        out.push_str(&format!(" cap=\"{caps}\""));
    }
    if let Some(vert) = font.vert_align {
        out.push_str(&format!(" baseline=\"{}\"", baseline(vert)));
    }
}

/// The child half, in schema order: fill, highlight, the typeface, the link.
fn write_font_children(font: &FontProps, link: Option<&str>, out: &mut String, tag: &str) {
    let has_children =
        font.color.is_some() || font.highlight.is_some() || font.name.is_some() || link.is_some();
    if !has_children {
        out.push_str("/>");
        return;
    }
    out.push('>');
    if let Some(colour) = &font.color {
        out.push_str(&format!(
            "<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>",
            hex(colour)
        ));
    }
    if let Some(colour) = &font.highlight {
        out.push_str(&format!(
            "<a:highlight><a:srgbClr val=\"{}\"/></a:highlight>",
            hex(colour)
        ));
    }
    if let Some(name) = &font.name {
        // The theme reference the run may have carried (`+mj-lt`) resolved to
        // this name on the way in, and what goes back is the name. A run that
        // followed the theme by reference now follows it by value: changing the
        // theme no longer moves it. It only happens where the run overrode what
        // it inherited, which is the case where the reference was already not
        // doing the work.
        out.push_str(&format!("<a:latin typeface=\"{}\"/>", esc_attr(name)));
    }
    if let Some(id) = link {
        // `CT_TextCharacterProperties` puts the link after the typeface and
        // before `rtl`; the id names a relationship of the part being written.
        out.push_str(&format!("<a:hlinkClick r:id=\"{}\"/>", esc_attr(id)));
    }
    out.push_str(&format!("</{tag}>"));
}

/// `cap`, or `None` when the pair cannot be stated as one keyword.
///
/// The format has one attribute for two IR fields, and `cap="none"` says both
/// are off. A delta that turns one off and leaves the other unstated has no
/// DrawingML form, so it is refused rather than rounded to the nearest lie.
fn caps_keyword(font: &FontProps) -> Option<&'static str> {
    match (font.caps, font.small_caps) {
        (Some(true), _) => Some("all"),
        (_, Some(true)) => Some("small"),
        (Some(false), Some(false)) => Some("none"),
        _ => None,
    }
}

fn hundredths_of_a_point(length: docsai_model::units::Length) -> i64 {
    (length.pt() * 100.0).round() as i64
}

fn bit(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
}

fn baseline(vert: VertAlign) -> &'static str {
    match vert {
        VertAlign::Superscript => "30000",
        VertAlign::Subscript => "-25000",
        VertAlign::Baseline => "0",
    }
}

fn align_keyword(align: Align) -> &'static str {
    match align {
        Align::Left => "l",
        Align::Center => "ctr",
        Align::Right => "r",
        Align::Justify => "just",
    }
}

fn underline_keyword(underline: Underline) -> &'static str {
    match underline {
        Underline::None => "none",
        Underline::Single => "sng",
        Underline::Double => "dbl",
        Underline::Thick => "heavy",
        Underline::Dotted => "dotted",
        Underline::Dashed => "dash",
        Underline::Wave => "wavy",
    }
}

/// `#rrggbb` back to the six digits `a:srgbClr` wants.
fn hex(colour: &str) -> String {
    colour
        .trim_start_matches('#')
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase()
}

fn esc_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn esc_attr(s: &str) -> String {
    esc_text(s).replace('"', "&quot;")
}
