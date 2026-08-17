//! DocMark-P → IR (spec §11.2; plan v2 Phase 14 increment H).
//!
//! The mirror of `presentation_slides.rs`, `presentation_shapes.rs`,
//! `presentation_notes.rs` and `presentation_objects.rs`: everything those pin
//! as output, this reads back. Two claims are under test — the round trip of
//! what the writer emits, and the **tolerant input** of analysis §6.6: a deck
//! typed by hand, with no attribute anywhere, has to parse.

use docsai_docmark::{parse, serialize, Fidelity, Options};
use docsai_model::addressing::IdPolicy;
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

/// `standard` as the pipeline builds it: no ids, because it does not write
/// back.
fn standard() -> Options {
    Options {
        ids: IdPolicy::Never,
        ..options(Fidelity::Standard)
    }
}

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

/// A deck of one slide: a title, the implicit body and `extra`.
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

fn read(markdown: &str) -> (Presentation, Vec<Warning>) {
    let mut assets = MemoryAssetStore::new();
    let (document, report) = parse(markdown, &mut assets).expect("parses");
    match document {
        Document::Presentation(deck) => (deck, report.warnings),
        other => panic!("expected a presentation, got {other:?}"),
    }
}

/// What every round trip here checks: the document the writer produced from
/// the parsed IR is the document it was parsed from, byte for byte.
fn round_trips(document: &Document, options: &Options) -> String {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(document, &store, options);
    let mut assets = MemoryAssetStore::new();
    let (parsed, _) = parse(&markdown, &mut assets).expect("parses");
    let (again, _) = serialize(&parsed, &assets, options);
    assert_eq!(again, markdown, "round trip changed the document");
    markdown
}

#[test]
fn a_slide_heading_is_a_slide_and_a_title_placeholder() {
    let (deck, warnings) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 results {#n1 .slide layout=L1 section=Opening}\n\n\
         Revenue up 12 %\n",
    );

    assert_eq!(deck.slides.len(), 1);
    let slide = &deck.slides[0];
    assert_eq!(slide.id.as_ref().map(|id| id.as_str()), Some("n1"));
    assert_eq!(slide.layout, Some(LayoutId::new("L1")));
    assert_eq!(slide.section.as_deref(), Some("Opening"));
    assert!(!slide.hidden);

    // Rule 1 and rule 2: the heading is the title, the blocks under it are the
    // body, and neither is a container.
    assert_eq!(slide.shapes.len(), 2);
    assert_eq!(slide.title().as_deref(), Some("Q3 results"));
    match &slide.shapes[1].kind {
        ShapeKind::Placeholder(ph) => {
            assert_eq!(ph.ph_type, PhType::Body);
            assert_eq!(ph.body.len(), 1);
        }
        other => panic!("expected the implicit body, got {other:?}"),
    }
    // The slide's id is the slide's: the implicit shapes take none (§11.2).
    assert!(slide.shapes.iter().all(|shape| shape.id.is_none()));
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn the_body_placeholder_takes_the_index_its_layout_gives_it() {
    // Without this the writer would not recognise the shape as implicit and
    // would write it as a `::: {.ph}` container — the round trip below is what
    // actually proves it.
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\nlayouts:\n  \
         L1: {name: \"Title and Content\", title: 0, body: 1}\n---\n\n\
         ## Q3 results {#n1 .slide layout=L1}\n\n\
         Revenue up 12 %\n",
    );

    match &deck.slides[0].shapes[1].kind {
        ShapeKind::Placeholder(ph) => assert_eq!(ph.idx, Some(1)),
        other => panic!("expected the implicit body, got {other:?}"),
    }
}

#[test]
fn a_hidden_slide_and_a_named_one_come_back() {
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Cierre {#n1 .slide hidden=true name=\"Backup\"}\n",
    );

    let slide = &deck.slides[0];
    assert!(slide.hidden);
    assert_eq!(slide.name.as_deref(), Some("Backup"));
}

#[test]
fn a_heading_with_no_title_is_a_slide_without_one() {
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## {#n1 .slide}\n\nsólo cuerpo\n",
    );

    // Rule 1 writes `##` both for a slide whose title placeholder is empty and
    // for one that has none; the second is what comes back, and the two write
    // the same line.
    let slide = &deck.slides[0];
    assert_eq!(slide.shapes.len(), 1);
    assert!(slide.title().is_none());
}

#[test]
fn every_container_of_rule_4_comes_back_as_its_shape() {
    let (deck, warnings) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 results {#n1 .slide layout=L1}\n\n\
         Revenue up 12 %\n\n\
         ::: {#n2 .ph idx=2 type=subTitle}\nun subtítulo\n:::\n\n\
         ::: {#n3 .shape name=\"TextBox 4\" pos=\"10px,20px\" size=\"300px,40px\"}\n\
         al margen\n:::\n\n\
         ::: {#n4 .shape geom=rightArrow raw=r7}\nCrece\n:::\n\n\
         ::: {#n5 .connector raw=r9}\n:::\n",
    );

    let shapes = &deck.slides[0].shapes;
    assert_eq!(shapes.len(), 6);

    match &shapes[2].kind {
        ShapeKind::Placeholder(ph) => {
            assert_eq!(ph.ph_type, PhType::Subtitle);
            assert_eq!(ph.idx, Some(2));
        }
        other => panic!("expected a placeholder, got {other:?}"),
    }
    // A `.shape` with nothing to declare is a text box; one that names its
    // preset or its markup is the stub of rule 8.
    assert!(matches!(shapes[3].kind, ShapeKind::TextBox { .. }));
    assert_eq!(shapes[3].name.as_deref(), Some("TextBox 4"));
    assert_eq!(
        shapes[3].geometry.pos,
        Some(Point::new(Length::from_px(10.0), Length::from_px(20.0)))
    );
    assert_eq!(
        shapes[3].geometry.size,
        Some(Size::new(Length::from_px(300.0), Length::from_px(40.0)))
    );
    match &shapes[4].kind {
        ShapeKind::Raw(raw) => {
            assert_eq!(raw.kind, RawShapeKind::Shape);
            // The label a stub shows is not swallowed by the stub.
            assert_eq!(raw.text, "Crece");
            assert_eq!(raw.raw.as_ref().map(|id| id.as_str()), Some("r7"));
        }
        other => panic!("expected a raw shape, got {other:?}"),
    }
    assert_eq!(shapes[4].geometry.preset.as_deref(), Some("rightArrow"));
    match &shapes[5].kind {
        ShapeKind::Raw(raw) => assert_eq!(raw.kind, RawShapeKind::Connector),
        other => panic!("expected a connector, got {other:?}"),
    }
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_picture_a_table_and_a_group_come_back_as_shapes() {
    let (deck, warnings) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 results {#n1 .slide}\n\n\
         ![Revenue chart](assets/img-1.png){#n2 height=240px name=\"Picture 3\" \
         pos=\"88px,20px\" width=320px}\n\n\
         ::: {#n3 .table pos=\"88px,20px\" size=\"320px,240px\"}\n\
         | Region | Revenue |\n\
         | ------ | ------- |\n\
         | EMEA   | 12 M    |\n\
         :::\n\n\
         ::: {#n4 .group}\n\
         ::: {#n5 .shape}\nizquierda\n:::\n\n\
         ::: {#n6 .shape}\nderecha\n:::\n\
         :::\n",
    );

    let shapes = &deck.slides[0].shapes;
    assert_eq!(shapes.len(), 4);

    // The image line carries the *shape's* address and position, and the size
    // stays the image's own — it is never written twice (§11.2).
    assert_eq!(shapes[1].id.as_ref().map(|id| id.as_str()), Some("n2"));
    assert_eq!(
        shapes[1].geometry.pos,
        Some(Point::new(Length::from_px(88.0), Length::from_px(20.0)))
    );
    assert!(shapes[1].geometry.size.is_none());
    match &shapes[1].kind {
        ShapeKind::Picture(image) => {
            assert_eq!(image.alt, "Revenue chart");
            assert_eq!(image.geometry.display_size.width, Length::from_px(320.0));
            // The address is the shape's, not the picture's inside it.
            assert!(image.id.is_none());
        }
        other => panic!("expected a picture, got {other:?}"),
    }

    match &shapes[2].kind {
        ShapeKind::Table(table) => {
            assert_eq!(table.rows.len(), 2);
            // One addressable node, not two.
            assert!(table.id.is_none());
        }
        other => panic!("expected a table, got {other:?}"),
    }
    assert_eq!(shapes[2].id.as_ref().map(|id| id.as_str()), Some("n3"));

    match &shapes[3].kind {
        ShapeKind::Group(children) => {
            assert_eq!(children.len(), 2);
            assert_eq!(children[1].id.as_ref().map(|id| id.as_str()), Some("n6"));
        }
        other => panic!("expected a group, got {other:?}"),
    }
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_chart_stub_keeps_what_it_says_and_where_its_numbers_are() {
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 results {#n1 .slide}\n\n\
         ::: {#n2 .chart data=\"assets/book-1.xlsx\" kind=barChart raw=r5}\n\
         Revenue by region\n\
         :::\n",
    );

    match &deck.slides[0].shapes[1].kind {
        ShapeKind::Chart(chart) => {
            assert_eq!(chart.kind.as_deref(), Some("barChart"));
            assert_eq!(chart.title.as_deref(), Some("Revenue by region"));
            assert_eq!(chart.raw.as_ref().map(|id| id.as_str()), Some("r5"));
            // The workbook is a file beside the document; there is none here,
            // and a missing asset is not an invented one.
            assert!(chart.workbook.is_none());
        }
        other => panic!("expected a chart, got {other:?}"),
    }
}

#[test]
fn notes_are_read_in_both_syntaxes() {
    let container = "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 results {#n1 .slide}\n\n\
         Revenue up 12 %\n\n\
         ::: {.notes}\nOpen with the churn number.\n:::\n";
    let quoted = "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 results {.slide}\n\n\
         Revenue up 12 %\n\n\
         > Open with the churn number.\n";

    for markdown in [container, quoted] {
        let (deck, _) = read(markdown);
        let notes = deck.slides[0].notes.as_ref().expect("notes");
        assert_eq!(notes.len(), 1, "{markdown}");
        match &notes[0] {
            Block::Paragraph(p) => assert_eq!(p.plain_text(), "Open with the churn number."),
            other => panic!("expected a paragraph, got {other:?}"),
        }
        // The notes are not a shape: the slide holds them.
        assert_eq!(deck.slides[0].shapes.len(), 2, "{markdown}");
    }
}

#[test]
fn a_multi_block_blockquote_keeps_its_shape() {
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 {.slide}\n\n\
         > Primero el número.\n>\n> - churn\n> - margen\n",
    );

    let notes = deck.slides[0].notes.as_ref().expect("notes");
    assert_eq!(notes.len(), 2);
    assert!(matches!(notes[0], Block::Paragraph(_)));
    assert!(matches!(notes[1], Block::List(_)));
}

#[test]
fn an_empty_notes_container_is_an_empty_notes_page() {
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Q3 {#n1 .slide}\n\n::: {.notes}\n:::\n",
    );
    // `Some(vec![])` and `None` are different documents to the writer that
    // puts the package back.
    assert_eq!(deck.slides[0].notes, Some(Vec::new()));

    let (without, _) =
        read("---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n## Q3 {#n1 .slide}\n");
    assert_eq!(without.slides[0].notes, None);
}

#[test]
fn a_deck_typed_by_hand_parses_with_no_attributes_at_all() {
    // Analysis §6.6: the tolerant-input claim. The only thing the file says is
    // that it is a deck — nothing else here was written by the serialiser: no
    // `docmark:` version, no `.slide`, no container, no attribute anywhere.
    let (deck, warnings) = read(
        "---\nsource-format: pptx\n---\n\n\
         ## Informe trimestral\n\n\
         - Ingresos al alza\n\
         - Costes estables\n\n\
         ## Siguientes pasos\n\n\
         Cerrar el trimestre\n",
    );

    assert_eq!(deck.slides.len(), 2);
    assert_eq!(
        deck.slides[0].title().as_deref(),
        Some("Informe trimestral")
    );
    match &deck.slides[0].shapes[1].kind {
        ShapeKind::Placeholder(ph) => {
            assert_eq!(ph.ph_type, PhType::Body);
            assert!(matches!(ph.body.as_slice(), [Block::List(_)]));
        }
        other => panic!("expected the implicit body, got {other:?}"),
    }
    assert!(deck.slides[0].layout.is_none());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn content_before_the_first_heading_is_a_slide_of_its_own() {
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         una nota suelta\n\n## Primera {.slide}\n\ncuerpo\n",
    );

    assert_eq!(deck.slides.len(), 2);
    assert!(deck.slides[0].title().is_none());
    assert_eq!(deck.slides[0].shapes.len(), 1);
    assert_eq!(deck.slides[1].title().as_deref(), Some("Primera"));
}

#[test]
fn a_heading_inside_a_container_is_content_and_not_a_slide() {
    let (deck, _) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Primera {.slide}\n\n\
         ::: {.shape}\n## no es una diapositiva\n:::\n",
    );

    assert_eq!(deck.slides.len(), 1);
    assert!(matches!(
        deck.slides[0].shapes[1].kind,
        ShapeKind::TextBox { .. }
    ));
}

#[test]
fn an_unknown_container_keeps_its_text_and_says_what_it_did() {
    let (deck, warnings) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Primera {.slide}\n\n::: {.carousel}\nno lo conozco\n:::\n",
    );

    // Tolerance is not silence: the text survives and the reader is told.
    assert!(matches!(
        deck.slides[0].shapes[1].kind,
        ShapeKind::TextBox { .. }
    ));
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::UnsupportedElement { kind, .. } if kind == ".carousel"
        )),
        "{warnings:?}"
    );
}

#[test]
fn an_unclosed_container_is_an_error_naming_its_line() {
    let mut assets = MemoryAssetStore::new();
    let error = parse(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
         ## Primera {.slide}\n\n::: {.shape}\nsin cerrar\n",
        &mut assets,
    )
    .expect_err("an unclosed container is not readable");

    let message = error.to_string();
    assert!(message.contains("unclosed"), "{message}");
    assert!(message.contains("line 6"), "{message}");
}

#[test]
fn a_missing_skeleton_is_warned_rather_than_invented() {
    let (deck, warnings) = read(
        "---\ndocmark: \"1.2\"\nsource-format: pptx\nskeleton: assets/_skeleton/deck-abc.pptx\n---\n\n\
         ## Primera {.slide}\n",
    );

    assert!(deck.skeleton.is_none());
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, Warning::AssetIssue { .. })),
        "{warnings:?}"
    );
}

#[test]
fn what_the_writer_writes_at_full_is_what_the_parser_reads_back() {
    let shape = Shape {
        id: None,
        name: Some("Arrow 1".into()),
        z_index: 7,
        geometry: ShapeGeometry {
            pos: Some(Point::new(Length::from_px(88.0), Length::from_px(20.0))),
            size: Some(Size::new(Length::from_px(320.0), Length::from_px(40.0))),
            preset: Some("rightArrow".into()),
            rotation_deg: 90.0,
            ..Default::default()
        },
        kind: ShapeKind::Raw(RawShape {
            kind: RawShapeKind::Shape,
            raw: None,
            text: "Crece".into(),
        }),
    };
    let mut document = deck(vec![shape]);
    if let Document::Presentation(presentation) = &mut document {
        presentation.slides[0].notes = Some(vec![Block::Paragraph(Paragraph::text("el número"))]);
        presentation.slides[0].section = Some("Apertura".into());
    }

    let markdown = round_trips(&document, &options(Fidelity::Full));
    assert!(markdown.contains("geom=rightArrow"), "{markdown}");
    assert!(markdown.contains("rotation=90"), "{markdown}");
}

#[test]
fn what_the_writer_writes_at_standard_is_what_the_parser_reads_back() {
    let mut document = deck(vec![placeholder(PhType::Subtitle, Some(2), "un subtítulo")]);
    if let Document::Presentation(presentation) = &mut document {
        presentation.slides[0].notes = Some(vec![Block::Paragraph(Paragraph::text("el número"))]);
    }

    // The level whose notes are a blockquote and whose slides carry no id: a
    // round trip here is what proves both syntaxes are read (rule 5).
    let markdown = round_trips(&document, &standard());
    assert!(markdown.contains("> el número"), "{markdown}");
    assert!(!markdown.contains(".notes"), "{markdown}");
}

#[test]
fn a_deck_at_agent_round_trips_with_its_addresses() {
    let document = deck(vec![placeholder(PhType::Subtitle, Some(2), "un subtítulo")]);
    let markdown = round_trips(&document, &options(Fidelity::Agent));

    let mut assets = MemoryAssetStore::new();
    let (parsed, _) = parse(&markdown, &mut assets).expect("parses");
    let Document::Presentation(parsed) = parsed else {
        panic!("expected a presentation");
    };
    // Every id the document carried is still addressing the same node, which
    // is the whole reason `agent` exists.
    assert_eq!(
        parsed.slides[0].id.as_ref().map(|id| id.as_str()),
        Some("n1")
    );
    assert_eq!(
        parsed.slides[0].shapes[2].id.as_ref().map(|id| id.as_str()),
        Some("n2")
    );
    assert!(parsed.addressing.next_id >= 3);
}

#[test]
fn a_plain_deck_is_a_text_document_and_stays_one() {
    // `plain` writes no front matter at all and no `.slide`, so nothing in the
    // file says «deck». That is the level being what it claims to be: a
    // one-way projection for reading, not a document that goes back
    // (spec §6). The text is all there; the slides are not.
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(Vec::new()), &store, &options(Fidelity::Plain));
    let mut assets = MemoryAssetStore::new();
    let (parsed, _) = parse(&markdown, &mut assets).expect("parses");

    assert!(parsed.is_text(), "{markdown}");
}

#[test]
fn markdown_that_says_nothing_about_slides_is_a_text_document() {
    // The other side of the tolerant rule: `##` alone is a heading. A deck is
    // what the front matter or a `.slide` marker says is one — guessing from
    // heading levels would turn every report into a presentation.
    let mut assets = MemoryAssetStore::new();
    let (parsed, _) = parse("## Informe trimestral\n\n- Ingresos\n", &mut assets).expect("parses");

    assert!(parsed.is_text());
}
