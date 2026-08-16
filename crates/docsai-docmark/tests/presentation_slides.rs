//! The body of a deck: the slide heading and the implicit body placeholder
//! (spec §11.2 rules 1–3, plan v2 Phase 14 increment C).
//!
//! The containers those rules leave over live in `presentation_shapes.rs`;
//! notes, pictures and tables arrive in the increments after this one, and
//! what they do here is warn.

use docsai_docmark::{serialize, Fidelity, Options};
use docsai_model::addressing::IdPolicy;
use docsai_model::image::RawId;
use docsai_model::presentation::{
    Layout, LayoutId, LayoutPlaceholder, MasterId, PhType, Placeholder, Presentation, Shape,
    ShapeKind, Slide,
};
use docsai_model::text::{Block, List, ListItem, Paragraph};
use docsai_model::{Document, Format, MemoryAssetStore, Warning};

fn options(fidelity: Fidelity) -> Options {
    Options {
        fidelity,
        source_format: Format::Pptx,
        ..Default::default()
    }
}

/// The layout of the spec's example: a title at index 0 and a body at 1.
fn layout() -> Layout {
    Layout {
        name: "Title and Content".into(),
        master: Some(MasterId::new("M1")),
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

fn placeholder(ph_type: PhType, idx: Option<u32>, body: Vec<Block>) -> Shape {
    Shape::new(
        0,
        ShapeKind::Placeholder(Placeholder {
            ph_type,
            idx,
            body,
            ..Default::default()
        }),
    )
}

fn bullets(items: &[&str]) -> Block {
    Block::List(List {
        id: None,
        def: None,
        ordered: false,
        level: 0,
        items: items
            .iter()
            .map(|text| ListItem {
                blocks: vec![Block::Paragraph(Paragraph::text(*text))],
            })
            .collect(),
    })
}

/// One slide: a title, a body of two bullets, and the layout that says which
/// is which.
fn deck() -> Document {
    let mut presentation = Presentation::default();
    presentation
        .layouts
        .layouts
        .insert(LayoutId::new("L1"), layout());
    presentation.slides.push(Slide {
        layout: Some(LayoutId::new("L1")),
        shapes: vec![
            placeholder(
                PhType::Title,
                None,
                vec![Block::Paragraph(Paragraph::text("Q3 results"))],
            ),
            placeholder(
                PhType::Body,
                Some(1),
                vec![bullets(&["Revenue up 12 %", "Churn flat"])],
            ),
        ],
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
fn a_slide_is_a_heading_and_the_heading_is_the_title() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(), &store, &options(Fidelity::Full));

    // Rule 1 and rule 2 together: no container repeats the title, and the
    // primary body is ordinary Markdown under it.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results {#n1 .slide layout=L1}\n\n- Revenue up 12 % {list-id=n2}\n- Churn flat\n",
        "{markdown}"
    );
}

#[test]
fn the_slide_id_lives_on_the_heading_and_nowhere_else() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(), &store, &options(Fidelity::Full));

    // The two implicit shapes take no id: there is nowhere to write one, and
    // an id that cannot be written changes on every round trip.
    assert_eq!(markdown.matches("#n1").count(), 1, "{markdown}");
    assert!(markdown.contains("next-id: 3"), "{markdown}");
}

#[test]
fn standard_writes_the_slide_without_ids() {
    let store = MemoryAssetStore::new();
    // The id policy is the caller's, as it is for a text document: the
    // pipeline derives it from the level (`ConvertOptions::id_policy`), and a
    // serializer that overrode it would take that choice away.
    let standard = Options {
        ids: IdPolicy::Never,
        ..options(Fidelity::Standard)
    };
    let (markdown, _) = serialize(&deck(), &store, &standard);

    // Rule 6: no ids, and no catalogue for an id to resolve against — but the
    // slide is still a `.slide`, which is what makes the profile 1.2.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results {.slide layout=L1}\n\n- Revenue up 12 %\n- Churn flat\n",
        "{markdown}"
    );
}

#[test]
fn plain_is_a_heading_and_bullets_and_nothing_else() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(), &store, &options(Fidelity::Plain));

    assert_eq!(
        markdown, "## Q3 results\n\n- Revenue up 12 %\n- Churn flat\n",
        "{markdown}"
    );
}

#[test]
fn a_titleless_slide_writes_an_empty_heading() {
    let mut presentation = Presentation::default();
    presentation
        .layouts
        .layouts
        .insert(LayoutId::new("L1"), layout());
    presentation.slides.push(Slide {
        layout: Some(LayoutId::new("L1")),
        shapes: vec![placeholder(
            PhType::Body,
            Some(1),
            vec![Block::Paragraph(Paragraph::text("solo cuerpo"))],
        )],
        ..Default::default()
    });
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(
        &Document::Presentation(presentation),
        &store,
        &options(Fidelity::Full),
    );

    // Rule 1, accepted as ugly and bounded: the alternative is a container on
    // every slide. No trailing space, so the line survives a round trip.
    assert!(
        body_of(&markdown).starts_with("## {#n1 .slide layout=L1}\n"),
        "{markdown}"
    );
}

#[test]
fn an_empty_title_placeholder_is_the_same_empty_heading() {
    let mut presentation = Presentation::default();
    presentation
        .layouts
        .layouts
        .insert(LayoutId::new("L1"), layout());
    presentation.slides.push(Slide {
        layout: Some(LayoutId::new("L1")),
        // The box is there and holds its layout position; it just has no text.
        shapes: vec![placeholder(PhType::Title, None, Vec::new())],
        ..Default::default()
    });
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(
        &Document::Presentation(presentation),
        &store,
        &options(Fidelity::Full),
    );

    assert_eq!(
        body_of(&markdown),
        "## {#n1 .slide layout=L1}\n",
        "{markdown}"
    );
}

#[test]
fn the_layout_decides_which_body_is_implicit() {
    // Two body placeholders: the layout names index 2, so index 1 is *not*
    // the implicit one — the catalogue lookup of rule 3, and the case a
    // «first body wins» heuristic gets wrong.
    let mut presentation = Presentation::default();
    presentation.layouts.layouts.insert(
        LayoutId::new("L2"),
        Layout {
            name: "Two Content".into(),
            master: None,
            placeholders: vec![
                LayoutPlaceholder {
                    ph_type: PhType::Title,
                    idx: None,
                    ..Default::default()
                },
                LayoutPlaceholder {
                    ph_type: PhType::Body,
                    idx: Some(2),
                    ..Default::default()
                },
            ],
        },
    );
    presentation.slides.push(Slide {
        layout: Some(LayoutId::new("L2")),
        shapes: vec![
            placeholder(
                PhType::Body,
                Some(1),
                vec![Block::Paragraph(Paragraph::text("izquierda"))],
            ),
            placeholder(
                PhType::Body,
                Some(2),
                vec![Block::Paragraph(Paragraph::text("derecha"))],
            ),
        ],
        ..Default::default()
    });
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(
        &Document::Presentation(presentation),
        &store,
        &options(Fidelity::Full),
    );

    // The implicit one is bare Markdown; the other is a container, not a loss.
    assert!(body_of(&markdown).contains("\nderecha\n"), "{markdown}");
    assert!(
        body_of(&markdown).contains("::: {#n2 .ph idx=1}\nizquierda\n:::"),
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_slide_carries_its_section_and_its_hidden_flag() {
    let Document::Presentation(mut presentation) = deck() else {
        unreachable!()
    };
    presentation.slides[0].section = Some("Cierre".into());
    presentation.slides[0].hidden = true;
    presentation.slides[0].name = Some("Slide 1".into());
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(
        &Document::Presentation(presentation.clone()),
        &store,
        &options(Fidelity::Full),
    );

    assert!(
        body_of(&markdown).starts_with(
            "## Q3 results {#n1 .slide hidden=true layout=L1 name=\"Slide 1\" section=Cierre}\n"
        ),
        "{markdown}"
    );

    // `name=` is what the writer puts back, so the levels that do not write
    // back drop it; the section is structure a reader wants either way.
    let (standard, _) = serialize(
        &Document::Presentation(presentation),
        &store,
        &options(Fidelity::Standard),
    );
    assert!(!standard.contains("name="), "{standard}");
    assert!(standard.contains("section=Cierre"), "{standard}");
}

#[test]
fn several_slides_are_separated_by_a_blank_line() {
    let Document::Presentation(mut presentation) = deck() else {
        unreachable!()
    };
    let second = presentation.slides[0].clone();
    presentation.slides.push(second);
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(
        &Document::Presentation(presentation),
        &store,
        &options(Fidelity::Full),
    );

    assert_eq!(body_of(&markdown).matches("{.slide").count(), 0);
    assert_eq!(
        body_of(&markdown).matches(".slide").count(),
        2,
        "{markdown}"
    );
    assert!(
        body_of(&markdown).contains("Churn flat\n\n## Q3 results {#n3 .slide"),
        "{markdown}"
    );
    assert_eq!(report.stats.slides, 2);
}

#[test]
fn what_is_not_written_yet_is_warned_not_dropped() {
    let Document::Presentation(mut presentation) = deck() else {
        unreachable!()
    };
    presentation.slides[0].shapes.push(placeholder(
        PhType::Body,
        Some(2),
        vec![Block::Paragraph(Paragraph::text("segundo cuerpo"))],
    ));
    // `p:transition` and `p:timing`: slide-level subtrees the IR keeps in the
    // sidecar and the serializer has nowhere to put.
    presentation.slides[0].raw = vec![RawId::new("raw-0007")];
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(
        &Document::Presentation(presentation),
        &store,
        &options(Fidelity::Full),
    );

    assert!(markdown.contains("::: {#n3 .ph idx=2}"), "{markdown}");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::RawBlockDropped { id, .. } if id == "raw-0007")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_multi_paragraph_title_keeps_its_text() {
    let Document::Presentation(mut presentation) = deck() else {
        unreachable!()
    };
    presentation.slides[0].shapes[0] = placeholder(
        PhType::Title,
        None,
        vec![
            Block::Paragraph(Paragraph::text("Q3")),
            Block::Paragraph(Paragraph::text("results")),
        ],
    );
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(
        &Document::Presentation(presentation),
        &store,
        &options(Fidelity::Full),
    );

    // A heading is one line: the text survives, the paragraph break does not,
    // and the loss is reported rather than silent.
    assert!(
        body_of(&markdown).starts_with("## Q3 results {#n1 .slide layout=L1}"),
        "{markdown}"
    );
    assert!(report.warnings.iter().any(
        |w| matches!(w, Warning::UnsupportedElement { action, .. } if action.contains("heading"))
    ));
}

#[test]
fn a_deck_is_byte_deterministic() {
    let store = MemoryAssetStore::new();
    let once = serialize(&deck(), &store, &options(Fidelity::Full)).0;
    let twice = serialize(&deck(), &store, &options(Fidelity::Full)).0;
    assert_eq!(once, twice, "spec §8");
}
