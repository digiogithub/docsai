//! The degradation rule as a test (plan v2 Phase 14-G).
//!
//! Spec §6: *«at `plain` a slide is its heading, its bullets and its images,
//! and nothing else»*. Spike P2 turned that sentence into a number — residue,
//! the characters a plain Markdown viewer prints that are not the document —
//! and measured it with a scratchpad binary over hand-written samples. Here it
//! is measured by CI over what the serialiser actually writes, which is the
//! only version of the claim that cannot go stale.
//!
//! The viewer is real: `comrak` with the GFM extensions and **no** container
//! or attribute extension, which is what a wiki, a code host or an editor
//! preview does with a DocMark file. Residue is counted per character, not per
//! line, because CommonMark's lazy continuation glues an opening fence, the
//! paragraph under it and the closing fence into one rendered paragraph — a
//! line-level count would charge a whole bullet to the syntax before it.

use std::path::{Path, PathBuf};

use docsai_docmark::{Fidelity, Options};
use docsai_model::addressing::IdPolicy;
use docsai_model::{Document, Format, MemoryAssetStore};

// --------------------------------------------------------------------------
// The corpus
// --------------------------------------------------------------------------

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

fn name(deck: &Path) -> String {
    deck.file_stem().unwrap().to_string_lossy().into_owned()
}

/// A deck, serialised at one level, exactly as the CLI would.
fn docmark(deck: &Path, fidelity: Fidelity) -> String {
    let file = std::fs::File::open(deck).unwrap_or_else(|e| panic!("{}: {e}", deck.display()));
    let mut assets = MemoryAssetStore::new();
    let (document, _) = docsai_office::read_pptx(file, &mut assets)
        .unwrap_or_else(|e| panic!("{}: {e}", deck.display()));
    assert!(
        matches!(document, Document::Presentation(_)),
        "{} is not a deck",
        deck.display()
    );
    let options = Options {
        fidelity,
        ids: match fidelity {
            Fidelity::Full => IdPolicy::Assign,
            _ => IdPolicy::Never,
        },
        source_format: Format::Pptx,
        ..Options::default()
    };
    let (markdown, _) = docsai_docmark::serialize(&document, &assets, &options);
    markdown
}

// --------------------------------------------------------------------------
// The probe (spike P2 §6, now in the tree)
// --------------------------------------------------------------------------

/// One visible line as the viewer prints it, with the syntax that leaked into
/// it kept as text: *how much* leaked is a number, *what* leaked is the rule.
#[derive(Debug)]
struct Line {
    text: String,
    residue: Vec<String>,
}

impl Line {
    fn residue(&self) -> usize {
        self.residue.iter().map(|s| s.chars().count()).sum()
    }
}

#[derive(Debug, Default)]
struct Measure {
    lines: Vec<Line>,
}

impl Measure {
    fn visible(&self) -> usize {
        self.lines.iter().map(|l| l.text.chars().count()).sum()
    }

    fn residue(&self) -> usize {
        self.lines.iter().map(|l| l.residue()).sum()
    }

    fn ratio(&self) -> f64 {
        let visible = self.visible();
        if visible == 0 {
            0.0
        } else {
            self.residue() as f64 * 100.0 / visible as f64
        }
    }

    fn spans(&self) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .flat_map(|l| l.residue.iter().map(String::as_str))
    }

    /// What the probe's `--show` printed, for a failure message that can be
    /// read without rerunning anything.
    fn show(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                let tag = if l.residue.is_empty() {
                    "content"
                } else {
                    "RESIDUE"
                };
                format!("{tag} {}", l.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Renders the body as a plain viewer would and classes every visible
/// character.
fn measure(markdown: &str) -> Measure {
    let body = strip_front_matter(markdown);
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    let html = comrak::markdown_to_html(body, &options);
    Measure {
        lines: visible_lines(&html)
            .into_iter()
            .map(|text| {
                let residue = residue_spans(&text);
                Line { text, residue }
            })
            .collect(),
    }
}

fn strip_front_matter(markdown: &str) -> &str {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return markdown;
    };
    match rest.find("\n---\n") {
        Some(end) => &rest[end + 5..],
        None => markdown,
    }
}

/// The visible text of rendered HTML, one block element per line.
fn visible_lines(html: &str) -> Vec<String> {
    // Inline elements keep the text flowing; everything else ends a line. The
    // list is the one comrak can emit for GFM, so an unknown tag breaking a
    // line is the safe direction: it can only split residue away from content,
    // never merge them.
    const INLINE: &[&str] = &[
        "a", "em", "strong", "code", "del", "sup", "sub", "span", "img",
    ];
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut chars = html.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let mut tag = String::new();
                for c in chars.by_ref() {
                    if c == '>' {
                        break;
                    }
                    tag.push(c);
                }
                let bare = tag.trim_start_matches('/');
                let bare = bare
                    .split([' ', '/', '\n'])
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !INLINE.contains(&bare.as_str()) && !current.trim().is_empty() {
                    lines.push(std::mem::take(&mut current).trim().to_string());
                } else if !INLINE.contains(&bare.as_str()) {
                    current.clear();
                }
            }
            '&' => {
                let mut entity = String::new();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == ';' {
                        break;
                    }
                    entity.push(c);
                    if entity.len() > 8 {
                        break;
                    }
                }
                current.push_str(match entity.as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "#39" => "'",
                    _ => "",
                });
            }
            '\n' => current.push(' '),
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        lines.push(current.trim().to_string());
    }
    lines
}

/// The runs of a visible line that are syntax the viewer could not hide: a
/// `:::` fence with the attribute block it carries, a bare attribute block,
/// and the `[]` of an empty span that dragged its brackets along.
fn residue_spans(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i..].starts_with(&[':', ':', ':']) {
            let mut j = i + 3;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if j < chars.len() && chars[j] == '{' {
                while j < chars.len() && chars[j] != '}' {
                    j += 1;
                }
                j = (j + 1).min(chars.len());
            }
            spans.push(chars[i..j].iter().collect());
            i = j;
            continue;
        }
        if chars[i] == '{' {
            let mut j = i;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            j = (j + 1).min(chars.len());
            let start = if i >= 2 && chars[i - 2] == '[' && chars[i - 1] == ']' {
                i - 2
            } else {
                i
            };
            spans.push(chars[start..j].iter().collect());
            i = j;
            continue;
        }
        i += 1;
    }
    spans
}

// --------------------------------------------------------------------------
// The rule
// --------------------------------------------------------------------------

#[test]
fn a_deck_at_plain_shows_nothing_but_the_deck() {
    let decks = decks();
    assert!(decks.len() >= 14, "only {} decks found", decks.len());
    let mut offenders = Vec::new();
    for deck in &decks {
        let markdown = docmark(deck, Fidelity::Plain);
        let measured = measure(&markdown);
        if measured.residue() > 0 {
            offenders.push(format!("{}:\n{}", name(deck), measured.show()));
        }
    }
    assert!(
        offenders.is_empty(),
        "residue in a plain viewer:\n\n{}",
        offenders.join("\n\n")
    );
}

#[test]
fn plain_writes_no_syntax_a_viewer_would_have_to_hide() {
    for deck in &decks() {
        let markdown = docmark(deck, Fidelity::Plain);
        let name = name(deck);
        assert!(
            !markdown.starts_with("---\n"),
            "{name}: plain wrote front matter"
        );
        for (number, line) in markdown.lines().enumerate() {
            assert!(
                !line.contains(":::"),
                "{name}:{}: a container at plain: {line}",
                number + 1
            );
            assert!(
                !line.contains("{#") && !line.contains("]{."),
                "{name}:{}: an attribute block at plain: {line}",
                number + 1
            );
        }
    }
}

/// The rule of §11.2 read as *what* may leak, not only how much: at
/// `standard` a viewer may print the slide marker of rule 1 and the containers
/// of rules 4 and 8, and nothing else. A percentage alone would let an
/// attribute creep back in as long as the file grew around it.
#[test]
fn standard_leaks_the_slide_marker_and_the_containers_and_nothing_else() {
    let mut offenders = Vec::new();
    for deck in &decks() {
        let measured = measure(&docmark(deck, Fidelity::Standard));
        for span in measured.spans() {
            if span.starts_with(":::") || is_slide_marker(span) {
                continue;
            }
            offenders.push(format!("{}: {span}", name(deck)));
        }
    }
    assert!(
        offenders.is_empty(),
        "syntax a `standard` deck is not allowed to show:\n{}",
        offenders.join("\n")
    );
}

/// `{.slide}` and what §11.2's attribute table lets it carry at `standard`:
/// the class, the section a slide belongs to and the hidden flag. An id, a
/// layout, a name or any measurement here is a bug, and this is where it is
/// caught.
fn is_slide_marker(span: &str) -> bool {
    let Some(inner) = span
        .strip_prefix("{.slide")
        .and_then(|s| s.strip_suffix('}'))
    else {
        return false;
    };
    inner.split_whitespace().all(|pair| {
        matches!(
            pair.split('=').next(),
            Some("section") | Some("hidden") | Some("")
        )
    })
}

/// The number P2 attached to risk P4, now measured over what the serialiser
/// writes rather than over samples written by hand.
///
/// It is *worse* than the spike's 11.4 %, by one construct: the spike's
/// samples omitted `{.slide}`, which rule 1 requires and which costs eight
/// characters per slide — the whole of the residue in ten of these seventeen
/// decks. The corpus is also mostly two-slide fixtures, where eight characters
/// is a large share of a small file; `forty-slides`, the only deck of
/// document size, sits at 13 %.
#[test]
fn the_deck_corpus_stays_inside_its_residue_budget() {
    let mut rows = Vec::new();
    let (mut visible, mut residue) = (0usize, 0usize);
    let (mut prose_visible, mut prose_residue) = (0usize, 0usize);
    for deck in &decks() {
        let name = name(deck);
        let measured = measure(&docmark(deck, Fidelity::Standard));
        rows.push(format!(
            "{name:<22} {:>4} visible {:>4} residue {:>6.1} %",
            measured.visible(),
            measured.residue(),
            measured.ratio()
        ));
        visible += measured.visible();
        residue += measured.residue();
        if !is_objects(&name) {
            prose_visible += measured.visible();
            prose_residue += measured.residue();
        }
    }
    let all = residue as f64 * 100.0 / visible as f64;
    let prose = prose_residue as f64 * 100.0 / prose_visible as f64;
    let table = rows.join("\n");
    assert!(
        all <= 18.0,
        "{all:.1} % residue over the corpus, budget 18.0 %\n{table}"
    );
    assert!(
        prose <= 15.0,
        "{prose:.1} % residue over the decks that are documents, budget 15.0 %\n{table}"
    );
}

/// A deck whose content *is* objects Markdown has no form for. It cannot read
/// as Markdown and the spec says so (rule 8): the alternative to a visible
/// stub is a silent loss (`AGENTS.md` §7 rule 3). A new deck lands in the
/// strict bucket unless someone writes down why it should not.
fn is_objects(deck: &str) -> bool {
    matches!(
        deck,
        "shapes-geometry" | "smartart-fallback" | "charts-embedded" | "raw-preserved"
    )
}

/// The other half of the rule. Zero residue says nothing leaked; this says
/// what came through — the heading, the bullets, the image, the table — for
/// the four fixtures that are exactly those four things.
#[test]
fn a_slide_at_plain_is_its_heading_its_bullets_and_its_images() {
    let expected = [
        (
            "basic-slides",
            "## Informe trimestral\n\n- Ingresos al alza\n- Costes estables\n\n\
             ## Siguientes pasos\n\n- Cerrar el trimestre\n",
        ),
        (
            "bullets-levels",
            "## Niveles de viñeta\n\n- Primer nivel\n  - Segundo nivel\n    - Tercer nivel\n\
             - Vuelve al primero\n\n1. Uno\n2. Dos\n",
        ),
        (
            "images-anchored",
            "## Con imagen\n\n![Gráfico de barras azul](assets/img-40e10599.png)\n",
        ),
        (
            "tables-simple",
            "## Ventas por región\n\n\
             | Región | Ventas |\n\
             | ------ | ------ |\n\
             | Norte  | 1 200  |\n\
             | Sur    | 980    |\n",
        ),
    ];
    for (deck, want) in expected {
        let path = corpus_pptx().join(format!("{deck}.pptx"));
        assert_eq!(docmark(&path, Fidelity::Plain), want, "{deck} at plain");
    }
}

/// What a slide does not show does not survive `plain` — and the loss is
/// typed, which is the whole of `AGENTS.md` §7 rule 3.
#[test]
fn plain_drops_what_the_slide_does_not_show_and_says_so() {
    let path = corpus_pptx().join("notes-speaker.pptx");
    let markdown = docmark(&path, Fidelity::Plain);
    assert!(
        !markdown.contains("interanual"),
        "the speaker notes reached a plain reader:\n{markdown}"
    );

    let file = std::fs::File::open(&path).unwrap();
    let mut assets = MemoryAssetStore::new();
    let (document, _) = docsai_office::read_pptx(file, &mut assets).unwrap();
    let options = Options {
        fidelity: Fidelity::Plain,
        ids: IdPolicy::Never,
        source_format: Format::Pptx,
        ..Options::default()
    };
    let (_, report) = docsai_docmark::serialize(&document, &assets, &options);
    let dropped = report
        .warnings
        .iter()
        .filter(|w| format!("{w:?}").contains("notes"))
        .count();
    assert_eq!(
        dropped, 2,
        "one warning per notes page: {:?}",
        report.warnings
    );
}
