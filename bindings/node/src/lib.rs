//! SYNX Node.js binding — exposes parse/parseActive/stringify to JS via napi-rs.

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
