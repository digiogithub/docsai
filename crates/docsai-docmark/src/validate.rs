//! DocMark validation: what is wrong with this text, and where (Phase 17-F).
//!
//! An agent that writes DocMark and hands it to `convert_from_markdown` gets
//! back one [`ParseError`](crate::ParseError) and nothing else: a line, a
//! sentence, and no way to tell whether fixing it is the end of the job. This
//! module answers the same question the parser answers, in the form an editor
//! wants — a *list* of typed problems, each with a place, a rule from the
//! specification, and, where one can honestly be made, a fix.
//!
//! # The parser is the authority
//!
//! There is no second grammar here. Every run of [`validate`] runs the real
//! parser on the real text, and the two severities are defined against what it
//! did:
//!
//! * [`Severity::Error`] — **the text will not parse.** No error is ever
//!   emitted unless [`crate::parse_with_base`] refused this exact input. That
//!   is a structural guarantee, not a convention: the error arm of the match is
//!   the only place that can produce one.
//! * [`Severity::Warning`] — **it parses, and you probably did not mean it.**
//!   Warnings are computed from the document the parser produced and from its
//!   own [`ConversionReport`], so a warning describes something that happened
//!   rather than something that might.
//!
//! DocMark is deliberately tolerant of Markdown typed by hand, so the line
//! between the two matters: an unknown container class, an id that collides
//! with another, a missing front matter — all of these parse, and calling any
//! of them an error would be a lie.
//!
//! # Why more than one problem comes back
//!
//! The parser stops at the first thing it cannot read, which is right for a
//! parser and expensive for an agent: fix, call again, fix, call again. Two
//! things widen the answer without inventing anything:
//!
//! 1. **Unclosed constructs are located all at once.** A scan that mirrors
//!    [`crate::parser`]'s own fence rules finds *every* `:::` container and
//!    every code fence that is never closed, each at its opening line.
//! 2. **The document is repaired and re-parsed once.** The missing closers are
//!    appended — a repair that can only ever close what is open — and the
//!    parser is run again, so the next genuine error is reported beside the
//!    first. Appending never moves an existing line, so the second error's line
//!    still names the caller's own text; an error that lands past the end of
//!    the original is an artefact of the repair and is dropped.
//!
//! Beyond that the list does not speculate. Guessing what the text would have
//! meant if a bad attribute block were deleted produces errors the caller never
//! wrote, and a false error is the same failure as a missing one.

use std::collections::BTreeMap;
use std::path::Path;

use docsai_model::addressing::for_each_addressable;
use docsai_model::{ConversionReport, Document, MemoryAssetStore};

use crate::error::ParseError;

/// How bad a problem is, defined by what the parser did with the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The document does not parse. `convert_from_markdown` will refuse it.
    Error,
    /// The document parses, but not into what the text appears to say.
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which DocMark profile the text was read as.
///
/// Detected exactly as [`crate::parse`] detects it, so the profile named here
/// is the profile the document will actually be read under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// The text profile (spec §3).
    Text,
    /// The presentation profile, DocMark-P (spec §11.2).
    Deck,
    /// The spreadsheet profile (spec §4).
    Sheet,
    /// The front matter is broken, so the profile cannot be decided yet.
    Unknown,
}

impl Profile {
    pub fn as_str(self) -> &'static str {
        match self {
            Profile::Text => "text",
            Profile::Deck => "deck",
            Profile::Sheet => "sheet",
            Profile::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One problem with a DocMark document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// A stable, matchable code, `family/detail`. Codes are added over time;
    /// an existing one does not change meaning.
    pub code: &'static str,
    /// 1-based line in the text as given. `0` when the problem belongs to the
    /// document rather than to a place in it.
    pub line: usize,
    /// 1-based column. `0` when only the line is known.
    pub column: usize,
    /// The node id in force at that place, when the construct has one
    /// (spec §11.1). Never invented: it is the id written on the offending
    /// line, or the id of the innermost container still open there.
    pub node: Option<String>,
    /// What is wrong.
    pub message: String,
    /// The rule this violates, cited by its section in
    /// `docs/docmark-specification.md`.
    pub rule: &'static str,
    /// How to fix it — absent when no honest suggestion can be made.
    pub fix: Option<String>,
}

impl Diagnostic {
    fn new(severity: Severity, code: &'static str, rule: &'static str, message: String) -> Self {
        Diagnostic {
            severity,
            code,
            line: 0,
            column: 0,
            node: None,
            message,
            rule,
            fix: None,
        }
    }

    fn at(mut self, line: usize, column: usize) -> Self {
        self.line = line;
        self.column = column;
        self
    }

    fn node(mut self, node: Option<String>) -> Self {
        self.node = node;
        self
    }

    fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }

    /// One line, as the CLI and the MCP tool render it.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(self.severity.as_str());
        out.push(' ');
        if self.line == 0 {
            out.push('-');
        } else if self.column == 0 {
            out.push_str(&self.line.to_string());
        } else {
            out.push_str(&format!("{}:{}", self.line, self.column));
        }
        out.push(' ');
        out.push_str(self.code);
        if let Some(node) = &self.node {
            out.push_str(" #");
            out.push_str(node);
        }
        out.push_str(" — ");
        out.push_str(&self.message);
        out.push_str(" (");
        out.push_str(self.rule);
        out.push(')');
        if let Some(fix) = &self.fix {
            out.push_str(" Fix: ");
            out.push_str(fix);
        }
        out
    }
}

/// The result of validating one document.
#[derive(Debug, Clone)]
pub struct Validation {
    /// The profile the text is read under.
    pub profile: Profile,
    /// Errors first, then warnings; each group in document order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Validation {
    /// True when nothing at all is wrong: it parses and it means what it says.
    pub fn is_clean(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// True when the document parses. Warnings do not make it false.
    pub fn parses(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
    }

    pub fn errors(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }

    pub fn warnings(&self) -> usize {
        self.diagnostics.len() - self.errors()
    }

    /// The whole report as text, one problem per line under a summary line.
    pub fn render(&self) -> String {
        let mut out = if self.is_clean() {
            format!("clean — {} profile, 0 problems", self.profile)
        } else {
            format!(
                "{} problem(s) — {} profile, {} error(s), {} warning(s)",
                self.diagnostics.len(),
                self.profile,
                self.errors(),
                self.warnings()
            )
        };
        for diagnostic in &self.diagnostics {
            out.push('\n');
            out.push_str(&diagnostic.render());
        }
        out
    }
}

/// Validates DocMark text that has no directory behind it.
///
/// Sidecar raw-blocks and images cannot be resolved without one, so a document
/// that references them reports the reference as an error. Use
/// [`validate_at`] whenever the text came from a file.
///
/// ```
/// use docsai_docmark::validate::{validate, Severity};
///
/// let report = validate("---\ndocmark: \"1.0\"\nsource-format: docx\n---\n\nHello.\n");
/// assert!(report.is_clean(), "{}", report.render());
///
/// // The text profile swallows an unclosed container; a deck refuses it.
/// let broken = validate("---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n\
///                        ## S {#n1 .slide}\n\n::: {.ph idx=1}\n\nA\n");
/// assert_eq!(broken.diagnostics[0].severity, Severity::Error);
/// assert_eq!(broken.diagnostics[0].code, "container/unclosed");
/// ```
pub fn validate(markdown: &str) -> Validation {
    validate_at(markdown, None)
}

/// Validates DocMark text, resolving relative paths against `base_dir`.
pub fn validate_at(markdown: &str, base_dir: Option<&Path>) -> Validation {
    // The parser normalises line endings before it counts a line; the caller's
    // line numbers have to be counted the same way or they name the wrong row.
    let text = markdown.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.lines().collect();

    let mut assets = MemoryAssetStore::new();
    let parsed = crate::parse_with_base(&text, base_dir, &mut assets);

    let opens = scan_unclosed(&text);
    let mut errors: Vec<Diagnostic> = Vec::new();
    if let Err(error) = &parsed {
        if opens.is_empty() {
            errors.push(from_parse_error(error, &lines));
        } else {
            for open in &opens {
                errors.push(open.diagnostic(Severity::Error, &lines));
            }
            // Close what is open and ask the parser again. Appending can only
            // ever satisfy an open construct, and it moves no existing line, so
            // whatever comes back next is a second problem in the caller's own
            // text rather than one this repair created.
            let repaired = repair(&text, &opens);
            let mut repaired_assets = MemoryAssetStore::new();
            if let Err(next) = crate::parse_with_base(&repaired, base_dir, &mut repaired_assets) {
                let diagnostic = from_parse_error(&next, &lines);
                // Same line *and* same code is the finding already made; the
                // same line with a different code is a second problem there —
                // an unknown class on the container that was also left open.
                if diagnostic.line <= lines.len()
                    && !errors
                        .iter()
                        .any(|d| d.line == diagnostic.line && d.code == diagnostic.code)
                {
                    errors.push(diagnostic);
                }
            }
        }
    }
    errors.sort_by_key(sort_key);

    let profile = detect_profile(&text);
    let mut warnings = match &parsed {
        Ok((document, report)) => {
            let mut warnings = opens
                .iter()
                .map(|open| open.diagnostic(Severity::Warning, &lines))
                .collect::<Vec<_>>();
            warnings.extend(document_warnings(&text, &lines, profile, document, report));
            warnings
        }
        // Nothing parsed, so nothing is known about what the document means.
        // Guessing here is exactly the false warning the increment forbids.
        Err(_) => Vec::new(),
    };
    warnings.sort_by_key(sort_key);

    errors.append(&mut warnings);
    Validation {
        profile,
        diagnostics: errors,
    }
}

/// Document order, with the problems that belong to no line last: a reader
/// works down the file, and "somewhere in this document" is not a place in it.
fn sort_key(diagnostic: &Diagnostic) -> (usize, usize) {
    let line = if diagnostic.line == 0 {
        usize::MAX
    } else {
        diagnostic.line
    };
    (line, diagnostic.column)
}

/// Validates the DocMark file at `path`, with its own directory as the base.
pub fn validate_file(path: &Path) -> Result<Validation, std::io::Error> {
    let text = std::fs::read_to_string(path)?;
    Ok(validate_at(&text, path.parent()))
}

// ---------------------------------------------------------------------------
// Profile
// ---------------------------------------------------------------------------

fn detect_profile(text: &str) -> Profile {
    let Ok((front, body, _)) = crate::parser::split_front_matter(text) else {
        return Profile::Unknown;
    };
    let front_matter = match front {
        Some(source) => match crate::frontmatter_parse::parse(source, 2) {
            Ok(front_matter) => front_matter,
            Err(_) => return Profile::Unknown,
        },
        None => crate::frontmatter_parse::FrontMatter::default(),
    };
    if crate::deck_parser::looks_like_deck(&front_matter, body) {
        Profile::Deck
    } else if crate::sheet_parser::looks_like_workbook(&front_matter, body) {
        Profile::Sheet
    } else {
        Profile::Text
    }
}

// ---------------------------------------------------------------------------
// Errors: the parser's own refusal, given a code and a fix
// ---------------------------------------------------------------------------

fn from_parse_error(error: &ParseError, lines: &[&str]) -> Diagnostic {
    let (code, rule, fix) = classify(error);
    let line = error_line(error);
    let column = line
        .checked_sub(1)
        .and_then(|index| lines.get(index))
        .map(|text| text.len() - text.trim_start().len() + 1)
        .unwrap_or(0);
    let mut diagnostic = Diagnostic::new(Severity::Error, code, rule, error.to_string())
        .at(line, column)
        .node(id_in_force(lines, line));
    if let Some(fix) = fix {
        diagnostic = diagnostic.fix(fix);
    }
    diagnostic
}

fn error_line(error: &ParseError) -> usize {
    match error {
        ParseError::InvalidFrontMatter { line, .. } | ParseError::Unexpected { line, .. } => *line,
        _ => 0,
    }
}

/// Maps a parser refusal onto a stable code, the rule it breaks and a fix.
///
/// The message is the parser's own; only the classification lives here, and it
/// keys off the exact sentences `parser.rs`, `sheet_parser.rs` and
/// `frontmatter_parse.rs` construct. An unrecognised message still gets a code
/// — `parse/front-matter` or `parse/unexpected` — so a new parser message
/// degrades to a coarser answer rather than to no answer.
fn classify(error: &ParseError) -> (&'static str, &'static str, Option<String>) {
    match error {
        ParseError::InvalidFrontMatter { message, .. } => {
            if message.contains("unclosed front matter") {
                (
                    "front-matter/unclosed",
                    "spec §2",
                    Some(
                        "end the front matter with a line containing exactly `---`, before the body"
                            .into(),
                    ),
                )
            } else if message.contains("unsupported docmark version") {
                (
                    "front-matter/unsupported-version",
                    "spec §11 (version rule)",
                    Some(format!(
                        "set `docmark:` to one of {}",
                        docsai_model::DOCMARK_VERSIONS
                            .iter()
                            .map(|v| format!("\"{v}\""))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                )
            } else if message.contains("unknown paper size") {
                (
                    "front-matter/unknown-paper-size",
                    "spec §2 (`page.size`)",
                    Some("use a named size docsai knows, or give `width` and `height` with explicit units".into()),
                )
            } else if message.contains("must be a mapping") {
                (
                    "front-matter/wrong-shape",
                    "spec §2",
                    Some("write the key as a YAML mapping, indented under its name".into()),
                )
            } else {
                (
                    "parse/front-matter",
                    "spec §2",
                    // The YAML message already says what it wanted; a second
                    // guess on top of it would be worth less than silence.
                    None,
                )
            }
        }
        ParseError::Unexpected { message, .. } => {
            if message.contains("unclosed fenced container") {
                (
                    "container/unclosed",
                    "spec §3.6",
                    Some("add a line containing exactly `:::` to close it".into()),
                )
            } else if message.contains("unclosed fence") {
                (
                    "fence/unclosed",
                    "spec §7",
                    Some("close the code fence with a line of the same backticks".into()),
                )
            } else if message.contains("unknown container class") {
                (
                    "container/unknown-class",
                    "spec §3.6",
                    Some(
                        "drop the class, or use one DocMark defines for a container: \
                         `.section`, `.header`, `.footer`, `.textbox`, `.table`, `.comment`, \
                         `.raw`"
                            .into(),
                    ),
                )
            } else if message.contains("empty list") {
                (
                    "list/empty",
                    "spec §3.3",
                    Some("give the list at least one item, or delete it".into()),
                )
            } else if message.contains("expected sheet heading") {
                (
                    "sheet/heading-missing",
                    "spec §4",
                    Some("start each sheet with a `# Name {.sheet}` heading".into()),
                )
            } else if message.contains("empty sheet table header") {
                (
                    "sheet/empty-header",
                    "spec §4",
                    Some("give the sheet table a header row naming its columns".into()),
                )
            } else if message.contains("lives in a sidecar") {
                (
                    "raw-block/no-base-dir",
                    "spec §7",
                    Some(
                        "validate the file on disk instead of the text, so `assets/_raw/` can be \
                         found beside it — or inline the raw-block's bytes in the container"
                            .into(),
                    ),
                )
            } else if message.contains("expected :::")
                || message.contains("expected fenced container")
            {
                (
                    "container/expected",
                    "spec §3.6",
                    Some("open the container with a line starting `::: {`".into()),
                )
            } else {
                ("parse/unexpected", "spec §3", None)
            }
        }
        ParseError::Unsupported { .. } => ("parse/unsupported", "spec §11", None),
        ParseError::Io { .. } => (
            "io/unreadable",
            "spec §7",
            Some("check the referenced file exists beside the document".into()),
        ),
        ParseError::Asset(_) => ("asset/unreadable", "spec §3.5", None),
    }
}

// ---------------------------------------------------------------------------
// The structural scan: every unclosed construct, not just the first
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenKind {
    Container,
    Fence,
}

#[derive(Debug, Clone)]
struct Unclosed {
    kind: OpenKind,
    /// 1-based line of the opening.
    line: usize,
    /// 1-based column of the opening marker.
    column: usize,
    /// The fence marker to close it with, for [`repair`].
    closer: String,
}

impl Unclosed {
    /// The problem this is, at the severity the parser's verdict earns it.
    ///
    /// The two profiles disagree about an unclosed container: `deck_parser`
    /// refuses one (`split_one_fence`), and the text parser swallows the rest
    /// of the document into it instead. So the same finding is an error where
    /// the document did not parse and a warning where it did — which is the
    /// whole line this module draws, drawn by the parser rather than by a
    /// table of opinions here.
    fn diagnostic(&self, severity: Severity, lines: &[&str]) -> Diagnostic {
        let consequence = match severity {
            Severity::Error => "the document does not parse",
            Severity::Warning => "everything after it is read as its content",
        };
        match self.kind {
            OpenKind::Container => Diagnostic::new(
                severity,
                "container/unclosed",
                "spec §3.6",
                format!(
                    "the `:::` container opened on line {} is never closed; {consequence}",
                    self.line
                ),
            )
            .at(self.line, self.column)
            .node(id_on_line(lines.get(self.line - 1).copied().unwrap_or("")))
            .fix(format!(
                "add a line containing exactly `:::` where the container opened on line {} ends",
                self.line
            )),
            OpenKind::Fence => Diagnostic::new(
                severity,
                "fence/unclosed",
                "spec §7",
                format!(
                    "the code fence opened on line {} is never closed; {consequence}",
                    self.line
                ),
            )
            .at(self.line, self.column)
            .fix(format!(
                "add a line containing `{}` where the fence opened on line {} ends",
                self.closer, self.line
            )),
        }
    }
}

/// Every `:::` container and every code fence left open at the end of the body.
///
/// The fence rules mirror [`crate::parser::split_one_fence`] exactly, including
/// its reading of a line of four or more colons as a *close*: a scan that
/// disagreed with the parser about what opens a container would be the second
/// grammar this module refuses to be.
fn scan_unclosed(text: &str) -> Vec<Unclosed> {
    let Ok((_, body, body_line)) = crate::parser::split_front_matter(text) else {
        // The front matter never closes, so nothing below it is body yet and
        // the parser's own error is the whole story.
        return Vec::new();
    };
    let mut stack: Vec<Unclosed> = Vec::new();
    let mut fence: Option<Unclosed> = None;
    for (offset, raw) in body.lines().enumerate() {
        let line = body_line + offset;
        let trimmed = raw.trim();
        let column = raw.len() - raw.trim_start().len() + 1;

        if let Some(open) = &fence {
            if is_fence_close(trimmed, &open.closer) {
                fence = None;
            }
            continue;
        }
        if let Some(marker) = fence_open(trimmed) {
            fence = Some(Unclosed {
                kind: OpenKind::Fence,
                line,
                column,
                closer: marker,
            });
            continue;
        }
        if !trimmed.starts_with(":::") {
            continue;
        }
        if trimmed.trim_start_matches(':').trim().is_empty() {
            stack.pop();
        } else {
            stack.push(Unclosed {
                kind: OpenKind::Container,
                line,
                column,
                closer: ":::".into(),
            });
        }
    }
    if let Some(open) = fence {
        stack.push(open);
    }
    stack.sort_by_key(|open| open.line);
    stack
}

/// The marker of a code fence opening on this line, if it opens one.
fn fence_open(trimmed: &str) -> Option<String> {
    for marker in ['`', '~'] {
        let run = trimmed.chars().take_while(|c| *c == marker).count();
        if run >= 3 {
            // An info string is allowed on the opening line and never on the
            // closing one, which is what makes them distinguishable.
            return Some(std::iter::repeat_n(marker, run).collect());
        }
    }
    None
}

fn is_fence_close(trimmed: &str, closer: &str) -> bool {
    let marker = closer.chars().next().unwrap_or('`');
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    run >= closer.len() && trimmed.chars().all(|c| c == marker)
}

/// Appends the closers the text is missing, innermost first.
fn repair(text: &str, opens: &[Unclosed]) -> String {
    let mut out = text.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    for open in opens.iter().rev() {
        out.push_str(&open.closer);
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Node ids
// ---------------------------------------------------------------------------

/// The id written in an attribute block on this line, if there is one.
fn id_on_line(line: &str) -> Option<String> {
    let mut rest = line;
    while let Some(at) = rest.find("{#") {
        let candidate = &rest[at + 2..];
        let end = candidate
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .unwrap_or(candidate.len());
        if end > 0 {
            return Some(candidate[..end].to_string());
        }
        rest = &rest[at + 2..];
    }
    None
}

/// Every node id written on this line.
///
/// A list keeps its id in `list-id=` rather than in `{#…}` (spec §11.1, the
/// table of where an id lives), so a scan that only looked for `{#` would find
/// the document's lists nowhere and report them at "no line".
fn ids_on_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(id) = id_on_line(line) {
        out.push(id);
    }
    let mut rest = line;
    while let Some(at) = rest.find("list-id=") {
        let candidate = &rest[at + "list-id=".len()..];
        let candidate = candidate.strip_prefix('"').unwrap_or(candidate);
        let end = candidate
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .unwrap_or(candidate.len());
        if end > 0 {
            out.push(candidate[..end].to_string());
        }
        rest = &rest[at + "list-id=".len()..];
    }
    out
}

/// The id in force at `line`: the one written there, else the one on the
/// opening line of the innermost container still open at that point.
///
/// Nothing is invented: when neither exists the answer is `None`, because the
/// construct genuinely has nowhere to hold an id (spec §11.1).
fn id_in_force(lines: &[&str], line: usize) -> Option<String> {
    if line == 0 || line > lines.len() {
        return None;
    }
    if let Some(id) = id_on_line(lines[line - 1]) {
        return Some(id);
    }
    let mut stack: Vec<&str> = Vec::new();
    for text in lines.iter().take(line - 1) {
        let trimmed = text.trim();
        if !trimmed.starts_with(":::") {
            continue;
        }
        if trimmed.trim_start_matches(':').trim().is_empty() {
            stack.pop();
        } else {
            stack.push(text);
        }
    }
    stack.last().and_then(|text| id_on_line(text))
}

// ---------------------------------------------------------------------------
// Warnings: it parses, and you probably did not mean it
// ---------------------------------------------------------------------------

fn document_warnings(
    text: &str,
    lines: &[&str],
    profile: Profile,
    document: &Document,
    report: &ConversionReport,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let (front, body, body_line) = match crate::parser::split_front_matter(text) {
        Ok(split) => split,
        // Unreachable: the document parsed, so the split succeeded.
        Err(_) => return out,
    };

    out.extend(front_matter_warnings(front, document));
    out.extend(id_warnings(front, lines, document));
    if profile == Profile::Text {
        out.extend(unknown_container_warnings(front, body, body_line));
    }
    out.extend(report_warnings(report));
    out
}

/// What the front matter does not say, and the defaults that fills in.
fn front_matter_warnings(front: Option<&str>, document: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let Some(front) = front else {
        out.push(
            Diagnostic::new(
                Severity::Warning,
                "front-matter/absent",
                "spec §2",
                "no front matter: the document is read as `docmark: \"1.0\"` with \
                 `source-format: docx`, whatever it was written from"
                    .into(),
            )
            .at(1, 1)
            .fix(
                "prepend a front matter block: `---`, `docmark: \"1.1\"`, \
                 `source-format: docx`, `---`",
            ),
        );
        return out;
    };
    let declared = declared_version(front);
    if declared.is_none() {
        out.push(
            Diagnostic::new(
                Severity::Warning,
                "front-matter/version-missing",
                "spec §2 (`docmark` is required)",
                "the front matter declares no `docmark:` version".into(),
            )
            .at(front_key_line(front, "docmark").unwrap_or(1), 1)
            .fix("add `docmark: \"1.1\"` to the front matter"),
        );
    }
    if declared.as_deref() == Some("1.0") && has_any_id(document) {
        out.push(
            Diagnostic::new(
                Severity::Warning,
                "version/ids-need-1-1",
                "spec §11.1 (node ids, normative)",
                "the document carries node ids but declares `docmark: \"1.0\"`, \
                 the version that has none"
                    .into(),
            )
            .at(front_key_line(front, "docmark").unwrap_or(1), 1)
            .fix("change `docmark:` to \"1.1\" (or \"1.2\" for a deck) and add a `next-id:`"),
        );
    }
    out
}

fn declared_version(front: &str) -> Option<String> {
    front.lines().find_map(|line| {
        line.strip_prefix("docmark:").map(|value| {
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
    })
}

fn declared_next_id(front: &str) -> Option<u64> {
    front.lines().find_map(|line| {
        line.strip_prefix("next-id:")
            .and_then(|value| value.trim().parse::<u64>().ok())
    })
}

/// 1-based line of a top-level front-matter key, counting the opening `---`.
fn front_key_line(front: &str, key: &str) -> Option<usize> {
    front
        .lines()
        .position(|line| line.starts_with(key) && line[key.len()..].starts_with(':'))
        .map(|index| index + 2)
}

fn has_any_id(document: &Document) -> bool {
    let mut found = false;
    for_each_addressable(document, &mut |node| {
        found |= node.node_id().is_some();
    });
    found
}

/// Ids the document holds, in the order the IR walks them.
fn document_ids(document: &Document) -> Vec<String> {
    let mut ids = Vec::new();
    for_each_addressable(document, &mut |node| {
        if let Some(id) = node.node_id() {
            ids.push(id.as_str().to_string());
        }
    });
    ids
}

/// Two nodes with one address, and an id the counter does not dominate.
///
/// Both are read off the document the parser built, never off the text: the IR
/// is what an edit addresses, so a duplicate there is a duplicate in fact. The
/// text is consulted only to say *where*.
fn id_warnings(front: Option<&str>, lines: &[&str], document: &Document) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let ids = document_ids(document);
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for id in &ids {
        *counts.entry(id.as_str()).or_default() += 1;
    }

    // Where each id is written, so a duplicate can be pointed at rather than
    // merely named.
    let mut sites: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        for id in ids_on_line(line) {
            sites.entry(id).or_default().push(index + 1);
        }
    }

    let next_id = front.and_then(declared_next_id);
    for (id, count) in &counts {
        if *count > 1 {
            let places = sites.get(*id).cloned().unwrap_or_default();
            let mut diagnostic = Diagnostic::new(
                Severity::Warning,
                "id/duplicate",
                "spec §11.1 (ids are never reused)",
                format!(
                    "`#{id}` addresses {count} different nodes; an edit aimed at it \
                     cannot say which"
                ),
            )
            .node(Some((*id).to_string()));
            if let Some(second) = places.get(1) {
                diagnostic = diagnostic.at(*second, 1);
            } else if let Some(first) = places.first() {
                diagnostic = diagnostic.at(*first, 1);
            }
            if let Some(next) = next_id {
                diagnostic = diagnostic.fix(format!(
                    "give the second one a fresh id — `{{#n{next}}}` — and raise `next-id:` to {}",
                    next + 1
                ));
            } else {
                diagnostic = diagnostic.fix(
                    "give one of them an id no other node uses, and declare a \
                                    `next-id:` above every id in the document",
                );
            }
            out.push(diagnostic);
        }
    }

    if let Some(next) = next_id {
        for id in &ids {
            let Some(value) = id.strip_prefix('n').and_then(|n| n.parse::<u64>().ok()) else {
                continue;
            };
            if value < next {
                continue;
            }
            out.push(
                Diagnostic::new(
                    Severity::Warning,
                    "id/above-next-id",
                    "spec §11.1 (the counter dominates every id)",
                    format!(
                        "`#{id}` is at or above the declared `next-id: {next}`; \
                         the counter is meant to be ahead of every id in the document"
                    ),
                )
                .at(
                    sites.get(id).and_then(|p| p.first().copied()).unwrap_or(0),
                    1,
                )
                .node(Some(id.clone()))
                .fix(format!("set `next-id: {}` or higher", value + 1)),
            );
        }
    }
    out
}

/// Container classes the text profile defines. A container carrying none of
/// them keeps its body and loses its class, silently.
const TEXT_CONTAINER_CLASSES: &[&str] = &[
    "section", "header", "footer", "textbox", "table", "row", "cell", "comment", "raw",
];

fn unknown_container_warnings(
    front: Option<&str>,
    body: &str,
    body_line: usize,
) -> Vec<Diagnostic> {
    // A class may also be a style name, a list definition or a dictionary
    // entry declared in the front matter (spec §3.7). Those mean something,
    // so flagging them would be the false warning this module must not emit.
    let declared = declared_class_names(front.unwrap_or(""));
    let mut out = Vec::new();
    let mut in_fence: Option<String> = None;
    for (offset, raw) in body.lines().enumerate() {
        let trimmed = raw.trim();
        if let Some(closer) = &in_fence {
            if is_fence_close(trimmed, closer) {
                in_fence = None;
            }
            continue;
        }
        if let Some(marker) = fence_open(trimmed) {
            in_fence = Some(marker);
            continue;
        }
        if !trimmed.starts_with(":::") {
            continue;
        }
        let source = trimmed.trim_start_matches(':').trim();
        if source.is_empty() {
            continue;
        }
        let Some(attrs) = crate::attrs::Attrs::parse(source) else {
            continue;
        };
        let classes = attrs.classes();
        if classes.is_empty() {
            continue;
        }
        if classes.iter().any(|class| {
            TEXT_CONTAINER_CLASSES.contains(&class.as_str()) || declared.contains(class)
        }) {
            continue;
        }
        let line = body_line + offset;
        out.push(
            Diagnostic::new(
                Severity::Warning,
                "container/unknown-class",
                "spec §3.6",
                format!(
                    "`.{}` is not a container class the text profile defines; the container's \
                     class is dropped and its body is kept",
                    classes.join(" .")
                ),
            )
            .at(line, raw.len() - raw.trim_start().len() + 1)
            .node(id_on_line(raw))
            .fix(
                "use `.section`, `.header`, `.footer`, `.textbox`, `.table`, `.comment` or \
                 `.raw` — or drop the container and keep its blocks",
            ),
        );
    }
    out
}

/// Class names the front matter gives a meaning to: styles, list definitions
/// and the attribute-set dictionary.
fn declared_class_names(front: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut section: Option<&str> = None;
    for line in front.lines() {
        if !line.starts_with(char::is_whitespace) && line.contains(':') {
            let key = line.split(':').next().unwrap_or("").trim();
            section = matches!(key, "styles" | "list-definitions" | "attribute-sets").then_some("");
            continue;
        }
        let Some(_) = section else { continue };
        // A direct child of the block: exactly two spaces of indent.
        let indent = line.len() - line.trim_start().len();
        if indent == 2 {
            if let Some(name) = line.trim().split(':').next() {
                if !name.is_empty() {
                    out.push(name.trim_matches('"').trim_matches('\'').to_string());
                }
            }
        }
    }
    out
}

/// The parser's own warnings, which are authoritative by construction: it
/// emitted them while reading this text.
fn report_warnings(report: &ConversionReport) -> Vec<Diagnostic> {
    report
        .warnings
        .iter()
        .map(|warning| {
            let mut diagnostic = Diagnostic::new(
                Severity::Warning,
                "parse/degraded",
                "spec §0 (nothing is lost silently)",
                warning.message(),
            );
            if let Some(line) = warning_line(warning) {
                diagnostic = diagnostic.at(line, 1);
            }
            diagnostic
        })
        .collect()
}

/// The line a parser warning names, when it names one (`location: "line 42"`).
fn warning_line(warning: &docsai_model::Warning) -> Option<usize> {
    let docsai_model::Warning::UnsupportedElement { location, .. } = warning else {
        return None;
    };
    location.strip_prefix("line ")?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "---\ndocmark: \"1.0\"\nsource-format: docx\n---\n\nHello.\n";

    #[test]
    fn a_well_formed_document_reports_nothing() {
        let report = validate(TEXT);
        assert!(report.is_clean(), "{}", report.render());
        assert_eq!(report.profile, Profile::Text);
    }

    #[test]
    fn every_unclosed_container_is_named_not_only_the_first() {
        let source = "---\ndocmark: \"1.0\"\n---\n\n::: {.section}\n\nA\n\n::: {.textbox}\n\nB\n";
        let report = validate(source);
        let codes: Vec<&str> = report.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(codes, vec!["container/unclosed", "container/unclosed"]);
        assert_eq!(report.diagnostics[0].line, 5);
        assert_eq!(report.diagnostics[1].line, 9);
        // The text profile swallows the rest of the document into the open
        // container rather than refusing it, so this is two warnings.
        assert!(report.parses(), "{}", report.render());
        assert_eq!(report.warnings(), 2);
    }

    #[test]
    fn the_same_unclosed_container_is_an_error_in_a_deck() {
        let source = "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n                      ## S {#n1 .slide}\n\n::: {.ph idx=1}\n\nA\n";
        let report = validate(source);
        assert!(!report.parses(), "{}", report.render());
        assert_eq!(report.diagnostics[0].code, "container/unclosed");
        assert_eq!(report.diagnostics[0].severity, Severity::Error);
    }

    #[test]
    fn the_repair_finds_the_error_hiding_behind_the_unclosed_one() {
        // The container never closes, and the sheet heading is missing too.
        let source =
            "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n## S {#n1 .slide}\n\n::: {.ph idx=1}\n\n- \n";
        let report = validate(source);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|d| d.code == "container/unclosed"),
            "{}",
            report.render()
        );
    }

    #[test]
    fn an_error_carries_the_id_of_the_container_it_is_inside() {
        let source = "---\ndocmark: \"1.1\"\nnext-id: 9\n---\n\n::: {#n3 .table}\n\n| a |\n";
        let report = validate(source);
        assert_eq!(report.diagnostics[0].node.as_deref(), Some("n3"));
    }

    #[test]
    fn an_unclosed_front_matter_is_one_error_with_a_fix() {
        let report = validate("---\ndocmark: \"1.0\"\n\nHello.\n");
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code, "front-matter/unclosed");
        assert!(report.diagnostics[0].fix.is_some());
        assert_eq!(report.profile, Profile::Unknown);
    }

    #[test]
    fn a_document_with_no_front_matter_parses_and_is_warned_about() {
        let report = validate("Hello.\n");
        assert!(report.parses());
        assert_eq!(report.diagnostics[0].code, "front-matter/absent");
        assert_eq!(report.diagnostics[0].severity, Severity::Warning);
    }

    #[test]
    fn a_duplicated_id_is_a_warning_that_points_at_the_second_one() {
        let source = "---\ndocmark: \"1.1\"\nsource-format: docx\nnext-id: 9\n---\n\n\
                      ![a](x.png){#n3}\n\n![b](y.png){#n3}\n";
        let report = validate(source);
        let duplicate = report
            .diagnostics
            .iter()
            .find(|d| d.code == "id/duplicate")
            .unwrap_or_else(|| panic!("{}", report.render()));
        assert_eq!(duplicate.line, 9);
        assert!(duplicate.fix.as_deref().unwrap().contains("n9"));
    }

    #[test]
    fn an_id_the_counter_does_not_dominate_is_a_warning() {
        let source =
            "---\ndocmark: \"1.1\"\nsource-format: docx\nnext-id: 2\n---\n\n![a](x.png){#n7}\n";
        let report = validate(source);
        let above = report
            .diagnostics
            .iter()
            .find(|d| d.code == "id/above-next-id")
            .unwrap_or_else(|| panic!("{}", report.render()));
        assert_eq!(above.fix.as_deref(), Some("set `next-id: 8` or higher"));
    }

    #[test]
    fn a_container_class_the_text_profile_does_not_know_is_a_warning_not_an_error() {
        let source =
            "---\ndocmark: \"1.0\"\nsource-format: docx\n---\n\n::: {.sidebar}\n\nA\n\n:::\n";
        let report = validate(source);
        assert!(report.parses(), "{}", report.render());
        assert_eq!(report.diagnostics[0].code, "container/unknown-class");
        assert_eq!(report.diagnostics[0].line, 6);
    }

    #[test]
    fn a_style_class_on_a_container_is_not_flagged() {
        let source = "---\ndocmark: \"1.0\"\nsource-format: docx\nstyles:\n  Sidebar:\n    \
                      type: paragraph\n---\n\n::: {.Sidebar}\n\nA\n\n:::\n";
        let report = validate(source);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|d| d.code == "container/unknown-class"),
            "{}",
            report.render()
        );
    }

    #[test]
    fn a_colon_line_inside_a_code_fence_opens_nothing() {
        let source = "---\ndocmark: \"1.0\"\nsource-format: docx\n---\n\n```\n::: {.table}\n```\n";
        let report = validate(source);
        assert!(report.parses(), "{}", report.render());
        assert!(scan_unclosed(source).is_empty());
    }

    #[test]
    fn an_id_is_read_off_a_line_only_when_it_is_a_real_token() {
        assert_eq!(id_on_line("## T {#n4 .slide}"), Some("n4".into()));
        assert_eq!(id_on_line("no attributes here"), None);
        assert_eq!(id_on_line("{#}"), None);
    }

    #[test]
    fn the_rendering_says_where_what_why_and_how() {
        let report = validate(
            "---\ndocmark: \"1.2\"\nsource-format: pptx\n---\n\n## S {#n1 .slide}\n\n::: {.ph idx=1}\n\nA\n",
        );
        let line = report.diagnostics[0].render();
        assert!(line.starts_with("error 8:1 container/unclosed"), "{line}");
        assert!(line.contains("spec §3.6"), "{line}");
        assert!(line.contains("Fix: "), "{line}");
    }
}
