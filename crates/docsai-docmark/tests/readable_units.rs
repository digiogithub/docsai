//! Readable units (plan v2 Phase 11, E; spec §2).
//!
//! A length is written in the unit of what it measures — points for layout,
//! pixels for drawings — and the unit is only used when it is **exact** at the
//! configured precision. That is the claim this file exists to falsify: the
//! plan allowed a tolerance here, and the implementation does not need one,
//! so the tolerance has to be measured rather than asserted. Every test below
//! compares EMU against EMU after a full render/parse cycle.

use docsai_docmark::units::{geometry, len};
use docsai_model::units::{Length, DEFAULT_PRECISION};

/// The round trip a serialised length actually makes.
fn reparse(text: &str) -> Length {
    Length::parse(text).unwrap_or_else(|| panic!("`{text}` must parse back"))
}

#[test]
fn every_word_layout_length_is_exact_in_points() {
    // Word writes layout in twips. A twip is exactly 0.05pt, so two decimals
    // name every one of them and none of these should reach the EMU hatch.
    for twips in -2000..=2000 {
        let value = Length::from_twips(twips);
        let text = len(value, DEFAULT_PRECISION);
        assert!(
            text.ends_with("pt") || text == "0",
            "{twips} twips should be points, got `{text}`"
        );
        assert_eq!(
            reparse(&text),
            value,
            "`{text}` must survive the round trip"
        );
    }
}

#[test]
fn every_bitmap_size_is_exact_in_pixels() {
    for px in 0..=2000 {
        let value = Length::from_px(px as f64);
        let text = geometry(value, DEFAULT_PRECISION);
        assert!(
            text.ends_with("px") || text == "0",
            "{px}px should stay pixels, got `{text}`"
        );
        assert_eq!(reparse(&text), value);
    }
}

#[test]
fn the_tolerance_is_zero_at_every_precision() {
    // Values chosen to hit each branch: twips, ODF hundredths of a
    // millimetre, half-points, and raw EMU that no readable unit can name.
    let awkward = [
        Length::from_twips(1417),
        Length::from_cm(1.251),
        Length::from_mm(4.37),
        Length::from_half_points(23),
        Length::from_emu(1),
        Length::from_emu(-7_777_777),
        Length::from_inch(1.0 / 3.0),
    ];
    for value in awkward {
        for precision in 0..=6 {
            for text in [len(value, precision), geometry(value, precision)] {
                assert_eq!(
                    reparse(&text),
                    value,
                    "`{text}` at precision {precision} moved the length"
                );
            }
        }
    }
}

#[test]
fn precision_buys_a_readable_unit_never_a_rounded_one() {
    // 1.251cm needs three decimals. At two, the length is written in EMU
    // rather than rounded to 1.25cm — which would move it by 3600 EMU.
    let value = Length::from_cm(1.251);
    assert_eq!(len(value, 2), "450360emu");
    assert_eq!(len(value, 3), "1.251cm");
    assert_eq!(reparse(&len(value, 2)), value);
    assert_eq!(reparse(&len(value, 3)), value);
}

#[test]
fn zero_carries_no_unit_and_still_parses() {
    assert_eq!(len(Length::ZERO, DEFAULT_PRECISION), "0");
    assert_eq!(geometry(Length::ZERO, DEFAULT_PRECISION), "0");
    assert_eq!(reparse("0"), Length::ZERO);
}

#[test]
fn what_a_length_measures_decides_its_unit() {
    // The same length, two roles: an indent reads in points, an image width
    // in pixels. Both are exact, so this is a readability choice only.
    let value = Length::from_twips(720);
    assert_eq!(len(value, DEFAULT_PRECISION), "36pt");
    assert_eq!(geometry(value, DEFAULT_PRECISION), "48px");
    assert_eq!(reparse("36pt"), reparse("48px"));
}
