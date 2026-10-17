@tool
class_name SynxStringify
extends RefCounted

# Value → SYNX text that parses back to the same data — synx-core
# `serialize`, which writes through the SYNXL §14.1 block writer
# (`synxl::write_synx_entry`):
#   • strings that would read back as another type or lose a part to a
#     comment are quoted ("007", "true", "x # y" goes to a `|+` body)
#   • multi-line text is a `|+` body, so its relative indentation survives
#   • a list item's object leads with a key whose value fits on the `- ` line,
#     repeated in full below when needed; an empty list is `key:join`
#   • secrets are written as their mask, `[SECRET]`
# Keys are sorted by code point.

const MAX_SERIALIZE_DEPTH: int = 128
const MAX_WRITE_DEPTH: int = 128


static func stringify(value: SynxValue) -> String:
	if value.kind != SynxValue.Kind.OBJECT:
		return format_primitive(value)
	var out := PackedStringArray()
	var keys: Array = (value.data as Dictionary).keys()
	keys.sort()
	for k in keys:
		write_entry(out, k, value.data[k], 0, 0)
	return "".join(out)


# synx-core `format_primitive` (a non-object root).
static func format_primitive(value: SynxValue) -> String:
	match value.kind:
		SynxValue.Kind.STRING:
			return value.data
		SynxValue.Kind.INT:
			return str(value.data)
		SynxValue.Kind.FLOAT:
			var s := SynxNum.display(value.data)
			return s if s.contains(".") else s + ".0"
		SynxValue.Kind.BOOL:
			return "true" if value.data else "false"
		SynxValue.Kind.NULL:
			return "null"
		SynxValue.Kind.ARRAY:
			var parts := PackedStringArray()
			for item in value.data:
				parts.append(format_primitive(item))
			return "[" + ", ".join(parts) + "]"
		SynxValue.Kind.OBJECT:
			return "[Object]"
		SynxValue.Kind.SECRET:
			return "[SECRET]"
	return ""


static func _indent(n: int) -> String:
	return " ".repeat(n)


static func write_entry(out: PackedStringArray, key: String, value: SynxValue, indent: int, depth: int) -> void:
	if depth > MAX_WRITE_DEPTH:
		return
	match value.kind:
		SynxValue.Kind.OBJECT:
			out.append(_indent(indent) + key + "\n")
			var keys: Array = (value.data as Dictionary).keys()
			keys.sort()
			for k in keys:
				write_entry(out, k, value.data[k], indent + 2, depth + 1)
		SynxValue.Kind.ARRAY:
			var items: Array = value.data
			out.append(_indent(indent) + key + (":join\n" if items.is_empty() else "\n"))
			for item in items:
				_write_item(out, item, indent + 2, depth + 1)
		_:
			if value.kind == SynxValue.Kind.STRING and (value.data as String).contains("\n"):
				_write_multiline(out, key, value.data, indent)
				return
			var text: Variant = scalar_token(value)
			if text != null:
				out.append(_indent(indent) + key + " " + String(text) + "\n")
			else:
				var s: String = value.data if value.kind == SynxValue.Kind.STRING or value.kind == SynxValue.Kind.SECRET else ""
				_write_multiline(out, key, s, indent)


static func _write_item(out: PackedStringArray, item: SynxValue, indent: int, depth: int) -> void:
	if depth > MAX_WRITE_DEPTH:
		return
	if item.kind != SynxValue.Kind.OBJECT:
		var text: Variant = scalar_token(item)
		var s := ""
		if text != null:
			s = text
		elif item.kind == SynxValue.Kind.STRING or item.kind == SynxValue.Kind.SECRET:
			s = item.data
		out.append(_indent(indent) + "- " + s + "\n")
		return
	var map: Dictionary = item.data
	var keys: Array = map.keys()
	keys.sort()
	if keys.is_empty():
		out.append(_indent(indent) + "-\n")
		return
	# The `- ` line carries a key whose value fits on it; deeper lines belong
	# to the item. With no such key, or only one key, the head is written
	# again in full below (the last declaration wins).
	var head_safe := -1
	for i in keys.size():
		if _dash_line_safe(map[keys[i]]):
			head_safe = i
			break
	var head := head_safe if head_safe >= 0 else 0
	var repeat_head := head_safe < 0 or keys.size() == 1
	var head_key: String = keys[head]
	var line := _indent(indent) + "- " + head_key
	var ht: Variant = scalar_token(map[head_key])
	if ht != null and not String(ht).is_empty():
		line += " " + String(ht)
	out.append(line + "\n")
	for i in keys.size():
		if i == head and not repeat_head:
			continue
		write_entry(out, keys[i], map[keys[i]], indent + 2, depth + 1)


static func _dash_line_safe(value: SynxValue) -> bool:
	match value.kind:
		SynxValue.Kind.OBJECT:
			return (value.data as Dictionary).is_empty()
		SynxValue.Kind.ARRAY:
			return false
		SynxValue.Kind.STRING:
			var s: String = value.data
			return not s.contains("\n") and scalar_token(value) != null and not s.is_empty()
	return true


static func _write_multiline(out: PackedStringArray, key: String, body: String, indent: int) -> void:
	out.append(_indent(indent) + key + " |+\n")
	var pad := _indent(indent + 2)
	for line in body.split("\n"):
		out.append(pad + line + "\n")


# A scalar as one SYNX value token, or null when no token can carry it.
static func scalar_token(value: SynxValue) -> Variant:
	match value.kind:
		SynxValue.Kind.NULL:
			return "null"
		SynxValue.Kind.BOOL:
			return "true" if value.data else "false"
		SynxValue.Kind.INT:
			return str(value.data)
		SynxValue.Kind.FLOAT:
			var f: float = value.data
			if is_nan(f) or is_inf(f):
				return "null"
			var s := SynxNum.display(f)
			return s if s.contains(".") else s + ".0"
		SynxValue.Kind.SECRET:
			return "[SECRET]"
		SynxValue.Kind.STRING:
			var s2: String = value.data
			if s2.contains("\n") or s2.contains(" #") or s2.contains(" //"):
				return null
			if not _needs_quote(s2):
				return s2
			if not s2.contains("\""):
				return "\"" + s2 + "\""
			if not s2.contains("'"):
				return "'" + s2 + "'"
			return null
	return null


static func _needs_quote(s: String) -> bool:
	if s.is_empty():
		return true
	if SynxText.is_ws(s.unicode_at(0)) or SynxText.is_ws(s.unicode_at(s.length() - 1)):
		return true
	return s == "|" or s == "|+" or _is_quote_wrapped(s) or not _auto_cast_keeps_string(s)


static func _is_quote_wrapped(s: String) -> bool:
	var n := s.length()
	if n < 2:
		return false
	var a := s.unicode_at(0)
	var b := s.unicode_at(n - 1)
	return (a == 34 and b == 34) or (a == 39 and b == 39)


# Does the SYNX auto-cast read `s` back as the string itself? (synx-core
# `auto_cast_keeps_string` — the decision of `SynxParser.cast` without
# building the value.)
static func _auto_cast_keeps_string(s: String) -> bool:
	if _is_quote_wrapped(s):
		return true
	if s == "true" or s == "false" or s == "null":
		return false
	var n := s.length()
	var start := 1 if s.unicode_at(0) == 45 else 0
	if n <= start:
		return true
	var c := s.unicode_at(start)
	if c < 48 or c > 57:
		return true
	var dot := s.find(".")
	if dot < 0:
		return not s.is_valid_int() or SynxNum.parse_i64(s) == null
	if dot <= start or dot >= n - 1:
		return true
	var fc := s.unicode_at(dot + 1)
	return fc < 48 or fc > 57 or not s.substr(start, dot - start).is_valid_int() or not s.substr(dot + 1).is_valid_int()
