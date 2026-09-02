//! The per-category fidelity metric of a round trip (architecture §4).
//!
//! `roundtrip` compares the document it read with the document it read back,
//! and the honest way to say how much survived is not one number: a deck that
//! kept every paragraph and lost every picture is not «95 % faithful», it is a
//! deck with no pictures. So the two documents are counted **by category**, and
//! each category is reported with both counts beside it — the percentage is a
//! convenience, the counts are the evidence.
//!
//! The categories are per document kind, and each one is a distinct way a
//! conversion can fail:
//!
//! - **Text**: paragraphs, headings, lists, tables, images, footnotes.
//! - **Workbook**: sheets, cells, formulas.
//! - **Presentation**: slides, shapes, paragraphs, pictures, tables, notes,
//!   objects. A deck is not a stream of blocks, so counting it as one would
//!   measure the wrong thing: a slide is the unit a person navigates, a shape
//!   is the unit they select and move, and the *objects* — a chart, a diagram,
//!   an OLE embedding, a custom geometry — are the ones this project keeps as a
//!   stub over preserved markup, which is exactly where a round trip is most
//!   likely to lose something without saying so. Notes are counted per slide
//!   rather than per paragraph because a notes page is present or it is not.
//!
//! A category neither document has is not reported: «tables 0/0» says nothing
//! about a deck with no tables, and a report full of them hides the ones that
//! matter.

use docsai_model::presentation::{Shape, ShapeKind};
use docsai_model::{ConversionStats, Document};
use serde::Serialize;

/// One category of node, before and after a round trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct CategoryFidelity {
    /// `slides`, `paragraphs`, `cells`… — the name is stable and is what the
    /// CLI and the MCP response print.
    pub category: &'static str,
    /// How many the source document held.
    pub before: u64,
    /// How many came back.
    pub after: u64,
}

impl CategoryFidelity {
    /// What fraction of the category survived, as a percentage.
    ///
    /// Over 100 % is a real answer, not a clamped one: a round trip that
    /// *invents* nodes has a defect of its own, and hiding it behind a ceiling
    /// would be the same lie as hiding a loss.
    pub fn preserved(&self) -> f32 {
        if self.before == 0 {
            return if self.after == 0 {
                100.0
            } else {
                f32::INFINITY
            };
        }
        self.after as f32 * 100.0 / self.before as f32
    }

    /// True when the category came back whole.
    pub fn intact(&self) -> bool {
        self.before == self.after
    }
}

/// Counts both documents and pairs the categories up.
///
/// The two documents are the same kind in every round trip this crate performs
/// — the second is what the first was written to and read back — so the
/// categories line up by name; a category only one of them has is still
/// reported, with a zero on the side that has none.
pub fn compare(before: &Document, after: &Document) -> Vec<CategoryFidelity> {
    let (first, second) = (categories(before), categories(after));
    let mut out = Vec::new();
    for (category, before) in first {
        let after = second
            .iter()
            .find(|(name, _)| *name == category)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        if before == 0 && after == 0 {
            continue;
        }
        out.push(CategoryFidelity {
            category,
            before,
            after,
        });
    }
    for (category, after) in second {
        if after > 0 && !out.iter().any(|entry| entry.category == category) {
            out.push(CategoryFidelity {
                category,
                before: 0,
                after,
            });
        }
    }
    out
}

/// The node counts of one document, by category.
pub fn categories(document: &Document) -> Vec<(&'static str, u64)> {
    match document {
        Document::Text(text) => {
            let stats = crate::inspect::stats_from_text(text, 0);
            vec![
                ("paragraphs", stats.paragraphs as u64),
                ("headings", stats.headings as u64),
                ("lists", stats.lists as u64),
                ("tables", stats.tables as u64),
                ("images", stats.images as u64),
                ("footnotes", stats.footnotes as u64),
            ]
        }
        Document::Workbook(book) => {
            let stats = crate::inspect::stats_from_workbook(book, 0);
            vec![
                ("sheets", stats.sheets as u64),
                ("cells", stats.cells as u64),
                ("formulas", stats.formulas as u64),
            ]
        }
        Document::Presentation(deck) => {
            let stats: ConversionStats = crate::inspect::stats_from_presentation(deck, 0);
            let mut shapes = Shapes::default();
            let mut notes = 0u64;
            for slide in &deck.slides {
                count_shapes(&slide.shapes, &mut shapes);
                if slide.notes.is_some() {
                    notes += 1;
                }
            }
            vec![
                ("slides", stats.slides as u64),
                ("shapes", shapes.total),
                ("paragraphs", stats.paragraphs as u64),
                ("pictures", shapes.pictures),
                ("tables", shapes.tables),
                ("notes", notes),
                ("objects", shapes.objects),
            ]
        }
    }
}

/// What a slide's shapes are, counted.
///
/// A picture and a table on a slide are *shapes*, not blocks, so neither the
/// block tally nor a walk over `Slide::blocks` sees them.
#[derive(Default)]
struct Shapes {
    total: u64,
    pictures: u64,
    tables: u64,
    objects: u64,
}

/// Shapes on a slide, by what they are.
///
/// A group counts as a shape and so do its children: a group that came back
/// empty has lost the slide's content even though the box is still there.
fn count_shapes(shapes: &[Shape], into: &mut Shapes) {
    for shape in shapes {
        into.total += 1;
        match &shape.kind {
            ShapeKind::Group(children) => count_shapes(children, into),
            ShapeKind::Picture(_) => into.pictures += 1,
            ShapeKind::Table(_) => into.tables += 1,
            // What the IR holds as a reference to markup it does not model.
            ShapeKind::Chart(_) | ShapeKind::Raw(_) => into.objects += 1,
            _ => {}
        }
    }
}

/// True when every category came back whole.
pub fn intact(categories: &[CategoryFidelity]) -> bool {
    categories.iter().all(CategoryFidelity::intact)
}

/// Only the categories that did not come back whole.
///
/// What a warning is allowed to name: reciting the intact ones beside them
/// would bury the loss in the report that exists to surface it.
pub fn changed(categories: &[CategoryFidelity]) -> Vec<CategoryFidelity> {
    categories
        .iter()
        .filter(|entry| !entry.intact())
        .cloned()
        .collect()
}

/// The categories as a single readable line, for the CLI and for a warning.
pub fn summary(categories: &[CategoryFidelity]) -> String {
    categories
        .iter()
        .map(|entry| {
            format!(
                "{} {}/{} ({:.0} %)",
                entry.category,
                entry.after,
                entry.before,
                entry.preserved()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::presentation::{Placeholder, Presentation, Slide};
    use docsai_model::text::Block;
    use docsai_model::text::Paragraph;

    fn deck() -> Document {
        let mut slide = Slide::default();
        slide.shapes.push(Shape::new(
            0,
            ShapeKind::Placeholder(Placeholder {
                body: vec![Block::Paragraph(Paragraph::text("Hola"))],
                ..Default::default()
            }),
        ));
        slide.notes = Some(vec![Block::Paragraph(Paragraph::text("Una nota"))]);
        Document::Presentation(Presentation {
            slides: vec![slide],
            ..Default::default()
        })
    }

    #[test]
    fn a_deck_is_counted_by_what_a_deck_is_made_of() {
        let counts = categories(&deck());
        let named: Vec<&str> = counts.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            named,
            vec![
                "slides",
                "shapes",
                "paragraphs",
                "pictures",
                "tables",
                "notes",
                "objects"
            ]
        );
        assert_eq!(counts[0], ("slides", 1));
        assert_eq!(counts[1], ("shapes", 1));
        assert_eq!(counts[5], ("notes", 1));
    }

    /// A category neither side has says nothing, and is left out.
    #[test]
    fn an_empty_category_is_not_reported() {
        let deck = deck();
        let compared = compare(&deck, &deck);
        assert!(intact(&compared));
        assert!(
            compared.iter().all(|entry| entry.category != "tables"),
            "{}",
            summary(&compared)
        );
    }

    /// The point of the metric: what is missing is named, not averaged away.
    #[test]
    fn a_lost_slide_is_named_with_its_two_counts() {
        let before = deck();
        let after = Document::Presentation(Presentation::default());
        let compared = compare(&before, &after);
        assert!(!intact(&compared));
        let slides = compared
            .iter()
            .find(|entry| entry.category == "slides")
            .expect("slides are always reported for a deck");
        assert_eq!((slides.before, slides.after), (1, 0));
        assert_eq!(slides.preserved(), 0.0);
        assert!(summary(&compared).contains("slides 0/1 (0 %)"));
    }
}
