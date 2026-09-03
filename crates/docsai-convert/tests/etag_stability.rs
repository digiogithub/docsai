//! Etag stability, end to end (plan v2, Phase 17-B).
//!
//! `crates/docsai-model/tests/target.rs` proves the etag is a pure function of
//! the node and its subtree. That is necessary and not sufficient: what an
//! agent actually holds is an etag it got from a *read*, and it comes back
//! later through a possibly different door, with a possibly warm cache, in a
//! different process. So the property this file measures is the one 17-B is
//! judged on:
//!
//! - **stable across a read that changed nothing** — twice from the same file,
//!   cold cache and warm cache, from a path and from bytes (the two doors'
//!   input shapes), and pinned by literal so a new process and a new release
//!   are covered too;
//! - **different after any change that reaches the package** — measured by
//!   actually writing the package and reading it back, not by editing the tree
//!   in memory.
//!
//! One honest limit is recorded rather than hidden, in
//! `formatting_alone_does_not_move_an_etag` over in the model tests: an etag
//! covers content and structure, not appearance. See `kb/87-phase-17-etags.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use docsai_convert::{
    convert_file, read_path, select_input, ConvertCache, ConvertOptions, Fidelity, Selector,
    SourceInput,
};
use docsai_model::addressing::{assign_ids, etag_of, node_ids, Etag, Target};
use docsai_model::{Document, IdPolicy, MemoryAssetStore, NodeId};

fn corpus(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
}

/// Documents covering the three IR roots and both package families.
const DOCUMENTS: [&str; 4] = [
    "docx/long-report.docx",
    "odt/table-simple.odt",
    "pptx/basic-slides.pptx",
    "xlsx/formulas-basic.xlsx",
];

/// Every id in the document, with the etag of the node holding it.
fn etags(doc: &Document) -> Vec<(NodeId, Etag)> {
    node_ids(doc)
        .into_iter()
        .map(|id| {
            let target = Target::parse(id.as_str()).expect("an allocated id is a valid target");
            let etag = etag_of(doc, &target).unwrap_or_else(|e| panic!("{id}: {e}"));
            (id, etag)
        })
        .collect()
}

/// Reads a document and gives it the ids a `full` write would, which is the
/// state an agent ever sees one in: ids are allocated by the serializer, not
/// by the reader, so a bare `read_path` addresses nothing.
fn read(path: &Path) -> Document {
    let mut store = MemoryAssetStore::new();
    let mut doc = read_path(path, &mut store).expect("the corpus reads").0;
    assign_ids(&mut doc, IdPolicy::Assign);
    doc
}

#[test]
fn two_reads_of_the_same_file_agree_on_every_etag() {
    for document in DOCUMENTS {
        let path = corpus(document);
        assert!(path.exists(), "corpus document {document} is missing");
        let first = etags(&read(&path));
        let second = etags(&read(&path));
        assert!(!first.is_empty(), "{document} addresses nothing");
        assert_eq!(first, second, "{document}: an idle read moved an etag");
    }
}

/// The cache is a pure optimisation, and an etag is exactly the kind of derived
/// value a cache gets wrong: a hit that returned a differently-shaped document
/// would hand the caller a precondition that fails against the file on disk.
#[test]
fn a_warm_cache_answers_with_the_same_etags_as_a_cold_one() {
    for document in DOCUMENTS {
        let path = corpus(document);
        let cache = ConvertCache::shared();
        let options = ConvertOptions {
            fidelity: Fidelity::Full,
            cache: Some(Arc::clone(&cache)),
            ..Default::default()
        };
        let selector: Selector = "s1-s5".parse().expect("a selector");

        let cold = select_input(SourceInput::Path(&path), &options, &selector).expect("cold read");
        let stats_after_cold = cache.stats();
        let warm = select_input(SourceInput::Path(&path), &options, &selector).expect("warm read");
        let stats_after_warm = cache.stats();

        assert_eq!(
            stats_after_cold.misses, 1,
            "{document}: the cold read missed"
        );
        assert_eq!(
            stats_after_warm.hits,
            stats_after_cold.hits + 1,
            "{document}: the second read did not hit the cache, so this test \
             proved nothing"
        );

        let cold: Vec<_> = cold.nodes.iter().map(|n| (&n.id, &n.etag)).collect();
        let warm: Vec<_> = warm.nodes.iter().map(|n| (&n.id, &n.etag)).collect();
        assert_eq!(cold, warm, "{document}: the cache moved an etag");
    }
}

/// The CLI reads a path (`docsai read --select`); the MCP server may be handed
/// `content_base64` instead. Both call [`select_input`], and the etags it
/// reports must not depend on which shape the bytes arrived in — otherwise an
/// agent that read through one door cannot write through the other.
#[test]
fn the_two_doors_report_the_same_etags() {
    for document in DOCUMENTS {
        let path = corpus(document);
        let options = ConvertOptions {
            fidelity: Fidelity::Full,
            ..Default::default()
        };
        let selector: Selector = "s1-s5".parse().expect("a selector");
        let filename = path.file_name().unwrap().to_str().unwrap();
        let bytes = std::fs::read(&path).expect("the corpus reads");

        let from_path =
            select_input(SourceInput::Path(&path), &options, &selector).expect("path read");
        let from_bytes = select_input(
            SourceInput::Bytes {
                data: &bytes,
                filename,
            },
            &options,
            &selector,
        )
        .expect("bytes read");

        let path_etags: Vec<_> = from_path.nodes.iter().map(|n| (&n.id, &n.etag)).collect();
        let byte_etags: Vec<_> = from_bytes.nodes.iter().map(|n| (&n.id, &n.etag)).collect();
        assert_eq!(path_etags, byte_etags, "{document}: the doors disagree");
    }
}

/// Cross-process and cross-release stability, pinned by literal.
///
/// The values below are what `docx/long-report.docx` hashes to today. They are
/// checked in for the same reason a golden is: «FNV-1a is deterministic» is an
/// argument, and an argument does not notice the day somebody changes what a
/// reader puts in the tree. A failure here is not a broken test — it means
/// every etag an agent is holding for this document just stopped matching, and
/// that is a specification-level event.
#[test]
fn etags_of_a_corpus_document_are_pinned() {
    let doc = read(&corpus("docx/long-report.docx"));
    let etags = etags(&doc);
    let found = |id: &str| {
        etags
            .iter()
            .find(|(node, _)| node.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is not in the document"))
            .1
            .clone()
    };
    for (id, expected) in PINNED {
        assert_eq!(found(id).as_str(), *expected, "{id}");
    }
}

const PINNED: &[(&str, &str)] = &[
    ("n1", "b62bbb"),
    ("n2", "5e8876"),
    ("n3", "8eb3e5"),
    ("n4", "36576c"),
];

/// The other half of the criterion, and the half that has to go through the
/// package to mean anything: edit one node, write a real `.docx`, read it back,
/// and see exactly one etag move.
#[test]
fn a_change_that_reaches_the_package_moves_exactly_that_node_s_etag() {
    let dir = tempfile::tempdir().expect("a working directory");
    let source = corpus("docx/long-report.docx");
    let before = etags(&read(&source));

    let markdown = dir.path().join("report.dmk.md");
    let options = ConvertOptions {
        fidelity: Fidelity::Full,
        ..Default::default()
    };
    convert_file(&source, Some(&markdown), &options).expect("docx to DocMark");

    // Edit the text of exactly one node, the way a caller would.
    let text = std::fs::read_to_string(&markdown).expect("the DocMark reads");
    let needle = "Estado";
    assert!(
        text.contains(needle),
        "the fixture no longer says {needle:?}"
    );
    let edited = text.replacen(needle, "Estadio", 1);
    assert_ne!(edited, text, "nothing was edited");
    std::fs::write(&markdown, &edited).expect("the DocMark writes");

    let rewritten = dir.path().join("report.docx");
    convert_file(&markdown, Some(&rewritten), &options).expect("DocMark to docx");
    let after = etags(&read(&rewritten));

    let moved: Vec<&NodeId> = before
        .iter()
        .filter_map(|(id, etag)| {
            let now = after.iter().find(|(other, _)| other == id)?;
            (now.1 != *etag).then_some(id)
        })
        .collect();

    assert_eq!(
        moved,
        vec![&NodeId::new("n2")],
        "exactly the edited node's etag must move: everything else in the \
         package was rewritten too, and none of it changed what it says"
    );
    assert_eq!(before.len(), 25, "the fixture's shape changed");
}
