//! `docs/edit-operations.md` §11's test matrix, walked cell by cell.
//!
//! Phase 17's first acceptance criterion is «every operation has a round-trip
//! test on all four document classes it applies to», and §11 turns that into a
//! ten-by-four table whose `—` cells are themselves tests: «a `—` is a
//! `NotApplicableToClass` or a `WrongKind`, and its refusal is itself the
//! test».
//!
//! `edit_ops.rs` proves what each operation *means*. This file proves the
//! table is **covered**: the matrix is a constant here, every cell of it is
//! visited by construction, and a combination this file does not know how to
//! exercise is a panic rather than a silent skip. §2 is obeyed too — **text**
//! is satisfied only by both `.docx` and `.odt`, **workbook** only by both
//! `.xlsx` and `.ods`, and **docmark** by a `.dmk.md` of every IR root the
//! operation applies to.

use std::path::{Path, PathBuf};

use docsai_convert::edit::{edit_path, EditOptions};
use docsai_convert::{ConvertOptions, EditRequest, Operation, Refusal, Scalar};
use docsai_model::addressing::NodeKind;

// ---------------------------------------------------------------- the matrix

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cell {
    /// `✔`: the operation applies, and a round trip proves it.
    Applies,
    /// `—`: the operation refuses, by name.
    Refuses,
}
use Cell::{Applies, Refuses};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Text,
    Workbook,
    Presentation,
    Docmark,
}

/// A package family, which is what §2 says a class is only covered by *both* of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Docx,
    Odt,
    Xlsx,
    Ods,
    Pptx,
}

/// §11, verbatim: `text`, `workbook`, `presentation`, `docmark`.
const MATRIX: &[(&str, [Cell; 4])] = &[
    ("replace_text", [Applies, Refuses, Applies, Applies]),
    ("set_props", [Applies, Refuses, Applies, Applies]),
    ("insert_after", [Applies, Applies, Applies, Applies]),
    ("insert_before", [Applies, Applies, Applies, Applies]),
    ("delete", [Applies, Applies, Applies, Applies]),
    ("move", [Applies, Applies, Applies, Applies]),
    ("duplicate_slide", [Refuses, Refuses, Applies, Applies]),
    ("set_cell", [Refuses, Applies, Refuses, Applies]),
    ("set_notes", [Refuses, Refuses, Applies, Applies]),
    ("replace_image", [Applies, Applies, Applies, Applies]),
];

const CLASSES: [Class; 4] = [
    Class::Text,
    Class::Workbook,
    Class::Presentation,
    Class::Docmark,
];

/// The packages a class is only covered by all of (§2).
fn families(class: Class) -> &'static [Family] {
    match class {
        Class::Text => &[Family::Docx, Family::Odt],
        Class::Workbook => &[Family::Xlsx, Family::Ods],
        Class::Presentation => &[Family::Pptx],
        // Filled in per operation: a `.dmk.md` of every root the operation
        // applies to, so `set_cell`'s DocMark case is a workbook's and
        // `set_notes`'s is a deck's.
        Class::Docmark => &[],
    }
}

/// The DocMark cases for one operation: one `.dmk.md` per **root** the
/// operation is `✔` on, because a DocMark file has a root like any other
/// document and an operation that refuses workbooks refuses a workbook's
/// DocMark too.
fn docmark_families(row: &[Cell; 4]) -> Vec<Family> {
    let mut out = Vec::new();
    if row[0] == Applies {
        out.push(Family::Docx);
    }
    if row[1] == Applies {
        out.push(Family::Xlsx);
    }
    if row[2] == Applies {
        out.push(Family::Pptx);
    }
    out
}

// ---------------------------------------------------------------- the fixtures

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-matrix-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

/// The corpus document this cell is exercised on: the smallest one that has
/// what the operation needs.
fn source(op: &str, family: Family) -> &'static str {
    match (op, family) {
        ("replace_image", Family::Docx) => "docx/images-inline.docx",
        ("replace_image", Family::Odt) => "odt/images-inline.odt",
        ("replace_image", Family::Xlsx) => "xlsx/images-anchored.xlsx",
        ("replace_image", Family::Ods) => "ods/images-anchored.ods",
        ("replace_image", Family::Pptx) => "pptx/images-anchored.pptx",
        ("set_notes", Family::Pptx) => "pptx/notes-speaker.pptx",
        // `move` and `delete` need something to reorder or something to spare.
        ("move" | "delete" | "duplicate_slide", Family::Pptx) => "pptx/slide-order.pptx",
        (_, Family::Docx) => "docx/long-report.docx",
        (_, Family::Odt) => "odt/basic-text.odt",
        (_, Family::Xlsx) => "xlsx/values-types.xlsx",
        (_, Family::Ods) => "ods/values-types.ods",
        (_, Family::Pptx) => "pptx/basic-slides.pptx",
    }
}

/// The file the cell is run against: the package itself, or its DocMark.
fn fixture(op: &str, family: Family, class: Class, dir: &Path) -> PathBuf {
    let rel = source(op, family);
    let from = corpus().join(rel);
    let path = if class == Class::Docmark {
        let stem = from.file_stem().expect("a stem").to_string_lossy();
        let target = dir.join(format!("{stem}.dmk.md"));
        docsai_convert::convert_file(&from, Some(&target), &ConvertOptions::default())
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        target
    } else {
        let target = dir.join(from.file_name().expect("a name"));
        std::fs::copy(&from, &target).unwrap_or_else(|e| panic!("{rel}: {e}"));
        target
    };
    // Some cells need a document the corpus does not have: a workbook with a
    // spare sheet, because §6.4 refuses to delete the last one and a move needs
    // somewhere to move to.
    if matches!(op, "move" | "delete") && matches!(family, Family::Xlsx | Family::Ods) {
        let sheet = first(&path, NodeKind::Sheet);
        apply(
            &path,
            vec![Operation::InsertAfter {
                target: sheet,
                content: "# Hoja de paso {.sheet}\n\n| A |\n|---|\n| uno |\n".into(),
                if_match: None,
            }],
        );
    }
    path
}

// ---------------------------------------------------------------- addressing

fn document(path: &Path) -> docsai_model::Document {
    let mut store = docsai_model::MemoryAssetStore::new();
    let (mut doc, _, _) = docsai_convert::read_path(path, &mut store)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    docsai_model::addressing::assign_ids(&mut doc, docsai_model::addressing::IdPolicy::Assign);
    doc
}

fn ids(path: &Path, kind: NodeKind) -> Vec<String> {
    docsai_model::addressing::ids_in_document_order(&document(path))
        .into_iter()
        .filter(|(_, k)| *k == kind)
        .map(|(id, _)| id.0)
        .collect()
}

fn first(path: &Path, kind: NodeKind) -> String {
    ids(path, kind)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("{} has no {kind}", path.display()))
}

/// The first target naming a place `pred` accepts, in document order,
/// preferring an id and falling back to the index path §5.1 provides — which
/// ordinary prose needs, because Phase 10 gives a plain paragraph no id.
fn find(
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
            *out = Some(name);
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

/// Something `replace_text` and `set_props` take: a paragraph, a heading, or a
/// placeholder's paragraph.
fn writable_text(path: &Path) -> String {
    use docsai_model::addressing::Place;
    find(path, &|place| {
        matches!(
            place,
            Place::Block(docsai_model::text::Block::Paragraph(_))
                | Place::Block(docsai_model::text::Block::Heading(_))
        )
    })
    .unwrap_or_else(|| panic!("{} has no paragraph", path.display()))
}

/// A picture, wherever the family keeps one: an inline image, a block image or
/// a shape whose kind is a picture (§6.9's kinds row).
fn picture(path: &Path) -> String {
    use docsai_model::addressing::Place;
    ids(path, NodeKind::Image)
        .into_iter()
        .next()
        .or_else(|| {
            find(path, &|place| {
                matches!(place, Place::Image(_))
                    || matches!(place, Place::Block(docsai_model::text::Block::Image(_)))
                    || matches!(place, Place::Shape(shape)
                        if matches!(shape.kind, docsai_model::presentation::ShapeKind::Picture(_)))
            })
        })
        .unwrap_or_else(|| panic!("{} has no picture", path.display()))
}

/// The document's own DocMark, which is what a round-trip assertion reads.
fn view(path: &Path) -> String {
    let dir = path.parent().expect("a directory").join("view");
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join("view.dmk.md");
    docsai_convert::convert_file(path, Some(&out), &ConvertOptions::default())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    std::fs::read_to_string(&out).expect("reads the view")
}

fn apply(path: &Path, operations: Vec<Operation>) -> docsai_convert::EditReport {
    edit_path(path, &EditRequest::new(operations), &EditOptions::default())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .report
}

fn refusal(path: &Path, operations: Vec<Operation>) -> Refusal {
    match edit_path(path, &EditRequest::new(operations), &EditOptions::default()) {
        Err(docsai_convert::EditError::Refused(refused)) => refused.refusal,
        other => panic!("{}: expected a refusal, got {other:?}", path.display()),
    }
}

/// A 1×1 GIF: the smallest thing that is a picture.
const GIF: &[u8] = &[
    0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00,
    0xff, 0xff, 0xff, 0x21, 0xf9, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
];

// ---------------------------------------------------------------- the cells

/// What one operation is on the document at hand, and what proves it landed.
struct Cellwork {
    operations: Vec<Operation>,
    /// A string the document's own DocMark must carry afterwards.
    expect: Option<String>,
}

/// Everything an `Applies` cell needs, built against the live document.
fn work(op: &str, path: &Path) -> Cellwork {
    match op {
        "replace_text" => {
            let target = writable_text(path);
            Cellwork {
                operations: vec![Operation::ReplaceText {
                    target,
                    text: "Resultados del Q3".into(),
                    if_match: None,
                }],
                expect: Some("Resultados del Q3".into()),
            }
        }
        "set_props" => {
            let target = writable_text(path);
            let props = [("bold".to_string(), Some(Scalar::Bool(true)))]
                .into_iter()
                .collect();
            // The round trip is the point: a `set_props` that reports a change
            // and leaves the document reading exactly as it did is a silent
            // no-op, and §6.2 says the paragraph must read uniformly.
            let text = plain_text(path, &target);
            Cellwork {
                operations: vec![Operation::SetProps {
                    target,
                    props,
                    if_match: None,
                }],
                expect: Some(format!("**{text}**")),
            }
        }
        "insert_after" | "insert_before" => {
            let (target, content, expect) = insertion(path);
            let operations = vec![if op == "insert_after" {
                Operation::InsertAfter {
                    target,
                    content,
                    if_match: None,
                }
            } else {
                Operation::InsertBefore {
                    target,
                    content,
                    if_match: None,
                }
            }];
            Cellwork {
                operations,
                expect: Some(expect),
            }
        }
        "delete" => {
            let target = sequence_element(path);
            Cellwork {
                operations: vec![Operation::Delete {
                    target,
                    if_match: None,
                }],
                expect: None,
            }
        }
        "move" => {
            let (target, after) = match root(path) {
                Root::Text => ("doc/1".to_string(), "doc/3".to_string()),
                _ => {
                    let siblings = siblings_of(path);
                    assert!(
                        siblings.len() >= 2,
                        "{}: a move needs somewhere to move to",
                        path.display()
                    );
                    (siblings[0].clone(), siblings[1].clone())
                }
            };
            Cellwork {
                operations: vec![Operation::Move {
                    target,
                    after: Some(after),
                    before: None,
                    if_match: None,
                }],
                expect: None,
            }
        }
        "duplicate_slide" => {
            let target = first(path, NodeKind::Slide);
            Cellwork {
                operations: vec![Operation::DuplicateSlide {
                    target,
                    if_match: None,
                }],
                expect: None,
            }
        }
        "set_cell" => {
            let target = first(path, NodeKind::Sheet);
            Cellwork {
                operations: vec![Operation::SetCell {
                    target,
                    cell: "H7".into(),
                    value: Some(Some(Scalar::Int(1200))),
                    if_match: None,
                }],
                expect: Some("1200".into()),
            }
        }
        "set_notes" => {
            let target = first(path, NodeKind::Slide);
            Cellwork {
                operations: vec![Operation::SetNotes {
                    target,
                    content: Some(Some("Preguntar por el Q4.".into())),
                    if_match: None,
                }],
                expect: Some("Preguntar por el Q4.".into()),
            }
        }
        "replace_image" => {
            let target = picture(path);
            let gif = path.parent().expect("a directory").join("nueva.gif");
            std::fs::write(&gif, GIF).expect("writes the picture");
            Cellwork {
                operations: vec![Operation::ReplaceImage {
                    target,
                    path: Some(gif.display().to_string()),
                    content_base64: None,
                    filename: None,
                    alt: Some("El logotipo nuevo".into()),
                    if_match: None,
                }],
                expect: Some("El logotipo nuevo".into()),
            }
        }
        other => panic!("the matrix names `{other}` and this file cannot exercise it"),
    }
}

/// The operation a `Refuses` cell is refused for: the same operation, aimed at
/// the most plausible target the class has, because a refusal earned by a
/// nonsense target would prove nothing.
fn refused_work(op: &str, path: &Path) -> Vec<Operation> {
    match op {
        "replace_text" => vec![Operation::ReplaceText {
            target: first(path, NodeKind::Sheet),
            text: "no".into(),
            if_match: None,
        }],
        "set_props" => vec![Operation::SetProps {
            target: first(path, NodeKind::Sheet),
            props: [("bold".to_string(), Some(Scalar::Bool(true)))]
                .into_iter()
                .collect(),
            if_match: None,
        }],
        "duplicate_slide" => vec![Operation::DuplicateSlide {
            target: match root(path) {
                Root::Workbook => first(path, NodeKind::Sheet),
                _ => writable_text(path),
            },
            if_match: None,
        }],
        "set_cell" => vec![Operation::SetCell {
            target: match root(path) {
                Root::Presentation => first(path, NodeKind::Slide),
                _ => writable_text(path),
            },
            cell: "A1".into(),
            value: Some(Some(Scalar::Int(1))),
            if_match: None,
        }],
        "set_notes" => vec![Operation::SetNotes {
            target: match root(path) {
                Root::Workbook => first(path, NodeKind::Sheet),
                _ => writable_text(path),
            },
            content: Some(Some("Nada".into())),
            if_match: None,
        }],
        other => panic!("the matrix has no `—` for `{other}`"),
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Root {
    Text,
    Workbook,
    Presentation,
}

fn root(path: &Path) -> Root {
    match document(path) {
        docsai_model::Document::Text(_) => Root::Text,
        docsai_model::Document::Workbook(_) => Root::Workbook,
        docsai_model::Document::Presentation(_) => Root::Presentation,
    }
}

/// A DocMark fragment of the kind the target's sequence takes (§6.3), with the
/// target to splice it beside and the text that proves it landed.
fn insertion(path: &Path) -> (String, String, String) {
    match root(path) {
        Root::Text => (
            "doc/1".into(),
            "| Trimestre | Ingresos |\n|---|---|\n| Q3 | 1200 |\n".into(),
            // A table is addressable, so `created` names it and DocMark writes
            // it back as one.
            "Trimestre".into(),
        ),
        Root::Presentation => (
            first(path, NodeKind::Slide),
            "## Diapositiva nueva {.slide}\n\nUn párrafo.\n".into(),
            "Diapositiva nueva".into(),
        ),
        Root::Workbook => (
            first(path, NodeKind::Sheet),
            "# Hoja nueva {.sheet}\n\n| A1 |\n|---|\n| uno |\n".into(),
            "Hoja nueva".into(),
        ),
    }
}

/// An element of a sequence: what `delete` and `move` address (§6.3's kinds).
fn sequence_element(path: &Path) -> String {
    match root(path) {
        Root::Text => "doc/1".into(),
        Root::Presentation => first(path, NodeKind::Slide),
        Root::Workbook => first(path, NodeKind::Sheet),
    }
}

fn siblings_of(path: &Path) -> Vec<String> {
    match root(path) {
        Root::Text => vec!["doc/1".into(), "doc/3".into()],
        Root::Presentation => ids(path, NodeKind::Slide),
        Root::Workbook => ids(path, NodeKind::Sheet),
    }
}

/// The plain text of a target, so a `set_props` assertion can name what the
/// bold marks must wrap.
fn plain_text(path: &Path, target: &str) -> String {
    use docsai_model::addressing::Place;
    let doc = document(path);
    match docsai_model::addressing::resolve_str(&doc, target) {
        Ok(Place::Block(docsai_model::text::Block::Paragraph(p))) => p.plain_text(),
        Ok(Place::Block(docsai_model::text::Block::Heading(h))) => h.paragraph.plain_text(),
        other => panic!("{target} is not a paragraph: {other:?}"),
    }
}

/// How many of the thing the operation removes, before and after.
fn population(path: &Path) -> usize {
    let doc = document(path);
    match &doc {
        docsai_model::Document::Text(_) => {
            docsai_model::addressing::Place::Doc(&doc).children().len()
        }
        docsai_model::Document::Workbook(book) => book.sheets.len(),
        docsai_model::Document::Presentation(deck) => deck.slides.len(),
    }
}

// ---------------------------------------------------------------- the test

#[test]
fn every_cell_of_the_section_11_matrix_is_exercised() {
    let mut visited = 0usize;
    for (op, row) in MATRIX {
        for (index, class) in CLASSES.into_iter().enumerate() {
            let cell = row[index];
            let families: Vec<Family> = if class == Class::Docmark {
                docmark_families(row)
            } else {
                families(class).to_vec()
            };
            assert!(
                !families.is_empty(),
                "{op}/{class:?}: the matrix names a class with no package to prove it on"
            );
            for family in families {
                let label = format!("{op}/{class:?}/{family:?}");
                let dir = scratch(&label.replace('/', "-").to_lowercase());
                let path = fixture(op, family, class, &dir);
                match cell {
                    Applies => {
                        let before = population(&path);
                        let plan = work(op, &path);
                        let report = apply(&path, plan.operations);
                        assert!(
                            !report.is_empty(),
                            "{label}: the operation reported nothing at all"
                        );
                        match &plan.expect {
                            Some(needle) => assert!(
                                view(&path).contains(needle.as_str()),
                                "{label}: `{needle}` did not survive the round trip"
                            ),
                            // `delete`, `move` and `duplicate_slide` are proved
                            // by the shape of the document, not by a string.
                            None => match *op {
                                "delete" => assert_eq!(
                                    population(&path),
                                    before - 1,
                                    "{label}: nothing left the document"
                                ),
                                "duplicate_slide" => assert_eq!(
                                    population(&path),
                                    before + 1,
                                    "{label}: nothing was added"
                                ),
                                "move" => assert_eq!(
                                    population(&path),
                                    before,
                                    "{label}: a move lost or gained a node"
                                ),
                                other => panic!("{other} needs an assertion"),
                            },
                        }
                    }
                    Refuses => {
                        let before = std::fs::read(&path).expect("reads the document");
                        let refusal = refusal(&path, refused_work(op, &path));
                        assert!(
                            matches!(
                                refusal,
                                Refusal::NotApplicableToClass { .. } | Refusal::WrongKind { .. }
                            ),
                            "{label}: §11 says a `—` is a NotApplicableToClass or a WrongKind, \
                             not {refusal:?}"
                        );
                        assert_eq!(
                            std::fs::read(&path).expect("reads the document"),
                            before,
                            "{label}: a refused operation changed the file"
                        );
                    }
                }
                let _ = std::fs::remove_dir_all(&dir);
                visited += 1;
            }
        }
    }
    // Ten operations over four classes, with §2's «both packages» for text and
    // workbook and one DocMark case per root an operation applies to: 72 cells.
    // The number is asserted so that a row quietly losing a package shows up.
    assert_eq!(visited, 72, "the matrix walk visited {visited} cells");
}
