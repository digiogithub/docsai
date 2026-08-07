//! Phase 0 acceptance criterion: the IR serializes to JSON and back
//! identically. Checked with hand-written documents covering every node kind
//! and with `proptest` over randomly generated IRs.

use docsai_model::assets::AssetId;
use docsai_model::image::*;
use docsai_model::list::*;
use docsai_model::sheet::*;
use docsai_model::style::*;
use docsai_model::text::*;
use docsai_model::units::{Length, Point, Size};
use docsai_model::Document;
use proptest::prelude::*;

fn assert_json_round_trip(doc: &Document) {
    let json = serde_json::to_string(doc).expect("serializes");
    let back: Document = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(doc, &back, "round-trip changed the document\n{json}");
    // Serialising the reconstructed value must produce the very same bytes.
    assert_eq!(json, serde_json::to_string(&back).unwrap());
}

#[test]
fn round_trips_a_document_using_every_block_kind() {
    let asset = AssetId::new("deadbeefcafe0001");
    let mut geometry = ImageGeometry::inline(Size::new(Length::from_cm(4.0), Length::from_cm(3.0)));
    geometry.anchor = Anchor::Floating {
        relative_to_h: RelBase::Margin,
        relative_to_v: RelBase::Paragraph,
        position: HVPos {
            h: AxisPos::Offset(Length::from_cm(1.2)),
            v: AxisPos::Align(AlignKeyword::Top),
        },
        wrap: WrapMode::Square,
        wrap_side: WrapSide::Right,
        behind_text: false,
    };
    geometry.rotation_deg = 45.0;
    geometry.flip = Flip::HV;
    geometry.crop = Some(CropRect {
        left: 10.0,
        top: 5.0,
        right: 20.0,
        bottom: 0.0,
    });
    geometry.border = Some(SimpleBorder {
        width: Length::from_pt(1.0),
        style: "solid".into(),
        color: "#000000".into(),
    });
    geometry.z_index = Some(2);
    geometry.native_size_px = Some((800, 600));
    geometry.dpi = Some(300);

    let mut image = ImageRef::new(asset, geometry);
    image.alt = "Diagrama".into();
    image.title = Some("Figura 1".into());
    image.name = Some("Logo".into());
    image.link = Some("https://example.com".into());
    image.effects_raw = Some(RawId::new("raw-0001"));

    let mut styles = StyleCatalog::default();
    styles.defaults.font.size = Some(Length::from_pt(11.0));
    let mut heading = Style::new("Heading1", StyleType::Paragraph);
    heading.based_on = Some(StyleId::new("Normal"));
    heading.font.color = Some("#2E74B5".into());
    heading.paragraph.outline_level = Some(0);
    heading.paragraph.line_height = Some(LineHeight::Multiple(240));
    styles.insert(heading);

    let mut list_defs = ListCatalog::default();
    list_defs.insert(
        ListId::new("L1"),
        ListDef {
            levels: vec![
                ListLevel {
                    format: NumFormat::Decimal,
                    text: "%1.".into(),
                    start: Some(1),
                    indent: Some(Length::from_twips(720)),
                    hanging: Some(Length::from_twips(360)),
                },
                ListLevel::new(NumFormat::Other("ideographDigital".into()), "%2"),
            ],
        },
    );

    let doc = Document::Text(TextDocument {
        addressing: Default::default(),
        meta: DocumentMeta {
            title: Some("Informe".into()),
            author: Some("Ana".into()),
            created: Some("2026-03-01T10:00:00Z".into()),
            language: Some("es-ES".into()),
            custom: [("Departamento".to_string(), "Ventas".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        },
        styles,
        list_defs,
        sections: vec![Section {
            id: None,
            page: PageGeometry {
                size: Size::new(Length::from_twips(11906), Length::from_twips(16838)),
                margins: Margins {
                    top: Length::from_twips(1417),
                    right: Length::from_twips(1701),
                    bottom: Length::from_twips(1417),
                    left: Length::from_twips(1701),
                    header: Length::from_twips(708),
                    footer: Length::from_twips(708),
                },
                orientation: Orientation::Landscape,
                columns: 2,
                title_page: true,
            },
            headers: vec![HeaderFooter {
                scope: HeaderScope::Default,
                blocks: vec![Block::Paragraph(Paragraph::text("Cabecera"))],
            }],
            footers: vec![HeaderFooter {
                scope: HeaderScope::First,
                blocks: vec![Block::Paragraph(Paragraph::new(vec![Inline::Field {
                    kind: FieldKind::Page,
                    cached: "1".into(),
                    instruction: " PAGE ".into(),
                }]))],
            }],
            blocks: vec![
                Block::Heading(Heading {
                    id: None,
                    level: 1,
                    paragraph: Paragraph {
                        id: None,
                        format: ParaFormat::styled("Heading1"),
                        content: vec![Inline::Text("Título".into())],
                    },
                }),
                Block::Paragraph(Paragraph::new(vec![
                    Inline::Styled {
                        content: vec![Inline::Text("negrita".into())],
                        props: RunProps::direct(FontProps {
                            bold: Some(true),
                            underline: Some(Underline::Wave),
                            vert_align: Some(VertAlign::Superscript),
                            ..Default::default()
                        }),
                    },
                    Inline::Link {
                        target: "https://example.com".into(),
                        content: vec![Inline::Text("enlace".into())],
                        props: RunProps {
                            style: Some(StyleId::new("Hyperlink")),
                            direct: FontProps::default(),
                        },
                    },
                    Inline::Break(BreakKind::Page),
                    Inline::Footnote(Footnote::new(vec![Block::Paragraph(Paragraph::text(
                        "nota",
                    ))])),
                    Inline::Image(Box::new(image.clone())),
                    Inline::Raw(RawFragment {
                        id: RawId::new("raw-0002"),
                        format: "ooxml".into(),
                        part: "word/document.xml".into(),
                        content: "<w:sdt/>".into(),
                    }),
                ])),
                Block::List(List {
                    id: None,
                    def: Some(ListId::new("L1")),
                    ordered: true,
                    level: 0,
                    items: vec![ListItem {
                        blocks: vec![
                            Block::Paragraph(Paragraph::text("uno")),
                            Block::List(List {
                                id: None,
                                def: Some(ListId::new("L1")),
                                ordered: false,
                                level: 1,
                                items: vec![ListItem {
                                    blocks: vec![Block::Paragraph(Paragraph::text("anidado"))],
                                }],
                            }),
                        ],
                    }],
                }),
                Block::Table(Table {
                    id: None,
                    style: Some(StyleId::new("TableGrid")),
                    col_widths: vec![Length::from_twips(2500), Length::from_twips(2500)],
                    header_row: true,
                    rows: vec![TableRow {
                        id: None,
                        cells: vec![
                            TableCell {
                                colspan: 2,
                                rowspan: 1,
                                background: Some("#F2F2F2".into()),
                                ..TableCell::text("combinada")
                            },
                            TableCell {
                                covered: true,
                                ..Default::default()
                            },
                        ],
                        is_header: true,
                    }],
                }),
                Block::Image(image),
                Block::TextBox(TextBox {
                    blocks: vec![Block::Paragraph(Paragraph::text("caja"))],
                    size: Some(Size::new(Length::from_cm(6.0), Length::from_cm(3.0))),
                    x: Some(Length::from_cm(5.0)),
                    y: Some(Length::from_cm(2.0)),
                }),
                Block::Raw(RawFragment {
                    id: RawId::new("raw-0003"),
                    format: "ooxml".into(),
                    part: "word/document.xml".into(),
                    content: "<w:customXml/>".into(),
                }),
            ],
        }],
    });

    assert_json_round_trip(&doc);
}

#[test]
fn round_trips_a_workbook_with_every_anchor_kind() {
    let asset = AssetId::new("deadbeefcafe0002");
    let sheet_image = |anchor: Anchor| {
        let mut g = ImageGeometry::inline(Size::new(Length::from_px(180.0), Length::from_px(60.0)));
        g.anchor = anchor;
        ImageRef::new(asset.clone(), g)
    };

    let mut sheet = Sheet::new("Ventas");
    sheet.cells.insert(
        CellRef::new(3, 1),
        Cell {
            value: CellValue::Number(300.0),
            formula: Some(Formula {
                text: "SUM(B2:C2)".into(),
                dialect: FormulaDialect::Ooxml,
                shared_over: Some(CellRange::new(CellRef::new(3, 1), CellRef::new(3, 3))),
                array_over: None,
            }),
            num_fmt: Some(NumFmt {
                code: "#,##0".into(),
                id: Some(3),
            }),
            style: Some(StyleId::new("HeaderRow")),
        },
    );
    sheet.cells.insert(
        CellRef::new(0, 0),
        Cell {
            value: CellValue::DateTime("2026-01-15T00:00:00".into()),
            ..Default::default()
        },
    );
    sheet.cells.insert(
        CellRef::new(1, 0),
        Cell {
            value: CellValue::Error("#DIV/0!".into()),
            ..Default::default()
        },
    );
    sheet
        .merges
        .push(CellRange::new(CellRef::new(0, 4), CellRef::new(2, 4)));
    sheet.cols.insert(
        0,
        ColProps {
            width_chars: Some(18.5),
            hidden: Some(false),
        },
    );
    sheet.rows.insert(
        0,
        RowProps {
            height: Some(Length::from_pt(15.0)),
            hidden: None,
        },
    );
    sheet.pane = Some(Pane {
        top_left: CellRef::new(0, 1),
        frozen: true,
    });
    sheet.images = vec![
        sheet_image(Anchor::SheetTwoCell {
            from: CellAnchor::new(
                CellRef::new(1, 1),
                Length::from_px(12.0),
                Length::from_px(3.0),
            ),
            to: CellAnchor::new(CellRef::new(3, 7), Length::ZERO, Length::ZERO),
            move_with_cells: true,
            size_with_cells: false,
        }),
        sheet_image(Anchor::SheetOneCell {
            from: CellAnchor::new(CellRef::new(5, 19), Length::ZERO, Length::ZERO),
        }),
        sheet_image(Anchor::SheetAbsolute {
            pos: Point::new(Length::from_cm(5.0), Length::from_cm(8.0)),
        }),
    ];

    let doc = Document::Workbook(Workbook {
        addressing: Default::default(),
        meta: DocumentMeta {
            title: Some("Libro".into()),
            ..Default::default()
        },
        styles: StyleCatalog::default(),
        defined_names: vec![DefinedName {
            name: "TOTAL_ANUAL".into(),
            refers_to: "Ventas!$D$10".into(),
            sheet: None,
        }],
        sheets: vec![sheet],
        active_sheet: Some("Ventas".into()),
    });

    assert_json_round_trip(&doc);
}

#[test]
fn round_trips_a_presentation_using_every_shape_kind() {
    use docsai_model::presentation::*;

    let asset = AssetId::new("deadbeefcafe0003");
    let picture = ImageRef::new(
        asset.clone(),
        ImageGeometry::inline(Size::new(Length::from_cm(8.0), Length::from_cm(4.5))),
    );

    let mut layouts = LayoutCatalog::default();
    layouts.masters.insert(
        MasterId::new("M1"),
        Master {
            name: "Office Theme".into(),
            theme: Some("ppt/theme/theme1.xml".into()),
            placeholders: vec![LayoutPlaceholder {
                ph_type: PhType::Title,
                ..Default::default()
            }],
        },
    );
    layouts.layouts.insert(
        LayoutId::new("L1"),
        Layout {
            name: "Title and Content".into(),
            master: Some(MasterId::new("M1")),
            placeholders: vec![
                LayoutPlaceholder {
                    ph_type: PhType::Title,
                    geometry: ShapeGeometry::at(
                        Point::new(Length::from_emu(838_200), Length::from_emu(365_125)),
                        Size::new(Length::from_emu(10_515_600), Length::from_emu(1_325_563)),
                    ),
                    ..Default::default()
                },
                LayoutPlaceholder {
                    ph_type: PhType::Body,
                    idx: Some(1),
                    props: ShapeProps {
                        font: FontProps {
                            size: Some(Length::from_pt(18.0)),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
        },
    );

    let mut geometry = ShapeGeometry::at(
        Point::new(Length::from_emu(3_200_000), Length::from_emu(2_200_000)),
        Size::new(Length::from_emu(1_400_000), Length::from_emu(500_000)),
    );
    geometry.rotation_deg = 27.0;
    geometry.flip = Flip::H;

    let slide = Slide {
        id: Some(docsai_model::NodeId::new("n1")),
        layout: Some(LayoutId::new("L1")),
        name: Some("Resultados".into()),
        shapes: vec![
            Shape::new(
                0,
                ShapeKind::Placeholder(Placeholder {
                    ph_type: PhType::CenterTitle,
                    body: vec![Block::Paragraph(Paragraph::text("Resultados"))],
                    ..Default::default()
                }),
            ),
            Shape::new(
                1,
                ShapeKind::Placeholder(Placeholder {
                    ph_type: PhType::Body,
                    idx: Some(1),
                    body: vec![Block::Paragraph(Paragraph::text("Crecimiento del 12 %"))],
                    delta: ShapeProps {
                        fill: Some("#ffffff".into()),
                        font_scale: Some(92.5),
                        ..Default::default()
                    },
                }),
            ),
            Shape::new(2, ShapeKind::TextBox { body: vec![] }),
            Shape::new(3, ShapeKind::Picture(picture)),
            Shape::new(
                4,
                ShapeKind::Table(Table {
                    rows: vec![TableRow {
                        cells: vec![TableCell::text("Región")],
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            ),
            Shape::new(
                5,
                ShapeKind::Chart(ChartRef {
                    kind: Some("barChart".into()),
                    title: Some("Ingresos".into()),
                    workbook: Some(AssetId::new("deadbeefcafe0004")),
                    raw: Some(RawId::new("raw-0007")),
                }),
            ),
            Shape {
                id: None,
                name: Some("Arrow 1".into()),
                z_index: 6,
                geometry,
                kind: ShapeKind::Raw(RawShape {
                    kind: RawShapeKind::Connector,
                    raw: Some(RawId::new("raw-0008")),
                    text: "hacia el anexo".into(),
                }),
            },
            Shape::new(
                7,
                ShapeKind::Group(vec![Shape::new(
                    0,
                    ShapeKind::TextBox {
                        body: vec![Block::Paragraph(Paragraph::text("dentro"))],
                    },
                )]),
            ),
        ],
        notes: Some(vec![Block::Paragraph(Paragraph::text("hablar despacio"))]),
        hidden: true,
        section: Some("Cierre".into()),
        raw: vec![RawId::new("raw-0009")],
    };

    let doc = Document::Presentation(Presentation {
        meta: DocumentMeta {
            title: Some("Q3".into()),
            ..Default::default()
        },
        layouts,
        slide_size: Size::new(Length::from_emu(12_192_000), Length::from_emu(6_858_000)),
        slides: vec![slide, Slide::default()],
        skeleton: Some(SkeletonRef {
            asset: AssetId::new("deadbeefcafe0005"),
            rebuilt_parts: vec!["ppt/slides/slide1.xml".into()],
        }),
        ..Default::default()
    });

    assert_json_round_trip(&doc);
}

// --------------------------------------------------------------------------
// Property testing
// --------------------------------------------------------------------------

prop_compose! {
    fn arb_length()(emu in -10_000_000i64..10_000_000i64) -> Length {
        Length::from_emu(emu)
    }
}

fn arb_font() -> impl Strategy<Value = FontProps> {
    (
        proptest::option::of("[a-zA-Z ]{1,12}"),
        proptest::option::of(arb_length()),
        proptest::option::of(any::<bool>()),
        proptest::option::of(any::<bool>()),
        proptest::option::of("#[0-9A-F]{6}"),
    )
        .prop_map(|(name, size, bold, italic, color)| FontProps {
            name,
            size,
            bold,
            italic,
            color,
            ..Default::default()
        })
}

fn arb_inline() -> impl Strategy<Value = Inline> {
    let leaf = prop_oneof![
        ".{0,24}".prop_map(Inline::Text),
        Just(Inline::Break(BreakKind::Line)),
        Just(Inline::Break(BreakKind::Page)),
        (".{0,8}", ".{0,8}").prop_map(|(cached, instr)| Inline::Field {
            kind: FieldKind::from_instruction(&instr),
            cached,
            instruction: instr,
        }),
    ];
    leaf.prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            (proptest::collection::vec(inner.clone(), 0..3), arb_font()).prop_map(
                |(content, direct)| Inline::Styled {
                    content,
                    props: RunProps::direct(direct),
                }
            ),
            (
                "https?://[a-z]{1,8}\\.test",
                proptest::collection::vec(inner, 0..3)
            )
                .prop_map(|(target, content)| Inline::Link {
                    target,
                    content,
                    props: RunProps::default(),
                }),
        ]
    })
}

fn arb_block() -> impl Strategy<Value = Block> {
    let leaf = prop_oneof![
        (
            proptest::collection::vec(arb_inline(), 0..4),
            proptest::option::of("[A-Za-z]{1,10}")
        )
            .prop_map(|(content, style)| Block::Paragraph(Paragraph {
                id: None,
                format: ParaFormat {
                    style: style.map(StyleId::new),
                    ..Default::default()
                },
                content,
            })),
        (1u8..=6, ".{0,16}").prop_map(|(level, text)| Block::Heading(Heading {
            id: None,
            level,
            paragraph: Paragraph::text(text),
        })),
        (".{0,8}", ".{0,32}").prop_map(|(part, content)| Block::Raw(RawFragment {
            id: RawId::new("raw-0001"),
            format: "ooxml".into(),
            part,
            content,
        })),
    ];
    leaf.prop_recursive(3, 16, 3, |inner| {
        prop_oneof![
            (
                any::<bool>(),
                proptest::collection::vec(inner.clone(), 1..3)
            )
                .prop_map(|(ordered, blocks)| Block::List(List {
                    id: None,
                    def: None,
                    ordered,
                    level: 0,
                    items: vec![ListItem { blocks }],
                })),
            proptest::collection::vec(inner, 1..3).prop_map(|blocks| Block::Table(Table {
                id: None,
                style: None,
                col_widths: vec![],
                header_row: false,
                rows: vec![TableRow {
                    id: None,
                    cells: vec![TableCell {
                        blocks,
                        ..Default::default()
                    }],
                    is_header: false,
                }],
            })),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn arbitrary_text_documents_round_trip_through_json(
        blocks in proptest::collection::vec(arb_block(), 0..6),
        title in proptest::option::of(".{0,20}"),
    ) {
        let doc = Document::Text(TextDocument {
        addressing: Default::default(),
            meta: DocumentMeta { title, ..Default::default() },
            sections: vec![Section { blocks, ..Default::default() }],
            ..Default::default()
        });
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&doc, &back);
        prop_assert_eq!(json, serde_json::to_string(&back).unwrap());
    }

    #[test]
    fn arbitrary_cells_round_trip_through_json(
        numbers in proptest::collection::vec(-1e12f64..1e12, 0..8),
        texts in proptest::collection::vec(".{0,16}", 0..8),
    ) {
        let mut sheet = Sheet::new("Hoja");
        for (i, n) in numbers.iter().enumerate() {
            sheet.cells.insert(
                CellRef::new(0, i as u32),
                Cell { value: CellValue::Number(*n), ..Default::default() },
            );
        }
        for (i, t) in texts.iter().enumerate() {
            sheet.cells.insert(
                CellRef::new(1, i as u32),
                Cell { value: CellValue::Text(t.clone()), ..Default::default() },
            );
        }
        let doc = Document::Workbook(Workbook {
        addressing: Default::default(), sheets: vec![sheet], ..Default::default() });
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&doc, &back);
    }

    #[test]
    fn style_resolution_never_panics_on_arbitrary_chains(
        edges in proptest::collection::vec((0u8..8, 0u8..8), 0..16),
    ) {
        let mut catalog = StyleCatalog::default();
        for (child, parent) in edges {
            let mut style = Style::new(format!("S{child}"), StyleType::Paragraph);
            style.based_on = Some(StyleId::new(format!("S{parent}")));
            style.font.bold = Some(child % 2 == 0);
            catalog.insert(style);
        }
        for i in 0..8u8 {
            let _ = catalog.resolve(Some(&StyleId::new(format!("S{i}"))));
        }
    }
}
