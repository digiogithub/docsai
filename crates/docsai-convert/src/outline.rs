//! The map of a document: what is in it, where, and what each part costs
//! (plan v2, Phase 10).
//!
//! An agent that has to edit one paragraph of a 200-page report cannot afford
//! to read the report to find it. The outline is the cheap thing it reads
//! first: the tree of addressable nodes with their ids, kinds, a short preview
//! and the measured cost of each — which is what makes it possible to decide
//! *not* to read the rest. The phase's own acceptance criterion is that this
//! costs under 5 % of the document it describes.
//!
//! The tree is rebuilt from the flat fragment list of
//! [`docsai_docmark::serialize_traced`]: a fragment is recorded when its node
//! finishes, so everything written while it was open sits immediately before it
//! and is counted in `descendants`.

use std::path::Path;

use docsai_docmark::{serialize_traced, NodeFragment, Options as DocMarkOptions};
use docsai_model::assets::AssetStore;
use docsai_model::{Document, NodeId, NodeKind};
use serde::Serialize;

use crate::pipeline::ConvertOptions;
use crate::service::{with_scratch_document, SourceInput};
use crate::tokens::{count, preview, ENCODING};
use crate::ConvertError;

/// One node of the tree.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct OutlineNode {
    pub id: NodeId,
    pub kind: NodeKind,
    /// What this node costs, its children included.
    pub tokens: usize,
    pub preview: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<OutlineNode>,
}

impl OutlineNode {
    /// This node and everything under it.
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(OutlineNode::count).sum::<usize>()
    }
}

/// The addressable structure of a document.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Outline {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub source_format: String,
    pub fidelity: String,
    pub encoding: &'static str,
    /// What the whole document would cost, for comparison.
    pub document_tokens: usize,
    /// What this outline costs to read, in its text form.
    pub outline_tokens: usize,
    /// Levels kept, when the caller asked for fewer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<usize>,
    pub nodes: Vec<OutlineNode>,
}

impl Outline {
    /// Nodes at every level.
    pub fn len(&self) -> usize {
        self.nodes.iter().map(OutlineNode::count).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The form an agent reads, and the one `outline_tokens` measures.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        render_nodes(&self.nodes, 0, &mut out);
        out
    }
}

fn render_nodes(nodes: &[OutlineNode], depth: usize, out: &mut String) {
    for node in nodes {
        out.push_str(&format!(
            "{:indent$}{} {} {} {}\n",
            "",
            node.id.0,
            node.kind.as_str(),
            node.tokens,
            node.preview,
            indent = depth * 2
        ));
        render_nodes(&node.children, depth + 1, out);
    }
}

/// Outlines an in-memory document.
pub fn outline(
    doc: &Document,
    assets: &dyn AssetStore,
    options: &DocMarkOptions,
    depth: Option<usize>,
) -> Outline {
    let (markdown, _, fragments) = serialize_traced(doc, assets, options);
    let nodes = build(&fragments, 0, fragments.len(), depth);
    let mut outline = Outline {
        path: None,
        source_format: options.source_format.as_str().to_string(),
        fidelity: options.fidelity.as_str().to_string(),
        encoding: ENCODING,
        document_tokens: count(&markdown),
        outline_tokens: 0,
        depth,
        nodes,
    };
    outline.outline_tokens = count(&outline.render_text());
    outline
}

/// Reads `input` and outlines it.
pub fn outline_path(
    input: &Path,
    options: &ConvertOptions,
    depth: Option<usize>,
) -> Result<Outline, ConvertError> {
    outline_input(SourceInput::Path(input), options, depth)
}

/// Outlines a path or in-memory document (the MCP `outline_document` tool).
pub fn outline_input(
    source: SourceInput<'_>,
    options: &ConvertOptions,
    depth: Option<usize>,
) -> Result<Outline, ConvertError> {
    let (mut result, label) =
        with_scratch_document(source, options, true, |doc, assets, docmark| {
            outline(doc, assets, docmark, depth)
        })?;
    result.path = label;
    Ok(result)
}

/// Rebuilds the nodes of `fragments[start..end]` in document order.
///
/// Walking backwards is what makes this work: the last fragment of a range is
/// the outermost node that closed there, and its `descendants` say exactly how
/// many fragments before it belong to it.
fn build(
    fragments: &[NodeFragment],
    start: usize,
    end: usize,
    depth: Option<usize>,
) -> Vec<OutlineNode> {
    if depth == Some(0) {
        return Vec::new();
    }
    let mut nodes = Vec::new();
    let mut cursor = end;
    while cursor > start {
        let index = cursor - 1;
        let fragment = &fragments[index];
        let first_child = index.saturating_sub(fragment.descendants);
        nodes.push(OutlineNode {
            id: fragment.id.clone(),
            kind: fragment.kind,
            tokens: count(&fragment.markdown),
            preview: preview(&fragment.markdown),
            children: build(fragments, first_child, index, depth.map(|d| d - 1)),
        });
        cursor = first_child;
    }
    nodes.reverse();
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use docsai_docmark::Fidelity;
    use docsai_model::text::{Block, Heading, Paragraph, Section, TextDocument};
    use docsai_model::MemoryAssetStore;

    fn doc() -> Document {
        Document::Text(TextDocument {
            sections: vec![Section {
                blocks: vec![
                    Block::Heading(Heading {
                        id: None,
                        level: 1,
                        paragraph: Paragraph::text("Title"),
                    }),
                    Block::Heading(Heading {
                        id: None,
                        level: 2,
                        paragraph: Paragraph::text("Sub"),
                    }),
                ],
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    #[test]
    fn headings_of_one_section_are_siblings_in_document_order() {
        let outline = outline(
            &doc(),
            &MemoryAssetStore::new(),
            &DocMarkOptions::default(),
            None,
        );
        let ids: Vec<&str> = outline.nodes.iter().map(|n| n.id.0.as_str()).collect();
        assert_eq!(ids, vec!["n1", "n2"]);
        assert!(outline.outline_tokens < outline.document_tokens);
    }

    #[test]
    fn a_lossy_level_has_nothing_to_outline() {
        let options = DocMarkOptions {
            fidelity: Fidelity::Plain,
            ..Default::default()
        };
        let outline = outline(&doc(), &MemoryAssetStore::new(), &options, None);
        assert!(outline.is_empty(), "plain carries no ids to address");
    }
}
