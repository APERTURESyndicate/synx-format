//! Build [JSON Schema](https://json-schema.org/) from `!active` [`Constraints`](crate::Constraints)
//! and validate [`Value`](crate::Value) instances (optional `jsonschema` feature).

use crate::value::{Constraints, MetaMap, Value};
use serde_json::{json, Map, Value as JsonValue};

const SCHEMA_URL: &str = "https://json-schema.org/draft/2020-12/schema";

/// Convert a parsed SYNX [`Value`](crate::Value) tree to [`serde_json::Value`] for JSON Schema tooling.
///
/// A secret is written as `"[SECRET]"`, as in `to_json`. It used to be
/// written in the clear, and whatever was built on the result — a dump, a
/// validator's message — carried it.
pub fn value_to_json_value(v: &Value) -> JsonValue {
    json_value(v, false, 0)
}

/// `reveal` keeps the text of a secret: validation checks the real value.
fn json_value(v: &Value, reveal: bool, depth: usize) -> JsonValue {
    // Same guard as `to_json`: a value nested deeper is written as null.
    if depth > 128 {
        return JsonValue::Null;
    }
    match v {
        Value::Null => JsonValue::Null,
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::Int(n) => JsonValue::Number((*n).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::String(s) => JsonValue::String(s.clone()),
        Value::Secret(s) => JsonValue::String(if reveal { s.clone() } else { "[SECRET]".to_string() }),
        Value::Array(items) => JsonValue::Array(items.iter().map(|item| json_value(item, reveal, depth + 1)).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, val) in map.iter() {
                out.insert(k.clone(), json_value(val, reveal, depth + 1));
            }
            JsonValue::Object(out)
        }
    }
}

/// Every secret in `v`, longest first, for [`validate_with_json_schema`].
#[cfg(feature = "jsonschema")]
fn collect_secrets<'a>(v: &'a Value, out: &mut Vec<&'a str>, depth: usize) {
    if depth > 128 {
        return;
    }
    match v {
        Value::Secret(s) if !s.is_empty() => out.push(s),
        Value::Array(items) => items.iter().for_each(|item| collect_secrets(item, out, depth + 1)),
        Value::Object(map) => map.values().for_each(|val| collect_secrets(val, out, depth + 1)),
        _ => {}
    }
}

/// Build a draft 2020-12 JSON Schema object from [`ParseResult::metadata`](crate::ParseResult::metadata).
///
/// Nested SYNX paths (`server`, `server.ssl`) become nested `properties`. Only keys that have
/// [`Meta::constraints`](crate::Meta::constraints) are emitted (same criterion as the JS `Synx.schema()`).
pub fn metadata_to_json_schema(metadata: &crate::Map<MetaMap>) -> JsonValue {
    let mut root = json!({
        "$schema": SCHEMA_URL,
        "type": "object",
        "properties": {},
        "required": []
    });

    let mut keys: Vec<&String> = metadata.keys().collect();
    keys.sort_by_key(|k| (k.matches('.').count(), k.len(), k.as_str()));

    for prefix in keys {
        let mmap = &metadata[prefix];
        // A list item's `#<index>` segment is not a property: its fields
        // describe the list's objects, as before 3.8.0.
        let owned = crate::engine::meta_path_segments(prefix);
        let segments: Vec<&str> = owned.iter().map(String::as_str).filter(|seg| !seg.starts_with('#')).collect();
        for (field_key, meta) in mmap.iter() {
            if let Some(ref c) = meta.constraints {
                let prop = constraints_to_property(c);
                insert_constraint_at(&mut root, &segments, field_key, prop, c.required);
            }
        }
    }

    root
}

/// The `properties` map of a schema node, created when the node has none.
///
/// safety: a group that carries a constraint of its own (`server[required]`)
/// is entered as a property without `properties`; a constrained field inside
/// it (`port[min:1]`) then looked the map up with `expect` and panicked — in
/// `synx schema` and `validate --self-schema`. A node that is not an object
/// at all is replaced by an empty one.
fn properties_of(node: &mut JsonValue) -> &mut Map<String, JsonValue> {
    if !node.is_object() {
        *node = JsonValue::Object(Map::new());
    }
    let obj = node.as_object_mut().expect("made an object above");
    let props = obj.entry("properties").or_insert_with(|| JsonValue::Object(Map::new()));
    if !props.is_object() {
        *props = JsonValue::Object(Map::new());
    }
    props.as_object_mut().expect("made an object above")
}

fn descend_create<'a>(root: &'a mut JsonValue, path: &[&str]) -> &'a mut JsonValue {
    let mut cur = root;
    for seg in path {
        cur = properties_of(cur).entry((*seg).to_string()).or_insert_with(|| {
            json!({
                "type": "object",
                "properties": {},
                "required": []
            })
        });
    }
    cur
}

fn insert_constraint_at(
    root: &mut JsonValue,
    path: &[&str],
    field_key: &str,
    prop: Map<String, JsonValue>,
    is_required: bool,
) {
    let target = descend_create(root, path);
    properties_of(target).insert(field_key.to_string(), JsonValue::Object(prop));
    if is_required {
        let obj = target.as_object_mut().expect("`properties_of` made it an object");
        let req = obj.entry("required").or_insert_with(|| JsonValue::Array(Vec::new()));
        if !req.is_array() {
            *req = JsonValue::Array(Vec::new());
        }
        let req = req.as_array_mut().expect("made an array above");
        if !req.iter().any(|e| e.as_str() == Some(field_key)) {
            req.push(JsonValue::String(field_key.to_string()));
        }
    }
}

fn constraints_to_property(c: &Constraints) -> Map<String, JsonValue> {
    let mut prop = Map::new();

    if let Some(ref t) = c.type_name {
        match t.as_str() {
            "int" => {
                prop.insert("type".into(), json!("integer"));
            }
            "float" => {
                prop.insert("type".into(), json!("number"));
            }
            "bool" => {
                prop.insert("type".into(), json!("boolean"));
            }
            "string" => {
                prop.insert("type".into(), json!("string"));
            }
            _ => {}
        }
    }

    if c.min.is_some() || c.max.is_some() {
        let is_string = matches!(c.type_name.as_deref(), Some("string"));
        if is_string {
            if let Some(mn) = c.min {
                if mn.fract() == 0.0 {
                    prop.insert("minLength".into(), json!(mn as u64));
                }
            }
            if let Some(mx) = c.max {
                if mx.fract() == 0.0 {
                    prop.insert("maxLength".into(), json!(mx as u64));
                }
            }
        } else {
            if let Some(mn) = c.min {
                prop.insert("minimum".into(), json!(mn));
            }
            if let Some(mx) = c.max {
                prop.insert("maximum".into(), json!(mx));
            }
        }
    }

    if let Some(ref p) = c.pattern {
        prop.insert("pattern".into(), json!(p));
    }

    if let Some(ref ev) = c.enum_values {
        let mut arr: Vec<JsonValue> = ev.iter().cloned().map(JsonValue::String).collect();
        if let Some(et) = c.type_name.as_deref() {
            if et == "int" {
                arr = ev
                    .iter()
                    .filter_map(|s| s.parse::<i64>().ok())
                    .map(|n| JsonValue::Number(n.into()))
                    .collect();
            } else if et == "float" {
                arr = ev
                    .iter()
                    .filter_map(|s| s.parse::<f64>().ok())
                    .filter_map(|f| serde_json::Number::from_f64(f).map(JsonValue::Number))
                    .collect();
            } else if et == "bool" {
                arr = ev
                    .iter()
                    .map(|s| JsonValue::Bool(s == "true"))
                    .collect();
            }
        }
        if !arr.is_empty() {
            prop.insert("enum".into(), JsonValue::Array(arr));
        }
    }

    prop
}

/// Validate `instance` against a JSON Schema value using the `jsonschema` crate.
#[cfg(feature = "jsonschema")]
///
/// A secret is checked by its real text, and that text is cut out of every
/// message returned: a validator quotes the value it rejects, so
/// `api[pattern:^sk_.+$]:secret:env KEY` printed the key into the build log.
pub fn validate_with_json_schema(
    instance: &Value,
    schema: &JsonValue,
) -> Result<(), Vec<String>> {
    let Err(mut messages) = validate_serde_json(&json_value(instance, true, 0), schema) else {
        return Ok(());
    };
    let mut secrets: Vec<&str> = Vec::new();
    collect_secrets(instance, &mut secrets, 0);
    secrets.sort_unstable_by_key(|s| std::cmp::Reverse(s.len()));
    secrets.dedup();
    for message in messages.iter_mut() {
        for secret in &secrets {
            if message.contains(secret) {
                *message = message.replace(secret, "[SECRET]");
            }
            // The validator prints a string as JSON, escapes included.
            let quoted = JsonValue::String((*secret).to_string()).to_string();
            let inner = &quoted[1..quoted.len() - 1];
            if inner != *secret && message.contains(inner) {
                *message = message.replace(inner, "[SECRET]");
            }
        }
    }
    Err(messages)
}

/// Validate a [`serde_json::Value`] instance (e.g. raw JSON file) against a JSON Schema.
#[cfg(feature = "jsonschema")]
pub fn validate_serde_json(instance: &JsonValue, schema: &JsonValue) -> Result<(), Vec<String>> {
    let validator = match jsonschema::validator_for(schema) {
        Ok(v) => v,
        Err(e) => return Err(vec![format!("invalid JSON Schema: {e}")]),
    };
    let mut errs = Vec::new();
    for e in validator.iter_errors(instance) {
        errs.push(e.to_string());
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Meta;

    #[test]
    fn nested_metadata_schema() {
        let mut metadata: crate::Map<MetaMap> = crate::Map::default();
        let mut root_map = MetaMap::default();
        root_map.insert(
            "name".into(),
            Meta {
                markers: vec![],
                args: vec![],
                type_hint: None,
                constraints: Some(Constraints {
                    required: true,
                    type_name: Some("string".into()),
                    min: Some(1.0),
                    max: Some(20.0),
                    ..Default::default()
                }),
            },
        );
        metadata.insert(String::new(), root_map);

        let sch = metadata_to_json_schema(&metadata);
        assert_eq!(sch["properties"]["name"]["type"], json!("string"));
        assert_eq!(sch["properties"]["name"]["minLength"], json!(1));
        assert_eq!(sch["required"], json!(["name"]));
    }

    #[test]
    fn secret_is_masked_in_the_json_value() {
        let mut env = std::collections::HashMap::new();
        env.insert("K".to_string(), "live_abcdef123".to_string());
        let mut r = crate::parse("!active\napi:secret:env K\nlist:secret\n  - hunter2\n");
        crate::resolve(&mut r, &crate::Options { env: Some(env), ..Default::default() });
        let json = value_to_json_value(&r.root).to_string();
        assert!(!json.contains("live_abcdef123") && !json.contains("hunter2"), "{}", json);
        assert_eq!(json, r#"{"api":"[SECRET]","list":["[SECRET]"]}"#);
    }

    #[cfg(feature = "jsonschema")]
    #[test]
    fn validation_messages_do_not_quote_a_secret() {
        let mut env = std::collections::HashMap::new();
        env.insert("K".to_string(), "live \"abc\" 123".to_string());
        let mut r = crate::parse("!active\napi:secret:env K\n");
        crate::resolve(&mut r, &crate::Options { env: Some(env), ..Default::default() });
        let schema = json!({"type": "object", "properties": {"api": {"type": "string", "pattern": "^sk_.+$"}}});
        let errs = validate_with_json_schema(&r.root, &schema).unwrap_err();
        assert!(!errs.is_empty());
        for e in &errs {
            assert!(!e.contains("live") && !e.contains("abc"), "{}", e);
        }
        // The real value is what gets checked.
        let schema = json!({"type": "object", "properties": {"api": {"type": "string", "pattern": "^live"}}});
        assert!(validate_with_json_schema(&r.root, &schema).is_ok());
    }

    // A constrained group with a constrained field inside it used to panic.
    #[test]
    fn constrained_group_with_constrained_field() {
        let r = crate::parse("!active\nserver[required]\n  port[min:1] 80\n  tls[required]\n    cert[type:string] a\n");
        let sch = metadata_to_json_schema(&r.metadata);
        assert_eq!(sch["required"], json!(["server"]));
        assert_eq!(sch["properties"]["server"]["properties"]["port"]["minimum"], json!(1.0));
        assert_eq!(sch["properties"]["server"]["required"], json!(["tls"]));
        assert_eq!(
            sch["properties"]["server"]["properties"]["tls"]["properties"]["cert"]["type"],
            json!("string")
        );
    }
}
