//! The request and the refusals: `docs/edit-operations.md` §3, §6 and §8.
//!
//! These types are the **wire shape** of operation-set version 1, and they are
//! here rather than behind a door because 17-E's rule is that neither door may
//! carry logic the other lacks. A CLI `--ops` file and an MCP `apply_edits`
//! argument deserialize into the same `Operation`, so a field one accepts and
//! the other does not cannot exist.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

/// The operation-set version this build speaks (§1).
pub const OPS_VERSION: u32 = 1;

/// A JSON scalar, which is every value the set's fields can take.
///
/// Hand-written rather than `serde_json::Value` so that this crate does not
/// take a JSON dependency to describe a spreadsheet cell, and so that the type
/// says out loud that an operation never carries a nested object or an array.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    Bool(bool),
    /// Integers stay integers: a cell holding `1200` must not come back
    /// `1200.0`, which is what a single `f64` arm would do to it.
    Int(i64),
    Float(f64),
    Text(String),
}

impl std::fmt::Display for Scalar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scalar::Bool(v) => write!(f, "{v}"),
            Scalar::Int(v) => write!(f, "{v}"),
            Scalar::Float(v) => write!(f, "{v}"),
            Scalar::Text(v) => f.write_str(v),
        }
    }
}

/// Distinguishes «the key was absent» from «the key was `null`».
///
/// §6.2 rests on that distinction — «`null` clears a property to whatever the
/// cascade implies; an absent key leaves it alone» — and so does §6.7's
/// `value: null`. Serde folds both onto `None` unless asked not to.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer).map(Some)
}

/// One operation of §6. The tag is the `op` field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    ReplaceText {
        target: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    SetProps {
        target: String,
        /// The closed set of §6.2. An unknown key is `UnknownField` and never
        /// an ignored key; `null` clears.
        props: BTreeMap<String, Option<Scalar>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    InsertAfter {
        target: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    InsertBefore {
        target: String,
        content: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    Delete {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    #[serde(rename = "move")]
    Move {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    DuplicateSlide {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    SetCell {
        target: String,
        cell: String,
        #[serde(default, deserialize_with = "present")]
        value: Option<Option<Scalar>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    SetNotes {
        target: String,
        #[serde(default, deserialize_with = "present")]
        content: Option<Option<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
    ReplaceImage {
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_base64: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_match: Option<String>,
    },
}

impl Operation {
    /// The `op` name, as §1.3 writes it.
    pub fn name(&self) -> &'static str {
        match self {
            Operation::ReplaceText { .. } => "replace_text",
            Operation::SetProps { .. } => "set_props",
            Operation::InsertAfter { .. } => "insert_after",
            Operation::InsertBefore { .. } => "insert_before",
            Operation::Delete { .. } => "delete",
            Operation::Move { .. } => "move",
            Operation::DuplicateSlide { .. } => "duplicate_slide",
            Operation::SetCell { .. } => "set_cell",
            Operation::SetNotes { .. } => "set_notes",
            Operation::ReplaceImage { .. } => "replace_image",
        }
    }

    /// The target string, before it is parsed.
    pub fn target(&self) -> &str {
        match self {
            Operation::ReplaceText { target, .. }
            | Operation::SetProps { target, .. }
            | Operation::InsertAfter { target, .. }
            | Operation::InsertBefore { target, .. }
            | Operation::Delete { target, .. }
            | Operation::Move { target, .. }
            | Operation::DuplicateSlide { target, .. }
            | Operation::SetCell { target, .. }
            | Operation::SetNotes { target, .. }
            | Operation::ReplaceImage { target, .. } => target,
        }
    }

    /// The `if_match` precondition, if the caller sent one (§7).
    pub fn if_match(&self) -> Option<&str> {
        match self {
            Operation::ReplaceText { if_match, .. }
            | Operation::SetProps { if_match, .. }
            | Operation::InsertAfter { if_match, .. }
            | Operation::InsertBefore { if_match, .. }
            | Operation::Delete { if_match, .. }
            | Operation::Move { if_match, .. }
            | Operation::DuplicateSlide { if_match, .. }
            | Operation::SetCell { if_match, .. }
            | Operation::SetNotes { if_match, .. }
            | Operation::ReplaceImage { if_match, .. } => if_match.as_deref(),
        }
    }
}

/// §3's request: an ordered list of operations, and two fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EditRequest {
    /// The operation-set version the caller wrote against. Absent means
    /// «whatever this server speaks» (§1.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ops: Option<u32>,
    pub operations: Vec<Operation>,
    /// 17-D. Everything except the replacement of the destination file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
}

impl EditRequest {
    /// A request that applies `operations` and writes.
    pub fn new(operations: Vec<Operation>) -> Self {
        EditRequest {
            ops: None,
            operations,
            dry_run: false,
        }
    }

    /// The same request, run to the edge of the write and no further (17-D).
    pub fn dry(mut self) -> Self {
        self.dry_run = true;
        self
    }
}

/// The four document classes of §2, as a refusal names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocClass {
    Text,
    Workbook,
    Presentation,
}

impl DocClass {
    pub fn of(doc: &docsai_model::Document) -> DocClass {
        match doc {
            docsai_model::Document::Text(_) => DocClass::Text,
            docsai_model::Document::Workbook(_) => DocClass::Workbook,
            docsai_model::Document::Presentation(_) => DocClass::Presentation,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DocClass::Text => "text",
            DocClass::Workbook => "workbook",
            DocClass::Presentation => "presentation",
        }
    }
}

impl std::fmt::Display for DocClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// §8's typed refusals. Every one names both sides of the mismatch.
///
/// The set is closed with the operation set: a new variant that refines an
/// existing case costs no version bump (§1.1), a new *case* is a new version.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Refusal {
    #[error("`{op}` is not an operation of set version {speaks}")]
    UnknownOperation { op: String, speaks: u32 },

    #[error("this build speaks operation set {speaks}; the request asks for {requested}")]
    UnsupportedOpsVersion { requested: u32, speaks: u32 },

    #[error("`{op}` has no field `{field}`; it takes {known}")]
    UnknownField {
        op: &'static str,
        field: String,
        known: &'static str,
    },

    #[error("`{op}` requires `{field}`")]
    MissingField {
        op: &'static str,
        field: &'static str,
    },

    #[error("malformed target `{target}`: {reason}")]
    MalformedTarget { target: String, reason: String },

    #[error("{reason}")]
    UnknownTarget { target: String, reason: String },

    #[error("`{op}` takes {takes}; `{target}` is a {got}{because}")]
    WrongKind {
        op: &'static str,
        target: String,
        got: String,
        takes: &'static str,
        /// A clause explaining the refusal where the kind alone would not, as
        /// §6.1 requires for a chart and §6.7 for a table cell. Empty otherwise.
        because: String,
    },

    #[error("this document is a {class} document and `{op}` applies to {applies_to}")]
    NotApplicableToClass {
        op: &'static str,
        class: DocClass,
        applies_to: &'static str,
    },

    #[error("`{target}` holds {expected}; the content given is {got}")]
    ContentKindMismatch {
        target: String,
        expected: String,
        got: String,
    },

    #[error("the content carries the id `{id}`, which this document already uses")]
    DuplicateId { id: String },

    #[error("`{style}` is not a style of this document")]
    UnknownStyle { style: String },

    #[error("`{cell}` is covered by the merged range {merge}; only its top-left holds a value")]
    CoveredCell { cell: String, merge: String },

    #[error("`{cell}` is outside the addressable range of a sheet")]
    AddressOutOfRange { cell: String },

    #[error("`{target}` is a linked picture: the package holds no bytes to replace")]
    ExternalImage { target: String },

    #[error("`{destination}` is inside `{target}`, so the move has nowhere to land")]
    CycleRefused { target: String, destination: String },

    #[error("`{target}` has {got} cells and the table at `{destination}` has {takes} columns")]
    GridMismatch {
        target: String,
        destination: String,
        got: usize,
        takes: usize,
    },

    #[error("`{target}` is the last {kind} of this document, and a document with none cannot be written")]
    LastOfKind { target: String, kind: &'static str },

    #[error("this request cannot be addressed: {reason}")]
    Unaddressable { reason: String },

    #[error("stale etag for `{target}`: expected `{expected}`, the document says `{actual}`")]
    StaleEtag {
        target: String,
        expected: String,
        actual: String,
    },

    /// Not a caller mistake: two nodes carrying one id, or a node whose id is
    /// the reserved root `doc`. Phase 10 guarantees neither happens; the
    /// guarantee is asserted rather than assumed (17-B).
    #[error("{0}")]
    DocumentDefect(String),
}

/// A refusal, with the position of the operation that raised it (§8).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("operation {index} (`{op}`) refused: {refusal}")]
pub struct Refused {
    /// 0-based index into `operations`.
    pub index: usize,
    pub op: String,
    pub refusal: Refusal,
}
