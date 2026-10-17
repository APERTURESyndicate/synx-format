@tool
class_name SynxFormatter
extends RefCounted

# Canonical formatter — synx-core `fmt_canonical`:
#   • directive lines (!active/!lock/!tool/!schema/!llm/#!mode:active) at the top
#   • comments stripped
#   • keys sorted at every level by their name, lowercased, by code point;
#     the sort is stable (a repeated key keeps document order, so the last
#     one still wins)
#   • exactly 2 spaces per level; one blank line after a top-level block
#   • a `|` / `|+` body is content: never parsed as keys nor sorted; a `|+`
#     body keeps its indentation relative to its first line
#   • nesting is capped at 128 (a deeper subtree is dropped)

const MAX_FMT_PARSE_DEPTH: int = 128


static func format(text: String) -> String:
	text = SynxParser.clamp_text(text)
	var lines := _lines(text)
	var directives := PackedStringArray()
	var body_start := 0
	for i in lines.size():
		var t := SynxText.trim(lines[i])
		if t == "!active" or t == "!lock" or t == "!tool" or t == "!schema" or t == "!llm" or t == "#!mode:active":
			directives.append(t)
			body_start = i + 1
		elif t.is_empty() or t.begins_with("#") or t.begins_with("//"):
			body_start = i + 1
		else:
			break
	var parsed := _parse(lines, body_start, 0, 0)
	var nodes: Array = parsed[0]
	_sort(nodes)
	var out := PackedStringArray()
	if not directives.is_empty():
		out.append("\n".join(directives))
		out.append("\n\n")
	_emit(nodes, 0, out)
	return SynxText.trim_end("".join(out)) + "\n"


# Rust `str::lines`: split on "\n", drop one trailing "\r" per line, no line
# after a final newline.
static func _lines(text: String) -> PackedStringArray:
	var lines := text.split("\n")
	if not lines.is_empty() and lines[lines.size() - 1].is_empty():
		lines.resize(lines.size() - 1)
	for i in lines.size():
		var l := lines[i]
		if l.ends_with("\r"):
			lines[i] = l.substr(0, l.length() - 1)
	return lines


# UTF-8 bytes of the leading Unicode whitespace.
static func _indent_of(line: String) -> int:
	var w := 0
	for i in line.length():
		var c := line.unicode_at(i)
		if not SynxText.is_ws(c):
			break
		w += SynxText.utf8_width(c)
	return w


# Byte offset → character offset of at most `limit` bytes of leading
# whitespace, ending on a whole character.
static func _ws_prefix_chars(s: String, limit: int) -> int:
	var off := 0
	var i := 0
	while i < s.length():
		var c := s.unicode_at(i)
		if off >= limit or not SynxText.is_ws(c):
			break
		off += SynxText.utf8_width(c)
		i += 1
	return i


# [nodes, next_line]; a node is [header, children, list_items].
static func _parse(lines: PackedStringArray, start: int, base: int, depth: int) -> Array:
	if depth > MAX_FMT_PARSE_DEPTH:
		# Past the cap the whole subtree is dropped: its lines are consumed up
		# to the next one above this level (synx-core `fmt_parse`).
		var j := start
		while j < lines.size():
			if not SynxText.trim(lines[j]).is_empty() and _indent_of(lines[j]) < base:
				break
			j += 1
		return [[], j]
	var nodes: Array = []
	var i := start
	var n := lines.size()
	while i < n:
		var raw := lines[i]
		var t := SynxText.trim(raw)
		if t.is_empty():
			i += 1
			continue
		var ind := _indent_of(raw)
		if ind < base:
			break
		if ind > base:
			i += 1
			continue
		if t.begins_with("- ") or t.begins_with("#") or t.begins_with("//"):
			i += 1
			continue
		var preserve := t.ends_with(" |+") or t == "|+"
		var multiline := preserve or t.ends_with(" |") or t == "|"
		var block_base := -1
		var children: Array = []
		var items := PackedStringArray()
		i += 1
		while i < n:
			var cr := lines[i]
			var ct := SynxText.trim(cr)
			if ct.is_empty():
				i += 1
				continue
			var ci := _indent_of(cr)
			if ci <= base:
				break
			if preserve:
				if block_base < 0:
					block_base = ci
				var body := SynxText.trim_end(cr)
				items.append(body.substr(_ws_prefix_chars(body, mini(block_base, ci))))
				i += 1
			elif multiline or ct.begins_with("- "):
				items.append(ct)
				i += 1
			elif ct.begins_with("#") or ct.begins_with("//"):
				i += 1
			else:
				var sub := _parse(lines, i, ci, depth + 1)
				children.append_array(sub[0])
				i = sub[1]
		nodes.append([t, children, items])
	return [nodes, i]


static func _sort_key(header: String) -> String:
	for i in header.length():
		var c := header.unicode_at(i)
		if c == 91 or c == 58 or c == 40 or SynxText.is_ws(c):
			return header.substr(0, i).to_lower()
	return header.to_lower()


static func _sort(nodes: Array) -> void:
	if nodes.size() > 1:
		var keyed: Array = []
		for i in nodes.size():
			keyed.append([_sort_key(nodes[i][0]), i, nodes[i]])
		keyed.sort_custom(func(a: Array, b: Array) -> bool:
			if a[0] != b[0]:
				return a[0] < b[0]
			return a[1] < b[1])
		for i in keyed.size():
			nodes[i] = keyed[i][2]
	for node in nodes:
		_sort(node[1])


static func _emit(nodes: Array, indent: int, out: PackedStringArray) -> void:
	var sp := " ".repeat(indent)
	var item_sp := " ".repeat(indent + 2)
	for node in nodes:
		out.append(sp + String(node[0]) + "\n")
		var children: Array = node[1]
		if not children.is_empty():
			_emit(children, indent + 2, out)
		var items: PackedStringArray = node[2]
		for li in items:
			out.append(item_sp + li + "\n")
		if indent == 0 and (not children.is_empty() or not items.is_empty()):
			out.append("\n")
