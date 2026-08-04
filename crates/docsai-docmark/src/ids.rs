//! Node ids on the way out (DocMark 1.1, spec §11.1).
//!
//! The serializer, not the IR, decides which ids reach the file: a document
//! read from `.docx` has no ids at all, and one parsed back from DocMark
//! carries the ids it was written with. Allocation therefore happens here, in
//! writing order, and the IR is never mutated.
//!
//! Two rules keep the numbering stable across a round trip:
//!
//! 1. Every id already present is observed *before* anything is allocated, so a
//!    fresh id can never collide with one further down the document.
//! 2. Only nodes the format can actually carry take part
//!    ([`for_each_addressable`] enforces the same set), because an id that
//!    cannot be written would be different on every pass.

use std::collections::HashMap;

use docsai_model::addressing::{for_each_addressable, Addressable, Addressing, Etag, IdPolicy};
use docsai_model::{Document, NodeId, NodeKind};

/// The DocMark one addressable node contributed to the output.
///
/// This is what makes a node's cost *measurable* instead of estimated: the
/// fragment is the exact text the node put in the file, so counting its tokens
/// counts what a model would actually pay for it. Fragments nest — a section's
/// fragment contains its headings' — so they add up to more than the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeFragment {
    pub id: NodeId,
    pub kind: NodeKind,
    pub markdown: String,
    /// What the node's *content* hashes to (spec §11.2). Derived from the IR,
    /// not from the text above: it survives reflowing and changes when the
    /// content does, which is what makes it usable as an if-match precondition
    /// when an agent sends an edited selection back.
    pub etag: Etag,
    /// How many fragments this one contains, all of them immediately before it
    /// in the list. A node is recorded when it finishes, so everything written
    /// while it was open sits in `[index - descendants, index)` — that
    /// contiguous block is what turns the flat list back into a tree.
    pub descendants: usize,
}

/// Hands out the ids a serialisation run writes.
pub(crate) struct IdSource {
    policy: IdPolicy,
    addressing: Addressing,
    emitted: bool,
    /// Collected only when the caller asked for a traced serialisation.
    trace: Option<Vec<NodeFragment>>,
    /// The etag of every node that took an id, by id.
    ///
    /// Kept here because the two ends of a node are two different calls:
    /// [`IdSource::take`] sees the node, [`IdSource::record`] sees the text it
    /// wrote. Going through the id rather than through call order means the
    /// pairing cannot drift when a writer grows a new nesting level.
    etags: HashMap<String, Etag>,
}

impl IdSource {
    /// Starts from the document's counter, raised past every id already in it.
    pub(crate) fn new(doc: &Document, policy: IdPolicy) -> Self {
        let mut addressing = doc.addressing().clone();
        for_each_addressable(doc, &mut |node| {
            if let Some(id) = node.node_id() {
                addressing.observe(id);
            }
        });
        IdSource {
            policy,
            addressing,
            emitted: false,
            trace: None,
            etags: HashMap::new(),
        }
    }

    /// Same, but keeping the DocMark each addressable node writes.
    pub(crate) fn traced(doc: &Document, policy: IdPolicy) -> Self {
        IdSource {
            trace: Some(Vec::new()),
            ..IdSource::new(doc, policy)
        }
    }

    /// The id to write for `node`, if any: the one it already has, a freshly
    /// allocated one under [`IdPolicy::Assign`], or nothing at all.
    pub(crate) fn take(&mut self, node: &dyn Addressable) -> Option<String> {
        if !self.policy.emits() {
            return None;
        }
        let id = match node.node_id() {
            Some(id) => id.clone(),
            None if self.policy.assigns() => self.addressing.alloc(),
            None => return None,
        };
        self.emitted = true;
        // Only a traced run ever reads these, and hashing every node of an
        // untraced one would be work nobody asked for.
        if self.trace.is_some() {
            self.etags.insert(id.0.clone(), node.etag());
        }
        Some(id.0)
    }

    /// The `next-id` to record in the front matter, or `None` when the body
    /// carries no id and the document therefore stays DocMark 1.0.
    pub(crate) fn next_id(&self) -> Option<u64> {
        self.emitted.then_some(self.addressing.next_id)
    }

    /// Where the trace stands, to be handed back to [`IdSource::record`] when
    /// the node finishes: everything recorded in between is inside it.
    pub(crate) fn mark(&self) -> usize {
        self.trace.as_ref().map_or(0, Vec::len)
    }

    /// Records what `id` wrote. A no-op unless the run is traced, and unless
    /// the node took an id at all — a node without an address cannot be
    /// reported on.
    pub(crate) fn record(&mut self, id: Option<&str>, kind: NodeKind, markdown: &str, mark: usize) {
        let (true, Some(id)) = (self.trace.is_some(), id) else {
            return;
        };
        // An id that took part always has an etag; the fallback keeps the
        // invariant local instead of turning it into a panic.
        let etag = self.etags.get(id).cloned().unwrap_or_else(|| Etag::new(""));
        let trace = self.trace.as_mut().expect("checked above");
        trace.push(NodeFragment {
            id: NodeId::new(id),
            kind,
            markdown: markdown.to_string(),
            etag,
            descendants: trace.len().saturating_sub(mark),
        });
    }

    /// The fragments collected by a traced run, innermost first.
    pub(crate) fn into_fragments(self) -> Vec<NodeFragment> {
        self.trace.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_model::text::{Heading, Paragraph, Section, TextDocument};
    use docsai_model::NodeId;

    fn heading(id: Option<&str>) -> Heading {
        Heading {
            id: id.map(NodeId::new),
            level: 1,
            paragraph: Paragraph::text("t"),
        }
    }

    fn doc() -> Document {
        Document::Text(TextDocument {
            sections: vec![Section::default()],
            ..Default::default()
        })
    }

    #[test]
    fn assign_fills_the_gaps_and_preserves_what_is_there() {
        let mut ids = IdSource::new(&doc(), IdPolicy::Assign);
        assert_eq!(ids.take(&heading(Some("n9"))), Some("n9".into()));
        assert_eq!(ids.take(&heading(None)), Some("n1".into()));
        assert_eq!(ids.next_id(), Some(2));
    }

    #[test]
    fn preserve_writes_only_the_ids_the_document_already_had() {
        let mut ids = IdSource::new(&doc(), IdPolicy::Preserve);
        assert_eq!(ids.take(&heading(Some("n9"))), Some("n9".into()));
        assert_eq!(ids.take(&heading(None)), None);
    }

    #[test]
    fn never_emits_nothing_and_keeps_the_document_at_1_0() {
        let mut ids = IdSource::new(&doc(), IdPolicy::Never);
        assert_eq!(ids.take(&heading(Some("n9"))), None);
        assert_eq!(ids.next_id(), None);
    }

    #[test]
    fn existing_ids_are_observed_before_anything_is_allocated() {
        let doc = Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![docsai_model::text::Block::Heading(heading(Some("n7")))],
                ..Default::default()
            }],
            ..Default::default()
        });
        let mut ids = IdSource::new(&doc, IdPolicy::Assign);
        assert_eq!(
            ids.take(&heading(None)),
            Some("n8".into()),
            "a fresh id must not collide with n7 further down"
        );
    }
}
