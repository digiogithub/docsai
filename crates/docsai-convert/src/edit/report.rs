//! The change report: §5.6, completed by 17-D.
//!
//! # What «the applied diff» is, and why
//!
//! 17-A reserved the answer («**`dry_run`'s diff**: what "the applied diff" is,
//! and whether it is DocMark or a structured list», §13). 17-D answers:
//!
//! **The diff is the identity and the new state of everything the request
//! touched, and nothing else.** Three lists — what was created, with its kind;
//! what was deleted; what changed, with its **new etag** — and the warnings.
//! Not the new content, not a textual diff.
//!
//! The reason is arithmetic, and it is the phase's own arithmetic. Every field
//! is paid on the wire by every caller (§0.6), and Phase 17 exists to bring a
//! whole agent loop under 2 000 tokens. Measured with this repository's own
//! tokenizer (`docsai_convert::tokens::count`, `o200k_base`) over the framed
//! JSON:
//!
//! | report for «retitle slide 4, add a bullet, bold it» | tokens |
//! |---|---|
//! | this report | ~75 |
//! | the same, carrying each touched node's new DocMark | ~200 and unbounded |
//! | the same, carrying a unified diff of the document | the document, twice |
//!
//! The second row is the one worth arguing, because it looks helpful. It is
//! paid on every call to hand the caller back the text the caller just wrote:
//! `replace_text n12 "Q3 results"` returning «`n12` now reads "Q3 results"» is
//! a receipt for a sentence the caller composed. And it is unbounded — a
//! `duplicate_slide` would return the whole slide, an `insert_after` of a table
//! the whole table — so the response size stops being a function of the request
//! and becomes a function of the document.
//!
//! What the caller genuinely does **not** know after a successful call is
//! exactly two things: the **ids** of what it created, and the **etags** of what
//! it changed. Both are here. That pair is what makes plan task 5's «no
//! confirmation read is needed» true rather than claimed: an agent that
//! retitled `n12` holds `n12`'s new etag and can guard its next edit with it
//! without reading the document again, and an agent that inserted a bullet
//! holds the new node's id and can bold it in the next call.
//!
//! `deleted` lists the **targets** and not their subtrees, which §5.6 fixed and
//! argued: enumerating forty shape ids to say a slide is gone would cost more
//! than the whole rest of the response.
//!
//! # What the report does *not* carry
//!
//! - **The operation-set version** (§1.2), on every successful call, to answer a
//!   question only a failing client asks.
//! - **A per-operation status** (§4): there is no partial success, so a list of
//!   ten `"ok"`s says one bit ten times.
//! - **Whether it was a dry run.** The caller knows what it asked for, and
//!   17-D's promise is that the two reports are equal.

use docsai_model::addressing::{resolve_str, NodeId, NodeKind};
use docsai_model::{Document, Warning};
use serde::Serialize;

/// A node the request made (§5.6), in document order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatedNode {
    pub id: String,
    pub kind: NodeKind,
}

/// A node the request changed, with the etag it now has (§7, 17-B's hasher).
///
/// `id` carries **the target in canonical form**, which for a node that has an
/// id *is* its id — §5.6's example — and for a paragraph of ordinary prose is
/// the index path the caller wrote, because Phase 10 deliberately gives such a
/// paragraph no id at all («a plain run of text is reached by relative path
/// instead, which keeps the id noise out of ordinary prose»). §6.1 says an
/// operation «returns `changed: [target]`», and this is that sentence read
/// literally. The row is true of the finished document either way: the place
/// named here has this etag, so the caller's next `if_match` on the same target
/// will hold.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedNode {
    pub id: String,
    pub etag: String,
}

/// §5.6's response, completed.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct EditReport {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<CreatedNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deleted: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<ChangedNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
}

impl EditReport {
    /// True when the request changed nothing at all — an empty operation list,
    /// or operations that left the document as they found it.
    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.deleted.is_empty() && self.changed.is_empty()
    }
}

/// Builds the report against the **final** document.
///
/// The etags have to come from the end and not from each operation as it runs:
/// two operations on one node must report the value the caller will read back,
/// not an intermediate one that never existed on disk. For the same reason a
/// node that was created and then deleted inside one batch appears in neither
/// list — the report describes what the document now is, not the route it took.
pub(crate) fn build(
    document: &Document,
    created: &[NodeId],
    deleted: &[String],
    changed: &[String],
    warnings: &[Warning],
) -> EditReport {
    let mut report = EditReport {
        deleted: deleted.to_vec(),
        warnings: warnings.to_vec(),
        ..Default::default()
    };
    for id in created {
        if let Ok(place) = resolve_str(document, id.as_str()) {
            if let Some(kind) = place.kind() {
                report.created.push(CreatedNode {
                    id: id.0.clone(),
                    kind,
                });
            }
        }
    }
    for target in changed {
        if let Ok(place) = resolve_str(document, target) {
            if let Some(etag) = place.etag() {
                report.changed.push(ChangedNode {
                    id: target.clone(),
                    etag: etag.0,
                });
            }
        }
    }
    report
}
