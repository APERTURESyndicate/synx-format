//! SYNX Python binding — exposes parse/parse_active/stringify/format to Python via PyO3.
//! SYNXL ("SYNX Lines") lives in [`synxl`].

use pyo3::prelude::*;
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::types::{PyDict, PyList};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
use synx_core::{self, Value, Mode, Options};

mod synxl;

fn decode_hex(s: &str) -> PyResult<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "hex string must have even length",
        ));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let byte = u8::from_str_radix(&s[i..i + 2], 16)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("invalid hex digit"))?;
        out.push(byte);
    }
    Ok(out)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}

/// Python key objects already made during one conversion, by key text.
///
/// perf: a dataset repeats the same field names in every record; reusing the
/// `str` object skips decoding and allocating it again (700k times for 100k
/// records of 7 fields). Scoped to one conversion, so it never outlives the
/// objects it hands out.
pub(crate) type KeyCache = std::collections::HashMap<String, Py<pyo3::types::PyString>, synx_core::MapHasher>;

/// [`value_to_py`] sharing key objects through `cache`.
pub(crate) fn value_to_py_cached(py: Python<'_>, val: &Value, cache: &mut KeyCache) -> Py<PyAny> {
    match val {
        Value::Array(arr) => {
            let items: Vec<Py<PyAny>> = arr.iter().map(|v| value_to_py_cached(py, v, cache)).collect();
            PyList::new(py, &items).unwrap().into_any().unbind()
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                let key = match cache.get(k.as_str()) {
                    Some(key) => key.clone_ref(py),
                    None => {
                        let key = pyo3::types::PyString::new(py, k).unbind();
                        cache.insert(k.clone(), key.clone_ref(py));
                        key
                    }
                };
                dict.set_item(key, value_to_py_cached(py, v, cache)).unwrap();
            }
            dict.into_any().unbind()
        }
        _ => value_to_py(py, val),
    }
}

/// Convert synx_core::Value to a Python object.
fn value_to_py(py: Python<'_>, val: &Value) -> Py<PyAny> {
    match val {
        Value::Null => py.None(),
        Value::Bool(b) => b.into_pyobject(py).unwrap().to_owned().into_any().unbind(),
        Value::Int(n) => n.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Float(f) => f.into_pyobject(py).unwrap().into_any().unbind(),
        Value::String(s) => s.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Secret(_) => "[SECRET]".into_pyobject(py).unwrap().into_any().unbind(),
        Value::Array(arr) => {
            let items: Vec<Py<PyAny>> = arr.iter().map(|v| value_to_py(py, v)).collect();
            let list = PyList::new(py, &items).unwrap();
            list.into_pyobject(py).unwrap().into_any().unbind()
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, value_to_py(py, v)).unwrap();
            }
            dict.into_pyobject(py).unwrap().into_any().unbind()
        }
    }
}

/// Nesting depth accepted when converting a Python value.
///
/// Matched to the SYNX parser's own cap, so anything this crate can *parse*
/// can also be converted back. Past it the conversion errors out instead of
/// recursing: `py_to_value` walks the object graph, and Python containers can
/// be arbitrarily deep or outright cyclic (`x["self"] = x`), neither of which
/// the caller can catch once the native stack is gone.
const MAX_PY_VALUE_DEPTH: usize = 128;

/// Convert a Python object back to synx_core::Value.
fn py_to_value(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    py_to_value_at(py, obj, 0)
}

fn py_to_value_at(py: Python<'_>, obj: &Bound<'_, PyAny>, depth: usize) -> PyResult<Value> {
    if depth > MAX_PY_VALUE_DEPTH {
        // A cycle reaches this too: it has no bottom, so it hits the cap. The
        // alternative is a stack overflow, which on Windows kills the process
        // outright — no traceback, nothing for `except` to catch.
        return Err(PyValueError::new_err(format!(
            "value nests deeper than {} levels, or contains a reference cycle",
            MAX_PY_VALUE_DEPTH
        )));
    }
    if obj.is_none() {
        Ok(Value::Null)
    } else if let Ok(b) = obj.extract::<bool>() {
        Ok(Value::Bool(b))
    } else if let Ok(n) = obj.extract::<i64>() {
        Ok(Value::Int(n))
    } else if obj.is_instance_of::<pyo3::types::PyInt>() {
        // A Python int that did not fit i64. Falling through to `f64` would
        // round it — a 20-digit id becomes a different id — and then to the
        // catch-all, which would turn it into a string. Neither is a number
        // the caller wrote, so this is refused instead.
        Err(PyValueError::new_err(
            "integer is outside the i64 range SYNX can represent",
        ))
    } else if let Ok(f) = obj.extract::<f64>() {
        Ok(Value::Float(f))
    } else if let Ok(s) = obj.extract::<String>() {
        Ok(Value::String(s))
    } else if let Ok(list) = obj.cast::<PyList>() {
        let mut arr = Vec::new();
        for item in list.iter() {
            arr.push(py_to_value_at(py, &item, depth + 1)?);
        }
        Ok(Value::Array(arr))
    } else if let Ok(tuple) = obj.cast::<pyo3::types::PyTuple>() {
        // A tuple is a sequence, so it maps to an array. The catch-all below
        // would have stringified it to `(1, 2, 3)`.
        let mut arr = Vec::new();
        for item in tuple.iter() {
            arr.push(py_to_value_at(py, &item, depth + 1)?);
        }
        Ok(Value::Array(arr))
    } else if let Ok(dict) = obj.cast::<PyDict>() {
        let mut map = synx_core::Map::default();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_value_at(py, &v, depth + 1)?);
        }
        Ok(Value::Object(map))
    } else {
        // Anything else has no SYNX representation. `str(obj)` used to stand in
        // for one, which silently turned a `set`, a `bytes` or a custom class
        // into its repr and wrote that to disk as if it were data.
        Err(PyTypeError::new_err(format!(
            "cannot convert {} to a SYNX value; pass a str, int, float, bool, None, list, tuple or dict",
            obj.get_type().name()?
        )))
    }
}

/// Parse a SYNX string and return a Python dict.
#[pyfunction]
fn parse(py: Python<'_>, text: &str) -> PyResult<Py<PyAny>> {
    let result = synx_core::parse(text);
    Ok(value_to_py(py, &result.root))
}

/// Parse a SYNX string as JSON. Returns a JSON string (faster for large files).
#[pyfunction]
fn parse_to_json(text: &str) -> PyResult<String> {
    let result = synx_core::parse(text);
    Ok(synx_core::to_json(&result.root))
}

/// Same as `parse_active` but returns **canonical JSON** (sorted keys) — eases non-Python callers (e.g. Mojo via `Python.import_module`).
#[pyfunction]
#[pyo3(signature = (text, env=None, base_path=None))]
fn parse_active_to_json(
    text: &str,
    env: Option<std::collections::HashMap<String, String>>,
    base_path: Option<String>,
) -> PyResult<String> {
    let mut result = synx_core::parse(text);
    if result.mode == Mode::Active {
        let mut opts = Options::default();
        if let Some(e) = env {
            opts.env = Some(e);
        }
        if let Some(bp) = base_path {
            opts.base_path = Some(bp);
        }
        synx_core::resolve(&mut result, &opts);
    }
    Ok(synx_core::to_json(&result.root))
}

/// Same as `parse_tool` but returns **canonical JSON** string.
#[pyfunction]
#[pyo3(signature = (text, env=None, base_path=None))]
fn parse_tool_to_json(
    text: &str,
    env: Option<std::collections::HashMap<String, String>>,
    base_path: Option<String>,
) -> PyResult<String> {
    let mut result = synx_core::parse(text);
    if result.mode == Mode::Active {
        let mut opts = Options::default();
        if let Some(e) = env {
            opts.env = Some(e);
        }
        if let Some(bp) = base_path {
            opts.base_path = Some(bp);
        }
        synx_core::resolve(&mut result, &opts);
    }
    let shaped = synx_core::reshape_tool_output(&result.root, result.schema);
    Ok(synx_core::to_json(&shaped))
}

/// JSON text → SYNX text (boundary helper for string-only interop).
#[pyfunction]
fn stringify_json(json_text: &str) -> PyResult<String> {
    let val: Value = serde_json::from_str(json_text).map_err(|e: serde_json::Error| {
        pyo3::exceptions::PyValueError::new_err(e.to_string())
    })?;
    Ok(synx_core::Synx::stringify(&val))
}

/// `.synxb` bytes as lowercase hex (no `0x` prefix) for environments without raw bytes (e.g. Mojo ↔ Python string bridge).
#[pyfunction]
#[pyo3(signature = (text, resolved=false))]
fn compile_hex(text: &str, resolved: bool) -> PyResult<String> {
    let v = synx_core::Synx::compile(text, resolved);
    Ok(encode_hex(&v))
}

#[pyfunction]
fn decompile_hex(hex: &str) -> PyResult<String> {
    let raw = decode_hex(hex)?;
    synx_core::Synx::decompile(&raw)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
}

/// True if decoding the **hex prefix** (up to 16 hex chars → 8 bytes) yields a `.synxb` magic header.
#[pyfunction]
fn is_synxb_hex(hex: &str) -> PyResult<bool> {
    let s = hex.trim();
    let prefix_len = s.len().min(16);
    if prefix_len < 10 {
        return Ok(false);
    }
    let raw = decode_hex(&s[..prefix_len])?;
    if raw.len() < 5 {
        return Ok(false);
    }
    Ok(synx_core::Synx::is_synxb(&raw))
}

/// Parse with engine resolution (!active mode).
/// Accepts optional env dict and base_path for :env and :include resolution.
#[pyfunction]
#[pyo3(signature = (text, env=None, base_path=None))]
fn parse_active(
    py: Python<'_>,
    text: &str,
    env: Option<std::collections::HashMap<String, String>>,
    base_path: Option<String>,
) -> PyResult<Py<PyAny>> {
    let mut result = synx_core::parse(text);
    if result.mode == Mode::Active {
        let mut opts = Options::default();
        if let Some(e) = env {
            opts.env = Some(e);
        }
        if let Some(bp) = base_path {
            opts.base_path = Some(bp);
        }
        synx_core::resolve(&mut result, &opts);
    }
    Ok(value_to_py(py, &result.root))
}

/// Parse a `!tool` SYNX string. Returns `{ tool: "name", params: { ... } }` for calls,
/// or `{ tools: [ { name, params } ] }` for schema definitions.
/// If the text is also `!active`, markers are resolved before reshaping.
#[pyfunction]
#[pyo3(signature = (text, env=None, base_path=None))]
fn parse_tool(
    py: Python<'_>,
    text: &str,
    env: Option<std::collections::HashMap<String, String>>,
    base_path: Option<String>,
) -> PyResult<Py<PyAny>> {
    let mut result = synx_core::parse(text);
    if result.mode == Mode::Active {
        let mut opts = Options::default();
        if let Some(e) = env {
            opts.env = Some(e);
        }
        if let Some(bp) = base_path {
            opts.base_path = Some(bp);
        }
        synx_core::resolve(&mut result, &opts);
    }
    let shaped = synx_core::reshape_tool_output(&result.root, result.schema);
    Ok(value_to_py(py, &shaped))
}

/// Convert a Python dict/list/value back to a SYNX string.
#[pyfunction]
fn stringify(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<String> {
    let val = py_to_value(py, obj)?;
    Ok(synx_core::Synx::stringify(&val))
}

/// Reformat a SYNX string into canonical form (sorted keys, normalized indentation).
#[pyfunction]
fn format(text: &str) -> PyResult<String> {
    Ok(synx_core::Synx::format(text))
}

/// Compile a SYNX string into compact binary .synxb format.
/// If `resolved` is true, metadata/includes are stripped and values are resolved.
#[pyfunction]
#[pyo3(signature = (text, resolved=false))]
fn compile(text: &str, resolved: bool) -> PyResult<Vec<u8>> {
    Ok(synx_core::Synx::compile(text, resolved))
}

/// Decompile a .synxb binary back into a SYNX string.
#[pyfunction]
fn decompile(data: &[u8]) -> PyResult<String> {
    synx_core::Synx::decompile(data)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
}

/// Check whether bytes start with the .synxb magic header.
#[pyfunction]
fn is_synxb(data: &[u8]) -> bool {
    synx_core::Synx::is_synxb(data)
}

/// Wrap raw SYNX text in a labeled code block suitable for LLM system prompts.
///
/// Returns a string like:
///   Core memory (SYNX):
///   ```synx
///   <text>
///   ```
#[pyfunction]
#[pyo3(signature = (text, label="Memory"))]
fn to_prompt_block(text: &str, label: &str) -> PyResult<String> {
    let trimmed = text.trim();
    Ok(format!("{label} (SYNX):\n```synx\n{trimmed}\n```"))
}

/// Structural diff between two parsed SYNX dicts.
/// Returns `{ "added": {...}, "removed": {...}, "changed": {...}, "unchanged": [...] }`.
#[pyfunction]
fn diff(py: Python<'_>, a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let val_a = py_to_value(py, a)?;
    let val_b = py_to_value(py, b)?;
    let map_a = match val_a { Value::Object(m) => m, _ => synx_core::Map::default() };
    let map_b = match val_b { Value::Object(m) => m, _ => synx_core::Map::default() };
    let result = synx_core::Synx::diff(&map_a, &map_b);
    let val = synx_core::diff_to_value(&result);
    Ok(value_to_py(py, &val))
}

/// Structural diff between two SYNX strings. Returns JSON.
#[pyfunction]
fn diff_json(text_a: &str, text_b: &str) -> PyResult<String> {
    let map_a = synx_core::Synx::parse(text_a);
    let map_b = synx_core::Synx::parse(text_b);
    let result = synx_core::Synx::diff(&map_a, &map_b);
    let val = synx_core::diff_to_value(&result);
    Ok(synx_core::to_json(&val))
}

/// Load and parse a .synx file. Returns a Python dict.
#[pyfunction]
fn load(py: Python<'_>, file_path: &str) -> PyResult<Py<PyAny>> {
    let text = std::fs::read_to_string(file_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))?;
    let map = synx_core::Synx::parse(&text);
    Ok(value_to_py(py, &Value::Object(map)))
}

/// Load, parse and resolve a .synx file with !active engine. Returns a Python dict.
#[pyfunction]
#[pyo3(signature = (file_path, base_path=None))]
fn load_active(py: Python<'_>, file_path: &str, base_path: Option<&str>) -> PyResult<Py<PyAny>> {
    let text = std::fs::read_to_string(file_path)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))?;
    let mut opts = synx_core::Options::default();
    if let Some(bp) = base_path.or_else(|| std::path::Path::new(file_path).parent().and_then(|p| p.to_str())) {
        opts.base_path = Some(bp.to_string());
    }
    let map = synx_core::Synx::parse_active(&text, &opts);
    Ok(value_to_py(py, &Value::Object(map)))
}

/// Serialize a Python dict and save to a .synx file.
#[pyfunction]
fn save(py: Python<'_>, file_path: &str, obj: &Bound<'_, PyAny>) -> PyResult<()> {
    let val = py_to_value(py, obj)?;
    let text = synx_core::Synx::stringify(&val);
    std::fs::write(file_path, text)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyIOError, _>(e.to_string()))
}

/// Convert a JSON string to SYNX text.
#[pyfunction]
fn from_json(json_text: &str) -> PyResult<String> {
    let val: Value = serde_json::from_str(json_text)
        .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
    Ok(synx_core::Synx::stringify(&val))
}

/// SYNX Python module.
#[pymodule]
fn synx_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(parse_active_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(parse_tool_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(stringify_json, m)?)?;
    m.add_function(wrap_pyfunction!(compile_hex, m)?)?;
    m.add_function(wrap_pyfunction!(decompile_hex, m)?)?;
    m.add_function(wrap_pyfunction!(is_synxb_hex, m)?)?;
    m.add_function(wrap_pyfunction!(parse_active, m)?)?;
    m.add_function(wrap_pyfunction!(parse_tool, m)?)?;
    m.add_function(wrap_pyfunction!(stringify, m)?)?;
    m.add_function(wrap_pyfunction!(format, m)?)?;
    m.add_function(wrap_pyfunction!(compile, m)?)?;
    m.add_function(wrap_pyfunction!(decompile, m)?)?;
    m.add_function(wrap_pyfunction!(is_synxb, m)?)?;
    m.add_function(wrap_pyfunction!(to_prompt_block, m)?)?;
    m.add_function(wrap_pyfunction!(diff, m)?)?;
    m.add_function(wrap_pyfunction!(diff_json, m)?)?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(load_active, m)?)?;
    m.add_function(wrap_pyfunction!(save, m)?)?;
    m.add_function(wrap_pyfunction!(from_json, m)?)?;
    synxl::register(m)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke_parse_to_json_and_format() {
        let text = "name John\nage 25\n";
        let json = super::parse_to_json(text).expect("parse_to_json should succeed");
        assert!(json.contains("\"name\":\"John\""));

        let formatted = super::format("b 2\na 1\n").expect("format should succeed");
        assert!(formatted.contains("a 1"));
        assert!(formatted.contains("b 2"));
    }

    #[test]
    fn smoke_core_parse_active_and_stringify() {
        // Test via synx-core directly (Python::with_gil requires auto-initialize
        // which conflicts with extension-module)
        let mut result = synx_core::parse("!active\nname John\n");
        let opts = synx_core::Options {
            env: Some(std::collections::HashMap::new()),
            base_path: Some(".".into()),
            ..Default::default()
        };
        synx_core::resolve(&mut result, &opts);

        match &result.root {
            synx_core::Value::Object(map) => {
                match map.get("name") {
                    Some(synx_core::Value::String(s)) => assert_eq!(s, "John"),
                    other => panic!("expected String(\"John\"), got {:?}", other),
                }
            }
            _ => panic!("expected Object root"),
        }

        let synx_text = synx_core::Synx::stringify(&result.root);
        assert!(synx_text.contains("name John"));
    }

    #[test]
    fn smoke_stringify_json_and_hex_codec() {
        let json = r#"{"x":1,"y":2}"#;
        let synx = super::stringify_json(json).expect("stringify_json");
        assert!(synx.contains("x 1"));

        let h = super::compile_hex("a 1\n", false).expect("compile_hex");
        assert!(!h.is_empty());
        let back = super::decompile_hex(&h).expect("decompile_hex");
        assert!(back.contains("a"));
        assert!(super::is_synxb_hex(&h).expect("is_synxb_hex"));

        let j2 = super::parse_active_to_json("!active\nport:env:default:99 PORT\n", None, None)
            .expect("parse_active_to_json");
        assert!(j2.contains("99"));
    }
}
