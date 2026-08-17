//! Speaker notes (spec §11.2 rule 5; plan v2 Phase 14 increment E).
//!
//! The one node whose *syntax* depends on the fidelity level: a container at
//! `full` and `agent`, a blockquote at `standard`. The parser of 14-H has to
//! read both, which is why every shape of both is pinned here.

use docsai_docmark::{serialize, Fidelity, Options};
use docsai_model::addressing::IdPolicy;
use docsai_model::presentation::{
    Layout, LayoutId, LayoutPlaceholder, PhType, Placeholder, Presentation, Shape, ShapeKind, Slide,
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

/// `standard` as the pipeline builds it: no ids, because it does not write
/// back (`ConvertOptions::id_policy`).
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

/// One slide with a title, an implicit body and the notes under test.
fn deck(notes: Option<Vec<Block>>) -> Document {
    let mut presentation = Presentation::default();
    presentation
        .layouts
        .layouts
        .insert(LayoutId::new("L1"), layout());
    presentation.slides.push(Slide {
        layout: Some(LayoutId::new("L1")),
        shapes: vec![
            placeholder(PhType::Title, None, "Q3 results"),
            placeholder(PhType::Body, Some(1), "Revenue up 12 %"),
        ],
        notes,
        ..Default::default()
    });
    Document::Presentation(presentation)
}

fn one_note() -> Option<Vec<Block>> {
    Some(vec![Block::Paragraph(Paragraph::text(
        "Open with the churn number.",
    ))])
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
fn full_writes_the_notes_as_a_container() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(one_note()), &store, &options(Fidelity::Full));

    assert_eq!(
        body_of(&markdown),
        "## Q3 results {#n1 .slide layout=L1}\n\nRevenue up 12 %\n\n\
         ::: {.notes}\nOpen with the churn number.\n:::\n",
        "{markdown}"
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn standard_writes_the_notes_as_a_blockquote() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(one_note()), &store, &standard());

    // A blockquote is native CommonMark and cannot collide with slide content:
    // PresentationML has no blockquote for a placeholder to occupy. And no
    // `layout=`: the catalogue it names is not written at this level (§11.2
    // rule 6), so the reference would point at nothing.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results {.slide}\n\nRevenue up 12 %\n\n\
         > Open with the churn number.\n",
        "{markdown}"
    );
    // The notes are not lost, so nothing is warned about them.
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn agent_writes_the_container_because_it_writes_back() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(one_note()), &store, &options(Fidelity::Agent));

    assert!(
        body_of(&markdown).contains("::: {.notes}\nOpen with the churn number.\n:::"),
        "{markdown}"
    );
}

#[test]
fn plain_drops_the_notes_and_says_so() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(one_note()), &store, &options(Fidelity::Plain));

    // `plain` writes what the slide shows; a notes page is not on the screen.
    assert_eq!(
        body_of(&markdown),
        "## Q3 results\n\nRevenue up 12 %\n",
        "{markdown}"
    );
    assert!(
        report.warnings.iter().any(|w| matches!(
            w,
            Warning::UnsupportedElement { kind, .. } if kind == "notes"
        )),
        "{:?}",
        report.warnings
    );
}

#[test]
fn an_empty_notes_slide_is_an_empty_container() {
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(Some(Vec::new())), &store, &options(Fidelity::Full));

    // `Some(vec![])` and `None` are different things to the writer that puts
    // the package back: one deck has a notes slide, the other has none.
    assert!(
        body_of(&markdown).ends_with("::: {.notes}\n:::\n"),
        "{markdown}"
    );

    let (standard_md, _) = serialize(&deck(Some(Vec::new())), &store, &standard());
    // A lone `>` is noise in a document that never writes back.
    assert!(!standard_md.contains('>'), "{standard_md}");
}

#[test]
fn a_slide_without_a_notes_page_writes_nothing() {
    let store = MemoryAssetStore::new();
    let (markdown, report) = serialize(&deck(None), &store, &options(Fidelity::Full));

    assert!(!markdown.contains(".notes"), "{markdown}");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn multi_block_notes_keep_their_shape_in_both_syntaxes() {
    let notes = Some(vec![
        Block::Paragraph(Paragraph::text("Primero el número.")),
        Block::List(List {
            id: None,
            def: None,
            ordered: false,
            level: 0,
            items: vec![
                ListItem {
                    blocks: vec![Block::Paragraph(Paragraph::text("churn"))],
                },
                ListItem {
                    blocks: vec![Block::Paragraph(Paragraph::text("margen"))],
                },
            ],
        }),
    ]);
    let store = MemoryAssetStore::new();
    let (markdown, _) = serialize(&deck(notes.clone()), &store, &options(Fidelity::Full));
    assert!(
        body_of(&markdown)
            .ends_with("::: {.notes}\nPrimero el número.\n\n- churn {list-id=n2}\n- margen\n:::\n"),
        "{markdown}"
    );

    let (standard_md, _) = serialize(&deck(notes), &store, &standard());
    // Every line of the quote is quoted, and the blank line between the
    // paragraph and the list stays inside it as a bare `>`.
    assert!(
        body_of(&standard_md).ends_with("> Primero el número.\n>\n> - churn\n> - margen\n"),
        "{standard_md}"
    );
}

#[test]
fn a_note_takes_the_ids_the_addressing_walk_expects() {
    let notes = Some(vec![Block::List(List {
        id: None,
        def: None,
        ordered: false,
        level: 0,
        items: vec![ListItem {
            blocks: vec![Block::Paragraph(Paragraph::text("churn"))],
        }],
    })]);
    let store = MemoryAssetStore::new();
    let document = deck(notes);
    let (markdown, _) = serialize(&document, &store, &options(Fidelity::Full));

    // The walk visits a slide, its shapes and then its notes; a list inside
    // the notes is addressable exactly like one on the slide.
    assert!(
        body_of(&markdown).contains("- churn {list-id=n2}"),
        "{markdown}"
    );
    assert!(markdown.contains("next-id: 3"), "{markdown}");
}

#[test]
fn notes_are_byte_deterministic_at_both_syntaxes() {
    let store = MemoryAssetStore::new();
    let document = deck(one_note());
    assert_eq!(
        serialize(&document, &store, &options(Fidelity::Full)).0,
        serialize(&document, &store, &options(Fidelity::Full)).0
    );
    assert_eq!(
        serialize(&document, &store, &standard()).0,
        serialize(&document, &store, &standard()).0
    );
}
