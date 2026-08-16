//! The front matter of a deck: DocMark 1.2, `layouts:` and `skeleton:`
//! (spec §11.2, plan v2 Phase 14 increment B).
//!
//! Everything asserted here is the front matter and the version it declares;
//! the body of a deck is `presentation_slides.rs`.

use docsai_docmark::{serialize, Fidelity, Options};
use docsai_model::presentation::{
    Layout, LayoutId, LayoutPlaceholder, Master, MasterId, PhType, Presentation, SkeletonRef,
};
use docsai_model::{AssetStore, Document, Format, MemoryAssetStore};

fn options(fidelity: Fidelity) -> Options {
    Options {
        fidelity,
        source_format: Format::Pptx,
        ..Default::default()
    }
}

/// A deck with one layout, its master, and a preserved package.
fn deck(store: &mut MemoryAssetStore) -> Document {
    let asset = store.put(b"PK\x03\x04 a whole pptx package").unwrap();
    let mut presentation = Presentation {
        skeleton: Some(SkeletonRef {
            asset,
            rebuilt_parts: vec!["ppt/slides/slide1.xml".into()],
        }),
        ..Default::default()
    };
    presentation.layouts.layouts.insert(
        LayoutId::new("L1"),
        Layout {
            name: "Title and Content".into(),
            master: Some(MasterId::new("M1")),
            placeholders: vec![
                LayoutPlaceholder {
                    // A title placeholder carries no `p:ph@idx`, which is
                    // index 0 in PresentationML.
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
        },
    );
    presentation.layouts.masters.insert(
        MasterId::new("M1"),
        Master {
            name: "Office Theme".into(),
            ..Default::default()
        },
    );
    Document::Presentation(presentation)
}

fn front_matter(markdown: &str) -> &str {
    markdown
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
        .map(|(fm, _)| fm)
        .expect("front matter")
}

#[test]
fn a_deck_declares_the_presentation_profile() {
    let mut store = MemoryAssetStore::new();
    let doc = deck(&mut store);
    let (markdown, _) = serialize(&doc, &store, &options(Fidelity::Full));

    // 1.2 with no ids in the body: the version names the profile, not the
    // addressing (spec §11.2). A text document with no ids still says 1.0.
    assert!(
        front_matter(&markdown).contains("docmark: \"1.2\""),
        "{markdown}"
    );
    assert!(front_matter(&markdown).contains("source-format: pptx"));
}

#[test]
fn the_layout_catalogue_says_which_placeholder_is_the_title_and_which_the_body() {
    let mut store = MemoryAssetStore::new();
    let doc = deck(&mut store);
    let (markdown, _) = serialize(&doc, &store, &options(Fidelity::Full));

    // This line is what makes the implicit form of rules 1 and 2 a lookup
    // rather than a guess.
    assert!(
        markdown.contains("  L1: { name: \"Title and Content\", master: M1, title: 0, body: 1 }\n"),
        "{markdown}"
    );
}

#[test]
fn the_skeleton_is_a_path_a_reader_can_open() {
    let mut store = MemoryAssetStore::new();
    let doc = deck(&mut store);
    let (markdown, _) = serialize(&doc, &store, &options(Fidelity::Full));

    let line = markdown
        .lines()
        .find(|l| l.starts_with("skeleton:"))
        .expect("a skeleton line");
    assert!(
        line.starts_with("skeleton: assets/_skeleton/deck-"),
        "{line}"
    );
    assert!(line.ends_with(".pptx"), "{line}");
}

#[test]
fn standard_carries_neither_catalogue_nor_skeleton() {
    let mut store = MemoryAssetStore::new();
    let doc = deck(&mut store);
    let (markdown, _) = serialize(&doc, &store, &options(Fidelity::Standard));

    // Rule 6: `standard` does not write back, and a catalogue nothing in the
    // body refers to is pure cost to a human reading it.
    assert!(!markdown.contains("layouts:"), "{markdown}");
    assert!(!markdown.contains("skeleton:"), "{markdown}");
    // It is still the presentation profile: the body is written as slides.
    assert!(markdown.contains("docmark: \"1.2\""), "{markdown}");
}

#[test]
fn agent_carries_both_because_it_writes_back() {
    let mut store = MemoryAssetStore::new();
    let doc = deck(&mut store);
    let (markdown, _) = serialize(&doc, &store, &options(Fidelity::Agent));

    assert!(markdown.contains("layouts:"), "{markdown}");
    assert!(markdown.contains("skeleton:"), "{markdown}");
    assert!(markdown.contains("fidelity: agent"), "{markdown}");
}

#[test]
fn plain_has_no_front_matter_at_all() {
    let mut store = MemoryAssetStore::new();
    let doc = deck(&mut store);
    let (markdown, _) = serialize(&doc, &store, &options(Fidelity::Plain));

    assert!(!markdown.starts_with("---"), "{markdown}");
}

#[test]
fn a_deck_authored_from_scratch_has_no_skeleton_line() {
    let store = MemoryAssetStore::new();
    let doc = Document::Presentation(Presentation::default());
    let (markdown, _) = serialize(&doc, &store, &options(Fidelity::Full));

    // Absent, not empty: that is what tells the writer to reach for the
    // default template instead of a package that does not exist.
    assert!(!markdown.contains("skeleton:"), "{markdown}");
    assert!(!markdown.contains("layouts:"), "{markdown}");
}

#[test]
fn the_front_matter_is_byte_deterministic() {
    let mut store = MemoryAssetStore::new();
    let doc = deck(&mut store);
    let once = serialize(&doc, &store, &options(Fidelity::Full)).0;
    let twice = serialize(&doc, &store, &options(Fidelity::Full)).0;
    assert_eq!(once, twice, "spec §8");
}

#[test]
fn a_selection_of_a_deck_declares_the_presentation_profile() {
    // A selection's body carries `.slide` and shape containers, and the
    // version names the *profile* the body is written in, not whether it has
    // ids (spec §11.2, version rule). A selection of a text document is
    // unchanged.
    let deck = docsai_docmark::selection_front_matter(&options(Fidelity::Full), 7, &[]);
    assert!(deck.contains("docmark: \"1.2\""), "{deck}");

    let text = docsai_docmark::selection_front_matter(
        &Options {
            source_format: Format::Docx,
            ..options(Fidelity::Full)
        },
        7,
        &[],
    );
    assert!(text.contains("docmark: \"1.1\""), "{text}");
}
