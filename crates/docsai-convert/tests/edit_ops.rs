//! The operation set, applied: `docs/edit-operations.md` §6 and §11's matrix.
//!
//! Every operation is exercised on every document class §11 says it applies to,
//! and each `—` of that table is exercised too, because §11 is explicit that
//! «a `—` is a `NotApplicableToClass` or a `WrongKind`, and its refusal is
//! itself the test».
//!
//! **text** is only satisfied by both `.docx` and `.odt` and **workbook** by
//! both `.xlsx` and `.ods` (§2), so the helpers below take a list of packages
//! and not one.

use std::path::{Path, PathBuf};

use docsai_convert::edit::{edit_path, EditOptions};
use docsai_convert::{ConvertOptions, EditRequest, Operation, Refusal, Scalar};
use docsai_model::addressing::NodeKind;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

/// A private copy of a corpus document, so a test never edits the corpus.
fn copy(rel: &str, dir: &Path) -> PathBuf {
    let source = corpus().join(rel);
    let name = source.file_name().expect("a file name");
    let target = dir.join(name);
    std::fs::copy(&source, &target).unwrap_or_else(|e| panic!("{}: {e}", source.display()));
    target
}

/// The same document as DocMark, which is §2's fourth class.
fn as_docmark(rel: &str, dir: &Path) -> PathBuf {
    let source = corpus().join(rel);
    let stem = source
        .file_stem()
        .expect("a stem")
        .to_string_lossy()
        .into_owned();
    let target = dir.join(format!("{stem}.dmk.md"));
    docsai_convert::convert_file(&source, Some(&target), &ConvertOptions::default())
        .unwrap_or_else(|e| panic!("{}: {e}", source.display()));
    target
}

/// Every addressable node of a document, in document order — the same list an
/// edit resolves against.
fn nodes(path: &Path) -> Vec<(String, NodeKind, String)> {
    let mut store = docsai_model::MemoryAssetStore::new();
    let (mut doc, _, _) = docsai_convert::read_path(path, &mut store)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    docsai_model::addressing::assign_ids(&mut doc, docsai_model::addressing::IdPolicy::Assign);
    docsai_model::addressing::ids_in_document_order(&doc)
        .into_iter()
        .map(|(id, kind)| (id.0, kind, String::new()))
        .collect()
}

/// The document, read and addressed the way an edit addresses it.
fn document(path: &Path) -> docsai_model::Document {
    let mut store = docsai_model::MemoryAssetStore::new();
    let (mut doc, _, _) = docsai_convert::read_path(path, &mut store)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    docsai_model::addressing::assign_ids(&mut doc, docsai_model::addressing::IdPolicy::Assign);
    doc
}

/// The first target that names a place `pred` accepts, searched in document
/// order, preferring an id over an index path exactly as a caller would.
///
/// Ordinary prose carries **no id** — Phase 10 gives one only to a paragraph
/// that contains something addressable — so a test that wants a paragraph has
/// to reach it by §5.1's index path, which is the whole reason index paths are
/// in the grammar.
fn find_target(
    path: &Path,
    pred: &dyn Fn(&docsai_model::addressing::Place<'_>) -> bool,
) -> Option<String> {
    fn walk(
        place: docsai_model::addressing::Place<'_>,
        here: String,
        pred: &dyn Fn(&docsai_model::addressing::Place<'_>) -> bool,
        out: &mut Option<String>,
    ) {
        if out.is_some() {
            return;
        }
        let name = match place.id() {
            Some(id) => id.0.clone(),
            None => here.clone(),
        };
        if pred(&place) {
            *out = Some(name.clone());
            return;
        }
        for (index, child) in place.children().into_iter().enumerate() {
            walk(child, format!("{name}/{}", index + 1), pred, out);
        }
    }
    let doc = document(path);
    let mut out = None;
    walk(
        docsai_model::addressing::Place::Doc(&doc),
        "doc".into(),
        pred,
        &mut out,
    );
    out
}

fn is_paragraph(place: &docsai_model::addressing::Place<'_>) -> bool {
    matches!(
        place,
        docsai_model::addressing::Place::Block(docsai_model::text::Block::Paragraph(_))
    )
}

fn paragraph(path: &Path) -> String {
    find_target(path, &is_paragraph)
        .unwrap_or_else(|| panic!("{} has no paragraph", path.display()))
}

fn first(path: &Path, kind: NodeKind) -> String {
    nodes(path)
        .into_iter()
        .find(|(_, k, _)| *k == kind)
        .unwrap_or_else(|| panic!("{} has no {kind}", path.display()))
        .0
}

/// The document's own DocMark, which is what an assertion reads.
fn text(path: &Path) -> String {
    let dir = path.parent().expect("a directory").join("view");
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join("view.dmk.md");
    docsai_convert::convert_file(path, Some(&out), &ConvertOptions::default())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    std::fs::read_to_string(&out).expect("reads the view")
}

fn run(path: &Path, operations: Vec<Operation>) -> docsai_convert::EditReport {
    edit_path(path, &EditRequest::new(operations), &EditOptions::default())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .report
}

fn refuse(path: &Path, operations: Vec<Operation>) -> Refusal {
    match edit_path(path, &EditRequest::new(operations), &EditOptions::default()) {
        Err(docsai_convert::EditError::Refused(refused)) => refused.refusal,
        other => panic!("{}: expected a refusal, got {other:?}", path.display()),
    }
}

fn replace_text(target: &str, text: &str) -> Operation {
    Operation::ReplaceText {
        target: target.into(),
        text: text.into(),
        if_match: None,
    }
}

// ------------------------------------------------------------------- §6.1

#[test]
fn replace_text_applies_to_text_presentation_and_docmark() {
    let dir = scratch("replace-text");
    let cases = [
        copy("docx/long-report.docx", &dir),
        copy("odt/basic-text.odt", &dir),
        copy("pptx/basic-slides.pptx", &dir),
        as_docmark("docx/long-report.docx", &dir),
        as_docmark("pptx/basic-slides.pptx", &dir),
    ];
    for path in cases {
        let target = paragraph(&path);
        let report = run(&path, vec![replace_text(&target, "Resultados del Q3")]);
        assert_eq!(report.changed.len(), 1, "{}", path.display());
        assert_eq!(report.changed[0].id, target, "{}", path.display());
        assert!(
            text(&path).contains("Resultados del Q3"),
            "{} does not carry the new text",
            path.display()
        );
        // 17-D's whole economic argument: the etag in the report is the one a
        // reader computes, so no confirmation read is needed.
        let mut store = docsai_model::MemoryAssetStore::new();
        let (mut doc, _, _) = docsai_convert::read_path(&path, &mut store).expect("re-reads");
        docsai_model::addressing::assign_ids(&mut doc, docsai_model::addressing::IdPolicy::Assign);
        let place = docsai_model::addressing::resolve_str(&doc, &target).expect("resolves");
        assert_eq!(
            place.etag().expect("an etag").0,
            report.changed[0].etag,
            "{}: the reported etag is not what a read computes",
            path.display()
        );
    }
}

#[test]
fn replace_text_refuses_a_workbook_and_a_chart() {
    let dir = scratch("replace-text-refusals");
    let book = copy("xlsx/values-types.xlsx", &dir);
    let sheet = first(&book, NodeKind::Sheet);
    assert!(matches!(
        refuse(&book, vec![replace_text(&sheet, "no")]),
        Refusal::WrongKind { .. }
    ));

    let deck = copy("pptx/charts-embedded.pptx", &dir);
    let chart = nodes(&deck)
        .into_iter()
        .find(|(id, kind, _)| {
            *kind == NodeKind::Shape && {
                let mut store = docsai_model::MemoryAssetStore::new();
                let (mut doc, _, _) = docsai_convert::read_path(&deck, &mut store).expect("reads");
                docsai_model::addressing::assign_ids(
                    &mut doc,
                    docsai_model::addressing::IdPolicy::Assign,
                );
                matches!(
                    docsai_model::addressing::resolve_str(&doc, id),
                    Ok(docsai_model::addressing::Place::Shape(shape))
                        if matches!(shape.kind, docsai_model::presentation::ShapeKind::Chart(_))
                )
            }
        })
        .map(|(id, _, _)| id);
    // Not `if let`: a corpus that stopped carrying an embedded chart would
    // turn this case into a silent pass, which is the failure mode 17-H exists
    // to find.
    let chart = chart.expect("charts-embedded.pptx carries a chart shape");
    match refuse(&deck, vec![replace_text(&chart, "no")]) {
        Refusal::WrongKind { because, .. } => assert!(
            because.contains("§9"),
            "a chart refusal must name the escape hatch: {because}"
        ),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------- §6.2

#[test]
fn set_props_applies_and_refuses_an_unknown_key() {
    let dir = scratch("set-props");
    for path in [
        copy("docx/basic-text.docx", &dir),
        copy("odt/basic-text.odt", &dir),
        copy("pptx/basic-slides.pptx", &dir),
        as_docmark("docx/basic-text.docx", &dir),
    ] {
        let target = paragraph(&path);
        let props = [("bold".to_string(), Some(Scalar::Bool(true)))]
            .into_iter()
            .collect();
        let report = run(
            &path,
            vec![Operation::SetProps {
                target: target.clone(),
                props,
                if_match: None,
            }],
        );
        assert_eq!(report.changed.len(), 1, "{}", path.display());

        // The round trip, which the report alone does not prove: §6.2 says the
        // property applies to «the paragraph's whole content», so the
        // paragraph's own text must come back marked. Asserting only
        // `changed.len() == 1` let a `set_props` that wrote nothing a reader
        // can see pass for four increments (17-H).
        assert!(
            text(&path).contains("**"),
            "{}: set_props reported a change the document does not show",
            path.display()
        );

        // §6.2's `highlight` is `w:highlight/@val`, an enumeration and not a
        // colour. A hex value is refused by name rather than written verbatim
        // into a package that will not honour it.
        let props = [("highlight".to_string(), Some(Scalar::Text("FFFF00".into())))]
            .into_iter()
            .collect();
        match refuse(
            &path,
            vec![Operation::SetProps {
                target: target.clone(),
                props,
                if_match: None,
            }],
        ) {
            Refusal::UnknownField { known, .. } => assert!(
                known.contains("yellow"),
                "{}: the refusal must name the highlights: {known}",
                path.display()
            ),
            other => panic!("{other:?}"),
        }
        let props = [("highlight".to_string(), Some(Scalar::Text("yellow".into())))]
            .into_iter()
            .collect();
        run(
            &path,
            vec![Operation::SetProps {
                target: target.clone(),
                props,
                if_match: None,
            }],
        );

        let props = [("bould".to_string(), Some(Scalar::Bool(true)))]
            .into_iter()
            .collect();
        assert!(
            matches!(
                refuse(
                    &path,
                    vec![Operation::SetProps {
                        target: target.clone(),
                        props,
                        if_match: None,
                    }]
                ),
                Refusal::UnknownField { .. }
            ),
            "{}: a misspelled property must refuse, not be ignored",
            path.display()
        );
    }
}

// ------------------------------------------------------------------- §6.3

#[test]
fn insert_after_and_before_apply_to_all_four_classes() {
    let dir = scratch("insert");
    // text, both packages, and DocMark
    for path in [
        copy("docx/basic-text.docx", &dir),
        copy("odt/basic-text.odt", &dir),
        as_docmark("docx/basic-text.docx", &dir),
    ] {
        let target = paragraph(&path);
        let report = run(
            &path,
            vec![Operation::InsertAfter {
                target: target.clone(),
                content: "Ingresos al alza un 12 %".into(),
                if_match: None,
            }],
        );
        assert!(text(&path).contains("Ingresos al alza un 12 %"));
        // §5.4 allocates an id to «every *addressable* node» of the fragment,
        // and Phase 10 gives a plain paragraph none on purpose. So `created` is
        // empty here and that is the specified answer, not a miss: the new
        // block is reached by the index path the insert position names.
        assert!(report.created.is_empty(), "{}: {report:?}", path.display());

        run(
            &path,
            vec![Operation::InsertBefore {
                target,
                content: "Antes de todo".into(),
                if_match: None,
            }],
        );
        assert!(text(&path).contains("Antes de todo"));

        // A fragment that *is* addressable comes back named.
        let report = run(
            &path,
            vec![Operation::InsertAfter {
                target: "doc/1".into(),
                content: "| a | b |\n|---|---|\n| 1 | 2 |\n".into(),
                if_match: None,
            }],
        );
        assert!(
            report.created.iter().any(|n| n.kind == NodeKind::Table),
            "{}: {report:?}",
            path.display()
        );
    }

    // presentation: a slide is inserted as a slide
    for path in [
        copy("pptx/basic-slides.pptx", &dir),
        as_docmark("pptx/basic-slides.pptx", &dir),
    ] {
        let target = first(&path, NodeKind::Slide);
        let report = run(
            &path,
            vec![Operation::InsertAfter {
                target,
                content: "## Diapositiva nueva {.slide}\n\nUn párrafo.\n".into(),
                if_match: None,
            }],
        );
        assert!(
            report.created.iter().any(|n| n.kind == NodeKind::Slide),
            "{}: no slide was created: {report:?}",
            path.display()
        );
        assert!(text(&path).contains("Diapositiva nueva"));
    }

    // workbook: a sheet is inserted as a sheet
    for path in [
        copy("xlsx/values-types.xlsx", &dir),
        copy("ods/values-types.ods", &dir),
    ] {
        let target = first(&path, NodeKind::Sheet);
        let report = run(
            &path,
            vec![Operation::InsertAfter {
                target,
                content: "# Hoja nueva {.sheet}\n\n| A1 |\n|---|\n| uno |\n".into(),
                if_match: None,
            }],
        );
        assert!(
            report.created.iter().any(|n| n.kind == NodeKind::Sheet),
            "{}: no sheet was created: {report:?}",
            path.display()
        );
    }
}

#[test]
fn a_fragment_of_the_wrong_kind_is_refused_and_names_both_sides() {
    let dir = scratch("insert-mismatch");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let slide = first(&deck, NodeKind::Slide);
    match refuse(
        &deck,
        vec![Operation::InsertAfter {
            target: slide,
            content: "Un párrafo suelto".into(),
            if_match: None,
        }],
    ) {
        Refusal::ContentKindMismatch { expected, got, .. } => {
            assert_eq!(expected, "slides");
            assert_eq!(got, "blocks");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_id_the_document_already_uses_is_refused_rather_than_renumbered() {
    let dir = scratch("duplicate-id");
    let path = as_docmark("docx/long-report.docx", &dir);
    let taken = nodes(&path)
        .into_iter()
        .next()
        .expect("the document carries an id")
        .0;
    match refuse(
        &path,
        vec![Operation::InsertAfter {
            target: "doc/1".into(),
            content: format!("Un párrafo con una nota[^1] {{#{taken}}}\n\n[^1]: la nota\n"),
            if_match: None,
        }],
    ) {
        Refusal::DuplicateId { id } => assert_eq!(id, taken),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------- §6.4

#[test]
fn delete_applies_to_all_four_classes() {
    let dir = scratch("delete");
    for path in [
        copy("docx/long-report.docx", &dir),
        copy("odt/basic-text.odt", &dir),
        as_docmark("docx/long-report.docx", &dir),
    ] {
        let target = paragraph(&path);
        let before = top_level_blocks(&path);
        let report = run(
            &path,
            vec![Operation::Delete {
                target: target.clone(),
                if_match: None,
            }],
        );
        assert_eq!(report.deleted, vec![target.clone()]);
        assert_eq!(
            top_level_blocks(&path),
            before - 1,
            "{}: nothing left the document",
            path.display()
        );
    }

    for path in [
        copy("pptx/slide-order.pptx", &dir),
        as_docmark("pptx/slide-order.pptx", &dir),
        two_sheets("xlsx/values-types.xlsx", &dir),
        two_sheets("ods/values-types.ods", &dir),
    ] {
        let (kind, count): (NodeKind, fn(&Path) -> usize) =
            if path.to_string_lossy().contains("values-types")
                && path.extension().is_some_and(|e| e != "md")
            {
                (NodeKind::Sheet, sheets)
            } else {
                (NodeKind::Slide, slides)
            };
        let target = first(&path, kind);
        let before = count(&path);
        run(
            &path,
            vec![Operation::Delete {
                target,
                if_match: None,
            }],
        );
        assert_eq!(count(&path), before - 1, "{}", path.display());
    }
}

/// A copy of a single-sheet workbook with a second sheet inserted, because the
/// corpus has none with two and §6.4 refuses to delete the last one.
fn two_sheets(rel: &str, dir: &Path) -> PathBuf {
    let path = copy(rel, dir);
    let sheet = first(&path, NodeKind::Sheet);
    run(
        &path,
        vec![Operation::InsertAfter {
            target: sheet,
            content: "# Hoja de paso {.sheet}\n\n| A |\n|---|\n| uno |\n".into(),
            if_match: None,
        }],
    );
    path
}

fn top_level_blocks(path: &Path) -> usize {
    let doc = document(path);
    docsai_model::addressing::Place::Doc(&doc).children().len()
}

fn slides(path: &Path) -> usize {
    match document(path) {
        docsai_model::Document::Presentation(deck) => deck.slides.len(),
        other => panic!("not a deck: {other:?}"),
    }
}

fn sheets(path: &Path) -> usize {
    match document(path) {
        docsai_model::Document::Workbook(book) => book.sheets.len(),
        other => panic!("not a workbook: {other:?}"),
    }
}

#[test]
fn deleting_the_last_slide_or_sheet_is_refused_by_name() {
    let dir = scratch("last-of-kind");
    let book = copy("xlsx/values-types.xlsx", &dir);
    let sheets: Vec<String> = nodes(&book)
        .into_iter()
        .filter(|(_, k, _)| *k == NodeKind::Sheet)
        .map(|(id, _, _)| id)
        .collect();
    // Delete every sheet but one, then watch the last refuse.
    for sheet in sheets.iter().take(sheets.len().saturating_sub(1)) {
        run(
            &book,
            vec![Operation::Delete {
                target: sheet.clone(),
                if_match: None,
            }],
        );
    }
    let last = first(&book, NodeKind::Sheet);
    match refuse(
        &book,
        vec![Operation::Delete {
            target: last,
            if_match: None,
        }],
    ) {
        Refusal::LastOfKind { kind, .. } => assert_eq!(kind, "visible sheet"),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------- §6.5

#[test]
fn move_reorders_and_keeps_every_id() {
    let dir = scratch("move");
    for path in [
        copy("pptx/slide-order.pptx", &dir),
        as_docmark("pptx/slide-order.pptx", &dir),
    ] {
        let slides: Vec<String> = nodes(&path)
            .into_iter()
            .filter(|(_, k, _)| *k == NodeKind::Slide)
            .map(|(id, _, _)| id)
            .collect();
        assert!(slides.len() >= 3, "{}", path.display());
        let before = slide_titles(&path);
        run(
            &path,
            vec![Operation::Move {
                target: slides[0].clone(),
                after: Some(slides[2].clone()),
                before: None,
                if_match: None,
            }],
        );
        let after = slide_titles(&path);
        assert_ne!(before, after, "{}: nothing moved", path.display());
        let mut a = before.clone();
        a.sort();
        let mut b = after.clone();
        b.sort();
        assert_eq!(a, b, "{}: a move must not lose a slide", path.display());
        assert_eq!(
            after,
            vec![before[1].clone(), before[2].clone(), before[0].clone()],
            "{}",
            path.display()
        );
    }

    // text: two top-level blocks swap places, by index path.
    let path = copy("docx/long-report.docx", &dir);
    let second = text_at(&path, 2);
    run(
        &path,
        vec![Operation::Move {
            target: "doc/1".into(),
            after: Some("doc/3".into()),
            before: None,
            if_match: None,
        }],
    );
    assert_eq!(
        text_at(&path, 1),
        second,
        "the first block did not move out of the way"
    );

    // workbook: two sheets swap places. The corpus has no two-sheet workbook,
    // so one is made — an `if sheets.len() >= 2` here would simply never run.
    let book = two_sheets("xlsx/values-types.xlsx", &dir);
    let sheets: Vec<String> = nodes(&book)
        .into_iter()
        .filter(|(_, k, _)| *k == NodeKind::Sheet)
        .map(|(id, _, _)| id)
        .collect();
    assert_eq!(sheets.len(), 2, "the fixture has two sheets");
    let before = sheet_names(&book);
    run(
        &book,
        vec![Operation::Move {
            target: sheets[0].clone(),
            after: Some(sheets[1].clone()),
            before: None,
            if_match: None,
        }],
    );
    let after = sheet_names(&book);
    assert_eq!(
        after,
        vec![before[1].clone(), before[0].clone()],
        "the sheets did not swap"
    );
}

fn sheet_names(path: &Path) -> Vec<String> {
    match document(path) {
        docsai_model::Document::Workbook(book) => {
            book.sheets.iter().map(|s| s.name.clone()).collect()
        }
        other => panic!("not a workbook: {other:?}"),
    }
}

#[test]
fn move_into_its_own_subtree_is_refused() {
    let dir = scratch("move-cycle");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let slide = first(&deck, NodeKind::Slide);
    // A shape *inside* the slide, reached by the index path §5.1 provides.
    match refuse(
        &deck,
        vec![Operation::Move {
            target: slide.clone(),
            after: Some(format!("{slide}/1")),
            before: None,
            if_match: None,
        }],
    ) {
        Refusal::CycleRefused { .. } => {}
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------- §6.6

#[test]
fn duplicate_slide_copies_the_slide_and_gives_everything_fresh_ids() {
    let dir = scratch("duplicate");
    for path in [
        copy("pptx/basic-slides.pptx", &dir),
        as_docmark("pptx/basic-slides.pptx", &dir),
    ] {
        let slide = first(&path, NodeKind::Slide);
        let before: Vec<String> = nodes(&path).into_iter().map(|(id, _, _)| id).collect();
        let report = run(
            &path,
            vec![Operation::DuplicateSlide {
                target: slide,
                if_match: None,
            }],
        );
        assert!(
            report.created.iter().any(|n| n.kind == NodeKind::Slide),
            "{}: {report:?}",
            path.display()
        );
        for created in &report.created {
            assert!(
                !before.contains(&created.id),
                "{}: `{}` is not a fresh id",
                path.display(),
                created.id
            );
        }
    }
}

#[test]
fn duplicate_slide_refuses_a_text_document_by_class() {
    let dir = scratch("duplicate-class");
    let path = copy("docx/basic-text.docx", &dir);
    let target = paragraph(&path);
    match refuse(
        &path,
        vec![Operation::DuplicateSlide {
            target,
            if_match: None,
        }],
    ) {
        Refusal::NotApplicableToClass { applies_to, .. } => assert_eq!(applies_to, "presentations"),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------- §6.7

#[test]
fn set_cell_applies_to_both_workbook_packages_and_to_docmark() {
    let dir = scratch("set-cell");
    for path in [
        copy("xlsx/values-types.xlsx", &dir),
        copy("ods/values-types.ods", &dir),
        as_docmark("xlsx/values-types.xlsx", &dir),
    ] {
        let sheet = first(&path, NodeKind::Sheet);
        let report = run(
            &path,
            vec![Operation::SetCell {
                target: sheet.clone(),
                cell: "H7".into(),
                value: Some(Some(Scalar::Int(1200))),
                if_match: None,
            }],
        );
        assert_eq!(report.changed.len(), 1, "{}", path.display());
        assert!(
            text(&path).contains("1200"),
            "{}: the value is not in the sheet",
            path.display()
        );
    }
}

#[test]
fn set_cell_refuses_a_covered_cell_and_a_wrong_kind() {
    let dir = scratch("set-cell-refusals");
    let merged = copy("xlsx/merged-cells.xlsx", &dir);
    let sheet = first(&merged, NodeKind::Sheet);
    let mut store = docsai_model::MemoryAssetStore::new();
    let (doc, _, _) = docsai_convert::read_path(&merged, &mut store).expect("reads");
    let covered = match &doc {
        docsai_model::Document::Workbook(book) => book
            .sheets
            .iter()
            .find_map(|s| s.merges.first().map(|m| (m.start, m.end)))
            .map(|(start, end)| {
                docsai_model::sheet::CellRef::new(end.col.max(start.col), end.row.max(start.row))
                    .a1()
            }),
        _ => None,
    };
    // Both guards were `if`s and both could pass by doing nothing: a corpus
    // without a merge, or a merge whose covered corner happened to be A1, made
    // this assertion disappear.
    let covered = covered.expect("merged-cells.xlsx carries a merge");
    assert_ne!(
        covered,
        docsai_model::sheet::CellRef::new(1, 1).a1(),
        "the covered corner of the merge must not be its anchor"
    );
    match refuse(
        &merged,
        vec![Operation::SetCell {
            target: sheet.clone(),
            cell: covered,
            value: Some(Some(Scalar::Int(1))),
            if_match: None,
        }],
    ) {
        Refusal::CoveredCell { .. } => {}
        other => panic!("expected CoveredCell, got {other:?}"),
    }

    let text_doc = copy("docx/basic-text.docx", &dir);
    let para = paragraph(&text_doc);
    match refuse(
        &text_doc,
        vec![Operation::SetCell {
            target: para,
            cell: "A1".into(),
            value: Some(Some(Scalar::Int(1))),
            if_match: None,
        }],
    ) {
        Refusal::NotApplicableToClass { applies_to, .. } => assert_eq!(applies_to, "workbooks"),
        other => panic!("{other:?}"),
    }
}

// ------------------------------------------------------------------- §6.8

#[test]
fn set_notes_replaces_the_notes_page_and_removes_it_on_null() {
    let dir = scratch("set-notes");
    for path in [
        copy("pptx/notes-speaker.pptx", &dir),
        as_docmark("pptx/notes-speaker.pptx", &dir),
    ] {
        let slide = first(&path, NodeKind::Slide);
        run(
            &path,
            vec![Operation::SetNotes {
                target: slide.clone(),
                content: Some(Some("Preguntar por el Q4.".into())),
                if_match: None,
            }],
        );
        assert!(
            text(&path).contains("Preguntar por el Q4."),
            "{}",
            path.display()
        );
        let report = run(
            &path,
            vec![Operation::SetNotes {
                target: slide,
                content: Some(None),
                if_match: None,
            }],
        );
        // §6.8 removes the notes page. Where the writer cannot take a notes
        // part out of a preserved package it says so by name — Phase 16's
        // refusal, carried through — and that is the one acceptable outcome
        // besides removal. What is not acceptable is silence.
        let gone = !text(&path).contains("Preguntar por el Q4.");
        assert!(
            gone || report
                .warnings
                .iter()
                .any(|w| format!("{w:?}").contains("notes")),
            "{}: the notes are still there and nothing was said",
            path.display()
        );
    }
}

// ------------------------------------------------------------------- §6.9

#[test]
fn replace_image_swaps_the_bytes_and_keeps_the_geometry() {
    // A 1x1 GIF, the smallest thing that is a picture.
    const GIF: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00,
        0x00, 0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ];
    let dir = scratch("replace-image");
    let picture = dir.join("nueva.gif");
    std::fs::write(&picture, GIF).expect("writes the picture");

    for rel in [
        "docx/images-inline.docx",
        "odt/images-inline.odt",
        "xlsx/images-anchored.xlsx",
        "pptx/images-anchored.pptx",
    ] {
        let path = copy(rel, &dir);
        // An inline picture carries an id of its own; a pptx picture is a
        // *shape* and is addressed as one (§6.9's kinds row).
        let target = nodes(&path)
            .into_iter()
            .find(|(_, k, _)| *k == NodeKind::Image)
            .map(|(id, _, _)| id)
            .or_else(|| {
                find_target(&path, &|place| {
                    use docsai_model::addressing::Place;
                    matches!(place, Place::Image(_))
                        || matches!(place, Place::Block(docsai_model::text::Block::Image(_)))
                        || matches!(place, Place::Shape(shape)
                    if matches!(shape.kind, docsai_model::presentation::ShapeKind::Picture(_)))
                })
            })
            .unwrap_or_else(|| panic!("{rel} has no picture"));
        let geometry_before = placements(&path);
        let report = run(
            &path,
            vec![Operation::ReplaceImage {
                target: target.clone(),
                path: Some(picture.display().to_string()),
                content_base64: None,
                filename: None,
                alt: Some("El logotipo nuevo".into()),
                if_match: None,
            }],
        );
        assert_eq!(report.changed.len(), 1, "{rel}");
        // Every picture's *placement* is untouched — §6.9 keeps «the placement a
        // person chose». Compared over the whole document rather than through
        // the target's id, because a package stores no ids and a re-read
        // renumbers from document order.
        assert_eq!(
            placements(&path),
            geometry_before,
            "{rel}: the geometry a person chose was not kept"
        );
        assert!(
            text(&path).contains("El logotipo nuevo"),
            "{rel}: the new alt text is not there"
        );
    }
}

fn placement(image: &docsai_model::image::ImageRef) -> String {
    let g = &image.geometry;
    // A crop of zero on every side is no crop, and the ODF writer normalises
    // one away on any write-back — an edit or a plain conversion alike. Two
    // spellings of «not cropped» are not a change to the placement.
    let crop = g
        .crop
        .filter(|c| c.left != 0.0 || c.top != 0.0 || c.right != 0.0 || c.bottom != 0.0);
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        g.display_size, g.anchor, g.rotation_deg, g.flip, crop, g.border
    )
}

/// Every picture's placement, in document order.
fn placements(path: &Path) -> Vec<String> {
    fn walk(place: docsai_model::addressing::Place<'_>, out: &mut Vec<String>) {
        match &place {
            docsai_model::addressing::Place::Image(image) => out.push(placement(image)),
            docsai_model::addressing::Place::Block(docsai_model::text::Block::Image(image)) => {
                out.push(placement(image))
            }
            docsai_model::addressing::Place::Shape(shape) => {
                if let docsai_model::presentation::ShapeKind::Picture(image) = &shape.kind {
                    out.push(placement(image));
                }
            }
            _ => {}
        }
        for child in place.children() {
            walk(child, out);
        }
    }
    let doc = document(path);
    let mut out = Vec::new();
    walk(docsai_model::addressing::Place::Doc(&doc), &mut out);
    // `children()` does not walk into inline content, so the inline pictures
    // are collected from the model directly.
    if let docsai_model::Document::Text(text) = &doc {
        for section in &text.sections {
            inline_images(&section.blocks, &mut out);
        }
    }
    out.sort();
    out
}

fn inline_images(blocks: &[docsai_model::text::Block], out: &mut Vec<String>) {
    use docsai_model::text::{Block, Inline};
    for block in blocks {
        match block {
            Block::Paragraph(p) => {
                for inline in &p.content {
                    if let Inline::Image(image) = inline {
                        out.push(placement(image));
                    }
                }
            }
            Block::List(list) => {
                for item in &list.items {
                    inline_images(&item.blocks, out);
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        inline_images(&cell.blocks, out);
                    }
                }
            }
            _ => {}
        }
    }
}

#[allow(dead_code)]
fn geometry(path: &Path, id: &str) -> String {
    let mut store = docsai_model::MemoryAssetStore::new();
    let (mut doc, _, _) = docsai_convert::read_path(path, &mut store).expect("reads");
    docsai_model::addressing::assign_ids(&mut doc, docsai_model::addressing::IdPolicy::Assign);
    match docsai_model::addressing::resolve_str(&doc, id) {
        // The *placement*, which §6.9 keeps. `native_size_px` is the new
        // picture's own size and is expected to change.
        Ok(docsai_model::addressing::Place::Image(image)) => placement(image),
        Ok(docsai_model::addressing::Place::Block(docsai_model::text::Block::Image(image))) => {
            placement(image)
        }
        other => panic!("{id} is not an image: {other:?}"),
    }
}

fn slide_titles(path: &Path) -> Vec<String> {
    match document(path) {
        docsai_model::Document::Presentation(deck) => deck
            .slides
            .iter()
            .map(|s| s.title().unwrap_or_default())
            .collect(),
        other => panic!("not a deck: {other:?}"),
    }
}

/// The plain text of the *n*-th top-level block, 1-based.
fn text_at(path: &Path, index: usize) -> String {
    let doc = document(path);
    let place = docsai_model::addressing::resolve_str(&doc, &format!("doc/{index}"))
        .unwrap_or_else(|e| panic!("{}: doc/{index}: {e}", path.display()));
    match place {
        docsai_model::addressing::Place::Block(block) => match block {
            docsai_model::text::Block::Paragraph(p) => p.plain_text(),
            docsai_model::text::Block::Heading(h) => h.paragraph.plain_text(),
            other => format!("{other:?}"),
        },
        other => format!("{other:?}"),
    }
}

// ------------------------------------------------------------------- §5.3

/// «Ids are stable, positions are not» (§5.3), demonstrated in one batch.
#[test]
fn an_id_target_is_immune_to_a_preceding_delete_and_an_index_path_is_not() {
    let dir = scratch("ordering");

    // An id target: a slide keeps its identity through a deletion above it.
    let deck = copy("pptx/slide-order.pptx", &dir);
    let slides: Vec<String> = nodes(&deck)
        .into_iter()
        .filter(|(_, k, _)| *k == NodeKind::Slide)
        .map(|(id, _, _)| id)
        .collect();
    assert!(slides.len() >= 3);
    let survivor = slides[2].clone();
    let report = run(
        &deck,
        vec![
            Operation::Delete {
                target: slides[0].clone(),
                if_match: None,
            },
            replace_text(
                &format!("{survivor}/1"),
                "Sigue siendo la misma diapositiva",
            ),
        ],
    );
    assert_eq!(report.changed[0].id, format!("{survivor}/1"));
    assert!(text(&deck).contains("Sigue siendo la misma diapositiva"));

    // An index path is not immune, and this is the cost a caller pays for
    // batching a structural change before a positional target: after the
    // deletion, `doc/1` names what `doc/2` named when the request was written.
    let doc = copy("docx/long-report.docx", &dir);
    let was_second = text_at(&doc, 2);
    run(
        &doc,
        vec![
            Operation::Delete {
                target: "doc/1".into(),
                if_match: None,
            },
            replace_text("doc/1", "Escrito sobre lo que era doc/2"),
        ],
    );
    assert_ne!(was_second, "Escrito sobre lo que era doc/2");
    assert_eq!(
        text_at(&doc, 1),
        "Escrito sobre lo que era doc/2",
        "the second operation addressed a position, not a node"
    );
}

// ------------------------------------------------------------------- §5.4.1

/// «Ids are never renumbered» has an edge, and it is measured here rather than
/// discovered by a caller: **an id is stable for the lifetime of a reading.**
///
/// No Office format stores a DocMark node id — the project decided that
/// deliberately, and a package is not annotated with bookmarks to carry one —
/// so a reader assigns ids in document order every time it opens one. An
/// order-changing operation therefore makes the next read of the *package* hand
/// the same token to a different node. A `.dmk.md` writes its ids down and does
/// not have the property.
///
/// §5.4.1 of `docs/edit-operations.md` states this; this test is what makes the
/// statement true rather than hopeful.
#[test]
fn a_package_reassigns_ids_on_the_next_read_and_docmark_does_not() {
    let dir = scratch("ids-on-reread");

    let deck = copy("pptx/slide-order.pptx", &dir);
    let before: Vec<(String, String)> = slide_ids_and_titles(&deck);
    assert!(before.len() >= 3);
    run(
        &deck,
        vec![Operation::Move {
            target: before[0].0.clone(),
            after: Some(before[2].0.clone()),
            before: None,
            if_match: None,
        }],
    );
    let after = slide_ids_and_titles(&deck);
    assert_eq!(
        after.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        before.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        "the reader hands out the same ids in the same order, whatever moved"
    );
    assert_ne!(
        after[0].1, before[0].1,
        "`{}` names the same slide after a move; the property under test is gone",
        before[0].0
    );

    // DocMark carries its ids in the file, so the same batch keeps them.
    let markdown = as_docmark("pptx/slide-order.pptx", &dir);
    let before = slide_ids_and_titles(&markdown);
    run(
        &markdown,
        vec![Operation::Move {
            target: before[0].0.clone(),
            after: Some(before[2].0.clone()),
            before: None,
            if_match: None,
        }],
    );
    let after = slide_ids_and_titles(&markdown);
    let mut moved: Vec<&(String, String)> = after.iter().collect();
    moved.sort();
    let mut original: Vec<&(String, String)> = before.iter().collect();
    original.sort();
    assert_eq!(
        moved, original,
        "a DocMark file's ids must follow their nodes across a move"
    );
    assert_ne!(
        after.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        before.iter().map(|(id, _)| id).collect::<Vec<_>>(),
        "if the ids are still in the same order, nothing moved"
    );
}

fn slide_ids_and_titles(path: &Path) -> Vec<(String, String)> {
    match document(path) {
        docsai_model::Document::Presentation(deck) => deck
            .slides
            .iter()
            .map(|s| {
                (
                    s.id.as_ref().map(|i| i.0.clone()).unwrap_or_default(),
                    s.title().unwrap_or_default(),
                )
            })
            .collect(),
        other => panic!("not a deck: {other:?}"),
    }
}
