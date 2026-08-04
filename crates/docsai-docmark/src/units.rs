//! How lengths and numbers appear in DocMark.
//!
//! Centralised so that every attribute renders a length the same way, which
//! the idempotence test of Phase 2 depends on.
//!
//! A length is written in the unit of **what it measures** (spec §2): points
//! for layout and typography, pixels for bitmaps and drawing offsets. Both are
//! lossless — the unit is only used when it is exact at the configured
//! precision, and raw `emu` is always there as the escape hatch — so the
//! choice is about reading, never about accuracy.

use docsai_model::units::{trim_float, Length, LengthStyle, DEFAULT_PRECISION};

/// A layout length: an indent, a margin, a column width. Points first, because
/// that is the unit a text document is authored in.
///
/// ```
/// use docsai_docmark::units::len;
/// use docsai_model::units::Length;
/// assert_eq!(len(Length::from_twips(720), 2), "36pt");
/// assert_eq!(len(Length::from_cm(3.5), 2), "3.5cm");
/// assert_eq!(len(Length::ZERO, 2), "0");
/// ```
pub fn len(value: Length, precision: u8) -> String {
    value.render(LengthStyle::Typographic, precision)
}

/// A drawing length: an image size, an anchor offset. Pixels first, because a
/// bitmap has a natural size in pixels.
///
/// ```
/// use docsai_docmark::units::geometry;
/// use docsai_model::units::Length;
/// assert_eq!(geometry(Length::from_px(120.0), 2), "120px");
/// ```
pub fn geometry(value: Length, precision: u8) -> String {
    value.render(LengthStyle::Geometric, precision)
}

/// A length as points, for typographic measures (font sizes, spacing).
///
/// Unlike [`len`] this one never changes unit: a font size is points by
/// definition, and a `size: 0.39cm` would be unreadable in every editor that
/// shows font sizes.
pub fn pt(value: Length) -> String {
    format!("{}pt", trim_float(value.pt(), DEFAULT_PRECISION as usize))
}

/// A percentage with at most two decimals.
pub fn percent(value: f32) -> String {
    format!("{}%", trim_float(value as f64, 2))
}

/// A plain number with at most two decimals.
pub fn number(value: f32) -> String {
    trim_float(value as f64, 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_lengths_are_points() {
        // Word writes layout in twips, and a twip is exactly 0.05pt.
        assert_eq!(len(Length::from_twips(720), 2), "36pt");
        assert_eq!(len(Length::from_twips(1417), 2), "70.85pt");
        assert_eq!(len(Length::from_cm(3.5), 2), "3.5cm");
    }

    #[test]
    fn drawing_lengths_are_pixels() {
        assert_eq!(geometry(Length::from_px(120.0), 2), "120px");
        assert_eq!(geometry(Length::from_cm(3.5), 2), "3.5cm");
    }

    #[test]
    fn zero_names_no_unit() {
        assert_eq!(len(Length::ZERO, 2), "0");
        assert_eq!(geometry(Length::ZERO, 2), "0");
    }

    #[test]
    fn precision_buys_readable_units_never_accuracy() {
        let odd = Length::from_cm(1.251);
        assert_eq!(len(odd, 2), "450360emu");
        assert_eq!(len(odd, 3), "1.251cm");
        assert_eq!(Length::parse(&len(odd, 2)), Some(odd));
        assert_eq!(Length::parse(&len(odd, 3)), Some(odd));
    }

    #[test]
    fn typographic_measures_are_points() {
        assert_eq!(pt(Length::from_half_points(22)), "11pt");
        assert_eq!(pt(Length::from_twips(240)), "12pt");
    }

    #[test]
    fn percentages_and_numbers_drop_trailing_zeros() {
        assert_eq!(percent(10.0), "10%");
        assert_eq!(percent(12.5), "12.5%");
        assert_eq!(number(45.0), "45");
        assert_eq!(number(-0.0), "0");
    }
}
