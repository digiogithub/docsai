//! Stable-addressing invariants (plan v2 Phase 10, risk P7).
//!
//! An id that moves, repeats or disappears makes an agent edit the wrong node
//! silently, so these are property tests rather than examples: ids must survive
//! repeated assignment, insertions and deletions.

use std::collections::HashSet;

use docsai_model::addressing::{assign_ids, clear_ids, node_ids, Addressable, IdPolicy};
use docsai_model::style::FontProps;
use docsai_model::text::{
    Block, Footnote, Heading, Inline, List, ListItem, Paragraph, RunProps, Section, Table,
    TableCell, TableRow, TextDocument,
};
use docsai_model::{Document, NodeId};
use proptest::prelude::*;

/// Rounds of assignment the ids must survive unchanged (plan 10, criterion 1).
const ROUNDS: usize = 10;

fn paragraph(text: &str) -> Block {
    Block::Paragraph(Paragraph::text(text))
}

fn arb_block() -> impl Strategy<Value = Block> {
    let leaf = prop_oneof![
        "[a-z ]{0,20}".prop_map(|t| paragraph(&t)),
        (1u8..=6, "[a-z ]{0,12}").prop_map(|(level, text)| Block::Heading(Heading {
            id: None,
            level,
            paragraph: Paragraph::text(text),
        })),
        "[a-z ]{0,12}".prop_map(|t| Block::Paragraph(Paragraph::new(vec![Inline::Footnote(
            Footnote::new(vec![paragraph(&t)])
        )]))),
    ];
    leaf.prop_recursive(3, 12, 3, |inner| {
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
                rows: vec![TableRow {
                    id: None,
                    cells: vec![TableCell {
                        blocks,
                        ..Default::default()
                    }],
                    is_header: false,
                }],
                ..Default::default()
            })),
        ]
    })
}

fn arb_document() -> impl Strategy<Value = Document> {
    proptest::collection::vec(arb_block(), 0..6).prop_map(|blocks| {
        Document::Text(TextDocument {
            sections: vec![Section {
                blocks,
                ..Default::default()
            }],
            ..Default::default()
        })
    })
}

fn addressable_count(doc: &Document) -> usize {
    let mut count = 0;
    docsai_model::addressing::for_each_addressable(doc, &mut |_| count += 1);
    count
}

fn unique(ids: &[NodeId]) -> bool {
    ids.iter().collect::<HashSet<_>>().len() == ids.len()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    /// Assignment is idempotent: the second and the tenth pass change nothing.
    #[test]
    fn ids_survive_repeated_assignment(mut doc in arb_document()) {
        assign_ids(&mut doc, IdPolicy::Assign);
        let first = node_ids(&doc);
        prop_assert!(unique(&first), "ids collide inside one document");

        for _ in 0..ROUNDS {
            assign_ids(&mut doc, IdPolicy::Assign);
            prop_assert_eq!(node_ids(&doc), first.clone(), "ids moved on reassignment");
        }
    }

    /// Inserting and deleting siblings never renumbers a surviving node and
    /// never hands a freed id to a new one.
    #[test]
    fn insertion_and_deletion_never_reuse_an_id(mut doc in arb_document()) {
        assign_ids(&mut doc, IdPolicy::Assign);
        let mut seen: HashSet<NodeId> = node_ids(&doc).into_iter().collect();

        for round in 0..ROUNDS {
            let Document::Text(text) = &mut doc else { unreachable!() };
            let blocks = &mut text.sections[0].blocks;
            // Delete the front, insert at both ends: the worst case for a
            // positional scheme.
            if !blocks.is_empty() {
                blocks.remove(0);
            }
            blocks.insert(0, paragraph(&format!("inserted {round}")));
            blocks.push(Block::Heading(Heading {
                id: None,
                level: 1,
                paragraph: Paragraph::text(format!("appended {round}")),
            }));

            let survivors: Vec<NodeId> = node_ids(&doc);
            assign_ids(&mut doc, IdPolicy::Assign);
            let after = node_ids(&doc);
            prop_assert!(unique(&after), "ids collide after edit round {}", round);
            for id in &survivors {
                prop_assert!(after.contains(id), "surviving node lost id {} ", id);
            }
            for id in &after {
                if !survivors.contains(id) {
                    prop_assert!(!seen.contains(id), "id {} was reused", id);
                }
            }
            seen.extend(after);
        }
    }

    /// `preserve` assigns nothing, `never` leaves the document id-free.
    #[test]
    fn policies_do_what_they_say(mut doc in arb_document()) {
        let mut untouched = doc.clone();
        assign_ids(&mut untouched, IdPolicy::Preserve);
        prop_assert!(node_ids(&untouched).is_empty());

        assign_ids(&mut doc, IdPolicy::Assign);
        prop_assert_eq!(node_ids(&doc).len(), addressable_count(&doc),
            "assign must leave every addressable node with an id");

        clear_ids(&mut doc);
        prop_assert!(node_ids(&doc).is_empty());
        prop_assert_eq!(doc.addressing().next_id, 1);
    }
}

#[test]
fn hand_written_ids_are_preserved_and_never_handed_out_again() {
    let mut doc = Document::Text(TextDocument {
        sections: vec![Section {
            blocks: vec![
                Block::Heading(Heading {
                    id: Some(NodeId::new("n5")),
                    level: 1,
                    paragraph: Paragraph::text("kept"),
                }),
                Block::Heading(Heading {
                    id: None,
                    level: 2,
                    paragraph: Paragraph::text("new"),
                }),
            ],
            ..Default::default()
        }],
        ..Default::default()
    });
    assign_ids(&mut doc, IdPolicy::Assign);
    let ids = node_ids(&doc);
    assert!(
        ids.contains(&NodeId::new("n5")),
        "existing id was renumbered"
    );
    assert!(
        unique(&ids),
        "a fresh id collided with the hand-written one"
    );
    assert!(doc.addressing().next_id > 5, "the counter must dominate n5");
}

#[test]
fn etag_tracks_content_and_ignores_formatting() {
    let plain = Paragraph::text("Revenue up 12 %");
    let mut bold = plain.clone();
    bold.format.run_direct = FontProps {
        bold: Some(true),
        ..Default::default()
    };
    assert_eq!(
        plain.etag(),
        bold.etag(),
        "paragraph-level formatting is not content"
    );

    let styled_run = Paragraph::new(vec![Inline::Styled {
        content: vec![Inline::Text("Revenue up 12 %".into())],
        props: RunProps::direct(FontProps {
            bold: Some(true),
            ..Default::default()
        }),
    }]);
    assert_ne!(
        plain.etag(),
        styled_run.etag(),
        "splitting the text into runs changes the structure"
    );

    let edited = Paragraph::text("Revenue up 13 %");
    assert_ne!(plain.etag(), edited.etag(), "an edit must change the etag");
}

#[test]
fn etag_ignores_the_id_itself() {
    let mut with_id = Paragraph::text("same text");
    let etag_before = with_id.etag();
    with_id.set_node_id(NodeId::new("n42"));
    assert_eq!(
        etag_before,
        with_id.etag(),
        "assigning an id must not churn the etag"
    );
}
