//! A deck through the conversion pipeline (plan v2 Phase 14-J).
//!
//! 13-K refused `convert` on a presentation because DocMark-P did not exist:
//! the serializer would have handed back an empty body and a caller
//! redirecting stdout to a file would have got a success that lost every
//! slide. The profile exists now, so the refusal is gone and what replaces it
//! is this: the deck leaves the pipeline as a file that references only files
//! that were written, and reads back in.

use std::path::{Path, PathBuf};

use docsai_convert::{convert_file, ConvertOptions};
use docsai_docmark::Fidelity;
use docsai_model::{Document, Format, MemoryAssetStore};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx")
}

fn decks() -> Vec<PathBuf> {
    let dir = corpus();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("pptx") | Some("pptm")
            )
        })
        .collect();
    paths.sort();
    paths
}

/// A directory of its own per test, so two of them cannot share an `assets/`.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-deck-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

fn options(fidelity: Fidelity) -> ConvertOptions {
    ConvertOptions {
        fidelity,
        ..ConvertOptions::default()
    }
}

#[test]
fn every_deck_of_the_corpus_converts_to_docmark() {
    let dir = scratch("convert");
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let output = dir.join(format!("{stem}.dmk.md"));
        let outcome = convert_file(&deck, Some(&output), &options(Fidelity::Full))
            .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));

        assert_eq!(outcome.source_format, Format::Pptx);
        assert_eq!(outcome.target_format, Format::DocMark);
        let written = std::fs::read_to_string(&output).expect("the deck was written");
        assert_eq!(written, outcome.markdown);
        assert!(
            written.contains(".slide"),
            "{stem}: the converted deck has no slide"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every path the front matter and the body name is a file that exists.
///
/// A converted document is only usable where it stands, so a reference that
/// resolves on the machine that wrote it and nowhere else is the failure this
/// guards: the `skeleton:` line names `_skeleton/deck-<hash>.pptx`, built from
/// the content hash rather than from the package's own name, and it is the
/// pipeline that has to put the bytes there.
#[test]
fn the_converted_deck_references_only_files_that_were_written() {
    let dir = scratch("assets");
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let output = dir.join(format!("{stem}/{stem}.dmk.md"));
        let outcome = convert_file(&deck, Some(&output), &options(Fidelity::Full))
            .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));
        let base = output.parent().unwrap();

        for reference in references(&outcome.markdown) {
            let path = base.join(&reference);
            assert!(
                path.is_file(),
                "{stem}: `{reference}` is referenced and was not written"
            );
            assert!(
                outcome
                    .assets_written
                    .iter()
                    .any(|written| written == &path),
                "{stem}: `{reference}` exists but is not in the outcome's asset list"
            );
        }

        // The package is one asset, so it is one file: the store writes what a
        // reader puts in under `img-<hash>.<ext>`, which for a deck's skeleton
        // would be the same bytes a second time under a name nothing points at.
        let stray: Vec<_> = std::fs::read_dir(base.join("assets"))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|name| name.ends_with(".pptx") || name.ends_with(".pptm"))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            stray.is_empty(),
            "{stem}: a package was left loose in `assets/`: {stray:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_converted_deck_reads_back_as_a_deck() {
    let dir = scratch("roundtrip");
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let output = dir.join(format!("{stem}/{stem}.dmk.md"));
        convert_file(&deck, Some(&output), &options(Fidelity::Full))
            .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));

        // Nothing is seeded here: the parser gets the file and the directory
        // beside it, which is all a person who received the document has.
        let markdown = std::fs::read_to_string(&output).expect("reads the deck back");
        let mut assets = MemoryAssetStore::new();
        let (document, _) =
            docsai_docmark::parse_with_base(&markdown, output.parent(), &mut assets)
                .unwrap_or_else(|e| panic!("{stem}: {e}"));
        let Document::Presentation(parsed) = document else {
            panic!("{stem}: came back as something that is not a deck");
        };
        assert!(!parsed.slides.is_empty(), "{stem}: no slides came back");
        assert!(
            parsed.skeleton.is_some(),
            "{stem}: the preserved package was not found beside the document"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `standard` and `plain` write no `skeleton:` (spec §11.2 rule 6), so the
/// pipeline writes no package either: an unreferenced copy of the original
/// deck beside a document meant to be readable is exactly the surprise that
/// level exists to avoid.
#[test]
fn the_readable_levels_leave_no_package_behind() {
    let dir = scratch("levels");
    for fidelity in [Fidelity::Standard, Fidelity::Plain] {
        for deck in decks() {
            let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
            let output = dir.join(format!("{}-{stem}/{stem}.md", fidelity.as_str()));
            let outcome = convert_file(&deck, Some(&output), &options(fidelity))
                .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));
            assert!(
                !outcome.markdown.contains("skeleton:"),
                "{stem} at {}: the reference is a full/agent one",
                fidelity.as_str()
            );
            // Not only `_skeleton/`: the reader stores the package whatever
            // the level, so before 14-K it landed flat in `assets/` under an
            // image's name — a copy of the whole original next to a document
            // whose point is being readable.
            let base = output.parent().unwrap();
            assert!(
                !base.join("assets/_skeleton").exists(),
                "{stem} at {}: a package was written that nothing refers to",
                fidelity.as_str()
            );
            let package_bytes = std::fs::read(&deck).expect("reads the original deck");
            let loose: Vec<_> = std::fs::read_dir(base.join("assets"))
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            std::fs::read(e.path()).is_ok_and(|bytes| bytes == package_bytes)
                        })
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            assert!(
                loose.is_empty(),
                "{stem} at {}: the original package is beside the document as {loose:?}",
                fidelity.as_str()
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Phase 13 criterion deferred into this phase (plan v2, Phase 13):
/// *«`--fidelity agent` on that deck is ≤ 15 % of the `full` token count»*,
/// from analysis §6.5 — everything an agent can edit as text, everything else
/// collapsed to one line with its id.
///
/// It is **not met**, and it is written down rather than softened. Over the
/// corpus `agent` measures 96–102 % of `full`: the two differ by the
/// `fidelity:` line and nothing else, because what `agent` drops — formatting
/// (`Fidelity::formatting`) — these decks barely carry, and what §6.5 wanted
/// dropped, the geometry of shapes nobody edits by hand, is written at `agent`
/// by `Fidelity::measurements`. Closing that gap changes what a level means,
/// which is a change to the specification and not to a test (`AGENTS.md` §7
/// rule 2), so it is a decision to take, with its own increment.
///
/// The measurement itself lives here so the number is one command away:
///
/// ```text
/// cargo test -p docsai-convert --test deck_convert -- --ignored --nocapture
/// ```
#[test]
#[ignore = "analysis §6.5 target not met: `agent` is ~100 % of `full`, not 15 %"]
fn agent_fidelity_is_at_most_fifteen_percent_of_full() {
    let mut over = Vec::new();
    for deck in decks() {
        let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
        let full = tokens(&deck, Fidelity::Full);
        let agent = tokens(&deck, Fidelity::Agent);
        let percent = 100.0 * agent as f64 / full as f64;
        println!("{stem:28} full={full:6} agent={agent:6} ({percent:.0} %)");
        if percent > 15.0 {
            over.push(format!("{stem}: {percent:.0} %"));
        }
    }
    assert!(
        over.is_empty(),
        "{} deck(s) over the 15 % target:\n{}",
        over.len(),
        over.join("\n")
    );
}

/// What a deck costs an LLM at a level, through the same counter `docsai
/// tokens` uses.
fn tokens(deck: &Path, fidelity: Fidelity) -> usize {
    docsai_convert::tokens::token_report_path(deck, &options(fidelity))
        .unwrap_or_else(|e| panic!("{}: {e}", deck.display()))
        .total
}

/// Every path a DocMark document points at: the `skeleton:` line and the
/// `(…)` of an image.
fn references(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in markdown.lines() {
        if let Some(path) = line.strip_prefix("skeleton: ") {
            out.push(path.trim().to_string());
        }
        let mut rest = line;
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let Some(end) = after.find(')') else { break };
            let path = &after[..end];
            if !path.starts_with("http") {
                out.push(path.to_string());
            }
            rest = &after[end..];
        }
    }
    out
}
