//! SYNXL ("SYNX Lines") Python binding — the record-stream counterpart of
//! JSONL/CSV. Wraps `synx_core::synxl`, the reference implementation of
//! `docs/spec/SYNXL-1-NORMATIVE.md`. Section references (`§11.1`, …) are to
//! that document.
//!
//! Shape of the Python API:
//!
//! * hard errors (§11.1) raise [`SynxlError`] carrying `kind` / `line` / `message`;
//! * I/O failures raise `OSError` — never `SynxlError`, because a failed read
//!   says nothing about the document (see [`synx_core::synxl::SynxlStreamError`]);
//! * diagnostics (§11.2) are plain dicts with `record_index` / `line` / `kind` / `message`;
//! * records are plain dicts (converted with the module's `value_to_py`);
//! * streaming (§15.1) is a real Python iterator — no document materialisation,
//!   in memory or straight off disk.

use std::fs::File;
use std::io::BufReader;

use pyo3::create_exception;
use pyo3::exceptions::{PyIOError, PyIndexError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use synx_core::synxl::{
    self, Diagnostic, FieldDecl, FieldList, SynxlDocument as CoreDocument,
    SynxlError as CoreError, SynxlErrorKind, SynxlOptions, SynxlReaderOwned,
    SynxlRecord as CoreRecord, SynxlStreamError, SynxlStreamReader,
};
use synx_core::Value;

use crate::{py_to_value, value_to_py};

create_exception!(
    synx_native,
    SynxlError,
    pyo3::exceptions::PyValueError,
    "SYNXL hard error (SYNXL-1 §11.1). Carries `kind`, `line` and `message`."
);

/// Turn a core hard error into the Python exception, keeping §11.1's fields.
fn synxl_err(py: Python<'_>, err: &CoreError) -> PyErr {
    let py_err = SynxlError::new_err(err.to_string());
    let value = py_err.value(py);
    let _ = value.setattr("kind", err.kind.as_str());
    let _ = value.setattr("line", err.line);
    let _ = value.setattr("message", err.message.as_str());
    py_err
}

fn raise(py: Python<'_>, kind: SynxlErrorKind, line: usize, message: impl Into<String>) -> PyErr {
    synxl_err(py, &CoreError::new(kind, line, message))
}

/// §15.1 — the two arms of a streaming failure stay apart in Python too: a
/// malformed document is a `SynxlError`, a failed read is an `OSError`.
fn stream_err(py: Python<'_>, err: &SynxlStreamError) -> PyErr {
    match err.as_format() {
        Some(format) => synxl_err(py, format),
        None => PyIOError::new_err(err.to_string()),
    }
}

/// §11.2 — one diagnostic as a plain dict.
fn diagnostic_to_py(py: Python<'_>, d: &Diagnostic) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("record_index", d.record_index)?;
    dict.set_item("line", d.line)?;
    dict.set_item("kind", d.kind.as_str())?;
    dict.set_item("message", d.message.as_str())?;
    Ok(dict.into_any().unbind())
}

fn diagnostics_to_py(py: Python<'_>, items: &[Diagnostic]) -> PyResult<PyObject> {
    let list: Vec<PyObject> = items
        .iter()
        .map(|d| diagnostic_to_py(py, d))
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, list)?.into_any().unbind())
}

/// §5 — one field declaration as a plain dict.
fn field_to_py(py: Python<'_>, f: &FieldDecl) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("name", f.name.as_str())?;
    dict.set_item("type", f.type_name())?;
    dict.set_item("block", f.block)?;
    let c = &f.constraints;
    dict.set_item("required", c.required)?;
    dict.set_item("readonly", c.readonly)?;
    dict.set_item("min", c.min)?;
    dict.set_item("max", c.max)?;
    dict.set_item("pattern", c.pattern.as_deref())?;
    dict.set_item("enum", c.enum_values.as_deref())?;
    Ok(dict.into_any().unbind())
}

/// §5 / §4.2 — a whole field list: its source line, arity and declarations.
fn field_list_to_py(py: Python<'_>, fl: &FieldList) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    dict.set_item("line", fl.line)?;
    dict.set_item("arity", fl.arity())?;
    let fields: Vec<PyObject> = fl
        .fields()
        .iter()
        .map(|f| field_to_py(py, f))
        .collect::<PyResult<_>>()?;
    dict.set_item("fields", PyList::new(py, fields)?)?;
    Ok(dict.into_any().unbind())
}

fn field_lists_to_py(py: Python<'_>, lists: &[FieldList]) -> PyResult<PyObject> {
    let items: Vec<PyObject> = lists
        .iter()
        .map(|fl| field_list_to_py(py, fl))
        .collect::<PyResult<_>>()?;
    Ok(PyList::new(py, items)?.into_any().unbind())
}

fn options(validate: bool) -> SynxlOptions {
    SynxlOptions { validate }
}

// ─── Document ────────────────────────────────────────────────

/// A fully parsed SYNXL document (§4–§10).
///
/// Iterating the document yields the record dicts; `diagnostics` holds every
/// §11.2 observation in record order.
#[pyclass(name = "SynxlDocument", module = "synx_native", frozen)]
pub struct PySynxlDocument {
    doc: CoreDocument,
    records_cache: std::sync::OnceLock<Py<PyList>>,
}

impl PySynxlDocument {
    fn new(doc: CoreDocument) -> Self {
        Self { doc, records_cache: std::sync::OnceLock::new() }
    }

    /// Records as Python dicts, converted once and cached.
    fn record_list(&self, py: Python<'_>) -> PyResult<&Py<PyList>> {
        if let Some(list) = self.records_cache.get() {
            return Ok(list);
        }
        let items: Vec<PyObject> = self.doc.records.iter().map(|v| value_to_py(py, v)).collect();
        let list = PyList::new(py, items)?.unbind();
        let _ = self.records_cache.set(list);
        Ok(self.records_cache.get().expect("record list just set"))
    }
}

#[pymethods]
impl PySynxlDocument {
    /// Format version from the prologue (§4.1).
    #[getter]
    fn version(&self) -> u32 {
        self.doc.version
    }

    /// Records in document order, each a dict (§12.1).
    #[getter]
    fn records(&self, py: Python<'_>) -> PyResult<Py<PyList>> {
        Ok(self.record_list(py)?.clone_ref(py))
    }

    /// Every §11.2 diagnostic, in record order.
    #[getter]
    fn diagnostics(&self, py: Python<'_>) -> PyResult<PyObject> {
        diagnostics_to_py(py, &self.doc.diagnostics)
    }

    /// Field lists in declaration order (§4.2).
    #[getter]
    fn field_lists(&self, py: Python<'_>) -> PyResult<PyObject> {
        field_lists_to_py(py, &self.doc.field_lists)
    }

    /// `records[i]` was parsed under `field_lists[record_field_lists[i]]`.
    #[getter]
    fn record_field_lists(&self) -> Vec<usize> {
        self.doc.record_field_lists.clone()
    }

    /// 1-based source line of each record's record line.
    #[getter]
    fn record_lines(&self) -> Vec<usize> {
        self.doc.record_lines.clone()
    }

    /// The field list in effect for record `index`, or `None`.
    fn field_list_for(&self, py: Python<'_>, index: usize) -> PyResult<Option<PyObject>> {
        match self.doc.field_list_for(index) {
            Some(fl) => Ok(Some(field_list_to_py(py, fl)?)),
            None => Ok(None),
        }
    }

    /// Canonical JSON **array** projection (§12.1).
    fn to_json(&self) -> String {
        self.doc.to_json()
    }

    /// Canonical **NDJSON** projection (§12.2).
    fn to_ndjson(&self) -> String {
        self.doc.to_ndjson()
    }

    /// Canonical SYNXL serialization (§14).
    ///
    /// Raises `SynxlError` with kind `Unwritable` when a value has no SYNXL
    /// rendering (§14.3) — unreachable for a parsed document (§14.1).
    fn to_synxl(&self, py: Python<'_>) -> PyResult<String> {
        self.doc.to_synxl().map_err(|e| synxl_err(py, &e))
    }

    fn __len__(&self) -> usize {
        self.doc.records.len()
    }

    fn __getitem__(&self, py: Python<'_>, index: isize) -> PyResult<PyObject> {
        let len = self.doc.records.len() as isize;
        let idx = if index < 0 { index + len } else { index };
        if idx < 0 || idx >= len {
            return Err(PyIndexError::new_err("SYNXL record index out of range"));
        }
        Ok(value_to_py(py, &self.doc.records[idx as usize]))
    }

    fn __iter__(&self, py: Python<'_>) -> PyResult<PyObject> {
        Ok(self.record_list(py)?.bind(py).try_iter()?.into_any().unbind())
    }

    fn __repr__(&self) -> String {
        format!(
            "<SynxlDocument version={} records={} field_lists={} diagnostics={}>",
            self.doc.version,
            self.doc.records.len(),
            self.doc.field_lists.len(),
            self.doc.diagnostics.len()
        )
    }
}

// ─── Streaming record (§15.1) ────────────────────────────────

/// One record produced by `synxl_stream_records` — the values dict plus its
/// position and its own diagnostics (§11.2).
#[pyclass(name = "SynxlRecord", module = "synx_native", frozen)]
pub struct PySynxlRecord {
    /// 0-based position in the document.
    #[pyo3(get)]
    index: usize,
    /// 1-based source line of the record line.
    #[pyo3(get)]
    line: usize,
    /// Index into the reader's field lists.
    #[pyo3(get)]
    field_list: usize,
    values: Py<PyDict>,
    diagnostics: PyObject,
}

#[pymethods]
impl PySynxlRecord {
    /// The record itself, as a plain dict.
    #[getter]
    fn values(&self, py: Python<'_>) -> Py<PyDict> {
        self.values.clone_ref(py)
    }

    /// Diagnostics produced by this record alone (§11.2).
    #[getter]
    fn diagnostics(&self, py: Python<'_>) -> PyObject {
        self.diagnostics.clone_ref(py)
    }

    /// Alias for `values` — mirrors `dict(record)` usage.
    fn to_dict(&self, py: Python<'_>) -> Py<PyDict> {
        self.values.clone_ref(py)
    }

    fn keys(&self, py: Python<'_>) -> PyResult<PyObject> {
        Ok(self.values.bind(py).keys().into_any().unbind())
    }

    fn __getitem__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<PyObject> {
        match self.values.bind(py).get_item(key)? {
            Some(v) => Ok(v.unbind()),
            None => Err(PyIndexError::new_err(format!("no field {}", key))),
        }
    }

    fn __contains__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<bool> {
        self.values.bind(py).contains(key)
    }

    fn __len__(&self, py: Python<'_>) -> usize {
        self.values.bind(py).len()
    }

    fn __repr__(&self, py: Python<'_>) -> String {
        format!(
            "<SynxlRecord index={} line={} fields={}>",
            self.index,
            self.line,
            self.values.bind(py).len()
        )
    }
}

fn record_to_py(py: Python<'_>, rec: &CoreRecord) -> PyResult<PySynxlRecord> {
    let values = value_to_py(py, &rec.value);
    let dict = values
        .bind(py)
        .downcast::<PyDict>()
        .map_err(|_| PyTypeError::new_err("SYNXL record is not an object"))?
        .clone()
        .unbind();
    Ok(PySynxlRecord {
        index: rec.index,
        line: rec.line,
        field_list: rec.field_list,
        values: dict,
        diagnostics: diagnostics_to_py(py, &rec.diagnostics)?,
    })
}

// ─── Streaming reader (§15.1) ────────────────────────────────

/// Turn one streamed record into what the iterator yields.
fn yield_record(py: Python<'_>, rec: &CoreRecord, with_meta: bool) -> PyResult<PyObject> {
    if with_meta {
        Ok(Py::new(py, record_to_py(py, rec)?)?.into_any())
    } else {
        Ok(value_to_py(py, &rec.value))
    }
}

/// Iterator over the records of an in-memory SYNXL document.
///
/// Yields record dicts, or `SynxlRecord` objects when built by
/// `synxl_stream_records`. A hard error (§11.1) is raised from `__next__` and
/// ends the iteration.
#[pyclass(name = "SynxlStream", module = "synx_native")]
pub struct PySynxlStream {
    /// Owns its document, so nothing borrows across the FFI boundary.
    reader: SynxlReaderOwned,
    /// Yield `SynxlRecord` objects instead of bare dicts.
    with_meta: bool,
}

impl PySynxlStream {
    fn build(text: String, opts: SynxlOptions, with_meta: bool) -> Result<Self, CoreError> {
        Ok(Self { reader: SynxlReaderOwned::with_options(text, opts)?, with_meta })
    }
}

#[pymethods]
impl PySynxlStream {
    /// Format version from the prologue (§4.1).
    #[getter]
    fn version(&self) -> u32 {
        self.reader.version()
    }

    /// Field lists seen so far, in declaration order (§4.2).
    #[getter]
    fn field_lists(&self, py: Python<'_>) -> PyResult<PyObject> {
        field_lists_to_py(py, self.reader.field_lists())
    }

    /// Diagnostics found after the last record — `OrphanBlockLine` at end of
    /// input (§11.2). Empty until iteration finishes.
    #[getter]
    fn trailing_diagnostics(&self, py: Python<'_>) -> PyResult<PyObject> {
        diagnostics_to_py(py, self.reader.trailing_diagnostics())
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        match self.reader.next() {
            None => Ok(None),
            Some(Err(err)) => Err(synxl_err(py, &err)),
            Some(Ok(rec)) => Ok(Some(yield_record(py, &rec, self.with_meta)?)),
        }
    }
}

/// Iterator over the records of a SYNXL **file**, read incrementally (§15.1).
///
/// The document is never held in memory: live memory is one record, bounded by
/// `MAX_SYNXL_RECORD_BYTES` (§13). A malformed document raises `SynxlError`; a
/// failed read raises `OSError`.
#[pyclass(name = "SynxlFileStream", module = "synx_native")]
pub struct PySynxlFileStream {
    reader: SynxlStreamReader<BufReader<File>>,
    with_meta: bool,
}

impl PySynxlFileStream {
    fn build(
        py: Python<'_>,
        file_path: &str,
        opts: SynxlOptions,
        with_meta: bool,
    ) -> PyResult<Self> {
        let file = File::open(file_path).map_err(|e| PyIOError::new_err(e.to_string()))?;
        let reader = SynxlStreamReader::with_options(BufReader::new(file), opts)
            .map_err(|e| stream_err(py, &e))?;
        Ok(Self { reader, with_meta })
    }
}

#[pymethods]
impl PySynxlFileStream {
    /// Format version from the prologue (§4.1).
    #[getter]
    fn version(&self) -> u32 {
        self.reader.version()
    }

    /// Field lists seen so far, in declaration order (§4.2).
    #[getter]
    fn field_lists(&self, py: Python<'_>) -> PyResult<PyObject> {
        field_lists_to_py(py, self.reader.field_lists())
    }

    /// Diagnostics recorded after the last record (§11.2).
    #[getter]
    fn trailing_diagnostics(&self, py: Python<'_>) -> PyResult<PyObject> {
        diagnostics_to_py(py, self.reader.trailing_diagnostics())
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<PyObject>> {
        match self.reader.next() {
            None => Ok(None),
            Some(Err(err)) => Err(stream_err(py, &err)),
            Some(Ok(rec)) => Ok(Some(yield_record(py, &rec, self.with_meta)?)),
        }
    }
}

// ─── Functions ───────────────────────────────────────────────

/// Parse a whole SYNXL document (§4–§10).
///
/// `validate=True` turns on the §8.4 validating mode, which reports declared
/// constraint violations as `ConstraintViolation` diagnostics.
/// Raises `SynxlError` on any §11.1 hard error.
#[pyfunction]
#[pyo3(signature = (text, *, validate=false))]
fn synxl_parse(py: Python<'_>, text: &str, validate: bool) -> PyResult<PySynxlDocument> {
    match synxl::parse_lines_with(text, &options(validate)) {
        Ok(doc) => Ok(PySynxlDocument::new(doc)),
        Err(err) => Err(synxl_err(py, &err)),
    }
}

/// Read a `.synxl` file and parse it (§4–§10).
#[pyfunction]
#[pyo3(signature = (file_path, *, validate=false))]
fn synxl_load(py: Python<'_>, file_path: &str, validate: bool) -> PyResult<PySynxlDocument> {
    let text = std::fs::read_to_string(file_path)
        .map_err(|e| PyIOError::new_err(e.to_string()))?;
    synxl_parse(py, &text, validate)
}

/// Stream records without materialising the document (§15.1).
///
/// Returns an iterator of record dicts: `for record in synxl_stream(text): ...`
#[pyfunction]
#[pyo3(signature = (text, *, validate=false))]
fn synxl_stream(py: Python<'_>, text: String, validate: bool) -> PyResult<PySynxlStream> {
    PySynxlStream::build(text, options(validate), false).map_err(|e| synxl_err(py, &e))
}

/// Like `synxl_stream`, but yields `SynxlRecord` objects, which add `index`,
/// `line`, `field_list` and per-record `diagnostics` (§11.2).
#[pyfunction]
#[pyo3(signature = (text, *, validate=false))]
fn synxl_stream_records(py: Python<'_>, text: String, validate: bool) -> PyResult<PySynxlStream> {
    PySynxlStream::build(text, options(validate), true).map_err(|e| synxl_err(py, &e))
}

/// Stream records straight off disk, without loading the file (§15.1).
///
/// Live memory is one record, bounded by `MAX_SYNXL_RECORD_BYTES` (§13) — use
/// this instead of `synxl_load` for datasets that do not fit in memory. A
/// malformed document raises `SynxlError`; a read failure raises `OSError`.
#[pyfunction]
#[pyo3(signature = (file_path, *, validate=false))]
fn synxl_stream_file(
    py: Python<'_>,
    file_path: &str,
    validate: bool,
) -> PyResult<PySynxlFileStream> {
    PySynxlFileStream::build(py, file_path, options(validate), false)
}

/// Like `synxl_stream_file`, but yields `SynxlRecord` objects, which add
/// `index`, `line`, `field_list` and per-record `diagnostics` (§11.2).
#[pyfunction]
#[pyo3(signature = (file_path, *, validate=false))]
fn synxl_stream_file_records(
    py: Python<'_>,
    file_path: &str,
    validate: bool,
) -> PyResult<PySynxlFileStream> {
    PySynxlFileStream::build(py, file_path, options(validate), true)
}

/// SYNXL text → canonical JSON array (§12.1).
#[pyfunction]
#[pyo3(signature = (text, *, validate=false))]
fn synxl_to_json(py: Python<'_>, text: &str, validate: bool) -> PyResult<String> {
    match synxl::parse_lines_with(text, &options(validate)) {
        Ok(doc) => Ok(doc.to_json()),
        Err(err) => Err(synxl_err(py, &err)),
    }
}

/// SYNXL text → canonical NDJSON (§12.2), one object per line.
#[pyfunction]
#[pyo3(signature = (text, *, validate=false))]
fn synxl_to_ndjson(py: Python<'_>, text: &str, validate: bool) -> PyResult<String> {
    match synxl::parse_lines_with(text, &options(validate)) {
        Ok(doc) => Ok(doc.to_ndjson()),
        Err(err) => Err(synxl_err(py, &err)),
    }
}

/// Field specs for `synxl_write`: `"name"` or `{"name": ..., "type": ..., "block": ...}`.
fn parse_field_specs(py: Python<'_>, spec: &Bound<'_, PyAny>) -> PyResult<Vec<FieldDecl>> {
    let mut out: Vec<FieldDecl> = Vec::new();
    for item in spec.try_iter()? {
        let item = item?;
        if let Ok(name) = item.extract::<String>() {
            out.push(FieldDecl::new(name));
            continue;
        }
        let dict = item.downcast::<PyDict>().map_err(|_| {
            PyTypeError::new_err("field spec must be a str or a dict with a `name` key")
        })?;
        let name: String = dict
            .get_item("name")?
            .ok_or_else(|| PyTypeError::new_err("field spec dict requires a `name` key"))?
            .extract()?;
        let block: bool = match dict.get_item("block")? {
            Some(v) => v.extract()?,
            None => false,
        };
        let type_hint: Option<String> = match dict.get_item("type")? {
            Some(v) if !v.is_none() => Some(v.extract()?),
            _ => None,
        };
        // §5.3.2 — a `block` field takes its shape from the embedded document;
        // emitting both would produce a document that fails to re-parse.
        if block && type_hint.is_some() {
            return Err(raise(
                py,
                SynxlErrorKind::BlockWithType,
                0,
                format!("field `{}` combines `block` with a type", name),
            ));
        }
        let mut decl = if block {
            FieldDecl::new_block(name)
        } else {
            FieldDecl::new(name)
        };
        decl.type_hint = type_hint;
        out.push(decl);
    }
    Ok(out)
}

/// Serialize records to canonical SYNXL text (§14).
///
/// `records` is any iterable of mappings. `fields` declares the columns and
/// their order; when omitted, the field list is synthesized from the keys of
/// the records in first-seen order. Values that have no inline form (objects,
/// arrays, multi-line strings) are promoted to `[block]` automatically (§14.3).
/// Raises `SynxlError` with kind `Unwritable` when a value has no SYNXL
/// rendering at all (§14.3).
#[pyfunction]
#[pyo3(signature = (records, fields=None))]
fn synxl_write(
    py: Python<'_>,
    records: &Bound<'_, PyAny>,
    fields: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    let mut values: Vec<Value> = Vec::new();
    let mut order: Vec<String> = Vec::new();
    for item in records.try_iter()? {
        let item = item?;
        let value = py_to_value(py, &item)?;
        let map = match &value {
            Value::Object(map) => map,
            _ => return Err(PyTypeError::new_err("each SYNXL record must be a mapping")),
        };
        if fields.is_none() {
            // Python dicts keep insertion order, which is the author's intent;
            // any other mapping falls back to a stable lexicographic order.
            let mut keys: Vec<String> = match item.downcast::<PyDict>() {
                Ok(dict) => dict
                    .keys()
                    .iter()
                    .map(|k| k.extract::<String>())
                    .collect::<PyResult<_>>()?,
                Err(_) => {
                    let mut ks: Vec<String> = map.keys().cloned().collect();
                    ks.sort();
                    ks
                }
            };
            for key in keys.drain(..) {
                if !order.contains(&key) {
                    order.push(key);
                }
            }
        }
        values.push(value);
    }

    let decls: Vec<FieldDecl> = match fields {
        Some(spec) => parse_field_specs(py, spec)?,
        None => order.into_iter().map(FieldDecl::new).collect(),
    };
    synxl::write_lines(&decls, &values).map_err(|e| synxl_err(py, &e))
}

/// Register the SYNXL surface on the `synx_native` module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("SynxlError", py.get_type::<SynxlError>())?;
    m.add_class::<PySynxlDocument>()?;
    m.add_class::<PySynxlRecord>()?;
    m.add_class::<PySynxlStream>()?;
    m.add_class::<PySynxlFileStream>()?;

    m.add_function(wrap_pyfunction!(synxl_parse, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_load, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_stream, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_stream_records, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_stream_file, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_stream_file_records, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_to_json, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_to_ndjson, m)?)?;
    m.add_function(wrap_pyfunction!(synxl_write, m)?)?;

    // §13 resource limits, exposed so callers can size their inputs.
    m.add("SYNXL_VERSION", synxl::SYNXL_VERSION)?;
    m.add("MAX_SYNXL_RECORD_BYTES", synxl::MAX_SYNXL_RECORD_BYTES)?;
    m.add("MAX_SYNXL_FIELDS", synxl::MAX_SYNXL_FIELDS)?;
    m.add("MAX_SYNXL_FIELD_NAME_BYTES", synxl::MAX_SYNXL_FIELD_NAME_BYTES)?;
    m.add("MAX_SYNXL_FIELD_LISTS", synxl::MAX_SYNXL_FIELD_LISTS)?;
    m.add("MAX_SYNXL_RECORDS", synxl::MAX_SYNXL_RECORDS)?;
    Ok(())
}
