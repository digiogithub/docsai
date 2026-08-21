//! Slimming of the JSON Schemas that `tools/list` publishes (E7).
//!
//! `tools/list` is the one response every session pays for before it has done
//! anything, and it is paid for again whenever a client re-lists. `schemars`
//! writes schemas for a validator; an agent reads them as prose. The two want
//! different bytes, and the parts removed here are the ones that carry no
//! information a caller can act on:
//!
//! * `$schema` — a dialect URI, 54 bytes per tool, identical for all of them.
//! * `"default": null` on an optional field — `null` is what "absent" already
//!   means, and the field is absent from `required` where it matters.
//! * `"type": ["string", "null"]` — the nullable union `schemars` writes for
//!   `Option<T>`. `required` is what says a field may be left out; the union
//!   only adds the right to *send* `null`, which no caller needs and every
//!   caller pays for.
//! * `"format": "uint"` beside a `"minimum": 0` — not a JSON Schema format any
//!   validator knows, and the `minimum` next to it already says the one thing
//!   it means.
//!
//! Nothing here touches field names, `required`, or descriptions: those are
//! what a caller reads to build a call. Shortening the prose is a separate
//! job, done at the source in `tools.rs`.

use serde_json::{Map, Value};

/// Rewrites a published schema into its cheapest equivalent form.
pub(crate) fn slim(schema: &Map<String, Value>) -> Map<String, Value> {
    let mut out = schema.clone();
    out.remove("$schema");
    slim_object(&mut out);
    out
}

fn slim_object(object: &mut Map<String, Value>) {
    if matches!(object.get("default"), Some(Value::Null)) {
        object.remove("default");
    }
    if let Some(collapsed) = object.get("type").and_then(collapse_nullable) {
        object.insert("type".into(), Value::String(collapsed));
    }
    if object.contains_key("minimum")
        && object
            .get("format")
            .and_then(Value::as_str)
            .is_some_and(|format| format.starts_with("uint"))
    {
        object.remove("format");
    }
    for value in object.values_mut() {
        slim_value(value);
    }
}

fn slim_value(value: &mut Value) {
    match value {
        Value::Object(object) => slim_object(object),
        Value::Array(items) => items.iter_mut().for_each(slim_value),
        _ => {}
    }
}

/// `["string", "null"]` becomes `"string"`; anything else is left alone.
///
/// A union of two real types is a genuine choice and stays; only the `null`
/// arm is dropped, and only when exactly one other arm remains.
fn collapse_nullable(type_value: &Value) -> Option<String> {
    let Value::Array(arms) = type_value else {
        return None;
    };
    if arms.len() != 2 || !arms.iter().any(|arm| arm == "null") {
        return None;
    }
    arms.iter()
        .find(|arm| *arm != "null")
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_dialect_uri_and_the_null_defaults_go() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {
                "path": { "default": null, "description": "d", "type": ["string", "null"] }
            }
        });
        let slimmed = slim(schema.as_object().unwrap());
        assert!(!slimmed.contains_key("$schema"));
        let path = &slimmed["properties"]["path"];
        assert!(path.get("default").is_none());
        assert_eq!(path["type"], json!("string"));
        assert_eq!(
            path["description"],
            json!("d"),
            "prose is not this pass's job"
        );
    }

    #[test]
    fn a_uint_format_goes_only_where_the_minimum_says_it_already() {
        let kept = json!({ "format": "uint", "type": "integer" });
        assert_eq!(
            slim(kept.as_object().unwrap())["format"],
            json!("uint"),
            "with no minimum beside it the format is the only constraint there is"
        );
        let dropped = json!({ "format": "uint", "minimum": 0, "type": "integer" });
        let slimmed = slim(dropped.as_object().unwrap());
        assert!(slimmed.get("format").is_none());
        assert_eq!(slimmed["minimum"], json!(0));
    }

    #[test]
    fn a_real_union_and_a_real_default_survive() {
        let schema = json!({
            "properties": {
                "n": { "default": 200, "type": ["string", "number"] },
                "deep": { "items": { "default": null, "type": ["string", "null"] } }
            }
        });
        let slimmed = slim(schema.as_object().unwrap());
        assert_eq!(slimmed["properties"]["n"]["default"], json!(200));
        assert_eq!(
            slimmed["properties"]["n"]["type"],
            json!(["string", "number"]),
            "two real arms are a choice a caller makes, not noise"
        );
        assert_eq!(
            slimmed["properties"]["deep"]["items"]["type"],
            json!("string"),
            "nested schemas are slimmed too"
        );
    }
}
