//! The one implementation behind both doors (17-E).
//!
//! `docsai edit` and the `apply_edits` MCP tool are the same three lines: turn
//! JSON into an [`EditRequest`], call [`docsai_convert::edit::edit_path`],
//! render what came back. Everything either door could get subtly different —
//! how a request is parsed, which refusals are typed, what the report looks
//! like — lives here, in a module both of them call, so that the rule «neither
//! door may carry logic the other lacks» is a structural property and not a
//! promise. `crates/docsai-mcp/tests/two_doors.rs` runs one edit through both
//! and compares the bytes and the report anyway.
//!
//! It sits in this crate rather than in `docsai-convert` for the reason
//! `validation_output` already sits here: the CLI depends on `docsai-mcp` (it
//! is the crate that hosts `docsai mcp`), the operation set is defined against
//! the IR in `docsai-convert` and has deliberately no JSON dependency
//! (`edit/request.rs`), and this is where JSON meets it.
//!
//! # What this can and cannot reach on the filesystem
//!
//! * It reads exactly one file: `path`.
//! * It writes exactly one file: `output_path` when given, otherwise `path`
//!   itself — plus, for a DocMark destination only, the `assets/` directory
//!   beside it, which is where the document it writes points its media.
//! * It creates a `.docsai-edit-*.tmp` beside the destination for the length of
//!   the transaction, and unlinks it on every failure.
//! * It never creates a directory: a destination whose parent does not exist is
//!   a failure, not a `mkdir -p`.
//! * `dry_run` writes the temporary file and throws it away, so it needs the
//!   destination directory to be writable and leaves nothing behind.
//!
//! There is no third path, no glob, and no batch: one call touches one
//! document. `replace_image` reads the picture it is given, which is the one
//! other path a request can name, and it is named by the caller.

// The same trade `docsai_convert::edit` records: a refusal is a large, rare
// value, and boxing it would put an allocation on the refusal path to save
// moving a struct that only exists when a request is already being rejected.
#![allow(clippy::result_large_err)]

use std::path::{Path, PathBuf};

use docsai_convert::edit::{
    edit_path, EditError, EditOptions, EditOutcome, EditRequest, Operation, Refusal, Refused,
    OPS_VERSION,
};
use docsai_convert::{parse_fidelity, validate_output_path, ConvertError, ConvertOptions};
use serde_json::{json, Map, Value};

use crate::config::McpConfig;
use crate::tools::ToolOutput;

/// One edit, as either door states it.
///
/// The fields outside `operations` are «the door's business» in the words of
/// `docs/edit-operations.md` §3, and are the same on both doors because there
/// is only one of them.
#[derive(Debug, Default)]
pub struct EditCall<'a> {
    /// The document to edit.
    pub path: &'a str,
    /// Either the §3 request object (`{ops?, operations, dry_run?}`) or, for
    /// short, the bare `operations` array.
    pub operations: &'a Value,
    /// Where the result goes. `None` replaces `path`.
    pub output_path: Option<&'a str>,
    /// `true` overrides the request's own `dry_run`; it never turns one off.
    pub dry_run: bool,
    /// `full` (the default) or `agent`; §3 refuses the lossy levels.
    pub fidelity: Option<&'a str>,
}

/// Why an edit did not happen.
///
/// Two arms and not one: a [`Refused`] is the caller's request being wrong in a
/// way §8 gave a name to, and it crosses either door **as a name with its
/// fields**, never as a sentence. Everything else — an unreadable file, a
/// writer that could not write — is a [`ConvertError`] and already has a door.
#[derive(Debug)]
pub enum EditFailure {
    Refused(Box<Refused>),
    Convert(ConvertError),
}

impl std::fmt::Display for EditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditFailure::Refused(refused) => write!(f, "{refused}"),
            EditFailure::Convert(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for EditFailure {}

impl EditFailure {
    /// The failure as a caller reads it: the typed object for a refusal, the
    /// message for everything else.
    pub fn output(&self) -> ToolOutput {
        match self {
            EditFailure::Refused(refused) => {
                let mut value = serde_json::to_value(refused.as_ref())
                    .unwrap_or_else(|_| json!({ "refusal": "DocumentDefect" }));
                if let Some(object) = value.as_object_mut() {
                    object.insert("message".into(), Value::String(refused.to_string()));
                }
                ToolOutput::json(value)
            }
            EditFailure::Convert(error) => {
                let text = error.to_string();
                ToolOutput::rendered(json!({ "error": text }), text)
            }
        }
    }
}

impl From<ConvertError> for EditFailure {
    fn from(error: ConvertError) -> Self {
        EditFailure::Convert(error)
    }
}

impl From<Refused> for EditFailure {
    fn from(refused: Refused) -> Self {
        EditFailure::Refused(Box::new(refused))
    }
}

impl From<EditError> for EditFailure {
    fn from(error: EditError) -> Self {
        match error {
            EditError::Refused(refused) => EditFailure::Refused(Box::new(refused)),
            EditError::Convert(error) => EditFailure::Convert(error),
            other => EditFailure::Convert(ConvertError::Invalid(other.to_string())),
        }
    }
}

/// Applies `call` and answers with the report.
///
/// The whole of both doors.
pub fn apply_edits(call: &EditCall<'_>, config: &McpConfig) -> Result<ToolOutput, EditFailure> {
    let mut request = parse_request(call.operations)?;
    request.dry_run |= call.dry_run;

    let input = PathBuf::from(call.path);
    readable(&input, config)?;
    let output = match call.output_path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            validate_output_path(&path)?;
            Some(path)
        }
        None => None,
    };

    let fidelity = match call.fidelity.map(str::trim).filter(|f| !f.is_empty()) {
        Some(name) => parse_fidelity(name)?,
        None => docsai_convert::Fidelity::Full,
    };
    let options = EditOptions {
        convert: ConvertOptions {
            fidelity,
            cache: config.cache.clone(),
            ..Default::default()
        },
        output,
        ..Default::default()
    };

    let outcome = edit_path(&input, &request, &options)?;
    Ok(report_output(&outcome))
}

/// The report, plus the one fact the report deliberately leaves out: which file
/// now holds it.
///
/// Nothing else is added. 17-D measured a two-target report at 31 tokens and
/// the door's job is not to inflate it, so there is no echo of the request, no
/// per-operation status and no format name a caller already knows.
pub fn report_output(outcome: &EditOutcome) -> ToolOutput {
    let mut value = serde_json::to_value(&outcome.report).unwrap_or_else(|_| json!({}));
    let object = value.as_object_mut().expect("a report is an object");
    let mut ordered = Map::new();
    ordered.insert(
        "path".into(),
        Value::String(outcome.path.display().to_string()),
    );
    if outcome.dry_run {
        ordered.insert("dry_run".into(), Value::Bool(true));
    }
    ordered.append(object);
    ToolOutput::json(Value::Object(ordered))
}

// ---------------------------------------------------------------------------
// The request, parsed
// ---------------------------------------------------------------------------

/// Turns a caller's JSON into an [`EditRequest`], with §8's names on every way
/// it can be wrong.
///
/// Serde alone would answer «unknown field `txt`, expected one of …» as a
/// string, and §0.4 asks for a refusal that a program can read. So the shape is
/// checked against the table below first, and only a request that has already
/// been found well-formed is handed to serde — which then cannot fail.
pub fn parse_request(value: &Value) -> Result<EditRequest, Refused> {
    let request = |refusal| Refused {
        index: 0,
        op: "<request>".into(),
        refusal,
    };

    let (ops, operations, dry_run) = match value {
        Value::Array(items) => (None, items.as_slice(), false),
        Value::Object(object) => {
            for key in object.keys() {
                if !matches!(key.as_str(), "ops" | "operations" | "dry_run") {
                    return Err(request(Refusal::UnknownField {
                        op: "<request>",
                        field: key.clone(),
                        known: "ops, operations, dry_run",
                    }));
                }
            }
            let ops = match object.get("ops") {
                None | Some(Value::Null) => None,
                Some(Value::Number(n)) if n.is_u64() => Some(n.as_u64().unwrap_or(0) as u32),
                Some(_) => {
                    return Err(request(Refusal::MalformedField {
                        op: "<request>",
                        field: "ops".into(),
                        reason: "is the operation-set version, a whole number".into(),
                    }))
                }
            };
            let operations = match object.get("operations") {
                Some(Value::Array(items)) => items.as_slice(),
                None => {
                    return Err(request(Refusal::MissingField {
                        op: "<request>",
                        field: "operations",
                    }))
                }
                Some(_) => {
                    return Err(request(Refusal::MalformedField {
                        op: "<request>",
                        field: "operations".into(),
                        reason: "is a list of operations, applied in order".into(),
                    }))
                }
            };
            let dry_run = match object.get("dry_run") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => {
                    return Err(request(Refusal::MalformedField {
                        op: "<request>",
                        field: "dry_run".into(),
                        reason: "is true or false".into(),
                    }))
                }
            };
            (ops, operations, dry_run)
        }
        _ => {
            return Err(request(Refusal::MalformedField {
                op: "<request>",
                field: "operations".into(),
                reason: "is a list of operations, or the request object that carries one".into(),
            }))
        }
    };

    // §1.2: the version is checked here as well as in the engine, so that a
    // client that asked for a version this build does not speak is told before
    // its operations are read against the wrong set.
    if let Some(requested) = ops {
        if requested != OPS_VERSION {
            return Err(request(Refusal::UnsupportedOpsVersion {
                requested,
                speaks: OPS_VERSION,
            }));
        }
    }

    let mut parsed = Vec::with_capacity(operations.len());
    for (index, operation) in operations.iter().enumerate() {
        parsed.push(parse_operation(index, operation)?);
    }
    Ok(EditRequest {
        ops,
        operations: parsed,
        dry_run,
    })
}

/// What a field may hold. The table below is §6 read as JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A string.
    Text,
    /// A string, or `null` to clear (§6.8's `content`).
    TextOrNull,
    /// A string, a number, a boolean, or `null` (§6.7's `value`).
    Scalar,
    /// An object of §6.2's property names onto scalars or `null`.
    Props,
}

/// `(field, kind, required)` for every operation of §1.3.
///
/// `required` here means «serde would refuse a request without it». The
/// conditional requirements — `move`'s exactly-one-of, `set_cell`'s `value`
/// having to be *present* even when null — belong to the engine's validation
/// pass (§4) and are left there, so that both doors and every in-process caller
/// get the same answer.
type Fields = &'static [(&'static str, Kind, bool)];

const IF_MATCH: (&str, Kind, bool) = ("if_match", Kind::Text, false);
const TARGET: (&str, Kind, bool) = ("target", Kind::Text, true);

fn fields_of(op: &str) -> Option<Fields> {
    Some(match op {
        "replace_text" => &[TARGET, ("text", Kind::Text, true), IF_MATCH],
        "set_props" => &[TARGET, ("props", Kind::Props, true), IF_MATCH],
        "insert_after" | "insert_before" => &[TARGET, ("content", Kind::Text, true), IF_MATCH],
        "delete" | "duplicate_slide" => &[TARGET, IF_MATCH],
        "move" => &[
            TARGET,
            ("after", Kind::Text, false),
            ("before", Kind::Text, false),
            IF_MATCH,
        ],
        "set_cell" => &[
            TARGET,
            ("cell", Kind::Text, true),
            ("value", Kind::Scalar, false),
            IF_MATCH,
        ],
        "set_notes" => &[TARGET, ("content", Kind::TextOrNull, false), IF_MATCH],
        "replace_image" => &[
            TARGET,
            ("path", Kind::Text, false),
            ("content_base64", Kind::Text, false),
            ("filename", Kind::Text, false),
            ("alt", Kind::Text, false),
            IF_MATCH,
        ],
        _ => return None,
    })
}

/// The `known` clause of an `UnknownField`, which §8 requires to list what the
/// operation does take.
fn known_of(op: &str) -> &'static str {
    match op {
        "replace_text" => "target, text, if_match",
        "set_props" => "target, props, if_match",
        "insert_after" | "insert_before" => "target, content, if_match",
        "delete" | "duplicate_slide" => "target, if_match",
        "move" => "target, after, before, if_match",
        "set_cell" => "target, cell, value, if_match",
        "set_notes" => "target, content, if_match",
        "replace_image" => "target, path, content_base64, filename, alt, if_match",
        _ => "",
    }
}

/// Every name §1.3 defines, for the `UnknownOperation` message.
pub const OPERATION_NAMES: &[&str] = &[
    "replace_text",
    "set_props",
    "insert_after",
    "insert_before",
    "delete",
    "move",
    "duplicate_slide",
    "set_cell",
    "set_notes",
    "replace_image",
];

fn parse_operation(index: usize, value: &Value) -> Result<Operation, Refused> {
    let unnamed = |refusal| Refused {
        index,
        op: "<operation>".into(),
        refusal,
    };
    let Some(object) = value.as_object() else {
        return Err(unnamed(Refusal::MalformedField {
            op: "<operation>",
            field: "operations".into(),
            reason: format!(
                "holds {} where an operation object belongs",
                describe(value)
            ),
        }));
    };
    let name = match object.get("op") {
        Some(Value::String(name)) => name.clone(),
        _ => {
            return Err(unnamed(Refusal::MissingField {
                op: "<operation>",
                field: "op",
            }))
        }
    };
    let Some(fields) = fields_of(&name) else {
        return Err(Refused {
            index,
            op: name.clone(),
            refusal: Refusal::UnknownOperation {
                op: name,
                speaks: OPS_VERSION,
            },
        });
    };
    // `fields_of` said yes, so the name is one of these; the lookup is how a
    // `&'static str` refusal field gets it.
    let op: &'static str = OPERATION_NAMES
        .iter()
        .copied()
        .find(|known| *known == name)
        .unwrap_or("<operation>");
    let refuse = |refusal| Refused {
        index,
        op: name.clone(),
        refusal,
    };

    for key in object.keys() {
        if key == "op" {
            continue;
        }
        if !fields.iter().any(|(field, _, _)| field == key) {
            return Err(refuse(Refusal::UnknownField {
                op,
                field: key.clone(),
                known: known_of(&name),
            }));
        }
    }
    for (field, kind, required) in fields {
        match object.get(*field) {
            None => {
                if *required {
                    return Err(refuse(Refusal::MissingField { op, field }));
                }
            }
            Some(found) => {
                if let Some(reason) = mismatch(*kind, found) {
                    return Err(refuse(Refusal::MalformedField {
                        op,
                        field: (*field).into(),
                        reason,
                    }));
                }
            }
        }
    }

    // Every shape the enum's `Deserialize` can reject has been rejected above,
    // so this cannot fail; it is still handled rather than unwrapped, because a
    // door that panics on a caller's input is a worse bug than the one it would
    // be reporting.
    serde_json::from_value(value.clone()).map_err(|error| {
        refuse(Refusal::MalformedField {
            op,
            field: "<operation>".into(),
            reason: error.to_string(),
        })
    })
}

/// `None` when `value` is one of the things `kind` admits.
fn mismatch(kind: Kind, value: &Value) -> Option<String> {
    let ok = match kind {
        Kind::Text => value.is_string(),
        Kind::TextOrNull => value.is_string() || value.is_null(),
        Kind::Scalar => {
            value.is_string() || value.is_number() || value.is_boolean() || value.is_null()
        }
        Kind::Props => value.as_object().is_some_and(|props| {
            props
                .values()
                .all(|v| v.is_string() || v.is_number() || v.is_boolean() || v.is_null())
        }),
    };
    if ok {
        return None;
    }
    let wants = match kind {
        Kind::Text => "is a string",
        Kind::TextOrNull => "is a string, or null to clear it",
        Kind::Scalar => "is a string, a number, a boolean, or null to clear the cell",
        Kind::Props => "is an object of property names onto scalars or null",
    };
    Some(format!("{wants}, and this is {}", describe(value)))
}

fn describe(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// The same read guard every other tool applies to a `path` argument.
fn readable(path: &Path, config: &McpConfig) -> Result<(), ConvertError> {
    crate::input::enforce_path_readable(path, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refuse(json: Value) -> Refused {
        parse_request(&json).expect_err("refused")
    }

    #[test]
    fn a_bare_array_and_the_request_object_parse_to_the_same_request() {
        let ops = json!([{ "op": "delete", "target": "n7" }]);
        let object = json!({ "ops": 1, "operations": [{ "op": "delete", "target": "n7" }] });
        let from_array = parse_request(&ops).expect("array");
        let from_object = parse_request(&object).expect("object");
        assert_eq!(from_array.operations, from_object.operations);
        assert_eq!(from_object.ops, Some(OPS_VERSION));
    }

    #[test]
    fn an_unknown_operation_is_named_with_the_version_that_refused_it() {
        let refused = refuse(json!([{ "op": "recolour", "target": "n7" }]));
        assert!(matches!(
            refused.refusal,
            Refusal::UnknownOperation { speaks: 1, .. }
        ));
        assert_eq!(refused.index, 0);
    }

    #[test]
    fn an_unknown_field_lists_what_the_operation_does_take() {
        let refused = refuse(json!([{ "op": "replace_text", "target": "n7", "txt": "x" }]));
        match refused.refusal {
            Refusal::UnknownField { op, field, known } => {
                assert_eq!(op, "replace_text");
                assert_eq!(field, "txt");
                assert!(known.contains("text"), "{known}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_field_of_the_wrong_type_is_neither_missing_nor_unknown() {
        let refused = refuse(json!([{ "op": "replace_text", "target": "n7", "text": 5 }]));
        match &refused.refusal {
            Refusal::MalformedField { op, field, reason } => {
                assert_eq!(*op, "replace_text");
                assert_eq!(field, "text");
                assert!(reason.contains("a number"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_required_field_that_is_absent_is_missing_and_says_which() {
        let refused = refuse(json!([{ "op": "replace_text", "target": "n7" }]));
        assert!(matches!(
            refused.refusal,
            Refusal::MissingField { field: "text", .. }
        ));
    }

    #[test]
    fn a_version_this_build_does_not_speak_is_refused_before_the_operations_are_read() {
        let refused = refuse(json!({ "ops": 9, "operations": [{ "op": "nonsense" }] }));
        assert!(matches!(
            refused.refusal,
            Refusal::UnsupportedOpsVersion {
                requested: 9,
                speaks: 1
            }
        ));
    }

    #[test]
    fn set_cell_takes_a_null_value_and_a_number_alike() {
        for value in [json!(null), json!(1200), json!("=A1*2"), json!(true)] {
            parse_request(&json!([{
                "op": "set_cell", "target": "n3", "cell": "B7", "value": value
            }]))
            .expect("a scalar or null");
        }
        let refused = refuse(json!([{
            "op": "set_cell", "target": "n3", "cell": "B7", "value": ["a"]
        }]));
        assert!(matches!(refused.refusal, Refusal::MalformedField { .. }));
    }

    #[test]
    fn a_refusal_crosses_the_door_as_a_name_with_its_fields() {
        let failure = EditFailure::from(Refused {
            index: 2,
            op: "replace_text".into(),
            refusal: Refusal::StaleEtag {
                target: "n7".into(),
                expected: "ab12cd".into(),
                actual: "9f0011".into(),
            },
        });
        let out = failure.output();
        assert_eq!(out.value["refusal"], json!("StaleEtag"));
        assert_eq!(out.value["expected"], json!("ab12cd"));
        assert_eq!(out.value["actual"], json!("9f0011"));
        assert_eq!(out.value["index"], json!(2));
        assert!(out.value["message"]
            .as_str()
            .is_some_and(|m| m.contains("ab12cd")));
    }

    #[test]
    fn an_unknown_key_on_the_request_itself_is_refused_and_not_ignored() {
        let refused = refuse(json!({ "operations": [], "dryrun": true }));
        assert!(matches!(refused.refusal, Refusal::UnknownField { .. }));
    }
}
