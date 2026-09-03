//! Comments read, both formats, one node (Phase 16-E; spec §11.3).
//!
//! The two halves of the increment are asserted here together on purpose. A
//! `.docx` anchors a comment over a range of text and a `.pptx` pins one to a
//! point on a slide, and the whole design claim is that those are the same
//! [`Comment`] with a different [`CommentAnchor`]. A test file per format would
//! let the two drift into two models without anything failing.
//!
//! The docx side builds its packages here rather than in `corpus/`: the corpus
//! round-trip gate asserts `docx → DocMark → docx → DocMark` identity, and the
//! docx **writer** does not write comments back until Phase 16-F. A corpus
//! fixture would therefore fail a gate for a reason that is not a defect, so
//! the fixture lands with the writer. The deck fixture
//! (`corpus/pptx/comments-slide.pptx`) has no such problem: its comment parts
//! are preserved, so the deck comes back byte for byte whether or not the
//! writer understands them.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options, RawPolicy};
use docsai_model::text::{Block, Comment, CommentAnchor, Inline};
use docsai_model::{Document, Format, MemoryAssetStore, Warning};

// --------------------------------------------------------------------------
// A .docx built in memory
// --------------------------------------------------------------------------

const W_NS: &str = concat!(
    r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
    r#" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml""#,
    r#" xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml""#,
);

fn docx(document_body: &str, comments: &str, extended: Option<&str>) -> Vec<u8> {
    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let options: zip::write::FileOptions<'_, ()> = Default::default();
        let mut put = |name: &str, body: String| {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        };
        put(
            "word/document.xml",
            format!("<w:document{W_NS}><w:body>{document_body}</w:body></w:document>"),
        );
        put(
            "word/comments.xml",
            format!("<w:comments{W_NS}>{comments}</w:comments>"),
        );
        if let Some(extended) = extended {
            put(
                "word/commentsExtended.xml",
                format!("<w15:commentsEx{W_NS}>{extended}</w15:commentsEx>"),
            );
        }
        zip.finish().unwrap();
    }
    buffer
}

fn read_docx(bytes: Vec<u8>) -> (Document, MemoryAssetStore, docsai_model::ConversionReport) {
    let mut assets = MemoryAssetStore::new();
    let (document, report) =
        docsai_office::read_docx(Cursor::new(bytes), &mut assets).expect("the document reads");
    (document, assets, report)
}

/// Every comment in the first section, in document order.
fn comments_of(document: &Document) -> Vec<&Comment> {
    let Document::Text(text) = document else {
        panic!("not a text document")
    };
    let mut out = Vec::new();
    fn walk<'a>(blocks: &'a [Block], out: &mut Vec<&'a Comment>) {
        for block in blocks {
            if let Block::Paragraph(p) = block {
                for inline in &p.content {
                    if let Inline::Comment(comment) = inline {
                        out.push(comment.as_ref());
                    }
                }
            }
        }
    }
    for section in &text.sections {
        walk(&section.blocks, &mut out);
    }
    out
}

fn plain_of(inlines: &[Inline]) -> String {
    docsai_model::text::Paragraph::new(inlines.to_vec()).plain_text()
}

fn run(text: &str) -> String {
    format!("<w:r><w:t>{text}</w:t></w:r>")
}

/// One `w:comment`, with the `w14:paraId` that `commentsExtended` joins on.
fn comment_part(
    id: i64,
    author: &str,
    initials: &str,
    date: &str,
    para: &str,
    text: &str,
) -> String {
    format!(
        r#"<w:comment w:id="{id}" w:author="{author}" w:initials="{initials}" w:date="{date}">"#
    ) + &format!(
        r#"<w:p w14:paraId="{para}">{}</w:p></w:comment>"#,
        run(text)
    )
}

// --------------------------------------------------------------------------
// The docx reader
// --------------------------------------------------------------------------

#[test]
fn a_commented_range_carries_the_words_it_covers() {
    let body = format!(
        r#"<w:p>{}<w:commentRangeStart w:id="1"/>{}<w:commentRangeEnd w:id="1"/>{}{}</w:p>"#,
        run("Enviar el cierre antes del "),
        run("viernes"),
        r#"<w:r><w:commentReference w:id="1"/></w:r>"#,
        run(", antes del consejo."),
    );
    let comments = comment_part(
        1,
        "Ada Lovelace",
        "AL",
        "2026-05-04T09:12:00Z",
        "11111111",
        "El jueves es la fecha real.",
    );
    let (document, _, report) = read_docx(docx(&body, &comments, None));

    let found = comments_of(&document);
    assert_eq!(found.len(), 1, "one comment, not {}", found.len());
    let comment = found[0];
    assert_eq!(comment.author.as_deref(), Some("Ada Lovelace"));
    assert_eq!(comment.initials.as_deref(), Some("AL"));
    assert_eq!(comment.date.as_deref(), Some("2026-05-04T09:12:00Z"));
    assert!(!comment.resolved);
    assert_eq!(plain_of(comment.covered()), "viernes");
    assert_eq!(
        docsai_model::text::Paragraph::new(vec![]).plain_text(),
        "",
        "sanity"
    );
    let Some(Block::Paragraph(said)) = comment.blocks.first() else {
        panic!("a comment says something in blocks: {:?}", comment.blocks)
    };
    assert_eq!(said.plain_text(), "El jueves es la fecha real.");

    // The warning `body.rs` used to emit unconditionally said comments were
    // «out of scope in v1». They are in scope now, so it must be gone: a false
    // warning is the same failure as a missing one (`AGENTS.md` §7 rule 3).
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.message().contains("out of scope in v1")),
        "the v1 warning survived: {:?}",
        report.warnings
    );
}

#[test]
fn the_paragraph_still_reads_as_the_sentence_it_is() {
    // The comment is a node *inside* the flow, so the text under it must still
    // come out of `plain_text` exactly once and in place.
    let body = format!(
        r#"<w:p>{}<w:commentRangeStart w:id="1"/>{}<w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r>{}</w:p>"#,
        run("uno "),
        run("dos"),
        run(" tres"),
    );
    let comments = comment_part(1, "A", "A", "2026-01-01T00:00:00Z", "1", "nota");
    let (document, _, _) = read_docx(docx(&body, &comments, None));
    let Document::Text(text) = &document else {
        unreachable!()
    };
    let Block::Paragraph(p) = &text.sections[0].blocks[0] else {
        panic!("expected a paragraph")
    };
    assert_eq!(p.plain_text(), "uno dos tres");
}

#[test]
fn a_reference_with_no_range_is_a_comment_with_an_empty_anchor() {
    let body = format!(
        r#"<w:p>{}<w:r><w:commentReference w:id="4"/></w:r></w:p>"#,
        run("Una linea sin rango marcado."),
    );
    let comments = comment_part(4, "Ada", "A", "2026-05-04T09:12:00Z", "1", "sin rango");
    let (document, _, _) = read_docx(docx(&body, &comments, None));
    let found = comments_of(&document);
    assert_eq!(found.len(), 1);
    assert!(
        matches!(&found[0].anchor, CommentAnchor::Span(content) if content.is_empty()),
        "a bare reference anchors an empty span, got {:?}",
        found[0].anchor
    );
}

#[test]
fn a_thread_is_nesting_and_done_is_resolved() {
    let body = format!(
        r#"<w:p>{}<w:commentRangeStart w:id="1"/>{}<w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r>
           <w:commentRangeStart w:id="2"/><w:commentRangeEnd w:id="2"/><w:r><w:commentReference w:id="2"/></w:r></w:p>"#,
        run("El margen bruto mejora "),
        run("dos puntos"),
    );
    let comments = comment_part(1, "Ada", "AL", "2026-05-04T09:12:00Z", "AAA", "¿Seguro?")
        + &comment_part(
            2,
            "Grace",
            "GH",
            "2026-05-04T10:01:00Z",
            "BBB",
            "Confirmado.",
        );
    let extended = concat!(
        r#"<w15:commentEx w15:paraId="AAA" w15:done="1"/>"#,
        r#"<w15:commentEx w15:paraId="BBB" w15:paraIdParent="AAA" w15:done="0"/>"#,
    );
    let (document, _, _) = read_docx(docx(&body, &comments, Some(extended)));

    let found = comments_of(&document);
    assert_eq!(found.len(), 1, "a reply is not a second anchor: {found:?}");
    let thread = found[0];
    assert!(thread.resolved, "w15:done=1 is resolved");
    assert_eq!(thread.replies.len(), 1);
    assert_eq!(thread.replies[0].author.as_deref(), Some("Grace"));
    assert!(!thread.replies[0].resolved);
}

#[test]
fn a_range_that_outlives_its_paragraph_is_cut_where_the_paragraph_ends() {
    let body = format!(
        r#"<w:p><w:commentRangeStart w:id="1"/>{}</w:p><w:p>{}<w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r></w:p>"#,
        run("primera"),
        run("segunda"),
    );
    let comments = comment_part(1, "Ada", "A", "2026-05-04T09:12:00Z", "1", "abarca dos");
    let (document, _, report) = read_docx(docx(&body, &comments, None));
    let found = comments_of(&document);
    assert_eq!(found.len(), 1, "placed once, not once per paragraph");
    assert_eq!(plain_of(found[0].covered()), "primera");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message().contains("runs past the end of its paragraph")),
        "the shortened anchor must be warned: {:?}",
        report.warnings
    );
}

#[test]
fn a_reference_to_a_comment_nothing_defines_keeps_the_text_and_says_so() {
    let body = format!(
        r#"<w:p><w:commentRangeStart w:id="9"/>{}<w:commentRangeEnd w:id="9"/><w:r><w:commentReference w:id="9"/></w:r></w:p>"#,
        run("texto marcado"),
    );
    let (document, _, report) = read_docx(docx(&body, "", None));
    assert!(comments_of(&document).is_empty());
    let Document::Text(text) = &document else {
        unreachable!()
    };
    let Block::Paragraph(p) = &text.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(p.plain_text(), "texto marcado");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message().contains("no comment part defines it")),
        "{:?}",
        report.warnings
    );
}

// --------------------------------------------------------------------------
// The docx comment through DocMark
// --------------------------------------------------------------------------

fn options(fidelity: Fidelity, format: Format) -> Options {
    Options {
        fidelity,
        ids: docsai_model::addressing::IdPolicy::Assign,
        assets_dir: "assets".into(),
        source_format: format,
        raw: RawPolicy::Inline,
        precision: docsai_model::units::DEFAULT_PRECISION,
        dictionary: false,
    }
}

fn commented_document() -> Vec<u8> {
    let body = format!(
        r#"<w:p>{}<w:commentRangeStart w:id="1"/>{}<w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r>{}</w:p>"#,
        run("Enviar el cierre antes del "),
        run("viernes"),
        run(", antes del consejo."),
    );
    let comments = comment_part(
        1,
        "Ada Lovelace",
        "AL",
        "2026-05-04T09:12:00Z",
        "AAA",
        "El jueves es la fecha real.",
    ) + &comment_part(
        2,
        "Grace Hopper",
        "GH",
        "2026-05-04T10:01:00Z",
        "BBB",
        "De acuerdo, lo muevo.",
    );
    let extended = concat!(
        r#"<w15:commentEx w15:paraId="AAA" w15:done="0"/>"#,
        r#"<w15:commentEx w15:paraId="BBB" w15:paraIdParent="AAA"/>"#,
    );
    docx(&body, &comments, Some(extended))
}

#[test]
fn the_docmark_form_is_the_anchor_plus_the_container() {
    let (document, assets, _) = read_docx(commented_document());
    let (markdown, _) =
        docsai_docmark::serialize(&document, &assets, &options(Fidelity::Full, Format::Docx));
    assert!(
        markdown.contains(r#"docmark: "1.3""#),
        "a document with a comment is 1.3:\n{markdown}"
    );
    assert!(
        markdown.contains("[viernes]{.comment ref=c1}"),
        "the anchor is a bracketed span:\n{markdown}"
    );
    assert!(
        markdown.contains(
            r#"{#n2 .comment author="Ada Lovelace" date="2026-05-04T09:12:00Z" initials=AL ref=c1}"#
        ),
        "the container names the same ref:\n{markdown}"
    );
    assert!(
        markdown.contains(r#"author="Grace Hopper""#) && markdown.matches("::: {").count() == 2,
        "the reply is a nested container:\n{markdown}"
    );
}

#[test]
fn the_comment_survives_docmark_and_comes_back() {
    let (document, assets, _) = read_docx(commented_document());
    let (markdown, _) =
        docsai_docmark::serialize(&document, &assets, &options(Fidelity::Full, Format::Docx));
    let mut store = MemoryAssetStore::new();
    let (parsed, _) = docsai_docmark::parse(&markdown, &mut store).expect("it parses back");

    let found = comments_of(&parsed);
    assert_eq!(found.len(), 1, "\n{markdown}");
    assert_eq!(found[0].author.as_deref(), Some("Ada Lovelace"));
    assert_eq!(found[0].initials.as_deref(), Some("AL"));
    assert_eq!(plain_of(found[0].covered()), "viernes");
    assert_eq!(found[0].replies.len(), 1, "the thread survives");
    assert_eq!(found[0].replies[0].author.as_deref(), Some("Grace Hopper"));

    // And writing it again gives the same file, byte for byte.
    let (again, _) =
        docsai_docmark::serialize(&parsed, &store, &options(Fidelity::Full, Format::Docx));
    assert_eq!(again, markdown, "serialize(parse(md)) == md");
}

#[test]
fn standard_and_plain_drop_the_comment_and_say_so() {
    let (document, assets, _) = read_docx(commented_document());
    for level in [Fidelity::Standard, Fidelity::Plain] {
        let (markdown, report) =
            docsai_docmark::serialize(&document, &assets, &options(level, Format::Docx));
        assert!(
            !markdown.contains(".comment"),
            "{level} must write no comment:\n{markdown}"
        );
        assert!(
            markdown.contains("viernes"),
            "{level} keeps the commented text:\n{markdown}"
        );
        assert!(
            !markdown.contains(r#"docmark: "1.3""#),
            "{level} writes no comment, so it is not 1.3:\n{markdown}"
        );
        assert!(
            report.warnings.iter().any(|w| matches!(
                w,
                Warning::UnsupportedElement { kind, .. } if kind == "comment"
            )),
            "{level} drops the comment silently: {:?}",
            report.warnings
        );
    }
}

// --------------------------------------------------------------------------
// The pptx reader: the same node, a different anchor
// --------------------------------------------------------------------------

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/pptx")
        .join(name)
}

fn read_deck(name: &str) -> (Document, MemoryAssetStore, docsai_model::ConversionReport) {
    let path = corpus(name);
    let mut assets = MemoryAssetStore::new();
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (document, report) = docsai_office::read_pptx(file, &mut assets)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (document, assets, report)
}

fn deck_of(document: &Document) -> &docsai_model::Presentation {
    match document {
        Document::Presentation(deck) => deck,
        _ => panic!("not a deck"),
    }
}

#[test]
fn a_slide_comment_is_anchored_at_a_point_and_names_its_author() {
    let (document, _, _) = read_deck("comments-slide.pptx");
    let deck = deck_of(&document);
    let first = &deck.slides[0].comments;
    assert_eq!(first.len(), 2, "two comments on the first slide");
    assert_eq!(first[0].author.as_deref(), Some("Ada Lovelace"));
    assert_eq!(first[0].initials.as_deref(), Some("AL"));
    assert_eq!(first[0].date.as_deref(), Some("2026-02-03T09:12:00Z"));
    assert_eq!(first[1].author.as_deref(), Some("Grace Hopper"));
    let CommentAnchor::Point(pos) = &first[0].anchor else {
        panic!(
            "a slide comment is pinned to a point: {:?}",
            first[0].anchor
        )
    };
    assert_eq!(pos.x.emu(), 1_200_000);
    assert_eq!(pos.y.emu(), 900_000);
    // `p:cmLst` has no reply mechanism, so a classic deck has no thread.
    assert!(first.iter().all(|c| c.replies.is_empty()));
    assert!(first.iter().all(|c| !c.resolved));
}

#[test]
fn an_author_id_the_deck_does_not_define_keeps_the_comment_and_warns() {
    let (document, _, report) = read_deck("comments-slide.pptx");
    let second = &deck_of(&document).slides[1].comments;
    assert_eq!(second.len(), 1);
    assert_eq!(
        second[0].author, None,
        "not attributed to nobody in particular"
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message().contains("author id 7 is not defined")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn the_modern_comment_part_is_refused_by_name_and_not_by_silence() {
    let (_, _, report) = read_deck("comments-slide.pptx");
    let named = report
        .warnings
        .iter()
        .find(|w| matches!(w, Warning::UnsupportedElement { kind, .. } if kind == "p188:cmLst"));
    let Some(Warning::UnsupportedElement {
        location, action, ..
    }) = named
    else {
        panic!("the modern part must be reported: {:?}", report.warnings)
    };
    assert_eq!(location, "ppt/comments/modernComment_1.xml");
    assert!(
        action.contains("vnd.ms-powerpoint.comments+xml") && action.contains("preserved"),
        "the refusal has to be true and specific: {action}"
    );
}

#[test]
fn a_deck_whose_comments_nobody_touched_comes_back_byte_for_byte() {
    // The comment parts are read but never rebuilt, so the skeleton hands them
    // back untouched. This is the property Phase 16-F must not break.
    let path = corpus("comments-slide.pptx");
    let original = std::fs::read(&path).expect("the fixture is there");
    let (document, assets, _) = read_deck("comments-slide.pptx");
    let mut out = Cursor::new(Vec::new());
    docsai_office::write_pptx(&document, &assets, &mut out).expect("the deck writes");
    assert_eq!(
        out.into_inner(),
        original,
        "reading comments moved a part out of the preserved set"
    );
}

#[test]
fn a_slide_comment_writes_its_position_and_parses_back() {
    let (document, assets, _) = read_deck("comments-slide.pptx");
    let (markdown, _) =
        docsai_docmark::serialize(&document, &assets, &options(Fidelity::Full, Format::Pptx));
    assert!(
        markdown.contains(r#".comment author="Ada Lovelace""#) && markdown.contains("pos="),
        "a slide comment carries `pos=` and no `ref=`:\n{markdown}"
    );
    assert!(
        !markdown.contains(".comment ref="),
        "a slide comment has no anchor span:\n{markdown}"
    );
    let mut store = MemoryAssetStore::new();
    let (parsed, _) = docsai_docmark::parse(&markdown, &mut store).expect("it parses back");
    let deck = deck_of(&parsed);
    assert_eq!(deck.slides[0].comments.len(), 2);
    assert_eq!(
        deck.slides[0].comments[0].author.as_deref(),
        Some("Ada Lovelace")
    );
    assert!(matches!(
        deck.slides[0].comments[0].anchor,
        CommentAnchor::Point(_)
    ));
}
