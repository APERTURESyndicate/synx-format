//! SYNX Node.js binding — exposes parse/parseActive/stringify/format to JS via napi-rs.

use napi::bindgen_prelude::*;
use napi::JsUnknown;
use napi_derive::napi;
use synx_core::{self, Value, Options, Mode};

/// Convert a synx_core::Value into a napi JsUnknown for zero-copy return to JS.
fn value_to_js(env: &Env, val: &Value) -> Result<JsUnknown> {
    match val {
        Value::Null => env.get_null().map(|v| v.into_unknown()),
        Value::Bool(b) => env.get_boolean(*b).map(|v| v.into_unknown()),
        Value::Int(n) => env.create_int64(*n).map(|v| v.into_unknown()),
        Value::Float(f) => env.create_double(*f).map(|v| v.into_unknown()),
        Value::String(s) => env.create_string(s).map(|v| v.into_unknown()),
        Value::Secret(_) => env.create_string("[SECRET]").map(|v| v.into_unknown()),
        Value::Array(arr) => {
            let mut js_arr = env.create_array_with_length(arr.len())?;
            for (i, item) in arr.iter().enumerate() {
                let js_val = value_to_js(env, item)?;
                js_arr.set_element(i as u32, js_val)?;
            }
            Ok(js_arr.into_unknown())
        }
        Value::Object(map) => {
            let mut obj = env.create_object()?;
            for (key, val) in map {
                let js_val = value_to_js(env, val)?;
                obj.set_named_property(key, js_val)?;
            }
            Ok(obj.into_unknown())
        }
    }
}

/// Convert a JS value back to synx_core::Value.
fn js_to_value(env: &Env, val: JsUnknown) -> Result<Value> {
    match val.get_type()? {
        napi::ValueType::Null | napi::ValueType::Undefined => Ok(Value::Null),
        napi::ValueType::Boolean => {
            let b: napi::JsBoolean = val.try_into()?;
            Ok(Value::Bool(b.get_value()?))
        }
        napi::ValueType::Number => {
            let n: napi::JsNumber = val.try_into()?;
            let f = n.get_double()?;
            if f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
                Ok(Value::Int(f as i64))
            } else {
                Ok(Value::Float(f))
            }
        }
        napi::ValueType::String => {
            let s: napi::JsString = val.try_into()?;
            Ok(Value::String(s.into_utf8()?.as_str()?.to_string()))
        }
        napi::ValueType::Object => {
            // Check if it's an array
            let obj: napi::JsObject = val.try_into()?;
            if obj.is_array()? {
                let len = obj.get_array_length()?;
                let mut arr = Vec::with_capacity(len as usize);
                for i in 0..len {
                    let item: JsUnknown = obj.get_element(i)?;
                    arr.push(js_to_value(env, item)?);
                }
                Ok(Value::Array(arr))
            } else {
                let keys = napi::JsObject::keys(&obj)?;
                let mut map = std::collections::HashMap::new();
                for key in keys {
                    let v: JsUnknown = obj.get_named_property(&key)?;
                    map.insert(key, js_to_value(env, v)?);
                }
                Ok(Value::Object(map))
            }
        }
        _ => Ok(Value::Null),
    }
}

/// Parse a SYNX string. Returns a JS object.
#[napi]
pub fn parse(env: Env, text: String) -> Result<JsUnknown> {
    let result = synx_core::parse(&text);
    value_to_js(&env, &result.root)
}

/// Parse a SYNX string as JSON. Returns a JSON string (faster for large files).
#[napi]
pub fn parse_to_json(text: String) -> String {
    let result = synx_core::parse(&text);
    synx_core::to_json(&result.root)
}

/// Parse a SYNX string with active mode engine resolution.
/// Optionally accepts an options object with `env` (Record<string, string>).
#[napi]
pub fn parse_active(env: Env, text: String, options: Option<napi::JsObject>) -> Result<JsUnknown> {
    let mut result = synx_core::parse(&text);
    if result.mode == Mode::Active {
        let mut opts = Options::default();
        if let Some(ref js_opts) = options {
            // Read env: Record<string, string>
            if let Ok(env_obj) = js_opts.get_named_property::<napi::JsObject>("env") {
                let mut hm = std::collections::HashMap::new();
                let keys = napi::JsObject::keys(&env_obj)?;
                for key in keys {
                    if let Ok(val) = env_obj.get_named_property::<napi::JsString>(&key) {
                        if let Ok(s) = val.into_utf8() {
                            hm.insert(key, s.as_str()?.to_string());
                        }
                    }
                }
                opts.env = Some(hm);
            }
            // Read basePath: string
            if let Ok(bp) = js_opts.get_named_property::<napi::JsString>("basePath") {
                if let Ok(s) = bp.into_utf8() {
                    opts.base_path = Some(s.as_str()?.to_string());
                }
            }
        }
        synx_core::resolve(&mut result, &opts);
    }
    value_to_js(&env, &result.root)
}

/// Parse a `!tool` SYNX string. Returns `{ tool: "name", params: { ... } }` for calls,
/// or `{ tools: [ { name, params } ] }` for schema definitions.
/// If the text is also `!active`, markers are resolved before reshaping.
#[napi]
pub fn parse_tool(env: Env, text: String, options: Option<napi::JsObject>) -> Result<JsUnknown> {
    let mut result = synx_core::parse(&text);
    if result.mode == Mode::Active {
        let mut opts = Options::default();
        if let Some(ref js_opts) = options {
            if let Ok(env_obj) = js_opts.get_named_property::<napi::JsObject>("env") {
                let mut hm = std::collections::HashMap::new();
                let keys = napi::JsObject::keys(&env_obj)?;
                for key in keys {
                    if let Ok(val) = env_obj.get_named_property::<napi::JsString>(&key) {
                        if let Ok(s) = val.into_utf8() {
                            hm.insert(key, s.as_str()?.to_string());
                        }
                    }
                }
                opts.env = Some(hm);
            }
        }
        synx_core::resolve(&mut result, &opts);
    }
    let shaped = synx_core::reshape_tool_output(&result.root, result.schema);
    value_to_js(&env, &shaped)
}

/// Convert a JS object back to a SYNX string.
#[napi]
pub fn stringify(env: Env, obj: JsUnknown) -> Result<String> {
    let val = js_to_value(&env, obj)?;
    Ok(synx_core::Synx::stringify(&val))
}

/// Reformat a SYNX string into canonical form (sorted keys, normalized indentation).
#[napi]
pub fn format(text: String) -> String {
    synx_core::Synx::format(&text)
}

/// Compile a SYNX string into compact binary .synxb format.
/// If `resolved` is true, metadata/includes are stripped and values are resolved.
#[napi]
pub fn compile(text: String, resolved: Option<bool>) -> Buffer {
    let data = synx_core::Synx::compile(&text, resolved.unwrap_or(false));
    Buffer::from(data)
}

/// Decompile a .synxb binary buffer back into a SYNX string.
#[napi]
pub fn decompile(data: Buffer) -> Result<String> {
    synx_core::Synx::decompile(&data)
        .map_err(|e| napi::Error::from_reason(e))
}

/// Check whether a buffer starts with the .synxb magic bytes.
#[napi]
pub fn is_synxb(data: Buffer) -> bool {
    synx_core::Synx::is_synxb(&data)
}

/// Structural diff between two parsed SYNX objects.
/// Returns `{ added, removed, changed, unchanged }`.
#[napi]
pub fn diff(env: Env, a: JsUnknown, b: JsUnknown) -> Result<JsUnknown> {
    let val_a = js_to_value(&env, a)?;
    let val_b = js_to_value(&env, b)?;
    let map_a = match val_a { Value::Object(m) => m, _ => std::collections::HashMap::new() };
    let map_b = match val_b { Value::Object(m) => m, _ => std::collections::HashMap::new() };
    let result = synx_core::Synx::diff(&map_a, &map_b);
    let val = synx_core::diff_to_value(&result);
    value_to_js(&env, &val)
}

/// Structural diff between two SYNX strings. Returns JSON.
#[napi]
pub fn diff_json(text_a: String, text_b: String) -> String {
    let map_a = synx_core::Synx::parse(&text_a);
    let map_b = synx_core::Synx::parse(&text_b);
    let result = synx_core::Synx::diff(&map_a, &map_b);
    let val = synx_core::diff_to_value(&result);
    synx_core::to_json(&val)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_parse_to_json_and_format() {
        let text = "name John\nage 25\n".to_string();
        let json = parse_to_json(text);
        assert!(json.contains("\"name\":\"John\""));

        let formatted = format("b 2\na 1\n".to_string());
        assert!(formatted.contains("a 1"));
        assert!(formatted.contains("b 2"));
    }
}
