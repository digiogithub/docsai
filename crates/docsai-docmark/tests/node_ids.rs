//! DocMark 1.1 node ids on the way out and back in (spec §11.1).

use docsai_docmark::{parse, serialize, Fidelity, Options};
use docsai_model::addressing::{node_ids, IdPolicy};
use docsai_model::text::{
    Block, Footnote, Heading, Inline, List, ListItem, Paragraph, Section, TextDocument,
};
use docsai_model::{Document, MemoryAssetStore, NodeId};

fn options(fidelity: Fidelity, ids: IdPolicy) -> Options {
    Options {
        fidelity,
        ids,
        ..Default::default()
    }
}

fn sample() -> Document {
    Document::Text(TextDocument {
        sections: vec![Section {
            blocks: vec![
                Block::Heading(Heading {
                    id: None,
                    level: 1,
                    paragraph: Paragraph::text("Title"),
                }),
                Block::Paragraph(Paragraph::new(vec![
                    Inline::Text("Body with a note".into()),
                    Inline::Footnote(Footnote::new(vec![Block::Paragraph(Paragraph::text(
                        "the note",
                    ))])),
                ])),
                Block::List(List {
                    id: None,
                    def: None,
                    ordered: false,
                    level: 0,
                    items: vec![ListItem {
                        blocks: vec![Block::Paragraph(Paragraph::text("item"))],
                    }],
                }),
            ],
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn round_trip(markdown: &str) -> Document {
    let mut assets = MemoryAssetStore::new();
    parse(markdown, &mut assets).expect("parse").0
}

#[test]
fn full_output_declares_1_1_and_carries_the_counter() {
    let (markdown, _) = serialize(
        &sample(),
        &MemoryAssetStore::new(),
        &options(Fidelity::Full, IdPolicy::Assign),
    );
    assert!(markdown.contains("docmark: \"1.1\""), "{markdown}");
    assert!(markdown.contains("next-id: 5"), "{markdown}");
    assert!(markdown.contains("# Title {#n1}"), "{markdown}");
    assert!(markdown.contains("list-id=n4"), "{markdown}");
}

#[test]
fn ids_survive_a_round_trip_unchanged() {
    let assets = MemoryAssetStore::new();
    let opts = options(Fidelity::Full, IdPolicy::Assign);
    let (first, _) = serialize(&sample(), &assets, &opts);

    let parsed = round_trip(&first);
    assert_eq!(
        node_ids(&parsed),
        vec![
            NodeId::new("n1"),
            NodeId::new("n2"),
            NodeId::new("n3"),
            NodeId::new("n4"),
        ],
        "every id must come back from the document"
    );

    let (second, _) = serialize(&parsed, &assets, &opts);
    assert_eq!(first, second, "a second pass must be byte-identical");
}

#[test]
fn never_reproduces_the_1_0_shape() {
    let (markdown, _) = serialize(
        &sample(),
        &MemoryAssetStore::new(),
        &options(Fidelity::Full, IdPolicy::Never),
    );
    assert!(markdown.contains("docmark: \"1.0\""), "{markdown}");
    assert!(!markdown.contains("next-id:"), "{markdown}");
    assert!(!markdown.contains("{#n"), "{markdown}");
}

#[test]
fn preserve_writes_back_what_the_document_had_and_nothing_more() {
    let mut doc = sample();
    if let Document::Text(text) = &mut doc {
        if let Block::Heading(h) = &mut text.sections[0].blocks[0] {
            h.id = Some(NodeId::new("intro"));
        }
    }
    let (markdown, _) = serialize(
        &doc,
        &MemoryAssetStore::new(),
        &options(Fidelity::Full, IdPolicy::Preserve),
    );
    assert!(markdown.contains("# Title {#intro}"), "{markdown}");
    assert!(
        !markdown.contains("{#n"),
        "preserve must not allocate: {markdown}"
    );
}

#[test]
fn a_hand_written_id_is_never_handed_out_again() {
    let markdown = "---\ndocmark: \"1.1\"\nsource-format: docx\nnext-id: 2\n---\n\n\
                    # Kept {#n7}\n\n## Fresh\n";
    let doc = round_trip(markdown);
    let (out, _) = serialize(
        &doc,
        &MemoryAssetStore::new(),
        &options(Fidelity::Full, IdPolicy::Assign),
    );
    assert!(out.contains("# Kept {#n7}"), "{out}");
    assert!(out.contains("## Fresh {#n8}"), "{out}");
    assert!(out.contains("next-id: 9"), "{out}");
}

#[test]
fn a_1_0_document_still_parses_and_gains_ids_on_the_next_write() {
    let markdown = "---\ndocmark: \"1.0\"\nsource-format: docx\n---\n\n# Old {.Heading1}\n";
    let doc = round_trip(markdown);
    assert!(node_ids(&doc).is_empty(), "1.0 documents carry no ids");

    let (out, _) = serialize(
        &doc,
        &MemoryAssetStore::new(),
        &options(Fidelity::Full, IdPolicy::Assign),
    );
    assert!(out.contains("# Old {#n1 .Heading1}"), "{out}");
    assert!(out.contains("docmark: \"1.1\""), "{out}");
}

#[test]
fn the_lossy_levels_stay_free_of_ids() {
    for fidelity in [Fidelity::Standard, Fidelity::Plain] {
        // `plain` forces `never` whatever the caller asks for.
        let (markdown, _) = serialize(
            &sample(),
            &MemoryAssetStore::new(),
            &options(fidelity, IdPolicy::Never),
        );
        assert!(!markdown.contains("{#n"), "{fidelity}: {markdown}");
    }

    let (plain, _) = serialize(
        &sample(),
        &MemoryAssetStore::new(),
        &options(Fidelity::Plain, IdPolicy::Assign),
    );
    assert!(
        !plain.contains("{#n"),
        "plain is CommonMark only, ids and all: {plain}"
    );
}
