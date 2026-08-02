//! ODF length parsing (`2.5cm`, `12pt`, `0.5in`, …) → IR [`Length`].

use docsai_model::units::Length;

/// Parses an ODF length attribute (`fo:font-size`, `svg:width`, …).
///
/// Accepts the units LibreOffice and the ODF spec emit: `cm`, `mm`, `in`, `pt`,
/// `pc`, `px`. Bare numbers are treated as centimetres (ODF default for many
/// layout attributes). Returns `None` when the value is empty or unparsable.
pub fn parse_length(raw: &str) -> Option<Length> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (num, unit) = split_number_unit(raw)?;
    let value: f64 = num.parse().ok()?;
    Some(match unit {
        "" | "cm" => Length::from_cm(value),
        "mm" => Length::from_mm(value),
        "in" | "inch" => Length::from_inch(value),
        "pt" => Length::from_pt(value),
        "pc" => Length::from_pt(value * 12.0),
        "px" => Length::from_px(value),
        // Percentage and relative units cannot become absolute EMU alone.
        _ => return None,
    })
}

/// Formats a length for ODF attributes, preferring centimetres with enough
/// precision for a clean round-trip through EMU.
pub fn format_cm(length: Length) -> String {
    let cm = length.cm();
    // Trim trailing zeros while keeping enough digits for EMU fidelity.
    let s = format!("{cm:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0cm".into()
    } else {
        format!("{s}cm")
    }
}

/// Formats a length in points (font sizes).
pub fn format_pt(length: Length) -> String {
    let pt = length.pt();
    let s = format!("{pt:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0pt".into()
    } else {
        format!("{s}pt")
    }
}

fn split_number_unit(raw: &str) -> Option<(&str, &str)> {
    let bytes = raw.as_bytes();
    let mut i = 0;
    if bytes.first() == Some(&b'+') || bytes.first() == Some(&b'-') {
        i = 1;
    }
    let start_digits = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    if i == start_digits {
        return None;
    }
    let num = &raw[..i];
    let unit = raw[i..].trim().to_ascii_lowercase();
    // Leak-free: return unit as owned via a small match on known suffixes.
    // We re-slice from raw with the same ASCII length.
    let unit_len = unit.len();
    let unit_slice = &raw[i..i + unit_len];
    Some((num, unit_slice))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_odf_units() {
        assert_eq!(parse_length("2.54cm"), Some(Length::from_inch(1.0)));
        assert_eq!(parse_length("72pt"), Some(Length::from_inch(1.0)));
        assert_eq!(parse_length("1in"), Some(Length::from_inch(1.0)));
        assert_eq!(parse_length("10mm"), Some(Length::from_cm(1.0)));
        assert!(parse_length("50%").is_none());
        assert!(parse_length("").is_none());
    }

    #[test]
    fn formats_round_trip_friendly_cm() {
        let l = Length::from_cm(2.5);
        assert_eq!(format_cm(l), "2.5cm");
        assert_eq!(parse_length(&format_cm(l)), Some(l));
    }
}
