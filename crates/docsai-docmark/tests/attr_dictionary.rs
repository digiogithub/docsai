//! The attribute-set dictionary (plan v2 Phase 11, D; spec §3.7).
//!
//! A pattern of attributes that repeats is named once in the front matter and
//! referenced by class. That is a compression of the bytes and nothing else, so
//! the properties worth testing are the ones that say it changed nothing:
//! re-reading a document that uses the dictionary yields the same IR as
//! re-reading one that does not, and serialising twice gives the same bytes.
//!
//! The threshold arithmetic itself lives in the unit tests of `dict.rs`; what
//! is tested here is the round trip through a real document.

use docsai_docmark::{parse, serialize, Fidelity, Options};
use docsai_model::style::{FontProps, Style, StyleCatalog, StyleType};
use docsai_model::text::{Block, Inline, Paragraph, RunProps, Section, TextDocument};
use docsai_model::units::Length;
use docsai_model::{Document, MemoryAssetStore};

/// A pattern long enough to earn a name: three attributes, 38 characters.
fn marked() -> FontProps {
    FontProps {
        name: Some("Consolas".into()),
        color: Some("#1F4E79".into()),
        size: Some(Length::from_pt(12.0)),
        ..FontProps::default()
    }
}

fn run(text: &str, props: FontProps) -> Inline {
    Inline::Styled {
        content: vec![Inline::Text(text.into())],
        props: RunProps {
            style: None,
            direct: props,
        },
    }
}

/// `uses` paragraphs carrying the same direct formatting.
fn repeated(uses: usize, props: FontProps) -> Document {
    document(
        (0..uses)
            .map(|n| {
                Block::Paragraph(Paragraph {
                    content: vec![run(&format!("termino {n}"), props.clone())],
                    ..Default::default()
                })
            })
            .collect(),
        StyleCatalog::default(),
    )
}

fn document(blocks: Vec<Block>, styles: StyleCatalog) -> Document {
    Document::Text(TextDocument {
        styles,
        sections: vec![Section {
            blocks,
            ..Default::default()
        }],
        ..Default::default()
    })
}

fn markdown_at(doc: &Document, fidelity: Fidelity) -> String {
    let assets = MemoryAssetStore::new();
    let options = Options {
        fidelity,
        ..Options::default()
    };
    serialize(doc, &assets, &options).0
}

fn markdown(doc: &Document) -> String {
    markdown_at(doc, Fidelity::Full)
}

#[test]
fn a_repeated_pattern_is_written_once_and_referenced() {
    let out = markdown(&repeated(6, marked()));
    assert!(
        out.contains("attribute-sets:\n  g1: \"color=#1F4E79 font=Consolas size=12pt\"\n"),
        "the pattern should be interned once:\n{out}"
    );
    assert_eq!(
        out.matches("{.g1}").count(),
        6,
        "every use should reference it:\n{out}"
    );
    assert_eq!(
        out.matches("color=#1F4E79").count(),
        1,
        "and no use should spell it out again:\n{out}"
    );
}

#[test]
fn a_pattern_used_twice_is_left_where_it_is() {
    let out = markdown(&repeated(2, marked()));
    assert!(
        !out.contains("attribute-sets:"),
        "two uses do not pay for an entry:\n{out}"
    );
    assert_eq!(out.matches("color=#1F4E79").count(), 2, "{out}");
}

#[test]
fn the_dictionary_says_exactly_what_the_document_said() {
    let doc = repeated(6, marked());
    let interned = markdown(&doc);

    let mut assets = MemoryAssetStore::new();
    let (reparsed, _) = parse(&interned, &mut assets).expect("the document re-parses");

    // The IR that comes back has the attributes, not the class: expansion
    // happens before anything interprets a block, so nothing downstream can
    // tell a dictionary was used.
    let Document::Text(text) = &reparsed else {
        panic!("expected a text document");
    };
    let paragraphs: Vec<&Paragraph> = text.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(paragraphs.len(), 6);
    for paragraph in paragraphs {
        match &paragraph.content[0] {
            Inline::Styled { props, .. } => {
                assert_eq!(props.direct.color.as_deref(), Some("#1F4E79"));
                assert_eq!(props.direct.name.as_deref(), Some("Consolas"));
                assert_eq!(props.direct.size, Some(Length::from_pt(12.0)));
            }
            other => panic!("expected a styled run, got {other:?}"),
        }
    }

    // And the dictionary is rebuilt identically from the parsed document,
    // which is what makes it part of idempotence (spec §8).
    assert_eq!(
        markdown(&reparsed),
        interned,
        "serialising the re-parsed document must give the same bytes"
    );
}

#[test]
fn a_generated_name_never_lands_on_a_style() {
    let mut styles = StyleCatalog::default();
    // A document whose author happened to name a style `g1`.
    styles.insert(Style::new("g1", StyleType::Character));
    let doc = document(
        (0..6)
            .map(|n| {
                Block::Paragraph(Paragraph {
                    content: vec![run(&format!("termino {n}"), marked())],
                    ..Default::default()
                })
            })
            .collect(),
        styles,
    );
    let out = markdown(&doc);
    assert!(
        out.contains("  g2: \"color=#1F4E79 font=Consolas size=12pt\"\n"),
        "a name already taken must be skipped, not reused:\n{out}"
    );
    assert!(!out.contains("{.g1}"), "{out}");
}

#[test]
fn only_the_levels_with_formatting_get_a_dictionary() {
    let doc = repeated(6, marked());
    assert!(markdown_at(&doc, Fidelity::Full).contains("attribute-sets:"));
    assert!(markdown_at(&doc, Fidelity::Standard).contains("attribute-sets:"));
    // `agent` has already dropped the formatting a dictionary would compress,
    // and adding indirection to the level built to be read directly would
    // trade the wrong thing (spec §6.1).
    assert!(!markdown_at(&doc, Fidelity::Agent).contains("attribute-sets:"));
    assert!(!markdown_at(&doc, Fidelity::Plain).contains("attribute-sets:"));
}

#[test]
fn a_class_the_dictionary_does_not_define_is_left_alone() {
    // `.Enfasis` is a style, not an entry; expansion must not eat it.
    let mut styles = StyleCatalog::default();
    styles.insert(Style::new("Enfasis", StyleType::Character));
    let doc = document(
        (0..6)
            .map(|n| {
                Block::Paragraph(Paragraph {
                    content: vec![Inline::Styled {
                        content: vec![Inline::Text(format!("termino {n}"))],
                        props: RunProps {
                            style: Some(docsai_model::style::StyleId::new("Enfasis")),
                            direct: marked(),
                        },
                    }],
                    ..Default::default()
                })
            })
            .collect(),
        styles,
    );
    let out = markdown(&doc);
    assert!(out.contains("{.Enfasis .g1}"), "{out}");

    let mut assets = MemoryAssetStore::new();
    let (reparsed, _) = parse(&out, &mut assets).expect("re-parses");
    let Document::Text(text) = &reparsed else {
        panic!("expected a text document");
    };
    match &text.sections[0].blocks[0] {
        Block::Paragraph(p) => match &p.content[0] {
            Inline::Styled { props, .. } => {
                assert_eq!(props.style.as_ref().map(|s| s.as_str()), Some("Enfasis"));
                assert_eq!(props.direct.color.as_deref(), Some("#1F4E79"));
            }
            other => panic!("expected a styled run, got {other:?}"),
        },
        other => panic!("expected a paragraph, got {other:?}"),
    }
}
