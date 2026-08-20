//! The risk P4 gate (plan v2 Phase 14-K, `technical-analysis-presentations.md` §7).
//!
//! P4 is *«DocMark-P becomes unreadable»*, and the gate plan v2 sets for it is
//! not a measurement but an act: a reviewer hand-edits three `--fidelity
//! standard` decks — retitle a slide, add a bullet, swap an image — **without
//! consulting the specification**, and what breaks blocks the phase.
//!
//! A test cannot be that reviewer. What it can do is hold the reviewer's
//! edits still so they never quietly stop working: each edit here is written
//! as the literal text substitution a person makes in an editor, applied to
//! the real `standard` output, and then read back. The human pass is recorded
//! in `kb/57-phase-14-p4-gate.md`; this is its regression net.
//!
//! The edits are deliberately naive. A reviewer who has not read the spec
//! retypes a heading without noticing `{.slide}`, adds a bullet by copying the
//! line above it, and swaps an image by changing the file name in the
//! parentheses — so those are the three edits, in that form.

use std::path::{Path, PathBuf};

use docsai_convert::{convert_file, ConvertOptions};
use docsai_docmark::Fidelity;
use docsai_model::presentation::{Presentation, ShapeKind};
use docsai_model::{Document, MemoryAssetStore};

fn deck(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/pptx")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("docsai-p4-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

/// A deck as a reviewer receives it: `standard`, in a directory of its own.
fn readable_deck(name: &str, dir: &Path) -> (PathBuf, String) {
    let output = dir.join("deck.md");
    let outcome = convert_file(
        &deck(name),
        Some(&output),
        &ConvertOptions {
            fidelity: Fidelity::Standard,
            ..ConvertOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"));
    (output, outcome.markdown)
}

/// Reads an edited document back the way its recipient would: the file, and
/// the directory beside it.
fn reread(path: &Path, markdown: &str) -> (Presentation, Vec<String>) {
    let mut assets = MemoryAssetStore::new();
    let (document, report) = docsai_docmark::parse_with_base(markdown, path.parent(), &mut assets)
        .unwrap_or_else(|e| panic!("the edited deck does not parse: {e}\n{markdown}"));
    let Document::Presentation(deck) = document else {
        panic!("the edited deck stopped being a deck:\n{markdown}");
    };
    let warnings = report
        .warnings
        .iter()
        .map(|warning| format!("{warning:?}"))
        .collect();
    (deck, warnings)
}

fn edit(markdown: &str, from: &str, to: &str) -> String {
    assert!(
        markdown.contains(from),
        "the fixture no longer contains `{from}`:\n{markdown}"
    );
    markdown.replacen(from, to, 1)
}

fn bullets(slide: &docsai_model::presentation::Slide) -> usize {
    fn count(blocks: &[docsai_model::text::Block]) -> usize {
        blocks
            .iter()
            .map(|block| match block {
                docsai_model::text::Block::List(list) => list.items.len(),
                _ => 0,
            })
            .sum()
    }
    slide
        .shapes
        .iter()
        .map(|shape| match &shape.kind {
            ShapeKind::Placeholder(ph) => count(&ph.body),
            ShapeKind::TextBox { body } => count(body),
            _ => 0,
        })
        .sum()
}

/// Edit one: retitle a slide — and retype the heading without the `{.slide}`
/// the reviewer never read about.
///
/// This is the edit that decides P4. If a heading has to carry a marker to
/// stay a slide, then the format is not hand-editable and the answer to
/// «what do I have to know before typing» is «the specification».
#[test]
fn a_reviewer_retitles_a_slide() {
    let dir = scratch("retitle");
    let (path, markdown) = readable_deck("notes-speaker.pptx", &dir);

    let edited = edit(&markdown, "## Resultados {.slide}", "## Resultados 2026");
    let (deck, warnings) = reread(&path, &edited);

    assert_eq!(deck.slides.len(), 2, "a slide was lost or gained");
    assert_eq!(deck.slides[0].title().as_deref(), Some("Resultados 2026"));
    assert_eq!(
        deck.slides[1].title().as_deref(),
        Some("Riesgos"),
        "the untouched slide changed"
    );
    // The notes of the retitled slide are a blockquote under it: dropping the
    // marker must not take them with it.
    assert!(
        deck.slides[0].notes.is_some(),
        "the speaker notes did not survive the retitle"
    );
    assert!(
        warnings.is_empty(),
        "the edit was reported as a loss: {warnings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Edit two: add a bullet, by copying the line above it.
#[test]
fn a_reviewer_adds_a_bullet() {
    let dir = scratch("bullet");
    let (path, markdown) = readable_deck("bullets-levels.pptx", &dir);

    let (before, _) = reread(&path, &markdown);
    let first = markdown
        .lines()
        .find(|line| line.starts_with("- "))
        .expect("the fixture has a bullet")
        .to_string();
    let edited = edit(
        &markdown,
        &first,
        &format!("{first}\n- Punto añadido a mano"),
    );
    let (after, warnings) = reread(&path, &edited);

    assert_eq!(after.slides.len(), before.slides.len());
    assert_eq!(
        bullets(&after.slides[0]),
        bullets(&before.slides[0]) + 1,
        "the new bullet did not arrive as a bullet"
    );
    assert!(
        warnings.is_empty(),
        "the edit was reported as a loss: {warnings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Edit three: swap an image, by changing the file name in the parentheses.
#[test]
fn a_reviewer_swaps_an_image() {
    let dir = scratch("image");
    let (path, markdown) = readable_deck("images-anchored.pptx", &dir);

    // The replacement is a file the reviewer dropped into `assets/` — here,
    // any second image will do, so it is the first one with a byte changed:
    // a different content hash is what makes the swap observable.
    let assets = dir.join("assets");
    let original = std::fs::read_dir(&assets)
        .expect("the deck brought its image")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .find(|p| p.extension().and_then(|e| e.to_str()) == Some("png"))
        .expect("a png beside the deck");
    let mut bytes = std::fs::read(&original).expect("reads the image");
    bytes.extend_from_slice(b"\n<!-- a different picture -->");
    std::fs::write(assets.join("nuevo.png"), &bytes).expect("writes the replacement");

    let old_name = original.file_name().unwrap().to_string_lossy().into_owned();
    let edited = edit(&markdown, &old_name, "nuevo.png");
    let (deck, warnings) = reread(&path, &edited);

    let picture = deck.slides[0]
        .shapes
        .iter()
        .find_map(|shape| match &shape.kind {
            ShapeKind::Picture(picture) => Some(picture),
            _ => None,
        })
        .expect("the slide still holds a picture");
    // The asset is the file's content hash, so «did the swap take» is a
    // question with an exact answer: the picture holds the new bytes.
    assert_eq!(
        picture.asset.as_str(),
        docsai_model::assets::content_hash(&bytes),
        "the picture is not the replacement file"
    );
    assert!(
        !markdown.contains("nuevo.png"),
        "the fixture already referenced the replacement"
    );
    assert!(
        warnings.is_empty(),
        "the edit was reported as a loss: {warnings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The gate's finding: a reviewer adds a note to a slide that already has one.
///
/// `standard` writes the notes as a blockquote under the slide, so «add a
/// note» looks like «type another blockquote». Before 14-K the second one
/// replaced the first, and the text a reader could see on screen was gone
/// without a word. A slide has one notes page: two blockquotes are two
/// paragraphs of it.
#[test]
fn a_reviewer_adds_a_note_where_one_already_exists() {
    let dir = scratch("note");
    let (path, markdown) = readable_deck("notes-speaker.pptx", &dir);

    let edited = edit(
        &markdown,
        "- Dependencia de un proveedor",
        "- Dependencia de un proveedor\n\n> Nota nueva escrita a mano.",
    );
    let (deck, warnings) = reread(&path, &edited);

    let notes = deck.slides[1]
        .notes
        .as_ref()
        .expect("the slide keeps a notes page");
    let text = format!("{notes:?}");
    assert!(
        text.contains("Nota nueva escrita a mano"),
        "the note the reviewer wrote is not there"
    );
    assert!(
        text.contains("remitir al anexo"),
        "the note that was already there was replaced"
    );
    assert!(
        warnings.is_empty(),
        "the edit was reported as a loss: {warnings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// What the gate is really about: the reviewer's file is still a deck, and it
/// is still the deck they were given.
///
/// A format survives hand editing when an edit changes what it touches and
/// nothing else. Here the three edits are applied to one document at once.
#[test]
fn the_three_edits_together_change_only_what_they_touch() {
    let dir = scratch("together");
    let (path, markdown) = readable_deck("notes-speaker.pptx", &dir);
    let (before, _) = reread(&path, &markdown);

    let edited = edit(&markdown, "## Resultados {.slide}", "## Resultados 2026");
    let edited = edit(
        &edited,
        "- Crecimiento del 12 %",
        "- Crecimiento del 12 %\n- Y del 3 % en el último trimestre",
    );
    let (after, warnings) = reread(&path, &edited);

    assert_eq!(after.slides.len(), before.slides.len());
    assert_eq!(after.slides[0].title().as_deref(), Some("Resultados 2026"));
    assert_eq!(bullets(&after.slides[0]), bullets(&before.slides[0]) + 1);
    assert_eq!(
        after.slides[1].title(),
        before.slides[1].title(),
        "the slide nobody touched changed"
    );
    assert_eq!(
        bullets(&after.slides[1]),
        bullets(&before.slides[1]),
        "the slide nobody touched lost or gained a bullet"
    );
    assert_eq!(
        after.slides[1].notes.is_some(),
        before.slides[1].notes.is_some(),
        "the notes of the slide nobody touched moved"
    );
    assert!(
        warnings.is_empty(),
        "the edits were reported as a loss: {warnings:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
