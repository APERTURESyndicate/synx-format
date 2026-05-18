@tool
class_name SynxMeta
extends RefCounted

# Per-key metadata captured at parse time and consumed by the engine.
# Mirrors synx-core::value::Meta + Constraints.

var markers: PackedStringArray = PackedStringArray()
var args: PackedStringArray = PackedStringArray()
var type_hint: String = ""
var has_constraints: bool = false

# Constraints (parsed from `[min:3, max:30, required, type:int, pattern:^x$, enum:a|b]`).
# `min`/`max` are NaN when unset (GDScript has no native nullable float).
var c_min: float = NAN
var c_max: float = NAN
var c_type_name: String = ""
var c_required: bool = false
var c_readonly: bool = false
var c_pattern: String = ""
var c_enum: PackedStringArray = PackedStringArray()
var c_has_enum: bool = false

func clone() -> SynxMeta:
	var m := SynxMeta.new()
	m.markers = markers.duplicate()
	m.args = args.duplicate()
	m.type_hint = type_hint
	m.has_constraints = has_constraints
	m.c_min = c_min
	m.c_max = c_max
	m.c_type_name = c_type_name
	m.c_required = c_required
	m.c_readonly = c_readonly
	m.c_pattern = c_pattern
	m.c_enum = c_enum.duplicate()
	m.c_has_enum = c_has_enum
	return m

func has_marker(name: String) -> bool:
	for m in markers:
		if m == name:
			return true
	return false

func marker_index(name: String) -> int:
	for i in markers.size():
		if markers[i] == name:
			return i
	return -1
