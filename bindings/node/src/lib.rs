//! SYNX Node.js binding — exposes parse/parseActive/stringify/format to JS via napi-rs.

use napi::bindgen_prelude::*;
use napi::JsUnknown;
use napi_derive::napi;
use synx_core::{self, Value, Options, Mode};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Deepest JS value `stringify` and `diff` follow — the nesting cap of the
/// parser. A value that contains itself reaches it too.
const MAX_JS_VALUE_DEPTH: usize = 128;

/// Longest JS array taken in — the parser's cap on the items of one list.
const MAX_JS_ARRAY_LEN: u32 = 1_048_576;

/// Largest integer a JS number holds exactly (2^53 - 1).
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// A key as a JS string.
///
/// safety: `set_named_property` passes the key as a C string, so a key with
/// U+0000 in it failed the whole document (`nul byte found in provided data`).
fn js_key(env: &Env, key: &str) -> Result<napi::JsString> {
    env.create_string(key)
}

/// Convert a synx_core::Value into a napi JsUnknown for zero-copy return to JS.
fn value_to_js(env: &Env, val: &Value) -> Result<JsUnknown> {
    match val {
        Value::Null => env.get_null().map(|v| v.into_unknown()),
        Value::Bool(b) => env.get_boolean(*b).map(|v| v.into_unknown()),
        // spec: an integer beyond 2^53 is a `bigint`, as the pure TypeScript
        // engine gives it. A double changed 9007199254740993 into ...992.
        Value::Int(n) if *n > MAX_SAFE_INTEGER || *n < -MAX_SAFE_INTEGER => {
            env.create_bigint_from_i64(*n)?.into_unknown()
        }
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
                obj.set_property(js_key(env, key)?, js_val)?;
            }
            Ok(obj.into_unknown())
        }
    }
}

/// Convert a JS value back to synx_core::Value.
///
/// safety: the value comes from the caller and may be anything. It was walked
/// without a depth limit (24 KB of nested arrays overflowed the native stack,
/// a value that contains itself aborted the process) and an array reserved
/// its declared length (`length = 2^32 - 1` asked for 256 GiB and aborted).
fn js_to_value(env: &Env, val: JsUnknown, depth: usize) -> Result<Value> {
    match val.get_type()? {
        napi::ValueType::Null | napi::ValueType::Undefined => Ok(Value::Null),
        napi::ValueType::Boolean => {
            let b: napi::JsBoolean = val.try_into()?;
            Ok(Value::Bool(b.get_value()?))
        }
        napi::ValueType::Number => {
            let n: napi::JsNumber = val.try_into()?;
            let f = n.get_double()?;
            // spec: as the pure TypeScript writer — a whole number is an int
            // only within 2^53; beyond it the double is all there is, and 2^63
            // was cast to 2^63 - 1.
            if f.fract() == 0.0 && f.abs() <= MAX_SAFE_INTEGER as f64 {
                Ok(Value::Int(f as i64))
            } else {
                Ok(Value::Float(f))
            }
        }
        napi::ValueType::BigInt => {
            // spec: a `bigint` is the integer it holds; beyond i64 its digits
            // are text, as SYNX reads such a literal back. It was `null`.
            // why: napi 2 has no checked conversion from `JsUnknown` to
            // `JsBigInt`; the decimal text says the same and needs no `unsafe`.
            let digits = val.coerce_to_string()?.into_utf8()?;
            let digits = digits.as_str()?;
            match digits.parse::<i64>() {
                Ok(n) => Ok(Value::Int(n)),
                Err(_) => Ok(Value::String(digits.to_string())),
            }
        }
        napi::ValueType::String => {
            let s: napi::JsString = val.try_into()?;
            Ok(Value::String(js_string_text(s)?))
        }
        napi::ValueType::Object => {
            if depth >= MAX_JS_VALUE_DEPTH {
                return Err(napi::Error::from_reason(format!(
                    "SYNX: value nested deeper than {} levels, or it contains itself",
                    MAX_JS_VALUE_DEPTH
                )));
            }
            let obj: napi::JsObject = val.try_into()?;
            if obj.is_array()? {
                let len = obj.get_array_length()?;
                if len > MAX_JS_ARRAY_LEN {
                    return Err(napi::Error::from_reason(format!(
                        "SYNX: array of {} items, the limit is {}",
                        len, MAX_JS_ARRAY_LEN
                    )));
                }
                let mut arr = Vec::with_capacity(len as usize);
                for i in 0..len {
                    let item: JsUnknown = obj.get_element(i)?;
                    arr.push(js_to_value(env, item, depth + 1)?);
                }
                Ok(Value::Array(arr))
            } else {
                // The value is fetched by the key as JS has it: a key with a
                // lone surrogate has no UTF-8 spelling, and fetched by its
                // replaced name it came back `null`.
                let names = obj.get_property_names()?;
                let count = names.get_array_length()?;
                let mut map = synx_core::Map::default();
                for i in 0..count {
                    let key: napi::JsString = names.get_element(i)?;
                    let v: JsUnknown = obj.get_property(key)?;
                    map.insert(js_string_text(key)?, js_to_value(env, v, depth + 1)?);
                }
                Ok(Value::Object(map))
            }
        }
        _ => Ok(Value::Null),
    }
}

/// The text of a JS string; a lone surrogate becomes U+FFFD.
fn js_string_text(s: napi::JsString) -> Result<String> {
    // perf: UTF-8 straight from the engine, which writes U+FFFD for a lone
    // surrogate itself. The UTF-16 road is for a runtime that does not.
    if let Ok(utf8) = s.into_utf8() {
        if let Ok(text) = utf8.as_str() {
            return Ok(text.to_owned());
        }
    }
    let len = s.utf16_len()?;
    let utf16 = s.into_utf16()?;
    let units = utf16.as_slice();
    // napi counts the NUL that ends its buffer as part of the slice: taken
    // whole, every key and every string came back one U+0000 longer.
    Ok(String::from_utf16_lossy(&units[..len.min(units.len())]))
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
    let val = js_to_value(&env, obj, 0)?;
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
    let val_a = js_to_value(&env, a, 0)?;
    let val_b = js_to_value(&env, b, 0)?;
    let map_a = match val_a { Value::Object(m) => m, _ => synx_core::Map::default() };
    let map_b = match val_b { Value::Object(m) => m, _ => synx_core::Map::default() };
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
