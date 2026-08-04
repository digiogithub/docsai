//! `--fidelity agent` (plan v2 Phase 11, increment B).
//!
//! The level is a *projection*: it is read whole and written back node by
//! node. Two things therefore have to hold, and neither is about bytes.
//!
//! 1. It loses no content. Every word the document says at `full`, it says at
//!    `agent` — what it drops is appearance.
//! 2. It addresses exactly what `full` addresses. A node an agent cannot name
//!    is a node it cannot write back, so a projection with fewer ids than the
//!    document would be a projection you cannot edit from.
//!
//! The third claim — that it is cheap — is measured in `corpus/token-budget.md`
//! and bounded here against the one number that cannot be argued with: what
//! the same document costs at `plain`, which is its content and nothing else.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use docsai_convert::{token_report_path, AssetMode, ConvertOptions, Fidelity, SourceInput};
use docsai_docmark::{
    parse_with_base, serialize, Fidelity as DocFidelity, Options as DocMarkOptions,
};
use docsai_model::{Document, MemoryAssetStore};

/// How much of the overhead `full` pays over `plain` a projection may keep.
/// Measured: the corpus worst case is 45 %, on the one document whose cost is
/// its prose rather than its formatting.
const MAX_OVERHEAD_KEPT: f64 = 50.0;

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// The text corpus: spreadsheets are excluded on purpose, their cost is data
/// (values, formulas, merges) that `agent` has to keep, not formatting.
fn text_documents() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for subdir in ["docx", "odt"] {
        let dir = corpus_root().join(subdir);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.ends_with(".expected.dmk.md") {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some(subdir) {
                out.push((format!("{subdir}/{name}"), path));
            }
        }
    }
    out.sort();
    assert!(!out.is_empty(), "the corpus should not be empty");
    out
}

/// Converts into a directory of its own, so raw sidecars are on disk and the
/// result can be parsed back the way a reader would find it.
fn render(path: &Path, fidelity: Fidelity) -> (String, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("a working directory");
    let options = ConvertOptions {
        fidelity,
        ..Default::default()
    };
    let result = docsai_convert::convert_to_markdown(
        SourceInput::Path(path),
        &options,
        AssetMode::Files {
            dir: Some(dir.path().join("assets")),
        },
    )
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (result.markdown, dir)
}

/// The `{#id}` addresses a serialisation handed out, in document order.
fn ids(markdown: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = markdown.as_bytes();
    for (index, _) in markdown.match_indices("{#") {
        let start = index + 2;
        let end = bytes[start..]
            .iter()
            .position(|c| !c.is_ascii_alphanumeric() && *c != b'-' && *c != b'_')
            .map(|offset| start + offset)
            .unwrap_or(markdown.len());
        out.insert(markdown[start..end].to_string());
    }
    // Lists carry their address as `list-id=` — the attribute block of a
    // Markdown list is its first item's, and an `#id` there would name the
    // item, not the list (spec §3.3).
    for (index, _) in markdown.match_indices("list-id=") {
        let start = index + "list-id=".len();
        let end = bytes[start..]
            .iter()
            .position(|c| !c.is_ascii_alphanumeric() && *c != b'-' && *c != b'_')
            .map(|offset| start + offset)
            .unwrap_or(markdown.len());
        out.insert(markdown[start..end].to_string());
    }
    out
}

/// What a DocMark document says with nothing about how it says it.
fn content(path: &Path, fidelity: Fidelity) -> String {
    let (markdown, dir) = render(path, fidelity);
    let mut assets = MemoryAssetStore::new();
    let (document, _) = parse_with_base(&markdown, Some(dir.path()), &mut assets)
        .unwrap_or_else(|e| panic!("{}: the projection has to re-parse: {e}", path.display()));
    plain(&document, &assets)
}

fn plain(document: &Document, assets: &MemoryAssetStore) -> String {
    let options = DocMarkOptions {
        fidelity: DocFidelity::Plain,
        ..DocMarkOptions::default()
    };
    serialize(document, assets, &options).0
}

#[test]
fn the_projection_says_everything_the_document_says() {
    for (name, path) in text_documents() {
        let full = content(&path, Fidelity::Full);
        let agent = content(&path, Fidelity::Agent);
        assert_eq!(
            agent, full,
            "{name}: the `agent` projection lost content, not appearance"
        );
    }
}

#[test]
fn the_projection_addresses_what_the_document_addresses() {
    for (name, path) in text_documents() {
        let full = ids(&render(&path, Fidelity::Full).0);
        let agent = ids(&render(&path, Fidelity::Agent).0);
        assert_eq!(
            agent, full,
            "{name}: a node addressed at `full` and not at `agent` cannot be written back"
        );
    }
}

#[test]
fn the_projection_never_pays_for_what_it_cannot_edit() {
    for (name, path) in text_documents() {
        let cost = |fidelity| {
            let options = ConvertOptions {
                fidelity,
                ..Default::default()
            };
            token_report_path(&path, &options)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .total as f64
        };
        let full = cost(Fidelity::Full);
        let agent = cost(Fidelity::Agent);
        let plain = cost(Fidelity::Plain);

        assert!(
            agent < full,
            "{name}: the projection costs {agent} against {full} at `full`"
        );
        // Everything above `plain` is what the reader pays for form rather
        // than for content. `agent` may keep some of it — ids and the stubs
        // are real value — but not most of it.
        let kept = (agent - plain) * 100.0 / (full - plain);
        assert!(
            kept <= MAX_OVERHEAD_KEPT,
            "{name}: `agent` keeps {kept:.0} % of the overhead `full` pays over `plain` \
             ({plain} → {agent} → {full} tokens)"
        );
    }
}

#[test]
fn the_projection_declares_itself() {
    let (_, path) = text_documents().into_iter().next().expect("a document");
    let agent = render(&path, Fidelity::Agent).0;
    assert!(
        agent.contains("\nfidelity: agent\n"),
        "a projection has to say it is one, or it will be written back whole:\n{}",
        &agent[..agent.len().min(400)]
    );
    assert!(
        !render(&path, Fidelity::Full).0.contains("fidelity:"),
        "the document itself is not a projection and says nothing"
    );
}
