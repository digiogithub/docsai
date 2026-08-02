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
///
/// Points are what a typographer reads, but never at the cost of exactness: a
/// value that is not a whole hundredth of a point falls back to [`len`], whose
/// last resort is raw EMU. Rounding here would move spacing and font sizes a
/// little on every round-trip, which is the defect the corpus found in the
/// length format during Fase 1 — the same trap, one function along.
///
/// ```
/// use docsai_docmark::units::pt;
/// use docsai_model::units::Length;
/// assert_eq!(pt(Length::from_pt(8.0)), "8pt");
/// assert_eq!(pt(Length::from_emu(-1)), "-1emu");
/// ```
pub fn pt(value: Length) -> String {
    /// EMU in a hundredth of a point: the precision `trim_float(.., 2)` keeps.
    const EMU_PER_CENTI_PT: i64 = docsai_model::units::EMU_PER_POINT / 100;
    if value.emu() % EMU_PER_CENTI_PT == 0 {
        format!("{}pt", trim_float(value.pt(), 2))
    } else {
        len(value)
    }
}

/// A percentage with at most two decimals.
pub fn percent(value: f32) -> String {
    format!("{}%", trim_float(value as f64, 2))
}

/// A plain number with at most two decimals.
pub fn number(value: f32) -> String {
    trim_float(value as f64, 2)
}

/// Reads a length back, in any of the units the serialiser may have chosen.
///
/// A bare number is read as pixels, which is what a hand editor most likely
/// means; everything docsai writes carries its unit.
///
/// ```
/// use docsai_docmark::units::parse_len;
/// use docsai_model::units::Length;
/// assert_eq!(parse_len("450px"), Some(Length::from_px(450.0)));
/// assert_eq!(parse_len("70.85pt"), Some(Length::from_twips(1417)));
/// assert_eq!(parse_len("no"), None);
/// ```
pub fn parse_len(text: &str) -> Option<Length> {
    let text = text.trim();
    let split = text
        .find(|c: char| c.is_ascii_alphabetic())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let value: f64 = number.trim().parse().ok()?;
    match unit.trim() {
        "px" | "" => Some(Length::from_px(value)),
        "cm" => Some(Length::from_cm(value)),
        "mm" => Some(Length::from_mm(value)),
        "pt" => Some(Length::from_pt(value)),
        "in" => Some(Length::from_inch(value)),
        // EMU are integers by definition; a fractional one is not a length.
        "emu" if value.fract() == 0.0 => Some(Length::from_emu(value as i64)),
        _ => None,
    }
}

/// Reads a percentage, with or without its `%`.
pub fn parse_percent(text: &str) -> Option<f32> {
    parse_number(text.trim().trim_end_matches('%'))
}

/// Reads a plain number.
pub fn parse_number(text: &str) -> Option<f32> {
    text.trim().parse().ok()
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
    fn typographic_measures_never_round_a_value_away() {
        // Anything a hundredth of a point cannot express falls back rather
        // than being flattened to a nearby number.
        assert_eq!(pt(Length::from_emu(-1)), "-1emu");
        assert_eq!(pt(Length::from_emu(1)), "1emu");
        assert_eq!(pt(Length::from_emu(127)), "0.01pt");
        for value in [
            Length::from_emu(-1),
            Length::from_emu(1),
            Length::from_emu(127),
            Length::from_pt(8.0),
            Length::from_twips(1417),
            Length::ZERO,
        ] {
            let text = pt(value);
            assert_eq!(parse_len(&text), Some(value), "`{text}` did not round-trip");
        }
    }

    #[test]
    fn percentages_and_numbers_drop_trailing_zeros() {
        assert_eq!(percent(10.0), "10%");
        assert_eq!(percent(12.5), "12.5%");
        assert_eq!(number(45.0), "45");
        assert_eq!(number(-0.0), "0");
    }

    #[test]
    fn every_unit_the_serialiser_can_write_reads_back_exactly() {
        // The whole point of the lossless `Display` of `Length`: whatever unit
        // it picked, parsing it must give the very same EMU back.
        let samples = [
            Length::from_twips(1417),
            Length::from_twips(2500),
            Length::from_cm(3.0),
            Length::from_px(120.0),
            Length::from_pt(11.0),
            Length::from_emu(1),
            Length::from_emu(-899_795),
            Length::ZERO,
        ];
        for length in samples {
            let text = len(length);
            assert_eq!(
                parse_len(&text),
                Some(length),
                "`{text}` did not round-trip"
            );
        }
    }

    #[test]
    fn a_bare_number_is_read_as_pixels() {
        assert_eq!(parse_len("96"), Some(Length::from_inch(1.0)));
        assert_eq!(parse_len("-24"), Some(Length::from_px(-24.0)));
    }

    #[test]
    fn nonsense_is_rejected_rather_than_guessed() {
        assert_eq!(parse_len(""), None);
        assert_eq!(parse_len("wide"), None);
        assert_eq!(parse_len("3furlongs"), None);
        assert_eq!(parse_len("1.5emu"), None, "EMU are whole by definition");
        assert_eq!(parse_percent("mucho"), None);
    }

    #[test]
    fn percentages_read_with_or_without_the_sign() {
        assert_eq!(parse_percent("12.5%"), Some(12.5));
        assert_eq!(parse_percent("12.5"), Some(12.5));
    }
}
