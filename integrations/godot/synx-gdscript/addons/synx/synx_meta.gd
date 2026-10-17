@tool
class_name SynxMeta
extends RefCounted

# The declarations of one key: markers, promoted args, `(type)` hint and
# `[constraints]`. Mirrors synx-core `Meta` + `Constraints`.
#
# Declarations live on the object that holds the key (`SynxValue.meta`, a
# Dictionary key → SynxMeta), so every list item has its own, a dotted key
# `a.b` never shares with `b` inside `a`, and a key that replaces an object
# drops the old subtree's declarations with it. `SynxParseResult.metadata`
# shows the same data as synx-core's dot-path map.

# Marker classification bits (synx-core `engine::mk`).
const MK_ALIAS := 1 << 0
const MK_CALC := 1 << 1
const MK_CLAMP := 1 << 2
const MK_DEFAULT := 1 << 3
const MK_ENV := 1 << 4
const MK_FALLBACK := 1 << 5
const MK_FORMAT := 1 << 6
const MK_GEO := 1 << 7
const MK_I18N := 1 << 8
const MK_IMPORT := 1 << 9
const MK_INCLUDE := 1 << 10
const MK_JOIN := 1 << 11
const MK_MAP := 1 << 12
const MK_ONCE := 1 << 13
const MK_PROMPT := 1 << 14
const MK_RANDOM := 1 << 15
const MK_REF := 1 << 16
const MK_REPLACE := 1 << 17
const MK_ROUND := 1 << 18
const MK_SECRET := 1 << 19
const MK_SORT := 1 << 20
const MK_SPAM := 1 << 21
const MK_SPLIT := 1 << 22
const MK_SUM := 1 << 23
const MK_UNIQUE := 1 << 24
const MK_VERSION := 1 << 25
const MK_WATCH := 1 << 26
const MK_INHERIT := 1 << 27

const _BIT_OF := {
	"alias": MK_ALIAS, "calc": MK_CALC, "clamp": MK_CLAMP, "default": MK_DEFAULT,
	"env": MK_ENV, "fallback": MK_FALLBACK, "format": MK_FORMAT, "geo": MK_GEO,
	"i18n": MK_I18N, "import": MK_IMPORT, "include": MK_INCLUDE, "join": MK_JOIN,
	"map": MK_MAP, "once": MK_ONCE, "prompt": MK_PROMPT, "random": MK_RANDOM,
	"ref": MK_REF, "replace": MK_REPLACE, "round": MK_ROUND, "secret": MK_SECRET,
	"sort": MK_SORT, "spam": MK_SPAM, "split": MK_SPLIT, "sum": MK_SUM,
	"unique": MK_UNIQUE, "version": MK_VERSION, "watch": MK_WATCH, "inherit": MK_INHERIT,
}

var markers: PackedStringArray = PackedStringArray()
var args: PackedStringArray = PackedStringArray()
# `(type)` hint: a String, or null when the key line had none.
var type_hint: Variant = null
# Bits of the built-in markers in `markers` (one pass, at parse time).
var bits: int = 0

# Constraints — `has_constraints` is false when the key line had no `[...]`.
# Every optional field is null when unset, as Rust's `Option`.
var has_constraints: bool = false
var c_min: Variant = null
var c_max: Variant = null
var c_type_name: Variant = null
var c_required: bool = false
var c_readonly: bool = false
var c_pattern: Variant = null
var c_enum: Variant = null


static func classify(chain: PackedStringArray) -> int:
	var b := 0
	for m in chain:
		b |= int(_BIT_OF.get(m, 0))
	return b


func set_markers(chain: PackedStringArray) -> void:
	markers = chain
	bits = classify(chain)


func has_marker(name: String) -> bool:
	return markers.has(name)


func marker_index(name: String) -> int:
	return markers.find(name)


func clone() -> SynxMeta:
	var m := SynxMeta.new()
	m.markers = markers.duplicate()
	m.args = args.duplicate()
	m.type_hint = type_hint
	m.bits = bits
	copy_constraints_to(m)
	return m


func copy_constraints_to(m: SynxMeta) -> void:
	m.has_constraints = has_constraints
	m.c_min = c_min
	m.c_max = c_max
	m.c_type_name = c_type_name
	m.c_required = c_required
	m.c_readonly = c_readonly
	m.c_pattern = c_pattern
	m.c_enum = (c_enum as PackedStringArray).duplicate() if c_enum != null else null


# `[a, b:c, ...]` exactly as synx-core `parse_constraints` reads it.
func parse_constraints(raw: String) -> void:
	has_constraints = true
	for part_raw in raw.split(","):
		var part := SynxText.trim(part_raw)
		if part.is_empty():
			continue
		if part == "required":
			c_required = true
		elif part == "readonly":
			c_readonly = true
		else:
			var colon := part.find(":")
			if colon < 0:
				continue
			var key := SynxText.trim(part.substr(0, colon))
			var val := SynxText.trim(part.substr(colon + 1))
			match key:
				"min":
					c_min = SynxNum.parse_f64(val)
				"max":
					c_max = SynxNum.parse_f64(val)
				"type":
					c_type_name = val
				"pattern":
					c_pattern = val
				"enum":
					var ev := val.split("|")
					if ev.size() > SynxParser.MAX_CONSTRAINT_ENUM_PARTS:
						ev.resize(SynxParser.MAX_CONSTRAINT_ENUM_PARTS)
					c_enum = ev


# synx-core `merge_constraints`: stricter bounds win; the first type,
# pattern and enum are kept.
func merge_constraints(incoming: SynxMeta) -> void:
	if incoming.c_required:
		c_required = true
	if incoming.c_readonly:
		c_readonly = true
	if incoming.c_min != null:
		c_min = maxf(float(c_min), float(incoming.c_min)) if c_min != null else incoming.c_min
	if incoming.c_max != null:
		c_max = minf(float(c_max), float(incoming.c_max)) if c_max != null else incoming.c_max
	if c_type_name == null:
		c_type_name = incoming.c_type_name
	if c_pattern == null:
		c_pattern = incoming.c_pattern
	if c_enum == null and incoming.c_enum != null:
		c_enum = (incoming.c_enum as PackedStringArray).duplicate()


# ─── Dot-path view (synx-core `ParseResult::metadata`) ──
#
# A path names an object by its keys from the root, joined with `.`; a `.`
# or `\` inside a key is escaped with `\`; a list item is `list.#<index>`.

static func key_segment(key: String) -> String:
	if not key.contains(".") and not key.contains("\\"):
		return key
	return key.replace("\\", "\\\\").replace(".", "\\.")


static func path_segments(path: String) -> PackedStringArray:
	var out := PackedStringArray()
	if path.is_empty():
		return out
	var seg := ""
	var i := 0
	var n := path.length()
	while i < n:
		var c := path.unicode_at(i)
		if c == 92:
			if i + 1 < n:
				seg += path.substr(i + 1, 1)
			i += 2
			continue
		if c == 46:
			out.append(seg)
			seg = ""
		else:
			seg += path.substr(i, 1)
		i += 1
	out.append(seg)
	return out


# Every object's declarations as { path: { key: SynxMeta } }.
static func collect_paths(root: SynxValue) -> Dictionary:
	var out := {}
	_collect(root, "", out, 0)
	return out


static func _collect(v: SynxValue, path: String, out: Dictionary, depth: int) -> void:
	if v.kind != SynxValue.Kind.OBJECT or depth > 512:
		return
	if v.meta != null and not (v.meta as Dictionary).is_empty():
		out[path] = (v.meta as Dictionary).duplicate()
	var map: Dictionary = v.data
	for key in map:
		var child: SynxValue = map[key]
		if child.kind == SynxValue.Kind.OBJECT:
			_collect(child, key_segment(key) if path.is_empty() else path + "." + key_segment(key), out, depth + 1)
		elif child.kind == SynxValue.Kind.ARRAY:
			var base: String = key_segment(key) if path.is_empty() else path + "." + key_segment(key)
			var arr: Array = child.data
			for i in arr.size():
				var item: SynxValue = arr[i]
				if item.kind == SynxValue.Kind.OBJECT:
					_collect(item, base + ".#" + str(i), out, depth + 1)


# The object a dot-path names, or null.
static func resolve_path(root: SynxValue, path: String) -> SynxValue:
	var cur := root
	for seg in path_segments(path):
		if cur.kind == SynxValue.Kind.OBJECT and (cur.data as Dictionary).has(seg):
			cur = cur.data[seg]
		elif cur.kind == SynxValue.Kind.ARRAY and seg.begins_with("#") and seg.substr(1).is_valid_int():
			var idx := seg.substr(1).to_int()
			var arr: Array = cur.data
			if idx < 0 or idx >= arr.size():
				return null
			cur = arr[idx]
		else:
			return null
	return cur if cur.kind == SynxValue.Kind.OBJECT else null


# Attach a dot-path map to the objects of `root` (paths that name no object
# are dropped).
static func attach_paths(root: SynxValue, metadata: Dictionary) -> void:
	for path in metadata:
		var target := resolve_path(root, String(path))
		if target != null:
			target.meta = (metadata[path] as Dictionary).duplicate()
