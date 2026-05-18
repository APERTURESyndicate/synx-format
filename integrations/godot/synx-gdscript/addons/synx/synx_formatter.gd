@tool
class_name SynxFormatter
extends RefCounted

# Canonical formatter: re-emit .synx text with sorted keys and stable spacing.
# Mirrors `fmt_canonical` in synx-core/src/lib.rs.
#
# Rules:
#   • Directive lines (!active/!lock/!tool/!schema/!llm) preserved at the top.
#   • Comments stripped.
#   • Keys at every nesting level sorted alphabetically (case-insensitive,
#     stripping marker/constraint/type-hint suffix when computing sort key).
#   • Exactly 2 spaces per nesting level.
#   • One blank line between top-level blocks.

const MAX_FMT_PARSE_DEPTH: int = 128


static func format(text: String) -> String:
	var lines: PackedStringArray = text.split("\n", true)
	var directives: Array[String] = []
	var body_start := 0
	for i in lines.size():
		var t := String(lines[i]).strip_edges()
		if t == "!active" or t == "!lock" or t == "!tool" or t == "!schema" or t == "!llm" or t == "#!mode:active":
			directives.append(t)
			body_start = i + 1
		elif t.is_empty() or t.begins_with("#") or t.begins_with("//"):
			body_start = i + 1
		else:
			break

	var parsed := _parse(lines, body_start, 0, 0)
	var nodes: Array = parsed["nodes"]
	_sort(nodes)

	var out := ""
	if directives.size() > 0:
		out = "\n".join(directives) + "\n\n"
	var sink: Array[String] = []
	_emit(nodes, 0, sink)
	out += "".join(sink)
	# Trim trailing newlines, append exactly one.
	var trimmed := out.rstrip("\n ")
	return trimmed + "\n"


# Returns { "nodes": Array, "next": int }.
static func _parse(lines: PackedStringArray, start: int, base: int, depth: int) -> Dictionary:
	if depth > MAX_FMT_PARSE_DEPTH:
		return {"nodes": [], "next": start}
	var nodes: Array = []
	var i := start
	var n := lines.size()
	while i < n:
		var raw := String(lines[i])
		var t := raw.strip_edges()
		if t.is_empty():
			i += 1
			continue
		var ind: int = raw.length() - raw.lstrip(" \t").length()
		if ind < base:
			break
		if ind > base:
			i += 1
			continue
		if t.begins_with("- ") or t.begins_with("#") or t.begins_with("//"):
			i += 1
			continue
		var is_multiline := t.ends_with(" |") or t == "|"
		var node := {
			"header": t,
			"children": [],
			"list_items": [],
			"is_multiline": is_multiline,
		}
		i += 1
		while i < n:
			var cr := String(lines[i])
			var ct := cr.strip_edges()
			if ct.is_empty():
				i += 1
				continue
			var ci: int = cr.length() - cr.lstrip(" \t").length()
			if ci <= base:
				break
			if node["is_multiline"] or ct.begins_with("- "):
				(node["list_items"] as Array).append(ct)
				i += 1
			elif ct.begins_with("#") or ct.begins_with("//"):
				i += 1
			else:
				var sub := _parse(lines, i, ci, depth + 1)
				(node["children"] as Array).append_array(sub["nodes"])
				i = sub["next"]
		nodes.append(node)
	return {"nodes": nodes, "next": i}


static func _sort_key(header: String) -> String:
	# Take everything before the first whitespace/[/:/( character — matches Rust.
	var n := header.length()
	for i in n:
		var ch := header.unicode_at(i)
		if ch == 32 or ch == 9 or ch == 91 or ch == 58 or ch == 40:
			return header.substr(0, i).to_lower()
	return header.to_lower()


static func _sort(nodes: Array) -> void:
	nodes.sort_custom(func(a, b): return SynxFormatter._sort_key(a["header"]) < SynxFormatter._sort_key(b["header"]))
	for node in nodes:
		SynxFormatter._sort(node["children"])


static func _emit(nodes: Array, indent: int, sink: Array[String]) -> void:
	var sp := " ".repeat(indent)
	var item_sp := " ".repeat(indent + 2)
	for n in nodes:
		sink.append(sp)
		sink.append(n["header"])
		sink.append("\n")
		var children: Array = n["children"]
		if children.size() > 0:
			_emit(children, indent + 2, sink)
		var items: Array = n["list_items"]
		for li in items:
			sink.append(item_sp)
			sink.append(String(li))
			sink.append("\n")
		if indent == 0 and (children.size() > 0 or items.size() > 0):
			sink.append("\n")
