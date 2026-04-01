//! SYNX Engine — resolves active markers (:random, :calc, :env, :alias, :secret, etc.)
//! in a parsed SYNX value tree. Only runs in !active mode.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use crate::calc::safe_calc;
use crate::parser;
use crate::rng;
use crate::value::*;

static SPAM_BUCKETS: OnceLock<Mutex<HashMap<String, Vec<Instant>>>> = OnceLock::new();

/// Maximum expression length accepted by :calc (prevents ReDoS/stack abuse).
const MAX_CALC_EXPR_LEN: usize = 4096;
/// Maximum resolved expression length produced by :calc substitutions.
/// Prevents pathological inputs from growing the expression until OOM.
const MAX_CALC_RESOLVED_LEN: usize = 64 * 1024;
/// Maximum file size for :include / :watch reads (10 MB).
const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;
/// Default maximum include depth.
const DEFAULT_MAX_INCLUDE_DEPTH: usize = 16;
/// Maximum object nesting depth for active-mode resolution (prevents stack overflow).
const MAX_RESOLVE_DEPTH: usize = 512;

/// Upper bound for single `String` scratch buffers built from hostile `:template` / replace paths.
const MAX_ENGINE_SCRATCH_STRING: usize = 4 * 1024 * 1024;

/// Validate that `full` path stays within the `base` directory (jail).
/// Returns `Ok(canonical)` or an `Err` describing the violation.
fn jail_path(base: &str, file_path: &str) -> Result<std::path::PathBuf, String> {
    // Block absolute paths in the value itself
    let fp = std::path::Path::new(file_path);
    if fp.is_absolute() {
        return Err(format!("SECURITY: absolute paths are not allowed: '{}'", file_path));
    }
    let base_canonical = match std::fs::canonicalize(base) {
        Ok(p) => p,
        Err(_) => std::path::PathBuf::from(base),
    };
    let full = base_canonical.join(file_path);
    let full_canonical = match std::fs::canonicalize(&full) {
        Ok(p) => p,
        Err(_) => {
            // File may not exist yet — at least verify no ".." escapes
            let normalized = full.to_string_lossy();
            if normalized.contains("..") {
                return Err(format!("SECURITY: path traversal detected: '{}'", file_path));
            }
            return Ok(full);
        }
    };
    if !full_canonical.starts_with(&base_canonical) {
        return Err(format!("SECURITY: path escapes base directory: '{}'", file_path));
    }
    Ok(full_canonical)
}

/// Check file size before reading.
fn check_file_size(path: &std::path::Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_FILE_SIZE => {
            Err(format!("SECURITY: file too large ({} bytes, max {})", meta.len(), MAX_FILE_SIZE))
        }
        _ => Ok(()),
    }
}

/// Resolve all active-mode markers in a ParseResult.
/// Returns the resolved root Value.
pub fn resolve(result: &mut ParseResult, options: &Options) {
    if result.mode != Mode::Active {
        return;
    }
    let metadata = std::mem::take(&mut result.metadata);
    let includes_directives = std::mem::take(&mut result.includes);

    // ── Load !include files ──
    let includes_map = load_includes(&includes_directives, options);

    // ── :inherit pre-pass ──
    apply_inheritance(&mut result.root, &metadata);
    // Remove private blocks (keys starting with _)
    if let Value::Object(ref mut root_map) = result.root {
        root_map.retain(|k, _| !k.starts_with('_'));
    }

    // ── Build type registry ──
    let type_registry = build_type_registry(&metadata);
    // ── Build constraint registry ──
    let constraint_registry = build_constraint_registry(&metadata);

    // SAFETY: `root_ptr` is a raw pointer to `result.root` used exclusively
    // for *immutable* read access inside marker handlers (:calc, :alias,
    // :map, :watch) that need to look up other keys in the root
    // while also holding a mutable reference to a child object.
    // The invariants that keep this sound:
    //   1. We never write through `root_ptr` — only reads via `&*root_ptr`.
    //   2. Mutable writes go through `map` (the current object), which is
    //      always a distinct subtree from what we read via `root_ptr`.
    //   3. The pointer is valid for the entire duration of `resolve_value`.
    let root_ptr = &mut result.root as *mut Value;
    resolve_value(&mut result.root, root_ptr, options, &metadata, "", &includes_map, 0);

    // ── Validate field constraints (global, by field name) ──
    validate_field_constraints(&mut result.root, &constraint_registry);
    
    // ── Validate field types ──
    validate_field_types(&mut result.root, &type_registry, "");
    
    result.metadata = metadata;
    result.includes = includes_directives;
}

fn resolve_value(
    value: &mut Value,
    root_ptr: *mut Value,
    options: &Options,
    metadata: &HashMap<String, MetaMap>,
    path: &str,
    includes: &HashMap<String, Value>,
    depth: usize,
) {
    // Guard: prevent stack overflow from deeply nested objects
    if depth >= MAX_RESOLVE_DEPTH {
        // Safety: recursion only descends into Object variants (see lines below),
        // so value is always an Object here. Non-Object values are safe to skip.
        if let Value::Object(ref mut map) = value {
            for val in map.values_mut() {
                *val = Value::String(
                    "NESTING_ERR: maximum object nesting depth exceeded".to_string()
                );
            }
        }
        return;
    }

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
                        resolve_value(child, root_ptr, options, metadata, &child_path, includes, depth + 1);
                    }
                    Value::Array(arr) => {
                        for item in arr.iter_mut() {
                            if let Value::Object(_) = item {
                                resolve_value(item, root_ptr, options, metadata, &child_path, includes, depth + 1);
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

                apply_markers(map, key, &meta, root_ptr, options, path, metadata, includes);
            }
        }

        // Third pass: auto-{} interpolation on all string values
        let keys2: Vec<String> = map.keys().cloned().collect();
        for key in &keys2 {
            if let Some(Value::String(s)) = map.get(key) {
                if s.contains('{') {
                    let root_ref = unsafe { &*root_ptr };
                    let result = resolve_interpolation(s, root_ref, map, includes);
                    if result != *s {
                        map.insert(key.to_string(), Value::String(result));
                    }
                }
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
    path: &str,
    metadata: &HashMap<String, MetaMap>,
    _includes: &HashMap<String, Value>,
) {
    let markers = &meta.markers;

    // ── :spam ──
    // Syntax: key:spam:MAX_CALLS:WINDOW_SEC target
    // WINDOW_SEC defaults to 1 when omitted.
    // If target is a key path, resolves its value after passing the limit check.
    if markers.contains(&"spam".to_string()) {
        let spam_idx = markers.iter().position(|m| m == "spam").unwrap();
        let max_calls = markers
            .get(spam_idx + 1)
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        let window_sec = markers
            .get(spam_idx + 2)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(1);

        if max_calls == 0 {
            map.insert(
                key.to_string(),
                Value::String("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]".to_string()),
            );
            return;
        }

        let target = map
            .get(key)
            .map(value_to_string)
            .unwrap_or_else(|| key.to_string());
        let bucket_key = format!("{}::{}", key, target);

        if !allow_spam_access(&bucket_key, max_calls, window_sec) {
            map.insert(
                key.to_string(),
                Value::String(format!(
                    "SPAM_ERR: '{}' exceeded {} calls per {}s",
                    target, max_calls, window_sec
                )),
            );
            return;
        }

        if let Some(resolved) = map
            .get(key)
            .and_then(|v| {
                let t = value_to_string(v);
                let root_ref = unsafe { &*root_ptr };
                deep_get(root_ref, &t).or_else(|| map.get(t.as_str()).cloned())
            })
        {
            map.insert(key.to_string(), resolved);
        }
    }

    // ── :include / :import ──
    if markers.contains(&"include".to_string()) || markers.contains(&"import".to_string()) {
        if let Some(Value::String(file_path)) = map.get(key) {
            let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
            if options._include_depth >= max_depth {
                map.insert(
                    key.to_string(),
                    Value::String(format!("INCLUDE_ERR: max include depth ({}) exceeded", max_depth)),
                );
                return;
            }
            let base = options
                .base_path
                .as_deref()
                .unwrap_or(".");
            let full = match jail_path(base, file_path) {
                Ok(p) => p,
                Err(e) => {
                    map.insert(key.to_string(), Value::String(format!("INCLUDE_ERR: {}", e)));
                    return;
                }
            };
            if let Err(e) = check_file_size(&full) {
                map.insert(key.to_string(), Value::String(format!("INCLUDE_ERR: {}", e)));
                return;
            }
            match std::fs::read_to_string(&full) {
                Ok(text) => {
                    let mut included = parser::parse(&text);
                    if included.mode == Mode::Active {
                        let mut child_opts = options.clone();
                        child_opts._include_depth += 1;
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
    // Supports pluralization: key:i18n:COUNT_FIELD
    //   When count field is specified, the language entry must contain plural forms:
    //   title:i18n:item_count
    //     en
    //       one {count} item
    //       other {count} items
    //     ru
    //       one {count} предмет
    //       few {count} предмета
    //       many {count} предметов
    //       other {count} предметов
    if markers.contains(&"i18n".to_string()) {
        if let Some(Value::Object(translations)) = map.get(key) {
            let lang = options.lang.as_deref().unwrap_or("en");
            let val = translations.get(lang)
                .or_else(|| translations.get("en"))
                .or_else(|| translations.values().next())
                .cloned()
                .unwrap_or(Value::Null);

            // Check for pluralization: i18n:count_field
            let i18n_idx = markers.iter().position(|m| m == "i18n").unwrap();
            let count_field = markers.get(i18n_idx + 1).cloned();

            if let (Some(ref cf), Value::Object(ref plural_forms)) = (&count_field, &val) {
                // Look up count value from current map or root
                let count_val = map.get(cf)
                    .and_then(value_as_number)
                    .or_else(|| {
                        let root_ref = unsafe { &*root_ptr };
                        deep_get(root_ref, cf).and_then(|v| value_as_number(&v))
                    })
                    .unwrap_or(0.0) as i64;

                let category = plural_category(lang, count_val);
                let chosen = plural_forms.get(category)
                    .or_else(|| plural_forms.get("other"))
                    .or_else(|| plural_forms.values().next())
                    .cloned()
                    .unwrap_or(Value::Null);

                // Substitute {count} in the result string
                if let Value::String(ref s) = chosen {
                    let replaced = s.replace("{count}", &count_val.to_string());
                    map.insert(key.to_string(), Value::String(replaced));
                } else {
                    map.insert(key.to_string(), chosen);
                }
            } else {
                map.insert(key.to_string(), val);
            }
        }
    }

    // ── :calc ──
    if markers.contains(&"calc".to_string()) {
        if let Some(Value::String(expr)) = map.get(key) {
            if expr.len() > MAX_CALC_EXPR_LEN {
                map.insert(
                    key.to_string(),
                    Value::String(format!("CALC_ERR: expression too long ({} chars, max {})", expr.len(), MAX_CALC_EXPR_LEN)),
                );
                return;
            }
            let mut resolved = expr.clone();

            // Substitute variables from root (flat keys)
            let root_ref = unsafe { &*root_ptr };
            if let Value::Object(ref root_map) = root_ref {
                for (rk, rv) in root_map {
                    if let Some(n) = value_as_number(rv) {
                        resolved = replace_word(&resolved, rk, &format_number(n));
                        if resolved.len() > MAX_CALC_RESOLVED_LEN {
                            map.insert(
                                key.to_string(),
                                Value::String(format!(
                                    "CALC_ERR: resolved expression too long (max {} bytes)",
                                    MAX_CALC_RESOLVED_LEN
                                )),
                            );
                            return;
                        }
                    }
                }
            }

            // Substitute from current object (flat keys)
            for (rk, rv) in map.iter() {
                if rk != key {
                    if let Some(n) = value_as_number(rv) {
                        resolved = replace_word(&resolved, rk, &format_number(n));
                        if resolved.len() > MAX_CALC_RESOLVED_LEN {
                            map.insert(
                                key.to_string(),
                                Value::String(format!(
                                    "CALC_ERR: resolved expression too long (max {} bytes)",
                                    MAX_CALC_RESOLVED_LEN
                                )),
                            );
                            return;
                        }
                    }
                }
            }

            // Substitute dot-path references (e.g., base.hp, server.port)
            let root_ref2 = unsafe { &*root_ptr };
            let mut dot_resolved = String::new();
            let bytes = resolved.as_bytes();
            let len = bytes.len();
            let mut i = 0;
            while i < len {
                if is_word_char(bytes[i]) {
                    let start = i;
                    let mut has_dot = false;
                    while i < len && (is_word_char(bytes[i]) || bytes[i] == b'.') {
                        if bytes[i] == b'.' { has_dot = true; }
                        i += 1;
                    }
                    let token = &resolved[start..i];
                    if has_dot && token.contains('.') {
                        if let Some(val) = deep_get(root_ref2, token) {
                            if let Some(n) = value_as_number(&val) {
                                dot_resolved.push_str(&format_number(n));
                                if dot_resolved.len() > MAX_CALC_RESOLVED_LEN {
                                    map.insert(
                                        key.to_string(),
                                        Value::String(format!(
                                            "CALC_ERR: resolved expression too long (max {} bytes)",
                                            MAX_CALC_RESOLVED_LEN
                                        )),
                                    );
                                    return;
                                }
                                continue;
                            }
                        }
                    }
                    dot_resolved.push_str(token);
                    if dot_resolved.len() > MAX_CALC_RESOLVED_LEN {
                        map.insert(
                            key.to_string(),
                            Value::String(format!(
                                "CALC_ERR: resolved expression too long (max {} bytes)",
                                MAX_CALC_RESOLVED_LEN
                            )),
                        );
                        return;
                    }
                } else {
                    dot_resolved.push(bytes[i] as char);
                    i += 1;
                    if dot_resolved.len() > MAX_CALC_RESOLVED_LEN {
                        map.insert(
                            key.to_string(),
                            Value::String(format!(
                                "CALC_ERR: resolved expression too long (max {} bytes)",
                                MAX_CALC_RESOLVED_LEN
                            )),
                        );
                        return;
                    }
                }
            }
            resolved = dot_resolved;

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
            let target = target.clone();
            // Build the full dot-path of the current key
            let current_path = if path.is_empty() {
                key.to_string()
            } else {
                format!("{}.{}", path, key)
            };
            // Detect direct self-reference: key:alias key
            if target == key || target == current_path {
                map.insert(
                    key.to_string(),
                    Value::String(format!("ALIAS_ERR: self-referential alias: {} → {}", current_path, target)),
                );
            } else {
                // Detect one-hop cycle: a → b → a
                // Only flag as cycle if the target key ALSO has an :alias marker.
                // Without this check, plain string values that happen to match the current
                // key name would produce false-positive ALIAS_ERR results.
                let root_ref = unsafe { &*root_ptr };
                let target_val = deep_get(root_ref, &target);
                // Determine the metadata path of the target key
                let (target_parent, target_key_name) = if let Some(dot) = target.rfind('.') {
                    (target[..dot].to_string(), target[dot + 1..].to_string())
                } else {
                    (String::new(), target.clone())
                };
                let target_has_alias = metadata
                    .get(&target_parent)
                    .and_then(|mm| mm.get(&target_key_name))
                    .map(|m| m.markers.contains(&"alias".to_string()))
                    .unwrap_or(false);
                let is_cycle = target_has_alias && match &target_val {
                    Some(Value::String(s)) => s == key || s == &current_path,
                    _ => false,
                };
                if is_cycle {
                    map.insert(
                        key.to_string(),
                        Value::String(format!("ALIAS_ERR: circular alias detected: {} → {}", current_path, target)),
                    );
                } else {
                    let val = target_val.unwrap_or(Value::Null);
                    map.insert(key.to_string(), val);
                }
            }
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

    // ── :template (legacy — handled by auto-{} in resolve_value) ──

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
                match jail_path(base, s) {
                    Ok(safe) => !safe.exists(),
                    Err(_) => true, // path escapes jail → treat as missing → use fallback
                }
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
            let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
            if options._include_depth >= max_depth {
                map.insert(
                    key.to_string(),
                    Value::String(format!("WATCH_ERR: max include depth ({}) exceeded", max_depth)),
                );
                return;
            }
            let base = options.base_path.as_deref().unwrap_or(".");
            let full = match jail_path(base, file_path) {
                Ok(p) => p,
                Err(e) => {
                    map.insert(key.to_string(), Value::String(format!("WATCH_ERR: {}", e)));
                    return;
                }
            };
            if let Err(e) = check_file_size(&full) {
                map.insert(key.to_string(), Value::String(format!("WATCH_ERR: {}", e)));
                return;
            }
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

    // ── :prompt ──
    // Syntax: key:prompt:LABEL subtree
    // Converts the resolved subtree (object) into a SYNX-formatted string
    // wrapped in a labeled code fence, ready for LLM prompt embedding.
    if markers.contains(&"prompt".to_string()) {
        let prompt_idx = markers.iter().position(|m| m == "prompt").unwrap();
        let label = markers.get(prompt_idx + 1).cloned().unwrap_or_else(|| key.to_string());
        if let Some(val) = map.get(key) {
            let synx_text = stringify_value(val, 0);
            let block = format!("{} (SYNX):\n```synx\n{}```", label, synx_text);
            map.insert(key.to_string(), Value::String(block));
        }
    }

    // ── :vision ──
    // Metadata-only marker. Recognized by the engine (no error), value passes through.
    // Applications detect this marker via metadata to dispatch image generation.

    // ── :audio ──
    // Metadata-only marker. Recognized by the engine (no error), value passes through.
    // Applications detect this marker via metadata to dispatch audio generation.

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
    // Guardrail: user-controlled width can be enormous (esp. under fuzzing).
    // Large widths can cause pathological allocations or panics in formatting internals.
    const MAX_FMT_WIDTH: usize = 4096;
    if let Some(s) = pattern.strip_prefix('%') {
        if let Some(inner) = s.strip_suffix('d').or_else(|| s.strip_suffix('i')) {
            if let Some(w) = inner.strip_prefix('0') {
                if let Ok(width) = w.parse::<usize>() {
                    let width = width.min(MAX_FMT_WIDTH);
                    return format!("{:0>width$}", n, width = width);
                }
            }
            if let Ok(width) = inner.parse::<usize>() {
                let width = width.min(MAX_FMT_WIDTH);
                return format!("{:>width$}", n, width = width);
            }
        }
    }
    n.to_string()
}

fn format_float_pattern(pattern: &str, f: f64) -> String {
    // Same rationale as MAX_FMT_WIDTH: avoid pathological precision values.
    const MAX_FMT_PREC: usize = 1024;
    if let Some(s) = pattern.strip_prefix('%') {
        if let Some(inner) = s.strip_suffix('f').or_else(|| s.strip_suffix('e')) {
            if let Some(prec_s) = inner.strip_prefix('.') {
                if let Ok(prec) = prec_s.parse::<usize>() {
                    let prec = prec.min(MAX_FMT_PREC);
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

fn allow_spam_access(bucket_key: &str, max_calls: usize, window_sec: u64) -> bool {
    let now = Instant::now();
    let window = Duration::from_secs(window_sec.max(1));

    let buckets = SPAM_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = match buckets.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    let calls = guard.entry(bucket_key.to_string()).or_default();
    calls.retain(|ts| now.duration_since(*ts) <= window);

    if calls.len() >= max_calls {
        return false;
    }

    calls.push(now);
    true
}

#[cfg(test)]
fn clear_spam_buckets() {
    let buckets = SPAM_BUCKETS.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut guard) = buckets.lock() {
        guard.clear();
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

/// Serialize a Value to SYNX format string (for :prompt marker).
fn stringify_value(value: &Value, indent: usize) -> String {
    let spaces = " ".repeat(indent);
    match value {
        Value::Object(map) => {
            let mut out = String::new();
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            for key in keys {
                let val = &map[key];
                match val {
                    Value::Object(_) => {
                        out.push_str(&format!("{}{}\n", spaces, key));
                        out.push_str(&stringify_value(val, indent + 2));
                    }
                    Value::Array(arr) => {
                        out.push_str(&format!("{}{}\n", spaces, key));
                        for item in arr {
                            out.push_str(&format!("{}  - {}\n", spaces, value_to_string(item)));
                        }
                    }
                    _ => {
                        out.push_str(&format!("{}{} {}\n", spaces, key, value_to_string(val)));
                    }
                }
            }
            out
        }
        _ => format!("{}{}\n", spaces, value_to_string(value)),
    }
}

fn cast_primitive(val: &str) -> Value {
    // Quoted strings preserve literal value
    if val.len() >= 2 {
        let bytes = val.as_bytes();
        if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
        {
            return Value::String(val[1..val.len() - 1].to_string());
        }
    }
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

    let mut result = String::with_capacity(hay_len.min(MAX_ENGINE_SCRATCH_STRING));
    let mut i = 0;

    while i <= hay_len - word_len {
        if result.len() >= MAX_ENGINE_SCRATCH_STRING {
            break;
        }
        if &hay_bytes[i..i + word_len] == word_bytes {
            let before_ok = i == 0 || !is_word_char(hay_bytes[i - 1]);
            let after_ok = i + word_len >= hay_len || !is_word_char(hay_bytes[i + word_len]);
            if before_ok && after_ok {
                let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
                if room > 0 {
                    let take = replacement.len().min(room);
                    let end = replacement.floor_char_boundary(take);
                    result.push_str(&replacement[..end]);
                }
                i += word_len;
                continue;
            }
        }
        if result.len() < MAX_ENGINE_SCRATCH_STRING {
            result.push(hay_bytes[i] as char);
        }
        i += 1;
    }
    while i < hay_len && result.len() < MAX_ENGINE_SCRATCH_STRING {
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

    // Collect inherit targets: child_key → [parent1, parent2, ...]
    let mut inherits: Vec<(String, Vec<String>)> = Vec::new();
    for (key, meta) in &root_meta {
        if meta.markers.contains(&"inherit".to_string()) {
            let idx = meta.markers.iter().position(|m| m == "inherit").unwrap();
            // All markers after "inherit" are parent names (multi-parent support)
            let parents: Vec<String> = meta.markers[idx + 1..].to_vec();
            if !parents.is_empty() {
                inherits.push((key.clone(), parents));
            }
        }
    }

    let map = unsafe { &mut *root_map };
    for (child_key, parents) in &inherits {
        // Merge parents left-to-right: first parent is base, each subsequent overrides
        let mut merged: HashMap<String, Value> = HashMap::new();
        for parent_name in parents {
            if let Some(Value::Object(p)) = map.get(parent_name) {
                for (k, v) in p {
                    merged.insert(k.clone(), v.clone());
                }
            }
        }
        // Child fields override all parents
        if let Some(Value::Object(c)) = map.get(child_key) {
            for (k, v) in c {
                merged.insert(k.clone(), v.clone());
            }
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
/// Supports: {key}, {key.nested}, {key:alias}, {key:include}
fn resolve_interpolation(
    tpl: &str,
    root: &Value,
    local_map: &HashMap<String, Value>,
    includes: &HashMap<String, Value>,
) -> String {
    let bytes = tpl.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len.min(MAX_ENGINE_SCRATCH_STRING));
    let mut i = 0;

    while i < len {
        if result.len() >= MAX_ENGINE_SCRATCH_STRING {
            break;
        }
        if bytes[i] == b'{' {
            if let Some(close) = tpl[i + 1..].find('}') {
                let inner = &tpl[i + 1..i + 1 + close];
                // Check for scope separator ':'
                if let Some(colon) = inner.find(':') {
                    let ref_name = &inner[..colon];
                    let scope = &inner[colon + 1..];
                    // Valid ref name?
                    if ref_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        let resolved = if scope == "include" {
                            // {key:include} — first/only include
                            if includes.len() == 1 {
                                let first = includes.values().next().unwrap();
                                deep_get(first, ref_name)
                            } else {
                                None
                            }
                        } else {
                            // {key:alias} — look up by alias
                            includes.get(scope).and_then(|inc| deep_get(inc, ref_name))
                        };
                        if let Some(val) = resolved {
                            let s = value_to_string(&val);
                            let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
                            if room > 0 {
                                let take = s.len().min(room);
                                let end = s.floor_char_boundary(take);
                                result.push_str(&s[..end]);
                            }
                        } else {
                            result.push('{');
                            let rem = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len() + 1);
                            if rem > 0 {
                                let end = inner.floor_char_boundary(inner.len().min(rem));
                                result.push_str(&inner[..end]);
                            }
                            if result.len() < MAX_ENGINE_SCRATCH_STRING {
                                result.push('}');
                            }
                        }
                        i += 2 + close;
                        continue;
                    }
                } else {
                    // {key} — local
                    let ref_name = inner;
                    if ref_name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.') {
                        let resolved = deep_get(root, ref_name).or_else(|| {
                            local_map.get(ref_name).cloned()
                        });
                        if let Some(val) = resolved {
                            let s = value_to_string(&val);
                            let room = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len());
                            if room > 0 {
                                let take = s.len().min(room);
                                let end = s.floor_char_boundary(take);
                                result.push_str(&s[..end]);
                            }
                        } else {
                            result.push('{');
                            let rem = MAX_ENGINE_SCRATCH_STRING.saturating_sub(result.len() + 1);
                            if rem > 0 {
                                let end = ref_name.floor_char_boundary(ref_name.len().min(rem));
                                result.push_str(&ref_name[..end]);
                            }
                            if result.len() < MAX_ENGINE_SCRATCH_STRING {
                                result.push('}');
                            }
                        }
                        i += 2 + close;
                        continue;
                    }
                }
            }
        }
        if result.len() < MAX_ENGINE_SCRATCH_STRING {
            result.push(bytes[i] as char);
        }
        i += 1;
    }
    result
}

/// Load !include files into a map<alias, Value>.
fn load_includes(
    directives: &[IncludeDirective],
    options: &Options,
) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    let base = options.base_path.as_deref().unwrap_or(".");
    let max_depth = options.max_include_depth.unwrap_or(DEFAULT_MAX_INCLUDE_DEPTH);
    if options._include_depth >= max_depth {
        return map;
    }
    for inc in directives {
        let full = match jail_path(base, &inc.path) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if check_file_size(&full).is_err() {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&full) {
            let mut included = parser::parse(&text);
            if included.mode == Mode::Active {
                let mut child_opts = options.clone();
                child_opts._include_depth += 1;
                if let Some(parent) = full.parent() {
                    child_opts.base_path = Some(parent.to_string_lossy().into_owned());
                }
                resolve(&mut included, &child_opts);
            }
            map.insert(inc.alias.clone(), included.root);
        }
    }
    map
}

// ─── Type validation ──────────────────────────────────────

/// Build a global type registry from all metadata.
/// Maps field name → expected type (e.g., "hp" → "int").
fn build_type_registry(metadata: &HashMap<String, MetaMap>) -> HashMap<String, String> {
    let mut registry: HashMap<String, String> = HashMap::new();

    for meta_map in metadata.values() {
        for (key, meta) in meta_map {
            if let Some(ref type_hint) = meta.type_hint {
                // If type already registered, check for conflict
                if let Some(existing) = registry.get(key) {
                    if existing != type_hint {
                        // Type conflict: same field defined with different types
                        // For now, first definition wins; could also log error
                    }
                } else {
                    registry.insert(key.clone(), type_hint.clone());
                }
            }
        }
    }

    registry
}

/// Build a global constraint registry from all metadata.
/// Maps field name → merged constraints from [] declarations.
fn build_constraint_registry(metadata: &HashMap<String, MetaMap>) -> HashMap<String, Constraints> {
    let mut registry: HashMap<String, Constraints> = HashMap::new();

    for meta_map in metadata.values() {
        for (key, meta) in meta_map {
            if let Some(ref constraints) = meta.constraints {
                registry
                    .entry(key.clone())
                    .and_modify(|existing| merge_constraints(existing, constraints))
                    .or_insert_with(|| constraints.clone());
            }
        }
    }

    registry
}

/// Merge constraints when the same field is declared multiple times.
/// Strategy is intentionally strict to keep schemas consistent across templates.
fn merge_constraints(base: &mut Constraints, incoming: &Constraints) {
    if incoming.required {
        base.required = true;
    }
    if incoming.readonly {
        base.readonly = true;
    }

    // Stricter numeric bounds win.
    base.min = match (base.min, incoming.min) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (None, Some(b)) => Some(b),
        (a, None) => a,
    };
    base.max = match (base.max, incoming.max) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (None, Some(b)) => Some(b),
        (a, None) => a,
    };

    // Keep first non-empty type/pattern/enum declaration.
    if base.type_name.is_none() {
        base.type_name = incoming.type_name.clone();
    }
    if base.pattern.is_none() {
        base.pattern = incoming.pattern.clone();
    }
    if base.enum_values.is_none() {
        base.enum_values = incoming.enum_values.clone();
    }
}

/// Validate [] constraints recursively for all object fields that have
/// a registered constraint rule.
fn validate_field_constraints(value: &mut Value, registry: &HashMap<String, Constraints>) {
    if let Value::Object(ref mut map) = value {
        let keys: Vec<String> = map.keys().cloned().collect();
        for key in &keys {
            if let Some(constraints) = registry.get(key) {
                validate_constraints(map, key, constraints);
            }

            if let Some(child) = map.get_mut(key) {
                match child {
                    Value::Object(_) => validate_field_constraints(child, registry),
                    Value::Array(arr) => {
                        for item in arr.iter_mut() {
                            if let Value::Object(_) = item {
                                validate_field_constraints(item, registry);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Validate that all values in the tree match their registered types.
fn validate_field_types(value: &mut Value, registry: &HashMap<String, String>, path: &str) {
    match value {
        Value::Object(ref mut map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in &keys {
                if let Some(expected_type) = registry.get(key) {
                    if let Some(val) = map.get(key) {
                        if !value_matches_type(val, expected_type) {
                            // Type mismatch: replace with error string
                            let current_type = value_type_name(val);
                            map.insert(key.clone(), Value::String(
                                format!("TYPE_ERR: '{}' expected {} but got {}", key, expected_type, current_type)
                            ));
                        }
                    }
                }
                
                // Recurse into nested objects and arrays
                if let Some(child) = map.get_mut(key) {
                    match child {
                        Value::Object(_) => {
                            let child_path = if path.is_empty() {
                                key.clone()
                            } else {
                                format!("{}.{}", path, key)
                            };
                            validate_field_types(child, registry, &child_path);
                        }
                        Value::Array(ref mut arr) => {
                            for item in arr.iter_mut() {
                                if let Value::Object(_) = item {
                                    validate_field_types(item, registry, path);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
}

/// Check if a value matches an expected type.
fn value_matches_type(value: &Value, expected_type: &str) -> bool {
    match expected_type {
        "int" => matches!(value, Value::Int(_)),
        "float" => matches!(value, Value::Float(_) | Value::Int(_)),
        "bool" => matches!(value, Value::Bool(_)),
        "string" => matches!(value, Value::String(_) | Value::Secret(_)),
        "array" => matches!(value, Value::Array(_)),
        "object" => matches!(value, Value::Object(_)),
        _ => true, // Unknown types are accepted
    }
}

/// Get the human-readable name of a value's type.
fn value_type_name(value: &Value) -> String {
    match value {
        Value::Int(_) => "int".to_string(),
        Value::Float(_) => "float".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Secret(_) => "secret".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Object(_) => "object".to_string(),
        Value::Null => "null".to_string(),
    }
}

// ─── CLDR plural rules ───────────────────────────────────

/// Return the CLDR plural category for a given language and integer count.
/// Categories: "zero", "one", "two", "few", "many", "other".
fn plural_category(lang: &str, n: i64) -> &'static str {
    let abs_n = n.unsigned_abs();
    let n10 = abs_n % 10;
    let n100 = abs_n % 100;

    match lang {
        // East Slavic: Russian, Ukrainian, Belarusian
        "ru" | "uk" | "be" => {
            if n10 == 1 && n100 != 11 {
                "one"
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                "few"
            } else {
                "many"
            }
        }
        // West/South Slavic: Polish
        "pl" => {
            if n10 == 1 && n100 != 11 {
                "one"
            } else if (2..=4).contains(&n10) && !(12..=14).contains(&n100) {
                "few"
            } else {
                "many"
            }
        }
        // Czech, Slovak
        "cs" | "sk" => {
            if abs_n == 1 { "one" }
            else if (2..=4).contains(&abs_n) { "few" }
            else { "other" }
        }
        // Arabic
        "ar" => {
            if abs_n == 0 { "zero" }
            else if abs_n == 1 { "one" }
            else if abs_n == 2 { "two" }
            else if (3..=10).contains(&n100) { "few" }
            else if (11..=99).contains(&n100) { "many" }
            else { "other" }
        }
        // French, Portuguese (Brazilian) — 0 and 1 are "one"
        "fr" | "pt" => {
            if abs_n <= 1 { "one" } else { "other" }
        }
        // Japanese, Chinese, Korean, Vietnamese, Thai — no plural forms
        "ja" | "zh" | "ko" | "vi" | "th" => "other",
        // English, German, Spanish, Italian, Dutch, Swedish, Norwegian, Danish, etc.
        _ => {
            if abs_n == 1 { "one" } else { "other" }
        }
    }
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

    #[test]
    fn test_auto_interpolation_simple() {
        let mut r = parse("!active\nname Wario\ngreeting Hello, {name}!");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["greeting"], Value::String("Hello, Wario!".into()));
    }

    #[test]
    fn test_auto_interpolation_nested() {
        let mut r = parse("!active\nserver\n  host localhost\n  port 8080\nurl http://{server.host}:{server.port}/api");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["url"], Value::String("http://localhost:8080/api".into()));
    }

    #[test]
    fn test_template_legacy_still_works() {
        let mut r = parse("!active\nname Wario\ngreeting:template Hello, {name}!");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["greeting"], Value::String("Hello, Wario!".into()));
    }

    #[test]
    fn test_type_validation() {
        // Test that type validation works: hp(int) defined in _base_unit,
        // then used in other places with correct type
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
              hp(int) 100\n  \
              speed(float) 1.5\n\
            infantry:inherit:_base_unit\n  \
              name Infantry\n  \
              hp 80"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        
        // _base_unit should be removed (private)
        assert!(!map.contains_key("_base_unit"));
        
        // infantry should exist with correct types
        let infantry = map["infantry"].as_object().unwrap();
        assert_eq!(infantry["hp"], Value::Int(80));  // Correct: int
        assert_eq!(infantry["speed"], Value::Float(1.5));  // Correct: float
    }

    #[test]
    fn test_type_validation_error() {
        // Test that type mismatch is detected and replaced with error
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
              hp(int) 100\n\
            infantry:inherit:_base_unit\n  \
              hp hello"  // Type mismatch: string instead of int
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        
        let infantry = map["infantry"].as_object().unwrap();
        // Should be replaced with error message
        if let Value::String(s) = &infantry["hp"] {
            assert!(s.contains("TYPE_ERR"));
        } else {
            panic!("Expected error string for type mismatch");
        }
    }

    #[test]
    fn test_constraint_validation_inherited_range() {
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
              hp[min:1, max:50000] 1000\n\
            infantry:inherit:_base_unit\n  \
              hp 60000"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        let infantry = map["infantry"].as_object().unwrap();

        if let Value::String(s) = &infantry["hp"] {
            assert!(s.contains("CONSTRAINT_ERR"));
            assert!(s.contains("exceeds max"));
        } else {
            panic!("Expected constraint error string");
        }
    }

    #[test]
    fn test_constraint_validation_required() {
        let mut r = parse(
            "!active\n\
            _base_unit\n  \
                            description[type:string, required] hello\n\
            scout:inherit:_base_unit\n  \
                            description null"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        let scout = map["scout"].as_object().unwrap();

        if let Value::String(s) = &scout["description"] {
            assert!(s.contains("CONSTRAINT_ERR"));
            assert!(s.contains("required"));
        } else {
            panic!("Expected required-constraint error string");
        }
    }

    #[test]
    fn test_multi_parent_inherit() {
        let mut r = parse(
            "!active\n\
            _movable\n  \
              speed 10\n  \
              can_move true\n\
            _damageable\n  \
              hp 100\n  \
              armor 5\n\
            tank:inherit:_movable:_damageable\n  \
              name Tank\n  \
              armor 20"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();

        assert!(!map.contains_key("_movable"));
        assert!(!map.contains_key("_damageable"));

        let tank = map["tank"].as_object().unwrap();
        assert_eq!(tank["speed"], Value::Int(10));        // from _movable
        assert_eq!(tank["can_move"], Value::Bool(true));   // from _movable
        assert_eq!(tank["hp"], Value::Int(100));           // from _damageable
        assert_eq!(tank["armor"], Value::Int(20));         // child overrides _damageable's 5
        assert_eq!(tank["name"], Value::String("Tank".into()));
    }

    #[test]
    fn test_calc_dot_path() {
        let mut r = parse(
            "!active\n\
            stats\n  \
              base_hp 100\n  \
              multiplier 3\n\
            total_hp:calc stats.base_hp * stats.multiplier"
        );
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["total_hp"], Value::Int(300));
    }

    #[test]
    fn test_i18n_plural_en() {
        let mut r = parse(
            "!active\n\
            count 5\n\
            items:i18n:count\n  \
              en\n    \
                one item\n    \
                other items"
        );
        let opts = Options { lang: Some("en".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["items"], Value::String("items".into()));
    }

    #[test]
    fn test_i18n_plural_en_one() {
        let mut r = parse(
            "!active\n\
            count 1\n\
            items:i18n:count\n  \
              en\n    \
                one {count} item\n    \
                other {count} items"
        );
        let opts = Options { lang: Some("en".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["items"], Value::String("1 item".into()));
    }

    #[test]
    fn test_i18n_plural_ru() {
        let mut r = parse(
            "!active\n\
            count 3\n\
            items:i18n:count\n  \
              ru\n    \
                one предмет\n    \
                few предмета\n    \
                many предметов\n    \
                other предметов"
        );
        let opts = Options { lang: Some("ru".into()), ..Default::default() };
        resolve(&mut r, &opts);
        let map = r.root.as_object().unwrap();
        assert_eq!(map["items"], Value::String("предмета".into()));
    }

    #[test]
    fn test_quoted_null_preserved() {
        let r = parse("status \"null\"\nenabled \"true\"\ncount \"42\"");
        let map = r.root.as_object().unwrap();
        assert_eq!(map["status"], Value::String("null".into()));
        assert_eq!(map["enabled"], Value::String("true".into()));
        assert_eq!(map["count"], Value::String("42".into()));
    }

    #[test]
    fn test_unquoted_null_is_null() {
        let r = parse("status null\nenabled true\ncount 42");
        let map = r.root.as_object().unwrap();
        assert_eq!(map["status"], Value::Null);
        assert_eq!(map["enabled"], Value::Bool(true));
        assert_eq!(map["count"], Value::Int(42));
    }

    #[test]
    fn test_spam_rate_limit_exceeded() {
        super::clear_spam_buckets();

        let mut r1 = parse("!active\nsecret_token abc\naccess:spam:1:5 secret_token");
        resolve(&mut r1, &Options::default());
        let map1 = r1.root.as_object().unwrap();
        assert_eq!(map1["access"], Value::String("abc".into()));

        let mut r2 = parse("!active\nsecret_token abc\naccess:spam:1:5 secret_token");
        resolve(&mut r2, &Options::default());
        let map2 = r2.root.as_object().unwrap();
        match &map2["access"] {
            Value::String(s) => assert!(s.starts_with("SPAM_ERR:")),
            _ => panic!("Expected SPAM_ERR string"),
        }
    }

    #[test]
    fn test_spam_default_window_sec_is_one() {
        super::clear_spam_buckets();

        let mut r = parse("!active\na 1\nx:spam:2 a");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["x"], Value::Int(1));
    }

    #[test]
    fn test_deep_nesting_does_not_overflow() {
        // Deep indentation chain: parser caps nesting (see `MAX_PARSE_NESTING_DEPTH` in parser);
        // this test only checks resolve + navigation do not panic and yield a bounded tree.
        let mut synx = String::from("!active\n");
        let mut indent = String::new();
        for i in 0..200 {
            synx.push_str(&format!("{}level_{}\n", indent, i));
            indent.push_str("  ");
        }
        synx.push_str(&format!("{}value deep\n", indent));

        let mut result = parse(&synx);
        resolve(&mut result, &Default::default());
        assert!(matches!(result.root, Value::Object(_)));

        let mut cur = &result.root;
        let mut depth = 0usize;
        loop {
            let Value::Object(map) = cur else { break };
            let key = format!("level_{}", depth);
            match map.get(&key) {
                Some(next) => {
                    cur = next;
                    depth += 1;
                }
                None => break,
            }
        }
        assert!(
            depth >= 100,
            "expected at least 100 chained levels from parse, got {}",
            depth
        );
        assert!(
            depth <= 130,
            "parser nesting cap should keep chain shallow, got {}",
            depth
        );
    }

    #[test]
    fn test_circular_alias_returns_error() {
        let mut r = parse("!active\na:alias b\nb:alias a");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        let a_val = root.get("a").unwrap();
        let b_val = root.get("b").unwrap();
        assert!(
            matches!(a_val, Value::String(s) if s.starts_with("ALIAS_ERR:")),
            "expected ALIAS_ERR for 'a', got: {:?}", a_val
        );
        assert!(
            matches!(b_val, Value::String(s) if s.starts_with("ALIAS_ERR:")),
            "expected ALIAS_ERR for 'b', got: {:?}", b_val
        );
    }

    #[test]
    fn test_self_alias_returns_error() {
        let mut r = parse("!active\na:alias a");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        let a_val = root.get("a").unwrap();
        assert!(
            matches!(a_val, Value::String(s) if s.starts_with("ALIAS_ERR:")),
            "expected ALIAS_ERR for self-alias, got: {:?}", a_val
        );
    }

    #[test]
    fn test_valid_alias_still_works() {
        let mut r = parse("!active\nbase 42\ncopy:alias base");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        assert_eq!(root.get("copy"), Some(&Value::Int(42)));
    }

    #[test]
    fn test_alias_to_string_valued_key_no_false_positive() {
        // 'a' holds a literal string "b". 'b' aliases 'a'.
        // b should resolve to "b" — NOT trigger ALIAS_ERR.
        let mut r = parse("!active\na b\nb:alias a");
        resolve(&mut r, &Default::default());
        let root = r.root.as_object().unwrap();
        assert_eq!(
            root.get("b"),
            Some(&Value::String("b".to_string())),
            "alias to a string-valued key should not produce ALIAS_ERR"
        );
    }

    #[test]
    fn test_prompt_marker() {
        let mut r = parse("!active\nmemory:prompt:Core\n  identity ASAI\n  creator APERTURESyndicate");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        if let Value::String(s) = &map["memory"] {
            assert!(s.starts_with("Core (SYNX):"));
            assert!(s.contains("```synx"));
            assert!(s.contains("identity ASAI"));
        } else {
            panic!("Expected :prompt to produce a string");
        }
    }

    #[test]
    fn test_vision_marker_passthrough() {
        let mut r = parse("!active\nimage:vision Generate a sunset");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["image"], Value::String("Generate a sunset".into()));
    }

    #[test]
    fn test_audio_marker_passthrough() {
        let mut r = parse("!active\nnarration:audio Read this summary aloud");
        resolve(&mut r, &Options::default());
        let map = r.root.as_object().unwrap();
        assert_eq!(map["narration"], Value::String("Read this summary aloud".into()));
    }
}
