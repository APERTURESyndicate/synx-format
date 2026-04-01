//! Build [JSON Schema](https://json-schema.org/) from `!active` [`Constraints`](crate::Constraints)
//! and validate [`Value`](crate::Value) instances (optional `jsonschema` feature).

use crate::value::{Constraints, MetaMap, Value};
use serde_json::{json, Map, Value as JsonValue};

const SCHEMA_URL: &str = "https://json-schema.org/draft/2020-12/schema";

/// Convert a parsed SYNX [`Value`](crate::Value) tree to [`serde_json::Value`] for JSON Schema tooling.
pub fn value_to_json_value(v: &Value) -> JsonValue {
    match v {
        Value::Null => JsonValue::Null,
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::Int(n) => JsonValue::Number((*n).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonValue::Number)
            .unwrap_or(JsonValue::Null),
        Value::String(s) | Value::Secret(s) => JsonValue::String(s.clone()),
        Value::Array(items) => JsonValue::Array(items.iter().map(value_to_json_value).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, val) in map.iter() {
                out.insert(k.clone(), value_to_json_value(val));
            }
            JsonValue::Object(out)
        }
    }
}

/// Build a draft 2020-12 JSON Schema object from [`ParseResult::metadata`](crate::ParseResult::metadata).
///
/// Nested SYNX paths (`server`, `server.ssl`) become nested `properties`. Only keys that have
/// [`Meta::constraints`](crate::Meta::constraints) are emitted (same criterion as the JS `Synx.schema()`).
pub fn metadata_to_json_schema(metadata: &std::collections::HashMap<String, MetaMap>) -> JsonValue {
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
        let segments: Vec<&str> = if prefix.is_empty() {
            vec![]
        } else {
            prefix.split('.').collect()
        };
        for (field_key, meta) in mmap.iter() {
            if let Some(ref c) = meta.constraints {
                let prop = constraints_to_property(c);
                insert_constraint_at(&mut root, &segments, field_key, prop, c.required);
            }
        }
    }

    root
}

fn descend_create<'a>(root: &'a mut JsonValue, path: &[&str]) -> &'a mut JsonValue {
    let mut cur = root;
    for seg in path {
        let o = cur.as_object_mut().expect("schema node");
        let props = o
            .get_mut("properties")
            .expect("properties")
            .as_object_mut()
            .expect("properties object");
        cur = props.entry((*seg).to_string()).or_insert_with(|| {
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
    {
        let obj = target.as_object_mut().expect("target schema");
        let props = obj
            .get_mut("properties")
            .expect("properties")
            .as_object_mut()
            .expect("properties map");
        props.insert(field_key.to_string(), JsonValue::Object(prop));
    }
    if is_required {
        let obj = target.as_object_mut().expect("target schema");
        let req = obj
            .get_mut("required")
            .expect("required")
            .as_array_mut()
            .expect("required array");
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
pub fn validate_with_json_schema(
    instance: &Value,
    schema: &JsonValue,
) -> Result<(), Vec<String>> {
    validate_serde_json(&value_to_json_value(instance), schema)
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
    use std::collections::HashMap;

    #[test]
    fn nested_metadata_schema() {
        let mut metadata: HashMap<String, MetaMap> = HashMap::new();
        let mut root_map = MetaMap::new();
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
}
