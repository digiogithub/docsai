//! IR goldens for the presentation corpus (plan v2 Phase 13 acceptance).
//!
//! The other corpora are pinned by their DocMark (`goldens.rs`). A deck cannot
//! be: DocMark-P is Phase 14. What a deck *does* have is the inspection report —
//! slides, layouts, shape counts, notes, SmartArt/OLE, media and the read-time
//! warnings — which is the IR seen from outside, and it is pinned here as
//! `<name>.expected.inspect.json` beside each deck.
//!
//! ```text
//! DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test pptx_goldens
//! ```
//!
//! …and the resulting diff has to be reviewed by hand, exactly as for DocMark.

use std::path::{Path, PathBuf};

use docsai_convert::inspect::build_report;
use docsai_model::{Format, MemoryAssetStore};

fn corpus_pptx() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx")
}

fn decks() -> Vec<PathBuf> {
    let dir = corpus_pptx();
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

/// The inspection report as JSON, with nothing in it that depends on where the
/// test ran: no path, and asset ids that are content hashes either way.
fn inventory(path: &Path) -> String {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut assets = MemoryAssetStore::new();
    let (document, report) = docsai_office::read_pptx(file, &mut assets)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if let Err(errors) = docsai_model::validate::validate(&document) {
        panic!("{}: invalid IR: {errors:?}", path.display());
    }
    let report = build_report(None, Format::Pptx, &document, &assets, report);
    let mut json = serde_json::to_string_pretty(&report).expect("the report serialises");
    json.push('\n');
    json
}

fn golden_path(deck: &Path) -> PathBuf {
    let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
    deck.with_file_name(format!("{stem}.expected.inspect.json"))
}

#[test]
fn the_pptx_corpus_matches_its_inspection_goldens() {
    let decks = decks();
    assert!(decks.len() >= 14, "only {} decks found", decks.len());
    let updating = std::env::var_os("DOCSAI_UPDATE_GOLDENS").is_some();
    let mut mismatches = Vec::new();

    for deck in &decks {
        let actual = inventory(deck);
        let golden = golden_path(deck);
        if updating {
            std::fs::write(&golden, &actual).expect("writes the golden");
            continue;
        }
        match std::fs::read_to_string(&golden) {
            Ok(expected) if expected == actual => {}
            Ok(expected) => mismatches.push(format!(
                "{}:\n{}",
                golden.display(),
                first_difference(&expected, &actual)
            )),
            Err(_) => mismatches.push(format!("{}: golden missing", golden.display())),
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} inspection golden(s) differ; review the diff, then regenerate with \
         DOCSAI_UPDATE_GOLDENS=1\n\n{}",
        mismatches.len(),
        mismatches.join("\n\n")
    );
}

#[test]
fn reading_a_deck_is_deterministic() {
    for deck in decks() {
        assert_eq!(inventory(&deck), inventory(&deck), "{}", deck.display());
    }
}

/// Plan v2 Phase 13 acceptance: «a real 40-slide deck reads in < 1 s».
///
/// The deck is `forty-slides.pptx`, synthesised by `corpus/generate.py` — no
/// real-world deck can live in the repository. It is the right *shape* (forty
/// slides, each with a title, a body and speaker notes, all resolving against
/// the same layout cascade) and the wrong *weight*: a real deck of that size
/// carries images and embedded objects this one does not. The budget is
/// deliberately generous, because the number that matters is the order of
/// magnitude — a reader that went quadratic in the slide count would miss it by
/// far more than the margin.
#[test]
fn a_forty_slide_deck_reads_well_inside_a_second() {
    let deck = corpus_pptx().join("forty-slides.pptx");
    let bytes = std::fs::read(&deck).unwrap_or_else(|e| panic!("{}: {e}", deck.display()));
    let start = std::time::Instant::now();
    let mut assets = MemoryAssetStore::new();
    let (document, _) =
        docsai_office::read_pptx(std::io::Cursor::new(bytes), &mut assets).expect("the deck reads");
    let elapsed = start.elapsed();
    let docsai_model::Document::Presentation(presentation) = &document else {
        panic!("a deck is a presentation");
    };
    assert_eq!(presentation.slides.len(), 40);
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "reading forty slides took {elapsed:?}"
    );
}

fn first_difference(expected: &str, actual: &str) -> String {
    for (index, (want, got)) in expected.lines().zip(actual.lines()).enumerate() {
        if want != got {
            return format!("  line {}\n  - {want}\n  + {got}", index + 1);
        }
    }
    format!(
        "  line count differs: expected {}, got {}",
        expected.lines().count(),
        actual.lines().count()
    )
}
