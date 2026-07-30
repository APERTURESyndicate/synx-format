"""Tests for the SYNXL surface of the `synx_native` Python module.

SYNXL is specified by `docs/spec/SYNXL-1-NORMATIVE.md`; the section references
below (§11.1, §12.2, …) are to that document.

Build the extension before running::

    maturin develop -m bindings/python/Cargo.toml
    pytest bindings/python/tests
"""

import json

import pytest

import synx_native as synx


CHAT = """!synxl 1
!fields id[type:int, required] ; score[type:float] ; messages[block]

1 ; 0.91
  messages
    - role system
      content You are a helpful assistant.
    - role user
      content |+
          def f(x):
              return x + 1

2 ; 0.74
  messages
    - role user
      content Привет
"""


# ─── document parse (§4–§10) ────────────────────────────────────────────


def test_parse_chat_dataset_with_block_fields():
    doc = synx.synxl_parse(CHAT)

    assert doc.version == synx.SYNXL_VERSION == 1
    assert len(doc) == 2
    assert doc.diagnostics == []

    first, second = doc.records
    assert first["id"] == 1
    assert first["score"] == pytest.approx(0.91)
    assert first["messages"] == [
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "def f(x):\n    return x + 1"},
    ]
    assert second["messages"] == [{"role": "user", "content": "Привет"}]

    # Records are plain dicts, not a custom mapping type.
    assert type(first) is dict


def test_document_is_iterable_and_indexable():
    doc = synx.synxl_parse(CHAT)
    assert [record["id"] for record in doc] == [1, 2]
    assert doc[0]["id"] == 1
    assert doc[-1]["id"] == 2
    with pytest.raises(IndexError):
        doc[2]


def test_field_lists_and_record_positions():
    doc = synx.synxl_parse(CHAT)

    (field_list,) = doc.field_lists
    assert field_list["arity"] == 2  # §5.3 — block fields do not count
    assert field_list["line"] == 2
    names = [f["name"] for f in field_list["fields"]]
    assert names == ["id", "score", "messages"]

    ident, score, messages = field_list["fields"]
    assert ident["type"] == "int" and ident["required"] is True
    assert score["type"] == "float"
    assert messages["block"] is True and messages["type"] is None

    assert doc.record_lines == [4, 13]
    assert doc.record_field_lists == [0, 0]
    assert doc.field_list_for(0) == field_list
    assert doc.field_list_for(99) is None


def test_field_list_redeclared_mid_document():
    text = (
        "!synxl 1\n"
        "!fields id(int) ; name\n"
        "1 ; Wario\n"
        "!fields id(int) ; name ; lang\n"
        "2 ; Waluigi ; it\n"
    )
    doc = synx.synxl_parse(text)
    assert doc.record_field_lists == [0, 1]
    assert len(doc.field_lists) == 2
    # §12.1 — a field absent from the list in effect is absent from the object.
    assert "lang" not in doc.records[0]
    assert doc.records[1]["lang"] == "it"


# ─── diagnostics (§11.2) ────────────────────────────────────────────────


def kinds(diagnostics):
    return [d["kind"] for d in diagnostics]


def test_missing_fields_diagnostic():
    doc = synx.synxl_parse("!synxl 1\n!fields a ; b ; c\n1 ; 2\n")
    (diag,) = doc.diagnostics
    assert diag["kind"] == "MissingFields"
    assert diag["record_index"] == 0
    assert diag["line"] == 3
    assert isinstance(diag["message"], str) and diag["message"]
    # §7.3 — the trailing field is null, the record survives.
    assert doc.records[0] == {"a": 1, "b": 2, "c": None}


def test_extra_fields_diagnostic():
    doc = synx.synxl_parse("!synxl 1\n!fields a ; b\n1 ; 2 ; 3\n")
    (diag,) = doc.diagnostics
    assert diag["kind"] == "ExtraFields"
    assert diag["record_index"] == 0
    assert diag["line"] == 3
    assert doc.records[0] == {"a": 1, "b": 2}


def test_cast_failed_diagnostic():
    doc = synx.synxl_parse("!synxl 1\n!fields id[type:int] ; name\nnope ; Wario\n")
    (diag,) = doc.diagnostics
    assert diag["kind"] == "CastFailed"
    assert diag["record_index"] == 0
    assert diag["line"] == 3
    # §8.2 — a failed cast nulls the cell but keeps the row.
    assert doc.records[0] == {"id": None, "name": "Wario"}


def test_block_key_diagnostics():
    doc = synx.synxl_parse("!synxl 1\n!fields id(int) ; label\n1 ; hello\n  extra_stuff xyz\n")
    assert kinds(doc.diagnostics) == ["UnknownBlockKey"]
    assert doc.diagnostics[0]["line"] == 4  # the block line, not the record line
    assert doc.records[0] == {"id": 1, "label": "hello"}


def test_diagnostics_are_dicts_with_the_normative_fields():
    doc = synx.synxl_parse("!synxl 1\n!fields a ; b ; c\n1 ; 2\n")
    diag = doc.diagnostics[0]
    assert type(diag) is dict
    assert set(diag) == {"record_index", "line", "kind", "message"}


# ─── validating mode (§8.4) ─────────────────────────────────────────────


VALIDATED = "!synxl 1\n!fields id[type:int] ; score[type:float, min:0, max:1]\n1 ; 5.0\n"


def test_constraints_are_not_enforced_by_default():
    doc = synx.synxl_parse(VALIDATED)
    assert doc.diagnostics == []
    assert doc.records[0]["score"] == pytest.approx(5.0)


def test_validate_reports_constraint_violations():
    doc = synx.synxl_parse(VALIDATED, validate=True)
    (diag,) = doc.diagnostics
    assert diag["kind"] == "ConstraintViolation"
    assert diag["record_index"] == 0
    assert diag["line"] == 3
    # §8.4 — reported, never applied: the value is untouched.
    assert doc.records[0]["score"] == pytest.approx(5.0)


def test_validate_flag_is_keyword_only():
    with pytest.raises(TypeError):
        synx.synxl_parse(VALIDATED, True)


# ─── hard errors (§11.1) ────────────────────────────────────────────────


@pytest.mark.parametrize(
    "text, kind, line",
    [
        ("!synxl 1\n!fields a\n1\n!active\n2\n", "UnknownDirective", 4),
        ("!synxl 1\n!fields a ; a\n1 ; 2\n", "DuplicateField", 2),
        ("!synxl 1\n!fields a[block] ; b[block]\n1\n", "ZeroArity", 2),
        ("name Wario\n", "MissingPrologue", 1),
        ("!synxl 2\n!fields a\n1\n", "UnsupportedVersion", 1),
        ("!synxl 1\n1 ; 2\n", "NoFieldList", 2),
        ("!synxl 1\n!fields\n1\n", "MalformedFieldList", 2),
        ("!synxl 1\n!fields a:env\n1\n", "MarkerChain", 2),
        ("!synxl 1\n!fields a(random)\n1\n", "NonDeterministicHint", 2),
        ("!synxl 1\n!fields a(int)[block]\n1\n", "BlockWithType", 2),
    ],
)
def test_hard_errors_raise(text, kind, line):
    with pytest.raises(synx.SynxlError) as excinfo:
        synx.synxl_parse(text)
    err = excinfo.value
    assert err.kind == kind
    assert err.line == line
    assert isinstance(err.message, str) and err.message
    assert kind in str(err) and err.message in str(err)


def test_synxl_error_is_a_value_error():
    assert issubclass(synx.SynxlError, ValueError)
    with pytest.raises(ValueError):
        synx.synxl_parse("nope\n")


def test_hard_error_yields_no_partial_result():
    # §11.1 — the first record parses fine, but nothing is returned.
    with pytest.raises(synx.SynxlError):
        synx.synxl_parse("!synxl 1\n!fields a\n1\n!bogus\n")


# ─── streaming (§15.1) ──────────────────────────────────────────────────


def test_stream_is_a_real_iterator_of_dicts():
    stream = synx.synxl_stream("!synxl 1\n!fields a ; b\n1 ; 2\n3 ; 4\n")
    assert iter(stream) is stream

    first = next(stream)
    assert type(first) is dict
    assert first == {"a": 1, "b": 2}
    assert next(stream) == {"a": 3, "b": 4}
    with pytest.raises(StopIteration):
        next(stream)


def test_stream_in_a_for_loop_matches_the_document_parse():
    streamed = [record for record in synx.synxl_stream(CHAT)]
    assert streamed == list(synx.synxl_parse(CHAT).records)


def test_stream_records_carry_position_and_diagnostics():
    records = list(synx.synxl_stream_records("!synxl 1\n!fields id[type:int] ; b\nx ; 2\n7 ; 8\n"))
    bad, good = records

    assert bad.index == 0 and bad.line == 3 and bad.field_list == 0
    assert type(bad.values) is dict
    assert bad.values == {"id": None, "b": 2}
    assert kinds(bad.diagnostics) == ["CastFailed"]
    assert bad["b"] == 2 and "id" in bad and len(bad) == 2
    assert sorted(bad.keys()) == ["b", "id"]

    assert good.index == 1 and good.line == 4
    assert good.diagnostics == []
    assert good.to_dict() == {"id": 7, "b": 8}


def test_stream_yields_records_before_a_later_hard_error():
    stream = synx.synxl_stream("!synxl 1\n!fields a\n1\n!bogus\n2\n")
    assert next(stream) == {"a": 1}  # produced without reading the whole input
    with pytest.raises(synx.SynxlError) as excinfo:
        next(stream)
    assert excinfo.value.kind == "UnknownDirective"
    assert excinfo.value.line == 4
    # §11.1 — the document ends there.
    with pytest.raises(StopIteration):
        next(stream)


def test_stream_prologue_error_is_raised_eagerly():
    with pytest.raises(synx.SynxlError) as excinfo:
        synx.synxl_stream("a ; b\n")
    assert excinfo.value.kind == "MissingPrologue"


def test_stream_exposes_version_and_field_lists():
    stream = synx.synxl_stream(CHAT)
    assert stream.version == 1
    next(stream)
    assert [f["name"] for f in stream.field_lists[0]["fields"]] == ["id", "score", "messages"]
    assert stream.trailing_diagnostics == []


def test_stream_file_reads_records_incrementally(tmp_path):
    path = tmp_path / "chat.synxl"
    path.write_text(CHAT, encoding="utf-8")

    stream = synx.synxl_stream_file(str(path))
    assert iter(stream) is stream
    assert stream.version == 1

    records = list(stream)
    assert records == list(synx.synxl_parse(CHAT).records)
    assert [f["name"] for f in stream.field_lists[0]["fields"]] == ["id", "score", "messages"]
    assert stream.trailing_diagnostics == []


def test_stream_file_records_carry_position_and_diagnostics(tmp_path):
    path = tmp_path / "bad.synxl"
    path.write_text("!synxl 1\n!fields id[type:int] ; b\nx ; 2\n7 ; 8\n", encoding="utf-8")

    bad, good = list(synx.synxl_stream_file_records(str(path)))
    assert bad.index == 0 and bad.line == 3
    assert bad.values == {"id": None, "b": 2}
    assert kinds(bad.diagnostics) == ["CastFailed"]
    assert good.values == {"id": 7, "b": 8}


def test_stream_file_validate_option(tmp_path):
    path = tmp_path / "validated.synxl"
    path.write_text(VALIDATED, encoding="utf-8")
    (record,) = list(synx.synxl_stream_file_records(str(path), validate=True))
    assert kinds(record.diagnostics) == ["ConstraintViolation"]
    (record,) = list(synx.synxl_stream_file_records(str(path)))
    assert record.diagnostics == []


def test_stream_file_reports_format_errors_as_synxl_error(tmp_path):
    path = tmp_path / "broken.synxl"
    path.write_text("!synxl 1\n!fields a\n1\n!bogus\n2\n", encoding="utf-8")

    stream = synx.synxl_stream_file(str(path))
    assert next(stream) == {"a": 1}
    with pytest.raises(synx.SynxlError) as excinfo:
        next(stream)
    assert excinfo.value.kind == "UnknownDirective"
    assert excinfo.value.line == 4


def test_stream_file_missing_prologue_raises_synxl_error(tmp_path):
    path = tmp_path / "noprologue.synxl"
    path.write_text("a ; b\n", encoding="utf-8")
    with pytest.raises(synx.SynxlError) as excinfo:
        synx.synxl_stream_file(str(path))
    assert excinfo.value.kind == "MissingPrologue"


def test_stream_file_missing_file_is_an_oserror(tmp_path):
    # A failed read says nothing about the document, so it is never a SynxlError.
    with pytest.raises(OSError) as excinfo:
        synx.synxl_stream_file(str(tmp_path / "nope.synxl"))
    assert not isinstance(excinfo.value, synx.SynxlError)


def test_stream_file_io_error_mid_iteration_is_an_oserror(tmp_path):
    # §3.1 — bytes that are not UTF-8 fail the read, not the parse.
    path = tmp_path / "notutf8.synxl"
    path.write_bytes(b"!synxl 1\n!fields a\n1\n2\n\xff\xfe not utf-8\n")

    stream = synx.synxl_stream_file(str(path))
    # Records whose boundary was decided before the bad byte still arrive.
    assert next(stream) == {"a": 1}
    with pytest.raises(OSError) as excinfo:
        next(stream)
    assert not isinstance(excinfo.value, synx.SynxlError)


def test_stream_validate_option():
    (record,) = list(synx.synxl_stream_records(VALIDATED, validate=True))
    assert kinds(record.diagnostics) == ["ConstraintViolation"]
    (record,) = list(synx.synxl_stream_records(VALIDATED))
    assert record.diagnostics == []


# ─── JSON projections (§12) ─────────────────────────────────────────────


def test_json_array_projection():
    doc = synx.synxl_parse(CHAT)
    text = doc.to_json()
    assert text == synx.synxl_to_json(CHAT)
    # §12.1 — canonical: sorted keys, no insignificant whitespace.
    assert text.startswith('[{"id":1,')
    assert json.loads(text) == list(doc.records)


def test_ndjson_projection():
    doc = synx.synxl_parse(CHAT)
    text = doc.to_ndjson()
    assert text == synx.synxl_to_ndjson(CHAT)
    # §12.2 — one canonical object per line, LF-separated, no enclosing array.
    lines = text.split("\n")
    assert lines[-1] == ""
    objects = [json.loads(line) for line in lines[:-1]]
    assert objects == list(doc.records)
    assert objects == json.loads(doc.to_json())


def test_projection_functions_raise_on_hard_errors():
    for fn in (synx.synxl_to_json, synx.synxl_to_ndjson):
        with pytest.raises(synx.SynxlError):
            fn("!synxl 1\n!fields a ; a\n1 ; 2\n")


# ─── writer (§14) and round-trips (§14.1) ───────────────────────────────


def test_write_synthesizes_a_field_list_from_the_records():
    text = synx.synxl_write([{"id": 1, "name": "Wario"}, {"id": 2, "name": "Waluigi"}])
    assert text.startswith("!synxl 1\n!fields ")
    assert synx.synxl_parse(text).records == [
        {"id": 1, "name": "Wario"},
        {"id": 2, "name": "Waluigi"},
    ]


def test_write_round_trip_with_explicit_fields_and_blocks():
    records = [
        {
            "id": 1,
            "score": 0.5,
            "messages": [{"role": "user", "content": "line one\nline two"}],
        },
        {"id": 2, "score": None, "messages": None},
    ]
    text = synx.synxl_write(
        records,
        fields=[{"name": "id", "type": "int"}, "score", {"name": "messages", "block": True}],
    )
    doc = synx.synxl_parse(text)
    assert doc.diagnostics == []
    assert list(doc.records) == records


def test_write_promotes_values_that_have_no_inline_form():
    # §14.3 — `;` inside a value, and a nested object, force block promotion.
    records = [{"id": 1, "note": "a ; b", "meta": {"k": "v"}}]
    text = synx.synxl_write(records)
    assert synx.synxl_parse(text).records == records


def test_document_round_trips_through_to_synxl():
    doc = synx.synxl_parse(CHAT)
    again = synx.synxl_parse(doc.to_synxl())
    # §14.1 — writing then re-parsing preserves the JSON projection.
    assert again.to_json() == doc.to_json()
    assert again.diagnostics == []


UNWRITABLE = "mix \" and ' and ; here"


def test_write_rejects_a_value_with_no_synxl_rendering():
    # §14.3 — needs quoting, holds both quote characters, and holds a `;`, so it
    # can neither be quoted nor written bare; promoting the only column would
    # leave arity 0 (§5.3.4). The writer must refuse rather than emit a document
    # its own parser rejects.
    with pytest.raises(synx.SynxlError) as excinfo:
        synx.synxl_write([{"a": UNWRITABLE}])
    assert excinfo.value.kind == "Unwritable"
    assert excinfo.value.line == 0
    assert excinfo.value.message


def test_unwritable_value_is_writable_next_to_an_inline_column():
    # The same value round-trips as soon as another column can stay inline.
    records = [{"id": 1, "a": UNWRITABLE}]
    text = synx.synxl_write(records, fields=[{"name": "id", "type": "int"}, "a"])
    assert synx.synxl_parse(text).records == records


def test_write_rejects_block_combined_with_a_type():
    with pytest.raises(synx.SynxlError) as excinfo:
        synx.synxl_write([{"a": 1}], fields=[{"name": "a", "type": "int", "block": True}])
    assert excinfo.value.kind == "BlockWithType"


def test_write_rejects_non_mapping_records():
    with pytest.raises(TypeError):
        synx.synxl_write([[1, 2, 3]])


# ─── file loading and limits ────────────────────────────────────────────


def test_load_reads_a_file(tmp_path):
    path = tmp_path / "chat.synxl"
    path.write_text(CHAT, encoding="utf-8")
    doc = synx.synxl_load(str(path))
    assert doc.to_json() == synx.synxl_parse(CHAT).to_json()


def test_load_missing_file_raises_ioerror(tmp_path):
    with pytest.raises(IOError):
        synx.synxl_load(str(tmp_path / "nope.synxl"))


def test_resource_limits_are_exposed():
    assert synx.MAX_SYNXL_RECORD_BYTES == 16 * 1024 * 1024
    assert synx.MAX_SYNXL_FIELDS == 4096
    assert synx.MAX_SYNXL_FIELD_NAME_BYTES == 255
    assert synx.MAX_SYNXL_FIELD_LISTS == 65536
    assert synx.MAX_SYNXL_RECORDS == 16777216
