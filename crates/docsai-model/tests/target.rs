//! Target resolution and the etag precondition (plan v2, Phase 17-B).
//!
//! Two properties, and every test here belongs to one of them:
//!
//! 1. **Resolution is total.** A target either names exactly one place or
//!    fails with a typed error naming what it got and what it expected. The
//!    failures get as much room as the successes, because a resolver that is
//!    right about the easy cases and vague about the rest is what makes an
//!    agent edit the wrong node.
//! 2. **An etag covers the node and its subtree.** A change anywhere beneath a
//!    node moves that node's etag, which is what makes `if_match` on a `move`
//!    or a `delete` mean anything at all.

use docsai_model::addressing::{
    check_if_match, etag_of, resolve_str, Etag, MalformedReason, PreconditionError, Target,
    TargetError,
};
use docsai_model::image::{ImageGeometry, ImageRef};
use docsai_model::presentation::{
    ChartRef, PhType, Placeholder, Presentation, Shape, ShapeKind, Slide,
};
use docsai_model::sheet::{Sheet, Workbook};
use docsai_model::text::{
    Block, Comment, Footnote, Heading, Inline, List, ListItem, Paragraph, Section, Table,
    TableCell, TableRow, TextBox, TextDocument,
};
use docsai_model::units::Size;
use docsai_model::{AssetId, Document, NodeId, NodeKind};

// --------------------------------------------------------------------------
// Builders. Deliberately hand-built rather than read from the corpus: these
// tests are about the resolver, and a hand-built tree is the only way to place
// a duplicate id, which no reader will ever produce.
// --------------------------------------------------------------------------

fn id(node: &str) -> NodeId {
    NodeId::new(node)
}

fn para(node_id: Option<&str>, text: &str) -> Paragraph {
    let mut p = Paragraph::text(text);
    p.id = node_id.map(id);
    p
}

fn para_block(node_id: Option<&str>, text: &str) -> Block {
    Block::Paragraph(para(node_id, text))
}

fn section(node_id: Option<&str>, blocks: Vec<Block>) -> Section {
    Section {
        id: node_id.map(id),
        blocks,
        ..Default::default()
    }
}

fn text_doc(sections: Vec<Section>) -> Document {
    Document::Text(TextDocument {
        sections,
        ..Default::default()
    })
}

fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        blocks,
        ..Default::default()
    }
}

fn table(node_id: Option<&str>, rows: Vec<TableRow>) -> Table {
    Table {
        id: node_id.map(id),
        rows,
        ..Default::default()
    }
}

fn row(node_id: Option<&str>, cells: Vec<TableCell>) -> TableRow {
    TableRow {
        id: node_id.map(id),
        cells,
        ..Default::default()
    }
}

fn image(node_id: &str) -> ImageRef {
    let mut picture = ImageRef::new(
        AssetId::new("aaaaaaaa"),
        ImageGeometry::inline(Size::default()),
    );
    picture.id = Some(id(node_id));
    picture
}

fn placeholder(node_id: &str, text: &str) -> Shape {
    let mut shape = Shape::new(
        0,
        ShapeKind::Placeholder(Placeholder {
            ph_type: PhType::Title,
            body: vec![para_block(None, text)],
            ..Default::default()
        }),
    );
    shape.id = Some(id(node_id));
    shape
}

fn deck(slides: Vec<Slide>) -> Document {
    Document::Presentation(Presentation {
        slides,
        ..Default::default()
    })
}

fn slide(node_id: &str, shapes: Vec<Shape>) -> Slide {
    Slide {
        id: Some(id(node_id)),
        shapes,
        ..Default::default()
    }
}

/// A document exercising every branch of the walk: a table with rows and cells,
/// a list with items, a text box, a footnote and a comment.
fn rich_document() -> Document {
    let footnote_para = Paragraph::new(vec![
        Inline::Text("see ".into()),
        Inline::Footnote(Footnote {
            id: Some(id("fn1")),
            blocks: vec![para_block(None, "the appendix")],
        }),
    ]);
    text_doc(vec![section(
        Some("sec1"),
        vec![
            Block::Heading(Heading {
                id: Some(id("h1")),
                level: 1,
                paragraph: para(None, "Title"),
            }),
            para_block(Some("p1"), "First"),
            Block::Table(table(
                Some("t1"),
                vec![
                    row(Some("r1"), vec![cell(vec![para_block(Some("c1"), "one")])]),
                    row(
                        Some("r2"),
                        vec![
                            cell(vec![para_block(Some("c2"), "two")]),
                            cell(vec![para_block(Some("c3"), "three")]),
                        ],
                    ),
                ],
            )),
            Block::List(List {
                id: Some(id("l1")),
                def: None,
                ordered: false,
                level: 0,
                items: vec![
                    ListItem {
                        blocks: vec![para_block(Some("i1"), "alpha")],
                    },
                    ListItem {
                        blocks: vec![para_block(Some("i2"), "beta")],
                    },
                ],
            }),
            Block::TextBox(TextBox {
                blocks: vec![para_block(Some("tb1"), "aside")],
                size: None,
                x: None,
                y: None,
            }),
            Block::Image(image("img1")),
            Block::Paragraph(footnote_para),
            Block::Paragraph(Paragraph::new(vec![Inline::Comment(Box::new(Comment {
                id: Some(id("cm1")),
                blocks: vec![para_block(None, "please check")],
                ..Default::default()
            }))])),
        ],
    )])
}

// --------------------------------------------------------------------------
// 1. The grammar
// --------------------------------------------------------------------------

#[test]
fn the_three_roots_each_resolve() {
    // `doc` on a single-section text document is that section's blocks.
    let one = text_doc(vec![section(None, vec![para_block(Some("n1"), "a")])]);
    assert_eq!(resolve_str(&one, "doc/1").unwrap().id(), Some(&id("n1")));

    // `doc` on a workbook is its sheets; on a deck, its slides.
    let book = Document::Workbook(Workbook {
        sheets: vec![Sheet::new("Data"), Sheet::new("Notes")],
        ..Default::default()
    });
    assert_eq!(
        resolve_str(&book, "doc/2").unwrap().kind(),
        Some(NodeKind::Sheet)
    );
    let deck = deck(vec![slide("s1", vec![]), slide("s2", vec![])]);
    assert_eq!(resolve_str(&deck, "doc/2").unwrap().id(), Some(&id("s2")));

    // A bare id, and an id with a path under it.
    let doc = rich_document();
    assert_eq!(
        resolve_str(&doc, "t1").unwrap().kind(),
        Some(NodeKind::Table)
    );
    assert_eq!(
        resolve_str(&doc, "#t1").unwrap().kind(),
        Some(NodeKind::Table)
    );
    assert_eq!(resolve_str(&doc, "t1/2").unwrap().id(), Some(&id("r2")));
    assert_eq!(resolve_str(&doc, "t1/2/2/1").unwrap().id(), Some(&id("c3")));
}

#[test]
fn an_index_path_reaches_every_sequence_the_operations_need() {
    let doc = rich_document();
    // §6.1: a table or a row is refused, and the cell's paragraph is reached
    // by path. That path has to exist, so this is the test that says it does.
    assert_eq!(
        resolve_str(&doc, "r2/1/1").unwrap().kind(),
        Some(NodeKind::Paragraph)
    );
    // A list item is a place with no id of its own; its blocks have one.
    assert_eq!(resolve_str(&doc, "l1/2/1").unwrap().id(), Some(&id("i2")));
    // A section indexes its blocks.
    assert_eq!(resolve_str(&doc, "sec1/2").unwrap().id(), Some(&id("p1")));
    // A shape's body, through a slide.
    let deck = deck(vec![slide("s1", vec![placeholder("sh1", "Q3")])]);
    assert_eq!(
        resolve_str(&deck, "s1/1").unwrap().kind(),
        Some(NodeKind::Shape)
    );
    assert_eq!(
        resolve_str(&deck, "s1/1/1").unwrap().kind(),
        Some(NodeKind::Paragraph)
    );
}

#[test]
fn every_id_carrying_node_is_reachable_by_its_id() {
    let doc = rich_document();
    for node in [
        "sec1", "h1", "p1", "t1", "r1", "r2", "c1", "c2", "c3", "l1", "i1", "i2", "tb1", "img1",
        "fn1", "cm1",
    ] {
        let place = resolve_str(&doc, node).unwrap_or_else(|e| panic!("{node}: {e}"));
        assert_eq!(place.id(), Some(&id(node)), "{node} resolved elsewhere");
        assert!(place.etag().is_some(), "{node} has no etag");
    }
}

// --------------------------------------------------------------------------
// 2. The failures. Every one of them typed, and naming both sides.
// --------------------------------------------------------------------------

#[test]
fn a_malformed_target_names_what_was_expected() {
    for (input, reason) in [
        ("", MalformedReason::EmptySegment),
        ("doc/", MalformedReason::EmptySegment),
        ("n1//1", MalformedReason::EmptySegment),
        ("n 1", MalformedReason::NotAnId),
        ("n1/two", MalformedReason::IndexNotANumber),
        ("n1/0", MalformedReason::IndexNotOneBased),
        ("n1/4294967296", MalformedReason::IndexTooLarge),
    ] {
        match Target::parse(input) {
            Err(TargetError::Malformed {
                target,
                reason: got,
            }) => {
                assert_eq!(target, input);
                assert_eq!(got, reason, "for {input:?}");
                assert!(!got.as_str().is_empty());
            }
            other => panic!("{input:?} should be malformed, got {other:?}"),
        }
    }
}

#[test]
fn an_id_that_does_not_exist_is_named_in_the_refusal() {
    let doc = rich_document();
    match resolve_str(&doc, "n999") {
        Err(TargetError::UnknownId { target, id: got }) => {
            assert_eq!(target, "n999");
            assert_eq!(got, id("n999"));
        }
        other => panic!("expected an unknown id, got {other:?}"),
    }
}

#[test]
fn an_index_off_the_end_names_the_segment_that_failed() {
    let doc = rich_document();
    match resolve_str(&doc, "t1/3") {
        Err(TargetError::IndexOutOfRange {
            target,
            at,
            kind,
            index,
            len,
        }) => {
            assert_eq!(target, "t1/3");
            assert_eq!(at, "t1", "the failing segment is the one that has 2 rows");
            assert_eq!(kind, "table");
            assert_eq!((index, len), (3, 2));
        }
        other => panic!("expected an out-of-range index, got {other:?}"),
    }
    // Deeper down, the prefix names where the walk got to and not the whole
    // target: `t1/2` exists, `t1/2/3` does not.
    match resolve_str(&doc, "t1/2/3") {
        Err(TargetError::IndexOutOfRange { at, kind, len, .. }) => {
            assert_eq!((at.as_str(), kind, len), ("t1/2", "row", 2));
        }
        other => panic!("expected an out-of-range index, got {other:?}"),
    }
}

#[test]
fn an_index_into_a_node_with_no_indexable_children_is_its_own_refusal() {
    let doc = rich_document();
    // A paragraph: its inlines are not a child sequence any operation splices
    // into, so there is no index that would have worked.
    match resolve_str(&doc, "p1/1") {
        Err(TargetError::NotIndexable { target, at, kind }) => {
            assert_eq!(
                (target.as_str(), at.as_str(), kind),
                ("p1/1", "p1", "paragraph")
            );
        }
        other => panic!("expected a not-indexable refusal, got {other:?}"),
    }
    // An image.
    assert!(matches!(
        resolve_str(&doc, "img1/1"),
        Err(TargetError::NotIndexable { kind: "image", .. })
    ));
    // A sheet: its cells are an A1-addressed map, not a sequence (§6.7).
    let book = Document::Workbook(Workbook {
        sheets: vec![Sheet {
            id: Some(id("sh1")),
            ..Sheet::new("Data")
        }],
        ..Default::default()
    });
    assert!(matches!(
        resolve_str(&book, "sh1/1"),
        Err(TargetError::NotIndexable { kind: "sheet", .. })
    ));
    // A chart shape: Phase 16 established its shape is not editable.
    let chart = {
        let mut shape = Shape::new(0, ShapeKind::Chart(ChartRef::default()));
        shape.id = Some(id("ch1"));
        shape
    };
    let deck = deck(vec![slide("s1", vec![chart])]);
    assert!(matches!(
        resolve_str(&deck, "ch1/1"),
        Err(TargetError::NotIndexable { kind: "chart", .. })
    ));
}

/// Phase 10 says an id cannot appear twice. This asserts it rather than
/// assuming it: the alternative to a refusal is resolving to whichever copy the
/// walk met first, which is an edit applied to a node the caller did not name.
#[test]
fn a_duplicated_id_is_refused_and_never_resolved_to_the_first_one() {
    let doc = text_doc(vec![section(
        None,
        vec![
            para_block(Some("n1"), "first"),
            para_block(Some("n2"), "second"),
            para_block(Some("n1"), "third"),
        ],
    )]);
    match resolve_str(&doc, "n1") {
        Err(TargetError::AmbiguousId {
            target,
            id: got,
            count,
        }) => {
            assert_eq!(target, "n1");
            assert_eq!(got, id("n1"));
            assert_eq!(count, 2);
        }
        other => panic!("expected an ambiguous id, got {other:?}"),
    }
    // The document is not otherwise poisoned: `n2` still resolves.
    assert!(resolve_str(&doc, "n2").is_ok());
}

#[test]
fn a_node_that_calls_itself_doc_makes_the_root_a_refusal() {
    let doc = text_doc(vec![section(None, vec![para_block(Some("doc"), "a")])]);
    match resolve_str(&doc, "doc") {
        Err(TargetError::ReservedId { kind }) => assert_eq!(kind, "paragraph"),
        other => panic!("expected the reserved name to be refused, got {other:?}"),
    }
}

#[test]
fn an_empty_document_refuses_an_index_rather_than_panicking() {
    let empty = text_doc(vec![]);
    assert!(matches!(
        resolve_str(&empty, "doc/1"),
        Err(TargetError::NotIndexable { .. })
    ));
    assert!(resolve_str(&empty, "doc").is_ok());
}

// --------------------------------------------------------------------------
// 3. The etag: scope, stability and the loud failure
// --------------------------------------------------------------------------

/// The decision of 17-B, one assertion per container kind.
#[test]
fn an_etag_covers_the_node_and_everything_under_it() {
    let mut doc = rich_document();
    let before: Vec<Etag> = ["sec1", "t1", "r2", "l1"]
        .iter()
        .map(|node| etag_of(&doc, &Target::parse(node).unwrap()).unwrap())
        .collect();

    // One character, three levels down: inside a cell, inside a row, inside a
    // table, inside a section.
    let Document::Text(text) = &mut doc else {
        unreachable!()
    };
    let Some(Block::Table(t)) = text.sections[0]
        .blocks
        .iter_mut()
        .find(|b| matches!(b, Block::Table(_)))
    else {
        unreachable!()
    };
    let Block::Paragraph(p) = &mut t.rows[1].cells[1].blocks[0] else {
        unreachable!()
    };
    p.content = vec![Inline::Text("THREE".into())];

    let after: Vec<Etag> = ["sec1", "t1", "r2", "l1"]
        .iter()
        .map(|node| etag_of(&doc, &Target::parse(node).unwrap()).unwrap())
        .collect();

    assert_ne!(before[0], after[0], "the section must see the change");
    assert_ne!(before[1], after[1], "the table must see the change");
    assert_ne!(before[2], after[2], "the row must see the change");
    assert_eq!(before[3], after[3], "an unrelated sibling must not move");
}

/// The same property on a deck, because that is where `move` and
/// `duplicate_slide` earn their living.
#[test]
fn a_slide_etag_sees_a_change_in_a_shape_and_in_a_group() {
    let inner = placeholder("g1", "inner");
    let mut group = Shape::new(1, ShapeKind::Group(vec![inner]));
    group.id = Some(id("grp"));
    let mut document = deck(vec![slide(
        "s1",
        vec![placeholder("sh1", "Q3 results"), group],
    )]);

    let slide_before = etag_of(&document, &Target::parse("s1").unwrap()).unwrap();
    let group_before = etag_of(&document, &Target::parse("grp").unwrap()).unwrap();

    let Document::Presentation(deck) = &mut document else {
        unreachable!()
    };
    let ShapeKind::Group(children) = &mut deck.slides[0].shapes[1].kind else {
        unreachable!()
    };
    let ShapeKind::Placeholder(ph) = &mut children[0].kind else {
        unreachable!()
    };
    ph.body = vec![para_block(None, "changed")];

    assert_ne!(
        group_before,
        etag_of(&document, &Target::parse("grp").unwrap()).unwrap(),
        "a group must see its children"
    );
    assert_ne!(
        slide_before,
        etag_of(&document, &Target::parse("s1").unwrap()).unwrap(),
        "a slide must see a change two levels inside it, or `delete s1` \
         guarded by an etag destroys an edit it was meant to protect"
    );
}

/// The half of the property that stops the etag being useless: a read that
/// changed nothing leaves it alone. Derived, never stored — so this is the
/// same document hashed twice, and the same document rebuilt from scratch.
#[test]
fn a_read_that_changed_nothing_leaves_every_etag_alone() {
    let first = rich_document();
    let second = rich_document();
    for node in ["sec1", "h1", "p1", "t1", "r2", "l1", "img1", "fn1", "cm1"] {
        let target = Target::parse(node).unwrap();
        assert_eq!(
            etag_of(&first, &target).unwrap(),
            etag_of(&second, &target).unwrap(),
            "{node} is not stable"
        );
        assert_eq!(
            etag_of(&first, &target).unwrap(),
            etag_of(&first, &target).unwrap(),
            "{node} is not stable within one document"
        );
    }
}

/// Stability across a process restart and across a release, pinned by literal.
///
/// [`docsai_model::addressing::EtagHasher`] is FNV-1a with fixed constants and
/// no state outside the bytes it is fed, so a new process computes the same
/// value — but «is a pure function» is an argument, and a checked-in literal is
/// evidence. If this test fails, every etag an agent is holding just became
/// wrong, which is a specification change and not a refactor.
#[test]
fn etag_values_are_pinned_across_processes_and_releases() {
    let doc = rich_document();
    for (node, expected) in [
        ("h1", "f6d050"),
        ("p1", "02e078"),
        ("t1", "2d81da"),
        ("r2", "f21e0e"),
        ("l1", "4cfa9f"),
        ("img1", "85cf68"),
    ] {
        let got = etag_of(&doc, &Target::parse(node).unwrap()).unwrap();
        assert_eq!(got.as_str(), expected, "{node}");
        assert!(got.is_valid(), "{node} is not canonical hex");
    }
}

#[test]
fn a_stale_etag_is_typed_loud_and_carries_both_values() {
    let doc = rich_document();
    let target = Target::parse("p1").unwrap();
    let place = resolve_str(&doc, "p1").unwrap();
    let actual = place.etag().unwrap();

    // Absent is not a loophole (§7): an unconditional edit is a normal edit.
    assert!(check_if_match(&place, &target, None).is_ok());
    assert!(check_if_match(&place, &target, Some(&actual)).is_ok());

    let stale = Etag::new("deadbe");
    match check_if_match(&place, &target, Some(&stale)) {
        Err(PreconditionError::StaleEtag {
            target,
            expected,
            actual: got,
        }) => {
            assert_eq!(target, "p1");
            assert_eq!(expected, stale, "the caller's value is carried back");
            assert_eq!(got, actual, "and so is the document's");
            let rendered = PreconditionError::StaleEtag {
                target,
                expected,
                actual: got,
            }
            .to_string();
            assert!(rendered.contains("deadbe") && rendered.contains(actual.as_str()));
        }
        other => panic!("expected a stale etag, got {other:?}"),
    }
}

/// An `if_match` against something with no etag is refused rather than passed:
/// a precondition the caller believes was checked and was not is worse than
/// none at all.
#[test]
fn a_precondition_on_a_place_that_has_no_etag_is_refused() {
    let doc = rich_document();
    for target in ["doc", "r2/1", "l1/1"] {
        let parsed = Target::parse(target).unwrap();
        let place = resolve_str(&doc, target).unwrap();
        assert!(place.etag().is_none(), "{target} unexpectedly has an etag");
        assert!(matches!(
            check_if_match(&place, &parsed, Some(&Etag::new("abcdef"))),
            Err(PreconditionError::NoEtag { .. })
        ));
        assert!(matches!(
            etag_of(&doc, &parsed),
            Err(PreconditionError::NoEtag { .. })
        ));
    }
}

/// A precondition on a target that does not exist fails as a *target* failure.
/// Reporting «the etag did not match» for a node that is not there would send
/// the caller looking for a concurrent writer that does not exist.
#[test]
fn a_precondition_on_a_missing_target_reports_the_missing_target() {
    let doc = rich_document();
    assert!(matches!(
        etag_of(&doc, &Target::parse("n404").unwrap()),
        Err(PreconditionError::Target(TargetError::UnknownId { .. }))
    ));
}

/// The known limitation, written down as a test so it cannot rot into a
/// surprise: formatting is not content, so `set_props` alone does not move an
/// etag. The reason is in `kb/87-phase-17-etags.md` — an etag that hashed
/// appearance would differ between `full` and `agent` and refuse edits nobody
/// conflicted over.
#[test]
fn formatting_alone_does_not_move_an_etag_and_that_is_the_documented_trade() {
    let mut doc = text_doc(vec![section(None, vec![para_block(Some("n1"), "Revenue")])]);
    let before = etag_of(&doc, &Target::parse("n1").unwrap()).unwrap();
    let Document::Text(text) = &mut doc else {
        unreachable!()
    };
    let Block::Paragraph(p) = &mut text.sections[0].blocks[0] else {
        unreachable!()
    };
    p.format.direct.align = Some(docsai_model::style::Align::Center);
    assert_eq!(
        before,
        etag_of(&doc, &Target::parse("n1").unwrap()).unwrap(),
        "if this ever changes, §7's contract changed with it"
    );
}
