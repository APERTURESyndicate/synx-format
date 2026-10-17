@tool
class_name SynxJson
extends RefCounted

# Canonical JSON, byte for byte as synx-core `to_json` writes it:
#   • keys sorted by code point at every level, no whitespace
#   • strings escape " \ \n \r \t and every other control character < 0x20
#     as \u00xx; nothing else is escaped
#   • floats in the ryu layout ("1.0", "0.001", "1e21"); NaN/±inf → null
#   • secrets → "[SECRET]"; deeper than MAX_JSON_DEPTH → null

const MAX_JSON_DEPTH: int = 128

static var _rx_escape: RegEx = null
static var _key_cache: Dictionary = {}


static func to_json(value: SynxValue) -> String:
	var sink := PackedStringArray()
	_emit(value, 0, sink)
	return "".join(sink)


static func _emit(value: SynxValue, depth: int, sink: PackedStringArray) -> void:
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
			sink.append(format_float(value.data))
		SynxValue.Kind.STRING:
			sink.append(quote(value.data))
		SynxValue.Kind.SECRET:
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
				# perf: keys repeat across objects; their JSON form is cached.
				var qk: Variant = _key_cache.get(k)
				if qk == null:
					qk = quote(k)
					if _key_cache.size() >= 8192:
						_key_cache.clear()
					_key_cache[k] = qk
				sink.append(qk)
				sink.append(":")
				_emit(map[k], depth + 1, sink)
			sink.append("}")


static func quote(s: String) -> String:
	if _rx_escape == null:
		_rx_escape = RegEx.new()
		_rx_escape.compile("[\"\\\\\\x00-\\x1f]")
	if _rx_escape.search(s) == null:
		return "\"" + s + "\""
	var out := PackedStringArray()
	out.append("\"")
	var n := s.length()
	var run := 0
	for i in n:
		var ch := s.unicode_at(i)
		var esc := ""
		if ch == 34:
			esc = "\\\""
		elif ch == 92:
			esc = "\\\\"
		elif ch == 10:
			esc = "\\n"
		elif ch == 13:
			esc = "\\r"
		elif ch == 9:
			esc = "\\t"
		elif ch < 0x20:
			esc = "\\u00%02x" % ch
		else:
			continue
		if i > run:
			out.append(s.substr(run, i - run))
		out.append(esc)
		run = i + 1
	if run < n:
		out.append(s.substr(run))
	out.append("\"")
	return "".join(out)


# Kept for callers of the 3.7 API.
static func _quote(s: String) -> String:
	return quote(s)


static func format_float(f: float) -> String:
	if is_nan(f) or is_inf(f):
		return "null"
	return SynxNum.ryu(f)


static func _format_float(f: float) -> String:
	return format_float(f)
