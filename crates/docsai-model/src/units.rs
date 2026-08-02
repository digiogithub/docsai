//! Normalised lengths.
//!
//! Every length in the IR is stored in EMU (English Metric Units), the unit
//! OOXML uses for drawings: 914 400 per inch, which divides exactly into
//! points, centimetres, millimetres and twips. Conversion to the human units
//! DocMark exposes (`pt`, `cm`, `px`) happens only at serialisation time.
//!
//! ```
//! use docsai_model::units::Length;
//! assert_eq!(Length::from_cm(2.54).emu(), Length::from_inch(1.0).emu());
//! assert_eq!(Length::from_pt(72.0), Length::from_inch(1.0));
//! ```

use serde::{Deserialize, Serialize};
use std::fmt;

/// EMU in one inch. The definition all other constants derive from.
pub const EMU_PER_INCH: i64 = 914_400;
/// EMU in one typographic point (1/72 in).
pub const EMU_PER_POINT: i64 = EMU_PER_INCH / 72;
/// EMU in one centimetre.
pub const EMU_PER_CM: i64 = 360_000;
/// EMU in one millimetre.
pub const EMU_PER_MM: i64 = EMU_PER_CM / 10;
/// EMU in one twip (1/20 pt), the unit WordprocessingML uses for layout.
pub const EMU_PER_TWIP: i64 = EMU_PER_POINT / 20;
/// Resolution assumed when a bitmap declares none.
pub const DEFAULT_DPI: u32 = 96;

/// A length, stored in EMU.
///
/// `Length` is deliberately a newtype over `i64`: negative values are legal
/// (indents and drawing offsets can be negative) and integer EMU keep the
/// round-trip exact for every source format.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Length(i64);

impl Length {
    /// The zero length.
    pub const ZERO: Length = Length(0);

    /// Builds a length from raw EMU.
    pub const fn from_emu(emu: i64) -> Self {
        Length(emu)
    }

    /// The length in EMU.
    pub const fn emu(self) -> i64 {
        self.0
    }

    /// Builds a length from twips (1/1440 in), the WordprocessingML layout unit.
    pub const fn from_twips(twips: i64) -> Self {
        Length(twips * EMU_PER_TWIP)
    }

    /// The length in twips, rounded to nearest.
    pub fn twips(self) -> i64 {
        div_round(self.0, EMU_PER_TWIP)
    }

    /// Builds a length from half-points, the unit of `w:sz` (font sizes).
    pub const fn from_half_points(half_points: i64) -> Self {
        Length(half_points * (EMU_PER_POINT / 2))
    }

    /// The length in half-points, rounded to nearest.
    pub fn half_points(self) -> i64 {
        div_round(self.0, EMU_PER_POINT / 2)
    }

    /// Builds a length from eighth-points, the unit of border widths (`w:sz`).
    ///
    /// An eighth of a point is 1587.5 EMU, so this is the one Word unit that
    /// does not divide EMU exactly; the result is rounded to nearest.
    pub const fn from_eighth_points(eighth_points: i64) -> Self {
        Length(div_round(eighth_points * EMU_PER_POINT, 8))
    }

    /// Builds a length from points.
    pub fn from_pt(pt: f64) -> Self {
        Length((pt * EMU_PER_POINT as f64).round() as i64)
    }

    /// The length in points.
    pub fn pt(self) -> f64 {
        self.0 as f64 / EMU_PER_POINT as f64
    }

    /// Builds a length from centimetres.
    pub fn from_cm(cm: f64) -> Self {
        Length((cm * EMU_PER_CM as f64).round() as i64)
    }

    /// The length in centimetres.
    pub fn cm(self) -> f64 {
        self.0 as f64 / EMU_PER_CM as f64
    }

    /// Builds a length from millimetres.
    pub fn from_mm(mm: f64) -> Self {
        Length((mm * EMU_PER_MM as f64).round() as i64)
    }

    /// The length in millimetres.
    pub fn mm(self) -> f64 {
        self.0 as f64 / EMU_PER_MM as f64
    }

    /// Builds a length from inches.
    pub fn from_inch(inch: f64) -> Self {
        Length((inch * EMU_PER_INCH as f64).round() as i64)
    }

    /// The length in inches.
    pub fn inch(self) -> f64 {
        self.0 as f64 / EMU_PER_INCH as f64
    }

    /// Builds a length from pixels at the given resolution.
    pub fn from_px_at(px: f64, dpi: u32) -> Self {
        Length((px * EMU_PER_INCH as f64 / dpi.max(1) as f64).round() as i64)
    }

    /// Builds a length from pixels at 96 dpi, the CSS reference resolution.
    pub fn from_px(px: f64) -> Self {
        Self::from_px_at(px, DEFAULT_DPI)
    }

    /// The length in pixels at the given resolution.
    pub fn px_at(self, dpi: u32) -> f64 {
        self.0 as f64 * dpi.max(1) as f64 / EMU_PER_INCH as f64
    }

    /// The length in pixels at 96 dpi.
    pub fn px(self) -> f64 {
        self.px_at(DEFAULT_DPI)
    }

    /// True when the length is exactly zero.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// Parses a DocMark length (`450px`, `3.5cm`, `70.85pt`, `123emu`, or a bare
    /// number treated as pixels at 96 dpi).
    ///
    /// ```
    /// use docsai_model::units::Length;
    /// assert_eq!(Length::parse("450px"), Some(Length::from_px(450.0)));
    /// assert_eq!(Length::parse("3.5cm"), Some(Length::from_cm(3.5)));
    /// assert_eq!(Length::parse("70.85pt"), Some(Length::from_pt(70.85)));
    /// assert_eq!(Length::parse("123emu"), Some(Length::from_emu(123)));
    /// assert_eq!(Length::parse("96"), Some(Length::from_px(96.0)));
    /// assert_eq!(Length::parse("nope"), None);
    /// ```
    pub fn parse(text: &str) -> Option<Length> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let split = text.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(text.len());
        let (number, unit) = text.split_at(split);
        let value: f64 = number.parse().ok()?;
        match unit {
            "" | "px" => Some(Length::from_px(value)),
            "cm" => Some(Length::from_cm(value)),
            "mm" => Some(Length::from_mm(value)),
            "pt" => Some(Length::from_pt(value)),
            "in" | "inch" => Some(Length::from_inch(value)),
            "emu" => Some(Length::from_emu(value.round() as i64)),
            "twip" | "twips" => Some(Length::from_twips(value.round() as i64)),
            _ => None,
        }
    }
}

const fn div_round(value: i64, divisor: i64) -> i64 {
    let half = divisor / 2;
    if value >= 0 {
        (value + half) / divisor
    } else {
        (value - half) / divisor
    }
}

/// EMU in one pixel at 96 dpi.
const EMU_PER_PX: i64 = EMU_PER_INCH / DEFAULT_DPI as i64;
/// EMU in a hundredth of a centimetre.
const EMU_PER_CENTI_CM: i64 = EMU_PER_CM / 100;
/// EMU in a hundredth of a point.
const EMU_PER_CENTI_PT: i64 = EMU_PER_POINT / 100;

impl fmt::Display for Length {
    /// Renders the length the way DocMark does, always **losslessly**: the
    /// first unit that represents it exactly wins, in the order `px`, `cm`,
    /// `pt`, and raw `emu` as the last resort.
    ///
    /// Choosing readability over exactness here would silently move margins
    /// and image sizes on every round-trip, so exactness comes first: a Word
    /// margin of 1417 twips prints as `70.85pt`, not as an approximate
    /// `2.499cm`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 % EMU_PER_PX == 0 {
            write!(f, "{}px", self.0 / EMU_PER_PX)
        } else if self.0 % EMU_PER_CENTI_CM == 0 {
            write!(f, "{}cm", trim_float(self.cm(), 2))
        } else if self.0 % EMU_PER_CENTI_PT == 0 {
            write!(f, "{}pt", trim_float(self.pt(), 2))
        } else {
            write!(f, "{}emu", self.0)
        }
    }
}

/// Formats a float with at most `decimals` digits and no trailing zeros.
///
/// Used by every unit rendered into DocMark, so that the serialiser stays
/// deterministic (`2.5cm`, never `2.50cm` or `2.4999999cm`).
pub fn trim_float(value: f64, decimals: usize) -> String {
    let mut s = format!("{value:.decimals$}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// A rectangular size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Size {
    pub width: Length,
    pub height: Length,
}

impl Size {
    pub const fn new(width: Length, height: Length) -> Self {
        Size { width, height }
    }
}

/// A point in a two-dimensional coordinate space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: Length,
    pub y: Length,
}

impl Point {
    pub const fn new(x: Length, y: Length) -> Self {
        Point { x, y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inch_is_the_anchor_of_every_unit() {
        let inch = Length::from_inch(1.0);
        assert_eq!(inch.emu(), 914_400);
        assert_eq!(inch, Length::from_pt(72.0));
        assert_eq!(inch, Length::from_cm(2.54));
        assert_eq!(inch, Length::from_mm(25.4));
        assert_eq!(inch, Length::from_twips(1440));
        assert_eq!(inch, Length::from_px(96.0));
    }

    #[test]
    fn round_trips_through_every_unit() {
        // A length representable in every unit: 1944 twips = 97.2 pt = 129.6 px.
        let l = Length::from_emu(1_234_440);
        assert_eq!(Length::from_pt(l.pt()), l);
        assert_eq!(Length::from_cm(l.cm()), l);
        assert_eq!(Length::from_twips(l.twips()), l);
        assert_eq!(Length::from_px(l.px()), l);
    }

    #[test]
    fn coarser_units_round_rather_than_truncate() {
        // 1 234 560 EMU is 1944.19 twips: the nearest twip, not the floor.
        assert_eq!(Length::from_emu(1_234_560).twips(), 1944);
        assert_eq!(Length::from_emu(1_234_880).twips(), 1945);
    }

    #[test]
    fn px_conversion_honours_dpi() {
        let l = Length::from_px_at(300.0, 300);
        assert_eq!(l, Length::from_inch(1.0));
        assert_eq!(l.px_at(300), 300.0);
        assert_eq!(l.px(), 96.0);
    }

    #[test]
    fn word_units_map_exactly() {
        // w:sz="22" is 11 pt; w:ind w:left="720" is half an inch.
        assert_eq!(Length::from_half_points(22), Length::from_pt(11.0));
        assert_eq!(Length::from_twips(720), Length::from_inch(0.5));
        assert_eq!(Length::from_eighth_points(8), Length::from_pt(1.0));
    }

    #[test]
    fn negative_lengths_round_symmetrically() {
        assert_eq!(Length::from_twips(-720).twips(), -720);
        assert_eq!(Length::from_pt(-11.0).pt(), -11.0);
    }

    #[test]
    fn display_prefers_whole_pixels() {
        assert_eq!(Length::from_px(96.0).to_string(), "96px");
        assert_eq!(Length::from_cm(3.5).to_string(), "3.5cm");
    }

    #[test]
    fn display_is_always_lossless() {
        let samples = [
            Length::from_twips(1417), // a Word margin: not a whole px or cm
            Length::from_twips(2500), // a table column width
            Length::from_cm(3.0),
            Length::from_px(120.0),
            Length::from_pt(11.0),
            Length::from_emu(1), // nothing but EMU can express this
            Length::from_emu(-899_795),
        ];
        for length in samples {
            let text = length.to_string();
            assert_eq!(Length::parse(&text), Some(length), "`{text}` did not round-trip");
        }
        assert_eq!(Length::from_twips(1417).to_string(), "70.85pt");
        assert_eq!(Length::from_emu(1).to_string(), "1emu");
    }

    #[test]
    fn parse_accepts_bare_pixels_and_rejects_garbage() {
        assert_eq!(Length::parse("450px"), Some(Length::from_px(450.0)));
        assert_eq!(Length::parse("  3.5cm "), Some(Length::from_cm(3.5)));
        assert_eq!(Length::parse("123"), Some(Length::from_px(123.0)));
        assert_eq!(Length::parse(""), None);
        assert_eq!(Length::parse("cm"), None);
        assert_eq!(Length::parse("12xy"), None);
    }

    #[test]
    fn trim_float_is_deterministic() {
        assert_eq!(trim_float(2.500, 3), "2.5");
        assert_eq!(trim_float(2.0, 3), "2");
        assert_eq!(trim_float(-0.0001, 3), "0");
        assert_eq!(trim_float(1.0 / 3.0, 3), "0.333");
    }
}
