//! What a slide holds that is not text: pictures, tables, groups and the stubs
//! for objects Markdown has no form for (spec §11.2 rules 4, 6 and 8; plan v2
//! Phase 14 increment F).
//!
//! The containers of increment D are in `presentation_shapes.rs`; this file is
//! about the three kinds that are *not* containers and the ones that are
//! nothing but a stub.

use docsai_docmark::{parse, serialize, Fidelity, Options};
use docsai_model::addressing::IdPolicy;
use docsai_model::assets::AssetStore;
use docsai_model::image::{ImageGeometry, ImageRef, RawId};
use docsai_model::presentation::{
    ChartRef, ChartSeries, Layout, LayoutId, LayoutPlaceholder, PhType, Placeholder, Presentation,
    Shape, ShapeGeometry, ShapeKind, Slide,
};
use docsai_model::text::{Block, Paragraph, Table, TableCell, TableRow};
use docsai_model::units::{Length, Point, Size};
use docsai_model::{Document, Format, MemoryAssetStore, Warning};

/// A PNG header, which is all the asset store needs to name and sniff a file.
const PNG: &[u8] =
    b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x78\x00\x00\x00\x5a\x08\x02\x00\x00\x00";

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

/// The geometry every object on these slides is placed with.
fn placed() -> ShapeGeometry {
    ShapeGeometry {
        pos: Some(Point::new(Length::from_px(88.0), Length::from_px(20.0))),
        size: Some(Size::new(Length::from_px(320.0), Length::from_px(240.0))),
        ..Default::default()
    }
}

/// A picture shape, and the path the document will refer to it by.
fn picture(store: &mut MemoryAssetStore) -> (Shape, String) {
    let asset = store.put(PNG).expect("stores the picture");
    let path = format!("assets/{}", store.info(&asset).expect("manifest").file_name);
    let mut image = ImageRef::new(
        asset,
        ImageGeometry::inline(Size::new(Length::from_px(320.0), Length::from_px(240.0))),
    );
    image.alt = "Revenue chart".into();
    image.name = Some("Picture 3".into());
    let shape = Shape {
        id: None,
        name: Some("Picture 3".into()),
        z_index: 3,
        geometry: placed(),
        kind: ShapeKind::Picture(image),
    };
    (shape, path)
}

fn table_shape() -> Shape {
    let row = |cells: [&str; 2]| TableRow {
        cells: cells
            .iter()
            .map(|text| TableCell {
                blocks: vec![Block::Paragraph(Paragraph::text(*text))],
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let table = Table {
        rows: vec![row(["Region", "Revenue"]), row(["EMEA", "12 M"])],
        header_row: true,
        ..Default::default()
    };
    Shape {
        id: None,
        name: Some("Table 4".into()),
        z_index: 4,
        geometry: placed(),
        kind: ShapeKind::Table(table),
    }
}

fn chart_shape(workbook: Option<docsai_model::assets::AssetId>) -> Shape {
    Shape {
        id: None,
        name: Some("Chart 5".into()),
        z_index: 5,
        geometry: ShapeGeometry::default(),
        kind: ShapeKind::Chart(ChartRef {
            kind: Some("barChart".into()),
            title: Some("Revenue by region".into()),
            workbook,
            raw: Some(RawId::new("r5")),
            ..ChartRef::default()
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
fn a_picture_is_an_image_line_carrying_its_shapes_id() {
    let mut store = MemoryAssetStore::new();
    let (shape, path) = picture(&mut store);
    let (markdown, report) = serialize(&deck(vec![shape]), &store, &options(Fidelity::Full));

    // No container: Markdown has an image. The size is the image's own, the
    // position is the shape's, and neither is written twice.
    assert_eq!(
        body_of(&markdown),
        format!(
            "## Q3 results {{#n1 .slide layout=L1}}\n\n\
             Revenue up 12 %\n\n\
             ![Revenue chart]({path})\
             {{#n2 height=240px name=\"Picture 3\" pos=\"88px,20px\" width=320px}}\n"
        ),
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.stats.images, 1);
}

#[test]
fn standard_writes_the_picture_and_none_of_its_measurements() {
    let mut store = MemoryAssetStore::new();
    let (shape, path) = picture(&mut store);
    let (markdown, report) = serialize(&deck(vec![shape]), &store, &standard());

    // Spike P2 §3.3: `{width=… height=…}` is residue a viewer cannot use — it
    // draws the picture at its own size regardless.
    assert_eq!(
        body_of(&markdown),
        format!(
            "## Q3 results {{.slide}}\n\n\
             Revenue up 12 %\n\n\
             ![Revenue chart]({path})\n"
        ),
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_document_keeps_the_image_size_that_a_slide_drops() {
    // The rule is about slides, not about `standard`: a figure in a text
    // document is still written with its size at the same level.
    let mut store = MemoryAssetStore::new();
    let asset = store.put(PNG).expect("stores the picture");
    let image = ImageRef::new(
        asset,
        ImageGeometry::inline(Size::new(Length::from_px(320.0), Length::from_px(240.0))),
    );
    let document = Document::Text(docsai_model::text::TextDocument {
        sections: vec![docsai_model::text::Section {
            blocks: vec![Block::Image(image)],
            ..Default::default()
        }],
        ..Default::default()
    });
    let opts = Options {
        source_format: Format::Docx,
        ..standard()
    };
    let (markdown, _) = serialize(&document, &store, &opts);
    assert!(markdown.contains("width=320px"), "{markdown}");
}

#[test]
fn agent_keeps_where_the_picture_is_and_not_how_big_it_is() {
    let mut store = MemoryAssetStore::new();
    let (shape, path) = picture(&mut store);
    let (markdown, _) = serialize(&deck(vec![shape]), &store, &options(Fidelity::Agent));

    assert!(
        body_of(&markdown).contains(&format!(
            "![Revenue chart]({path}){{#n2 name=\"Picture 3\" pos=\"88px,20px\"}}"
        )),
        "{markdown}"
    );
}

#[test]
fn a_table_shape_is_a_gfm_table_with_the_shapes_id() {
    let store = MemoryAssetStore::new();
    let (markdown, report) =
        serialize(&deck(vec![table_shape()]), &store, &options(Fidelity::Full));

    // The walk addresses the shape, not the table inside it, so there is one
    // id here and not two.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results {#n1 .slide layout=L1}\n\n\
         Revenue up 12 %\n\n\
         ::: {#n2 .table name=\"Table 4\" pos=\"88px,20px\" size=\"320px,240px\"}\n\
         | Region | Revenue |\n\
         | ------ | ------- |\n\
         | EMEA   | 12 M    |\n\
         :::\n",
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.stats.tables, 1);
    assert_eq!(ids_in_order(&markdown), vec!["n1", "n2"]);
}

#[test]
fn standard_writes_the_table_bare() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(vec![table_shape()]), &store, &standard());

    assert_eq!(
        body_of(&markdown),
        "## Q3 results {.slide}\n\n\
         Revenue up 12 %\n\n\
         | Region | Revenue |\n\
         | ------ | ------- |\n\
         | EMEA   | 12 M    |\n",
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_table_with_no_rows_is_warned_rather_than_written() {
    let store = MemoryAssetStore::new();
    let mut shape = table_shape();
    shape.kind = ShapeKind::Table(Table::default());
    let (markdown, report) = serialize(&deck(vec![shape]), &store, &options(Fidelity::Full));

    // A header rule over nothing is not a table; the loss is typed.
    assert!(!markdown.contains('|'), "{markdown}");
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            Warning::UnsupportedElement { kind, .. } if kind == "table"
        )),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_group_is_a_container_and_its_children_keep_their_addresses() {
    let store = MemoryAssetStore::new();
    let group = Shape {
        id: None,
        name: Some("Group 6".into()),
        z_index: 6,
        geometry: placed(),
        kind: ShapeKind::Group(vec![
            Shape::new(
                7,
                ShapeKind::TextBox {
                    body: vec![Block::Paragraph(Paragraph::text("izquierda"))],
                },
            ),
            Shape::new(
                8,
                ShapeKind::TextBox {
                    body: vec![Block::Paragraph(Paragraph::text("derecha"))],
                },
            ),
        ]),
    };
    let (markdown, report) = serialize(&deck(vec![group]), &store, &options(Fidelity::Full));

    assert_eq!(
        body_of(&markdown),
        "## Q3 results {#n1 .slide layout=L1}\n\n\
         Revenue up 12 %\n\n\
         ::: {#n2 .group name=\"Group 6\" pos=\"88px,20px\" size=\"320px,240px\"}\n\
         ::: {#n3 .shape}\n\
         izquierda\n\
         :::\n\n\
         ::: {#n4 .shape}\n\
         derecha\n\
         :::\n\
         :::\n",
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // The group takes its id before its children, which is the order the
    // addressing walk visits them in.
    assert_eq!(ids_in_order(&markdown), vec!["n1", "n2", "n3", "n4"]);
}

#[test]
fn plain_flattens_a_group_into_what_it_says() {
    let store = MemoryAssetStore::new();
    let group = Shape::new(
        6,
        ShapeKind::Group(vec![Shape::new(
            7,
            ShapeKind::TextBox {
                body: vec![Block::Paragraph(Paragraph::text("izquierda"))],
            },
        )]),
    );
    let (markdown, _) = serialize(&deck(vec![group]), &store, &options(Fidelity::Plain));

    assert_eq!(
        body_of(&markdown),
        "## Q3 results\n\nRevenue up 12 %\n\nizquierda\n",
        "{markdown}"
    );
}

#[test]
fn a_chart_is_a_stub_naming_its_kind_and_where_its_numbers_are() {
    let mut store = MemoryAssetStore::new();
    let workbook = store
        .put(b"PK\x03\x04 not really a workbook")
        .expect("stores");
    let path = format!(
        "assets/{}",
        store.info(&workbook).expect("manifest").file_name
    );
    let (markdown, report) = serialize(
        &deck(vec![chart_shape(Some(workbook))]),
        &store,
        &options(Fidelity::Full),
    );

    // A chart the reader could not resolve into data is the stub it always
    // was: an empty table would claim it plots nothing. Its title is the body,
    // because a title is what a reader can see.
    assert_eq!(
        body_of(&markdown),
        format!(
            "## Q3 results {{#n1 .slide layout=L1}}\n\n\
             Revenue up 12 %\n\n\
             ::: {{#n2 .chart data=\"{path}\" kind=barChart name=\"Chart 5\" raw=r5}}\n\
             Revenue by region\n\
             :::\n"
        ),
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn standard_keeps_the_chart_and_loses_its_payload() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(vec![chart_shape(None)]), &store, &standard());

    assert!(
        body_of(&markdown).contains("::: {.chart kind=barChart}\nRevenue by region\n:::"),
        "{markdown}"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::RawBlockDropped { id, .. } if id == "r5")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn plain_keeps_what_a_chart_says_and_says_it_lost_the_chart() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(
        &deck(vec![chart_shape(None)]),
        &store,
        &options(Fidelity::Plain),
    );

    assert_eq!(
        body_of(&markdown),
        "## Q3 results\n\nRevenue up 12 %\n\nRevenue by region\n",
        "{markdown}"
    );

    // A chart with no title at all leaves nothing behind, and that is a loss.
    let mut untitled = chart_shape(None);
    if let ShapeKind::Chart(chart) = &mut untitled.kind {
        chart.title = None;
    }
    let (_, report) = serialize(&deck(vec![untitled]), &store, &options(Fidelity::Plain));
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            Warning::UnsupportedElement { kind, .. } if kind == "chart"
        )),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_slide_of_objects_is_byte_deterministic() {
    let mut store = MemoryAssetStore::new();
    let (picture, _) = picture(&mut store);
    let document = deck(vec![picture, table_shape(), chart_shape(None)]);
    let (first, _) = serialize(&document, &store, &options(Fidelity::Full));
    let (second, _) = serialize(&document, &store, &options(Fidelity::Full));
    assert_eq!(first, second);
}

// --------------------------------------------------------------------------
// The chart's data table (spec §11.2; plan v2 Phase 16 increment B)
// --------------------------------------------------------------------------

/// A chart that plots something: two series over three categories, a hole in
/// the second, and a format code on one of them.
fn plotting_chart() -> Shape {
    let mut shape = chart_shape(None);
    if let ShapeKind::Chart(chart) = &mut shape.kind {
        chart.categories = vec!["Q1".into(), "Q2".into(), "Q3".into()];
        chart.series = vec![
            ChartSeries {
                name: Some("North".into()),
                values: vec![Some(1200.0), Some(980.0), Some(1440.5)],
                number_format: Some("#,##0".into()),
            },
            ChartSeries {
                name: Some("South".into()),
                values: vec![Some(800.0), None, Some(910.0)],
                number_format: None,
            },
        ];
    }
    shape
}

/// The `ChartRef` a deck's single object shape holds.
fn chart_of(document: &Document) -> &ChartRef {
    let Document::Presentation(deck) = document else {
        panic!("not a deck");
    };
    deck.slides
        .iter()
        .flat_map(|slide| &slide.shapes)
        .find_map(|shape| match &shape.kind {
            ShapeKind::Chart(chart) => Some(chart),
            _ => None,
        })
        .expect("a chart")
}

#[test]
fn a_chart_writes_its_series_as_a_table() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(
        &deck(vec![plotting_chart()]),
        &store,
        &options(Fidelity::Full),
    );

    // The title first — it is what the object *shows* — then the table it
    // plots. The first header cell is empty because the category axis has no
    // title in the model, and the gap in South is an empty cell: a `0` there
    // would draw a line through the floor.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results {#n1 .slide layout=L1}\n\n\
         Revenue up 12 %\n\n\
         ::: {#n2 .chart kind=barChart name=\"Chart 5\" raw=r5}\n\
         Revenue by region\n\n\
         |     | North {format=\"#,##0\"} | South |\n\
         | --- | ---------------------- | ----- |\n\
         | Q1  | 1200                   | 800   |\n\
         | Q2  | 980                    |       |\n\
         | Q3  | 1440.5                 | 910   |\n\
         :::\n",
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn standard_writes_the_table_and_not_the_number_formats() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(vec![plotting_chart()]), &store, &standard());

    // `format=` is the one attribute block that would show *inside* a rendered
    // table cell, and `standard` is the level a plain viewer reads.
    assert!(
        body_of(&markdown).contains(
            "|     | North  | South |\n\
             | --- | ------ | ----- |\n\
             | Q1  | 1200   | 800   |\n"
        ),
        "{markdown}"
    );
    assert!(!markdown.contains("format="), "{markdown}");
}

#[test]
fn plain_still_writes_only_what_the_chart_shows() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(
        &deck(vec![plotting_chart()]),
        &store,
        &options(Fidelity::Plain),
    );

    // `plain` writes no container, so there is nowhere for the table to go —
    // and a bare GFM table of numbers with no box around it is not what the
    // slide shows either.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results\n\nRevenue up 12 %\n\nRevenue by region\n",
        "{markdown}"
    );
}

#[test]
fn a_chart_round_trips_through_docmark() {
    let mut store = MemoryAssetStore::new();
    let document = deck(vec![plotting_chart()]);
    let (markdown, _) = serialize(&document, &store, &options(Fidelity::Full));
    let (back, report) = parse(&markdown, &mut store).expect("parses");

    assert_eq!(chart_of(&back), chart_of(&document));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn a_category_less_chart_round_trips_as_one() {
    let mut store = MemoryAssetStore::new();
    let mut shape = plotting_chart();
    if let ShapeKind::Chart(chart) = &mut shape.kind {
        chart.categories.clear();
    }
    let document = deck(vec![shape]);
    let (markdown, _) = serialize(&document, &store, &options(Fidelity::Full));

    // The first column is still there and is empty from top to bottom: one
    // rule — trailing empty category cells are not categories — covers both a
    // scatter and a series longer than its axis.
    assert!(
        markdown.contains(
            "|     | North {format=\"#,##0\"} | South |\n\
             | --- | ---------------------- | ----- |\n\
             |     | 1200                   | 800   |\n"
        ),
        "{markdown}"
    );
    let (back, _) = parse(&markdown, &mut store).expect("parses");
    assert_eq!(chart_of(&back), chart_of(&document));
}

#[test]
fn a_chart_table_typed_by_hand_parses() {
    let mut store = MemoryAssetStore::new();
    // No front matter, no ids, no `kind=`, no padding, no leading pipes: a
    // person writing DocMark in an editor. Input is tolerant (spec §11.2).
    let markdown = "## Sales {.slide}\n\n\
                    ::: {.chart}\n\
                    Sales by quarter\n\n\
                    Quarter | North | South\n\
                    --- | --- | ---\n\
                    Q1 | 1200 | 800\n\
                    Q2 | 980 |\n\
                    :::\n";
    let (document, report) = parse(markdown, &mut store).expect("parses");
    let chart = chart_of(&document);

    assert_eq!(chart.title.as_deref(), Some("Sales by quarter"));
    assert_eq!(chart.categories, vec!["Q1".to_string(), "Q2".to_string()]);
    assert_eq!(chart.series.len(), 2);
    assert_eq!(chart.series[0].name.as_deref(), Some("North"));
    assert_eq!(chart.series[0].values, vec![Some(1200.0), Some(980.0)]);
    // An empty cell is a gap, and a row that stops short is the same thing.
    assert_eq!(chart.series[1].values, vec![Some(800.0), None]);

    // The axis label is the one thing a hand-typed table can carry that the
    // model cannot hold, so it is dropped — and said.
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            Warning::Degraded { why, .. } if why.contains("`Quarter`")
        )),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_value_that_is_not_a_number_is_a_gap_and_a_warning() {
    let mut store = MemoryAssetStore::new();
    let markdown = "## Sales {.slide}\n\n\
                    ::: {.chart}\n\
                    |  | North |\n\
                    | --- | --- |\n\
                    | Q1 | 1.200,00 |\n\
                    | Q2 | 980 |\n\
                    :::\n";
    let (document, report) = parse(markdown, &mut store).expect("parses");

    // Never a silent zero: a zero plots a point the author did not write.
    assert_eq!(
        chart_of(&document).series[0].values,
        vec![None, Some(980.0)]
    );
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            Warning::UnsupportedElement { kind, .. } if kind == "chart value `1.200,00`"
        )),
        "{:?}",
        report.warnings
    );
}
