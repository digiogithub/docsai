//! Stable node addressing: ids and etags (DocMark 1.1, spec §11.1).
//!
//! An agent edits a document by pointing at a node, so the pointer has to
//! outlive every edit: ids come from a **monotonic counter** stored in the
//! document ([`Addressing::next_id`]), are **never renumbered** when a sibling
//! is inserted and **never reused** after a deletion. A renumbered id makes an
//! agent silently edit the wrong node, which is why this is a correctness
//! feature and not a convenience.
//!
//! ```
//! use docsai_model::addressing::{Addressing, NodeId};
//!
//! let mut addressing = Addressing::default();
//! let first = addressing.alloc();
//! let second = addressing.alloc();
//! assert_eq!(first, NodeId::new("n1"));
//! assert_eq!(second, NodeId::new("n2"));
//! assert_eq!(addressing.next_id, 3);
//! ```
//!
//! The companion mechanism is the [`Etag`]: a short hash of the *normalised*
//! content of a node, used as an edit precondition. Normalisation excludes ids,
//! etags and formatting, so restyling a paragraph does not churn its etag.

pub mod walk;

pub use walk::{
    assign_ids, clear_ids, for_each_addressable, list_is_addressable, node_ids, observe_ids,
    paragraph_is_container, Addressable, NodeKind,
};

use serde::{Deserialize, Serialize};

/// Prefix of every allocated id. Ids are opaque: `s4`-style positional labels
/// are *selectors* (plan v2 Phase 11), not identities.
const ID_PREFIX: char = 'n';

/// A persistent identifier for an addressable node.
///
/// Serialised transparently, so `inspect --json` shows `"id": "n7"` rather than
/// a wrapper object.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        NodeId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when the id can be written as a DocMark `{#id}` token: ASCII
    /// alphanumerics plus `-`, `_` and `.`, never empty.
    ///
    /// ```
    /// # use docsai_model::addressing::NodeId;
    /// assert!(NodeId::new("n12").is_valid());
    /// assert!(NodeId::new("intro.title").is_valid());
    /// assert!(!NodeId::new("has space").is_valid());
    /// assert!(!NodeId::new("").is_valid());
    /// ```
    pub fn is_valid(&self) -> bool {
        !self.0.is_empty()
            && self
                .0
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    }

    /// The counter an allocated id came from, when it has the allocator's
    /// shape (`n42`). Ids written by hand return `None` and are simply
    /// preserved.
    pub fn counter(&self) -> Option<u64> {
        self.0.strip_prefix(ID_PREFIX)?.parse().ok()
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A 6-character content hash used as an edit precondition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Etag(pub String);

/// Number of hex characters in an [`Etag`] (spec §11.1).
pub const ETAG_LEN: usize = 6;

impl Etag {
    pub fn new(tag: impl Into<String>) -> Self {
        Etag(tag.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when the value has the canonical shape: [`ETAG_LEN`] lowercase hex
    /// digits.
    pub fn is_valid(&self) -> bool {
        self.0.len() == ETAG_LEN
            && self
                .0
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
    }
}

impl std::fmt::Display for Etag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a reader or writer does with node ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IdPolicy {
    /// Assign ids to addressable nodes that lack one, preserving those present.
    /// The default for the `full` fidelity level.
    #[default]
    Assign,
    /// Keep the ids already in the document, assign none.
    Preserve,
    /// Emit no ids at all; the DocMark 1.0 behaviour, and what `plain` uses.
    Never,
}

impl IdPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            IdPolicy::Assign => "assign",
            IdPolicy::Preserve => "preserve",
            IdPolicy::Never => "never",
        }
    }

    /// Parses an `--ids` value.
    pub fn parse(value: &str) -> Option<IdPolicy> {
        match value.trim().to_ascii_lowercase().as_str() {
            "assign" => Some(IdPolicy::Assign),
            "preserve" => Some(IdPolicy::Preserve),
            "never" => Some(IdPolicy::Never),
            _ => None,
        }
    }

    /// True when nodes without an id should get one.
    pub fn assigns(self) -> bool {
        self == IdPolicy::Assign
    }

    /// True when ids reach the output at all.
    pub fn emits(self) -> bool {
        self != IdPolicy::Never
    }
}

impl std::fmt::Display for IdPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The document-level id counter, serialised as `next-id` in the front matter.
///
/// It only ever grows. Deleting the node holding `n7` does not free `n7`, and
/// inserting a node before it does not shift it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct Addressing {
    /// The next counter value to hand out.
    pub next_id: u64,
}

impl Default for Addressing {
    fn default() -> Self {
        Addressing { next_id: 1 }
    }
}

impl Addressing {
    /// Hands out the next id.
    pub fn alloc(&mut self) -> NodeId {
        let id = NodeId(format!("{ID_PREFIX}{}", self.next_id));
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Raises the counter so `id` can never be handed out again.
    ///
    /// Called for every id read from a document, including ids written by
    /// hand: the counter must dominate everything already in the file.
    ///
    /// ```
    /// # use docsai_model::addressing::{Addressing, NodeId};
    /// let mut addressing = Addressing::default();
    /// addressing.observe(&NodeId::new("n40"));
    /// assert_eq!(addressing.alloc(), NodeId::new("n41"));
    /// ```
    pub fn observe(&mut self, id: &NodeId) {
        if let Some(counter) = id.counter() {
            self.next_id = self.next_id.max(counter.saturating_add(1));
        }
    }
}

/// Accumulates the normalised content of a node into an [`Etag`].
///
/// Callers feed only what identifies the node's *content*: its kind, its text
/// and its structure. Ids, etags and formatting are deliberately left out, so
/// bolding a word changes the etag (the text is the same, but the structure
/// is not) while restyling a whole paragraph does not.
///
/// ```
/// use docsai_model::addressing::EtagHasher;
///
/// let mut a = EtagHasher::new("paragraph");
/// a.text("Revenue up 12 %");
/// let mut b = EtagHasher::new("paragraph");
/// b.text("Revenue up 12 %");
/// assert_eq!(a.finish(), b.finish());
/// ```
#[derive(Debug, Clone)]
pub struct EtagHasher {
    state: u64,
}

/// FNV-1a 64-bit: no dependency, stable across platforms and releases, and the
/// output is truncated to 24 bits anyway. Etags are compared against the
/// previous value *of the same node*, so cross-node collisions are harmless.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

impl EtagHasher {
    /// Starts a hash for a node of the given kind.
    pub fn new(kind: &str) -> Self {
        let mut hasher = EtagHasher { state: FNV_OFFSET };
        hasher.token(kind);
        hasher
    }

    /// Mixes in a structural token (a node kind, a marker, a boundary).
    pub fn token(&mut self, token: &str) {
        self.write(token.as_bytes());
        self.write(b"\x1f");
    }

    /// Mixes in a run of user-visible text, with whitespace normalised: runs of
    /// whitespace collapse to one space and the ends are trimmed, so a
    /// re-flowed source that reads identically hashes identically.
    pub fn text(&mut self, text: &str) {
        let mut pending_space = false;
        let mut started = false;
        for ch in text.chars() {
            if ch.is_whitespace() {
                pending_space = started;
                continue;
            }
            if pending_space {
                self.write(b" ");
                pending_space = false;
            }
            let mut buf = [0u8; 4];
            self.write(ch.encode_utf8(&mut buf).as_bytes());
            started = true;
        }
    }

    /// Mixes in a number (a level, a span, a count).
    pub fn number(&mut self, value: u64) {
        self.token(&value.to_string());
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    /// The resulting etag: the low [`ETAG_LEN`] hex digits of the hash.
    pub fn finish(&self) -> Etag {
        Etag(format!(
            "{:0width$x}",
            self.state & 0x00ff_ffff,
            width = ETAG_LEN
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_monotonic_and_never_reused() {
        let mut addressing = Addressing::default();
        let ids: Vec<NodeId> = (0..5).map(|_| addressing.alloc()).collect();
        assert_eq!(ids[0], NodeId::new("n1"));
        assert_eq!(ids[4], NodeId::new("n5"));

        // Deleting a node does not free its id: the counter only grows.
        addressing.observe(&ids[0]);
        assert_eq!(addressing.alloc(), NodeId::new("n6"));
    }

    #[test]
    fn observing_a_higher_id_moves_the_counter_past_it() {
        let mut addressing = Addressing::default();
        addressing.observe(&NodeId::new("n99"));
        addressing.observe(&NodeId::new("n3")); // lower ids never lower it
        assert_eq!(addressing.alloc(), NodeId::new("n100"));
    }

    #[test]
    fn hand_written_ids_are_preserved_but_do_not_move_the_counter() {
        let mut addressing = Addressing::default();
        addressing.observe(&NodeId::new("intro"));
        assert_eq!(addressing.alloc(), NodeId::new("n1"));
        assert_eq!(NodeId::new("intro").counter(), None);
    }

    #[test]
    fn id_validity_matches_the_docmark_token_rules() {
        assert!(NodeId::new("n1").is_valid());
        assert!(NodeId::new("a-b_c.d").is_valid());
        assert!(!NodeId::new("").is_valid());
        assert!(!NodeId::new("a b").is_valid());
        assert!(!NodeId::new("a{b}").is_valid());
    }

    #[test]
    fn etags_have_the_canonical_shape() {
        let tag = EtagHasher::new("paragraph").finish();
        assert_eq!(tag.as_str().len(), ETAG_LEN);
        assert!(tag.is_valid(), "{tag} is not canonical");
        assert!(!Etag::new("ABC123").is_valid(), "hex is lowercase");
        assert!(!Etag::new("abc").is_valid(), "exactly six digits");
    }

    #[test]
    fn etag_ignores_whitespace_reflow_but_not_the_text() {
        let mut reflowed = EtagHasher::new("paragraph");
        reflowed.text("  Revenue up\n  12 %  ");
        let mut single_line = EtagHasher::new("paragraph");
        single_line.text("Revenue up 12 %");
        assert_eq!(reflowed.finish(), single_line.finish());

        let mut edited = EtagHasher::new("paragraph");
        edited.text("Revenue up 13 %");
        assert_ne!(edited.finish(), single_line.finish());
    }

    #[test]
    fn etag_depends_on_the_node_kind_and_structure() {
        let mut para = EtagHasher::new("paragraph");
        para.text("x");
        let mut heading = EtagHasher::new("heading");
        heading.text("x");
        assert_ne!(para.finish(), heading.finish());

        let mut one_run = EtagHasher::new("paragraph");
        one_run.text("ab");
        let mut two_runs = EtagHasher::new("paragraph");
        two_runs.text("a");
        two_runs.token("run");
        two_runs.text("b");
        assert_ne!(one_run.finish(), two_runs.finish());
    }

    #[test]
    fn id_policy_parses_its_cli_values() {
        assert_eq!(IdPolicy::parse("assign"), Some(IdPolicy::Assign));
        assert_eq!(IdPolicy::parse(" NEVER "), Some(IdPolicy::Never));
        assert_eq!(IdPolicy::parse("preserve"), Some(IdPolicy::Preserve));
        assert_eq!(IdPolicy::parse("sometimes"), None);
        assert!(IdPolicy::Assign.assigns() && IdPolicy::Assign.emits());
        assert!(!IdPolicy::Preserve.assigns() && IdPolicy::Preserve.emits());
        assert!(!IdPolicy::Never.emits());
    }
}
