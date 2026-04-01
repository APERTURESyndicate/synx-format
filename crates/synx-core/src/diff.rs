//! Structural diff between two SYNX values.

use std::collections::HashMap;
use crate::Value;

/// Result of a structural diff between two SYNX objects.
#[derive(Debug, Clone)]
pub struct DiffResult {
    pub added: HashMap<String, Value>,
    pub removed: HashMap<String, Value>,
    pub changed: HashMap<String, DiffChange>,
    pub unchanged: Vec<String>,
}

/// A single changed key with its previous and new value.
#[derive(Debug, Clone)]
pub struct DiffChange {
    pub from: Value,
    pub to: Value,
}

/// Compute a structural diff between two SYNX objects (top-level keys).
///
/// Keys present only in `b` appear in `added`.
/// Keys present only in `a` appear in `removed`.
/// Keys present in both but with different values appear in `changed`.
/// Keys present in both with equal values appear in `unchanged`.
pub fn diff(a: &HashMap<String, Value>, b: &HashMap<String, Value>) -> DiffResult {
    let mut added = HashMap::new();
    let mut removed = HashMap::new();
    let mut changed = HashMap::new();
    let mut unchanged = Vec::new();

    for (key, a_val) in a {
        match b.get(key) {
            None => { removed.insert(key.clone(), a_val.clone()); }
            Some(b_val) => {
                if deep_equal(a_val, b_val) {
                    unchanged.push(key.clone());
                } else {
                    changed.insert(key.clone(), DiffChange {
                        from: a_val.clone(),
                        to: b_val.clone(),
                    });
                }
            }
        }
    }

    for (key, b_val) in b {
        if !a.contains_key(key) {
            added.insert(key.clone(), b_val.clone());
        }
    }

    unchanged.sort();

    DiffResult { added, removed, changed, unchanged }
}

/// Convert a `DiffResult` into a `Value::Object` suitable for JSON serialisation.
///
/// Shape: `{ "added": {...}, "removed": {...}, "changed": { key: { "from": ..., "to": ... } }, "unchanged": [...] }`
pub fn diff_to_value(d: &DiffResult) -> Value {
    let mut root = HashMap::new();

    root.insert("added".into(), Value::Object(d.added.clone()));
    root.insert("removed".into(), Value::Object(d.removed.clone()));

    let mut changed_map = HashMap::new();
    for (k, c) in &d.changed {
        let mut entry = HashMap::new();
        entry.insert("from".into(), c.from.clone());
        entry.insert("to".into(), c.to.clone());
        changed_map.insert(k.clone(), Value::Object(entry));
    }
    root.insert("changed".into(), Value::Object(changed_map));

    let unchanged_arr: Vec<Value> = d.unchanged.iter().map(|s| Value::String(s.clone())).collect();
    root.insert("unchanged".into(), Value::Array(unchanged_arr));

    Value::Object(root)
}

fn deep_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Secret(x), Value::Secret(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| deep_equal(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|yv| deep_equal(v, yv)))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: Vec<(&str, Value)>) -> HashMap<String, Value> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    #[test]
    fn identical_objects() {
        let a = obj(vec![("name", Value::String("John".into())), ("age", Value::Int(25))]);
        let b = a.clone();
        let d = diff(&a, &b);
        assert!(d.added.is_empty());
        assert!(d.removed.is_empty());
        assert!(d.changed.is_empty());
        assert_eq!(d.unchanged.len(), 2);
    }

    #[test]
    fn added_and_removed() {
        let a = obj(vec![("x", Value::Int(1))]);
        let b = obj(vec![("y", Value::Int(2))]);
        let d = diff(&a, &b);
        assert_eq!(d.added.len(), 1);
        assert_eq!(d.removed.len(), 1);
        assert!(d.changed.is_empty());
        assert!(d.unchanged.is_empty());
    }

    #[test]
    fn changed_value() {
        let a = obj(vec![("name", Value::String("Alice".into()))]);
        let b = obj(vec![("name", Value::String("Bob".into()))]);
        let d = diff(&a, &b);
        assert_eq!(d.changed.len(), 1);
        assert!(d.changed.contains_key("name"));
    }

    #[test]
    fn nested_diff() {
        let inner_a = obj(vec![("host", Value::String("localhost".into()))]);
        let inner_b = obj(vec![("host", Value::String("0.0.0.0".into()))]);
        let a = obj(vec![("server", Value::Object(inner_a))]);
        let b = obj(vec![("server", Value::Object(inner_b))]);
        let d = diff(&a, &b);
        assert_eq!(d.changed.len(), 1);
    }

    #[test]
    fn to_value_roundtrip() {
        let a = obj(vec![("x", Value::Int(1)), ("y", Value::Int(2))]);
        let b = obj(vec![("x", Value::Int(1)), ("z", Value::Int(3))]);
        let d = diff(&a, &b);
        let val = diff_to_value(&d);
        assert!(matches!(val, Value::Object(_)));
    }
}
