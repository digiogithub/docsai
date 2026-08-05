//! `docsai search`: an address and some context, never the document
//! (plan v2, Phase 11 G).
//!
//! The two properties that make the command worth having are tested against the
//! corpus rather than against a fixture: **it finds what the document says**,
//! including the prose that carries no id, and **it composes with
//! `read --select`** — a hit that names a selector is a hit whose selector
//! reads the text back.

use std::path::{Path, PathBuf};

use docsai_convert::{search_path, select_path, ConvertOptions, Location, Query, Selector};
use docsai_docmark::Fidelity;

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(name)
}

fn results(name: &str, query: &str) -> docsai_convert::SearchResults {
    search_path(
        &corpus(name),
        &ConvertOptions::default(),
        &query.parse::<Query>().unwrap(),
    )
    .unwrap()
}

#[test]
fn prose_that_carries_no_id_is_still_found() {
    // The whole reason the unit is the block and not the addressed node: the
    // 9 000-token report is 60 paragraphs of prose under 40 headings, and only
    // the headings have ids (spec §11.1).
    let results = results("docx/long-report.docx", "rendimiento medido");
    assert!(results.matches > 5, "{} matches", results.matches);
    let relative = results
        .hits
        .iter()
        .filter(|hit| matches!(hit.location, Location::Relative { .. }))
        .count();
    assert!(
        relative > 0,
        "a search that only reported addressed nodes would find headings and nothing else"
    );
    for hit in &results.hits {
        if let Location::Relative { anchor, path, .. } = &hit.location {
            let anchor = anchor.as_ref().expect("the report starts with a heading");
            assert!(
                path.starts_with(&format!("{}.b", anchor.0)),
                "a relative address has to say what it is relative to: {path}"
            );
        }
    }
}

#[test]
fn a_hit_that_names_a_selector_reads_back_the_text_it_matched() {
    // Search and read compose, or search is a dead end.
    let results = results("docx/long-report.docx", "Estado de modelo de datos");
    let mut checked = 0;
    for hit in &results.hits {
        let Some(select) = hit.select() else { continue };
        let selection = select_path(
            &corpus("docx/long-report.docx"),
            &ConvertOptions::default(),
            &select.parse::<Selector>().unwrap(),
        )
        .unwrap();
        assert!(
            selection
                .docmark
                .to_lowercase()
                .contains(&hit.snippets[0].matched.to_lowercase()),
            "`read --select {select}` does not contain what search found there:\n{}",
            selection.docmark
        );
        checked += 1;
    }
    assert!(checked > 0, "no hit named a selector");
}

#[test]
fn a_position_means_the_same_thing_it_means_in_outline() {
    let results = results("docx/long-report.docx", "Estado de modelo de datos");
    let hit = results
        .hits
        .iter()
        .find(|hit| matches!(hit.location, Location::Node { .. }))
        .expect("the heading matched");
    let Location::Node { position, id, .. } = &hit.location else {
        unreachable!()
    };
    let selection = select_path(
        &corpus("docx/long-report.docx"),
        &ConvertOptions::default(),
        &format!("s{position}").parse::<Selector>().unwrap(),
    )
    .unwrap();
    assert_eq!(
        selection.nodes[0].id, *id,
        "`sN` from a hit must select the node the hit named"
    );
}

#[test]
fn the_answer_costs_a_fraction_of_the_document() {
    let results = results("docx/long-report.docx", "conclusion");
    assert!(
        results.tokens * 10 < results.document_tokens,
        "{} tokens of a {}-token document",
        results.tokens,
        results.document_tokens
    );
    assert!(results.matches > 10);
}

#[test]
fn a_common_word_is_capped_rather_than_dumped() {
    // The cap is what keeps "not the document" true for a query that matches
    // everywhere: the matches are counted, the blocks are not all listed.
    let mut query = Query::new("de").unwrap();
    query.limit = Some(3);
    let results = search_path(
        &corpus("docx/long-report.docx"),
        &ConvertOptions::default(),
        &query,
    )
    .unwrap();
    assert_eq!(results.hits.len(), 3);
    assert!(results.omitted > 0, "the rest have to be counted, not lost");
    assert!(results.blocks > 3);
    for hit in &results.hits {
        assert!(
            hit.snippets.len() <= 3,
            "a block that says it twenty times is still one place to go"
        );
    }
}

#[test]
fn a_footnote_is_found_and_handed_over_at_its_reference() {
    let results = results("docx/footnotes.docx", "nota");
    let footnote = results
        .hits
        .iter()
        .find(|hit| {
            matches!(&hit.location, Location::Node { kind, .. }
                if *kind == docsai_model::NodeKind::Footnote)
        })
        .expect("the footnote definition holds the word");
    let select = footnote.select().expect("a footnote hit names a selector");
    let selection = select_path(
        &corpus("docx/footnotes.docx"),
        &ConvertOptions::default(),
        &select.parse::<Selector>().unwrap(),
    )
    .unwrap();
    assert!(
        selection
            .nodes
            .iter()
            .all(|n| n.kind != docsai_model::NodeKind::Footnote),
        "a footnote is never selected on its own; the block that refers to it is"
    );
    assert!(
        selection.docmark.contains("[^1]: "),
        "and that block carries the definition search matched:\n{}",
        selection.docmark
    );
}

#[test]
fn every_corpus_document_can_be_searched_for_what_it_says() {
    let mut checked = 0;
    for path in corpus_documents() {
        // A word the document itself wrote, taken out of its own DocMark: if
        // search cannot find that, it cannot find anything.
        let Some(expected) = golden_word(&path) else {
            continue;
        };
        let results = search_path(
            &path,
            &ConvertOptions::default(),
            &Query::new(expected.clone()).unwrap(),
        )
        .unwrap();
        assert!(
            results.matches > 0,
            "{}: `{expected}` is in the document and search missed it",
            path.display()
        );
        assert!(
            results.tokens <= results.document_tokens,
            "{}: the answer must not cost more than the question",
            path.display()
        );
        checked += 1;
    }
    assert!(checked > 20, "only {checked} documents were searched");
}

#[test]
fn a_lossy_level_finds_the_text_and_says_it_cannot_address_it() {
    let options = ConvertOptions {
        fidelity: Fidelity::Plain,
        ..Default::default()
    };
    let results = search_path(
        &corpus("docx/long-report.docx"),
        &options,
        &"conclusion".parse::<Query>().unwrap(),
    )
    .unwrap();
    assert!(results.matches > 0, "plain still writes the text");
    assert!(
        results.hits.iter().all(|hit| hit.select().is_none()),
        "and carries no id to hand back"
    );
}

/// A word from the document's golden DocMark, long enough to be distinctive.
fn golden_word(path: &Path) -> Option<String> {
    let golden = path.with_extension("expected.dmk.md");
    let text = std::fs::read_to_string(golden).ok()?;
    let body = text.split_once("\n---\n").map(|(_, b)| b).unwrap_or(&text);
    body.split_whitespace()
        .find(|word| word.chars().count() > 5 && word.chars().all(|c| c.is_alphabetic()))
        .map(str::to_string)
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
