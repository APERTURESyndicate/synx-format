@tool
class_name SynxValue
extends RefCounted

# Tagged-union value matching synx-core::Value.
#
# The `kind` integer mirrors the order of variants in Rust:
#   0 NULL    Variant.Type.NIL
#   1 BOOL    Variant.Type.BOOL
#   2 INT     Variant.Type.INT
#   3 FLOAT   Variant.Type.FLOAT
#   4 STRING  Variant.Type.STRING
#   5 ARRAY   Variant.Type.ARRAY  → Array of SynxValue
#   6 OBJECT  Variant.Type.DICT   → Dictionary[String, SynxValue]
#   7 SECRET  Variant.Type.STRING (redacted in to_json output)

enum Kind { NULL, BOOL, INT, FLOAT, STRING, ARRAY, OBJECT, SECRET }

var kind: int = Kind.NULL
var data = null

static func make_null() -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.NULL
	v.data = null
	return v

static func make_bool(b: bool) -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.BOOL
	v.data = b
	return v

static func make_int(n: int) -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.INT
	v.data = n
	return v

static func make_float(f: float) -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.FLOAT
	v.data = f
	return v

static func make_string(s: String) -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.STRING
	v.data = s
	return v

static func make_array(arr: Array) -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.ARRAY
	v.data = arr
	return v

static func make_object(d: Dictionary) -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.OBJECT
	v.data = d
	return v

static func make_secret(s: String) -> SynxValue:
	var v := SynxValue.new()
	v.kind = Kind.SECRET
	v.data = s
	return v

func is_null() -> bool: return kind == Kind.NULL
func is_bool() -> bool: return kind == Kind.BOOL
func is_int() -> bool: return kind == Kind.INT
func is_float() -> bool: return kind == Kind.FLOAT
func is_string() -> bool: return kind == Kind.STRING
func is_array() -> bool: return kind == Kind.ARRAY
func is_object() -> bool: return kind == Kind.OBJECT
func is_secret() -> bool: return kind == Kind.SECRET
func is_number() -> bool: return kind == Kind.INT or kind == Kind.FLOAT

func as_bool() -> Variant:
	return data if kind == Kind.BOOL else null

func as_int() -> Variant:
	return data if kind == Kind.INT else null

func as_float() -> Variant:
	if kind == Kind.FLOAT: return data
	if kind == Kind.INT: return float(data)
	return null

func as_string() -> Variant:
	if kind == Kind.STRING or kind == Kind.SECRET: return data
	return null

func as_secret() -> Variant:
	return data if kind == Kind.SECRET else null

func as_array() -> Variant:
	return data if kind == Kind.ARRAY else null

func as_object() -> Variant:
	return data if kind == Kind.OBJECT else null

func as_number_f64() -> Variant:
	if kind == Kind.INT: return float(data)
	if kind == Kind.FLOAT: return data
	return null

# Whole-tree primitive equality, matching diff::deep_equal in Rust.
func equals(other: SynxValue) -> bool:
	if other == null:
		return false
	if kind != other.kind:
		return false
	match kind:
		Kind.NULL:
			return true
		Kind.BOOL, Kind.INT, Kind.FLOAT, Kind.STRING, Kind.SECRET:
			return data == other.data
		Kind.ARRAY:
			var a: Array = data
			var b: Array = other.data
			if a.size() != b.size():
				return false
			for i in a.size():
				var av: SynxValue = a[i]
				var bv: SynxValue = b[i]
				if not av.equals(bv):
					return false
			return true
		Kind.OBJECT:
			var ma: Dictionary = data
			var mb: Dictionary = other.data
			if ma.size() != mb.size():
				return false
			for k in ma.keys():
				if not mb.has(k):
					return false
				var av: SynxValue = ma[k]
				var bv: SynxValue = mb[k]
				if not av.equals(bv):
					return false
			return true
	return false

# Deep clone — used by markers that fork values.
func clone() -> SynxValue:
	match kind:
		Kind.NULL:
			return SynxValue.make_null()
		Kind.BOOL:
			return SynxValue.make_bool(data)
		Kind.INT:
			return SynxValue.make_int(data)
		Kind.FLOAT:
			return SynxValue.make_float(data)
		Kind.STRING:
			return SynxValue.make_string(data)
		Kind.SECRET:
			return SynxValue.make_secret(data)
		Kind.ARRAY:
			var out: Array = []
			for item in data:
				var v: SynxValue = item
				out.append(v.clone())
			return SynxValue.make_array(out)
		Kind.OBJECT:
			var d: Dictionary = {}
			for k in data.keys():
				var v: SynxValue = data[k]
				d[k] = v.clone()
			return SynxValue.make_object(d)
	return SynxValue.make_null()

# String form for display/format — matches `Display for Value` in Rust.
func _to_string() -> String:
	match kind:
		Kind.NULL:
			return "null"
		Kind.BOOL:
			return "true" if data else "false"
		Kind.INT:
			return str(data)
		Kind.FLOAT:
			var s := str(data)
			return s if "." in s else s + ".0"
		Kind.STRING, Kind.SECRET:
			return data
		Kind.ARRAY:
			var parts: Array[String] = []
			for item in data:
				var v: SynxValue = item
				parts.append(v._to_string())
			return "[" + ", ".join(parts) + "]"
		Kind.OBJECT:
			return "[Object]"
	return ""

# Type name for TYPE_ERR / debug.
func type_name() -> String:
	match kind:
		Kind.NULL: return "null"
		Kind.BOOL: return "bool"
		Kind.INT: return "int"
		Kind.FLOAT: return "float"
		Kind.STRING: return "string"
		Kind.SECRET: return "secret"
		Kind.ARRAY: return "array"
		Kind.OBJECT: return "object"
	return "unknown"

# ── Plain-Variant conversion (Godot-idiomatic API) ──
# Convert any GDScript Variant (int, float, bool, String, Array, Dictionary) to SynxValue.
static func from_variant(v) -> SynxValue:
	if v == null:
		return SynxValue.make_null()
	var t := typeof(v)
	match t:
		TYPE_BOOL:
			return SynxValue.make_bool(v)
		TYPE_INT:
			return SynxValue.make_int(v)
		TYPE_FLOAT:
			return SynxValue.make_float(v)
		TYPE_STRING, TYPE_STRING_NAME:
			return SynxValue.make_string(String(v))
		TYPE_ARRAY:
			var arr: Array = []
			for item in v:
				arr.append(SynxValue.from_variant(item))
			return SynxValue.make_array(arr)
		TYPE_DICTIONARY:
			var d: Dictionary = {}
			for k in v.keys():
				d[String(k)] = SynxValue.from_variant(v[k])
			return SynxValue.make_object(d)
	# Fallback: stringify any other Variant kind.
	return SynxValue.make_string(str(v))

# Unwrap to plain Variant — strings, ints, floats, bools, Arrays, Dictionaries.
# Secrets become a redaction marker, matching JSON output policy.
func to_variant() -> Variant:
	match kind:
		Kind.NULL:
			return null
		Kind.BOOL, Kind.INT, Kind.FLOAT, Kind.STRING:
			return data
		Kind.SECRET:
			return "[SECRET]"
		Kind.ARRAY:
			var out: Array = []
			for item in data:
				var v: SynxValue = item
				out.append(v.to_variant())
			return out
		Kind.OBJECT:
			var d: Dictionary = {}
			for k in data.keys():
				var v: SynxValue = data[k]
				d[k] = v.to_variant()
			return d
	return null
