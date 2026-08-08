//! Raw preservation: what the IR does not model, kept anyway (13-I).
//!
//! A deck is full of things Markdown has no word for — a connector, a group, a
//! diagram, an animation timeline. Three rules apply to every one of them, and
//! all three have to hold at once (`AGENTS.md` §7 rule 3):
//!
//! 1. **A visible stub**, so an agent reading the slide knows the object is
//!    there and does not delete it by writing the slide back without it.
//! 2. **The markup, verbatim**, in a [`RawFragment`] the stub points at by id.
//!    Byte for byte from the source part, never re-serialised: a re-serialised
//!    subtree has already lost namespace prefixes and attribute order.
//! 3. **A typed warning**, so the loss of *fidelity* — the stub is not the
//!    shape — is in the report rather than in the user's surprise.
//!
//! The ids are deck-wide (`raw-0001`, `raw-0002`, …) because the payloads are:
//! [`Presentation::raw`](docsai_model::presentation::Presentation::raw) is one
//! list, and a slide or a shape refers into it.

use docsai_model::image::RawId;
use docsai_model::presentation::{RawShape, RawShapeKind, Shape, ShapeGeometry, ShapeKind};
use docsai_model::report::ConversionReport;
use docsai_model::text::RawFragment;

use crate::xml::Element;

/// Collects the fragments of one deck and hands out their ids.
#[derive(Debug, Default)]
pub(super) struct Sink {
    seq: u32,
    pub(super) fragments: Vec<RawFragment>,
}

impl Sink {
    /// Preserves `element` verbatim and reports it as a raw block.
    ///
    /// `source` is the text of `part`, which is where the bytes come from: the
    /// element knows its own byte span, so preserving it is a slice, not a
    /// re-serialisation.
    pub(super) fn capture(
        &mut self,
        element: &Element,
        part: &str,
        source: &str,
        report: &mut ConversionReport,
    ) -> RawId {
        self.seq += 1;
        let id = RawId::new(format!("raw-{:04}", self.seq));
        report.raw_block(qualified(element), format!("{part}:{}", element.span.start));
        self.fragments.push(RawFragment {
            id: id.clone(),
            format: "ooxml".into(),
            part: part.to_string(),
            content: element.raw(source).to_string(),
        });
        id
    }

    /// A shape the IR has no kind for: the stub, its text and its geometry.
    ///
    /// The text is carried on the stub as well as inside the fragment, because
    /// a stub that swallowed an arrow's label would hide real content behind a
    /// payload no agent is meant to read.
    pub(super) fn shape(
        &mut self,
        element: &Element,
        kind: RawShapeKind,
        z_index: u32,
        part: &str,
        source: &str,
        report: &mut ConversionReport,
    ) -> Shape {
        let raw = self.capture(element, part, source, report);
        Shape {
            id: None,
            name: name_of(element),
            z_index,
            geometry: geometry_of(element),
            kind: ShapeKind::Raw(RawShape {
                kind,
                raw: Some(raw),
                text: element.deep_text().trim().to_string(),
            }),
        }
    }
}

/// `p:cxnSp` rather than `cxnSp`: the prefix is what makes the warning
/// searchable against the spec.
fn qualified(element: &Element) -> String {
    if element.prefix.is_empty() {
        element.name.clone()
    } else {
        format!("{}:{}", element.prefix, element.name)
    }
}

fn name_of(element: &Element) -> Option<String> {
    [
        "nvSpPr",
        "nvCxnSpPr",
        "nvGrpSpPr",
        "nvGraphicFramePr",
        "nvPicPr",
    ]
    .iter()
    .find_map(|nv| element.path(&[nv, "cNvPr"]))
    .and_then(|nv| nv.attr("name"))
    .filter(|name| !name.is_empty())
    .map(str::to_string)
}

/// Where the stub sits. A group states its transform on `p:grpSpPr` and a
/// graphic frame on `p:xfrm`, one level up from where a shape does; a stub with
/// no position at all would be read last on every slide (see `order`).
fn geometry_of(element: &Element) -> ShapeGeometry {
    let xfrm = element
        .path(&["spPr", "xfrm"])
        .or_else(|| element.path(&["grpSpPr", "xfrm"]))
        .or_else(|| element.child("xfrm"));
    let mut geometry = super::read_geometry(xfrm);
    geometry.preset = element
        .path(&["spPr", "prstGeom"])
        .and_then(|geom| geom.attr("prst"))
        .map(str::to_string);
    geometry
}
