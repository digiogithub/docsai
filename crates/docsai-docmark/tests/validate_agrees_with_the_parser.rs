//! `validate_docmark` and `parse` must never disagree (Phase 17-F).
//!
//! The value of a validator is entirely in being the same authority as the
//! thing it validates. A document it calls clean that `convert_from_markdown`
//! then refuses is worse than no validator at all, and a document it calls
//! broken that converts fine teaches an agent to ignore it.
//!
//! So the property, over every corpus golden and over eight mutations of each —
//! the mistakes an agent actually makes when it edits DocMark by hand:
//!
//! ```text
//! validate(text).parses()  ==  parse(text).is_ok()
//! ```
//!
//! Both DocMark profiles are covered because the corpus covers them: the
//! goldens under `corpus/docx`, `corpus/odt` and `corpus/xlsx` are the text and
//! sheet profiles, those under `corpus/pptx` and `corpus/odp` are DocMark-P.

use std::path::{Path, PathBuf};

use docsai_docmark::validate::{validate_at, Severity};
use docsai_docmark::{parse_with_base, ParseError};
use docsai_model::MemoryAssetStore;

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Every `*.expected.dmk.md` in the corpus, with the directory it lives in.
fn goldens() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for family in ["docx", "odt", "xlsx", "ods", "pptx", "odp", "doc"] {
        let dir = corpus_root().join(family);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.to_string_lossy().ends_with(".expected.dmk.md") {
                out.push(path);
            }
        }
    }
    out.sort();
    assert!(out.len() > 20, "the corpus should have goldens: {out:?}");
    out
}

fn parses(text: &str, base: Option<&Path>) -> Result<(), ParseError> {
    let mut assets = MemoryAssetStore::new();
    parse_with_base(text, base, &mut assets).map(|_| ())
}

/// The mistakes an agent makes when it edits DocMark by hand.
///
/// Each is a whole-document rewrite, named, so a failure says which edit broke
/// the agreement. A mutation that changes nothing (the document has no such
/// construct) is skipped by the caller.
fn mutations(text: &str) -> Vec<(&'static str, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();

    // 1. Deleted the closing `:::` of a container.
    if let Some(index) = lines.iter().position(|line| line.trim() == ":::") {
        out.push(("drop-a-container-closer", without(&lines, index)));
    }
    // 2. Deleted the closing `---` of the front matter.
    if let Some(index) = lines.iter().skip(1).position(|line| line.trim() == "---") {
        out.push(("drop-the-front-matter-closer", without(&lines, index + 1)));
    }
    // 3. Deleted the closing fence of a code block.
    let fences: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().starts_with("```"))
        .map(|(index, _)| index)
        .collect();
    if let Some(index) = fences.last() {
        out.push(("drop-a-code-fence-closer", without(&lines, *index)));
    }
    // 4. Reused a node id, the classic copy-paste.
    if let Some(index) = lines.iter().position(|line| line.contains("{#n")) {
        let mut copy = lines.clone();
        let duplicate = lines[index].to_string();
        copy.insert(index + 1, &duplicate);
        out.push(("duplicate-a-node-id", copy.join("\n") + "\n"));
    }
    // 5. Invented a container class.
    out.push((
        "invent-a-container-class",
        format!("{text}\n::: {{.sidebar}}\n\nA note.\n\n:::\n"),
    ));
    // 6. Declared a DocMark version that does not exist.
    if text.contains("docmark: ") {
        out.push((
            "declare-a-future-version",
            replace_first_line_starting(text, "docmark: ", "docmark: \"9.9\""),
        ));
    }
    // 7. Wrote a bullet with nothing after it.
    out.push(("append-an-empty-list", format!("{text}\n-\n")));
    // 8. Broke the YAML indentation of the front matter.
    if let Some(index) = lines.iter().position(|line| line.starts_with("  ")) {
        let mut copy: Vec<String> = lines.iter().map(|line| (*line).to_string()).collect();
        copy[index] = format!("   {}", copy[index].trim_start());
        out.push(("misindent-the-front-matter", copy.join("\n") + "\n"));
    }
    out
}

fn without(lines: &[&str], index: usize) -> String {
    let mut copy: Vec<&str> = lines.to_vec();
    copy.remove(index);
    copy.join("\n") + "\n"
}

fn replace_first_line_starting(text: &str, prefix: &str, replacement: &str) -> String {
    let mut done = false;
    text.lines()
        .map(|line| {
            if !done && line.starts_with(prefix) {
                done = true;
                replacement.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn the_validator_and_the_parser_never_disagree() {
    let mut checked = 0usize;
    let mut broken = 0usize;
    for golden in goldens() {
        let base = golden.parent();
        let text = std::fs::read_to_string(&golden).expect("golden readable");

        let mut cases = vec![("as-written".to_string(), text.clone())];
        cases.extend(
            mutations(&text)
                .into_iter()
                .map(|(name, source)| (name.to_string(), source)),
        );

        for (name, source) in cases {
            let report = validate_at(&source, base);
            let parsed = parses(&source, base).is_ok();
            assert_eq!(
                report.parses(),
                parsed,
                "{} [{name}]: the validator says parses={}, the parser says {}\n{}",
                golden.display(),
                report.parses(),
                parsed,
                report.render()
            );
            checked += 1;
            if !parsed {
                broken += 1;
                assert!(
                    report.errors() > 0,
                    "{} [{name}]: the parser refused it and the validator named nothing",
                    golden.display()
                );
                // Every error says where. A problem an agent cannot locate is
                // a parse failure with extra steps.
                for diagnostic in report
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                {
                    assert!(
                        diagnostic.line > 0,
                        "{} [{name}]: {} has no line",
                        golden.display(),
                        diagnostic.code
                    );
                }
            }
        }
    }
    // 442 cases over 58 goldens today, 204 of them genuinely broken; the
    // floors are what makes the numbers a gate rather than a note.
    assert!(checked > 400, "only {checked} cases");
    assert!(
        broken > 150,
        "only {broken} of {checked} mutations actually broke a document; \
         the mutation set is not exercising the error path"
    );
}

/// The corpus is what this repository writes. Whatever the validator says about
/// it is the baseline; nothing it writes may be an *error*.
#[test]
fn nothing_this_repository_writes_is_invalid() {
    for golden in goldens() {
        let text = std::fs::read_to_string(&golden).expect("golden readable");
        let report = validate_at(&text, golden.parent());
        assert_eq!(
            report.errors(),
            0,
            "{}:\n{}",
            golden.display(),
            report.render()
        );
    }
}

/// A warning is a promise that the document parses. If one ever fires on text
/// the parser refuses, it is describing a document that does not exist.
#[test]
fn a_warning_never_stands_alone_on_text_that_does_not_parse() {
    for golden in goldens() {
        let base = golden.parent();
        let text = std::fs::read_to_string(&golden).expect("golden readable");
        for (name, source) in mutations(&text) {
            if parses(&source, base).is_ok() {
                continue;
            }
            let report = validate_at(&source, base);
            assert_eq!(
                report.warnings(),
                0,
                "{} [{name}]: warnings on unparseable text\n{}",
                golden.display(),
                report.render()
            );
        }
    }
}
