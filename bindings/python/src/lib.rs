//! SYNX Python binding — exposes parse/parse_active/stringify/format to Python via PyO3.

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use synx_core::{self, Value, Mode, Options};

/// Convert synx_core::Value to a Python object.
fn value_to_py(py: Python<'_>, val: &Value) -> PyObject {
    match val {
        Value::Null => py.None(),
        Value::Bool(b) => b.into_pyobject(py).unwrap().to_owned().into_any().unbind(),
        Value::Int(n) => n.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Float(f) => f.into_pyobject(py).unwrap().into_any().unbind(),
        Value::String(s) => s.into_pyobject(py).unwrap().into_any().unbind(),
        Value::Secret(_) => "[SECRET]".into_pyobject(py).unwrap().into_any().unbind(),
        Value::Array(arr) => {
            let items: Vec<PyObject> = arr.iter().map(|v| value_to_py(py, v)).collect();
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

/// Convert a Python object back to synx_core::Value.
fn py_to_value(py: Python<'_>, obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    if obj.is_none() {
        Ok(Value::Null)
    } else if let Ok(b) = obj.extract::<bool>() {
        Ok(Value::Bool(b))
    } else if let Ok(n) = obj.extract::<i64>() {
        Ok(Value::Int(n))
    } else if let Ok(f) = obj.extract::<f64>() {
        Ok(Value::Float(f))
    } else if let Ok(s) = obj.extract::<String>() {
        Ok(Value::String(s))
    } else if let Ok(list) = obj.downcast::<PyList>() {
        let mut arr = Vec::new();
        for item in list.iter() {
            arr.push(py_to_value(py, &item)?);
        }
        Ok(Value::Array(arr))
    } else if let Ok(dict) = obj.downcast::<PyDict>() {
        let mut map = std::collections::HashMap::new();
        for (k, v) in dict.iter() {
            let key: String = k.extract()?;
            map.insert(key, py_to_value(py, &v)?);
        }
        Ok(Value::Object(map))
    } else {
        Ok(Value::String(format!("{}", obj)))
    }
}

/// Parse a SYNX string and return a Python dict.
#[pyfunction]
fn parse(py: Python<'_>, text: &str) -> PyResult<PyObject> {
    let result = synx_core::parse(text);
    Ok(value_to_py(py, &result.root))
}

/// Parse a SYNX string as JSON. Returns a JSON string (faster for large files).
#[pyfunction]
fn parse_to_json(text: &str) -> PyResult<String> {
    let result = synx_core::parse(text);
    Ok(synx_core::to_json(&result.root))
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
) -> PyResult<PyObject> {
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

/// SYNX Python module.
#[pymodule]
fn synx_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(parse_active, m)?)?;
    m.add_function(wrap_pyfunction!(stringify, m)?)?;
    m.add_function(wrap_pyfunction!(format, m)?)?;
    m.add_function(wrap_pyfunction!(to_prompt_block, m)?)?;
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
}
