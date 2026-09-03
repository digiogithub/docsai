//! Comments read, both formats, one node (Phase 16-E; spec §11.3).
//!
//! The two halves of the increment are asserted here together on purpose. A
//! `.docx` anchors a comment over a range of text and a `.pptx` pins one to a
//! point on a slide, and the whole design claim is that those are the same
//! [`Comment`] with a different [`CommentAnchor`]. A test file per format would
//! let the two drift into two models without anything failing.
//!
//! The docx side builds most of its packages here rather than in `corpus/`,
//! because a case is easier to isolate in fifteen lines of XML than in a whole
//! document. The corpus does now carry one — `corpus/docx/comments-review.docx`,
//! which 16-E could not add because the docx writer did not write comments back
//! and `goldens.rs::docx_roundtrip_is_idempotent` runs over every file in that
//! directory. It does now, so the fixture landed with 16-F. The deck fixture is
//! `corpus/pptx/comments-slide.pptx`.
//!
//! **Phase 16-F adds the writing half here**, under the same rule: one file, so
//! that the two formats cannot drift into two models.

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
    // `p:pos` is written on PowerPoint's *master unit* grid — 1/576 in, which
    // is an eighth of a point — and not in the EMU `ST_Coordinate` claims; the
    // evidence is in `pptx::comments`' module documentation, and the fixture
    // now states the numbers a real deck states. `x="1134"` is 5 cm.
    assert_eq!(pos.x, docsai_model::units::Length::from_eighth_points(1134));
    assert_eq!(pos.y, docsai_model::units::Length::from_eighth_points(678));
    assert!(
        (pos.x.cm() - 5.0).abs() < 0.001,
        "1134 master units is 5 cm, not {} cm",
        pos.x.cm()
    );
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

// --------------------------------------------------------------------------
// The writers (Phase 16-F)
// --------------------------------------------------------------------------

/// Every member of a `.pptx`, by name, with its bytes.
fn members(package: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(package.to_vec())).expect("a zip");
    let mut out = std::collections::BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("a member");
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).expect("readable");
        out.insert(name, bytes);
    }
    out
}

fn write_deck(document: &Document, assets: &MemoryAssetStore) -> (Vec<u8>, Vec<Warning>) {
    let mut out = Cursor::new(Vec::new());
    let report = docsai_office::write_pptx(document, assets, &mut out).expect("the deck writes");
    (out.into_inner(), report.warnings)
}

fn deck_mut(document: &mut Document) -> &mut docsai_model::Presentation {
    match document {
        Document::Presentation(deck) => deck,
        _ => panic!("not a deck"),
    }
}

#[test]
fn a_comment_nobody_touched_produces_no_edit_and_no_warning() {
    // The other half of `a_deck_whose_comments_nobody_touched_…`: the bytes are
    // already asserted there, and what this asserts is that the silence is
    // real. A writer that rewrote the part and happened to produce the same
    // bytes would still have raised «it was copied from the preserved package».
    let (document, assets, _) = read_deck("comments-slide.pptx");
    let (_, warnings) = write_deck(&document, &assets);
    let about_comments: Vec<String> = warnings
        .iter()
        .map(|w| w.message())
        .filter(|m| m.contains("comment") || m.contains("author"))
        .collect();
    assert!(
        about_comments.is_empty(),
        "a deck nobody edited said something about its comments: {about_comments:?}"
    );
}

#[test]
fn a_comment_the_model_changed_reaches_exactly_one_member_in_exactly_one_place() {
    let original = std::fs::read(corpus("comments-slide.pptx")).expect("the fixture");
    let (mut document, assets, _) = read_deck("comments-slide.pptx");
    deck_mut(&mut document).slides[0].comments[0].blocks = vec![Block::Paragraph(
        docsai_model::text::Paragraph::text("La cifra es la definitiva del cierre auditado."),
    )];
    let (written, _) = write_deck(&document, &assets);

    let before = members(&original);
    let after = members(&written);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "editing a comment added or removed a package member"
    );
    let differing: Vec<&String> = before
        .keys()
        .filter(|name| before[*name] != after[*name])
        .collect();
    assert_eq!(
        differing,
        vec!["ppt/comments/comment1.xml"],
        "exactly one member differs"
    );

    // And inside it, exactly one place: the `p:text` of the first comment. The
    // second comment, the positions, the ids and the XML declaration stand.
    let was = String::from_utf8(before["ppt/comments/comment1.xml"].clone()).unwrap();
    let now = String::from_utf8(after["ppt/comments/comment1.xml"].clone()).unwrap();
    assert_eq!(
        was.replace(
            "La cifra de ingresos viene del cierre provisional.",
            "La cifra es la definitiva del cierre auditado."
        ),
        now,
        "the edit was not confined to the one string that changed"
    );

    // It reads back as what was written.
    let mut store = MemoryAssetStore::new();
    let (reread, _) = docsai_office::read_pptx(Cursor::new(written), &mut store).expect("reads");
    let Document::Presentation(deck) = &reread else {
        panic!("not a deck")
    };
    let Some(Block::Paragraph(said)) = deck.slides[0].comments[0].blocks.first() else {
        panic!("a comment says something")
    };
    assert_eq!(
        said.plain_text(),
        "La cifra es la definitiva del cierre auditado."
    );
}

#[test]
fn a_comment_added_to_a_slide_that_had_none_brings_the_parts_into_existence() {
    let (mut document, assets, _) = read_deck("basic-slides.pptx");
    let before = members(&std::fs::read(corpus("basic-slides.pptx")).expect("the fixture"));
    assert!(
        !before.keys().any(|name| name.starts_with("ppt/comments/")),
        "the fixture must start with no comment part"
    );
    let mut comment = Comment::new(vec![Block::Paragraph(docsai_model::text::Paragraph::text(
        "Revisar la cifra de la primera linea.",
    ))]);
    comment.author = Some("Ada Lovelace".into());
    comment.initials = Some("AL".into());
    comment.date = Some("2026-05-04T09:12:00Z".into());
    comment.anchor = CommentAnchor::Point(docsai_model::units::Point::new(
        docsai_model::units::Length::from_eighth_points(1134),
        docsai_model::units::Length::from_eighth_points(678),
    ));
    deck_mut(&mut document).slides[0].comments.push(comment);
    let (written, _) = write_deck(&document, &assets);
    let after = members(&written);

    assert!(
        after.contains_key("ppt/comments/comment1.xml"),
        "the comment part was not written: {:?}",
        after.keys().collect::<Vec<_>>()
    );
    assert!(
        after.contains_key("ppt/commentAuthors.xml"),
        "the author table was not written"
    );
    let types = String::from_utf8(after["[Content_Types].xml"].clone()).unwrap();
    assert!(
        types.contains("/ppt/comments/comment1.xml") && types.contains("/ppt/commentAuthors.xml"),
        "a part with no content type is the one thing OPC refuses:\n{types}"
    );
    let slide_rels = String::from_utf8(after["ppt/slides/_rels/slide1.xml.rels"].clone()).unwrap();
    assert!(
        slide_rels.contains("../comments/comment1.xml"),
        "the slide does not point at its comment part:\n{slide_rels}"
    );

    // And the deck reads back as the deck that was written.
    let mut store = MemoryAssetStore::new();
    let (reread, _) = docsai_office::read_pptx(Cursor::new(written), &mut store).expect("reads");
    let Document::Presentation(deck) = &reread else {
        panic!("not a deck")
    };
    assert_eq!(deck.slides[0].comments.len(), 1);
    assert_eq!(
        deck.slides[0].comments[0].author.as_deref(),
        Some("Ada Lovelace")
    );
    assert!(matches!(
        deck.slides[0].comments[0].anchor,
        CommentAnchor::Point(_)
    ));
}

#[test]
fn a_new_author_is_appended_and_the_ids_the_deck_already_gave_do_not_move() {
    let (mut document, assets, _) = read_deck("comments-slide.pptx");
    let mut comment = Comment::new(vec![Block::Paragraph(docsai_model::text::Paragraph::text(
        "Anado una linea al hilo.",
    ))]);
    comment.author = Some("Alan Turing".into());
    comment.anchor = CommentAnchor::Point(docsai_model::units::Point::new(
        docsai_model::units::Length::from_eighth_points(600),
        docsai_model::units::Length::from_eighth_points(600),
    ));
    deck_mut(&mut document).slides[0].comments.push(comment);
    let (written, _) = write_deck(&document, &assets);
    let after = members(&written);
    let authors = String::from_utf8(after["ppt/commentAuthors.xml"].clone()).unwrap();

    // The two the deck already had keep their ids and their bytes.
    assert!(
        authors.contains(r#"id="1" name="Ada Lovelace" initials="AL""#),
        "{authors}"
    );
    assert!(
        authors.contains(r#"id="2" name="Grace Hopper" initials="GH""#),
        "{authors}"
    );
    // The new one is appended after the last, with initials derived from the
    // name because `p:cmAuthor@initials` is required and DocMark stated none.
    assert!(
        authors.contains(r#"id="3" name="Alan Turing" initials="AT""#),
        "{authors}"
    );
    let comment1 = String::from_utf8(after["ppt/comments/comment1.xml"].clone()).unwrap();
    assert!(
        comment1.contains(r#"authorId="1""#) && comment1.contains(r#"authorId="2""#),
        "an existing comment was renumbered:\n{comment1}"
    );
    assert!(comment1.contains(r#"authorId="3""#), "{comment1}");
}

#[test]
fn a_slide_whose_last_comment_was_deleted_loses_the_part_the_relationship_and_the_override() {
    let (mut document, assets, _) = read_deck("comments-slide.pptx");
    deck_mut(&mut document).slides[0].comments.clear();
    let (written, _) = write_deck(&document, &assets);
    let after = members(&written);

    assert!(
        !after.contains_key("ppt/comments/comment1.xml"),
        "the part stayed behind"
    );
    assert!(
        after.contains_key("ppt/comments/comment2.xml"),
        "the other slide's comments must not be touched"
    );
    let types = String::from_utf8(after["[Content_Types].xml"].clone()).unwrap();
    assert!(
        !types.contains("/ppt/comments/comment1.xml"),
        "an override naming a part the package does not carry:\n{types}"
    );
    let rels = String::from_utf8(after["ppt/slides/_rels/slide1.xml.rels"].clone()).unwrap();
    assert!(
        !rels.contains("comment1.xml"),
        "a relationship pointing at nothing:\n{rels}"
    );
    let mut store = MemoryAssetStore::new();
    let (reread, report) =
        docsai_office::read_pptx(Cursor::new(written), &mut store).expect("reads");
    let Document::Presentation(deck) = &reread else {
        panic!("not a deck")
    };
    assert!(deck.slides[0].comments.is_empty());
    assert_eq!(deck.slides[1].comments.len(), 1);
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.message().contains("comment1.xml")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn what_a_classic_deck_cannot_hold_is_named_and_never_silently_dropped() {
    let (mut document, assets, _) = read_deck("comments-slide.pptx");
    {
        let comment = &mut deck_mut(&mut document).slides[0].comments[0];
        comment.resolved = true;
        comment.replies = vec![Comment::new(vec![Block::Paragraph(
            docsai_model::text::Paragraph::text("De acuerdo."),
        )])];
        comment.date = Some("ayer por la tarde".into());
        comment
            .blocks
            .push(Block::Table(docsai_model::text::Table::default()));
    }
    let (written, warnings) = write_deck(&document, &assets);
    let said: Vec<String> = warnings.iter().map(|w| w.message()).collect();
    let joined = said.join("\n");
    for expected in [
        "repl",      // the reply `p:cmLst` cannot hold
        "done flag", // the resolved mark
        "xsd:dateTime",
        "not a paragraph",
    ] {
        assert!(
            joined.contains(expected),
            "no warning named `{expected}`:\n{joined}"
        );
    }
    // And the part is still a valid one: no `dt` at all rather than a `dt`
    // the schema rejects.
    let after = members(&written);
    let part = String::from_utf8(after["ppt/comments/comment1.xml"].clone()).unwrap();
    assert!(!part.contains("ayer"), "{part}");
}

#[test]
fn a_docx_comment_thread_survives_the_writer() {
    // The docx writer regenerates the whole package (it holds no skeleton), so
    // its contract is the model's, not the bytes': what the reader read must
    // come back out of what the writer wrote.
    let (document, assets, _) = read_docx(commented_document());
    let mut out = Cursor::new(Vec::new());
    let report = docsai_office::write_docx(&document, &assets, &mut out).expect("writes");
    assert!(
        !report
            .warnings
            .iter()
            .any(|w| w.message().contains("does not write comments back")),
        "the 16-E warning survived: {:?}",
        report.warnings
    );
    let written = out.into_inner();
    let (reread, _, _) = read_docx(written.clone());
    let found = comments_of(&reread);
    let before = comments_of(&document);
    assert_eq!(
        found.len(),
        before.len(),
        "a comment was lost on the way out"
    );
    for (was, now) in before.iter().zip(&found) {
        assert_eq!(was.author, now.author);
        assert_eq!(was.initials, now.initials);
        assert_eq!(was.date, now.date);
        assert_eq!(was.resolved, now.resolved);
        assert_eq!(was.replies.len(), now.replies.len());
        assert_eq!(plain_of(was.covered()), plain_of(now.covered()));
    }

    // The two parts, their content types and the relationship that binds the
    // Microsoft extension.
    let mut zip = zip::ZipArchive::new(Cursor::new(written)).expect("a zip");
    let names: Vec<String> = (0..zip.len())
        .map(|i| zip.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "word/comments.xml"), "{names:?}");
    assert!(
        names.iter().any(|n| n == "word/commentsExtended.xml"),
        "{names:?}"
    );
    let read_part = |zip: &mut zip::ZipArchive<Cursor<Vec<u8>>>, name: &str| {
        let mut entry = zip.by_name(name).expect(name);
        let mut text = String::new();
        std::io::Read::read_to_string(&mut entry, &mut text).unwrap();
        text
    };
    let types = read_part(&mut zip, "[Content_Types].xml");
    assert!(types.contains("/word/comments.xml"), "{types}");
    assert!(types.contains("/word/commentsExtended.xml"), "{types}");
    let rels = read_part(&mut zip, "word/_rels/document.xml.rels");
    assert!(
        rels.contains("http://schemas.microsoft.com/office/2011/relationships/commentsExtended"),
        "commentsExtended is not an ECMA-376 relationship type:\n{rels}"
    );
    let extended = read_part(&mut zip, "word/commentsExtended.xml");
    assert!(extended.contains("w15:paraIdParent"), "{extended}");
}

#[test]
fn a_docx_comment_marker_inside_a_field_is_dropped_on_read_and_cannot_be_written() {
    // 16-E's carried defect 5, settled from the write side. A comment marker
    // between `fldChar begin` and `end` is dropped on read with a named
    // warning, and the writer is consistent with that by construction: what
    // the model then holds is a `Inline::Field`, whose `cached` is a string,
    // so there is nothing for the writer to claim it wrote.
    let body = format!(
        r#"<w:p>{}<w:r><w:fldChar w:fldCharType="begin"/></w:r>\
<w:r><w:instrText> PAGE </w:instrText></w:r>\
<w:commentRangeStart w:id="1"/>\
<w:r><w:fldChar w:fldCharType="separate"/></w:r>{}\
<w:commentRangeEnd w:id="1"/>\
<w:r><w:commentReference w:id="1"/></w:r>\
<w:r><w:fldChar w:fldCharType="end"/></w:r>{}</w:p>"#,
        run("Pagina "),
        run("7"),
        run(" de 12."),
    );
    let comments = comment_part(1, "Ada", "A", "2026-05-04T09:12:00Z", "1", "revisar");
    let (document, assets, report) = read_docx(docx(&body, &comments, None));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message().contains("anchor falls inside a field")),
        "the loss must be named: {:?}",
        report.warnings
    );
    assert!(
        comments_of(&document).is_empty(),
        "the comment cannot be in the model if its anchor was refused"
    );
    let mut out = Cursor::new(Vec::new());
    let written = docsai_office::write_docx(&document, &assets, &mut out).expect("writes");
    assert!(
        !written
            .warnings
            .iter()
            .any(|w| w.message().contains("comment")),
        "the writer must not claim anything about a comment it never received: {:?}",
        written.warnings
    );
    let mut zip = zip::ZipArchive::new(Cursor::new(out.into_inner())).expect("a zip");
    assert!(
        (0..zip.len()).all(|i| zip.by_index(i).unwrap().name() != "word/comments.xml"),
        "a comments part with no comment in it"
    );
}

#[test]
fn a_generated_deck_carries_its_comments_too() {
    // A deck typed as DocMark has no preserved package (15-G), so its slide is
    // *written* rather than spliced — and its comment part, its author table
    // and the relationships that bind them do not exist anywhere to be edited.
    // This is the «the source had no part for it» case at its widest.
    let markdown = "---\n\
docmark: \"1.3\"\n\
source-format: pptx\n\
---\n\n\
## Cierre del trimestre {#n1 .slide}\n\n\
- Los ingresos crecen un doce por ciento.\n\n\
::: {#n2 .comment author=\"Ada Lovelace\" initials=AL date=\"2026-05-04T09:12:00Z\" pos=\"5cm,3cm\"}\n\
Revisar la cifra con tesoreria.\n\
:::\n";
    let mut store = MemoryAssetStore::new();
    let (document, _) = docsai_docmark::parse(markdown, &mut store).expect("it parses");
    let (written, _) = write_deck(&document, &store);
    let after = members(&written);
    assert!(
        after.contains_key("ppt/comments/comment1.xml")
            && after.contains_key("ppt/commentAuthors.xml"),
        "{:?}",
        after.keys().collect::<Vec<_>>()
    );
    let main_rels = String::from_utf8(after["ppt/_rels/presentation.xml.rels"].clone()).unwrap();
    assert!(
        main_rels.contains("commentAuthors.xml"),
        "the author table hangs off the deck's own part:\n{main_rels}"
    );

    let mut back = MemoryAssetStore::new();
    let (reread, _) = docsai_office::read_pptx(Cursor::new(written), &mut back).expect("reads");
    let Document::Presentation(deck) = &reread else {
        panic!("not a deck")
    };
    assert_eq!(deck.slides[0].comments.len(), 1);
    assert_eq!(
        deck.slides[0].comments[0].author.as_deref(),
        Some("Ada Lovelace")
    );
}
