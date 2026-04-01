# SYNX 3.6.0 — Mojo ↔ CPython `synx_native` (same Rust engine as `synx-core`).
# Requires: `pip install synx-format` (or maturin develop) so `synx_native` is importable.

from std.python import Python
from std.python.python_object import PythonObject


def _sn() raises -> PythonObject:
    return Python.import_module("synx_native")


def _py_to_string(obj: PythonObject) raises -> String:
    var py = Python()
    var s = Python.str(obj)
    return String(py.as_string_slice(s))


def parse_json(text: String) raises -> String:
    var sn = _sn()
    var out = sn.parse_to_json(PythonObject(text))
    return _py_to_string(out)


def parse_active_json(text: String) raises -> String:
    var sn = _sn()
    var out = sn.parse_active_to_json(PythonObject(text))
    return _py_to_string(out)


def parse_tool_json(text: String) raises -> String:
    var sn = _sn()
    var out = sn.parse_tool_to_json(PythonObject(text))
    return _py_to_string(out)


def stringify_json(json_text: String) raises -> String:
    var sn = _sn()
    var out = sn.stringify_json(PythonObject(json_text))
    return _py_to_string(out)


def format_synx(text: String) raises -> String:
    var sn = _sn()
    var out = sn.format(PythonObject(text))
    return _py_to_string(out)


def diff_json(text_a: String, text_b: String) raises -> String:
    var sn = _sn()
    var out = sn.diff_json(PythonObject(text_a), PythonObject(text_b))
    return _py_to_string(out)


def compile_hex(text: String, resolved: Bool) raises -> String:
    var sn = _sn()
    var out = sn.compile_hex(PythonObject(text), PythonObject(resolved))
    return _py_to_string(out)


def decompile_hex(hex_text: String) raises -> String:
    var sn = _sn()
    var out = sn.decompile_hex(PythonObject(hex_text))
    return _py_to_string(out)


def is_synxb_hex(hex_text: String) raises -> Bool:
    var sn = _sn()
    var r = sn.is_synxb_hex(PythonObject(hex_text))
    return Python.is_true(r)
