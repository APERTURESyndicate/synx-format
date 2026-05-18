@tool
class_name SynxParser
extends RefCounted

# Static parser for SYNX text → SynxParseResult.
# Behaviour parity targets `synx-core::parser::parse` and `synx-js::parseData`.
#
# This module is intentionally written as a single class with file-static
# helpers so it can be used without instantiating an autoload.

const MAX_SYNX_INPUT_BYTES: int = 16 * 1024 * 1024
const MAX_LINE_STARTS: int = 2_000_000
const MAX_PARSE_NESTING_DEPTH: int = 128
const MAX_MULTILINE_BLOCK_BYTES: int = 1024 * 1024
const MAX_LIST_ITEMS: int = 1_048_576
const MAX_INCLUDE_DIRECTIVES: int = 4096
const MAX_CONSTRAINT_ENUM_PARTS: int = 4096
const MAX_MARKER_CHAIN_SEGMENTS: int = 512

# Stack-entry kinds — mirrors Rust StackEntry enum.
const STACK_ROOT := 0
const STACK_KEY := 1
const STACK_LIST_ITEM := 2


static func parse(text: String) -> SynxParseResult:
	if text.length() > MAX_SYNX_INPUT_BYTES:
		text = text.substr(0, MAX_SYNX_INPUT_BYTES)

	var lines: PackedStringArray = text.split("\n", true)
	if lines.size() > MAX_LINE_STARTS:
		lines.resize(MAX_LINE_STARTS)

	var result := SynxParseResult.new()
	var root: Dictionary = {}
	result.root = SynxValue.make_object(root)

	# Stack frame layout: { "indent": int, "kind": int, "key": String, "list_key": String, "item_idx": int }
	var stack: Array = [_stack_entry(-1, STACK_ROOT, "", "", 0)]
	var mode: int = SynxParseResult.Mode.STATIC
	var locked := false
	var tool_dir := false
	var schema := false
	var llm := false
	var metadata: Dictionary = {}
	var includes: Array = []
	var uses: Array = []

	# Block / list state.
	var block_active := false
	var block_indent := 0
	var block_key := ""
	var block_stack_idx := 0
	var block_content := ""

	var list_active := false
	var list_indent := 0
	var list_key := ""
	var list_stack_idx := 0

	var in_block_comment := false
	var line_count := lines.size()
	var i := 0

	while i < line_count:
		var raw: String = lines[i]
		# strip trailing \r (Windows CRLF)
		if raw.length() > 0 and raw[raw.length() - 1] == "\r":
			raw = raw.substr(0, raw.length() - 1)

		var trimmed: String = raw.strip_edges()

		# ── Directives ──
		if trimmed == "!active":
			mode = SynxParseResult.Mode.ACTIVE
			i += 1
			continue
		if trimmed == "!lock":
			locked = true
			i += 1
			continue
		if trimmed == "!tool":
			tool_dir = true
			i += 1
			continue
		if trimmed == "!schema":
			schema = true
			i += 1
			continue
		if trimmed == "!llm":
			llm = true
			i += 1
			continue
		if trimmed.begins_with("!include "):
			if includes.size() < MAX_INCLUDE_DIRECTIVES:
				var rest := trimmed.substr(9).strip_edges()
				var path_part := ""
				var alias_part := ""
				var ws := _index_of_ws(rest, 0)
				if ws == -1:
					path_part = rest
				else:
					path_part = rest.substr(0, ws)
					alias_part = rest.substr(ws).strip_edges()
				if alias_part == "":
					alias_part = _derive_alias(path_part)
				includes.append(SynxParseResult.IncludeDirective.new(path_part, alias_part))
			i += 1
			continue
		if trimmed.begins_with("!use "):
			var rest_u := trimmed.substr(5).strip_edges()
			if rest_u.begins_with("@"):
				var pkg := ""
				var alias_u := ""
				var as_idx := rest_u.find(" as ")
				if as_idx == -1:
					pkg = rest_u.strip_edges()
					var slash := pkg.rfind("/")
					alias_u = pkg.substr(slash + 1) if slash >= 0 else pkg
				else:
					pkg = rest_u.substr(0, as_idx).strip_edges()
					alias_u = rest_u.substr(as_idx + 4).strip_edges()
				if pkg != "":
					uses.append(SynxParseResult.UseDirective.new(pkg, alias_u))
			i += 1
			continue
		if trimmed.begins_with("#!mode:"):
			var declared := trimmed.substr(7).strip_edges()
			mode = SynxParseResult.Mode.ACTIVE if declared == "active" else SynxParseResult.Mode.STATIC
			i += 1
			continue

		# ── Block comments: ### … ### ──
		if trimmed == "###":
			in_block_comment = not in_block_comment
			i += 1
			continue
		if in_block_comment:
			i += 1
			continue

		# ── Skip empty / comments ──
		if trimmed.is_empty() or trimmed.begins_with("#") or trimmed.begins_with("//"):
			i += 1
			continue

		var indent: int = raw.length() - raw.lstrip(" \t").length()

		# ── Continue multiline block ──
		if block_active:
			if indent > block_indent:
				if block_content.length() < MAX_MULTILINE_BLOCK_BYTES:
					if not block_content.is_empty():
						block_content += "\n"
					var room := MAX_MULTILINE_BLOCK_BYTES - block_content.length()
					if room > 0:
						block_content += trimmed.substr(0, min(trimmed.length(), room))
				i += 1
				continue
			else:
				_insert_at(root, stack, block_stack_idx, block_key, SynxValue.make_string(block_content))
				block_active = false
				block_content = ""

		# ── Continue list items ──
		if trimmed.begins_with("- "):
			if list_active and indent > list_indent:
				# Pop list-item frames at same or deeper indent so successive items don't accumulate.
				while stack.size() > 1:
					var top: Dictionary = stack[stack.size() - 1]
					if int(top["kind"]) == STACK_LIST_ITEM and int(top["indent"]) >= indent:
						stack.pop_back()
					else:
						break

				var val_str := _strip_comment(trimmed.substr(2).strip_edges())

				# Peek next non-empty line — nested?
				var peek := i + 1
				var nested := false
				while peek < line_count:
					var pl: String = lines[peek]
					if pl.length() > 0 and pl[pl.length() - 1] == "\r":
						pl = pl.substr(0, pl.length() - 1)
					var pt := pl.strip_edges()
					if pt.is_empty():
						peek += 1
						continue
					var pi: int = pl.length() - pl.lstrip(" \t").length()
					if pi > indent and not pt.begins_with("- ") and not pt.begins_with("#") and not pt.begins_with("//"):
						nested = true
					break

				var parent_map := _navigate_to_parent(root, stack, list_stack_idx)
				if parent_map != null:
					if not parent_map.has(list_key):
						parent_map[list_key] = SynxValue.make_array([])
					var arr_val: SynxValue = parent_map[list_key]
					if arr_val.kind == SynxValue.Kind.ARRAY:
						var arr: Array = arr_val.data
						if arr.size() < MAX_LIST_ITEMS:
							if nested:
								var item_obj: Dictionary = {}
								var parsed := _parse_line(val_str)
								if parsed != null:
									var val: SynxValue
									if not parsed["type_hint"].is_empty():
										val = _cast_typed(parsed["value"], parsed["type_hint"])
									elif not parsed["value"].is_empty():
										val = _cast(parsed["value"])
									else:
										val = SynxValue.make_object({})
									item_obj[parsed["key"]] = val
								else:
									item_obj["_value"] = _cast(val_str)
								var item_idx := arr.size()
								arr.append(SynxValue.make_object(item_obj))
								if stack.size() < MAX_PARSE_NESTING_DEPTH:
									stack.append(_stack_entry(indent, STACK_LIST_ITEM, "", list_key, item_idx))
							else:
								arr.append(_cast(val_str))
				i += 1
				continue
		else:
			# Close list if non-item line at-or-below list indent.
			if list_active and indent <= list_indent:
				list_active = false
				while stack.size() > 1:
					var top2: Dictionary = stack[stack.size() - 1]
					if int(top2["kind"]) == STACK_LIST_ITEM and int(top2["indent"]) >= indent:
						stack.pop_back()
					else:
						break

		# ── Parse key line ──
		var parsed_line := _parse_line(trimmed)
		if parsed_line == null:
			i += 1
			continue

		# Reject prototype-polluting keys (parity with Rust / JS).
		if parsed_line["key"] == "__proto__" or parsed_line["key"] == "constructor" or parsed_line["key"] == "prototype":
			i += 1
			continue

		# Pop stack to correct parent.
		while stack.size() > 1 and int(stack[stack.size() - 1]["indent"]) >= indent:
			stack.pop_back()
		var parent_idx := stack.size() - 1

		# Capture metadata in active mode.
		if mode == SynxParseResult.Mode.ACTIVE:
			var has_meta_payload := (parsed_line["markers"] as Array).size() > 0 \
				or parsed_line["has_constraints"] \
				or not String(parsed_line["type_hint"]).is_empty()
			if has_meta_payload:
				var path_key := _build_path(stack)
				if not metadata.has(path_key):
					metadata[path_key] = {}
				var meta_map: Dictionary = metadata[path_key]
				var meta := SynxMeta.new()
				for m in parsed_line["markers"]:
					meta.markers.append(String(m))
				for a in parsed_line["marker_args"]:
					meta.args.append(String(a))
				meta.type_hint = parsed_line["type_hint"]
				meta.has_constraints = parsed_line["has_constraints"]
				if meta.has_constraints:
					meta.c_min = parsed_line["c_min"]
					meta.c_max = parsed_line["c_max"]
					meta.c_type_name = parsed_line["c_type_name"]
					meta.c_required = parsed_line["c_required"]
					meta.c_readonly = parsed_line["c_readonly"]
					meta.c_pattern = parsed_line["c_pattern"]
					meta.c_enum = parsed_line["c_enum"]
					meta.c_has_enum = parsed_line["c_has_enum"]
				meta_map[parsed_line["key"]] = meta

		var is_block := String(parsed_line["value"]) == "|"
		var is_list_marker := false
		for m2 in parsed_line["markers"]:
			var s := String(m2)
			if s == "random" or s == "unique" or s == "geo" or s == "join":
				is_list_marker = true
				break

		if is_block:
			_insert_at(root, stack, parent_idx, parsed_line["key"], SynxValue.make_string(""))
			block_active = true
			block_indent = indent
			block_key = parsed_line["key"]
			block_stack_idx = parent_idx
			block_content = ""
		elif is_list_marker and String(parsed_line["value"]).is_empty():
			_insert_at(root, stack, parent_idx, parsed_line["key"], SynxValue.make_array([]))
			list_active = true
			list_indent = indent
			list_key = parsed_line["key"]
			list_stack_idx = parent_idx
		elif String(parsed_line["value"]).is_empty():
			# Peek for a list under this key.
			var peek_idx := i + 1
			while peek_idx < line_count:
				var pl2: String = lines[peek_idx]
				if pl2.length() > 0 and pl2[pl2.length() - 1] == "\r":
					pl2 = pl2.substr(0, pl2.length() - 1)
				var pt2 := pl2.strip_edges()
				if not pt2.is_empty():
					break
				peek_idx += 1

			if peek_idx < line_count:
				var pl3: String = lines[peek_idx]
				if pl3.length() > 0 and pl3[pl3.length() - 1] == "\r":
					pl3 = pl3.substr(0, pl3.length() - 1)
				var pt3 := pl3.strip_edges()
				if pt3.begins_with("- "):
					_insert_at(root, stack, parent_idx, parsed_line["key"], SynxValue.make_array([]))
					list_active = true
					list_indent = indent
					list_key = parsed_line["key"]
					list_stack_idx = parent_idx
					i += 1
					continue

			_insert_at(root, stack, parent_idx, parsed_line["key"], SynxValue.make_object({}))
			if stack.size() < MAX_PARSE_NESTING_DEPTH:
				stack.append(_stack_entry(indent, STACK_KEY, parsed_line["key"], "", 0))
		else:
			var v: SynxValue
			if not String(parsed_line["type_hint"]).is_empty():
				v = _cast_typed(parsed_line["value"], parsed_line["type_hint"])
			else:
				v = _cast(parsed_line["value"])
			_insert_at(root, stack, parent_idx, parsed_line["key"], v)

		i += 1

	# Flush pending block.
	if block_active:
		_insert_at(root, stack, block_stack_idx, block_key, SynxValue.make_string(block_content))

	result.root = SynxValue.make_object(root)
	result.mode = mode
	result.locked = locked
	result.tool_directive = tool_dir
	result.schema = schema
	result.llm = llm
	result.metadata = metadata
	result.includes = includes
	result.uses = uses
	return result


# ─── Tool reshape (deferred from parse, matches reshape_tool_output) ──

static func reshape_tool_output(root: SynxValue, schema_mode: bool) -> SynxValue:
	if root.kind != SynxValue.Kind.OBJECT:
		return root.clone()
	var map: Dictionary = root.data

	if schema_mode:
		var tools_arr: Array = []
		var keys: Array = map.keys()
		keys.sort()
		for key in keys:
			var v: SynxValue = map[key]
			var def: Dictionary = {}
			def["name"] = SynxValue.make_string(String(key))
			def["params"] = v.clone()
			tools_arr.append(SynxValue.make_object(def))
		var out: Dictionary = {}
		out["tools"] = SynxValue.make_array(tools_arr)
		return SynxValue.make_object(out)

	# Call mode.
	if map.is_empty():
		var out_e: Dictionary = {}
		out_e["tool"] = SynxValue.make_null()
		out_e["params"] = SynxValue.make_object({})
		return SynxValue.make_object(out_e)

	var keys_c: Array = map.keys()
	keys_c.sort()
	var tool_key: String = String(keys_c[0])
	var tool_val: SynxValue = map[tool_key]
	var params: SynxValue
	if tool_val.kind == SynxValue.Kind.OBJECT:
		params = tool_val.clone()
	else:
		params = SynxValue.make_object({})
	var out_c: Dictionary = {}
	out_c["tool"] = SynxValue.make_string(tool_key)
	out_c["params"] = params
	return SynxValue.make_object(out_c)


# ─── Stack helpers ──

static func _stack_entry(indent: int, kind: int, key: String, list_key: String, item_idx: int) -> Dictionary:
	return {
		"indent": indent,
		"kind": kind,
		"key": key,
		"list_key": list_key,
		"item_idx": item_idx,
	}


static func _build_path(stack: Array) -> String:
	var parts: Array[String] = []
	for i in range(1, stack.size()):
		var e: Dictionary = stack[i]
		if int(e["kind"]) == STACK_KEY:
			parts.append(String(e["key"]))
	return ".".join(parts)


static func _navigate_to_parent(root: Dictionary, stack: Array, target_idx: int) -> Variant:
	if target_idx == 0:
		return root
	var current: Dictionary = root
	for i in range(1, target_idx + 1):
		var e: Dictionary = stack[i]
		var kind: int = int(e["kind"])
		if kind == STACK_KEY:
			var k: String = String(e["key"])
			if not current.has(k):
				return null
			var child: SynxValue = current[k]
			if child.kind != SynxValue.Kind.OBJECT:
				return null
			current = child.data
		elif kind == STACK_LIST_ITEM:
			var lk: String = String(e["list_key"])
			var idx: int = int(e["item_idx"])
			if not current.has(lk):
				return null
			var arr_v: SynxValue = current[lk]
			if arr_v.kind != SynxValue.Kind.ARRAY:
				return null
			var arr: Array = arr_v.data
			if idx >= arr.size():
				return null
			var item: SynxValue = arr[idx]
			if item.kind != SynxValue.Kind.OBJECT:
				return null
			current = item.data
		else:
			return null
	return current


static func _insert_at(root: Dictionary, stack: Array, parent_idx: int, key: String, value: SynxValue) -> void:
	var target := _navigate_to_parent(root, stack, parent_idx)
	if target != null:
		target[key] = value


# ─── Line parser ──

static func _parse_line(trimmed: String) -> Variant:
	if trimmed.is_empty() or trimmed.begins_with("#") or trimmed.begins_with("//") or trimmed.begins_with("- "):
		return null

	var first := trimmed.unicode_at(0)
	# Reject lines starting with: [ : - # / (   (parity with Rust)
	if first == 91 or first == 58 or first == 45 or first == 35 or first == 47 or first == 40:
		return null

	var len_t := trimmed.length()
	var pos := 0
	# Extract key.
	while pos < len_t:
		var ch := trimmed.unicode_at(pos)
		if ch == 32 or ch == 9 or ch == 91 or ch == 58 or ch == 40:
			break
		pos += 1
	if pos == 0:
		return null
	var key := trimmed.substr(0, pos)

	var type_hint := ""
	# Optional (type).
	if pos < len_t and trimmed.unicode_at(pos) == 40:
		var start := pos + 1
		var close_rel := trimmed.substr(start).find(")")
		if close_rel >= 0:
			type_hint = trimmed.substr(start, close_rel)
			pos = start + close_rel + 1
		else:
			pos += 1

	# Optional [constraints] with balanced-bracket scan.
	var has_constraints := false
	var c_min := NAN
	var c_max := NAN
	var c_type_name := ""
	var c_required := false
	var c_readonly := false
	var c_pattern := ""
	var c_enum: PackedStringArray = PackedStringArray()
	var c_has_enum := false

	if pos < len_t and trimmed.unicode_at(pos) == 91:
		var cstart := pos + 1
		var depth := 1
		var scan := cstart
		while scan < len_t and depth > 0:
			var ch2 := trimmed.unicode_at(scan)
			if ch2 == 91:
				depth += 1
			elif ch2 == 93:
				depth -= 1
				if depth == 0:
					break
			scan += 1
		var constraint_str := ""
		if depth == 0:
			constraint_str = trimmed.substr(cstart, scan - cstart)
			pos = scan + 1
		else:
			var fb := trimmed.substr(cstart).find("]")
			if fb >= 0:
				constraint_str = trimmed.substr(cstart, fb)
				pos = cstart + fb + 1
			else:
				constraint_str = trimmed.substr(cstart)
				pos = len_t

		# Parse constraints.
		has_constraints = true
		var parts := constraint_str.split(",", false)
		for raw_p in parts:
			var p := String(raw_p).strip_edges()
			if p.is_empty():
				continue
			if p == "required":
				c_required = true
			elif p == "readonly":
				c_readonly = true
			else:
				var colon := p.find(":")
				if colon != -1:
					var ckey := p.substr(0, colon).strip_edges()
					var cval := p.substr(colon + 1).strip_edges()
					match ckey:
						"min":
							if cval.is_valid_float():
								c_min = cval.to_float()
						"max":
							if cval.is_valid_float():
								c_max = cval.to_float()
						"type":
							c_type_name = cval
						"pattern":
							c_pattern = cval
						"enum":
							var ev := cval.split("|", false)
							var lim := min(ev.size(), MAX_CONSTRAINT_ENUM_PARTS)
							for j in lim:
								c_enum.append(String(ev[j]))
							c_has_enum = true

	# Optional :markers.
	var markers: Array = []
	var marker_args: Array = []
	if pos < len_t and trimmed.unicode_at(pos) == 58:
		var marker_start := pos + 1
		var marker_end := marker_start
		while marker_end < len_t:
			var ch3 := trimmed.unicode_at(marker_end)
			if ch3 == 32 or ch3 == 9:
				break
			marker_end += 1
		var chain := trimmed.substr(marker_start, marker_end - marker_start)
		var split_chain := chain.split(":", true)
		var lim2 := min(split_chain.size(), MAX_MARKER_CHAIN_SEGMENTS)
		for j in lim2:
			markers.append(String(split_chain[j]))
		pos = marker_end

	# Skip whitespace.
	while pos < len_t:
		var ch4 := trimmed.unicode_at(pos)
		if ch4 != 32 and ch4 != 9:
			break
		pos += 1

	var raw_value := ""
	if pos < len_t:
		raw_value = _strip_comment(trimmed.substr(pos))

	# :random with weights — promote numeric tokens into marker_args.
	if _contains_marker(markers, "random") and not raw_value.is_empty():
		var nums: Array = []
		for tok in raw_value.split(" ", false):
			var s := String(tok).strip_edges()
			if s.is_valid_float():
				nums.append(s)
		if nums.size() > 0:
			marker_args = nums
			raw_value = ""

	# :inherit — promote parent name into marker_args.
	if _contains_marker(markers, "inherit") and not raw_value.is_empty():
		marker_args = [raw_value.strip_edges()]
		raw_value = ""

	return {
		"key": key,
		"type_hint": type_hint,
		"value": raw_value,
		"markers": markers,
		"marker_args": marker_args,
		"has_constraints": has_constraints,
		"c_min": c_min,
		"c_max": c_max,
		"c_type_name": c_type_name,
		"c_required": c_required,
		"c_readonly": c_readonly,
		"c_pattern": c_pattern,
		"c_enum": c_enum,
		"c_has_enum": c_has_enum,
	}


static func _contains_marker(markers: Array, name: String) -> bool:
	for m in markers:
		if String(m) == name:
			return true
	return false


# ─── Value casting ──

static func _cast(val: String) -> SynxValue:
	# Quoted strings preserve literal value (bypass auto-casting).
	if val.length() >= 2:
		var c0 := val.unicode_at(0)
		var cN := val.unicode_at(val.length() - 1)
		if (c0 == 34 and cN == 34) or (c0 == 39 and cN == 39):
			return SynxValue.make_string(val.substr(1, val.length() - 2))

	if val == "true":
		return SynxValue.make_bool(true)
	if val == "false":
		return SynxValue.make_bool(false)
	if val == "null":
		return SynxValue.make_null()

	var len_v := val.length()
	if len_v == 0:
		return SynxValue.make_string("")

	var start := 0
	if val.unicode_at(0) == 45: # '-'
		if len_v == 1:
			return SynxValue.make_string(val)
		start = 1

	var c_first := val.unicode_at(start)
	if c_first >= 48 and c_first <= 57: # '0'-'9'
		var dot_pos := -1
		var all_num := true
		for j in range(start, len_v):
			var ch := val.unicode_at(j)
			if ch == 46:
				if dot_pos != -1:
					all_num = false
					break
				dot_pos = j
			elif ch < 48 or ch > 57:
				all_num = false
				break
		if all_num:
			if dot_pos == -1:
				if val.is_valid_int():
					return SynxValue.make_int(val.to_int())
			elif dot_pos > start and dot_pos < len_v - 1:
				if val.is_valid_float():
					return SynxValue.make_float(val.to_float())

	return SynxValue.make_string(val)


static func _cast_typed(val: String, hint: String) -> SynxValue:
	match hint:
		"int":
			return SynxValue.make_int(val.to_int() if val.is_valid_int() else 0)
		"float":
			return SynxValue.make_float(val.to_float() if val.is_valid_float() else 0.0)
		"bool":
			return SynxValue.make_bool(val.strip_edges() == "true")
		"string":
			return SynxValue.make_string(val)
		"random", "random:int":
			return SynxValue.make_int(SynxRng.random_i64())
		"random:float":
			return SynxValue.make_float(SynxRng.random_f64_01())
		"random:bool":
			return SynxValue.make_bool(SynxRng.random_bool())
		_:
			return _cast(val)


# ─── String helpers ──

static func _strip_comment(val: String) -> String:
	var result := val
	var sl := result.find(" //")
	if sl >= 0:
		result = result.substr(0, sl)
	var hp := result.find(" #")
	if hp >= 0:
		result = result.substr(0, hp)
	return result.rstrip(" \t")


static func _index_of_ws(s: String, start: int) -> int:
	var n := s.length()
	for i in range(start, n):
		var ch := s.unicode_at(i)
		if ch == 32 or ch == 9:
			return i
	return -1


static func _derive_alias(path: String) -> String:
	var slash := max(path.rfind("/"), path.rfind("\\"))
	var name := path.substr(slash + 1) if slash >= 0 else path
	var dot := name.rfind(".")
	if dot >= 0:
		var ext := name.substr(dot).to_lower()
		if ext == ".synx":
			name = name.substr(0, dot)
	return name
