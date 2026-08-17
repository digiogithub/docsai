//! The parser against the corpus (plan v2 Phase 14-H).
//!
//! `presentation_parse.rs` pins the parser's rules on decks built by hand, one
//! construct at a time. This reads the seventeen real decks of `corpus/pptx`
//! through the pptx reader, serialises them, and parses the result back — the
//! only version of «the parser reads what the writer writes» that covers what
//! PowerPoint actually produces.
//!
//! What it does *not* check is byte idempotence over the corpus, which needs
//! the goldens of 14-I. Here the question is coarser and comes first: does
//! every deck come back, with its slides, its shapes and its notes.

use std::path::{Path, PathBuf};

use docsai_docmark::{parse, serialize, Fidelity, Options};
use docsai_model::addressing::IdPolicy;
use docsai_model::presentation::{Presentation, ShapeKind};
use docsai_model::{Document, Format, MemoryAssetStore};

fn decks() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/pptx");
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

fn name(deck: &Path) -> String {
    deck.file_stem().unwrap().to_string_lossy().into_owned()
}

fn options(fidelity: Fidelity) -> Options {
    Options {
        fidelity,
        ids: match fidelity {
            // The two levels that write back address every node; the other two
            // write none, which is what `ConvertOptions` builds (spec §11.1).
            Fidelity::Full | Fidelity::Agent => IdPolicy::Assign,
            _ => IdPolicy::Never,
        },
        source_format: Format::Pptx,
        ..Options::default()
    }
}

/// A deck as the pptx reader gives it, and its DocMark at `fidelity`.
fn read_deck(path: &Path, fidelity: Fidelity) -> (Presentation, MemoryAssetStore, String) {
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut assets = MemoryAssetStore::new();
    let (document, _) = docsai_office::read_pptx(file, &mut assets)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (markdown, _) = serialize(&document, &assets, &options(fidelity));
    let Document::Presentation(deck) = document else {
        panic!("{} is not a deck", path.display());
    };
    (deck, assets, markdown)
}

fn parse_deck(markdown: &str, assets: &mut MemoryAssetStore, deck: &str) -> Presentation {
    match parse(markdown, assets) {
        Ok((Document::Presentation(parsed), _)) => parsed,
        Ok(_) => panic!("{deck} came back as something that is not a deck"),
        Err(error) => panic!("{deck}: {error}\n{markdown}"),
    }
}

/// Shapes, groups flattened: what the slide holds, however it is boxed.
fn shape_count(deck: &Presentation) -> usize {
    fn count(kinds: &[docsai_model::presentation::Shape]) -> usize {
        kinds
            .iter()
            .map(|shape| match &shape.kind {
                ShapeKind::Group(children) => 1 + count(children),
                _ => 1,
            })
            .sum()
    }
    deck.slides.iter().map(|slide| count(&slide.shapes)).sum()
}

#[test]
fn every_deck_of_the_corpus_parses_back_at_full() {
    for path in decks() {
        let deck = name(&path);
        let (original, mut assets, markdown) = read_deck(&path, Fidelity::Full);
        let parsed = parse_deck(&markdown, &mut assets, &deck);

        assert_eq!(
            parsed.slides.len(),
            original.slides.len(),
            "{deck}: slides lost"
        );
        // `full` is the round-trip level: every shape the writer wrote is a
        // shape the parser read. Furniture is written here too, so the count
        // is the whole tree and not a subset of it.
        assert_eq!(
            shape_count(&parsed),
            shape_count(&original),
            "{deck}: shapes lost\n{markdown}"
        );
        for (index, (before, after)) in original.slides.iter().zip(&parsed.slides).enumerate() {
            // The ids are allocated by the write, not by the reader, so what
            // there is to check is that every one of them survived the parse.
            assert!(
                after.id.is_some(),
                "{deck} slide {}: the address is gone",
                index + 1
            );
            assert_eq!(
                after.layout,
                before.layout,
                "{deck} slide {}: layout changed",
                index + 1
            );
            assert_eq!(
                after.notes.is_some(),
                before.notes.is_some(),
                "{deck} slide {}: the notes page appeared or vanished",
                index + 1
            );
            assert_eq!(
                after.title(),
                before.title(),
                "{deck} slide {}: title changed",
                index + 1
            );
        }
    }
}

#[test]
fn every_deck_of_the_corpus_parses_back_at_standard() {
    for path in decks() {
        let deck = name(&path);
        let (original, mut assets, markdown) = read_deck(&path, Fidelity::Standard);
        let parsed = parse_deck(&markdown, &mut assets, &deck);

        assert_eq!(
            parsed.slides.len(),
            original.slides.len(),
            "{deck}: slides lost"
        );
        // The level that writes no id, no geometry and a blockquote for its
        // notes still says what every slide is: `standard` is hand-editable,
        // and a hand-editable document that does not parse is a dead end.
        for (index, (before, after)) in original.slides.iter().zip(&parsed.slides).enumerate() {
            assert_eq!(
                after.title(),
                before.title(),
                "{deck} slide {}: title changed",
                index + 1
            );
            assert!(
                after.id.is_none() && after.layout.is_none(),
                "{deck} slide {}: `standard` writes no address and no layout",
                index + 1
            );
        }
    }
}

#[test]
fn a_deck_at_agent_keeps_every_address_it_was_written_with() {
    for path in decks() {
        let deck = name(&path);
        let (original, mut assets, markdown) = read_deck(&path, Fidelity::Agent);
        let parsed = parse_deck(&markdown, &mut assets, &deck);

        // `agent` output is written back node by node, so an address that does
        // not survive the parser is a node an agent cannot edit — and two
        // nodes sharing one address is worse than none at all.
        let ids: Vec<_> = parsed
            .slides
            .iter()
            .map(|slide| {
                slide
                    .id
                    .clone()
                    .unwrap_or_else(|| panic!("{deck}: a slide came back with no address"))
            })
            .collect();
        assert_eq!(ids.len(), original.slides.len(), "{deck}: slides lost");
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            ids.len(),
            "{deck}: two slides share an address"
        );
    }
}

#[test]
fn every_shape_comes_back_as_the_kind_it_was_written_as() {
    // The count alone would pass with every shape read as a text box. What a
    // shape *is* survives too: a placeholder with its type, a picture, a
    // table, a chart and the stubs of rule 8 with theirs.
    for path in decks() {
        let deck = name(&path);
        let (original, mut assets, markdown) = read_deck(&path, Fidelity::Full);
        let parsed = parse_deck(&markdown, &mut assets, &deck);

        assert_eq!(
            kinds(&parsed),
            kinds(&original),
            "{deck}: a shape changed kind\n{markdown}"
        );
    }
}

/// What every shape of a deck is, in reading order, groups descended into.
fn kinds(deck: &Presentation) -> Vec<String> {
    fn walk(shapes: &[docsai_model::presentation::Shape], out: &mut Vec<String>) {
        for shape in shapes {
            match &shape.kind {
                ShapeKind::Placeholder(ph) => out.push(format!("placeholder {}", ph.ph_type)),
                ShapeKind::TextBox { .. } => out.push("textbox".into()),
                ShapeKind::Picture(_) => out.push("picture".into()),
                ShapeKind::Table(_) => out.push("table".into()),
                ShapeKind::Chart(_) => out.push("chart".into()),
                ShapeKind::Raw(raw) => out.push(format!("raw {}", raw.kind)),
                ShapeKind::Group(children) => {
                    out.push("group".into());
                    walk(children, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    for slide in &deck.slides {
        walk(&slide.shapes, &mut out);
    }
    out
}
