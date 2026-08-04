//! Reading *part* of a document (plan v2, Phase 11, spec §2.1).
//!
//! `docsai outline` tells an agent where the paragraph it has to edit is.
//! This is what it calls next: give me those nodes and nothing else, as valid
//! self-contained DocMark it can parse, edit and write back.
//!
//! Two decisions shape the module:
//!
//! - The body is **stitched from the fragments of a traced serialisation**, so
//!   a selected node's DocMark is byte for byte what the whole document wrote
//!   for it. Nothing is re-derived, so nothing can drift.
//! - The front matter is the **minimum** ([`docsai_docmark::selection_front_matter`]):
//!   a caller that declined to read the document did not ask for its metadata,
//!   its page geometry or its catalogues either. What it does carry is what a
//!   write-back needs — the id counter, `partial: true`, and an etag per node.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use docsai_docmark::{
    selection_front_matter, serialize_traced, NodeFragment, Options as DocMarkOptions,
};
use docsai_model::addressing::Etag;
use docsai_model::assets::AssetStore;
use docsai_model::{Document, NodeId, NodeKind};
use serde::Serialize;

use crate::assets::DirAssetStore;
use crate::pipeline::{read_path_with_options, ConvertOptions};
use crate::tokens::{count, preview, strip_machinery};
use crate::ConvertError;

/// The kinds a `type:` term may name.
///
/// [`NodeKind::Footnote`] is deliberately absent, and so is a footnote from any
/// selection: a footnote is addressed at its *reference*, which lives inside a
/// paragraph, while its text is written at the foot of the document. Handed out
/// on its own it is a definition nothing refers to — not a standalone document,
/// which is the one thing a selection has to be. Select the block carrying the
/// reference and the footnote comes with it.
const KINDS: &[NodeKind] = &[
    NodeKind::Section,
    NodeKind::Heading,
    NodeKind::Paragraph,
    NodeKind::List,
    NodeKind::Table,
    NodeKind::TableRow,
    NodeKind::Image,
    NodeKind::Sheet,
];

/// What a selector could not be read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorError(String);

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SelectorError {}

/// One term of a selector.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Term {
    /// `s4`, or `s7-s9`: positions in document order, 1-based, inclusive.
    Range(usize, usize),
    /// `#n7`.
    Id(NodeId),
    /// `type:heading`.
    Kind(NodeKind),
    /// `text:foo`, matched case-insensitively against the node's text.
    Text(String),
}

/// A parsed `--select` expression: comma-separated terms, unioned.
///
/// Order is never the selector's: the output is always in document order, so
/// `#n9,#n2` and `#n2,#n9` produce the same file. A selection is a *reading* of
/// the document, and reading it out of order would make it a different one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selector {
    terms: Vec<Term>,
    source: String,
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl FromStr for Selector {
    type Err = SelectorError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut terms = Vec::new();
        for part in text.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            terms.push(term(part)?);
        }
        if terms.is_empty() {
            return Err(SelectorError(
                "empty selector; try `s1`, `s7-s9`, `#n7`, `type:heading` or `text:revenue`".into(),
            ));
        }
        Ok(Selector {
            terms,
            source: text.trim().to_string(),
        })
    }
}

fn term(part: &str) -> Result<Term, SelectorError> {
    if let Some(id) = part.strip_prefix('#') {
        if id.is_empty() {
            return Err(SelectorError("`#` needs a node id, like `#n7`".into()));
        }
        return Ok(Term::Id(NodeId::new(id)));
    }
    if let Some(kind) = part.strip_prefix("type:") {
        let kind = kind.trim();
        if kind == NodeKind::Footnote.as_str() {
            return Err(SelectorError(
                "a footnote cannot be selected on its own: it is addressed at its reference, \
                 inside a block. Select that block and the footnote comes with it"
                    .into(),
            ));
        }
        return KINDS
            .iter()
            .copied()
            .find(|k| k.as_str() == kind)
            .map(Term::Kind)
            .ok_or_else(|| {
                let known: Vec<&str> = KINDS.iter().map(|k| k.as_str()).collect();
                SelectorError(format!(
                    "unknown node kind `{kind}`; this document addresses {}",
                    known.join(", ")
                ))
            });
    }
    if let Some(text) = part.strip_prefix("text:") {
        let text = text.trim();
        if text.is_empty() {
            return Err(SelectorError("`text:` needs something to look for".into()));
        }
        return Ok(Term::Text(text.to_lowercase()));
    }
    if let Some(rest) = part.strip_prefix('s') {
        // `s7-s9` and `s7-9` mean the same thing; both read naturally and
        // refusing one would only be a rule to remember.
        let (from, to) = match rest.split_once('-') {
            Some((from, to)) => (from, to.trim().trim_start_matches('s')),
            None => (rest, rest),
        };
        let position = |value: &str| {
            value
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| {
                    SelectorError(format!(
                        "`{part}` is not a position; positions are 1-based, as `outline` prints them"
                    ))
                })
        };
        let (from, to) = (position(from)?, position(to)?);
        if to < from {
            return Err(SelectorError(format!("`{part}` runs backwards")));
        }
        return Ok(Term::Range(from, to));
    }
    Err(SelectorError(format!(
        "`{part}` is not a selector term; use `s4`, `s7-s9`, `#n7`, `type:heading` or `text:foo`"
    )))
}

/// One node of a selection, for `--json`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct SelectedNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub etag: Etag,
    pub tokens: usize,
    pub preview: String,
}

/// A partial document, ready to be written out.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Selection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub selector: String,
    pub source_format: String,
    pub fidelity: String,
    /// The DocMark of the selection, front matter included.
    pub docmark: String,
    /// What the selection costs, and what the whole document would have cost.
    pub tokens: usize,
    pub document_tokens: usize,
    /// The nodes it names, in document order. Nodes *inside* them are in the
    /// text but not listed: the selection addresses what was asked for.
    pub nodes: Vec<SelectedNode>,
}

impl Selection {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Selects from an in-memory document.
///
/// `options.ids` decides what can be selected at all, exactly as it does for
/// [`crate::outline`]: a level that writes no id addresses nothing, so a
/// selector there matches nothing rather than something `outline` never showed.
/// `options.dictionary` is overridden — the selection must depend on nothing
/// outside itself.
pub fn select(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &DocMarkOptions,
    selector: &Selector,
) -> Selection {
    let options = DocMarkOptions {
        dictionary: false,
        ..options.clone()
    };
    let (markdown, _, fragments) = serialize_traced(doc, assets, &options);

    let order = document_order(&fragments);
    let chosen = choose(&fragments, &order, selector);

    let mut body = String::new();
    let mut nodes = Vec::new();
    let mut etags: Vec<(NodeId, Etag)> = Vec::new();
    for index in &chosen {
        let fragment = &fragments[*index];
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(fragment.markdown.trim_end_matches('\n'));
        nodes.push(SelectedNode {
            id: fragment.id.clone(),
            kind: fragment.kind,
            etag: fragment.etag.clone(),
            tokens: count(&fragment.markdown),
            preview: preview(&fragment.markdown),
        });
        // Every node the body carries, not only the ones named: a re-write of
        // the selection recomputes the map from what is in it, and the two have
        // to agree.
        let first = index.saturating_sub(fragment.descendants);
        for inner in &fragments[first..=*index] {
            etags.push((inner.id.clone(), inner.etag.clone()));
        }
    }
    // A footnote reference in the body needs its definition, which the document
    // writes at its foot — outside every fragment that refers to it. Without
    // this the selection would carry a marker pointing at nothing, which is
    // exactly the silent loss the project refuses.
    for fragment in &fragments {
        if fragment.kind != NodeKind::Footnote {
            continue;
        }
        if !body.contains(&format!("{{#{}}}", fragment.id.0)) {
            continue;
        }
        body.push_str("\n\n");
        body.push_str(fragment.markdown.trim_end_matches('\n'));
        etags.push((fragment.id.clone(), fragment.etag.clone()));
    }
    if !body.is_empty() {
        body.push('\n');
    }
    sort_by_id(&mut etags);

    let front_matter = if body.is_empty() {
        String::new()
    } else {
        selection_front_matter(&options, next_id(doc, &fragments), &etags)
    };
    let docmark = format!("{front_matter}{body}");

    Selection {
        path: None,
        selector: selector.to_string(),
        source_format: options.source_format.as_str().to_string(),
        fidelity: options.fidelity.as_str().to_string(),
        tokens: count(&docmark),
        document_tokens: count(&markdown),
        docmark,
        nodes,
    }
}

/// Reads `input` and selects from it.
pub fn select_path(
    input: &Path,
    options: &ConvertOptions,
    selector: &Selector,
) -> Result<Selection, ConvertError> {
    // As for `outline`: the image links are written, the bytes are not.
    let dir = tempfile::tempdir().map_err(|source| ConvertError::Io {
        path: input.display().to_string(),
        source,
    })?;
    let mut assets = DirAssetStore::new(dir.path());
    let (doc, source_format, _) = read_path_with_options(input, &mut assets, options)?;
    let docmark = DocMarkOptions {
        fidelity: options.fidelity,
        ids: options.id_policy(),
        assets_dir: "assets".into(),
        source_format,
        raw: options.raw,
        precision: options.precision,
        dictionary: false,
    };
    let mut selection = select(&doc, &assets, &docmark, selector);
    selection.path = Some(input.display().to_string());
    Ok(selection)
}

/// Fragment indices in document order.
///
/// The trace is in *finishing* order — a node arrives after everything inside
/// it — so document order is recovered the way [`crate::outline`] rebuilds the
/// tree: walking a range backwards, the last fragment is the outermost node,
/// and its `descendants` say how many before it are its own.
fn document_order(fragments: &[NodeFragment]) -> Vec<usize> {
    let mut order = Vec::with_capacity(fragments.len());
    walk(fragments, 0, fragments.len(), &mut order);
    order
}

fn walk(fragments: &[NodeFragment], start: usize, end: usize, out: &mut Vec<usize>) {
    let mut roots = Vec::new();
    let mut cursor = end;
    while cursor > start {
        let index = cursor - 1;
        let first_child = index.saturating_sub(fragments[index].descendants);
        roots.push((index, first_child));
        cursor = first_child;
    }
    for (index, first_child) in roots.into_iter().rev() {
        out.push(index);
        walk(fragments, first_child, index, out);
    }
}

/// The fragments a selector names, in document order, without any node that is
/// already inside another one: its text is there either way, and naming it
/// twice would write it twice.
fn choose(fragments: &[NodeFragment], order: &[usize], selector: &Selector) -> Vec<usize> {
    let mut wanted = vec![false; fragments.len()];
    for (position, index) in order.iter().enumerate() {
        let fragment = &fragments[*index];
        let position = position + 1;
        if selector.terms.iter().any(|term| match term {
            Term::Range(from, to) => position >= *from && position <= *to,
            Term::Id(id) => *id == fragment.id,
            Term::Kind(kind) => *kind == fragment.kind,
            Term::Text(needle) => strip_machinery(&fragment.markdown)
                .to_lowercase()
                .contains(needle),
        }) {
            wanted[*index] = true;
        }
    }

    let mut chosen = Vec::new();
    for index in order {
        // A footnote is never a selection of its own (see `KINDS`), whichever
        // term reached it.
        if !wanted[*index] || fragments[*index].kind == NodeKind::Footnote {
            continue;
        }
        let first_child = index.saturating_sub(fragments[*index].descendants);
        let contained = chosen.iter().any(|outer: &usize| {
            let outer_first = outer.saturating_sub(fragments[*outer].descendants);
            outer_first <= first_child && *index <= *outer
        });
        if !contained {
            chosen.push(*index);
        }
    }
    chosen
}

/// The counter the selection declares: the source document's, so a node added
/// to the selection can never collide with one left behind in the document.
fn next_id(doc: &Document, fragments: &[NodeFragment]) -> u64 {
    let mut next = doc.addressing().next_id;
    for fragment in fragments {
        if let Some(number) = fragment
            .id
            .0
            .strip_prefix('n')
            .and_then(|n| n.parse::<u64>().ok())
        {
            next = next.max(number + 1);
        }
    }
    next
}

/// Id order, by the counter the id encodes: `n9` before `n10`.
fn sort_by_id(etags: &mut Vec<(NodeId, Etag)>) {
    etags.sort_by(|(a, _), (b, _)| {
        let key = |id: &NodeId| {
            id.0.strip_prefix('n')
                .and_then(|n| n.parse::<u64>().ok())
                .map(|n| (0, n, String::new()))
                .unwrap_or((1, 0, id.0.clone()))
        };
        key(a).cmp(&key(b))
    });
    etags.dedup_by(|(a, _), (b, _)| a == b);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_terms_read_as_they_are_written() {
        let selector: Selector = "s4, s7-s9, #n2, type:heading, text:Revenue"
            .parse()
            .unwrap();
        assert_eq!(
            selector.terms,
            vec![
                Term::Range(4, 4),
                Term::Range(7, 9),
                Term::Id(NodeId::new("n2")),
                Term::Kind(NodeKind::Heading),
                Term::Text("revenue".into()),
            ]
        );
        assert_eq!(
            "s7-9".parse::<Selector>().unwrap().terms,
            vec![Term::Range(7, 9)]
        );
    }

    #[test]
    fn a_selector_that_cannot_be_read_says_what_it_expected() {
        for bad in ["", "s0", "s9-s7", "#", "type:notes", "text:", "paragraph"] {
            let error = bad.parse::<Selector>().unwrap_err().to_string();
            assert!(!error.is_empty(), "{bad} produced no message");
        }
        let error = "type:notes".parse::<Selector>().unwrap_err().to_string();
        assert!(
            error.contains("heading"),
            "an unknown kind must list the known ones: {error}"
        );
    }
}
