//! IR goldens for the `.odp` corpus (Phase 18-B).
//!
//! The sibling of `pptx_goldens.rs`, over the seventeen fixtures 18-A built. A
//! deck cannot be pinned by its DocMark yet — that is 18-C — so what is pinned
//! is the inspection report: slides, layouts, shape counts, notes, OLE, media
//! and, crucially, **the read-time warnings**. The warning list is the half of
//! the golden that enforces «never degrade silently, never warn falsely»: a
//! construct that stops warning and a construct that starts warning both move
//! the golden, and both have to be argued in the diff.
//!
//! ```text
//! DOCSAI_UPDATE_GOLDENS=1 cargo test -p docsai-convert --test odp_goldens
//! ```
//!
//! …and the resulting diff has to be reviewed by hand, exactly as for DocMark.

use std::path::{Path, PathBuf};

use docsai_convert::inspect::build_report;
use docsai_model::{Format, MemoryAssetStore};

fn corpus_odp() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/odp")
}

fn decks() -> Vec<PathBuf> {
    let dir = corpus_odp();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("odp"))
        .collect();
    paths.sort();
    paths
}

/// The inspection report as JSON, with nothing in it that depends on where the
/// test ran: no path, and asset ids that are content hashes either way.
fn inventory(path: &Path) -> String {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut assets = MemoryAssetStore::new();
    let (document, report) = docsai_odf::read_odp(file, &mut assets)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if let Err(errors) = docsai_model::validate::validate(&document) {
        panic!("{}: invalid IR: {errors:?}", path.display());
    }
    let report = build_report(None, Format::Odp, &document, &assets, report);
    let mut json = serde_json::to_string_pretty(&report).expect("the report serialises");
    json.push('\n');
    json
}

fn golden_path(deck: &Path) -> PathBuf {
    let stem = deck.file_stem().unwrap().to_string_lossy().into_owned();
    deck.with_file_name(format!("{stem}.expected.inspect.json"))
}

#[test]
fn the_odp_corpus_matches_its_inspection_goldens() {
    let decks = decks();
    assert_eq!(decks.len(), 17, "the 18-A corpus is seventeen fixtures");
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

/// The property the increment turns on, asserted over the whole corpus rather
/// than fixture by fixture: **every raw fragment is reachable from a stub, and
/// every stub points at a fragment that exists**.
///
/// A payload nothing refers to is a silent loss wearing a preserved-markup
/// costume — the bytes are in the model and no consumer will ever write them
/// back — and a stub pointing at nothing is the same loss with a dangling
/// pointer on top.
#[test]
fn every_raw_fragment_is_referred_to_and_every_reference_resolves() {
    use docsai_model::presentation::{Shape, ShapeKind};
    use std::collections::BTreeSet;

    fn referenced(shapes: &[Shape], into: &mut BTreeSet<String>) {
        for shape in shapes {
            match &shape.kind {
                ShapeKind::Raw(raw) => {
                    if let Some(id) = &raw.raw {
                        into.insert(id.as_str().to_string());
                    }
                }
                ShapeKind::Chart(chart) => {
                    if let Some(id) = &chart.raw {
                        into.insert(id.as_str().to_string());
                    }
                }
                ShapeKind::Group(children) => referenced(children, into),
                _ => {}
            }
        }
    }

    for deck in decks() {
        let file = std::fs::File::open(&deck).expect("the fixture opens");
        let mut assets = MemoryAssetStore::new();
        let (document, report) = docsai_odf::read_odp(file, &mut assets).expect("it reads");
        let docsai_model::Document::Presentation(presentation) = &document else {
            panic!("{} is not a presentation", deck.display());
        };

        let mut used: BTreeSet<String> = BTreeSet::new();
        for slide in &presentation.slides {
            for id in &slide.raw {
                used.insert(id.as_str().to_string());
            }
            referenced(&slide.shapes, &mut used);
        }
        let held: BTreeSet<String> = presentation
            .raw
            .iter()
            .map(|fragment| fragment.id.as_str().to_string())
            .collect();
        assert_eq!(
            used,
            held,
            "{}: raw fragments and the stubs that refer to them disagree",
            deck.display()
        );
        // And each one was reported, so the loss of fidelity is in the report
        // rather than in the user's surprise.
        assert_eq!(
            report.raw_blocks_emitted as usize,
            held.len(),
            "{}: {} fragments, {} reported",
            deck.display(),
            held.len(),
            report.raw_blocks_emitted
        );
    }
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
