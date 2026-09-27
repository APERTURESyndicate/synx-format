"""Regression set for the SYNXL hardening pass (Python binding).

Every case here is the minimised form of a defect that shipped. The binding's
parsing side delegates to the reference implementation and is covered by the
conformance suite; what is exercised here is the direction the suite cannot
reach — Python values going *out* through ``synxl_write``, and the object model
the binding hands back.

Section references are to ``docs/spec/SYNXL-1-NORMATIVE.md``.
"""

import math
import sys

import pytest

synx_native = pytest.importorskip("synx_native")

P = "!synxl 1"


# ── Conversion from Python (the direction the conformance suite cannot cover) ──

def test_reference_cycle_raises_instead_of_killing_the_process():
    """A cycle used to overflow the native stack: no traceback, nothing to catch.

    ``json.dumps`` raises ``ValueError`` on the same input, and so must this.
    """
    cyclic = {"a": 1}
    cyclic["self"] = cyclic
    with pytest.raises(ValueError):
        synx_native.synxl_write([cyclic])


def test_deep_nesting_raises_below_the_interpreter_recursion_limit():
    """The old threshold sat under ``sys.getrecursionlimit()``, so ordinary
    Python code could reach it and take the process down."""
    deep = node = {}
    for _ in range(sys.getrecursionlimit() * 2):
        node["n"] = {}
        node = node["n"]
    with pytest.raises(ValueError):
        synx_native.synxl_write([{"id": 1, "d": deep}])


@pytest.mark.parametrize(
    "fields",
    [
        ["a;b", "c"],       # a `;` in a name splits one column into two (§5.1.1)
        ["a\nb", "c"],      # a newline splits one record into two
        ["a[block]", "c"],  # read back as a constraint, dropping the column
        ["a(int)", "c"],    # read back as a type, renaming the column
        ["a b", "c"],       # whitespace is excluded by §5.1.1
        ["", "c"],          # an empty name has no representation
        ["a", "a"],         # §5.1.4 rejects duplicates
    ],
)
def test_invalid_field_names_are_refused_not_written(fields):
    """§14.3 — reject loudly. These used to produce a document that either
    failed to parse or, worse, parsed into a *different* schema in silence."""
    records = [{name or "x": 1 for name in fields}]
    with pytest.raises(Exception) as excinfo:
        synx_native.synxl_write(records, fields=fields)
    assert type(excinfo.value).__name__ in {"SynxlError", "ValueError", "TypeError"}


def test_non_finite_float_keeps_its_record():
    """§14.3 writes a non-finite float as an empty part, and §7.2 gives the
    all-null record the single token `;`. Emitting an empty line instead made
    the record invisible (§3.5) and it disappeared on the way back in."""
    text = synx_native.synxl_write([{"a": math.inf}])
    assert synx_native.synxl_parse(text).records == [{"a": None}]

    text = synx_native.synxl_write([{"a": math.nan}])
    assert synx_native.synxl_parse(text).records == [{"a": None}]


def test_integer_outside_i64_is_refused():
    """Silently degrading to float rewrote the value; degrading further to a
    string changed its type. Neither is what the caller wrote."""
    with pytest.raises(ValueError):
        synx_native.synxl_write([{"id": 1, "n": 2 ** 63}])
    with pytest.raises(ValueError):
        synx_native.synxl_write([{"id": 1, "n": 10 ** 400}])


def test_i64_boundary_values_are_written_exactly():
    text = synx_native.synxl_write([{"lo": -(2 ** 63), "hi": 2 ** 63 - 1}])
    record = synx_native.synxl_parse(text).records[0]
    assert record["hi"] == 2 ** 63 - 1
    assert record["lo"] == -(2 ** 63)


@pytest.mark.parametrize("value", [{1, 2}, b"raw", complex(1, 2), object()])
def test_unconvertible_values_are_refused_not_stringified(value):
    """The catch-all used to write ``str(obj)``, so a set or a class landed in
    the dataset as its repr and looked like data."""
    with pytest.raises(TypeError):
        synx_native.synxl_write([{"id": 1, "v": value}])


def test_tuple_becomes_an_array():
    text = synx_native.synxl_write([{"id": 1, "t": (1, 2, 3)}])
    assert synx_native.synxl_parse(text).records[0]["t"] == [1, 2, 3]


def test_fields_rejects_a_bare_string():
    """A `str` is iterable, so ``fields="ab"`` used to declare two columns."""
    with pytest.raises(TypeError):
        synx_native.synxl_write([{"a": 1, "b": 2}], fields="ab")


# ── The object model handed back ──────────────────────────────────────────────

def test_records_cannot_be_mutated_into_disagreeing_with_the_document():
    """`records` used to hand out one shared list, while ``len()``, indexing and
    ``to_json()`` kept answering from the Rust vector."""
    doc = synx_native.synxl_parse(f"{P}\n!fields a ; b\n1 ; 2\n3 ; 4\n")
    grabbed = doc.records
    grabbed.append({"a": 999})
    grabbed.clear()

    assert len(doc) == 2
    assert len(doc.records) == 2
    assert doc.to_json() == '[{"a":1,"b":2},{"a":3,"b":4}]'


def test_record_is_a_complete_mapping():
    """Without ``__iter__`` CPython falls back to the sequence protocol, so
    ``for name in record`` silently yielded nothing."""
    record = next(synx_native.synxl_stream_records(f"{P}\n!fields id ; name\n1 ; Wario\n"))

    assert sorted(record) == ["id", "name"]
    assert dict(record) == {"id": 1, "name": "Wario"}
    assert sorted(record.items()) == [("id", 1), ("name", "Wario")]
    assert record.get("missing", "default") == "default"
    assert record["id"] == 1
    with pytest.raises(KeyError):
        record["missing"]


def test_paths_accept_os_pathlike(tmp_path):
    path = tmp_path / "data.synxl"
    path.write_text(f"{P}\n!fields a\n1\n", encoding="utf-8")
    assert synx_native.synxl_load(path).records == [{"a": 1}]
    assert list(synx_native.synxl_stream_file(path)) == [{"a": 1}]


# ── Semantics inherited from the reference implementation ─────────────────────

def test_directive_inside_a_block_is_never_executed(tmp_path):
    """§9.4 — the load-bearing security requirement: a dataset row must not
    become a file-read primitive."""
    secret = tmp_path / "secret.synx"
    secret.write_text("leaked CANARY_VALUE_9f21\n", encoding="utf-8")

    doc = f"{P}\n!fields id ; b[block]\n1\n  b\n    !include {secret}\n  c\n"
    assert "CANARY" not in synx_native.synxl_parse(doc).to_json()

    inside_body = f"{P}\n!fields id ; b[block]\n1\n  b |+\n      !include {secret}\n"
    projection = synx_native.synxl_parse(inside_body).to_json()
    assert "CANARY" not in projection      # not executed
    assert "!include" in projection        # but preserved as data


def test_random_hint_in_a_block_is_not_interpreted():
    """§8.3 / §1.4 — two reads of the same bytes must project identically."""
    doc = f"{P}\n!fields id ; b[block]\n1\n  b\n    n(random) x\n"
    first = synx_native.synxl_parse(doc).to_json()
    second = synx_native.synxl_parse(doc).to_json()
    assert first == second
    assert first == '[{"b":{"n":"x"},"id":1}]'


def test_comments_inside_a_multiline_body_are_content():
    doc = f"{P}\n!fields id ; code[block]\n1\n  code |+\n      def f():\n          # keep me\n          return 1\n"
    assert "# keep me" in synx_native.synxl_parse(doc).to_json()


def test_block_truncation_is_reported():
    """§11.2 / §16.4 — a consumer that treats "no diagnostics" as "nothing was
    lost" has to be told when a SYNX limit dropped block content."""
    body = "\n".join("      " + "x" * 100 for _ in range(21000))
    doc = f"{P}\n!fields id(int) ; text[block]\n1\n  text |+\n{body}\n"
    parsed = synx_native.synxl_parse(doc)
    assert any(d["kind"] == "RecordTruncated" for d in parsed.diagnostics)
