@tool
class_name SynxJson
extends RefCounted

# Canonical JSON emitter — output parity with synx-core::to_json:
#   • keys at every object level sorted lexicographically
#   • strings escape: " \ \n \r \t and other < 0x20 → \uXXXX
#   • secrets → "[SECRET]" literal
#   • floats use Godot's str() (shortest round-trip)
#   • depth capped at MAX_JSON_DEPTH; deeper subtrees become null
#
# Output is intentionally compact (no spaces) — matches Rust output.

const MAX_JSON_DEPTH: int = 128


static func to_json(value: SynxValue) -> String:
	var out := ""
	_write(value, 0, _StringBuf.new(out)) if false else null
	# GDScript can't pass strings by reference cleanly, so build via array join.
	var sink: Array[String] = []
	_emit(value, 0, sink)
	return "".join(sink)


static func _emit(value: SynxValue, depth: int, sink: Array[String]) -> void:
	if depth > MAX_JSON_DEPTH:
		sink.append("null")
		return
	match value.kind:
		SynxValue.Kind.NULL:
			sink.append("null")
		SynxValue.Kind.BOOL:
			sink.append("true" if value.data else "false")
		SynxValue.Kind.INT:
			sink.append(str(value.data))
		SynxValue.Kind.FLOAT:
			sink.append(_format_float(value.data))
		SynxValue.Kind.STRING:
			sink.append(_quote(value.data))
		SynxValue.Kind.SECRET:
			# Secrets are redacted in JSON — never expose the underlying string.
			sink.append("\"[SECRET]\"")
		SynxValue.Kind.ARRAY:
			sink.append("[")
			var arr: Array = value.data
			var first := true
			for item in arr:
				if not first:
					sink.append(",")
				first = false
				_emit(item, depth + 1, sink)
			sink.append("]")
		SynxValue.Kind.OBJECT:
			sink.append("{")
			var map: Dictionary = value.data
			var keys: Array = map.keys()
			keys.sort()
			var first2 := true
			for k in keys:
				if not first2:
					sink.append(",")
				first2 = false
				sink.append(_quote(String(k)))
				sink.append(":")
				_emit(map[k], depth + 1, sink)
			sink.append("}")


static func _quote(s: String) -> String:
	var out := "\""
	var n := s.length()
	for i in n:
		var ch := s.unicode_at(i)
		if ch == 34:
			out += "\\\""
		elif ch == 92:
			out += "\\\\"
		elif ch == 10:
			out += "\\n"
		elif ch == 13:
			out += "\\r"
		elif ch == 9:
			out += "\\t"
		elif ch < 0x20:
			out += "\\u%04x" % ch
		else:
			out += s.substr(i, 1)
	out += "\""
	return out


static func _format_float(f: float) -> String:
	# Reproduce Rust ryu-like shortest representation: "1.5" not "1.50000",
	# and "1.0" not "1" when the value is integral.
	if f != f: # NaN — JSON has no NaN; emit null.
		return "null"
	if f == INF:
		return "1e999"
	if f == -INF:
		return "-1e999"
	var s := str(f)
	# Godot prints "1" for integral floats; canonical JSON for SYNX keeps "1.0".
	if not ("." in s) and not ("e" in s) and not ("E" in s):
		s += ".0"
	return s


# Reserved for a future streaming version — currently unused.
class _StringBuf:
	var s: String
	func _init(initial: String = "") -> void:
		s = initial
