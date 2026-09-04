//! The cross conversion, trait by trait (Phase 18, increment 18-E).
//!
//! Phase 18's second acceptance criterion:
//!
//! > **pptx → DocMark → odp and back work for common traits; drops are warned,
//! > never silent.**
//!
//! `corpus/pptx` and `corpus/odp` were built to mirror each other trait by
//! trait ([[96-phase-18-corpus]]), so the matrix below is that pairing written
//! down: sixteen traits both corpora carry, five `.pptx` traits ODF has no
//! counterpart for, and one ODF trait PresentationML has none for. Every deck
//! of both corpora appears in exactly one cell and the walk asserts it, so a
//! fixture added tomorrow fails the build rather than quietly not being
//! measured — the shape `edit_matrix.rs` gave §11's matrix in Phase 17.
//!
//! ## What "work for common traits" is asserted to mean
//!
//! Not byte identity: the two dialects are different packages and a cross
//! conversion cannot have it. Not a render diff either — the two decks resolve
//! through different masters and different default fonts, so a pixel
//! comparison would be measuring the template. What is asserted is the strongest
//! thing that is actually true, and it is stated per cell rather than in
//! general:
//!
//! 1. **The visible text survives, as a multiset.** Every line of text the
//!    source deck's DocMark shows is in the crossed deck's DocMark, with the
//!    same multiplicity, **except** the lines the cell declares as lost — and
//!    every line the crossing does lose is one the cell declares. It is a
//!    multiset and not a sequence on purpose: the two readers order a page's
//!    shapes by different rules (PresentationML ranks placeholders by `p:ph`
//!    and ODF has no index to rank by, so 18-B orders by position), and a
//!    reading order that changes is a fact about the dialect, not a loss.
//! 2. **The warnings cover the losses exactly, in both directions.** Every
//!    warning the conversion raises matches one of the fragments the cell
//!    declares, and every fragment the cell declares matches at least one
//!    warning. A drop with no warning fails; a warning for a loss that did not
//!    happen fails just as hard (`AGENTS.md` §7 rule 3).
//! 3. **The package is real**, non-empty and readable back by this project's
//!    own reader — which is what produces the DocMark rule 1 compares.
//!
//! That the crossed package is *schema-valid* and that *LibreOffice opens it*
//! are asserted too, and not here: `corpus/schema_gate.py` and
//! `corpus/render_gate.py` each grew a cross arm in this increment, because
//! neither a grammar nor a renderer belongs in a unit test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use docsai_convert::{convert_file, ConvertOptions};
use docsai_docmark::Fidelity;

/// What a crossing does to a trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// Nothing is lost and nothing is warned beyond the design substitution
    /// every cross conversion takes.
    Intact,
    /// Every line of text survives, and something else does not: a placeholder
    /// index, a chart's colours, a role one dialect names and the other does
    /// not. Named in a warning.
    Warned,
    /// A construct cannot be written in the target dialect at all, and its text
    /// goes with it. Named in a warning.
    Dropped,
}

/// One direction of one cell.
struct Cross {
    verdict: Verdict,
    /// The lines of text this crossing loses, **verbatim**. Each must be in
    /// the source deck and gone from the crossed one, and every line the
    /// crossing loses must be one of them. Whole lines rather than fragments,
    /// because a fragment matches lines nobody meant: `Ventas` is a chart's
    /// column header and also half of the slide title above it.
    lost: &'static [&'static str],
    /// Warning fragments. Every warning must contain one; each must be in at
    /// least one warning.
    warnings: &'static [&'static str],
}

/// One trait, and what each dialect calls it.
struct Cell {
    /// The trait as `corpus/README.md` names it.
    trait_name: &'static str,
    /// The `corpus/pptx` fixture, or `None` when ODF has the trait and
    /// PresentationML's corpus does not.
    pptx: Option<&'static str>,
    /// The `corpus/odp` fixture, or `None` when the trait is one ODF does not
    /// have — each of the five is a fact about the format, and
    /// `corpus/README.md` gives the reason for each.
    odp: Option<&'static str>,
    to_odp: Option<Cross>,
    to_pptx: Option<Cross>,
}

/// The warning every cross conversion raises, in both directions: the deck's
/// design was substituted, because a package of the other dialect cannot be
/// spliced into and the writer fell back on its embedded template.
const DESIGN: &str = "the deck's design";

/// PresentationML numbers a placeholder and ODF does not, so an `idx` above 0
/// cannot be written. Fires on the way to `.odp` only.
const INDEX: &str = "the index of a `body` placeholder";

/// The `.pptx` generator's layout chooser, which says so when a slide's shape
/// matches neither of its two rules. Fires on the way to `.pptx` only.
const LAYOUT: &str = "matches neither of the rules that choose a layout";

/// A shape whose rectangle falls off the page it was written onto, because
/// DocMark-P carries no canvas size and the two corpora's canvases differ (the
/// `.pptx` set is 4:3, the embedded ODF template 16:9). Fires on the way to
/// `.odp` only.
const PLACEMENT: &str = "the placement of a shape on page";

const fn intact() -> Cross {
    Cross {
        verdict: Verdict::Intact,
        lost: &[],
        warnings: &[DESIGN],
    }
}

/// The matrix. Twenty-two cells: sixteen traits both corpora carry, five with
/// no `.odp` and one with no `.pptx`.
const MATRIX: &[Cell] = &[
    Cell {
        trait_name: "two slides, title and body",
        pptx: Some("basic-slides.pptx"),
        odp: Some("basic-slides.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: Some(intact()),
    },
    Cell {
        trait_name: "slide order against the part names",
        pptx: Some("slide-order.pptx"),
        odp: Some("slide-order.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: Some(intact()),
    },
    Cell {
        trait_name: "a placeholder inheriting everything",
        pptx: Some("placeholders-cascade.pptx"),
        odp: Some("placeholders-cascade.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: Some(intact()),
    },
    Cell {
        trait_name: "an empty placeholder is content",
        pptx: Some("placeholders-empty.pptx"),
        odp: Some("placeholders-empty.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX, PLACEMENT],
        }),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "a `text` placeholder"],
        }),
    },
    Cell {
        trait_name: "nested bullets at two kinds and three levels",
        pptx: Some("bullets-levels.pptx"),
        odp: Some("bullets-levels.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX, PLACEMENT],
        }),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "a `text` placeholder"],
        }),
    },
    Cell {
        trait_name: "speaker notes",
        pptx: Some("notes-speaker.pptx"),
        odp: Some("notes-speaker.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: Some(intact()),
    },
    Cell {
        trait_name: "document order that is not reading order",
        pptx: Some("reading-order.pptx"),
        odp: Some("reading-order.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "a `text` placeholder"],
        }),
    },
    Cell {
        trait_name: "the constructs with no IR node",
        pptx: Some("raw-preserved.pptx"),
        odp: Some("raw-preserved.odp"),
        // A raw fragment is markup of the dialect it was read in; DocMark drops
        // it on the way out with `RawBlockDropped` (18-C), and what reaches the
        // `.odp` writer is a stub with a label and no geometry to put it in.
        to_odp: Some(Cross {
            verdict: Verdict::Dropped,
            lost: &["Dentro del grupoTambién dentro", "Etiqueta del triángulo"],
            warnings: &[DESIGN, "a preserved shape on page"],
        }),
        // The other way the label survives, because the `.pptx` generator
        // writes a text box for a stub it cannot reproduce.
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "a preserved object"],
        }),
    },
    Cell {
        trait_name: "a table on a slide",
        pptx: Some("tables-simple.pptx"),
        odp: Some("tables-simple.odp"),
        to_odp: Some(intact()),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT],
        }),
    },
    Cell {
        trait_name: "a picture with explicit geometry",
        pptx: Some("images-anchored.pptx"),
        odp: Some("images-anchored.odp"),
        to_odp: Some(intact()),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT],
        }),
    },
    Cell {
        trait_name: "free shapes with their own geometry vocabulary",
        pptx: Some("shapes-geometry.pptx"),
        odp: Some("shapes-geometry.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, "a preserved shape on page"],
        }),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "a preserved object"],
        }),
    },
    Cell {
        trait_name: "an embedded chart, one series",
        pptx: Some("charts-embedded.pptx"),
        odp: Some("charts-embedded.odp"),
        // An ODF chart is a whole sub-document with its own numbers. Nothing in
        // this project writes one, so the plot and its cached series go.
        to_odp: Some(Cross {
            verdict: Verdict::Dropped,
            lost: &["Ventas", "Norte", "Sur", "Este", "1200", "980", "1440"],
            warnings: &[DESIGN, "a chart on page"],
        }),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "its chart's design"],
        }),
    },
    Cell {
        trait_name: "an embedded chart, two series and a legend",
        pptx: Some("charts-multi-series.pptx"),
        odp: Some("charts-multi-series.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Dropped,
            lost: &[
                "Ventas por trimestre",
                "Norte",
                "Sur",
                "Total",
                "Q1",
                "Q2",
                "Q3",
                "1200",
                "800",
                "2000",
                "980",
                "1440",
                "910",
                "2350",
            ],
            warnings: &[DESIGN, INDEX, "a chart on page"],
        }),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "its chart's design"],
        }),
    },
    Cell {
        trait_name: "an embedded OLE object",
        pptx: Some("ole-embedded.pptx"),
        odp: Some("ole-embedded.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX, "an embedded object on page"],
        }),
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "a preserved object"],
        }),
    },
    Cell {
        trait_name: "a commented slide",
        pptx: Some("comments-slide.pptx"),
        odp: Some("comments-slide.odp"),
        // No writer in this project writes an ODF annotation, so a comment
        // crossing into `.odp` is a drop. The other way it crosses whole.
        to_odp: Some(Cross {
            verdict: Verdict::Dropped,
            lost: &[
                "La cifra de ingresos viene del cierre provisional.",
                "De acuerdo, pero conviene fechar el dato en el pie.",
                "Falta el desglose por region.",
            ],
            warnings: &[DESIGN, INDEX, "comment(s) on page"],
        }),
        to_pptx: Some(intact()),
    },
    Cell {
        trait_name: "forty slides",
        pptx: Some("forty-slides.pptx"),
        odp: Some("forty-slides.odp"),
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: Some(intact()),
    },
    // ---- the five `.pptx` traits ODF has no counterpart for -------------
    Cell {
        trait_name: "a notes part bound by a crossed relationship",
        pptx: Some("notes-crossed.pptx"),
        odp: None,
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: None,
    },
    Cell {
        trait_name: "a cached autofit scale",
        pptx: Some("autofit-stale.pptx"),
        odp: None,
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: None,
    },
    Cell {
        trait_name: "a chart whose embedded workbook disagrees with its cache",
        pptx: Some("charts-stale-workbook.pptx"),
        odp: None,
        to_odp: Some(Cross {
            verdict: Verdict::Dropped,
            lost: &["Ventas", "Norte", "Sur", "Este", "1200", "980", "1440"],
            warnings: &[DESIGN, INDEX, "a chart on page"],
        }),
        to_pptx: None,
    },
    Cell {
        trait_name: "SmartArt behind mc:AlternateContent",
        pptx: Some("smartart-fallback.pptx"),
        odp: None,
        to_odp: Some(Cross {
            verdict: Verdict::Dropped,
            lost: &["Planificar · Ejecutar · Revisar"],
            warnings: &[DESIGN, "a preserved shape on page"],
        }),
        to_pptx: None,
    },
    Cell {
        trait_name: "a macro-enabled package",
        pptx: Some("macro-enabled.pptm"),
        odp: None,
        to_odp: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, INDEX],
        }),
        to_pptx: None,
    },
    // ---- the one ODF trait PresentationML's corpus has no counterpart for -
    Cell {
        trait_name: "the presentation:class vocabulary",
        pptx: None,
        odp: Some("presentation-classes.odp"),
        to_odp: None,
        to_pptx: Some(Cross {
            verdict: Verdict::Warned,
            lost: &[],
            warnings: &[DESIGN, LAYOUT, "a `text` placeholder"],
        }),
    },
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// A directory of its own per conversion.
///
/// Not the corpus, and never beside the input: a conversion with default
/// options writes its sidecar assets beside the *file it was given*, so a test
/// that hands it a corpus path writes into `corpus/`. Phase 19 had to fix
/// exactly that, and this test hands every conversion a working directory of
/// its own instead.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "docsai-cross-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("creates the scratch directory");
    dir
}

fn options() -> ConvertOptions {
    ConvertOptions {
        fidelity: Fidelity::Full,
        ..ConvertOptions::default()
    }
}

/// The text a person reading the deck would see, as a multiset of lines.
///
/// Everything DocMark writes *about* the deck is removed — the front matter,
/// the container fences, every attribute block, the list and heading markers,
/// the table rules — so that what is left is what the slides say. `[]` is
/// dropped with them: it is DocMark's marker for a placeholder with no text in
/// it, so it carries none.
fn text_lines(markdown: &str) -> BTreeMap<String, usize> {
    let body = match markdown.strip_prefix("---\n") {
        Some(rest) => match rest.find("\n---\n") {
            Some(at) => &rest[at + 5..],
            None => rest,
        },
        None => markdown,
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in body.lines() {
        let line = strip_attrs(line.trim());
        let line = line.trim();
        if line.is_empty() || line.starts_with(":::") {
            continue;
        }
        if line.starts_with('|') {
            let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
            if cells
                .iter()
                .all(|cell| cell.is_empty() || cell.chars().all(|c| c == '-' || c == ':'))
            {
                continue;
            }
            for cell in cells {
                push(&mut counts, cell);
            }
            continue;
        }
        let line = line.trim_start_matches('#').trim_start();
        let line = line.strip_prefix("> ").unwrap_or(line);
        let line = line.strip_prefix("- ").unwrap_or(line);
        let line = match line.split_once(". ") {
            Some((number, rest)) if number.chars().all(|c| c.is_ascii_digit()) => rest,
            _ => line,
        };
        push(&mut counts, line);
    }
    counts
}

fn push(counts: &mut BTreeMap<String, usize>, text: &str) {
    let text = text.trim();
    if text.is_empty() || text == "[]" {
        return;
    }
    *counts.entry(text.to_string()).or_insert(0) += 1;
}

/// Every `{…}` attribute block removed, at any depth of nesting.
fn strip_attrs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut depth = 0usize;
    for ch in line.chars() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// One crossing: the deck, its DocMark, the package in the other dialect, and
/// that package's DocMark.
struct Crossed {
    before: BTreeMap<String, usize>,
    after: BTreeMap<String, usize>,
    warnings: Vec<String>,
}

fn cross(deck: &Path, suffix: &str, label: &str) -> Crossed {
    let dir = scratch(label);
    let markdown = dir.join("deck.dmk.md");
    let first = convert_file(deck, Some(&markdown), &options())
        .unwrap_or_else(|e| panic!("{label}: converts to DocMark: {e}"));

    let package = dir.join(format!("crossed.{suffix}"));
    let outcome = convert_file(&markdown, Some(&package), &options())
        .unwrap_or_else(|e| panic!("{label}: converts to .{suffix}: {e}"));
    let written = std::fs::metadata(&package)
        .unwrap_or_else(|e| panic!("{label}: the package was not written: {e}"));
    assert!(written.len() > 0, "{label}: the package is empty");

    let back = dir.join("back.dmk.md");
    let last = convert_file(&package, Some(&back), &options())
        .unwrap_or_else(|e| panic!("{label}: reads the crossed package back: {e}"));

    let crossed = Crossed {
        before: text_lines(&first.markdown),
        after: text_lines(&last.markdown),
        warnings: outcome
            .report
            .warnings
            .iter()
            .map(|w| w.message())
            .collect(),
    };
    let _ = std::fs::remove_dir_all(&dir);
    crossed
}

fn check(label: &str, deck: &Path, suffix: &str, expected: &Cross) {
    let crossed = cross(deck, suffix, &label.replace(['/', '.', ' '], "-"));

    // 1. The text, as a multiset, with the declared losses taken out.
    let mut unexplained = Vec::new();
    let mut claimed: Vec<&str> = Vec::new();
    for (line, count) in &crossed.before {
        let after = crossed.after.get(line).copied().unwrap_or(0);
        if after >= *count {
            assert!(
                !expected.lost.contains(&line.as_str()),
                "{label}: `{line}` is declared lost and survived — a warning for a \
                 loss that did not happen is the same failure as a missing one"
            );
            continue;
        }
        match expected.lost.iter().find(|lost| **lost == line.as_str()) {
            Some(lost) => claimed.push(lost),
            None => unexplained.push(format!("`{line}` ({count} -> {after})")),
        }
    }
    assert!(
        unexplained.is_empty(),
        "{label}: text the crossing lost and the matrix does not declare: {}",
        unexplained.join(", ")
    );
    for fragment in expected.lost {
        assert!(
            claimed.contains(fragment),
            "{label}: the matrix declares `{fragment}` lost and it was not"
        );
    }

    // 2. The warnings cover the losses, both ways round.
    for warning in &crossed.warnings {
        assert!(
            expected
                .warnings
                .iter()
                .any(|fragment| warning.contains(fragment)),
            "{label}: a warning the matrix does not declare: {warning}"
        );
    }
    for fragment in expected.warnings {
        assert!(
            crossed
                .warnings
                .iter()
                .any(|warning| warning.contains(fragment)),
            "{label}: the matrix declares the warning `{fragment}` and it did not \
             fire; the warnings were {:?}",
            crossed.warnings
        );
    }

    // 3. The verdict the matrix states is the verdict its own data describes.
    let verdict = if !expected.lost.is_empty() {
        Verdict::Dropped
    } else if expected.warnings.len() > 1 {
        Verdict::Warned
    } else {
        Verdict::Intact
    };
    assert_eq!(
        expected.verdict, verdict,
        "{label}: the cell's verdict and the cell's own data disagree"
    );
}

/// The matrix is walked cell by cell, in both directions.
#[test]
fn every_trait_crosses_the_way_the_matrix_says_it_does() {
    for cell in MATRIX {
        if let (Some(fixture), Some(expected)) = (cell.pptx, &cell.to_odp) {
            check(
                &format!("{} [{fixture} -> .odp]", cell.trait_name),
                &root().join("pptx").join(fixture),
                "odp",
                expected,
            );
        }
        if let (Some(fixture), Some(expected)) = (cell.odp, &cell.to_pptx) {
            check(
                &format!("{} [{fixture} -> .pptx]", cell.trait_name),
                &root().join("odp").join(fixture),
                "pptx",
                expected,
            );
        }
    }
}

/// The matrix covers both corpora exactly: every fixture in exactly one cell,
/// every cell naming a fixture that exists, and a size that has to be edited
/// when either corpus moves.
///
/// This is what makes the walk above a matrix rather than a list of examples.
/// A trait pair that is not covered fails the build; it is not quietly absent.
#[test]
fn the_matrix_covers_both_corpora_and_says_how_big_it_is() {
    assert_eq!(
        MATRIX.len(),
        22,
        "the matrix is 16 shared traits, 5 with no .odp and 1 with no .pptx"
    );
    assert_eq!(
        MATRIX
            .iter()
            .filter(|c| c.pptx.is_some() && c.odp.is_some())
            .count(),
        16,
        "the number of traits both corpora carry moved"
    );

    for corpus in ["pptx", "odp"] {
        let dir = root().join(corpus);
        let mut fixtures: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .filter_map(|entry| entry.ok().map(|e| e.file_name()))
            .filter_map(|name| name.to_str().map(str::to_string))
            .filter(|name| {
                name.ends_with(".pptx") || name.ends_with(".pptm") || name.ends_with(".odp")
            })
            .collect();
        fixtures.sort();

        let mut declared: Vec<String> = MATRIX
            .iter()
            .filter_map(|cell| {
                if corpus == "pptx" {
                    cell.pptx
                } else {
                    cell.odp
                }
            })
            .map(str::to_string)
            .collect();
        declared.sort();
        let unique: std::collections::BTreeSet<&String> = declared.iter().collect();
        assert_eq!(
            unique.len(),
            declared.len(),
            "a corpus/{corpus} fixture appears in two cells"
        );
        assert_eq!(
            fixtures, declared,
            "corpus/{corpus} and the matrix disagree about which fixtures exist"
        );
    }

    // Every declared crossing has a fixture on the side it starts from, and
    // every fixture has a crossing declared for it.
    for cell in MATRIX {
        assert_eq!(
            cell.pptx.is_some(),
            cell.to_odp.is_some(),
            "{}: a .pptx fixture and a crossing to .odp go together",
            cell.trait_name
        );
        assert_eq!(
            cell.odp.is_some(),
            cell.to_pptx.is_some(),
            "{}: a .odp fixture and a crossing to .pptx go together",
            cell.trait_name
        );
        assert!(
            cell.pptx.is_some() || cell.odp.is_some(),
            "{}: a cell with no fixture at all",
            cell.trait_name
        );
    }
}
