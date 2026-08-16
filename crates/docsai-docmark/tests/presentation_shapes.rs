//! The shape containers of a slide: the placeholders the layout does not make
//! implicit, free shapes and connectors (spec §11.2 rules 4, 6, 7 and 8; plan
//! v2 Phase 14 increment D).
//!
//! Pictures, tables, groups and charts have a form of their own — increment F,
//! `presentation_objects.rs`.

use docsai_docmark::{serialize, Fidelity, Options};
use docsai_model::addressing::IdPolicy;
use docsai_model::image::{Flip, RawId};
use docsai_model::presentation::{
    Layout, LayoutId, LayoutPlaceholder, PhType, Placeholder, Presentation, RawShape, RawShapeKind,
    Shape, ShapeGeometry, ShapeKind, Slide,
};
use docsai_model::text::{Block, Paragraph};
use docsai_model::units::{Length, Point, Size};
use docsai_model::{Document, Format, MemoryAssetStore, Warning};

fn options(fidelity: Fidelity) -> Options {
    Options {
        fidelity,
        source_format: Format::Pptx,
        ..Default::default()
    }
}

/// `standard` as the pipeline builds it: a level that does not write back
/// carries no ids (`ConvertOptions::id_policy`).
fn standard() -> Options {
    Options {
        ids: IdPolicy::Never,
        ..options(Fidelity::Standard)
    }
}

/// A layout with a title at index 0 and a body at 1: everything else on a
/// slide is a container.
fn layout() -> Layout {
    Layout {
        name: "Title and Content".into(),
        master: None,
        placeholders: vec![
            LayoutPlaceholder {
                ph_type: PhType::Title,
                idx: None,
                ..Default::default()
            },
            LayoutPlaceholder {
                ph_type: PhType::Body,
                idx: Some(1),
                ..Default::default()
            },
        ],
    }
}

fn placeholder(ph_type: PhType, idx: Option<u32>, text: &str) -> Shape {
    Shape::new(
        0,
        ShapeKind::Placeholder(Placeholder {
            ph_type,
            idx,
            body: vec![Block::Paragraph(Paragraph::text(text))],
            ..Default::default()
        }),
    )
}

/// The spec's own example shape: an arrow the author dragged, with its markup
/// in the sidecar.
fn arrow() -> Shape {
    Shape {
        id: None,
        name: Some("Arrow 1".into()),
        z_index: 3,
        geometry: ShapeGeometry {
            pos: Some(Point::new(Length::from_px(88.0), Length::from_emu(5200000))),
            size: Some(Size::new(
                Length::from_emu(1400000),
                Length::from_emu(500000),
            )),
            preset: Some("rightArrow".into()),
            ..Default::default()
        },
        kind: ShapeKind::Raw(RawShape {
            kind: RawShapeKind::Shape,
            raw: Some(RawId::new("r7")),
            text: "Crece".into(),
        }),
    }
}

/// A deck of one slide holding a title, the implicit body, and `extra`.
fn deck(extra: Vec<Shape>) -> Document {
    let mut presentation = Presentation::default();
    presentation
        .layouts
        .layouts
        .insert(LayoutId::new("L1"), layout());
    let mut shapes = vec![
        placeholder(PhType::Title, None, "Q3 results"),
        placeholder(PhType::Body, Some(1), "Revenue up 12 %"),
    ];
    shapes.extend(extra);
    presentation.slides.push(Slide {
        layout: Some(LayoutId::new("L1")),
        shapes,
        ..Default::default()
    });
    Document::Presentation(presentation)
}

fn body_of(markdown: &str) -> String {
    match markdown.strip_prefix("---\n") {
        Some(rest) => rest
            .split_once("\n---\n")
            .map(|(_, body)| body.trim_start_matches('\n').to_string())
            .expect("front matter"),
        None => markdown.to_string(),
    }
}

#[test]
fn a_free_shape_is_a_container_with_its_geometry() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(vec![arrow()]), &store, &options(Fidelity::Full));

    // Rule 7: readable units where they are exact, `emu` where they are not —
    // and the shape's text is inside the container, not swallowed by the stub.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results {#n1 .slide layout=L1}\n\nRevenue up 12 %\n\n\
         ::: {#n2 .shape geom=rightArrow name=\"Arrow 1\" \
         pos=\"88px,5200000emu\" raw=r7 size=\"1400000emu,500000emu\"}\nCrece\n:::\n",
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn standard_keeps_the_shape_and_drops_its_measurements() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(vec![arrow()]), &store, &standard());

    // Rule 6 takes the geometry, the name, the index and the raw reference;
    // rule 8 keeps the stub, because a reader must know the arrow is there.
    // `geom=` is identity, not measurement: without it a box is not an arrow.
    let body = body_of(&markdown);
    assert!(
        body.contains("::: {.shape geom=rightArrow}\nCrece\n:::"),
        "{markdown}"
    );
    assert!(!body.contains("pos="), "{markdown}");
    assert!(!body.contains("raw="), "{markdown}");
    // The bytes the stub points at are not in this document, and that is said.
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::RawBlockDropped { id, .. } if id == "r7")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn agent_keeps_the_geometry_because_it_writes_back() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(vec![arrow()]), &store, &options(Fidelity::Agent));

    // The one place where `agent` keeps a measurement: rule 7 names `full` and
    // `agent`, because a position an author dragged is what the writer has to
    // put back and no cascade can recover it.
    assert!(
        body_of(&markdown).contains("pos=\"88px,5200000emu\""),
        "{markdown}"
    );
    assert!(body_of(&markdown).contains("raw=r7"), "{markdown}");
}

#[test]
fn a_connector_says_it_is_a_connector() {
    let mut shape = arrow();
    shape.name = None;
    shape.geometry = ShapeGeometry::default();
    shape.kind = ShapeKind::Raw(RawShape {
        kind: RawShapeKind::Connector,
        raw: Some(RawId::new("r9")),
        text: String::new(),
    });
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(vec![shape]), &store, &options(Fidelity::Full));

    // An empty container is written on one pair of lines: there is nothing
    // between the fences and nothing pretending there is.
    assert!(
        body_of(&markdown).ends_with("::: {#n2 .connector raw=r9}\n:::\n"),
        "{markdown}"
    );
}

#[test]
fn a_text_box_is_a_free_shape() {
    let text_box = Shape {
        id: None,
        name: Some("TextBox 4".into()),
        z_index: 2,
        geometry: ShapeGeometry::at(
            Point::new(Length::from_px(10.0), Length::from_px(20.0)),
            Size::new(Length::from_px(300.0), Length::from_px(40.0)),
        ),
        kind: ShapeKind::TextBox {
            body: vec![Block::Paragraph(Paragraph::text("al margen"))],
        },
    };
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(vec![text_box]), &store, &options(Fidelity::Full));

    assert!(
        body_of(&markdown).contains(
            "::: {#n2 .shape name=\"TextBox 4\" pos=\"10px,20px\" size=\"300px,40px\"}\n\
             al margen\n:::"
        ),
        "{markdown}"
    );
}

#[test]
fn a_rotated_and_flipped_shape_keeps_both() {
    let mut shape = arrow();
    shape.geometry.rotation_deg = 45.0;
    shape.geometry.flip = Flip::HV;
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(vec![shape]), &store, &options(Fidelity::Full));

    let body = body_of(&markdown);
    assert!(body.contains("flip=hv"), "{markdown}");
    assert!(body.contains("rotation=45"), "{markdown}");
}

#[test]
fn a_placeholder_that_is_not_implicit_names_its_index_and_its_type() {
    let store = MemoryAssetStore::new();
    let subtitle = placeholder(PhType::Subtitle, Some(2), "un subtítulo");
    let (markdown, _) = serialize(
        &deck(vec![subtitle.clone()]),
        &store,
        &options(Fidelity::Full),
    );

    // `idx` matches the shape to its layout placeholder, so it is only useful
    // where the document writes back; the type is what a reader needs.
    assert!(
        body_of(&markdown).contains("::: {#n2 .ph idx=2 type=subTitle}\nun subtítulo\n:::"),
        "{markdown}"
    );

    let (standard_md, _) = serialize(&deck(vec![subtitle]), &store, &standard());
    assert!(
        body_of(&standard_md).contains("::: {.ph type=subTitle}\nun subtítulo\n:::"),
        "{standard_md}"
    );
}

#[test]
fn a_body_placeholder_writes_no_type_because_body_is_the_default() {
    let store = MemoryAssetStore::new();
    let second = placeholder(PhType::Body, Some(2), "segundo cuerpo");
    let (markdown, _) = serialize(&deck(vec![second]), &store, &options(Fidelity::Full));

    assert!(
        body_of(&markdown).contains("::: {#n2 .ph idx=2}\nsegundo cuerpo\n:::"),
        "{markdown}"
    );
}

#[test]
fn slide_furniture_survives_where_it_is_written_back_and_is_warned_where_it_is_not() {
    let store = MemoryAssetStore::new();
    let footer = placeholder(PhType::Footer, Some(11), "Digio · 2026");
    let (markdown, report) = serialize(
        &deck(vec![footer.clone()]),
        &store,
        &options(Fidelity::Full),
    );
    assert!(
        body_of(&markdown).contains(".ph idx=11 type=ftr"),
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // A footer repeated on every slide is inherited from the layout: it costs
    // a box per slide and carries nothing the reader wrote.
    let (standard_md, standard_report) = serialize(&deck(vec![footer]), &store, &standard());
    assert!(!standard_md.contains("Digio"), "{standard_md}");
    assert!(
        standard_report.warnings.iter().any(|w| matches!(
            w,
            Warning::UnsupportedElement { kind, action, .. }
                if kind == "placeholder ftr" && action.starts_with("dropped")
        )),
        "{:?}",
        standard_report.warnings
    );
}

#[test]
fn plain_writes_no_container_and_keeps_the_text() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(
        &deck(vec![
            placeholder(PhType::Subtitle, Some(2), "un subtítulo"),
            arrow(),
        ]),
        &store,
        &options(Fidelity::Plain),
    );

    // A `:::` fence is literal text to a CommonMark viewer, so `plain` has
    // none: what the shapes say survives, the boxes do not.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results\n\nRevenue up 12 %\n\nun subtítulo\n\nCrece\n",
        "{markdown}"
    );
}

#[test]
fn plain_says_when_a_shape_leaves_nothing_behind() {
    let mut shape = arrow();
    let ShapeKind::Raw(raw) = &mut shape.kind else {
        unreachable!()
    };
    raw.text = String::new();
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(vec![shape]), &store, &options(Fidelity::Plain));

    assert!(!markdown.contains(":::"), "{markdown}");
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            Warning::UnsupportedElement { kind, .. } if kind == "shape"
        )),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_shape_takes_its_id_before_what_it_holds() {
    let store = MemoryAssetStore::new();
    let second = placeholder(PhType::Body, Some(2), "segundo cuerpo");
    let (markdown, _) = serialize(&deck(vec![second]), &store, &options(Fidelity::Full));

    // The addressing walk visits a shape and then its blocks; the writer that
    // allocated them the other way round would hand out ids the walk cannot
    // find again.
    assert_eq!(
        ids_in_order(&body_of(&markdown)),
        vec!["n1", "n2"],
        "{markdown}"
    );
}

/// The ids the body carries, in writing order.
fn ids_in_order(markdown: &str) -> Vec<String> {
    markdown
        .match_indices("{#")
        .map(|(at, _)| {
            markdown[at + 2..]
                .split(|c: char| !c.is_alphanumeric())
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[test]
fn an_unmodelled_object_is_a_stub_of_its_own_kind() {
    let store = MemoryAssetStore::new();
    let smartart = Shape::new(
        4,
        ShapeKind::Raw(RawShape {
            kind: RawShapeKind::SmartArt,
            raw: Some(RawId::new("r3")),
            text: String::new(),
        }),
    );
    let (markdown, report) = serialize(
        &deck(vec![smartart.clone()]),
        &store,
        &options(Fidelity::Full),
    );

    assert!(
        markdown.contains("::: {#n2 .smartart raw=r3}\n:::\n"),
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // Rule 8: the stub is at every level. Rule 6 takes the payload away, and
    // says so.
    let (markdown, report) = serialize(&deck(vec![smartart]), &store, &standard());
    assert!(markdown.contains("::: {.smartart}\n:::\n"), "{markdown}");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::RawBlockDropped { id, .. } if id == "r3")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_deck_with_containers_is_byte_deterministic() {
    let store = MemoryAssetStore::new();
    let document = deck(vec![arrow(), placeholder(PhType::Subtitle, Some(2), "sub")]);
    let (first, _) = serialize(&document, &store, &options(Fidelity::Full));
    let (second, _) = serialize(&document, &store, &options(Fidelity::Full));
    assert_eq!(first, second);
}
