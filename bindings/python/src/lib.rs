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

/// SYNX Python module.
#[pymodule]
fn synx_native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(parse_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(parse_active, m)?)?;
    m.add_function(wrap_pyfunction!(stringify, m)?)?;
    m.add_function(wrap_pyfunction!(format, m)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::PyDict;
    use std::collections::HashMap;

    #[test]
    fn smoke_parse_to_json_and_format() {
        let text = "name John\nage 25\n";
        let json = parse_to_json(text).expect("parse_to_json should succeed");
        assert!(json.contains("\"name\":\"John\""));

        let formatted = format("b 2\na 1\n").expect("format should succeed");
        assert!(formatted.contains("a 1"));
        assert!(formatted.contains("b 2"));
    }

    #[test]
    fn smoke_parse_active_and_stringify() {
        Python::with_gil(|py| {
            let parsed = parse_active(
                py,
                "!active\nname John\n",
                Some(HashMap::new()),
                Some(".".to_string()),
            )
            .expect("parse_active should succeed");

            let dict = parsed.bind(py).downcast::<PyDict>().expect("dict expected");
            let name: String = dict
                .get_item("name")
                .expect("name key must exist")
                .expect("name value must be present")
                .extract()
                .expect("name must be string");
            assert_eq!(name, "John");

            let out = stringify(py, dict.as_any()).expect("stringify should succeed");
            assert!(out.contains("name John"));
        });
    }
}
