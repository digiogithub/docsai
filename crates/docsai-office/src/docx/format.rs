//! `w:rPr` and `w:pPr` → the IR's delta types.
//!
//! Nothing here flattens: a property absent from the XML stays `None` in the
//! IR so that the style it inherits from keeps supplying it.

use docsai_model::style::{Align, FontProps, LineHeight, ParaProps, Underline, VertAlign};
use docsai_model::units::Length;

use crate::xml::Element;

/// Reads a `w:rPr` into character-level deltas.
pub fn run_props(rpr: &Element) -> FontProps {
    let mut font = FontProps::default();

    if let Some(fonts) = rpr.child("rFonts") {
        font.name = fonts
            .attr("ascii")
            .or_else(|| fonts.attr("hAnsi"))
            .or_else(|| fonts.attr("cs"))
            .map(str::to_string);
    }
    if let Some(sz) = rpr.child("sz").and_then(|e| e.attr_i64("val")) {
        font.size = Some(Length::from_half_points(sz));
    }
    if let Some(b) = rpr.child("b") {
        font.bold = Some(b.ooxml_flag());
    }
    if let Some(i) = rpr.child("i") {
        font.italic = Some(i.ooxml_flag());
    }
    if let Some(s) = rpr.child("strike") {
        font.strike = Some(s.ooxml_flag());
    }
    if let Some(u) = rpr.child("u") {
        font.underline = Some(underline(u.attr("val").unwrap_or("single")));
    }
    if let Some(color) = rpr.child("color").and_then(|e| e.attr("val")) {
        // `auto` means "let the renderer decide": not a colour, so not a delta.
        if let Some(hex) = hex_color(color) {
            font.color = Some(hex);
        }
    }
    if let Some(h) = rpr.child("highlight").and_then(|e| e.attr("val")) {
        if h != "none" {
            font.highlight = Some(h.to_string());
        }
    }
    if let Some(v) = rpr.child("vertAlign").and_then(|e| e.attr("val")) {
        font.vert_align = Some(match v {
            "superscript" => VertAlign::Superscript,
            "subscript" => VertAlign::Subscript,
            _ => VertAlign::Baseline,
        });
    }
    if let Some(c) = rpr.child("smallCaps") {
        font.small_caps = Some(c.ooxml_flag());
    }
    if let Some(c) = rpr.child("caps") {
        font.caps = Some(c.ooxml_flag());
    }
    font
}

/// Reads a `w:pPr` into paragraph-level deltas.
///
/// `w:pStyle` and `w:numPr` are *not* read here: they are references, not
/// formatting, and the body reader handles them.
pub fn paragraph_props(ppr: &Element) -> ParaProps {
    let mut para = ParaProps::default();

    if let Some(jc) = ppr.child("jc").and_then(|e| e.attr("val")) {
        para.align = Some(match jc {
            "center" => Align::Center,
            "right" | "end" => Align::Right,
            "both" | "distribute" | "justify" => Align::Justify,
            _ => Align::Left,
        });
    }
    if let Some(ind) = ppr.child("ind") {
        para.indent_left = twips_attr(ind, &["left", "start"]);
        para.indent_right = twips_attr(ind, &["right", "end"]);
        para.indent_first_line = twips_attr(ind, &["firstLine"]);
        para.indent_hanging = twips_attr(ind, &["hanging"]);
    }
    if let Some(spacing) = ppr.child("spacing") {
        para.space_before = twips_attr(spacing, &["before"]);
        para.space_after = twips_attr(spacing, &["after"]);
        if let Some(line) = spacing.attr_i64("line") {
            para.line_height = Some(match spacing.attr("lineRule") {
                // `auto` counts in 240ths of a line; the IR stores 1000ths.
                Some("exact") => LineHeight::Exact(Length::from_twips(line)),
                Some("atLeast") => LineHeight::AtLeast(Length::from_twips(line)),
                // Round half-up so writer↔reader stays stable for common multiples.
                _ => LineHeight::Multiple(((line * 1000 + 120) / 240) as i32),
            });
        }
    }
    if let Some(k) = ppr.child("keepNext") {
        para.keep_with_next = Some(k.ooxml_flag());
    }
    if let Some(b) = ppr.child("pageBreakBefore") {
        para.page_break_before = Some(b.ooxml_flag());
    }
    if let Some(shd) = ppr.child("shd") {
        if let Some(fill) = shd.attr("fill").and_then(hex_color) {
            para.background = Some(fill);
        }
    }
    if let Some(lvl) = ppr.child("outlineLvl").and_then(|e| e.attr_i64("val")) {
        if (0..=8).contains(&lvl) {
            para.outline_level = Some(lvl as u8);
        }
    }
    para
}

fn twips_attr(element: &Element, names: &[&str]) -> Option<Length> {
    names
        .iter()
        .find_map(|n| element.attr_i64(n))
        .map(Length::from_twips)
}

/// Normalises an OOXML colour to `#RRGGBB`, or `None` for `auto`/garbage.
pub fn hex_color(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches('#');
    if value.eq_ignore_ascii_case("auto") {
        return None;
    }
    // Word writes both RRGGBB and (in DrawingML themes) AARRGGBB.
    let hex = match value.len() {
        6 => value,
        8 => &value[2..],
        _ => return None,
    };
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", hex.to_ascii_uppercase()))
}

fn underline(value: &str) -> Underline {
    match value {
        "none" => Underline::None,
        "double" => Underline::Double,
        "thick" => Underline::Thick,
        "dotted" | "dottedHeavy" => Underline::Dotted,
        "dash" | "dashed" | "dashLong" | "dashDotHeavy" => Underline::Dashed,
        "wave" | "wavyHeavy" | "wavyDouble" => Underline::Wave,
        _ => Underline::Single,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(xml: &str) -> Element {
        Element::parse("t.xml", xml.as_bytes()).unwrap()
    }

    #[test]
    fn reads_the_inline_formatting_of_the_spec_table() {
        let rpr = parse(
            r#"<w:rPr xmlns:w="urn:w">
                 <w:rFonts w:ascii="Arial" w:hAnsi="Arial"/>
                 <w:b/><w:i/><w:strike/><w:u w:val="double"/>
                 <w:color w:val="FF0000"/><w:highlight w:val="yellow"/>
                 <w:sz w:val="28"/><w:vertAlign w:val="superscript"/>
               </w:rPr>"#,
        );
        let font = run_props(&rpr);
        assert_eq!(font.name.as_deref(), Some("Arial"));
        assert_eq!(font.size, Some(Length::from_pt(14.0)));
        assert_eq!(font.bold, Some(true));
        assert_eq!(font.italic, Some(true));
        assert_eq!(font.strike, Some(true));
        assert_eq!(font.underline, Some(Underline::Double));
        assert_eq!(font.color.as_deref(), Some("#FF0000"));
        assert_eq!(font.highlight.as_deref(), Some("yellow"));
        assert_eq!(font.vert_align, Some(VertAlign::Superscript));
    }

    #[test]
    fn explicit_off_is_a_delta_not_an_absence() {
        let rpr = parse(r#"<w:rPr xmlns:w="urn:w"><w:b w:val="0"/></w:rPr>"#);
        assert_eq!(
            run_props(&rpr).bold,
            Some(false),
            "turning bold off must override the style"
        );
        let empty = parse(r#"<w:rPr xmlns:w="urn:w"/>"#);
        assert_eq!(run_props(&empty).bold, None, "absent means inherit");
    }

    #[test]
    fn auto_colour_is_not_a_colour() {
        let rpr = parse(r#"<w:rPr xmlns:w="urn:w"><w:color w:val="auto"/></w:rPr>"#);
        assert_eq!(run_props(&rpr).color, None);
        assert_eq!(hex_color("2e74b5").as_deref(), Some("#2E74B5"));
        assert_eq!(hex_color("FF2E74B5").as_deref(), Some("#2E74B5"));
        assert_eq!(hex_color("zzz"), None);
    }

    #[test]
    fn reads_paragraph_geometry() {
        let ppr = parse(
            r#"<w:pPr xmlns:w="urn:w">
                 <w:jc w:val="center"/>
                 <w:ind w:left="720" w:firstLine="360" w:right="240"/>
                 <w:spacing w:before="240" w:after="120" w:line="360" w:lineRule="auto"/>
                 <w:keepNext/><w:shd w:val="clear" w:fill="F2F2F2"/>
                 <w:outlineLvl w:val="1"/>
               </w:pPr>"#,
        );
        let para = paragraph_props(&ppr);
        assert_eq!(para.align, Some(Align::Center));
        assert_eq!(para.indent_left, Some(Length::from_twips(720)));
        assert_eq!(para.indent_first_line, Some(Length::from_twips(360)));
        assert_eq!(para.indent_right, Some(Length::from_twips(240)));
        assert_eq!(para.space_before, Some(Length::from_twips(240)));
        assert_eq!(para.line_height, Some(LineHeight::Multiple(1500)));
        assert_eq!(para.keep_with_next, Some(true));
        assert_eq!(para.background.as_deref(), Some("#F2F2F2"));
        assert_eq!(para.outline_level, Some(1));
    }

    #[test]
    fn exact_line_spacing_keeps_its_unit() {
        let ppr =
            parse(r#"<w:pPr xmlns:w="urn:w"><w:spacing w:line="240" w:lineRule="exact"/></w:pPr>"#);
        assert_eq!(
            paragraph_props(&ppr).line_height,
            Some(LineHeight::Exact(Length::from_twips(240)))
        );
    }

    #[test]
    fn nonsense_values_are_ignored_rather_than_fatal() {
        let ppr = parse(
            r#"<w:pPr xmlns:w="urn:w"><w:outlineLvl w:val="99"/><w:ind w:left="abc"/></w:pPr>"#,
        );
        let para = paragraph_props(&ppr);
        assert_eq!(para.outline_level, None);
        assert_eq!(para.indent_left, None);
    }
}
