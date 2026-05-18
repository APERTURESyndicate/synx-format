@tool
class_name SynxStringify
extends RefCounted

# Value → SYNX text. Mirrors `serialize` in synx-core/src/lib.rs.

const MAX_SERIALIZE_DEPTH: int = 128


static func stringify(value: SynxValue) -> String:
	var sink: Array[String] = []
	_serialize(value, 0, sink)
	return "".join(sink)


static func _serialize(value: SynxValue, depth: int, sink: Array[String]) -> void:
	if depth > MAX_SERIALIZE_DEPTH:
		sink.append("[synx:max-depth]\n")
		return
	var indent := depth * 2
	var spaces := " ".repeat(indent)
	match value.kind:
		SynxValue.Kind.OBJECT:
			var map: Dictionary = value.data
			var keys: Array = map.keys()
			keys.sort()
			for key in keys:
				var k := String(key)
				var val: SynxValue = map[k]
				match val.kind:
					SynxValue.Kind.ARRAY:
						sink.append(spaces)
						sink.append(k)
						sink.append("\n")
						var arr: Array = val.data
						for item in arr:
							var it: SynxValue = item
							if it.kind == SynxValue.Kind.OBJECT:
								var inner: Dictionary = it.data
								var entries: Array = inner.keys()
								if entries.size() > 0:
									var ek: String = String(entries[0])
									var ev: SynxValue = inner[ek]
									sink.append(spaces)
									sink.append("  - ")
									sink.append(ek)
									sink.append(" ")
									sink.append(_format_primitive(ev))
									sink.append("\n")
									for j in range(1, entries.size()):
										var k2: String = String(entries[j])
										var v2: SynxValue = inner[k2]
										sink.append(spaces)
										sink.append("    ")
										sink.append(k2)
										sink.append(" ")
										sink.append(_format_primitive(v2))
										sink.append("\n")
							else:
								sink.append(spaces)
								sink.append("  - ")
								sink.append(_format_primitive(it))
								sink.append("\n")
					SynxValue.Kind.OBJECT:
						sink.append(spaces)
						sink.append(k)
						sink.append("\n")
						_serialize(val, depth + 1, sink)
					SynxValue.Kind.STRING:
						var s: String = val.data
						if "\n" in s:
							sink.append(spaces)
							sink.append(k)
							sink.append(" |\n")
							for line in s.split("\n"):
								sink.append(spaces)
								sink.append("  ")
								sink.append(String(line))
								sink.append("\n")
						else:
							sink.append(spaces)
							sink.append(k)
							sink.append(" ")
							sink.append(_format_primitive(val))
							sink.append("\n")
					_:
						sink.append(spaces)
						sink.append(k)
						sink.append(" ")
						sink.append(_format_primitive(val))
						sink.append("\n")
		_:
			sink.append(_format_primitive(value))


static func _format_primitive(value: SynxValue) -> String:
	match value.kind:
		SynxValue.Kind.STRING:
			return value.data
		SynxValue.Kind.INT:
			return str(value.data)
		SynxValue.Kind.FLOAT:
			var s := str(value.data)
			return s if "." in s else s + ".0"
		SynxValue.Kind.BOOL:
			return "true" if value.data else "false"
		SynxValue.Kind.NULL:
			return "null"
		SynxValue.Kind.ARRAY:
			var arr: Array = value.data
			var parts: Array[String] = []
			for item in arr:
				var v: SynxValue = item
				parts.append(_format_primitive(v))
			return "[" + ", ".join(parts) + "]"
		SynxValue.Kind.OBJECT:
			return "[Object]"
		SynxValue.Kind.SECRET:
			return "[SECRET]"
	return ""
