@tool
class_name SynxEngine
extends RefCounted

# Active-mode engine — resolves SYNX markers in a parsed tree, following
# synx-core `engine::resolve` (SYNX 3.8.0):
#
#   1. `!use` packages and `!include` files are loaded; root keys with
#      `:include` / `:import` are registered as include aliases.
#   2. Type and constraint registries are collected from the live tree —
#      each object's own declarations, then its children in document order.
#   3. `:inherit` merges parents left to right, each field keeping the
#      declaration of the object its value came from; `_` keys are removed.
#   4. Literal `:secret` values are masked.
#   5. One pass per object in document order: for each key, its child object
#      (or each object item of its list) first, then the key's own markers;
#      after all keys, `{…}` interpolation. A marker sees every key above it
#      resolved, at any depth.
#   6. Every field is checked against the registries.
#
# Marker dispatch lives in synx_engine_markers.gd.

const MAX_CALC_EXPR_LEN: int = 4096
const MAX_CALC_RESOLVED_LEN: int = 64 * 1024
const MAX_FILE_SIZE: int = 10 * 1024 * 1024
const DEFAULT_MAX_INCLUDE_DEPTH: int = 16
const MAX_RESOLVE_DEPTH: int = 512
const MAX_ENGINE_SCRATCH_STRING: int = 4 * 1024 * 1024

# Markers whose value must be resolved before it becomes a secret.
const _RESOLVING_BEFORE_SECRET: int = SynxMeta.MK_ENV | SynxMeta.MK_INCLUDE | SynxMeta.MK_IMPORT \
	| SynxMeta.MK_WATCH | SynxMeta.MK_REF | SynxMeta.MK_ALIAS | SynxMeta.MK_CALC | SynxMeta.MK_MAP \
	| SynxMeta.MK_I18N | SynxMeta.MK_DEFAULT | SynxMeta.MK_FALLBACK | SynxMeta.MK_ONCE \
	| SynxMeta.MK_SPLIT | SynxMeta.MK_JOIN | SynxMeta.MK_FORMAT | SynxMeta.MK_REPLACE

# Spam-rate buckets shared across all engine calls ("key::target" → Array of ms).
static var _spam_buckets: Dictionary = {}


static func resolve(result: SynxParseResult, options: SynxOptions) -> void:
	if result.mode != SynxParseResult.Mode.ACTIVE:
		return
	if options == null:
		options = SynxOptions.new()
	var root := result.root

	var packages_map := _load_packages(result.uses, options)
	var includes_map := _load_includes(result.includes, options)

	if root.kind == SynxValue.Kind.OBJECT and root.meta != null:
		var root_meta: Dictionary = root.meta
		for key in root.data:
			var meta: SynxMeta = root_meta.get(key)
			if meta == null or (meta.bits & (SynxMeta.MK_INCLUDE | SynxMeta.MK_IMPORT)) == 0:
				continue
			var v: SynxValue = root.data[key]
			if v.kind != SynxValue.Kind.STRING:
				continue
			var loaded: Variant = _load_synx_file(String(v.data), options)
			if loaded != null and not includes_map.has(key):
				includes_map[key] = loaded

	if root.kind == SynxValue.Kind.OBJECT:
		for alias in packages_map:
			if not (root.data as Dictionary).has(alias):
				root.data[alias] = (packages_map[alias] as SynxValue).clone()

	var types: Dictionary = {}
	var constraints: Dictionary = {}
	_collect_registries(root, types, constraints, 0)

	_apply_inheritance(root)
	if root.kind == SynxValue.Kind.OBJECT:
		var private: Array = []
		for k in root.data:
			if String(k).begins_with("_"):
				private.append(k)
		for k in private:
			(root.data as Dictionary).erase(k)

	_mask_secret_literals(root, 0)

	# Root keys a single-token `:calc` lookup cannot name (fixed from here:
	# resolution replaces values, it never adds or removes keys).
	var calc_special := PackedStringArray()
	if root.kind == SynxValue.Kind.OBJECT:
		for k in root.data:
			if is_calc_special_key(k):
				calc_special.append(k)

	_resolve_value(root, root, options, "", includes_map, calc_special, 0)

	if not constraints.is_empty() or not types.is_empty():
		_validate_fields(root, constraints, types, 0)


static func _resolve_value(value: SynxValue, root: SynxValue, options: SynxOptions, path: String, includes: Dictionary, calc_special: PackedStringArray, depth: int) -> void:
	if value.kind != SynxValue.Kind.OBJECT:
		return
	var map: Dictionary = value.data
	if depth >= MAX_RESOLVE_DEPTH:
		for k in map:
			map[k] = SynxValue.make_string("NESTING_ERR: maximum object nesting depth exceeded")
		return
	var mm: Variant = value.meta
	for key in map:
		var child: SynxValue = map[key]
		if child.kind == SynxValue.Kind.OBJECT:
			_resolve_value(child, root, options, key if path.is_empty() else path + "." + key, includes, calc_special, depth + 1)
		elif child.kind == SynxValue.Kind.ARRAY:
			var item_path: String = key if path.is_empty() else path + "." + key
			for item in child.data:
				if (item as SynxValue).kind == SynxValue.Kind.OBJECT:
					_resolve_value(item, root, options, item_path, includes, calc_special, depth + 1)
		if mm != null:
			var meta: SynxMeta = (mm as Dictionary).get(key)
			if meta != null:
				SynxEngineMarkers.apply_markers(value, key, meta, root, options, path, includes, calc_special)
	for key in map:
		var v: SynxValue = map[key]
		if v.kind == SynxValue.Kind.STRING and (v.data as String).contains("{"):
			var replaced := _resolve_interpolation(v.data, root, map, includes)
			if replaced != v.data:
				map[key] = SynxValue.make_string(replaced)


# ─── Registries and validation ──

static func _collect_registries(value: SynxValue, types: Dictionary, constraints: Dictionary, depth: int) -> void:
	if depth > MAX_RESOLVE_DEPTH or value.kind != SynxValue.Kind.OBJECT:
		return
	if value.meta != null:
		var mm: Dictionary = value.meta
		for key in mm:
			var m: SynxMeta = mm[key]
			if m.type_hint != null and not types.has(key):
				types[key] = m.type_hint
			if m.has_constraints:
				if constraints.has(key):
					(constraints[key] as SynxMeta).merge_constraints(m)
				else:
					var c := SynxMeta.new()
					m.copy_constraints_to(c)
					constraints[key] = c
	for key in value.data:
		var child: SynxValue = value.data[key]
		if child.kind == SynxValue.Kind.OBJECT:
			_collect_registries(child, types, constraints, depth + 1)
		elif child.kind == SynxValue.Kind.ARRAY:
			for item in child.data:
				if (item as SynxValue).kind == SynxValue.Kind.OBJECT:
					_collect_registries(item, types, constraints, depth + 1)


# Constraints then type for every field of the tree, in one walk. A value
# that already holds a CONSTRAINT_ERR / TYPE_ERR is not re-checked against
# constraints (the message's own length would be measured).
static func _validate_fields(value: SynxValue, constraints: Dictionary, types: Dictionary, depth: int) -> void:
	if value.kind != SynxValue.Kind.OBJECT:
		return
	var map: Dictionary = value.data
	for key in map:
		var val: SynxValue = map[key]
		var c: SynxMeta = constraints.get(key)
		if c != null:
			var errored := val.kind == SynxValue.Kind.STRING and ((val.data as String).begins_with("CONSTRAINT_ERR:") or (val.data as String).begins_with("TYPE_ERR:"))
			if not errored:
				var err := SynxEngineMarkers.constraint_violation(key, val, c)
				if not err.is_empty():
					val = SynxValue.make_string(err)
					map[key] = val
		if types.has(key):
			var expected: String = types[key]
			if not value_matches_type(val, expected):
				val = SynxValue.make_string("TYPE_ERR: '%s' expected %s but got %s" % [key, expected, val.type_name()])
				map[key] = val
		if val.kind == SynxValue.Kind.OBJECT:
			_validate_fields(val, constraints, types, depth + 1)
		elif val.kind == SynxValue.Kind.ARRAY:
			for item in val.data:
				if (item as SynxValue).kind == SynxValue.Kind.OBJECT:
					_validate_fields(item, constraints, types, depth + 1)


static func value_matches_type(v: SynxValue, expected: String) -> bool:
	match expected:
		"int":
			return v.kind == SynxValue.Kind.INT
		"float":
			return v.kind == SynxValue.Kind.FLOAT or v.kind == SynxValue.Kind.INT
		"bool":
			return v.kind == SynxValue.Kind.BOOL
		"string":
			return v.kind == SynxValue.Kind.STRING or v.kind == SynxValue.Kind.SECRET
		"array":
			return v.kind == SynxValue.Kind.ARRAY
		"object":
			return v.kind == SynxValue.Kind.OBJECT
	return true


# ─── :inherit pre-pass ──

static func _apply_inheritance(root: SynxValue) -> void:
	if root.kind != SynxValue.Kind.OBJECT or root.meta == null:
		return
	var root_meta: Dictionary = root.meta
	var inherits: Array = []
	for key in root_meta:
		var m: SynxMeta = root_meta[key]
		if (m.bits & SynxMeta.MK_INHERIT) == 0:
			continue
		var idx := m.markers.find("inherit")
		var parents := m.markers.slice(idx + 1)
		if parents.is_empty() and not m.args.is_empty():
			parents = m.args.duplicate()
		if not parents.is_empty():
			inherits.append([key, parents])

	var map: Dictionary = root.data
	for entry in inherits:
		var child_key: String = entry[0]
		var parents: PackedStringArray = entry[1]
		# Only an object inherits; a list or a scalar stays as written.
		var child: SynxValue = map.get(child_key)
		if child == null or child.kind != SynxValue.Kind.OBJECT:
			continue
		var merged: Dictionary = {}
		var source: Dictionary = {}
		for parent_name in parents:
			var p: SynxValue = map.get(parent_name)
			if p == null or p.kind != SynxValue.Kind.OBJECT:
				continue
			for k in p.data:
				merged[k] = (p.data[k] as SynxValue).clone()
				source[k] = p
		for k in child.data:
			merged[k] = child.data[k]
			source[k] = child
		var top: Dictionary = {}
		for k in source:
			var from: SynxValue = source[k]
			if from.meta != null and (from.meta as Dictionary).has(k):
				top[k] = from.meta[k]
		var merged_v := SynxValue.make_object(merged)
		if not top.is_empty():
			merged_v.meta = top
		map[child_key] = merged_v


# ─── :secret pre-pass ──

static func _mask_secret_literals(value: SynxValue, depth: int) -> void:
	if value.kind != SynxValue.Kind.OBJECT or depth > MAX_RESOLVE_DEPTH:
		return
	var map: Dictionary = value.data
	if value.meta != null:
		var mm: Dictionary = value.meta
		for key in mm:
			var m: SynxMeta = mm[key]
			if (m.bits & SynxMeta.MK_SECRET) != 0 and (m.bits & _RESOLVING_BEFORE_SECRET) == 0:
				var v: SynxValue = map.get(key)
				if v != null and v.kind != SynxValue.Kind.SECRET and v.kind != SynxValue.Kind.OBJECT and v.kind != SynxValue.Kind.ARRAY:
					map[key] = SynxValue.make_secret(value_to_string(v))
	for key in map:
		var child: SynxValue = map[key]
		if child.kind == SynxValue.Kind.OBJECT:
			_mask_secret_literals(child, depth + 1)
		elif child.kind == SynxValue.Kind.ARRAY:
			for item in child.data:
				if (item as SynxValue).kind == SynxValue.Kind.OBJECT:
					_mask_secret_literals(item, depth + 1)


# ─── Interpolation ──

# `{key}`, `{key.path}`, `{key:alias}`, `{key:include}` (synx-core
# `resolve_interpolation`).
static func _resolve_interpolation(tpl: String, root: SynxValue, local_map: Dictionary, includes: Dictionary) -> String:
	var out := PackedStringArray()
	var chars := 0
	var n := tpl.length()
	var i := 0
	while i < n:
		var lb := tpl.find("{", i)
		if lb < 0:
			out.append(tpl.substr(i))
			chars += n - i
			break
		if lb > i:
			out.append(tpl.substr(i, lb - i))
			chars += lb - i
		var close := tpl.find("}", lb + 1)
		if close < 0:
			out.append(tpl.substr(lb))
			chars += n - lb
			break
		var inner := tpl.substr(lb + 1, close - lb - 1)
		var colon := inner.find(":")
		var handled := false
		var piece := ""
		if colon >= 0:
			var ref_name := inner.substr(0, colon)
			if SynxText.all_alnum_or(ref_name, true):
				var scope := inner.substr(colon + 1)
				var resolved: SynxValue = null
				if scope == "include":
					if includes.size() == 1:
						resolved = deep_get(includes.values()[0], ref_name)
				elif includes.has(scope):
					resolved = deep_get(includes[scope], ref_name)
				piece = value_to_string(resolved) if resolved != null else "{" + inner + "}"
				handled = true
		elif SynxText.all_alnum_or(inner, true):
			var resolved2 := deep_get(root, inner)
			if resolved2 == null:
				resolved2 = local_map.get(inner)
			piece = value_to_string(resolved2) if resolved2 != null else "{" + inner + "}"
			handled = true
		if handled:
			out.append(piece)
			chars += piece.length()
			i = close + 1
		else:
			out.append("{")
			chars += 1
			i = lb + 1
		if chars * 4 >= MAX_ENGINE_SCRATCH_STRING and SynxText.utf8_len("".join(out)) >= MAX_ENGINE_SCRATCH_STRING - 8:
			return _resolve_interpolation_capped(tpl, root, local_map, includes)
	return "".join(out)


# The same, byte for byte as synx-core caps a result at 4 MiB: literal text
# stops once the cap is reached, a value is cut to the room left on a
# character boundary.
static func _resolve_interpolation_capped(tpl: String, root: SynxValue, local_map: Dictionary, includes: Dictionary) -> String:
	var out := PackedStringArray()
	var bytes := 0
	var cap := MAX_ENGINE_SCRATCH_STRING
	var n := tpl.length()
	var i := 0
	while i < n:
		if bytes >= cap:
			break
		var c := tpl.unicode_at(i)
		if c == 123:
			var close := tpl.find("}", i + 1)
			if close >= 0:
				var inner := tpl.substr(i + 1, close - i - 1)
				var colon := inner.find(":")
				var resolved: SynxValue = null
				var valid := false
				var shown := inner
				if colon >= 0:
					var ref_name := inner.substr(0, colon)
					if SynxText.all_alnum_or(ref_name, true):
						valid = true
						var scope := inner.substr(colon + 1)
						if scope == "include":
							if includes.size() == 1:
								resolved = deep_get(includes.values()[0], ref_name)
						elif includes.has(scope):
							resolved = deep_get(includes[scope], ref_name)
				elif SynxText.all_alnum_or(inner, true):
					valid = true
					resolved = deep_get(root, inner)
					if resolved == null:
						resolved = local_map.get(inner)
				if valid:
					if resolved != null:
						var s := SynxText.utf8_prefix(value_to_string(resolved), cap - bytes)
						out.append(s)
						bytes += SynxText.utf8_len(s)
					else:
						out.append("{")
						bytes += 1
						var rem := cap - bytes - 1
						if rem > 0:
							var part := SynxText.utf8_prefix(shown, rem)
							out.append(part)
							bytes += SynxText.utf8_len(part)
						if bytes < cap:
							out.append("}")
							bytes += 1
					i = close + 1
					continue
		if bytes < cap:
			out.append(tpl.substr(i, 1))
			bytes += SynxText.utf8_width(c)
		i += 1
	return "".join(out)


# ─── Path traversal ──

# synx-core `deep_get`: the key itself first, then a dot-path. Lists and
# objects come back as copies when `copy` is set (Rust returns clones; a
# shared object would be resolved twice).
static func deep_get(root: SynxValue, path: String, copy: bool = false) -> SynxValue:
	if root == null:
		return null
	var found: SynxValue = null
	if root.kind == SynxValue.Kind.OBJECT and (root.data as Dictionary).has(path):
		found = root.data[path]
	else:
		var cur := root
		for part in path.split("."):
			if cur.kind != SynxValue.Kind.OBJECT:
				return null
			var nx: SynxValue = (cur.data as Dictionary).get(part)
			if nx == null:
				return null
			cur = nx
		found = cur
	if copy and (found.kind == SynxValue.Kind.OBJECT or found.kind == SynxValue.Kind.ARRAY):
		return found.clone()
	return found


# ─── Value → text (synx-core `value_to_string`) ──

static func value_to_string(v: SynxValue) -> String:
	match v.kind:
		SynxValue.Kind.STRING:
			return v.data
		SynxValue.Kind.INT:
			return str(v.data)
		SynxValue.Kind.FLOAT:
			return SynxNum.format_number(v.data)
		SynxValue.Kind.BOOL:
			return "true" if v.data else "false"
		SynxValue.Kind.NULL:
			return "null"
		SynxValue.Kind.SECRET:
			return "[SECRET]"
	return ""


# A key whole-word substitution can reach and a token lookup cannot: one
# with a non-word character, all digits, or a spelling of a number.
static func is_calc_special_key(k: String) -> bool:
	var all_digits := true
	for i in k.length():
		var c := k.unicode_at(i)
		var word := (c >= 48 and c <= 57) or (c >= 65 and c <= 90) or (c >= 97 and c <= 122) or c == 95
		if not word:
			return true
		if c < 48 or c > 57:
			all_digits = false
	return all_digits or k == "NaN" or k == "inf"


# ─── File loading ──

# Reads, parses and (when `!active`) resolves a SYNX file; the root, or null.
static func _load_synx_file(rel_path: String, options: SynxOptions) -> Variant:
	var max_depth := options.max_include_depth if options.max_include_depth > 0 else DEFAULT_MAX_INCLUDE_DEPTH
	if options._include_depth >= max_depth:
		return null
	var jail := jail_path(_effective_base(options), rel_path)
	if not bool(jail["ok"]):
		return null
	var full: String = jail["path"]
	if not FileAccess.file_exists(full) or _file_size(full) > MAX_FILE_SIZE:
		return null
	var text := _read_text(full)
	if text == null:
		return null
	var included := SynxParser.parse(text)
	if included.mode == SynxParseResult.Mode.ACTIVE:
		var child := options.clone()
		child._include_depth += 1
		child.base_path = _parent_dir(full)
		resolve(included, child)
	return included.root


static func _load_includes(directives: Array, options: SynxOptions) -> Dictionary:
	var out: Dictionary = {}
	var max_depth := options.max_include_depth if options.max_include_depth > 0 else DEFAULT_MAX_INCLUDE_DEPTH
	if options._include_depth >= max_depth:
		return out
	for inc in directives:
		var v: Variant = _load_synx_file(inc.path, options)
		if v != null:
			out[inc.alias] = v
	return out


# `!use @scope/name` → `<packages_path>/@scope/name/src/main.synx` (or the
# manifest's `main`). WASM marker packages have no GDScript runtime; use
# `SynxOptions.marker_callables` instead.
static func _load_packages(directives: Array, options: SynxOptions) -> Dictionary:
	var out: Dictionary = {}
	var pkg_base := options.packages_path if not options.packages_path.is_empty() else "synx_packages"
	for ud in directives:
		if not is_contained_relative(ud.package):
			continue
		var rel_entry := _read_manifest_main(pkg_base, ud.package, options)
		if rel_entry.is_empty():
			rel_entry = pkg_base + "/" + ud.package + "/src/main.synx"
		var v: Variant = _load_synx_file(rel_entry, options)
		if v != null:
			out[ud.alias] = v
	return out


static func _read_manifest_main(pkg_base: String, package: String, options: SynxOptions = null) -> String:
	var rel := pkg_base + "/" + package + "/synx-pkg.synx"
	var base := _effective_base(options if options != null else SynxOptions.new())
	var full_jail := jail_path(base, rel)
	if not bool(full_jail["ok"]):
		return ""
	var full: String = full_jail["path"]
	if not FileAccess.file_exists(full):
		return ""
	var text: Variant = _read_text(full)
	if text == null:
		return ""
	for line in (text as String).split("\n"):
		var t := SynxText.trim(line)
		# A field that does not stay inside the package counts as absent.
		if t.begins_with("main "):
			var p := SynxText.trim(t.substr(5))
			if is_contained_relative(p):
				return pkg_base + "/" + package + "/" + p
		if t.begins_with("entry "):
			var p2 := SynxText.trim(t.substr(6))
			if is_contained_relative(p2):
				return pkg_base + "/" + package + "/" + p2
	return ""


# synx-core `is_contained_relative`: a package name or manifest path names
# something inside its base by its text — not empty, no leading `/` or `\`,
# no `:` (drive, scheme), no `..` segment.
static func is_contained_relative(path: String) -> bool:
	if path.is_empty() or path.begins_with("/") or path.begins_with("\\") or path.contains(":"):
		return false
	for seg in path.replace("\\", "/").split("/"):
		if seg == "..":
			return false
	return true


# The file as text (a leading U+FEFF kept for the parser to drop, as Rust's
# `read_to_string` does), or null when it cannot be read or is not UTF-8.
static func _read_text(full: String) -> Variant:
	var f := FileAccess.open(full, FileAccess.READ)
	if f == null:
		return null
	var bytes := f.get_buffer(f.get_length())
	f.close()
	return SynxText.utf8_decode(bytes)


# ─── Path utilities ──

static func _effective_base(options: SynxOptions) -> String:
	if options.base_path.is_empty():
		return "res://"
	return options.base_path


# Returns { "ok": bool, "path": String, "error": String }, error texts as in
# synx-core `jail_path`. Honors res://, user:// and filesystem bases.
static func jail_path(base: String, rel: String) -> Dictionary:
	if rel.is_empty():
		return {"ok": false, "error": "SECURITY: path escapes base directory: ''"}
	if rel.begins_with("/") or rel.begins_with("\\"):
		return {"ok": false, "error": "SECURITY: rooted paths are not allowed: '%s'" % rel}
	if (rel.length() >= 2 and rel[1] == ":") or rel.begins_with("res://") or rel.begins_with("user://"):
		return {"ok": false, "error": "SECURITY: absolute paths are not allowed: '%s'" % rel}
	var b := base
	if not b.ends_with("/") and not b.ends_with("\\"):
		b += "/"
	var joined := b + rel
	if rel.contains(".."):
		var simple := joined.simplify_path()
		var base_simple := b.simplify_path()
		if not simple.begins_with(base_simple.trim_suffix("/") + "/") and simple != base_simple:
			return {"ok": false, "error": "SECURITY: path traversal detected: '%s'" % rel}
		joined = simple
	return {"ok": true, "path": joined}


static func _parent_dir(full: String) -> String:
	var slash := maxi(full.rfind("/"), full.rfind("\\"))
	if slash >= 0:
		return full.substr(0, slash)
	return ""


static func _file_size(path: String) -> int:
	var f := FileAccess.open(path, FileAccess.READ)
	if f == null:
		return -1
	var n := f.get_length()
	f.close()
	return n


# ─── Spam buckets ──

static func allow_spam_access(bucket_key: String, max_calls: int, window_sec: int) -> bool:
	var now_ms := Time.get_ticks_msec()
	var window_ms: int = maxi(window_sec, 1) * 1000
	var bucket: Array = _spam_buckets.get(bucket_key, [])
	var keep: Array = []
	for ts in bucket:
		if now_ms - int(ts) <= window_ms:
			keep.append(ts)
	if keep.size() >= max_calls:
		_spam_buckets[bucket_key] = keep
		return false
	keep.append(now_ms)
	_spam_buckets[bucket_key] = keep
	return true


static func clear_spam_buckets() -> void:
	_spam_buckets.clear()
