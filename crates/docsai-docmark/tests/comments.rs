//! The DocMark form of a comment, and the three answers a second reader has to
//! give (spec §11.3, *Reading a document with comments back*).

use docsai_docmark::{Fidelity, Options, RawPolicy};
use docsai_model::addressing::IdPolicy;
use docsai_model::text::{Block, Comment, CommentAnchor, Inline};
use docsai_model::units::DEFAULT_PRECISION;
use docsai_model::{Document, Format, MemoryAssetStore};

fn options(fidelity: Fidelity) -> Options {
    Options {
        fidelity,
        ids: IdPolicy::Assign,
        assets_dir: "assets".into(),
        source_format: Format::Docx,
        raw: RawPolicy::Inline,
        precision: DEFAULT_PRECISION,
        dictionary: false,
    }
}

fn parse(markdown: &str) -> (Document, docsai_model::ConversionReport) {
    let mut assets = MemoryAssetStore::new();
    docsai_docmark::parse(markdown, &mut assets).expect("it parses")
}

fn comments_of(document: &Document) -> Vec<&Comment> {
    let Document::Text(text) = document else {
        panic!("not a text document")
    };
    let mut out = Vec::new();
    for section in &text.sections {
        for block in &section.blocks {
            if let Block::Paragraph(p) = block {
                for inline in &p.content {
                    if let Inline::Comment(comment) = inline {
                        out.push(comment.as_ref());
                    }
                }
            }
        }
    }
    out
}

fn plain(document: &Document) -> String {
    let Document::Text(text) = document else {
        panic!()
    };
    text.sections
        .iter()
        .flat_map(|s| s.blocks.iter())
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.plain_text()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const DOCUMENT: &str = concat!(
    "---\n",
    "docmark: \"1.3\"\n",
    "source-format: docx\n",
    "next-id: 12\n",
    "---\n",
    "\n",
    "Ship the reconciliation by [Friday]{.comment ref=c1}, before the board pack goes out. {#n11}\n",
    "\n",
    "::: {#n9 .comment author=\"Ada Lovelace\" date=\"2026-05-04T09:12:00Z\" initials=AL ref=c1}\n",
    "Thursday is the real deadline.\n",
    "\n",
    "::: {#n10 .comment author=\"Grace Hopper\" date=\"2026-05-04T10:01:00Z\" initials=GH}\n",
    "Agreed, moving it.\n",
    ":::\n",
    ":::\n",
);

#[test]
fn the_two_halves_come_back_as_one_node() {
    let (document, report) = parse(DOCUMENT);
    let found = comments_of(&document);
    assert_eq!(found.len(), 1, "{report:?}");
    let comment = found[0];
    assert_eq!(comment.author.as_deref(), Some("Ada Lovelace"));
    assert_eq!(comment.initials.as_deref(), Some("AL"));
    assert_eq!(comment.date.as_deref(), Some("2026-05-04T09:12:00Z"));
    assert_eq!(comment.replies.len(), 1);
    assert_eq!(comment.replies[0].author.as_deref(), Some("Grace Hopper"));
    let CommentAnchor::Span(covered) = &comment.anchor else {
        panic!("the anchor is a span: {:?}", comment.anchor)
    };
    assert_eq!(
        docsai_model::text::Paragraph::new(covered.clone()).plain_text(),
        "Friday"
    );
    // The prose is unchanged: a comment is not a word of the document.
    assert_eq!(
        plain(&document),
        "Ship the reconciliation by Friday, before the board pack goes out."
    );
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn writing_it_again_gives_the_same_file() {
    let (document, _) = parse(DOCUMENT);
    let store = MemoryAssetStore::new();
    let (markdown, _) = docsai_docmark::serialize(&document, &store, &options(Fidelity::Full));
    assert_eq!(markdown, DOCUMENT, "serialize(parse(md)) == md");
}

#[test]
fn an_anchor_with_no_container_keeps_its_text_and_warns() {
    let markdown = concat!(
        "---\ndocmark: \"1.3\"\nsource-format: docx\n---\n\n",
        "The deadline is [Friday]{.comment ref=c9}, no later.\n",
    );
    let (document, report) = parse(markdown);
    assert!(comments_of(&document).is_empty());
    assert_eq!(plain(&document), "The deadline is Friday, no later.");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message().contains("no `.comment` container declares it")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_container_no_anchor_points_at_is_kept_unanchored() {
    let markdown = concat!(
        "---\ndocmark: \"1.3\"\nsource-format: docx\n---\n\n",
        "A paragraph nobody commented on.\n",
        "\n",
        "::: {.comment author=\"Ada Lovelace\" ref=c4}\n",
        "This was about a sentence that is no longer here.\n",
        ":::\n",
    );
    let (document, report) = parse(markdown);
    let found = comments_of(&document);
    assert_eq!(found.len(), 1, "the comment is kept: {report:?}");
    assert_eq!(found[0].author.as_deref(), Some("Ada Lovelace"));
    assert!(
        found[0].covered().is_empty(),
        "an unanchored comment covers nothing: {:?}",
        found[0].anchor
    );
    assert!(
        report.warnings.iter().any(|w| w
            .message()
            .contains("no anchor in the document refers to it")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn a_hand_typed_comment_needs_no_attribute_at_all() {
    // Input is tolerant (analysis §6.6): a container with no author, no date
    // and no `ref=` is a comment somebody typed.
    let markdown = concat!(
        "---\ndocmark: \"1.3\"\nsource-format: docx\n---\n\n",
        "The numbers are provisional.\n",
        "\n",
        "::: {.comment}\n",
        "Where does this come from?\n",
        ":::\n",
    );
    let (document, _) = parse(markdown);
    let found = comments_of(&document);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].author, None);
    assert!(!found[0].resolved);
}

#[test]
fn resolved_is_only_true_when_it_says_true() {
    for (value, expected) in [("true", true), ("false", false), ("1", false)] {
        let markdown = format!(
            "---\ndocmark: \"1.3\"\nsource-format: docx\n---\n\nText.\n\n::: {{.comment resolved={value}}}\nnote\n:::\n"
        );
        let (document, _) = parse(&markdown);
        assert_eq!(
            comments_of(&document)[0].resolved,
            expected,
            "resolved={value}"
        );
    }
}

#[test]
fn a_document_with_no_comment_keeps_the_version_it_had() {
    let markdown = "---\ndocmark: \"1.0\"\nsource-format: docx\n---\n\nNothing to say here.\n";
    let (document, _) = parse(markdown);
    let store = MemoryAssetStore::new();
    let mut without_ids = options(Fidelity::Full);
    without_ids.ids = IdPolicy::Never;
    let (out, _) = docsai_docmark::serialize(&document, &store, &without_ids);
    assert!(out.starts_with("---\ndocmark: \"1.0\"\n"), "{out}");
}
