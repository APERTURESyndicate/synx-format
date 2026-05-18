@tool
class_name SynxEngine
extends RefCounted

# Active-mode engine — resolves SYNX markers in a parsed tree.
# Mirrors synx-core::engine::resolve.
#
# Public entry point is `resolve(result, options)`. The engine mutates
# `result.root` in place, replaces marker placeholders with resolved
# values, validates constraints, and strips private _-prefixed top-level
# keys (used as :inherit base templates).
#
# Marker dispatch lives in synx_engine_markers.gd for readability.

const MAX_CALC_EXPR_LEN: int = 4096
const MAX_CALC_RESOLVED_LEN: int = 64 * 1024
const MAX_FILE_SIZE: int = 10 * 1024 * 1024
const DEFAULT_MAX_INCLUDE_DEPTH: int = 16
const MAX_RESOLVE_DEPTH: int = 512
const MAX_ENGINE_SCRATCH_STRING: int = 4 * 1024 * 1024

# Spam-rate buckets shared across all engine calls.
# Keyed by "key::target". Value: Array[int] of unix-ms timestamps.
static var _spam_buckets: Dictionary = {}


static func resolve(result: SynxParseResult, options: SynxOptions) -> void:
	if result.mode != SynxParseResult.Mode.ACTIVE:
		return

	var metadata: Dictionary = result.metadata
	var include_dirs: Array = result.includes
	var use_dirs: Array = result.uses

	# Load !use packages into root before include processing.
	var packages_map := _load_packages(use_dirs, options)

	# Load !include files into alias map.
	var includes_map := _load_includes(include_dirs, options)

	# Pre-pass: also register `:include`/`:import` marker keys as aliases.
	if result.root.kind == SynxValue.Kind.OBJECT:
		var root_map: Dictionary = result.root.data
		for key in root_map.keys():
			var path_meta: Dictionary = metadata.get("", {})
			if not path_meta.has(key):
				continue
			var meta: SynxMeta = path_meta[key]
			var is_inc := meta.has_marker("include") or meta.has_marker("import")
			if not is_inc:
				continue
			var v: SynxValue = root_map[key]
			if v.kind != SynxValue.Kind.STRING:
				continue
			var loaded := _load_synx_file(String(v.data), options)
			if loaded != null and not includes_map.has(key):
				includes_map[key] = loaded

	# Merge packages into root.
	if result.root.kind == SynxValue.Kind.OBJECT:
		var rm: Dictionary = result.root.data
		for alias in packages_map.keys():
			if not rm.has(alias):
				rm[alias] = packages_map[alias]

	# :inherit pre-pass — must run before normal marker dispatch.
	SynxEngineMarkers.apply_inheritance(result.root, metadata)

	# Strip private `_`-prefixed top-level keys (used as inherit bases).
	if result.root.kind == SynxValue.Kind.OBJECT:
		var to_del: Array = []
		for k in result.root.data.keys():
			if String(k).begins_with("_"):
				to_del.append(k)
		for k in to_del:
			result.root.data.erase(k)

	var type_registry := SynxEngineMarkers.build_type_registry(metadata)
	var constraint_registry := SynxEngineMarkers.build_constraint_registry(metadata)

	_resolve_value(result.root, result.root, options, metadata, "", includes_map, 0)

	# Whole-tree validation passes.
	SynxEngineMarkers.validate_field_constraints(result.root, constraint_registry)
	SynxEngineMarkers.validate_field_types(result.root, type_registry)


static func _resolve_value(value: SynxValue, root: SynxValue, options: SynxOptions, metadata: Dictionary, path: String, includes: Dictionary, depth: int) -> void:
	if depth >= MAX_RESOLVE_DEPTH:
		if value.kind == SynxValue.Kind.OBJECT:
			var map: Dictionary = value.data
			for k in map.keys():
				map[k] = SynxValue.make_string("NESTING_ERR: maximum object nesting depth exceeded")
		return

	if value.kind != SynxValue.Kind.OBJECT:
		return

	var map: Dictionary = value.data
	var keys: Array = map.keys()
	# Recurse first so child resolved values are available to parent markers.
	for key in keys:
		var child: SynxValue = map[key]
		var child_path := key if path.is_empty() else "%s.%s" % [path, key]
		match child.kind:
			SynxValue.Kind.OBJECT:
				_resolve_value(child, root, options, metadata, child_path, includes, depth + 1)
			SynxValue.Kind.ARRAY:
				var arr: Array = child.data
				for item in arr:
					if item is SynxValue and item.kind == SynxValue.Kind.OBJECT:
						_resolve_value(item, root, options, metadata, child_path, includes, depth + 1)

	# Apply markers (second pass).
	var meta_map: Dictionary = metadata.get(path, {})
	if not meta_map.is_empty():
		for key in keys:
			if not meta_map.has(key):
				continue
			var meta: SynxMeta = meta_map[key]
			SynxEngineMarkers.apply_markers(map, key, meta, root, options, path, metadata, includes)

	# Interpolation pass — substitute {key}, {key.path}, {key:alias}, {key:include}.
	var keys2: Array = map.keys()
	for key in keys2:
		var v: SynxValue = map[key]
		if v.kind == SynxValue.Kind.STRING and "{" in String(v.data):
			var replaced := _resolve_interpolation(String(v.data), root, map, includes)
			if replaced != String(v.data):
				map[key] = SynxValue.make_string(replaced)


# ─── Interpolation ──

static func _resolve_interpolation(tpl: String, root: SynxValue, local_map: Dictionary, includes: Dictionary) -> String:
	var out := ""
	var n := tpl.length()
	var i := 0
	while i < n:
		if out.length() >= MAX_ENGINE_SCRATCH_STRING:
			break
		var ch := tpl.unicode_at(i)
		if ch == 123: # '{'
			var close_rel := tpl.substr(i + 1).find("}")
			if close_rel >= 0:
				var inner := tpl.substr(i + 1, close_rel)
				var colon := inner.find(":")
				if colon >= 0:
					var ref_name := inner.substr(0, colon)
					var scope := inner.substr(colon + 1)
					if _is_valid_ref(ref_name):
						var resolved: Variant = null
						if scope == "include":
							if includes.size() == 1:
								var first_alias: String = String(includes.keys()[0])
								resolved = deep_get(includes[first_alias], ref_name)
						else:
							if includes.has(scope):
								resolved = deep_get(includes[scope], ref_name)
						if resolved != null:
							out += _value_to_string(resolved)
						else:
							out += "{" + inner + "}"
						i += 2 + close_rel
						continue
				else:
					var ref_name2 := inner
					if _is_valid_ref(ref_name2):
						var resolved2: Variant = deep_get(root, ref_name2)
						if resolved2 == null and local_map.has(ref_name2):
							resolved2 = local_map[ref_name2]
						if resolved2 != null:
							out += _value_to_string(resolved2)
						else:
							out += "{" + ref_name2 + "}"
						i += 2 + close_rel
						continue
		out += tpl.substr(i, 1)
		i += 1
	return out


static func _is_valid_ref(s: String) -> bool:
	if s.is_empty():
		return false
	for i in s.length():
		var ch := s.unicode_at(i)
		var ok := (ch >= 48 and ch <= 57) or (ch >= 65 and ch <= 90) or (ch >= 97 and ch <= 122) or ch == 95 or ch == 46
		if not ok:
			return false
	return true


# ─── Path traversal ──

static func deep_get(root: SynxValue, path: String) -> Variant:
	if root == null:
		return null
	if root.kind == SynxValue.Kind.OBJECT and (root.data as Dictionary).has(path):
		return root.data[path]
	var parts := path.split(".")
	var current: SynxValue = root
	for part in parts:
		if current.kind != SynxValue.Kind.OBJECT:
			return null
		var map: Dictionary = current.data
		if not map.has(part):
			return null
		current = map[part]
	return current


# ─── Helpers ──

static func _value_to_string(v: SynxValue) -> String:
	match v.kind:
		SynxValue.Kind.STRING, SynxValue.Kind.SECRET:
			return String(v.data)
		SynxValue.Kind.INT:
			return str(v.data)
		SynxValue.Kind.FLOAT:
			return _format_number(float(v.data))
		SynxValue.Kind.BOOL:
			return "true" if v.data else "false"
		SynxValue.Kind.NULL:
			return "null"
		SynxValue.Kind.ARRAY, SynxValue.Kind.OBJECT:
			return ""
	return ""


static func _format_number(n: float) -> String:
	if absf(n - floorf(n)) < 1e-12 and absf(n) < 9.22e18:
		return str(int(n))
	return str(n)


# ─── File loading ──

# Loads a SYNX file and runs the engine on it. Returns SynxValue or null on failure.
static func _load_synx_file(rel_path: String, options: SynxOptions) -> Variant:
	var max_depth := options.max_include_depth if options.max_include_depth > 0 else DEFAULT_MAX_INCLUDE_DEPTH
	if options._include_depth >= max_depth:
		return null
	var jail_res := jail_path(_effective_base(options), rel_path)
	if not bool(jail_res["ok"]):
		return null
	var full: String = jail_res["path"]
	if not FileAccess.file_exists(full):
		return null
	if _file_size(full) > MAX_FILE_SIZE:
		return null
	var f := FileAccess.open(full, FileAccess.READ)
	if f == null:
		return null
	var text := f.get_as_text()
	f.close()
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
		var v := _load_synx_file(inc.path, options)
		if v != null:
			out[inc.alias] = v
	return out


# Packages support is `!use @scope/name` → loads `<packages_path>/@scope/name/src/main.synx`.
# Marker (WASM) packages are not supported in GDScript — see synx_engine_markers.gd for
# Callable-based replacements.
static func _load_packages(directives: Array, options: SynxOptions) -> Dictionary:
	var out: Dictionary = {}
	var pkg_base := options.packages_path if not options.packages_path.is_empty() else "synx_packages"
	for ud in directives:
		var rel_entry := _read_manifest_main(pkg_base, ud.package, options)
		if rel_entry.is_empty():
			rel_entry = pkg_base + "/" + ud.package + "/src/main.synx"
		var v := _load_synx_file(rel_entry, options)
		if v != null:
			out[ud.alias] = v
	return out


static func _read_manifest_main(pkg_base: String, package: String, options: SynxOptions = null) -> String:
	var rel := pkg_base + "/" + package + "/synx-pkg.synx"
	# Honour the caller's base_path so packages resolve relative to the
	# current file rather than always `res://`.
	var base := _effective_base(options if options != null else SynxOptions.new())
	var full_jail := jail_path(base, rel)
	if not bool(full_jail["ok"]):
		return ""
	var full: String = full_jail["path"]
	if not FileAccess.file_exists(full):
		return ""
	var f := FileAccess.open(full, FileAccess.READ)
	if f == null:
		return ""
	var text := f.get_as_text()
	f.close()
	for line in text.split("\n"):
		var t := String(line).strip_edges()
		if t.begins_with("main "):
			return pkg_base + "/" + package + "/" + t.substr(5).strip_edges()
		if t.begins_with("entry "):
			return pkg_base + "/" + package + "/" + t.substr(6).strip_edges()
	return ""


# ─── Path utilities ──

static func _effective_base(options: SynxOptions) -> String:
	if options.base_path.is_empty():
		return "res://"
	return options.base_path


# Returns { "ok": bool, "path": String, "error": String }.
# Honors res://, user:// — falls back to absolute filesystem path otherwise.
static func jail_path(base: String, rel: String) -> Dictionary:
	if rel.is_empty():
		return {"ok": false, "error": "empty path"}
	# Reject rooted paths and absolute paths (security).
	if rel.begins_with("/") or rel.begins_with("\\"):
		return {"ok": false, "error": "SECURITY: rooted paths are not allowed: '%s'" % rel}
	if rel.length() >= 2 and rel[1] == ":":
		return {"ok": false, "error": "SECURITY: absolute paths are not allowed: '%s'" % rel}
	if rel.begins_with("res://") or rel.begins_with("user://"):
		return {"ok": false, "error": "SECURITY: absolute paths are not allowed: '%s'" % rel}
	if "../" in rel or "..\\" in rel or rel.ends_with("/..") or rel.ends_with("\\.."):
		return {"ok": false, "error": "SECURITY: path traversal detected: '%s'" % rel}

	var b := base
	if not b.ends_with("/") and not b.ends_with("\\"):
		b += "/"
	var joined := b + rel
	return {"ok": true, "path": joined}


static func _parent_dir(full: String) -> String:
	var slash := max(full.rfind("/"), full.rfind("\\"))
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
	var window_ms := max(window_sec, 1) * 1000
	var bucket: Array = _spam_buckets.get(bucket_key, [])
	# Drop expired entries.
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
