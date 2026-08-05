//! `docsai read --select`: a selection is a document (plan v2, Phase 11 F).
//!
//! The phase's acceptance criterion is that the output re-parses standalone and
//! that re-writing it produces no warning other than the documented partial one.
//! These tests take that literally: they parse what `select` wrote, serialise it
//! again and compare bytes.

use std::path::{Path, PathBuf};

use docsai_convert::{outline_path, select_path, ConvertOptions, Selector};
use docsai_docmark::{Fidelity, Options as DocMarkOptions};
use docsai_model::{MemoryAssetStore, Warning};

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(name)
}

fn selection(name: &str, selector: &str) -> docsai_convert::Selection {
    let options = ConvertOptions::default();
    select_path(
        &corpus(name),
        &options,
        &selector.parse::<Selector>().unwrap(),
    )
    .unwrap()
}

/// Parses a selection and writes it out again, as a caller editing it would.
fn rewrite(markdown: &str) -> (String, Vec<Warning>) {
    rewrite_in(markdown, MemoryAssetStore::new())
}

/// Same, with the store seeded from the source document.
///
/// A selection links its images the way the whole document does, so re-parsing
/// one needs the same media in reach — the way a real caller has them next to
/// the file it is editing. Without seeding, the comparison would only be
/// measuring that a missing asset is missing.
fn rewrite_with_assets(markdown: &str, source: &Path) -> (String, Vec<Warning>) {
    let mut assets = MemoryAssetStore::new();
    let _ = docsai_convert::read_path(source, &mut assets).unwrap();
    rewrite_in(markdown, assets)
}

fn rewrite_in(markdown: &str, mut assets: MemoryAssetStore) -> (String, Vec<Warning>) {
    let (doc, _) = docsai_docmark::parse(markdown, &mut assets).unwrap();
    let options = DocMarkOptions {
        source_format: doc_source_format(markdown),
        ..Default::default()
    };
    let (out, report) = docsai_docmark::serialize(&doc, &assets, &options);
    (out, report.warnings)
}

/// The `source-format` the selection declared, so re-writing it declares the
/// same one — it says where the document came from, not what it is now.
fn doc_source_format(markdown: &str) -> docsai_model::Format {
    markdown
        .lines()
        .find_map(|line| line.strip_prefix("source-format: "))
        .and_then(docsai_model::Format::parse)
        .unwrap_or(docsai_model::Format::Docx)
}

#[test]
fn a_selection_re_parses_and_re_writes_to_the_same_bytes() {
    for (name, selector) in [
        ("docx/nested-lists.docx", "s1-s4"),
        ("docx/long-report.docx", "s3-s5"),
        ("docx/footnotes.docx", "type:paragraph"),
        ("docx/table-merged.docx", "type:table"),
    ] {
        let selection = selection(name, selector);
        assert!(!selection.is_empty(), "{name} {selector} matched nothing");
        let (again, _) = rewrite(&selection.docmark);
        assert_eq!(
            again, selection.docmark,
            "{name} {selector}: a selection must re-write to itself"
        );
    }
}

/// The strong form of the phase's acceptance criterion: not on four documents,
/// on every one the corpus has.
#[test]
fn every_corpus_document_selects_into_a_document() {
    let mut checked = 0;
    for path in corpus_documents() {
        let selection = select_path(
            &path,
            &ConvertOptions::default(),
            &"s1-s5".parse::<Selector>().unwrap(),
        )
        .unwrap();
        if selection.is_empty() {
            continue; // nothing addressable; `outline` shows nothing either
        }
        let (again, warnings) = rewrite_with_assets(&selection.docmark, &path);
        // `ETAGS_MOVE_ON_REPARSE` is written with `/`, so the separator has to
        // be the same one on every host: on Windows this name arrives with
        // backslashes, misses the list, and the exception reads as a failure.
        let name = path
            .strip_prefix(corpus(""))
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        assert_eq!(
            body_of(&again),
            body_of(&selection.docmark),
            "{name}: the body of a selection must re-write to itself"
        );
        if ETAGS_MOVE_ON_REPARSE.contains(&name.as_str()) {
            assert_ne!(again, selection.docmark, "{name} is no longer an exception");
        } else {
            assert_eq!(
                again, selection.docmark,
                "{name} does not re-write to itself"
            );
        }
        assert!(
            warnings
                .iter()
                .all(|w| matches!(w, Warning::PartialDocument { .. })),
            "{}: {warnings:?}",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 20, "only {checked} documents were selected from");
}

/// Documents whose etags move across a round trip, and why.
///
/// `odt/table-merged.odt` reads as rows of three cells, two of them spanning;
/// the DocMark of that table pads every row to four columns, so re-parsing it
/// gives rows of four. The *text* is identical either way — which is why the
/// goldens never showed it — but the etag hashes the cells, so it moves. That
/// is a defect of the ODF table round trip, not of the selection, and fixing it
/// belongs where tables are read, not here. Pinned rather than tolerated: a
/// second entry appearing in this list is a regression.
const ETAGS_MOVE_ON_REPARSE: &[&str] = &["odt/table-merged.odt"];

/// Everything after the front matter.
fn body_of(markdown: &str) -> &str {
    markdown
        .split_once("\n---\n\n")
        .map_or(markdown, |(_, body)| body)
}

fn corpus_documents() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (subdir, extension) in [
        ("docx", "docx"),
        ("xlsx", "xlsx"),
        ("odt", "odt"),
        ("ods", "ods"),
    ] {
        let dir = corpus(subdir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some(extension) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn a_footnote_reference_never_travels_without_its_definition() {
    let selection = selection("docx/footnotes.docx", "s1");
    assert!(
        selection.docmark.contains("[^1]{#n2}"),
        "the selected paragraph refers to a footnote:\n{}",
        selection.docmark
    );
    assert!(
        selection.docmark.contains("[^1]: "),
        "and the definition has to come with it:\n{}",
        selection.docmark
    );
}

#[test]
fn re_writing_a_selection_warns_that_it_is_one_and_nothing_else() {
    let selection = selection("docx/long-report.docx", "s2-s4");
    let (_, warnings) = rewrite(&selection.docmark);
    assert_eq!(
        warnings.len(),
        1,
        "the only warning a selection earns is that it is one: {warnings:?}"
    );
    assert!(
        matches!(warnings[0], Warning::PartialDocument { .. }),
        "{:?}",
        warnings[0]
    );
    assert!(
        warnings[0].message().contains("writing it back whole"),
        "the warning has to say what the danger is: {}",
        warnings[0].message()
    );
}

#[test]
fn the_selection_is_what_the_whole_document_wrote_for_those_nodes() {
    let selection = selection("docx/long-report.docx", "#n4,#n5");
    let whole = std::fs::read_to_string(corpus("docx/long-report.expected.dmk.md")).unwrap();
    for node in &selection.nodes {
        let fragment = selection
            .docmark
            .lines()
            .find(|line| line.contains(&format!("{{#{}}}", node.id.0)))
            .expect("the selected node is in the body");
        assert!(
            whole.contains(fragment),
            "{} was rewritten instead of quoted: {fragment}",
            node.id.0
        );
    }
}

#[test]
fn positions_are_the_ones_outline_prints() {
    let outline = outline_path(
        &corpus("docx/long-report.docx"),
        &ConvertOptions::default(),
        None,
    )
    .unwrap();
    let flat: Vec<String> = {
        fn walk(nodes: &[docsai_convert::OutlineNode], out: &mut Vec<String>) {
            for node in nodes {
                out.push(node.id.0.clone());
                walk(&node.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&outline.nodes, &mut out);
        out
    };
    let selection = selection("docx/long-report.docx", "s2,s5");
    let ids: Vec<&str> = selection.nodes.iter().map(|n| n.id.0.as_str()).collect();
    assert_eq!(
        ids,
        vec![flat[1].as_str(), flat[4].as_str()],
        "`sN` must mean the N-th line of the outline, or the pair is unusable"
    );
}

#[test]
fn a_node_already_inside_a_selected_one_is_not_written_twice() {
    // `type:list` and the items inside it: asking for both must not duplicate
    // the items.
    let selection = selection("docx/nested-lists.docx", "s1-s9");
    let items = selection.docmark.matches("Sub-punto a").count();
    assert_eq!(items, 1, "\n{}", selection.docmark);
}

#[test]
fn a_selection_carries_no_catalogue_and_no_dictionary() {
    let selection = selection("docx/repeated-formatting.docx", "s1-s3");
    for key in [
        "styles:",
        "style-defaults:",
        "list-definitions:",
        "attribute-sets:",
        "page:",
    ] {
        assert!(
            !selection.docmark.contains(key),
            "the minimum front matter must not carry `{key}`:\n{}",
            selection.docmark
        );
    }
    assert!(
        !selection.docmark.contains("{.g1}"),
        "a selection must not reference a dictionary it does not carry:\n{}",
        selection.docmark
    );
}

#[test]
fn an_etag_changes_when_the_node_does_and_only_then() {
    let first = selection("docx/long-report.docx", "#n2");
    let second = selection("docx/long-report.docx", "#n2");
    assert_eq!(
        first.nodes[0].etag, second.nodes[0].etag,
        "etags are derived"
    );

    let edited = first.docmark.replace("Estado", "Estadio");
    let (rewritten, _) = rewrite(&edited);
    assert!(
        !rewritten.contains(first.nodes[0].etag.as_str()),
        "editing the node has to move its etag, or an if-match write is blind"
    );
}

#[test]
fn a_level_that_addresses_nothing_cannot_be_selected_from() {
    let options = ConvertOptions {
        fidelity: Fidelity::Standard,
        ..Default::default()
    };
    let selection = select_path(
        &corpus("docx/long-report.docx"),
        &options,
        &"s1".parse::<Selector>().unwrap(),
    )
    .unwrap();
    assert!(
        selection.is_empty(),
        "`standard` carries no ids, so it has nothing to select"
    );
    assert!(selection.docmark.is_empty());
}

#[test]
fn a_selection_costs_a_fraction_of_the_document() {
    let selection = selection("docx/long-report.docx", "s2-s3");
    assert!(
        selection.tokens * 10 < selection.document_tokens,
        "two headings of a 9 000-token report cost {} of {}",
        selection.tokens,
        selection.document_tokens
    );
}

#[test]
fn a_footnote_is_not_a_document_of_its_own() {
    let error = "type:footnote".parse::<Selector>().unwrap_err().to_string();
    assert!(
        error.contains("reference"),
        "the refusal has to say where a footnote is addressed: {error}"
    );
    // And a selection that reaches one another way simply does not take it:
    // the paragraph that refers to it is what carries it.
    let selection = selection("docx/footnotes.docx", "type:paragraph");
    assert!(selection
        .nodes
        .iter()
        .all(|node| node.kind != docsai_model::NodeKind::Footnote));
}
