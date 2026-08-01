//! How lengths and numbers appear in DocMark.
//!
//! Centralised so that every attribute renders a length the same way, which
//! the idempotence test of Fase 2 will depend on.

use docsai_model::units::{trim_float, Length};

/// A length as DocMark writes it: whole pixels when it lands exactly on one at
/// 96 dpi, centimetres otherwise.
///
/// ```
/// use docsai_docmark::units::len;
/// use docsai_model::units::Length;
/// assert_eq!(len(Length::from_px(450.0)), "450px");
/// assert_eq!(len(Length::from_cm(3.5)), "3.5cm");
/// ```
pub fn len(value: Length) -> String {
    value.to_string()
}

/// A length as points, for typographic measures (font sizes, spacing).
pub fn pt(value: Length) -> String {
    format!("{}pt", trim_float(value.pt(), 2))
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
    fn lengths_prefer_exact_pixels() {
        assert_eq!(len(Length::from_px(120.0)), "120px");
        assert_eq!(len(Length::from_cm(3.5)), "3.5cm");
        assert_eq!(len(Length::ZERO), "0px");
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
