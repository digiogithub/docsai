//! What keeps the contextual cheat-sheet honest as the specification grows.
//!
//! The sheet is a projection of `docs/docmark-specification.md`, and a
//! projection maintained by hand rots. Two tests here stop it:
//!
//! * [`the_specification_is_fully_accounted_for`] reads the specification's
//!   own headings and requires every one of them to be either claimed by a
//!   [`Feature`] or excluded with a written reason. A new section — a DocMark
//!   1.4, a new block type — is red until somebody says which it is.
//! * [`every_feature_is_reachable`] runs the detector over the whole corpus
//!   and requires every feature to be produced by a real document, or by a
//!   named IR fixture for the three the corpus does not yet carry. An entry
//!   nothing can trigger is an entry every reader pays for and no reader uses.

use std::collections::BTreeSet;

use docsai_convert::cheatsheet::{cheatsheet_input, features, Feature, FEATURES};
use docsai_convert::{ConvertOptions, SourceInput};
use docsai_model::presentation::{
    Layout, LayoutCatalog, LayoutId, LayoutPlaceholder, PhType, Placeholder, Presentation, Shape,
    ShapeKind, Slide,
};
use docsai_model::text::{Block, Paragraph, Section, TextBox, TextDocument};
use docsai_model::Document;

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// How a specification heading is accounted for.
enum Covered {
    /// The cheat-sheet documents it, through these features.
    By(&'static [Feature]),
    /// It is deliberately not syntax the sheet projects, and this is why.
    Not(&'static str),
}

use Covered::{By, Not};

/// Every heading of `docs/docmark-specification.md`, and what answers for it.
///
/// Adding a heading to the specification without adding a row here fails
/// [`the_specification_is_fully_accounted_for`]. That is the point: the
/// alternative is a list nobody updates and a construct an agent is never
/// told about.
const SPEC_MAP: &[(&str, Covered)] = &[
    ("0. Principles", Not("prose: no syntax")),
    (
        "1. File conventions",
        Not("file naming, not syntax in the document"),
    ),
    ("2. Front matter (YAML)", By(&[Feature::FrontMatter])),
    (
        "2.1 Partial documents (selections)",
        Not("`read_selection` writes those keys; an agent does not hand-write a selection"),
    ),
    ("3. Text blocks", Not("umbrella heading over §3.1–§3.7")),
    (
        "3.1 Headings and paragraphs",
        By(&[
            Feature::Heading,
            Feature::ParagraphAttributes,
            Feature::EmptyParagraph,
        ]),
    ),
    (
        "3.2 Inline formatting",
        By(&[
            Feature::Emphasis,
            Feature::SpanAttributes,
            Feature::Link,
            Feature::Footnote,
            Feature::Field,
            Feature::Break,
        ]),
    ),
    ("3.3 Lists", By(&[Feature::List])),
    (
        "3.4 Tables (text documents)",
        By(&[Feature::Table, Feature::ComplexTable]),
    ),
    ("3.5 Images and objects", By(&[Feature::Image])),
    (
        "3.6 Sections, headers and footers, text boxes",
        By(&[Feature::Section, Feature::HeaderFooter, Feature::TextBox]),
    ),
    (
        "3.7 The attribute-set dictionary",
        By(&[Feature::AttributeSets]),
    ),
    ("4. Spreadsheets", By(&[Feature::Sheet, Feature::CellMeta])),
    ("4.1 Images in spreadsheets", By(&[Feature::SheetImages])),
    (
        "5. Configurable style mapping (\"publication\" mode)",
        Not("a one-way CLI mode; its output is not DocMark an agent writes back"),
    ),
    (
        "6. Fidelity levels (`--fidelity`)",
        Not("the sheet states its own level in its header instead"),
    ),
    (
        "6.1 `agent` is a projection, not a conversion",
        Not("same: a level, not a construct"),
    ),
    (
        "7. Fidelity escape hatch: raw-blocks",
        By(&[Feature::RawBlock]),
    ),
    (
        "8. Escape rules and serializer determinism",
        Not("what the serializer guarantees; a reader needs no syntax for it"),
    ),
    (
        "9. Pandoc compatibility",
        Not("relates DocMark to another tool"),
    ),
    ("10. v1.0 change log", Not("history")),
    (
        "11. Committed future versions (1.1, 1.2 and 1.3)",
        Not("umbrella heading over §11.1–§11.3"),
    ),
    (
        "11.1 DocMark 1.1 — addressing, sidecar, `agent` fidelity",
        By(&[Feature::FrontMatter]),
    ),
    ("Node ids (normative)", By(&[Feature::FrontMatter])),
    (
        "Implemented in Phase 11",
        Not("an implementation status list, not syntax"),
    ),
    (
        "The rest of 1.1 (not yet implemented)",
        Not("not implemented: documenting it would describe syntax nothing writes"),
    ),
    (
        "11.2 DocMark 1.2 — the presentation profile (DocMark-P)",
        By(&[Feature::Deck]),
    ),
    ("The eight rules", By(&[Feature::Deck])),
    ("Slide attributes", By(&[Feature::Deck])),
    (
        "Shape container attributes",
        By(&[Feature::ShapeContainer, Feature::Placeholder]),
    ),
    (
        "Pictures, tables, groups and stubs",
        By(&[Feature::Group, Feature::Chart, Feature::Stub]),
    ),
    ("A chart's data table", By(&[Feature::ChartData])),
    ("Speaker notes", By(&[Feature::Notes])),
    ("Front matter keys added by 1.2", By(&[Feature::Deck])),
    (
        "Reading a deck back",
        Not("parser tolerance: what a hand-written deck may leave out, never what to write"),
    ),
    (
        "Version rule and compatibility",
        Not("which version a document declares; the sheet writes it into the front-matter entry"),
    ),
    (
        "Not yet implemented",
        Not("not implemented: see `The rest of 1.1`"),
    ),
    ("11.3 DocMark 1.3 — comments", By(&[Feature::Comment])),
    ("The form", By(&[Feature::Comment])),
    ("The six rules", By(&[Feature::Comment])),
    ("Comment attributes", By(&[Feature::Comment])),
    ("Where the container is written", By(&[Feature::Comment])),
    (
        "Reading a document with comments back",
        Not("parser tolerance"),
    ),
    ("Version rule", Not("see `Version rule and compatibility`")),
    (
        "Writing a comment back",
        Not("what the writer does, not what to write"),
    ),
];

/// The headings of a Markdown file, ignoring anything inside a fenced block.
///
/// The specification's examples are DocMark, so they are full of `#` lines;
/// scanning without tracking fences would read `## Q3 results {.slide}` as a
/// section of the specification.
fn headings(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fence: Option<usize> = None;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        let ticks = trimmed.chars().take_while(|c| *c == '`').count();
        if ticks >= 3 {
            match fence {
                // A closing fence is at least as long as the one it closes.
                Some(open) if ticks >= open => fence = None,
                Some(_) => {}
                None => fence = Some(ticks),
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if (2..=4).contains(&hashes) && line.chars().nth(hashes) == Some(' ') {
            out.push(line[hashes + 1..].trim().to_string());
        }
    }
    out
}

#[test]
fn the_specification_is_fully_accounted_for() {
    let spec = std::fs::read_to_string(repo_root().join("docs/docmark-specification.md"))
        .expect("the specification is in the repository");
    let headings = headings(&spec);
    assert!(
        headings.len() > 30,
        "the heading scanner found only {} headings, which is not this document",
        headings.len()
    );

    let mapped: Vec<&str> = SPEC_MAP.iter().map(|(name, _)| *name).collect();
    for heading in &headings {
        assert!(
            mapped.contains(&heading.as_str()),
            "the specification has a section the cheat-sheet has not decided about: \
             `{heading}`. Add a row to SPEC_MAP in this file — either the Feature that \
             documents it, or the reason it is not syntax an agent writes."
        );
    }
    for (name, covered) in SPEC_MAP {
        match covered {
            By(features) => assert!(!features.is_empty(), "`{name}` claims no feature"),
            Not(reason) => assert!(!reason.is_empty(), "`{name}` is excluded without a reason"),
        }
    }
    for name in &mapped {
        assert!(
            headings.iter().any(|h| h == name),
            "SPEC_MAP claims a section `{name}` the specification no longer has"
        );
    }

    // The other direction: nothing in the sheet documents a section that is
    // not in the specification, and every feature answers for at least one.
    for &feature in FEATURES {
        assert!(
            SPEC_MAP
                .iter()
                .any(|(_, covered)| matches!(covered, By(fs) if fs.contains(&feature))),
            "{feature:?} documents no section of the specification"
        );
    }
}

/// Every document of the corpus a reader in this build can open.
fn corpus_documents() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let corpus = repo_root().join("corpus");
    let mut dirs: Vec<_> = std::fs::read_dir(&corpus)
        .expect("corpus")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .expect("corpus subdirectory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        files.sort();
        for file in files {
            let extension = file.extension().and_then(|e| e.to_str()).unwrap_or("");
            // `.doc` needs LibreOffice, which is not a build dependency.
            if matches!(extension, "docx" | "xlsx" | "odt" | "ods" | "pptx") {
                out.push(file);
            }
        }
    }
    out
}

/// The three features the corpus does not carry, built as IR instead.
///
/// They are real DocMark constructs the readers can produce — a `.docx` text
/// box, a multi-section document, a grouped shape — so documenting them is
/// not speculative; the corpus simply has no file for them yet. Building the
/// IR by hand is what keeps them from being entries nothing exercises.
fn fixtures() -> Vec<(&'static str, Document)> {
    let mut text = TextDocument::default();
    let mut first = Section::default();
    first.blocks.push(Block::TextBox(TextBox {
        blocks: vec![Block::Paragraph(Paragraph::text("Boxed."))],
        size: None,
        x: None,
        y: None,
    }));
    let mut second = Section::default();
    second
        .blocks
        .push(Block::Paragraph(Paragraph::text("Second section.")));
    text.sections.push(first);
    text.sections.push(second);

    let mut deck = Presentation::default();
    let layout_id = LayoutId("L1".into());
    deck.layouts = LayoutCatalog::default();
    deck.layouts.layouts.insert(
        layout_id.clone(),
        Layout {
            placeholders: vec![LayoutPlaceholder {
                ph_type: PhType::Title,
                idx: None,
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    deck.slides.push(Slide {
        layout: Some(layout_id),
        shapes: vec![Shape::new(
            0,
            ShapeKind::Group(vec![Shape::new(
                0,
                ShapeKind::Placeholder(Placeholder::default()),
            )]),
        )],
        ..Default::default()
    });

    vec![
        ("a text box and two sections", Document::Text(text)),
        ("a grouped shape", Document::Presentation(deck)),
    ]
}

#[test]
fn every_feature_is_reachable() {
    let mut seen: BTreeSet<Feature> = BTreeSet::new();
    let options = ConvertOptions::default();
    for path in corpus_documents() {
        let sheet = cheatsheet_input(SourceInput::Path(&path), &options)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for &feature in FEATURES {
            if sheet.contains(&format!("\n{} §", feature.key())) {
                seen.insert(feature);
            }
        }
    }
    for (_, document) in fixtures() {
        seen.extend(features(&document));
    }

    let unreachable: Vec<Feature> = FEATURES
        .iter()
        .copied()
        .filter(|feature| !seen.contains(feature))
        .collect();
    assert!(
        unreachable.is_empty(),
        "these entries are documented but nothing produces them, so every reader pays \
         for syntax no document uses: {unreachable:?}"
    );
}

#[test]
fn a_richer_document_costs_more_than_a_plainer_one() {
    let options = ConvertOptions::default();
    let plain = cheatsheet_input(
        SourceInput::Path(&repo_root().join("corpus/docx/basic-text.docx")),
        &options,
    )
    .expect("the plain report");
    let deck = cheatsheet_input(
        SourceInput::Path(&repo_root().join("corpus/pptx/charts-multi-series.pptx")),
        &options,
    )
    .expect("the deck");

    assert!(
        !plain.contains("chart") && !plain.contains(".slide") && !plain.contains("cell-meta"),
        "a plain report must not pay for the presentation or spreadsheet profiles:\n{plain}"
    );
    assert!(
        deck.contains("chart data") && deck.contains(".slide"),
        "a deck with charts documents them:\n{deck}"
    );
    assert!(
        docsai_convert::tokens::count(&deck) > docsai_convert::tokens::count(&plain) * 2,
        "the whole point is that the sheet tracks the document: plain {} tokens, deck {}",
        docsai_convert::tokens::count(&plain),
        docsai_convert::tokens::count(&deck)
    );
}
