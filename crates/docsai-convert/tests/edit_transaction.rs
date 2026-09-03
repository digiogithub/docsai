//! 17-C's transaction and 17-D's report, proved rather than described.
//!
//! Everything here is about the *file*: what is on disk when an edit succeeds,
//! and — the harder half — what is on disk when one does not.

use std::path::{Path, PathBuf};

use docsai_convert::edit::{edit_path, EditError, EditOptions, FaultPoint};
use docsai_convert::{ConvertOptions, EditRequest, Operation};
use docsai_model::addressing::NodeKind;

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-tx-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

fn copy(rel: &str, dir: &Path) -> PathBuf {
    let source = corpus().join(rel);
    let target = dir.join(source.file_name().expect("a file name"));
    std::fs::copy(&source, &target).unwrap_or_else(|e| panic!("{}: {e}", source.display()));
    target
}

/// The content hash of a file, which is how «byte-identical» is asserted.
fn digest(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    docsai_model::assets::content_hash(&bytes)
}

fn document(path: &Path) -> docsai_model::Document {
    let mut store = docsai_model::MemoryAssetStore::new();
    let (mut doc, _, _) = docsai_convert::read_path(path, &mut store)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    docsai_model::addressing::assign_ids(&mut doc, docsai_model::addressing::IdPolicy::Assign);
    doc
}

fn slide_ids(path: &Path) -> Vec<String> {
    docsai_model::addressing::ids_in_document_order(&document(path))
        .into_iter()
        .filter(|(_, kind)| *kind == NodeKind::Slide)
        .map(|(id, _)| id.0)
        .collect()
}

/// Every member of a package, by name, with its bytes.
fn members(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut zip = zip::ZipArchive::new(file).expect("a readable package");
    let mut out = std::collections::BTreeMap::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).expect("a member");
        let name = entry.name().to_string();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).expect("reads the member");
        out.insert(name, bytes);
    }
    out
}

/// Anything the transaction left behind in the destination's directory.
fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .expect("reads the directory")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".docsai-edit-"))
        .collect()
}

fn replace_text(target: &str, text: &str) -> Operation {
    Operation::ReplaceText {
        target: target.into(),
        text: text.into(),
        if_match: None,
    }
}

// ---------------------------------------------------------------- all or nothing

/// §4: «Either every operation applied or none did.»
///
/// Five operations, the third of which cannot apply. The document on disk must
/// come back **byte-identical**, which is asserted by hashing it, and the first
/// two operations must have reached nothing.
#[test]
fn a_batch_whose_third_operation_fails_leaves_the_file_byte_identical() {
    let dir = scratch("batch-failure");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let slides = slide_ids(&deck);
    let first = slides.first().expect("a slide").clone();
    let before = digest(&deck);

    let error = edit_path(
        &deck,
        &EditRequest::new(vec![
            replace_text(&format!("{first}/1"), "Uno"),
            Operation::SetNotes {
                target: first.clone(),
                content: Some(Some("Dos".into())),
                if_match: None,
            },
            // Three: a target that is not in the document. §5.3 resolves a
            // target when its operation runs, so this fails here and not in the
            // validation pass — which is the case that needs a transaction.
            replace_text("n9999", "Tres"),
            replace_text(&format!("{first}/1"), "Cuatro"),
            Operation::DuplicateSlide {
                target: first,
                if_match: None,
            },
        ]),
        &EditOptions::default(),
    );

    match error {
        Err(EditError::Refused(refused)) => {
            assert_eq!(
                refused.index, 2,
                "the third operation is the one that failed"
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        digest(&deck),
        before,
        "a failed batch changed the file on disk"
    );
    assert!(leftovers(&dir).is_empty(), "{:?}", leftovers(&dir));
}

/// The same property for each package family, because each has its own writer.
#[test]
fn a_refused_batch_leaves_every_package_family_byte_identical() {
    let dir = scratch("families");
    for rel in [
        "docx/long-report.docx",
        "odt/basic-text.odt",
        "xlsx/values-types.xlsx",
        "ods/values-types.ods",
        "pptx/basic-slides.pptx",
    ] {
        let path = copy(rel, &dir);
        let before = digest(&path);
        let result = edit_path(
            &path,
            &EditRequest::new(vec![
                replace_text("doc/1", "Algo"),
                replace_text("n9999", "Nada"),
            ]),
            &EditOptions::default(),
        );
        assert!(result.is_err(), "{rel}: the batch should have been refused");
        assert_eq!(digest(&path), before, "{rel}: the original moved");
        assert!(leftovers(&dir).is_empty(), "{rel}: {:?}", leftovers(&dir));
    }
}

// ---------------------------------------------------------------- injected faults

/// «Proven by injecting one, not by argument.»
///
/// A refusal a caller can trigger always happens *before* a byte is written, so
/// it cannot exercise the write half at all. These two do.
#[test]
fn an_injected_failure_after_the_operations_applied_leaves_the_original_untouched() {
    let dir = scratch("fault-apply");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let slides = slide_ids(&deck);
    let before = digest(&deck);

    let error = edit_path(
        &deck,
        &EditRequest::new(vec![replace_text(
            &format!("{}/1", slides[0]),
            "Un título nuevo",
        )]),
        &EditOptions {
            fault: Some(FaultPoint::AfterApply),
            ..EditOptions::default()
        },
    );
    assert!(matches!(
        error,
        Err(EditError::InjectedFault {
            at: FaultPoint::AfterApply,
            ..
        })
    ));
    assert_eq!(digest(&deck), before);
    assert!(leftovers(&dir).is_empty());
}

/// The interesting one: the whole edited document is on disk, complete, under a
/// temporary name, and the process fails before the rename.
#[test]
fn an_injected_failure_before_the_rename_leaves_the_original_untouched() {
    let dir = scratch("fault-rename");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let slides = slide_ids(&deck);
    let before = digest(&deck);

    let error = edit_path(
        &deck,
        &EditRequest::new(vec![replace_text(
            &format!("{}/1", slides[0]),
            "Un título nuevo",
        )]),
        &EditOptions {
            fault: Some(FaultPoint::BeforeRename),
            ..EditOptions::default()
        },
    );
    match error {
        Err(EditError::InjectedFault {
            at: FaultPoint::BeforeRename,
            temporary: Some(temporary),
        }) => {
            // The temporary file lives in the destination's **own** directory,
            // which is what makes the rename a rename and not a copy.
            assert_eq!(
                temporary.parent(),
                deck.parent(),
                "the temporary file is not beside the destination"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        digest(&deck),
        before,
        "a failure before the rename changed the original"
    );
    // The handle owns the temporary file, so its `Drop` unlinked it on the way
    // out of the failure.
    assert!(
        leftovers(&dir).is_empty(),
        "the temporary file survived the failure: {:?}",
        leftovers(&dir)
    );
    assert!(
        !text_of(&deck).contains("Un título nuevo"),
        "the edit reached the document anyway"
    );
}

/// Both fault points, on **every** package family, because each has its own
/// writer and the criterion is about the file rather than about a deck.
///
/// The two tests above prove the mechanism on a deck in detail; this one proves
/// it is not a property of that one writer. Every case asserts the same three
/// things: the fault is the typed one that was asked for, the original is
/// byte-identical, and nothing was left in the directory.
#[test]
fn an_injected_failure_leaves_every_package_family_untouched() {
    let dir = scratch("fault-families");
    for rel in [
        "docx/long-report.docx",
        "odt/basic-text.odt",
        "xlsx/values-types.xlsx",
        "ods/values-types.ods",
        "pptx/basic-slides.pptx",
    ] {
        for point in [FaultPoint::AfterApply, FaultPoint::BeforeRename] {
            let path = copy(rel, &dir);
            let before = digest(&path);
            // An operation that certainly applies, so the fault is what stops
            // the request and not the operation.
            let error = edit_path(
                &path,
                &EditRequest::new(vec![Operation::InsertAfter {
                    target: "doc/1".into(),
                    content: match rel.split('/').next() {
                        Some("pptx") => "## Diapositiva nueva {.slide}\n\nUn párrafo.\n".into(),
                        Some("xlsx") | Some("ods") => {
                            "# Hoja nueva {.sheet}\n\n| A |\n|---|\n| uno |\n".to_string()
                        }
                        _ => "| a | b |\n|---|---|\n| 1 | 2 |\n".to_string(),
                    },
                    if_match: None,
                }]),
                &EditOptions {
                    fault: Some(point),
                    ..EditOptions::default()
                },
            );
            match &error {
                Err(EditError::InjectedFault { at, temporary }) => {
                    assert_eq!(*at, point, "{rel}: the wrong fault fired");
                    // A fault before the rename has a complete document on
                    // disk, beside the destination; one before serialisation
                    // has nothing at all.
                    match point {
                        FaultPoint::BeforeRename => assert_eq!(
                            temporary.as_ref().and_then(|t| t.parent()),
                            path.parent(),
                            "{rel}: the temporary file is not beside the destination"
                        ),
                        FaultPoint::AfterApply => {
                            assert!(temporary.is_none(), "{rel}: nothing is serialised yet")
                        }
                    }
                }
                other => panic!("{rel} at {point:?}: {other:?}"),
            }
            assert_eq!(
                digest(&path),
                before,
                "{rel} at {point:?}: the original moved"
            );
            assert!(
                leftovers(&dir).is_empty(),
                "{rel} at {point:?}: {:?}",
                leftovers(&dir)
            );
        }
    }
}

fn text_of(path: &Path) -> String {
    let out = path.with_extension("view.dmk.md");
    docsai_convert::convert_file(path, Some(&out), &ConvertOptions::default())
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let text = std::fs::read_to_string(&out).expect("reads the view");
    let _ = std::fs::remove_file(&out);
    text
}

// ---------------------------------------------------------------- byte identity

/// 15-B's rule reaches here: nothing is written unless it differs from what the
/// part already says. A batch of **zero** operations must leave the package
/// exactly as it was.
#[test]
fn an_empty_batch_leaves_a_preserved_deck_byte_identical() {
    let dir = scratch("empty-batch");
    for rel in [
        "pptx/basic-slides.pptx",
        "pptx/notes-speaker.pptx",
        "pptx/tables-simple.pptx",
        "pptx/raw-preserved.pptx",
    ] {
        let deck = copy(rel, &dir);
        let before = digest(&deck);
        let outcome = edit_path(&deck, &EditRequest::new(vec![]), &EditOptions::default())
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(outcome.report.is_empty(), "{rel}: {:?}", outcome.report);
        assert_eq!(digest(&deck), before, "{rel}: an empty batch rewrote it");
    }
}

/// And an operation that changes nothing is the same case arrived at the other
/// way: the writer compares against the part, not against the request.
#[test]
fn an_operation_that_changes_nothing_leaves_a_preserved_deck_byte_identical() {
    let dir = scratch("no-op-batch");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let slides = slide_ids(&deck);
    let target = format!("{}/1", slides[0]);
    let doc = document(&deck);
    let current = match docsai_model::addressing::resolve_str(&doc, &target).expect("resolves") {
        docsai_model::addressing::Place::Shape(shape) => match &shape.kind {
            docsai_model::presentation::ShapeKind::Placeholder(ph) => ph
                .body
                .iter()
                .filter_map(|b| match b {
                    docsai_model::text::Block::Paragraph(p) => Some(p.plain_text()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(""),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    let before = digest(&deck);
    edit_path(
        &deck,
        &EditRequest::new(vec![replace_text(&target, &current)]),
        &EditOptions::default(),
    )
    .expect("applies");
    assert_eq!(
        digest(&deck),
        before,
        "writing a shape's own text back rewrote the package"
    );
}

/// «Editing a node never perturbs a sibling node's serialization», at the level
/// the claim is made: the members of the package.
#[test]
fn an_edit_touches_exactly_the_members_it_has_to() {
    let dir = scratch("members");
    let deck = copy("pptx/slide-order.pptx", &dir);
    let before = members(&deck);
    let slides = slide_ids(&deck);
    // The *second* slide, so that a change to the first cannot be confused with
    // a package-wide rewrite.
    edit_path(
        &deck,
        &EditRequest::new(vec![replace_text(
            &format!("{}/1", slides[1]),
            "Segunda, retitulada",
        )]),
        &EditOptions::default(),
    )
    .expect("applies");
    let after = members(&deck);

    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the member list changed"
    );
    let differing: Vec<&String> = before
        .keys()
        .filter(|name| before[*name] != after[*name])
        .collect();
    assert_eq!(
        differing.len(),
        1,
        "the edit reached members it had no business in: {differing:?}"
    );
    // The part is a slide part, and it is the one holding the new title. Its
    // *number* is the package's, not the deck's: `slide-order.pptx` presents
    // its slides in an order its `ppt/slides/slideN.xml` names do not follow,
    // which is precisely why an edit addresses by node and not by file.
    let name = differing[0].clone();
    assert!(
        name.starts_with("ppt/slides/slide") && name.ends_with(".xml"),
        "{name} is not a slide part"
    );
    let changed = String::from_utf8_lossy(&after[&name]).into_owned();
    assert!(
        changed.contains("Segunda, retitulada"),
        "the new title is not in {name}"
    );
}

/// The same claim one level finer, and this is the level the criterion is
/// actually made at: **inside** the part that changed, every sibling's
/// serialization is byte-identical.
///
/// `an_edit_touches_exactly_the_members_it_has_to` proves no other part moves,
/// which for a deck is most of the claim because a slide is a part of its own.
/// It is not the claim for a text document or a sheet, where every sibling
/// lives in the same XML as the target. So: edit the second element of a
/// sequence, split the changed part on the family's own element boundary, and
/// assert that exactly one element differs.
///
/// **What «before» is, stated rather than assumed.** The preserved skeleton is
/// a pptx property (Phase 15): a deck is written back over its own package and
/// an untouched part keeps its bytes. The docx, odt, xlsx and ods writers build
/// their parts from the model, so an edit's baseline for them is not the
/// original package — it is *what the writer would have produced from the same
/// document with no operations at all*, which this test takes by running an
/// empty batch first. Against that baseline the criterion means exactly what it
/// says for all five families, and it is the strongest form the writers
/// currently support. That pptx is the only family where the baseline is the
/// original bytes is measured here too, and carried forward rather than hidden:
/// see `kb/92-phase-17-loop.md`.
#[test]
fn an_edit_leaves_every_sibling_byte_identical_inside_the_part_it_changed() {
    let dir = scratch("siblings");
    std::fs::create_dir_all(dir.join("baseline")).expect("creates the baseline directory");
    // The element boundary each family serializes a sibling to, and the
    // operation that changes exactly one of them.
    let cases: [(&str, &str, Operation); 5] = [
        (
            "docx/long-report.docx",
            "</w:p>",
            replace_text("doc/2", "Segundo, reescrito"),
        ),
        (
            "odt/basic-text.odt",
            "</text:p>",
            replace_text("doc/2", "Segundo, reescrito"),
        ),
        (
            "pptx/slide-order.pptx",
            "</a:p>",
            replace_text("doc/2/1", "Segunda, retitulada"),
        ),
        (
            "xlsx/values-types.xlsx",
            "</row>",
            Operation::SetCell {
                target: "n1".into(),
                // A number into a numeric cell: the sheet part is the only
                // one with anything to say about it, so the comparison below is
                // about rows and not about a shared-string table.
                cell: "B2".into(),
                value: Some(Some(docsai_convert::Scalar::Int(1200))),
                if_match: None,
            },
        ),
        (
            "ods/values-types.ods",
            "</table:table-row>",
            Operation::SetCell {
                target: "n1".into(),
                // A number into a numeric cell: the sheet part is the only
                // one with anything to say about it, so the comparison below is
                // about rows and not about a shared-string table.
                cell: "B2".into(),
                value: Some(Some(docsai_convert::Scalar::Int(1200))),
                if_match: None,
            },
        ),
    ];

    for (rel, close, operation) in cases {
        // The baseline: the same document, written back with nothing done to
        // it. For a deck this is the original package byte for byte, and the
        // assertion below says so.
        let baseline = copy(rel, &dir.join("baseline"));
        let original = digest(&baseline);
        edit_path(
            &baseline,
            &EditRequest::new(vec![]),
            &EditOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(
            digest(&baseline) == original,
            rel.starts_with("pptx/"),
            "{rel}: whether an empty batch is byte-identical is a property of \
             the preserved skeleton, and only pptx has one"
        );

        let path = copy(rel, &dir);
        let before = members(&baseline);
        edit_path(
            &path,
            &EditRequest::new(vec![operation]),
            &EditOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{rel}: {e}"));
        let after = members(&path);

        assert_eq!(
            before.keys().collect::<Vec<_>>(),
            after.keys().collect::<Vec<_>>(),
            "{rel}: the member list changed"
        );
        let changed: Vec<&String> = before
            .keys()
            .filter(|name| before[*name] != after[*name])
            .collect();
        assert_eq!(
            changed.len(),
            1,
            "{rel}: the edit reached members it had no business in: {changed:?}"
        );
        let name = changed[0].clone();

        let was = elements(&before[&name], close);
        let now = elements(&after[&name], close);
        assert!(
            was.len() > 1,
            "{rel}: {name} has no siblings to leave alone"
        );
        assert_eq!(
            was.len(),
            now.len(),
            "{rel}: {name} gained or lost a `{close}` element"
        );
        let differing: Vec<usize> = (0..was.len()).filter(|i| was[*i] != now[*i]).collect();
        assert_eq!(
            differing.len(),
            1,
            "{rel}: {} of {name}'s {close} elements moved, not 1",
            differing.len()
        );
    }
}

/// A part split into the elements a sibling serializes to, boundaries kept, so
/// that comparing two splits compares whole serializations.
fn elements(bytes: &[u8], close: &str) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut out = Vec::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(close) {
        let (head, tail) = rest.split_at(at + close.len());
        out.push(head.to_string());
        rest = tail;
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

// ---------------------------------------------------------------- 17-D

/// «`dry_run` runs everything except the write and answers with the same report
/// the real call would return.»
///
/// Nothing in the report can only be known after the rename, so the two reports
/// are asserted **equal**, not merely similar.
#[test]
fn a_dry_run_and_the_real_call_return_the_same_report_and_only_one_writes() {
    let dir = scratch("dry-run");
    for rel in [
        "pptx/basic-slides.pptx",
        "docx/long-report.docx",
        "xlsx/values-types.xlsx",
        "odt/basic-text.odt",
        "ods/values-types.ods",
    ] {
        let path = copy(rel, &dir);
        let before = digest(&path);
        let operations = vec![
            replace_text("doc/1", "Una cosa distinta"),
            Operation::InsertAfter {
                target: "doc/1".into(),
                content: "| a | b |\n|---|---|\n| 1 | 2 |\n".into(),
                if_match: None,
            },
        ];
        let operations = if rel.contains("xlsx") || rel.contains("ods") {
            let sheet = docsai_model::addressing::ids_in_document_order(&document(&path))
                .into_iter()
                .find(|(_, k)| *k == NodeKind::Sheet)
                .expect("a sheet")
                .0
                 .0;
            vec![Operation::SetCell {
                target: sheet,
                cell: "H9".into(),
                value: Some(Some(docsai_convert::Scalar::Int(42))),
                if_match: None,
            }]
        } else if rel.contains("pptx") {
            let slide = slide_ids(&path)[0].clone();
            vec![replace_text(&format!("{slide}/1"), "Una cosa distinta")]
        } else {
            operations
        };

        let dry = edit_path(
            &path,
            &EditRequest::new(operations.clone()).dry(),
            &EditOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(dry.dry_run);
        assert_eq!(
            digest(&path),
            before,
            "{rel}: a dry run wrote to the document"
        );
        assert!(leftovers(&dir).is_empty(), "{rel}: {:?}", leftovers(&dir));

        let wet = edit_path(
            &path,
            &EditRequest::new(operations),
            &EditOptions::default(),
        )
        .unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert!(!wet.dry_run);
        assert_eq!(
            dry.report, wet.report,
            "{rel}: the dry run and the real call disagree"
        );
        assert_eq!(
            serde_json::to_string(&dry.report).expect("serialises"),
            serde_json::to_string(&wet.report).expect("serialises"),
            "{rel}: the two reports differ byte for byte on the wire"
        );
        assert_ne!(digest(&path), before, "{rel}: the real call wrote nothing");
    }
}

/// The report carries the **new etags**, so a caller never needs a confirmation
/// read. That is the phase's economic argument, and it is a test.
#[test]
fn the_report_carries_etags_a_reader_would_compute_so_no_confirmation_read_is_needed() {
    let dir = scratch("etags");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let slides = slide_ids(&deck);
    let target = format!("{}/1", slides[0]);
    let report = edit_path(
        &deck,
        &EditRequest::new(vec![replace_text(&target, "Resultados del Q3")]),
        &EditOptions::default(),
    )
    .expect("applies")
    .report;

    assert_eq!(report.changed.len(), 1);
    let row = &report.changed[0];
    assert_eq!(row.id, target);

    let doc = document(&deck);
    let place = docsai_model::addressing::resolve_str(&doc, &row.id).expect("resolves");
    assert_eq!(
        place.etag().expect("an etag").0,
        row.etag,
        "the etag in the report is not the one a read computes"
    );

    // And it is a usable precondition: the very next call may guard on it.
    edit_path(
        &deck,
        &EditRequest::new(vec![Operation::ReplaceText {
            target: target.clone(),
            text: "Resultados del Q4".into(),
            if_match: Some(row.etag.clone()),
        }]),
        &EditOptions::default(),
    )
    .expect("the etag from the previous report is still current");

    // A stale one is refused, loudly, and never overwrites.
    let error = edit_path(
        &deck,
        &EditRequest::new(vec![Operation::ReplaceText {
            target: target.clone(),
            text: "Nunca".into(),
            if_match: Some(row.etag.clone()),
        }]),
        &EditOptions::default(),
    );
    match error {
        Err(EditError::Refused(refused)) => assert!(
            matches!(refused.refusal, docsai_convert::Refusal::StaleEtag { .. }),
            "{:?}",
            refused.refusal
        ),
        other => panic!("{other:?}"),
    }
    assert!(!text_of(&deck).contains("Nunca"));
}

/// What the report costs on the wire, measured with the repository's own
/// tokenizer over the framed JSON — the number 17-D's diff decision was taken
/// on (`crates/docsai-convert/src/edit/report.rs`).
#[test]
fn the_report_for_a_realistic_batch_is_small_enough_to_be_free() {
    let dir = scratch("wire-cost");
    let deck = copy("pptx/forty-slides.pptx", &dir);
    let slides = slide_ids(&deck);
    let fourth = slides[3].clone();
    let report = edit_path(
        &deck,
        &EditRequest::new(vec![
            replace_text(&format!("{fourth}/1"), "Resultados del Q3"),
            Operation::SetNotes {
                target: fourth.clone(),
                content: Some(Some("Ingresos al alza un 12 %".into())),
                if_match: None,
            },
        ]),
        &EditOptions::default(),
    )
    .expect("applies")
    .report;

    let json = serde_json::to_string(&report).expect("serialises");
    let tokens = docsai_convert::tokens::count(&json);
    println!("report: {json}\n{tokens} tokens");
    assert!(
        tokens <= 120,
        "the change report costs {tokens} tokens, which is no longer free: {json}"
    );
}

/// A document nobody may address is refused before it is read (§3).
#[test]
fn editing_at_a_fidelity_that_carries_no_ids_is_refused() {
    let dir = scratch("unaddressable");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let before = digest(&deck);
    for (fidelity, ids) in [
        (docsai_convert::Fidelity::Plain, None),
        (docsai_convert::Fidelity::Standard, None),
        (
            docsai_convert::Fidelity::Full,
            Some(docsai_model::addressing::IdPolicy::Never),
        ),
    ] {
        let options = EditOptions {
            convert: ConvertOptions {
                fidelity,
                ids,
                ..ConvertOptions::default()
            },
            ..EditOptions::default()
        };
        match edit_path(&deck, &EditRequest::new(vec![]), &options) {
            Err(EditError::Refused(refused)) => assert!(
                matches!(
                    refused.refusal,
                    docsai_convert::Refusal::Unaddressable { .. }
                ),
                "{:?}",
                refused.refusal
            ),
            other => panic!("{fidelity:?}/{ids:?}: {other:?}"),
        }
    }
    assert_eq!(digest(&deck), before);
}

/// An `ops` version this build does not speak is refused rather than
/// best-efforted (§1.2).
#[test]
fn a_future_operation_set_version_is_refused() {
    let dir = scratch("ops-version");
    let deck = copy("pptx/basic-slides.pptx", &dir);
    let request = EditRequest {
        ops: Some(docsai_convert::OPS_VERSION + 1),
        operations: vec![],
        dry_run: false,
    };
    match edit_path(&deck, &request, &EditOptions::default()) {
        Err(EditError::Refused(refused)) => assert!(matches!(
            refused.refusal,
            docsai_convert::Refusal::UnsupportedOpsVersion { .. }
        )),
        other => panic!("{other:?}"),
    }
}
