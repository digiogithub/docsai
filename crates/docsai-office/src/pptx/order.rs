//! Reading order: the sequence a human reads a slide in.
//!
//! `p:spTree` order is **z-order** — what is drawn on top of what. It is not
//! reading order, and the two disagree the moment somebody sends a box to the
//! back or adds a title last. So the reader computes the order instead of
//! taking it from the file: placeholders first by type, then the remaining
//! shapes by top-left (analysis §5.3).
//!
//! The policy is only allowed to exist because it is **reversible**: every
//! shape keeps its `p:spTree` index in [`Shape::z_index`], so sorting by that
//! field puts the slide back exactly as it was written. A reordering that could
//! not be undone would permute a deck on every round trip.
//!
//! The order is also **total and deterministic**: every comparison ends in
//! `z_index`, which is unique on a slide, so no two shapes ever compare equal
//! and the result does not depend on the sort algorithm.

use docsai_model::presentation::{Shape, ShapeKind};

/// Two shapes whose tops differ by less than this are on the same row, and are
/// read left to right. An eighth of an inch: hand-placed boxes meant to line up
/// rarely agree to the EMU, and a reader that demanded exact equality would
/// read a row of three cards top-down by whichever one is a hair higher.
const ROW_BAND_EMU: i64 = 114_300;

/// Sorts a slide's shapes into reading order, in place.
pub(super) fn sort(shapes: &mut [Shape]) {
    shapes.sort_by_key(key);
}

/// The sort key, as `(rank, placeholder index, row band, x, z-order)`.
///
/// The last component is the tie-break that makes the order total; the middle
/// two are only meaningful for the shapes whose rank uses them.
fn key(shape: &Shape) -> (u8, u32, i64, i64, u32) {
    match &shape.kind {
        ShapeKind::Placeholder(ph) => {
            let rank = if ph.ph_type.is_title() {
                0
            } else if ph.ph_type.is_body() {
                1
            } else if ph.ph_type.is_furniture() {
                // Slide number, date, header and footer are read last of the
                // placeholders: they repeat on every slide and say nothing
                // about this one.
                3
            } else {
                2
            };
            // Within a rank, `p:ph@idx` is what matches a slide placeholder to
            // its layout one, so it is also the order the layout intended.
            (rank, ph.idx.unwrap_or(0), 0, 0, shape.z_index)
        }
        _ => match shape.geometry.pos {
            Some(pos) => (
                4,
                0,
                // `div_euclid`, not `/`: truncation toward zero would make the
                // band straddling y = 0 twice as tall as every other one.
                pos.y.emu().div_euclid(ROW_BAND_EMU),
                pos.x.emu(),
                shape.z_index,
            ),
            // A shape with no position of its own inherits one this reader
            // cannot resolve. Guessing where it sits would be worse than
            // leaving it where the file put it, so it keeps source order,
            // after everything that can be placed.
            None => (5, 0, 0, 0, shape.z_index),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::presentation::ShapeGeometry;
    use docsai_model::presentation::{PhType, Placeholder};
    use docsai_model::units::{Length, Point, Size};

    fn placeholder(z: u32, ph_type: PhType, idx: Option<u32>) -> Shape {
        Shape::new(
            z,
            ShapeKind::Placeholder(Placeholder {
                ph_type,
                idx,
                body: Vec::new(),
                delta: Default::default(),
            }),
        )
    }

    fn box_at(z: u32, x: i64, y: i64) -> Shape {
        let mut shape = Shape::new(z, ShapeKind::TextBox { body: Vec::new() });
        shape.geometry = ShapeGeometry::at(
            Point::new(Length::from_emu(x), Length::from_emu(y)),
            Size::new(Length::from_emu(1), Length::from_emu(1)),
        );
        shape
    }

    fn unplaced(z: u32) -> Shape {
        Shape::new(z, ShapeKind::TextBox { body: Vec::new() })
    }

    fn order(shapes: &[Shape]) -> Vec<u32> {
        let mut shapes = shapes.to_vec();
        sort(&mut shapes);
        shapes.iter().map(|s| s.z_index).collect()
    }

    #[test]
    fn placeholders_come_first_and_by_type() {
        let shapes = vec![
            box_at(0, 0, 0),
            placeholder(1, PhType::SlideNumber, Some(10)),
            placeholder(2, PhType::Body, Some(2)),
            placeholder(3, PhType::Title, None),
            placeholder(4, PhType::Body, Some(1)),
            placeholder(5, PhType::Picture, Some(3)),
        ];
        assert_eq!(order(&shapes), vec![3, 4, 2, 5, 1, 0]);
    }

    #[test]
    fn free_shapes_are_read_top_left_and_row_by_row() {
        // The middle two are a hair apart vertically: one row, read left to
        // right. The last one is a full row below whatever it started as.
        let shapes = vec![
            box_at(0, 838_200, 5_000_000),
            box_at(1, 5_000_000, 2_000_000),
            box_at(2, 838_200, 2_010_000),
        ];
        assert_eq!(order(&shapes), vec![2, 1, 0]);
    }

    #[test]
    fn a_shape_with_no_position_keeps_source_order_at_the_end() {
        let shapes = vec![unplaced(0), box_at(1, 100, 100), unplaced(2)];
        assert_eq!(order(&shapes), vec![1, 0, 2]);
    }

    #[test]
    fn the_order_is_reversible_and_stable() {
        let mut shapes = vec![
            box_at(0, 4_000_000, 3_000_000),
            placeholder(1, PhType::Body, Some(1)),
            box_at(2, 1_000_000, 3_000_000),
            placeholder(3, PhType::Title, None),
        ];
        sort(&mut shapes);
        assert_eq!(
            shapes.iter().map(|s| s.z_index).collect::<Vec<_>>(),
            vec![3, 1, 2, 0]
        );

        let once = shapes.clone();
        sort(&mut shapes);
        assert_eq!(shapes, once, "sorting an ordered slide changes nothing");

        // What makes the policy admissible: the source order is still there.
        shapes.sort_by_key(|shape| shape.z_index);
        assert_eq!(
            shapes.iter().map(|s| s.z_index).collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
    }
}
