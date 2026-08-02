//! ODF `style:text-properties` / `style:paragraph-properties` → IR deltas.

use docsai_model::style::{Align, FontProps, LineHeight, ParaProps, Underline, VertAlign};
use docsai_model::units::Length;

use crate::length::parse_length;
use crate::xml::Element;

/// Reads `style:text-properties` into character-level deltas.
pub fn text_properties(props: &Element) -> FontProps {
    let mut font = FontProps::default();

    if let Some(name) = props
        .attr("font-name")
        .or_else(|| props.attr_qualified("style:font-name"))
        .or_else(|| props.attr("font-name-asian"))
        .or_else(|| props.attr("font-name-complex"))
    {
        font.name = Some(name.to_string());
    }
    if let Some(size) = props
        .attr("font-size")
        .or_else(|| props.attr_qualified("fo:font-size"))
        .and_then(parse_length)
    {
        font.size = Some(size);
    }
    if let Some(w) = props
        .attr("font-weight")
        .or_else(|| props.attr_qualified("fo:font-weight"))
    {
        font.bold = Some(matches!(w, "bold" | "700" | "800" | "900"));
    }
    if let Some(s) = props
        .attr("font-style")
        .or_else(|| props.attr_qualified("fo:font-style"))
    {
        font.italic = Some(s == "italic" || s == "oblique");
    }
    if let Some(line) = props
        .attr("text-line-through-style")
        .or_else(|| props.attr_qualified("style:text-line-through-style"))
    {
        font.strike = Some(line != "none");
    }
    if let Some(u) = props
        .attr("text-underline-style")
        .or_else(|| props.attr_qualified("style:text-underline-style"))
    {
        if u == "none" {
            font.underline = Some(Underline::None);
        } else {
            let kind = props
                .attr("text-underline-type")
                .or_else(|| props.attr_qualified("style:text-underline-type"))
                .unwrap_or("single");
            font.underline = Some(match kind {
                "double" => Underline::Double,
                _ if u == "dotted" || u == "dash" || u == "wave" => match u {
                    "dotted" => Underline::Dotted,
                    "dash" | "long-dash" | "dot-dash" => Underline::Dashed,
                    "wave" => Underline::Wave,
                    _ => Underline::Single,
                },
                _ => Underline::Single,
            });
        }
    }
    if let Some(color) = props
        .attr("color")
        .or_else(|| props.attr_qualified("fo:color"))
        .and_then(hex_color)
    {
        font.color = Some(color);
    }
    if let Some(bg) = props
        .attr("background-color")
        .or_else(|| props.attr_qualified("fo:background-color"))
        .and_then(hex_color)
    {
        font.highlight = Some(bg);
    }
    if let Some(pos) = props
        .attr("text-position")
        .or_else(|| props.attr_qualified("style:text-position"))
    {
        // e.g. "super 58%" or "sub 58%" or "0% 100%"
        let lower = pos.to_ascii_lowercase();
        if lower.starts_with("super") {
            font.vert_align = Some(VertAlign::Superscript);
        } else if lower.starts_with("sub") {
            font.vert_align = Some(VertAlign::Subscript);
        } else if let Some(first) = lower.split_whitespace().next() {
            if let Ok(pct) = first.trim_end_matches('%').parse::<f64>() {
                if pct > 0.0 {
                    font.vert_align = Some(VertAlign::Superscript);
                } else if pct < 0.0 {
                    font.vert_align = Some(VertAlign::Subscript);
                } else {
                    font.vert_align = Some(VertAlign::Baseline);
                }
            }
        }
    }
    if let Some(variant) = props
        .attr("font-variant")
        .or_else(|| props.attr_qualified("fo:font-variant"))
    {
        if variant == "small-caps" {
            font.small_caps = Some(true);
        }
    }
    if let Some(transform) = props
        .attr("text-transform")
        .or_else(|| props.attr_qualified("fo:text-transform"))
    {
        if transform == "uppercase" {
            font.caps = Some(true);
        }
    }
    font
}

/// Reads `style:paragraph-properties` into paragraph-level deltas.
pub fn paragraph_properties(props: &Element) -> ParaProps {
    let mut para = ParaProps::default();

    if let Some(align) = props
        .attr("text-align")
        .or_else(|| props.attr_qualified("fo:text-align"))
    {
        para.align = Some(match align {
            "center" => Align::Center,
            "end" | "right" => Align::Right,
            "justify" => Align::Justify,
            _ => Align::Left,
        });
    }
    if let Some(l) = props
        .attr("margin-left")
        .or_else(|| props.attr_qualified("fo:margin-left"))
        .and_then(parse_length)
    {
        para.indent_left = Some(l);
    }
    if let Some(r) = props
        .attr("margin-right")
        .or_else(|| props.attr_qualified("fo:margin-right"))
        .and_then(parse_length)
    {
        para.indent_right = Some(r);
    }
    if let Some(t) = props
        .attr("text-indent")
        .or_else(|| props.attr_qualified("fo:text-indent"))
        .and_then(parse_length)
    {
        if t.emu() < 0 {
            para.indent_hanging = Some(Length::from_emu(-t.emu()));
        } else {
            para.indent_first_line = Some(t);
        }
    }
    if let Some(b) = props
        .attr("margin-top")
        .or_else(|| props.attr_qualified("fo:margin-top"))
        .and_then(parse_length)
    {
        para.space_before = Some(b);
    }
    if let Some(a) = props
        .attr("margin-bottom")
        .or_else(|| props.attr_qualified("fo:margin-bottom"))
        .and_then(parse_length)
    {
        para.space_after = Some(a);
    }
    if let Some(lh) = props
        .attr("line-height")
        .or_else(|| props.attr_qualified("fo:line-height"))
    {
        if let Some(pct) = lh
            .strip_suffix('%')
            .and_then(|s| s.trim().parse::<f64>().ok())
        {
            // 100% → 1000 thousandths.
            para.line_height = Some(LineHeight::Multiple((pct * 10.0).round() as i32));
        } else if let Some(len) = parse_length(lh) {
            para.line_height = Some(LineHeight::Exact(len));
        }
    }
    if let Some(keep) = props
        .attr("keep-with-next")
        .or_else(|| props.attr_qualified("fo:keep-with-next"))
    {
        para.keep_with_next = Some(keep == "always");
    }
    if let Some(br) = props
        .attr("break-before")
        .or_else(|| props.attr_qualified("fo:break-before"))
    {
        para.page_break_before = Some(br == "page");
    }
    if let Some(bg) = props
        .attr("background-color")
        .or_else(|| props.attr_qualified("fo:background-color"))
        .and_then(hex_color)
    {
        para.background = Some(bg);
    }
    para
}

/// Normalises a colour to `#RRGGBB`.
pub fn hex_color(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("transparent") {
        return None;
    }
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("#{}", hex.to_ascii_uppercase()));
    }
    if hex.len() == 3 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        let b = hex.as_bytes();
        return Some(
            format!(
                "#{0}{0}{1}{1}{2}{2}",
                b[0] as char, b[1] as char, b[2] as char
            )
            .to_ascii_uppercase(),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml::Element;

    #[test]
    fn reads_text_and_paragraph_properties() {
        let xml = r##"
        <style:style xmlns:style="urn:s" xmlns:fo="urn:fo">
          <style:text-properties fo:font-name="Liberation Sans" fo:font-size="12pt"
            fo:font-weight="bold" fo:font-style="italic" fo:color="#FF0000"
            style:text-underline-style="solid" style:text-line-through-style="solid"/>
          <style:paragraph-properties fo:text-align="center" fo:margin-top="0.5cm"
            fo:margin-bottom="0.2cm" fo:line-height="150%"/>
        </style:style>"##;
        let root = Element::parse("t.xml", xml.as_bytes()).unwrap();
        let font = text_properties(root.child("text-properties").unwrap());
        assert_eq!(font.name.as_deref(), Some("Liberation Sans"));
        assert_eq!(font.size, Some(Length::from_pt(12.0)));
        assert_eq!(font.bold, Some(true));
        assert_eq!(font.italic, Some(true));
        assert_eq!(font.strike, Some(true));
        assert_eq!(font.underline, Some(Underline::Single));
        assert_eq!(font.color.as_deref(), Some("#FF0000"));
        let para = paragraph_properties(root.child("paragraph-properties").unwrap());
        assert_eq!(para.align, Some(Align::Center));
        assert_eq!(para.space_before, Some(Length::from_cm(0.5)));
        assert_eq!(para.line_height, Some(LineHeight::Multiple(1500)));
    }
}
