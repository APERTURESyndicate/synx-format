//! SYNX Engine — resolves active markers (:random, :calc, :env, :alias, :secret, etc.)
//! in a parsed SYNX value tree. Only runs in !active mode.

use std::collections::HashMap;
use crate::calc::safe_calc;
use crate::parser;
use crate::rng;
use crate::value::*;

/// Resolve all active-mode markers in a ParseResult.
/// Returns the resolved root Value.
pub fn resolve(result: &mut ParseResult, options: &Options) {
    if result.mode != Mode::Active {
        return;
    }
    let metadata = std::mem::take(&mut result.metadata);

    // ── :inherit pre-pass ──
    apply_inheritance(&mut result.root, &metadata);
    // Remove private blocks (keys starting with _)
    if let Value::Object(ref mut root_map) = result.root {
        root_map.retain(|k, _| !k.starts_with('_'));
    }

    // SAFETY: `root_ptr` is a raw pointer to `result.root` used exclusively
    // for *immutable* read access inside marker handlers (:calc, :alias,
    // :map, :template, :watch) that need to look up other keys in the root
    // while also holding a mutable reference to a child object.
    // The invariants that keep this sound:
    //   1. We never write through `root_ptr` — only reads via `&*root_ptr`.
    //   2. Mutable writes go through `map` (the current object), which is
    //      always a distinct subtree from what we read via `root_ptr`.
    //   3. The pointer is valid for the entire duration of `resolve_value`.
    let root_ptr = &mut result.root as *mut Value;
    resolve_value(&mut result.root, root_ptr, options, &metadata, "");
    result.metadata = metadata;
}

fn resolve_value(
    value: &mut Value,
    root_ptr: *mut Value,
    options: &Options,
    metadata: &HashMap<String, MetaMap>,
    path: &str,
) {
    let meta_map = metadata.get(path).cloned();

    if let Value::Object(ref mut map) = value {
        let keys: Vec<String> = map.keys().cloned().collect();

        // First pass: recurse into nested objects/arrays
        for key in &keys {
            let child_path = if path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", path, key)
            };

            if let Some(child) = map.get_mut(key) {
                match child {
                    Value::Object(_) => {
                        resolve_value(child, root_ptr, options, metadata, &child_path);
                    }
                    Value::Array(arr) => {
                        for item in arr.iter_mut() {
                            if let Value::Object(_) = item {
                                resolve_value(item, root_ptr, options, metadata, &child_path);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Second pass: apply markers
        if let Some(ref mm) = meta_map {
            for key in &keys {
                let meta = match mm.get(key) {
                    Some(m) => m.clone(),
                    None => continue,
                };

                apply_markers(map, key, &meta, root_ptr, options, path, metadata);
            }
        }
    }
}

fn apply_markers(
    map: &mut HashMap<String, Value>,
    key: &str,
    meta: &Meta,
    root_ptr: *mut Value,
    options: &Options,
    _path: &str,
    _metadata: &HashMap<String, MetaMap>,
) {
    let markers = &meta.markers;

    // ── :include ──
    if markers.contains(&"include".to_string()) {
        if let Some(Value::String(file_path)) = map.get(key) {
            let base = options
                .base_path
                .as_deref()
                .unwrap_or(".");
            let full = std::path::Path::new(base).join(file_path);
            match std::fs::read_to_string(&full) {
                Ok(text) => {
                    let mut included = parser::parse(&text);
                    if included.mode == Mode::Active {
                        let mut child_opts = options.clone();
                        if let Some(parent) = full.parent() {
                            child_opts.base_path = Some(parent.to_string_lossy().into_owned());
                        }
                        resolve(&mut included, &child_opts);
                    }
                    map.insert(key.to_string(), included.root);
                }
                Err(e) => {
                    map.insert(
                        key.to_string(),
                        Value::String(format!("INCLUDE_ERR: {}", e)),
                    );
                }
            }
        }
        return;
    }

    // ── :env ──
    if markers.contains(&"env".to_string()) {
        if let Some(Value::String(var_name)) = map.get(key) {
            let env_val = if let Some(ref env_map) = options.env {
                env_map.get(var_name.as_str()).cloned()
            } else {
                std::env::var(var_name).ok()
            };

            let force_string = meta.type_hint.as_deref() == Some("string");
            let default_idx = markers.iter().position(|m| m == "default");
            if let Some(val) = env_val.filter(|v| !v.is_empty()) {
                let resolved = if force_string {
                    Value::String(val)
                } else {
                    cast_primitive(&val)
                };
                map.insert(key.to_string(), resolved);
            } else if let Some(di) = default_idx {
                if markers.len() > di + 1 {
                    // Join all parts after 'default' back with ':'
                    // to preserve IPs (0.0.0.0) and compound values
                    let fallback = markers[di + 1..].join(":");
                    let resolved = if force_string {
                        Value::String(fallback)
                    } else {
                        cast_primitive(&fallback)
                    };
                    map.insert(key.to_string(), resolved);
                } else {
                    map.insert(key.to_string(), Value::Null);
                }
            } else {
                map.insert(key.to_string(), Value::Null);
            }
        }
    }

    // ── :random ──
    if markers.contains(&"random".to_string()) {
        if let Some(Value::Array(arr)) = map.get(key) {
            if arr.is_empty() {
                map.insert(key.to_string(), Value::Null);
                return;
            }
            let picked = if !meta.args.is_empty() {
                let weights: Vec<f64> = meta.args.iter().filter_map(|s| s.parse().ok()).collect();
                weighted_random(arr, &weights)
            } else {
                arr[rng::random_usize(arr.len())].clone()
            };
            map.insert(key.to_string(), picked);
        }
    }

    // ── :ref ──
    // Like :alias but feeds the resolved value into subsequent markers.
    // Supports :ref:calc shorthand: key:ref:calc:*2 base_rate → resolves base_rate, then applies "VALUE * 2".
    if markers.contains(&"ref".to_string()) {
        if let Some(Value::String(target)) = map.get(key) {
            let root_ref = unsafe { &*root_ptr };
            let resolved = deep_get(root_ref, target)
                .or_else(|| map.get(target.as_str()).cloned())
                .unwrap_or(Value::Null);

            // If :calc follows with a shorthand expression
            if markers.contains(&"calc".to_string()) {
                if let Some(n) = value_as_number(&resolved) {
                    let calc_idx = markers.iter().position(|m| m == "calc").unwrap();
                    if let Some(calc_expr) = markers.get(calc_idx + 1) {
                        let first = calc_expr.chars().next().unwrap_or(' ');
                        if "+-*/%".contains(first) {
                            let expr = format!("{} {}", format_number(n), calc_expr);
                            match safe_calc(&expr) {
                                Ok(result) => {
                                    let v = if result.fract() == 0.0 && result.abs() < i64::MAX as f64 {
                                        Value::Int(result as i64)
                                    } else {
                                        Value::Float(result)
                                    };
                                    map.insert(key.to_string(), v);
                                }
                                Err(e) => {
                                    map.insert(key.to_string(), Value::String(format!("CALC_ERR: {}", e)));
                                }
                            }
                        } else {
                            map.insert(key.to_string(), resolved);
                        }
                    } else {
                        map.insert(key.to_string(), resolved);
                    }
                } else {
                    map.insert(key.to_string(), resolved);
                }
            } else {
                map.insert(key.to_string(), resolved);
            }
        }
    }

    // ── :i18n ──
    // Selects a localized value from a nested object based on options.lang.
    if markers.contains(&"i18n".to_string()) {
        if let Some(Value::Object(translations)) = map.get(key) {
            let lang = options.lang.as_deref().unwrap_or("en");
            let val = translations.get(lang)
                .or_else(|| translations.get("en"))
                .or_else(|| translations.values().next())
                .cloned()
                .unwrap_or(Value::Null);
            map.insert(key.to_string(), val);
        }
    }

    // ── :calc ──
    if markers.contains(&"calc".to_string()) {
        if let Some(Value::String(expr)) = map.get(key) {
            let mut resolved = expr.clone();

            // Substitute variables from root
            let root_ref = unsafe { &*root_ptr };
            if let Value::Object(ref root_map) = root_ref {
                for (rk, rv) in root_map {
                    if let Some(n) = value_as_number(rv) {
                        resolved = replace_word(&resolved, rk, &format_number(n));
                    }
                }
            }

            // Substitute from current object
            for (rk, rv) in map.iter() {
                if rk != key {
                    if let Some(n) = value_as_number(rv) {
                        resolved = replace_word(&resolved, rk, &format_number(n));
                    }
                }
            }

            match safe_calc(&resolved) {
                Ok(result) => {
                    let v = if result.fract() == 0.0 && result.abs() < i64::MAX as f64 {
                        Value::Int(result as i64)
                    } else {
                        Value::Float(result)
                    };
                    map.insert(key.to_string(), v);
                }
                Err(e) => {
                    map.insert(
                        key.to_string(),
                        Value::String(format!("CALC_ERR: {}", e)),
                    );
                }
            }
        }
    }

    // ── :alias ──
    if markers.contains(&"alias".to_string()) {
        if let Some(Value::String(target)) = map.get(key) {
            let root_ref = unsafe { &*root_ptr };
            let val = deep_get(root_ref, target).unwrap_or(Value::Null);
            map.insert(key.to_string(), val);
        }
    }

    // ── :secret ──
    if markers.contains(&"secret".to_string()) {
        if let Some(val) = map.get(key) {
            let s = value_to_string(val);
            map.insert(key.to_string(), Value::Secret(s));
        }
    }

    // ── :unique ──
    if markers.contains(&"unique".to_string()) {
        if let Some(Value::Array(arr)) = map.get(key) {
            let mut seen = Vec::new();
            let mut unique = Vec::new();
            for item in arr {
                let s = value_to_string(item);
                if !seen.contains(&s) {
                    seen.push(s);
                    unique.push(item.clone());
                }
            }
            map.insert(key.to_string(), Value::Array(unique));
        }
    }

    // ── :geo ──
    if markers.contains(&"geo".to_string()) {
        if let Some(Value::Array(arr)) = map.get(key) {
            let region = options.region.as_deref().unwrap_or("US");
            let prefix = format!("{} ", region);
            let found = arr.iter().find(|item| {
                if let Value::String(s) = item {
                    s.starts_with(&prefix)
                } else {
                    false
                }
            });

            let result = if let Some(Value::String(s)) = found {
                Value::String(s[prefix.len()..].trim().to_string())
            } else if let Some(first) = arr.first() {
                if let Value::String(s) = first {
                    if let Some(space) = s.find(' ') {
                        Value::String(s[space + 1..].trim().to_string())
                    } else {
                        first.clone()
                    }
                } else {
                    first.clone()
                }
            } else {
                Value::Null
            };
            map.insert(key.to_string(), result);
        }
    }

    // ── :template ──
    if markers.contains(&"template".to_string()) {
        if let Some(Value::String(tpl)) = map.get(key) {
            let root_ref = unsafe { &*root_ptr };
            let result = resolve_template(tpl, root_ref, map);
            map.insert(key.to_string(), Value::String(result));
        }
    }

    // ── :split ──
    if markers.contains(&"split".to_string()) {
        if let Some(Value::String(s)) = map.get(key) {
            let split_idx = markers.iter().position(|m| m == "split").unwrap();
            let sep = if split_idx + 1 < markers.len() {
                delimiter_from_keyword(&markers[split_idx + 1])
            } else {
                ",".to_string()
            };
            let items: Vec<Value> = s
                .split(&sep)
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .map(|p| cast_primitive(p))
                .collect();
            map.insert(key.to_string(), Value::Array(items));
        }
    }

    // ── :join ──
    if markers.contains(&"join".to_string()) {
        if let Some(Value::Array(arr)) = map.get(key) {
            let join_idx = markers.iter().position(|m| m == "join").unwrap();
            let sep = if join_idx + 1 < markers.len() {
                delimiter_from_keyword(&markers[join_idx + 1])
            } else {
                ",".to_string()
            };
            let joined: String = arr
                .iter()
                .map(|v| value_to_string(v))
                .collect::<Vec<_>>()
                .join(&sep);
            map.insert(key.to_string(), Value::String(joined));
        }
    }

    // ── :default (standalone, without :env) ──
    if markers.contains(&"default".to_string()) && !markers.contains(&"env".to_string()) {
        let is_empty = match map.get(key) {
            Some(Value::Null) | None => true,
            Some(Value::String(s)) if s.is_empty() => true,
            _ => false,
        };
        if is_empty {
            let di = markers.iter().position(|m| m == "default").unwrap();
            if markers.len() > di + 1 {
                let fallback = markers[di + 1..].join(":");
                let resolved = if meta.type_hint.as_deref() == Some("string") {
                    Value::String(fallback)
                } else {
                    cast_primitive(&fallback)
                };
                map.insert(key.to_string(), resolved);
            }
        }
    }

    // ── :clamp ──
    // Syntax: key:clamp:MIN:MAX value
    // Clamps a numeric value to [MIN, MAX].
    if markers.contains(&"clamp".to_string()) {
        let clamp_idx = markers.iter().position(|m| m == "clamp").unwrap();
        let min_s = markers.get(clamp_idx + 1).cloned().unwrap_or_default();
        let max_s = markers.get(clamp_idx + 2).cloned().unwrap_or_default();
        if let (Ok(lo), Ok(hi)) = (min_s.parse::<f64>(), max_s.parse::<f64>()) {
            if lo > hi {
                map.insert(key.to_string(), Value::String(
                    format!("CONSTRAINT_ERR: clamp min ({}) > max ({})", lo, hi),
                ));
            } else if let Some(n) = map.get(key).and_then(value_as_number) {
                let clamped = n.clamp(lo, hi);
                let v = if clamped.fract() == 0.0 && clamped.abs() < i64::MAX as f64 {
                    Value::Int(clamped as i64)
                } else {
                    Value::Float(clamped)
                };
                map.insert(key.to_string(), v);
            }
        }
    }

    // ── :round ──
    // Syntax: key:round:N value  (N = decimal places, default 0)
    // Works standalone or after :calc: key:calc:round:2 expr
    if markers.contains(&"round".to_string()) {
        let round_idx = markers.iter().position(|m| m == "round").unwrap();
        let decimals: u32 = markers.get(round_idx + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if let Some(n) = map.get(key).and_then(value_as_number) {
            let factor = 10f64.powi(decimals as i32);
            let rounded = (n * factor).round() / factor;
            let v = if decimals == 0 {
                Value::Int(rounded as i64)
            } else {
                Value::Float(rounded)
            };
            map.insert(key.to_string(), v);
        }
    }

    // ── :map ──
    // Syntax: key:map:source_key\n  - lookup_val result
    // Looks up `source_key` in root, finds matching "lookup_val result" entry in the array.
    if markers.contains(&"map".to_string()) {
        if let Some(Value::Array(arr)) = map.get(key) {
            let map_idx = markers.iter().position(|m| m == "map").unwrap();
            let source_key = markers.get(map_idx + 1).cloned().unwrap_or_default();
            let lookup_val = if !source_key.is_empty() {
                let root_ref = unsafe { &*root_ptr };
                deep_get(root_ref, &source_key)
                    .or_else(|| map.get(&source_key).cloned())
                    .map(|v| value_to_string(&v))
                    .unwrap_or_default()
            } else {
                // Use the current string value as lookup key
                match map.get(key) {
                    Some(Value::String(s)) => s.clone(),
                    _ => String::new(),
                }
            };

            // Find matching entry: "lookup_val result_text"
            let arr_clone = arr.clone();
            let result = arr_clone.iter().find_map(|item| {
                if let Value::String(s) = item {
                    if let Some(space) = s.find(' ') {
                        if s[..space].trim() == lookup_val {
                            return Some(cast_primitive(s[space + 1..].trim()));
                        }
                    }
                }
                None
            });
            map.insert(key.to_string(), result.unwrap_or(Value::Null));
        }
    }

    // ── :format ──
    // Syntax: key:format:PATTERN value  (printf-style: %.2f, %d, %05d, %e)
    // Converts numeric or string value to a formatted string.
    if markers.contains(&"format".to_string()) {
        let fmt_idx = markers.iter().position(|m| m == "format").unwrap();
        let pattern = markers.get(fmt_idx + 1).cloned().unwrap_or_else(|| "%s".to_string());
        if let Some(current) = map.get(key) {
            let formatted = apply_format_pattern(&pattern, current);
            map.insert(key.to_string(), Value::String(formatted));
        }
    }

    // ── :fallback ──
    // Syntax: key:fallback:DEFAULT_PATH value
    // If the value (treated as a file path) doesn't exist on disk, use the fallback.
    // Falls back to default if value is also null/empty.
    if markers.contains(&"fallback".to_string()) {
        let fb_idx = markers.iter().position(|m| m == "fallback").unwrap();
        let default_val = markers.get(fb_idx + 1).cloned().unwrap_or_default();
        let use_fallback = match map.get(key) {
            None | Some(Value::Null) => true,
            Some(Value::String(s)) if s.is_empty() => true,
            Some(Value::String(s)) => {
                let base = options.base_path.as_deref().unwrap_or(".");
                !std::path::Path::new(base).join(s).exists()
            }
            _ => false,
        };
        if use_fallback && !default_val.is_empty() {
            map.insert(key.to_string(), Value::String(default_val));
        }
    }

    // ── :once ──
    // Syntax: key:once  or  key:once:uuid  or  key:once:random  or  key:once:timestamp
    // Generates a value once and persists it in a .synx.lock sidecar file.
    if markers.contains(&"once".to_string()) {
        let once_idx = markers.iter().position(|m| m == "once").unwrap();
        let gen_type = markers.get(once_idx + 1).map(|s| s.as_str()).unwrap_or("uuid");
        let lock_path = options.base_path.as_deref()
            .map(|b| std::path::Path::new(b).join(".synx.lock"))
            .unwrap_or_else(|| std::path::Path::new(".synx.lock").to_path_buf());

        // Try to read existing value from lock file
        let existing = read_lock_value(&lock_path, key);
        if let Some(locked) = existing {
            map.insert(key.to_string(), Value::String(locked));
        } else {
            let generated = match gen_type {
                "uuid" => rng::generate_uuid(),
                "timestamp" => std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string(),
                "random" => rng::random_usize(u32::MAX as usize).to_string(),
                _ => rng::generate_uuid(),
            };
            write_lock_value(&lock_path, key, &generated);
            map.insert(key.to_string(), Value::String(generated));
        }
    }

    // ── :version ──
    // Syntax: key:version:OP:REQUIRED value
    // Compares the value (current version) against REQUIRED using OP (>=, <=, >, <, ==, !=).
    // Returns a bool.
    if markers.contains(&"version".to_string()) {
        if let Some(Value::String(current_ver)) = map.get(key) {
            let ver_idx = markers.iter().position(|m| m == "version").unwrap();
            let op = markers.get(ver_idx + 1).map(|s| s.as_str()).unwrap_or(">=");
            let required = markers.get(ver_idx + 2).cloned().unwrap_or_default();
            let result = compare_versions(current_ver, op, &required);
            map.insert(key.to_string(), Value::Bool(result));
        }
    }

    // ── :watch ──
    // Syntax: key:watch:KEY_PATH ./file.json  (or ./file.synx)
    // Reads the referenced file at parse time. Optionally extracts a key path (JSON/SYNX).
    if markers.contains(&"watch".to_string()) {
        if let Some(Value::String(file_path)) = map.get(key) {
            let base = options.base_path.as_deref().unwrap_or(".");
            let full = std::path::Path::new(base).join(file_path);
            let watch_idx = markers.iter().position(|m| m == "watch").unwrap();
            let key_path = markers.get(watch_idx + 1).cloned();

            match std::fs::read_to_string(&full) {
                Ok(content) => {
                    let value = if let Some(ref kp) = key_path {
                        extract_from_file_content(&content, kp, full.extension().and_then(|e| e.to_str()).unwrap_or("")).unwrap_or(Value::Null)
                    } else {
                        Value::String(content.trim().to_string())
                    };
                    map.insert(key.to_string(), value);
                }
                Err(e) => {
                    map.insert(key.to_string(), Value::String(format!("WATCH_ERR: {}", e)));
                }
            }
        }
    }

    // ── Constraint validation (always last, after all markers resolved) ──
    if let Some(ref c) = meta.constraints {
        validate_constraints(map, key, c);
    }
}

// ─── Constraint enforcement ───────────────────────────────

fn validate_constraints(map: &mut HashMap<String, Value>, key: &str, c: &Constraints) {
    let val = match map.get(key) {
        Some(v) => v.clone(),
        None => {
            if c.required {
                map.insert(key.to_string(), Value::String(
                    format!("CONSTRAINT_ERR: '{}' is required", key),
                ));
            }
            return;
        }
    };

    // required
    if c.required {
        let empty = matches!(val, Value::Null)
            || matches!(&val, Value::String(s) if s.is_empty());
        if empty {
            map.insert(key.to_string(), Value::String(
                format!("CONSTRAINT_ERR: '{}' is required", key),
            ));
            return;
        }
    }

    // type check
    if let Some(ref type_name) = c.type_name {
        let ok = match type_name.as_str() {
            "int"    => matches!(val, Value::Int(_)),
            "float"  => matches!(val, Value::Float(_) | Value::Int(_)),
            "bool"   => matches!(val, Value::Bool(_)),
            "string" => matches!(val, Value::String(_)),
            _        => true,
        };
        if !ok {
            map.insert(key.to_string(), Value::String(
                format!("CONSTRAINT_ERR: '{}' expected type '{}'", key, type_name),
            ));
            return;
        }
    }

    // enum check
    if let Some(ref enum_vals) = c.enum_values {
        let val_str = match &val {
            Value::String(s) => s.clone(),
            Value::Int(n)    => n.to_string(),
            Value::Float(f)  => f.to_string(),
            Value::Bool(b)   => b.to_string(),
            _                => String::new(),
        };
        if !enum_vals.contains(&val_str) {
            map.insert(key.to_string(), Value::String(
                format!("CONSTRAINT_ERR: '{}' must be one of [{}]", key, enum_vals.join("|")),
            ));
            return;
        }
    }

    // min / max  (numbers: value range; strings: length range)
    let num = match &val {
        Value::Int(n)    => Some(*n as f64),
        Value::Float(f)  => Some(*f),
        Value::String(s) if c.min.is_some() || c.max.is_some() => Some(s.len() as f64),
        _                => None,
    };
    if let Some(n) = num {
        if let Some(min) = c.min {
            if n < min {
                map.insert(key.to_string(), Value::String(
                    format!("CONSTRAINT_ERR: '{}' value {} is below min {}", key, n, min),
                ));
                return;
            }
        }
        if let Some(max) = c.max {
            if n > max {
                map.insert(key.to_string(), Value::String(
                    format!("CONSTRAINT_ERR: '{}' value {} exceeds max {}", key, n, max),
                ));
                return;
            }
        }
    }
    // Note: `pattern` validation is intentionally skipped in the Rust engine
    // (adding a regex crate would bloat the dependency tree).  Pattern
    // validation is fully implemented in the JS engine.
}

// ─── New-marker helpers ───────────────────────────────────

/// Apply a printf-style format pattern to a value.
fn apply_format_pattern(pattern: &str, value: &Value) -> String {
    match value {
        Value::Int(n) => {
            if pattern.contains('d') || pattern.contains('i') {
                format_int_pattern(pattern, *n)
            } else if pattern.contains('f') || pattern.contains('e') {
                format_float_pattern(pattern, *n as f64)
            } else {
                n.to_string()
            }
        }
        Value::Float(f) => {
            if pattern.contains('f') || pattern.contains('e') {
                format_float_pattern(pattern, *f)
            } else {
                format_number(*f)
            }
        }
        Value::String(s) => s.clone(),
        other => value_to_string(other),
    }
}

fn format_int_pattern(pattern: &str, n: i64) -> String {
    if let Some(s) = pattern.strip_prefix('%') {
        if let Some(inner) = s.strip_suffix('d').or_else(|| s.strip_suffix('i')) {
            if let Some(w) = inner.strip_prefix('0') {
                if let Ok(width) = w.parse::<usize>() {
                    return format!("{:0>width$}", n, width = width);
                }
            }
            if let Ok(width) = inner.parse::<usize>() {
                return format!("{:>width$}", n, width = width);
            }
        }
    }
    n.to_string()
}

fn format_float_pattern(pattern: &str, f: f64) -> String {
    if let Some(s) = pattern.strip_prefix('%') {
        if let Some(inner) = s.strip_suffix('f').or_else(|| s.strip_suffix('e')) {
            if let Some(prec_s) = inner.strip_prefix('.') {
                if let Ok(prec) = prec_s.parse::<usize>() {
                    return format!("{:.prec$}", f, prec = prec);
                }
            }
        }
    }
    f.to_string()
}

/// Read a persisted value from the .synx.lock file.
fn read_lock_value(lock_path: &std::path::Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(lock_path).ok()?;
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(key) {
            if rest.starts_with(' ') {
                return Some(rest.trim_start().to_string());
            }
        }
    }
    None
}

/// Write/update a key value pair in the .synx.lock file.
fn write_lock_value(lock_path: &std::path::Path, key: &str, value: &str) {
    let mut lines: Vec<String> = std::fs::read_to_string(lock_path)
        .unwrap_or_default()
        .lines()
        .map(|l| l.to_string())
        .collect();

    let new_line = format!("{} {}", key, value);
    let mut found = false;
    for line in lines.iter_mut() {
        if line.starts_with(key) && line[key.len()..].starts_with(' ') {
            *line = new_line.clone();
            found = true;
            break;
        }
    }
    if !found {
        lines.push(new_line);
    }
    let _ = std::fs::write(lock_path, lines.join("\n") + "\n");
}

/// Compare two version strings using a comparison operator.
fn compare_versions(current: &str, op: &str, required: &str) -> bool {
    let parse_ver = |s: &str| -> Vec<u64> {
        s.split('.').filter_map(|p| p.parse().ok()).collect()
    };
    let cv = parse_ver(current);
    let rv = parse_ver(required);
    let len = cv.len().max(rv.len());
    let mut ord = std::cmp::Ordering::Equal;
    for i in 0..len {
        let a = cv.get(i).copied().unwrap_or(0);
        let b = rv.get(i).copied().unwrap_or(0);
        if a != b {
            ord = a.cmp(&b);
            break;
        }
    }
    match op {
        ">=" => ord != std::cmp::Ordering::Less,
        "<=" => ord != std::cmp::Ordering::Greater,
        ">"  => ord == std::cmp::Ordering::Greater,
        "<"  => ord == std::cmp::Ordering::Less,
        "==" | "=" => ord == std::cmp::Ordering::Equal,
        "!=" => ord != std::cmp::Ordering::Equal,
        _ => false,
    }
}

/// Extract a value from file content by key path (JSON dot-path or SYNX key).
fn extract_from_file_content(content: &str, key_path: &str, ext: &str) -> Option<Value> {
    if ext == "json" {
        let search = format!("\"{}\"", key_path);
        if let Some(pos) = content.find(&search) {
            let after = content[pos + search.len()..].trim_start();
            if let Some(rest) = after.strip_prefix(':') {
                let val_s = rest.trim_start()
                    .trim_end_matches(',')
                    .trim_end_matches('}')
                    .trim()
                    .trim_matches('"');
                return Some(cast_primitive(val_s));
            }
        }
        None
    } else {
        for line in content.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with(key_path) {
                let rest = &trimmed[key_path.len()..];
                if rest.starts_with(' ') {
                    return Some(cast_primitive(rest.trim_start()));
                }
            }
        }
        None
    }
}

// ─── Helpers ─────────────────────────────────────────────

fn cast_primitive(val: &str) -> Value {
    match val {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        "null" => Value::Null,
        _ => {
            if let Ok(i) = val.parse::<i64>() {
                Value::Int(i)
            } else if let Ok(f) = val.parse::<f64>() {
                Value::Float(f)
            } else {
                Value::String(val.to_string())
            }
        }
    }
}

fn delimiter_from_keyword(keyword: &str) -> String {
    match keyword {
        "space" => " ".to_string(),
        "pipe" => "|".to_string(),
        "dash" => "-".to_string(),
        "dot" => ".".to_string(),
        "semi" => ";".to_string(),
        "tab" => "\t".to_string(),
        "slash" => "/".to_string(),
        other => other.to_string(),
    }
}

fn value_as_number(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => format_number(*f as f64),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Secret(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => String::new(),
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < i64::MAX as f64 {
        (n as i64).to_string()
    } else {
        n.to_string()
    }
}

/// Replace whole-word occurrences of `word` with `replacement`.
fn replace_word(haystack: &str, word: &str, replacement: &str) -> String {
    let word_bytes = word.as_bytes();
    let word_len = word_bytes.len();
    let hay_bytes = haystack.as_bytes();
    let hay_len = hay_bytes.len();

    if word_len > hay_len {
        return haystack.to_string();
    }

    let mut result = String::with_capacity(hay_len);
    let mut i = 0;

    while i <= hay_len - word_len {
        if &hay_bytes[i..i + word_len] == word_bytes {
            let before_ok = i == 0 || !is_word_char(hay_bytes[i - 1]);
            let after_ok = i + word_len >= hay_len || !is_word_char(hay_bytes[i + word_len]);
            if before_ok && after_ok {
                result.push_str(replacement);
                i += word_len;
                continue;
            }
        }
        result.push(hay_bytes[i] as char);
        i += 1;
    }
    while i < hay_len {
        result.push(hay_bytes[i] as char);
        i += 1;
    }
    result
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn weighted_random(items: &[Value], weights: &[f64]) -> Value {
    let mut w: Vec<f64> = weights.to_vec();
    if w.len() < items.len() {
        let assigned: f64 = w.iter().sum();
        // If the explicit weights already exceed 100, give unassigned items
        // the same average weight as the assigned ones so they remain visible.
        // If there is room left under 100, distribute the remainder equally.
        let per_item = if assigned < 100.0 {
            (100.0 - assigned) / (items.len() - w.len()) as f64
        } else {
            assigned / w.len() as f64
        };
        while w.len() < items.len() {
            w.push(per_item);
        }
    }
    let total: f64 = w.iter().sum();
    if total <= 0.0 {
        return items[rng::random_usize(items.len())].clone();
    }

    let rand_val = rng::random_f64_01();
    let mut cumulative = 0.0;
    for (i, item) in items.iter().enumerate() {
        cumulative += w[i] / total;
        if rand_val <= cumulative {
            return item.clone();
        }
    }
    items.last().cloned().unwrap_or(Value::Null)
}

// ─── Inheritance pre-pass ─────────────────────────────────

fn apply_inheritance(root: &mut Value, metadata: &HashMap<String, MetaMap>) {
    let root_meta = match metadata.get("") {
        Some(m) => m.clone(),
        None => return,
    };

    let root_map = match root.as_object_mut() {
        Some(m) => m as *mut HashMap<String, Value>,
        None => return,
    };

    // Collect inherit targets first to avoid borrow issues
    let mut inherits: Vec<(String, String)> = Vec::new();
    for (key, meta) in &root_meta {
        if meta.markers.contains(&"inherit".to_string()) {
            let idx = meta.markers.iter().position(|m| m == "inherit").unwrap();
            if let Some(parent_name) = meta.markers.get(idx + 1) {
                inherits.push((key.clone(), parent_name.clone()));
            }
        }
    }

    let map = unsafe { &mut *root_map };
    for (child_key, parent_name) in &inherits {
        let parent_fields: HashMap<String, Value> = match map.get(parent_name) {
            Some(Value::Object(p)) => p.clone(),
            _ => continue,
        };
        let child_fields: HashMap<String, Value> = match map.get(child_key) {
            Some(Value::Object(c)) => c.clone(),
            _ => continue,
        };
        // Merge: parent fields first, child overrides
        let mut merged = parent_fields;
        for (k, v) in child_fields {
            merged.insert(k, v);
        }
        map.insert(child_key.clone(), Value::Object(merged));
    }
}

fn deep_get(root: &Value, path: &str) -> Option<Value> {
    // Try direct key
    if let Value::Object(map) = root {
        if let Some(val) = map.get(path) {
            return Some(val.clone());
        }
    }
    // Dot-path traversal
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = root;
    for part in parts {
        match current {
            Value::Object(map) => match map.get(part) {
                Some(v) => current = v,
                None => return None,
            },
            _ => return None,
        }
    }
    Some(current.clone())
}

/// Resolve {placeholder} references in a template string.
fn resolve_template(tpl: &str, root: &Value, local_map: &HashMap<String, Value>) -> String {
    let bytes = tpl.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        if bytes[i] == b'{' {
            if let Some(close) = tpl[i + 1..].find('}') {
                let ref_name = &tpl[i + 1..i + 1 + close];
                // Only resolve if it looks like a valid reference
                if ref_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                    let resolved = deep_get(root, ref_name).or_else(|| {
                        if let Value::Object(m) = root {
                            // try local
                            let _ = m;
                        }
                        local_map.get(ref_name).cloned()
                    });
                    if let Some(val) = resolved {
                        result.push_str(&value_to_string(&val));
                    } else {
                        result.push('{');
                        result.push_str(ref_name);
                        result.push('}');
                    }
                    i += 2 + close;
                    continue;
                }
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use crate::{parse, Options, Value};
    use super::resolve;

    #[test]
    fn test_ref_simple() {
        let mut r = parse("!active\nbase_rate 50\nquick_rate:ref base_rate");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["quick_rate"], Value::Int(50));
    }

    #[test]
    fn test_ref_calc_shorthand() {
        let mut r = parse("!active\nbase_rate 50\ndouble_rate:ref:calc:*2 base_rate");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["double_rate"], Value::Int(100));
    }

    #[test]
    fn test_inherit() {
        let mut r = parse("!active\n_base\n  weight 10\n  stackable true\nsteel:inherit:_base\n  weight 25\n  material metal");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert!(!map.contains_key("_base"));
        let steel = map["steel"].as_object().unwrap();
        assert_eq!(steel["weight"], Value::Int(25));
        assert_eq!(steel["stackable"], Value::Bool(true));
        assert_eq!(steel["material"], Value::String("metal".into()));
    }

    #[test]
    fn test_i18n_select_lang() {
        let mut r = parse("!active\ntitle:i18n\n  en Hello\n  ru Привет\n  de Hallo");
        let opts = Options { lang: Some("ru".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["title"], Value::String("Привет".into()));
    }

    #[test]
    fn test_i18n_fallback_en() {
        let mut r = parse("!active\ntitle:i18n\n  en Hello\n  ru Привет");
        let opts = Options { lang: Some("fr".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["title"], Value::String("Hello".into()));
    }
}
