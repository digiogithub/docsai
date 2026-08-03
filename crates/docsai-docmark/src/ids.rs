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

use docsai_model::addressing::{for_each_addressable, Addressable, Addressing, IdPolicy};
use docsai_model::Document;

/// Hands out the ids a serialisation run writes.
pub(crate) struct IdSource {
    policy: IdPolicy,
    addressing: Addressing,
    emitted: bool,
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
        Some(id.0)
    }

    /// The `next-id` to record in the front matter, or `None` when the body
    /// carries no id and the document therefore stays DocMark 1.0.
    pub(crate) fn next_id(&self) -> Option<u64> {
        self.emitted.then_some(self.addressing.next_id)
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
