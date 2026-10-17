@tool
class_name SynxParser
extends RefCounted

# SYNX text → SynxParseResult, following synx-core `parser::parse_impl` line
# for line (SYNX 3.8.0).
#
# • Indentation is the UTF-8 byte length of the leading Unicode whitespace
#   (SYNX §4: U+00A0 counts 2, U+3000 counts 3); lines are trimmed of Unicode
#   whitespace at both ends, and a line of only whitespace is blank.
# • A `|` / `|+` body is read before any other line rule, so `#`, `###` and
#   `!` lines inside it are content; `|+` keeps indentation relative to its
#   first line, cut by whole characters.
# • A `- ` item closes every frame at or deeper than its indent; a list is
#   not a frame; an object opened between a list key and its first item is
#   not the item's parent.
# • Declarations are stored on the object that holds the key (SynxMeta).
#
# Lines are walked as offsets into the text; only keys and values are copied.

const MAX_SYNX_INPUT_BYTES: int = 16 * 1024 * 1024
const MAX_LINE_STARTS: int = 2_000_000
const MAX_PARSE_NESTING_DEPTH: int = 128
const MAX_MULTILINE_BLOCK_BYTES: int = 1024 * 1024
const MAX_LIST_ITEMS: int = 1_048_576
const MAX_INCLUDE_DIRECTIVES: int = 4096
const MAX_CONSTRAINT_ENUM_PARTS: int = 4096
const MAX_MARKER_CHAIN_SEGMENTS: int = 512

# The last key line `_parse_line` read (perf: fields instead of an Array
# per line). `_pl_hint` and `_pl_constraints` are null when absent.
static var _pl_key: String = ""
static var _pl_hint: Variant = null
static var _pl_value: String = ""
static var _pl_markers: PackedStringArray = PackedStringArray()
static var _pl_args: PackedStringArray = PackedStringArray()
static var _pl_constraints: Variant = null
# Shared empty chain — never mutated.
static var _no_markers: PackedStringArray = PackedStringArray()

# Set by the last `_peek` call: indent and first non-blank offset of the
# next content line, and that line's trimmed end.
static var _peek_indent: int = 0
static var _peek_first: int = -1
static var _peek_end: int = 0


static func parse(text: String) -> SynxParseResult:
	return parse_with(text, true, true)


# `directives` = false and `nondeterministic_hints` = false give the embedded
# parse SYNXL block delegation uses (synx-core `ParserOptions`).
static func parse_with(text: String, directives: bool = true, nondeterministic_hints: bool = true) -> SynxParseResult:
	text = clamp_text(text)
	var n := text.length()

	var result := SynxParseResult.new()
	var root := SynxValue.make_object({})
	var mode: int = SynxParseResult.Mode.STATIC
	var includes: Array = []
	var uses: Array = []

	# Frames: indent, object, and whether the frame is a list item.
	var st_ind := PackedInt64Array([-1])
	var st_obj: Array = [root]
	var st_item := PackedByteArray([0])

	var blk_active := false
	var blk_indent := 0
	var blk_key := ""
	var blk_obj: SynxValue = null
	var blk_preserve := false
	var blk_base := -1
	var blk_parts := PackedStringArray()
	var blk_chars := 0
	var blk_exact := false
	var blk_bytes := 0

	var lst_active := false
	var lst_indent := 0
	var lst_key := ""
	var lst_parent: SynxValue = null

	var in_block_comment := false

	var pos := 0
	while pos <= n:
		var start := pos
		var nl := text.find("\n", pos)
		if nl < 0:
			nl = n
		pos = nl + 1
		var end := nl
		if end > start and text.unicode_at(end - 1) == 13:
			end -= 1

		# Indent (UTF-8 bytes of leading Unicode whitespace) and first content.
		var a := start
		var indent := 0
		while a < end:
			var c := text.unicode_at(a)
			if c == 32 or c == 9 or (c >= 10 and c <= 13):
				indent += 1
			elif c >= 0x85 and SynxText.is_ws(c):
				indent += SynxText.utf8_width(c)
			else:
				break
			a += 1
		if a == end:
			continue
		var te := end
		var lc := text.unicode_at(te - 1)
		if lc <= 32 or lc >= 0x85:
			while SynxText.is_ws(text.unicode_at(te - 1)):
				te -= 1

		# ── Multiline body: before every other line rule ──
		if blk_active:
			if indent > blk_indent:
				var slice: String
				if blk_preserve:
					if blk_base < 0:
						blk_base = indent
					var strip := mini(blk_base, indent)
					var cut := start
					var off := 0
					while cut < te and off < strip:
						var cc := text.unicode_at(cut)
						if not SynxText.is_ws(cc):
							break
						off += SynxText.utf8_width(cc)
						cut += 1
					slice = text.substr(cut, te - cut)
				else:
					slice = text.substr(a, te - a)
				if not blk_exact:
					var sep := 1 if blk_parts.size() > 0 else 0
					if (blk_chars + sep + slice.length()) * 4 < MAX_MULTILINE_BLOCK_BYTES:
						blk_parts.append(slice)
						blk_chars += sep + slice.length()
						continue
					blk_exact = true
					blk_bytes = SynxText.utf8_len("\n".join(blk_parts))
				if blk_bytes < MAX_MULTILINE_BLOCK_BYTES:
					if blk_parts.size() > 0:
						blk_bytes += 1
					var room := MAX_MULTILINE_BLOCK_BYTES - blk_bytes
					var taken := SynxText.utf8_prefix(slice, room) if room > 0 else ""
					blk_parts.append(taken)
					blk_bytes += SynxText.utf8_len(taken)
				continue
			blk_obj.data[blk_key] = SynxValue.make_string("\n".join(blk_parts))
			blk_active = false
			blk_obj = null

		var fc := text.unicode_at(a)

		# ── Block comment toggle ──
		if fc == 35 and te - a == 3 and text.unicode_at(a + 1) == 35 and text.unicode_at(a + 2) == 35:
			in_block_comment = not in_block_comment
			continue
		if in_block_comment:
			continue

		# ── Directives ──
		if fc == 33 or (fc == 35 and te - a >= 7 and text.unicode_at(a + 1) == 33):
			if directives:
				var tr := text.substr(a, te - a)
				if fc == 33:
					if tr == "!active":
						mode = SynxParseResult.Mode.ACTIVE
						continue
					if tr == "!lock":
						result.locked = true
						continue
					if tr == "!tool":
						result.tool_directive = true
						continue
					if tr == "!schema":
						result.schema = true
						continue
					if tr == "!llm":
						result.llm = true
						continue
					if tr.begins_with("!include "):
						if includes.size() < MAX_INCLUDE_DIRECTIVES:
							includes.append(_include_directive(tr))
						continue
					if tr.begins_with("!use "):
						var ud := _use_directive(tr)
						if ud != null:
							uses.append(ud)
						continue
				elif tr.begins_with("#!mode:"):
					var declared := SynxText.trim(tr.substr(7))
					mode = SynxParseResult.Mode.ACTIVE if declared == "active" else SynxParseResult.Mode.STATIC
					continue
			elif fc == 33:
				continue

		# ── Comments ──
		if fc == 35:
			continue
		if fc == 47 and a + 1 < te and text.unicode_at(a + 1) == 47:
			continue

		# ── List items ──
		if fc == 45 and a + 1 < te and text.unicode_at(a + 1) == 32:
			if lst_active and indent > lst_indent:
				# Every frame of the previous item ends here, its own and any
				# object nested inside it.
				while st_ind.size() > 1 and st_ind[st_ind.size() - 1] >= indent:
					_pop(st_ind, st_obj, st_item)
				var live: SynxValue = lst_parent.data.get(lst_key)
				if live == null or live.kind != SynxValue.Kind.ARRAY:
					continue
				var arr: Array = live.data
				if arr.size() >= MAX_LIST_ITEMS:
					continue
				var val_str := _strip_comment(SynxText.trim(text.substr(a + 2, te - a - 2)))
				var nested := false
				if _peek(text, pos):
					var pf := _peek_first
					var pc := text.unicode_at(pf)
					if _peek_indent > indent and pc != 35 \
							and not (pc == 45 and pf + 1 < _peek_end and text.unicode_at(pf + 1) == 32) \
							and not (pc == 47 and pf + 1 < _peek_end and text.unicode_at(pf + 1) == 47):
						nested = true
				if nested:
					var item := SynxValue.make_object({})
					if _parse_line(val_str, 0, val_str.length()):
						var iv := _pl_value
						var ival: SynxValue
						if _pl_hint != null:
							ival = cast_typed(iv, _pl_hint, nondeterministic_hints)
						elif not iv.is_empty():
							ival = cast(iv)
						else:
							ival = SynxValue.make_object({})
						if not _is_reserved(_pl_key):
							item.data[_pl_key] = ival
					else:
						item.data["_value"] = cast(val_str)
					arr.append(item)
					if st_ind.size() < MAX_PARSE_NESTING_DEPTH:
						st_ind.append(indent)
						st_obj.append(item)
						st_item.append(1)
				else:
					arr.append(cast(val_str))
			continue

		# A non-item line at or above the list's indent closes it.
		if lst_active and indent <= lst_indent:
			lst_active = false
			lst_parent = null
			while st_ind.size() > 1 and st_item[st_item.size() - 1] == 1 and st_ind[st_ind.size() - 1] >= indent:
				_pop(st_ind, st_obj, st_item)

		# ── Key line ──
		if not _parse_line(text, a, te):
			continue
		var key := _pl_key
		if _is_reserved(key):
			continue
		while st_ind.size() > 1 and st_ind[st_ind.size() - 1] >= indent:
			_pop(st_ind, st_obj, st_item)
		var parent: SynxValue = st_obj[st_obj.size() - 1]
		var markers := _pl_markers
		var hint: Variant = _pl_hint

		if mode == SynxParseResult.Mode.ACTIVE and (not markers.is_empty() or _pl_constraints != null or hint != null):
			var meta := SynxMeta.new()
			meta.set_markers(markers.duplicate())
			meta.args = _pl_args.duplicate()
			meta.type_hint = hint
			if _pl_constraints != null:
				meta.parse_constraints(_pl_constraints)
			if parent.meta == null:
				parent.meta = {}
			parent.meta[key] = meta

		var value := _pl_value
		if value == "|" or value == "|+":
			parent.data[key] = SynxValue.make_string("")
			blk_active = true
			blk_indent = indent
			blk_key = key
			blk_obj = parent
			blk_preserve = value == "|+"
			blk_base = -1
			blk_parts = PackedStringArray()
			blk_chars = 0
			blk_exact = false
			blk_bytes = 0
		elif value.is_empty() and _has_list_marker(markers):
			parent.data[key] = SynxValue.make_array([])
			lst_active = true
			lst_indent = indent
			lst_key = key
			lst_parent = parent
		elif value.is_empty():
			if _peek(text, pos):
				var qf := _peek_first
				if text.unicode_at(qf) == 45 and qf + 1 < _peek_end and text.unicode_at(qf + 1) == 32:
					parent.data[key] = SynxValue.make_array([])
					lst_active = true
					lst_indent = indent
					lst_key = key
					lst_parent = parent
					continue
			var obj := SynxValue.make_object({})
			parent.data[key] = obj
			if st_ind.size() < MAX_PARSE_NESTING_DEPTH:
				st_ind.append(indent)
				st_obj.append(obj)
				st_item.append(0)
		elif hint != null:
			parent.data[key] = cast_typed(value, hint, nondeterministic_hints)
		else:
			parent.data[key] = cast(value)

	if blk_active:
		blk_obj.data[blk_key] = SynxValue.make_string("\n".join(blk_parts))

	result.root = root
	result.mode = mode
	result.includes = includes
	result.uses = uses
	return result


# ─── Input caps ──

# Drop a leading U+FEFF, cut at 16 MiB of UTF-8 (on a character boundary) and
# at 2 000 000 lines.
static func clamp_text(text: String) -> String:
	if not text.is_empty() and text.unicode_at(0) == 0xFEFF:
		text = text.substr(1)
	if text.length() * 4 > MAX_SYNX_INPUT_BYTES:
		if SynxText.utf8_len(text) > MAX_SYNX_INPUT_BYTES:
			text = SynxText.utf8_prefix(text, MAX_SYNX_INPUT_BYTES)
	if text.length() >= MAX_LINE_STARTS - 1:
		var seen := 0
		var p := 0
		while true:
			var nl := text.find("\n", p)
			if nl < 0:
				break
			if seen >= MAX_LINE_STARTS - 1:
				text = text.substr(0, nl)
				break
			seen += 1
			p = nl + 1
	return text


# ─── Helpers ──

static func _pop(st_ind: PackedInt64Array, st_obj: Array, st_item: PackedByteArray) -> void:
	var top := st_ind.size() - 1
	st_ind.resize(top)
	st_obj.resize(top)
	st_item.resize(top)


static func _is_reserved(key: String) -> bool:
	return key == "__proto__" or key == "constructor" or key == "prototype"


static func _has_list_marker(markers: PackedStringArray) -> bool:
	for m in markers:
		if m == "random" or m == "unique" or m == "geo" or m == "join":
			return true
	return false


# Find the next line at or after `from` that is not blank; sets `_peek_*`.
static func _peek(text: String, from: int) -> bool:
	var n := text.length()
	var p := from
	while p <= n:
		var nl := text.find("\n", p)
		if nl < 0:
			nl = n
		var e := nl
		if e > p and text.unicode_at(e - 1) == 13:
			e -= 1
		var q := p
		var w := 0
		while q < e:
			var c := text.unicode_at(q)
			if c == 32 or c == 9 or (c >= 10 and c <= 13):
				w += 1
			elif c >= 0x85 and SynxText.is_ws(c):
				w += SynxText.utf8_width(c)
			else:
				break
			q += 1
		if q < e:
			var te := e
			while SynxText.is_ws(text.unicode_at(te - 1)):
				te -= 1
			_peek_indent = w
			_peek_first = q
			_peek_end = te
			return true
		p = nl + 1
	return false


static func _include_directive(tr: String) -> SynxParseResult.IncludeDirective:
	var rest := SynxText.trim(tr.substr(9))
	var ws := -1
	for i in rest.length():
		if SynxText.is_ws(rest.unicode_at(i)):
			ws = i
			break
	var path := rest if ws < 0 else rest.substr(0, ws)
	var alias := ""
	if ws >= 0:
		alias = SynxText.trim(rest.substr(ws + 1))
	else:
		var slash := maxi(path.rfind("/"), path.rfind("\\"))
		alias = path.substr(slash + 1) if slash >= 0 else path
		if alias.ends_with(".synx") or alias.ends_with(".SYNX"):
			alias = alias.substr(0, alias.length() - 5)
	return SynxParseResult.IncludeDirective.new(path, alias)


static func _use_directive(tr: String) -> SynxParseResult.UseDirective:
	var rest := SynxText.trim(tr.substr(5))
	if not rest.begins_with("@"):
		return null
	var package := ""
	var alias := ""
	var as_idx := rest.find(" as ")
	if as_idx < 0:
		package = SynxText.trim(rest)
		var slash := package.rfind("/")
		alias = package.substr(slash + 1) if slash >= 0 else package
	else:
		package = SynxText.trim(rest.substr(0, as_idx))
		alias = SynxText.trim(rest.substr(as_idx + 4))
	if package.is_empty():
		return null
	return SynxParseResult.UseDirective.new(package, alias)


# ─── Key line ──

# synx-core `parse_line` over s[a, te) (already trimmed) into the `_pl_*`
# fields. False when the line is not a key line.
static func _parse_line(s: String, a: int, te: int) -> bool:
	if a >= te:
		return false
	var first := s.unicode_at(a)
	if first == 91 or first == 58 or first == 45 or first == 35 or first == 47 or first == 40:
		return false
	var pos := a
	while pos < te:
		var ch := s.unicode_at(pos)
		if ch == 32 or ch == 9 or ch == 91 or ch == 58 or ch == 40:
			break
		pos += 1
	var key := s.substr(a, pos - a)

	# `s` may be the whole document: searches stop at the line end, or a line
	# without `)` would scan the rest of the text.
	var hint: Variant = null
	if pos < te and s.unicode_at(pos) == 40:
		var close := pos + 1
		while close < te and s.unicode_at(close) != 41:
			close += 1
		if close < te:
			hint = s.substr(pos + 1, close - pos - 1)
			pos = close + 1
		else:
			pos += 1

	var constraints: Variant = null
	if pos < te and s.unicode_at(pos) == 91:
		var cstart := pos + 1
		var depth := 1
		var scan := cstart
		while scan < te:
			var ch2 := s.unicode_at(scan)
			if ch2 == 91:
				depth += 1
			elif ch2 == 93:
				depth -= 1
				if depth == 0:
					break
			scan += 1
		if depth == 0:
			constraints = s.substr(cstart, scan - cstart)
			pos = scan + 1
		else:
			var fb := cstart
			while fb < te and s.unicode_at(fb) != 93:
				fb += 1
			if fb < te:
				constraints = s.substr(cstart, fb - cstart)
				pos = fb + 1
			else:
				constraints = s.substr(cstart, te - cstart)
				pos = te

	var markers := _no_markers
	var args := _no_markers
	if pos < te and s.unicode_at(pos) == 58:
		var mstart := pos + 1
		var mend := mstart
		while mend < te:
			var ch3 := s.unicode_at(mend)
			if ch3 == 32 or ch3 == 9:
				break
			mend += 1
		markers = s.substr(mstart, mend - mstart).split(":")
		if markers.size() > MAX_MARKER_CHAIN_SEGMENTS:
			markers.resize(MAX_MARKER_CHAIN_SEGMENTS)
		pos = mend

	while pos < te:
		var ch4 := s.unicode_at(pos)
		if ch4 != 32 and ch4 != 9:
			break
		pos += 1

	var value := _strip_comment(s.substr(pos, te - pos)) if pos < te else ""

	if not markers.is_empty() and not value.is_empty():
		if markers.has("random"):
			var nums := PackedStringArray()
			for tok in _split_ws(value):
				if SynxNum.is_f64_text(tok):
					nums.append(tok)
			if not nums.is_empty():
				args = nums
				value = ""
		if not value.is_empty() and markers.has("inherit"):
			args = PackedStringArray([SynxText.trim(value)])
			value = ""

	_pl_key = key
	_pl_hint = hint
	_pl_value = value
	_pl_markers = markers
	_pl_args = args
	_pl_constraints = constraints
	return true


# Rust `str::split_whitespace`.
static func _split_ws(s: String) -> PackedStringArray:
	var out := PackedStringArray()
	var n := s.length()
	var i := 0
	while i < n:
		while i < n and SynxText.is_ws(s.unicode_at(i)):
			i += 1
		if i >= n:
			break
		var j := i
		while j < n and not SynxText.is_ws(s.unicode_at(j)):
			j += 1
		out.append(s.substr(i, j - i))
		i = j
	return out


# Cut an inline ` //` or ` #` comment, then trailing whitespace.
# perf: the value is a suffix of a trimmed line, so without a cut there is
# no trailing whitespace to remove.
static func _strip_comment(val: String) -> String:
	var cut := val.find(" #")
	var sl := val.find(" //")
	if sl >= 0 and (cut < 0 or sl < cut):
		cut = sl
	if cut < 0:
		return val
	return SynxText.trim_end(val.substr(0, cut))


# ─── Casting ──

# SYNX §8.3 automatic casting (synx-core `cast`).
static func cast(val: String) -> SynxValue:
	var n := val.length()
	if n == 0:
		return SynxValue.make_string("")
	var c := val.unicode_at(0)
	if n >= 2:
		var cn := val.unicode_at(n - 1)
		if (c == 34 and cn == 34) or (c == 39 and cn == 39):
			return SynxValue.make_string(val.substr(1, n - 2))
	if c == 116 and val == "true":
		return SynxValue.make_bool(true)
	if c == 102 and val == "false":
		return SynxValue.make_bool(false)
	if c == 110 and val == "null":
		return SynxValue.make_null()
	var start := 0
	if c == 45:
		if n == 1:
			return SynxValue.make_string(val)
		start = 1
		c = val.unicode_at(1)
	if c >= 48 and c <= 57:
		var dot := val.find(".")
		if dot < 0:
			if val.is_valid_int():
				# perf: up to 18 digits cannot overflow i64.
				if n <= 18:
					return SynxValue.make_int(val.to_int())
				var iv: Variant = SynxNum.parse_i64(val)
				if iv != null:
					return SynxValue.make_int(iv)
		elif dot > start and dot < n - 1:
			var fc := val.unicode_at(dot + 1)
			var ip := val.substr(start, dot - start)
			var fp := val.substr(dot + 1)
			if fc >= 48 and fc <= 57 and ip.is_valid_int() and fp.is_valid_int():
				# perf: Clinger's fast path — at most 15 significant digits
				# and 22 fraction digits are one exact division.
				if n - start <= 16 and fp.length() <= 22:
					var f := float((ip + fp).to_int()) / SynxNum._pow10f()[fp.length()]
					return SynxValue.make_float(-f if start == 1 else f)
				return SynxValue.make_float(SynxNum.parse_f64_valid(val))
	return SynxValue.make_string(val)


# SYNX §8.3 typed casting (synx-core `cast_typed_with`).
static func cast_typed(val: String, hint: String, nondeterministic: bool = true) -> SynxValue:
	match hint:
		"int":
			var iv: Variant = SynxNum.parse_i64(val)
			return SynxValue.make_int(iv if iv != null else 0)
		"float":
			var fv: Variant = SynxNum.parse_f64(val)
			return SynxValue.make_float(fv if fv != null else 0.0)
		"bool":
			return SynxValue.make_bool(SynxText.trim(val) == "true")
		"string":
			return SynxValue.make_string(val)
		"random", "random:int":
			if nondeterministic:
				return SynxValue.make_int(SynxRng.random_i64())
		"random:float":
			if nondeterministic:
				return SynxValue.make_float(SynxRng.random_f64_01())
		"random:bool":
			if nondeterministic:
				return SynxValue.make_bool(SynxRng.random_bool())
	return cast(val)


# Kept for callers of the 3.7 API.
static func _cast(val: String) -> SynxValue:
	return cast(val)


static func _cast_typed(val: String, hint: String) -> SynxValue:
	return cast_typed(val, hint, true)


# ─── Tool reshape (deferred from parse, matches reshape_tool_output) ──

static func reshape_tool_output(root: SynxValue, schema_mode: bool) -> SynxValue:
	if root.kind != SynxValue.Kind.OBJECT:
		return root.clone()
	var map: Dictionary = root.data
	var keys: Array = map.keys()
	keys.sort()

	if schema_mode:
		var tools_arr: Array = []
		for key in keys:
			var v: SynxValue = map[key]
			var def: Dictionary = {}
			def["name"] = SynxValue.make_string(String(key))
			def["params"] = v.clone()
			tools_arr.append(SynxValue.make_object(def))
		var out: Dictionary = {}
		out["tools"] = SynxValue.make_array(tools_arr)
		return SynxValue.make_object(out)

	if map.is_empty():
		var out_e: Dictionary = {}
		out_e["tool"] = SynxValue.make_null()
		out_e["params"] = SynxValue.make_object({})
		return SynxValue.make_object(out_e)

	var tool_key: String = String(keys[0])
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
