@tool
class_name SynxEngineMarkers
extends RefCounted

# Marker dispatch + validation. Mirrors apply_markers / validate_* in
# synx-core::engine.
#
# Each marker reads from / writes to `map` (the current object's
# Dictionary[String, SynxValue]). Marker order follows the Rust file for
# binary parity.

const MAX_CALC_EXPR_LEN: int = 4096
const MAX_CALC_RESOLVED_LEN: int = 64 * 1024


# ─── Public dispatcher ──

static func apply_markers(map: Dictionary, key: String, meta: SynxMeta, root: SynxValue, options: SynxOptions, path: String, metadata: Dictionary, includes: Dictionary) -> void:
	var markers := meta.markers

	# :spam — rate-limit checked first; on failure replaces value with SPAM_ERR.
	if _has(markers, "spam"):
		if not _apply_spam(map, key, markers, root):
			return

	# :include / :import — short-circuits other markers on the same key.
	if _has(markers, "include") or _has(markers, "import"):
		_apply_include(map, key, options)
		return

	if _has(markers, "env"):
		_apply_env(map, key, meta, markers, options)

	if _has(markers, "random"):
		_apply_random(map, key, meta)

	if _has(markers, "ref"):
		_apply_ref(map, key, markers, root)

	if _has(markers, "i18n"):
		_apply_i18n(map, key, markers, options, root)

	if _has(markers, "calc"):
		_apply_calc(map, key, root)

	if _has(markers, "alias"):
		_apply_alias(map, key, path, root, metadata)

	if _has(markers, "secret"):
		_apply_secret(map, key)

	if _has(markers, "unique"):
		_apply_unique(map, key)

	if _has(markers, "geo"):
		_apply_geo(map, key, options)

	if _has(markers, "split"):
		_apply_split(map, key, markers)

	if _has(markers, "join"):
		_apply_join(map, key, markers)

	if _has(markers, "default") and not _has(markers, "env"):
		_apply_default_standalone(map, key, meta, markers)

	if _has(markers, "clamp"):
		_apply_clamp(map, key, markers)

	if _has(markers, "round"):
		_apply_round(map, key, markers)

	if _has(markers, "map"):
		_apply_map_lookup(map, key, markers, root)

	if _has(markers, "format"):
		_apply_format(map, key, markers)

	if _has(markers, "replace"):
		_apply_replace(map, key, markers)

	if _has(markers, "sort"):
		_apply_sort(map, key, markers)

	if _has(markers, "sum"):
		_apply_sum(map, key)

	if _has(markers, "fallback"):
		_apply_fallback(map, key, markers, options)

	if _has(markers, "once"):
		_apply_once(map, key, markers, options)

	if _has(markers, "version"):
		_apply_version(map, key, markers)

	if _has(markers, "watch"):
		_apply_watch(map, key, markers, options)

	if _has(markers, "prompt"):
		_apply_prompt(map, key, markers)

	# :vision / :audio — metadata-only no-ops.

	# Custom Callable markers (GDScript replacement for WASM markers).
	if not options.marker_callables.is_empty():
		for marker in markers:
			var m := String(marker)
			if _BUILTIN_MARKERS.has(m):
				continue
			if options.marker_callables.has(m):
				var callable: Callable = options.marker_callables[m]
				var current: SynxValue = map.get(key, SynxValue.make_null())
				var idx := markers.find(m)
				var args := PackedStringArray()
				for j in range(idx + 1, markers.size()):
					args.append(String(markers[j]))
				var result = callable.call(current, args)
				if result is SynxValue:
					map[key] = result
				else:
					map[key] = SynxValue.make_string("CUSTOM_ERR: marker '%s' did not return SynxValue" % m)
				break

	# Constraint validation — must be last.
	if meta.has_constraints:
		validate_per_key_constraints(map, key, meta)


const _BUILTIN_MARKERS := [
	"spam", "include", "import", "env", "random", "ref", "i18n", "calc",
	"alias", "secret", "unique", "geo", "split", "join", "default", "clamp",
	"round", "map", "format", "replace", "sort", "sum", "fallback", "once",
	"version", "watch", "prompt", "vision", "audio", "inherit", "template",
]


static func _has(markers: PackedStringArray, name: String) -> bool:
	for m in markers:
		if String(m) == name:
			return true
	return false


# ─── :spam ──

static func _apply_spam(map: Dictionary, key: String, markers: PackedStringArray, root: SynxValue) -> bool:
	var idx := markers.find("spam")
	var max_calls := int(markers[idx + 1].to_int()) if idx + 1 < markers.size() else 0
	var window := int(markers[idx + 2].to_int()) if idx + 2 < markers.size() else 1
	if max_calls == 0:
		map[key] = SynxValue.make_string("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]")
		return false
	var current: SynxValue = map.get(key, SynxValue.make_null())
	var target := _value_to_string(current) if current != null else key
	var bucket_key := "%s::%s" % [key, target]
	if not SynxEngine.allow_spam_access(bucket_key, max_calls, window):
		map[key] = SynxValue.make_string("SPAM_ERR: '%s' exceeded %d calls per %ds" % [target, max_calls, window])
		return false
	# Optionally resolve target as a key path.
	var resolved: Variant = SynxEngine.deep_get(root, target)
	if resolved == null and map.has(target):
		resolved = map[target]
	if resolved != null:
		map[key] = resolved
	return true


# ─── :include / :import ──

static func _apply_include(map: Dictionary, key: String, options: SynxOptions) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var rel := String(v.data)
	var max_depth := options.max_include_depth if options.max_include_depth > 0 else SynxEngine.DEFAULT_MAX_INCLUDE_DEPTH
	if options._include_depth >= max_depth:
		map[key] = SynxValue.make_string("INCLUDE_ERR: max include depth (%d) exceeded" % max_depth)
		return
	var jail := SynxEngine.jail_path(SynxEngine._effective_base(options), rel)
	if not bool(jail["ok"]):
		map[key] = SynxValue.make_string("INCLUDE_ERR: %s" % jail["error"])
		return
	var full: String = jail["path"]
	if not FileAccess.file_exists(full):
		map[key] = SynxValue.make_string("INCLUDE_ERR: file not found: %s" % rel)
		return
	if SynxEngine._file_size(full) > SynxEngine.MAX_FILE_SIZE:
		map[key] = SynxValue.make_string("INCLUDE_ERR: file too large")
		return
	var f := FileAccess.open(full, FileAccess.READ)
	if f == null:
		map[key] = SynxValue.make_string("INCLUDE_ERR: cannot open %s" % rel)
		return
	var text := f.get_as_text()
	f.close()
	var included := SynxParser.parse(text)
	if included.mode == SynxParseResult.Mode.ACTIVE:
		var child := options.clone()
		child._include_depth += 1
		child.base_path = SynxEngine._parent_dir(full)
		SynxEngine.resolve(included, child)
	map[key] = included.root


# ─── :env ──

static func _apply_env(map: Dictionary, key: String, meta: SynxMeta, markers: PackedStringArray, options: SynxOptions) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var var_name := String(v.data)
	var env_val: Variant = null
	if options.env != null and options.env.has(var_name):
		env_val = String(options.env[var_name])
	else:
		var os_val := OS.get_environment(var_name)
		if not os_val.is_empty():
			env_val = os_val

	var force_string := meta.type_hint == "string"
	var default_idx := markers.find("default")

	if env_val != null and not String(env_val).is_empty():
		map[key] = SynxValue.make_string(String(env_val)) if force_string else _cast_primitive(String(env_val))
	elif default_idx >= 0 and default_idx + 1 < markers.size():
		var parts: Array[String] = []
		for j in range(default_idx + 1, markers.size()):
			parts.append(String(markers[j]))
		var fallback := ":".join(parts)
		map[key] = SynxValue.make_string(fallback) if force_string else _cast_primitive(fallback)
	else:
		map[key] = SynxValue.make_null()


# ─── :random ──

static func _apply_random(map: Dictionary, key: String, meta: SynxMeta) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.ARRAY:
		return
	var arr: Array = v.data
	if arr.is_empty():
		map[key] = SynxValue.make_null()
		return
	if meta.args.size() > 0:
		var weights: Array[float] = []
		for a in meta.args:
			if String(a).is_valid_float():
				weights.append(String(a).to_float())
		map[key] = _weighted_random(arr, weights)
	else:
		map[key] = arr[SynxRng.random_usize(arr.size())]


static func _weighted_random(items: Array, weights_in: Array) -> SynxValue:
	var w: Array[float] = []
	for f in weights_in:
		w.append(float(f))
	if w.size() < items.size():
		var assigned := 0.0
		for f in w:
			assigned += f
		var per := (100.0 - assigned) / max(items.size() - w.size(), 1)
		if assigned >= 100.0:
			per = assigned / max(w.size(), 1)
		while w.size() < items.size():
			w.append(per)
	var total := 0.0
	for f in w:
		total += f
	if total <= 0:
		return items[SynxRng.random_usize(items.size())]
	var r := SynxRng.random_f64_01()
	var cumulative := 0.0
	for i in items.size():
		cumulative += w[i] / total
		if r <= cumulative:
			return items[i]
	return items[items.size() - 1]


# ─── :ref (with optional :calc shorthand) ──

static func _apply_ref(map: Dictionary, key: String, markers: PackedStringArray, root: SynxValue) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var target := String(v.data)
	var resolved: Variant = SynxEngine.deep_get(root, target)
	if resolved == null and map.has(target):
		resolved = map[target]
	if resolved == null:
		resolved = SynxValue.make_null()

	if _has(markers, "calc"):
		var num := _as_number(resolved)
		if num != null:
			var calc_idx := markers.find("calc")
			if calc_idx + 1 < markers.size():
				var expr_tail := String(markers[calc_idx + 1])
				var first := expr_tail.substr(0, 1)
				if first in ["+", "-", "*", "/", "%"]:
					var expr := _format_number(num) + " " + expr_tail
					var calc_res := SynxSafeCalc.evaluate(expr)
					if bool(calc_res["ok"]):
						map[key] = _coerce_calc_number(float(calc_res["value"]))
					else:
						map[key] = SynxValue.make_string("CALC_ERR: %s" % calc_res["error"])
					return
	map[key] = resolved


# ─── :i18n ──

static func _apply_i18n(map: Dictionary, key: String, markers: PackedStringArray, options: SynxOptions, root: SynxValue) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.OBJECT:
		return
	var translations: Dictionary = v.data
	var lang := options.lang if not options.lang.is_empty() else "en"
	var selected: SynxValue = SynxValue.make_null()
	if translations.has(lang):
		selected = translations[lang]
	elif translations.has("en"):
		selected = translations["en"]
	elif translations.size() > 0:
		selected = translations.values()[0]

	var idx := markers.find("i18n")
	var count_field := String(markers[idx + 1]) if idx + 1 < markers.size() else ""

	if not count_field.is_empty() and selected.kind == SynxValue.Kind.OBJECT:
		var plural_forms: Dictionary = selected.data
		var count_val := 0
		if map.has(count_field):
			var cn := _as_number(map[count_field])
			if cn != null: count_val = int(cn)
		else:
			var rn := _as_number(SynxEngine.deep_get(root, count_field))
			if rn != null: count_val = int(rn)
		var category := plural_category(lang, count_val)
		var chosen: SynxValue = SynxValue.make_null()
		if plural_forms.has(category):
			chosen = plural_forms[category]
		elif plural_forms.has("other"):
			chosen = plural_forms["other"]
		elif plural_forms.size() > 0:
			chosen = plural_forms.values()[0]
		if chosen.kind == SynxValue.Kind.STRING:
			map[key] = SynxValue.make_string(String(chosen.data).replace("{count}", str(count_val)))
		else:
			map[key] = chosen
	else:
		map[key] = selected


# CLDR plural category — matches synx-core::engine::plural_category.
static func plural_category(lang: String, n: int) -> String:
	var abs_n: int = absi(n)
	var n10: int = abs_n % 10
	var n100: int = abs_n % 100
	match lang:
		"ru", "uk", "be", "pl":
			if n10 == 1 and n100 != 11:
				return "one"
			elif n10 >= 2 and n10 <= 4 and not (n100 >= 12 and n100 <= 14):
				return "few"
			else:
				return "many"
		"cs", "sk":
			if abs_n == 1:
				return "one"
			elif abs_n >= 2 and abs_n <= 4:
				return "few"
			else:
				return "other"
		"ar":
			if abs_n == 0:
				return "zero"
			elif abs_n == 1:
				return "one"
			elif abs_n == 2:
				return "two"
			elif n100 >= 3 and n100 <= 10:
				return "few"
			elif n100 >= 11 and n100 <= 99:
				return "many"
			else:
				return "other"
		"fr", "pt":
			return "one" if abs_n <= 1 else "other"
		"ja", "zh", "ko", "vi", "th":
			return "other"
		_:
			return "one" if abs_n == 1 else "other"


# ─── :calc ──

static func _apply_calc(map: Dictionary, key: String, root: SynxValue) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var expr := String(v.data)
	if expr.length() > MAX_CALC_EXPR_LEN:
		map[key] = SynxValue.make_string("CALC_ERR: expression too long (%d chars, max %d)" % [expr.length(), MAX_CALC_EXPR_LEN])
		return

	var resolved := expr
	# Substitute flat root keys.
	if root.kind == SynxValue.Kind.OBJECT:
		for rk in root.data.keys():
			var n := _as_number(root.data[rk])
			if n != null:
				resolved = _replace_word(resolved, String(rk), _format_number(n))
				if resolved.length() > MAX_CALC_RESOLVED_LEN:
					map[key] = SynxValue.make_string("CALC_ERR: resolved expression too long (max %d bytes)" % MAX_CALC_RESOLVED_LEN)
					return
	# Substitute current map keys (except self).
	for rk in map.keys():
		if rk == key:
			continue
		var n := _as_number(map[rk])
		if n != null:
			resolved = _replace_word(resolved, String(rk), _format_number(n))
			if resolved.length() > MAX_CALC_RESOLVED_LEN:
				map[key] = SynxValue.make_string("CALC_ERR: resolved expression too long (max %d bytes)" % MAX_CALC_RESOLVED_LEN)
				return

	# Substitute dot-paths.
	resolved = _substitute_dot_paths(resolved, root)
	if resolved.length() > MAX_CALC_RESOLVED_LEN:
		map[key] = SynxValue.make_string("CALC_ERR: resolved expression too long (max %d bytes)" % MAX_CALC_RESOLVED_LEN)
		return

	var calc := SynxSafeCalc.evaluate(resolved)
	if bool(calc["ok"]):
		map[key] = _coerce_calc_number(float(calc["value"]))
	else:
		map[key] = SynxValue.make_string("CALC_ERR: %s" % calc["error"])


static func _substitute_dot_paths(expr: String, root: SynxValue) -> String:
	var out := ""
	var n := expr.length()
	var i := 0
	while i < n:
		var ch := expr.unicode_at(i)
		if _is_word_char(ch):
			var start := i
			var has_dot := false
			while i < n:
				var c := expr.unicode_at(i)
				if _is_word_char(c):
					i += 1
				elif c == 46:
					has_dot = true
					i += 1
				else:
					break
			var token := expr.substr(start, i - start)
			if has_dot and "." in token:
				var v: Variant = SynxEngine.deep_get(root, token)
				var num := _as_number(v)
				if num != null:
					out += _format_number(num)
					continue
			out += token
		else:
			out += expr.substr(i, 1)
			i += 1
	return out


# ─── :alias ──

static func _apply_alias(map: Dictionary, key: String, path: String, root: SynxValue, metadata: Dictionary) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var target := String(v.data)
	var current_path := key if path.is_empty() else "%s.%s" % [path, key]
	if target == key or target == current_path:
		map[key] = SynxValue.make_string("ALIAS_ERR: self-referential alias: %s → %s" % [current_path, target])
		return
	var target_val: Variant = SynxEngine.deep_get(root, target)
	var dot := target.rfind(".")
	var target_parent := target.substr(0, dot) if dot >= 0 else ""
	var target_key_name := target.substr(dot + 1) if dot >= 0 else target
	var target_has_alias := false
	if metadata.has(target_parent):
		var mm: Dictionary = metadata[target_parent]
		if mm.has(target_key_name):
			var tm: SynxMeta = mm[target_key_name]
			target_has_alias = tm.has_marker("alias")
	var is_cycle := false
	if target_has_alias and target_val is SynxValue and target_val.kind == SynxValue.Kind.STRING:
		var s := String(target_val.data)
		is_cycle = (s == key or s == current_path)
	if is_cycle:
		var a := current_path
		var b := target
		if b < a:
			var tmp := a; a = b; b = tmp
		map[key] = SynxValue.make_string("ALIAS_ERR: circular alias detected: %s → %s" % [a, b])
		return
	if target_val == null:
		target_val = SynxValue.make_null()
	map[key] = target_val


# ─── :secret ──

static func _apply_secret(map: Dictionary, key: String) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	var s := _value_to_string(v)
	map[key] = SynxValue.make_secret(s)


# ─── :unique ──

static func _apply_unique(map: Dictionary, key: String) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.ARRAY:
		return
	var seen: Array[String] = []
	var unique: Array = []
	for item in v.data:
		var s := _value_to_string(item)
		if not seen.has(s):
			seen.append(s)
			unique.append(item)
	map[key] = SynxValue.make_array(unique)


# ─── :geo ──

static func _apply_geo(map: Dictionary, key: String, options: SynxOptions) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.ARRAY:
		return
	var arr: Array = v.data
	var region := options.region if not options.region.is_empty() else "US"
	var prefix := "%s " % region
	for item in arr:
		if item is SynxValue and item.kind == SynxValue.Kind.STRING and String(item.data).begins_with(prefix):
			map[key] = SynxValue.make_string(String(item.data).substr(prefix.length()).strip_edges())
			return
	if arr.size() > 0:
		var first: SynxValue = arr[0]
		if first.kind == SynxValue.Kind.STRING:
			var s := String(first.data)
			var sp := s.find(" ")
			if sp >= 0:
				map[key] = SynxValue.make_string(s.substr(sp + 1).strip_edges())
			else:
				map[key] = first
		else:
			map[key] = first
	else:
		map[key] = SynxValue.make_null()


# ─── :split / :join ──

static func _apply_split(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var idx := markers.find("split")
	var sep := _delimiter_from_keyword(String(markers[idx + 1])) if idx + 1 < markers.size() else ","
	var items: Array = []
	for part in String(v.data).split(sep):
		var p := String(part).strip_edges()
		if not p.is_empty():
			items.append(_cast_primitive(p))
	map[key] = SynxValue.make_array(items)


static func _apply_join(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.ARRAY:
		return
	var idx := markers.find("join")
	var sep := _delimiter_from_keyword(String(markers[idx + 1])) if idx + 1 < markers.size() else ","
	var parts: Array[String] = []
	for item in v.data:
		parts.append(_value_to_string(item))
	map[key] = SynxValue.make_string(sep.join(parts))


static func _delimiter_from_keyword(kw: String) -> String:
	match kw:
		"space": return " "
		"pipe": return "|"
		"dash": return "-"
		"dot": return "."
		"semi": return ";"
		"tab": return "\t"
		"slash": return "/"
	return kw


# ─── :default (standalone) ──

static func _apply_default_standalone(map: Dictionary, key: String, meta: SynxMeta, markers: PackedStringArray) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	var is_empty := v.kind == SynxValue.Kind.NULL or (v.kind == SynxValue.Kind.STRING and String(v.data).is_empty())
	if not is_empty:
		return
	var idx := markers.find("default")
	if idx + 1 >= markers.size():
		return
	var parts: Array[String] = []
	for j in range(idx + 1, markers.size()):
		parts.append(String(markers[j]))
	var fallback := ":".join(parts)
	map[key] = SynxValue.make_string(fallback) if meta.type_hint == "string" else _cast_primitive(fallback)


# ─── :clamp ──

static func _apply_clamp(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var idx := markers.find("clamp")
	var min_s := String(markers[idx + 1]) if idx + 1 < markers.size() else ""
	var max_s := String(markers[idx + 2]) if idx + 2 < markers.size() else ""
	if not (min_s.is_valid_float() and max_s.is_valid_float()):
		return
	var lo := min_s.to_float()
	var hi := max_s.to_float()
	if lo > hi:
		map[key] = SynxValue.make_string("CONSTRAINT_ERR: clamp min (%s) > max (%s)" % [min_s, max_s])
		return
	var n := _as_number(map.get(key, SynxValue.make_null()))
	if n == null:
		return
	map[key] = _coerce_calc_number(clampf(float(n), lo, hi))


# ─── :round ──

static func _apply_round(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var idx := markers.find("round")
	var decimals := int(markers[idx + 1].to_int()) if idx + 1 < markers.size() else 0
	var n := _as_number(map.get(key, SynxValue.make_null()))
	if n == null:
		return
	var factor := pow(10.0, decimals)
	var r := round(float(n) * factor) / factor
	if decimals == 0:
		map[key] = SynxValue.make_int(int(r))
	else:
		map[key] = SynxValue.make_float(r)


# ─── :map ──

static func _apply_map_lookup(map: Dictionary, key: String, markers: PackedStringArray, root: SynxValue) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.ARRAY:
		return
	var idx := markers.find("map")
	var source_key := String(markers[idx + 1]) if idx + 1 < markers.size() else ""
	var lookup_val := ""
	if not source_key.is_empty():
		var r: Variant = SynxEngine.deep_get(root, source_key)
		if r == null and map.has(source_key):
			r = map[source_key]
		lookup_val = _value_to_string(r) if r != null else ""
	else:
		if v.kind == SynxValue.Kind.STRING:
			lookup_val = String(v.data)
	var result: SynxValue = SynxValue.make_null()
	for item in v.data:
		if item is SynxValue and item.kind == SynxValue.Kind.STRING:
			var s := String(item.data)
			var sp := s.find(" ")
			if sp >= 0 and s.substr(0, sp).strip_edges() == lookup_val:
				result = _cast_primitive(s.substr(sp + 1).strip_edges())
				break
	map[key] = result


# ─── :format ──

static func _apply_format(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var idx := markers.find("format")
	var pattern := String(markers[idx + 1]) if idx + 1 < markers.size() else "%s"
	var current: SynxValue = map.get(key, SynxValue.make_null())
	map[key] = SynxValue.make_string(_apply_format_pattern(pattern, current))


static func _apply_format_pattern(pattern: String, value: SynxValue) -> String:
	match value.kind:
		SynxValue.Kind.INT:
			if "d" in pattern or "i" in pattern:
				return _format_int_pattern(pattern, int(value.data))
			if "f" in pattern or "e" in pattern:
				return _format_float_pattern(pattern, float(value.data))
			return str(value.data)
		SynxValue.Kind.FLOAT:
			if "f" in pattern or "e" in pattern:
				return _format_float_pattern(pattern, float(value.data))
			return _format_number(value.data)
		SynxValue.Kind.STRING:
			return String(value.data)
	return _value_to_string(value)


static func _format_int_pattern(pattern: String, n: int) -> String:
	const MAX_W := 4096
	if pattern.begins_with("%") and (pattern.ends_with("d") or pattern.ends_with("i")):
		var inner := pattern.substr(1, pattern.length() - 2)
		var zero_pad := inner.begins_with("0")
		var width_s := inner.substr(1) if zero_pad else inner
		if width_s.is_valid_int():
			var w := min(width_s.to_int(), MAX_W)
			var s := str(n)
			var pad := w - s.length()
			if pad > 0:
				return ("0" if zero_pad else " ").repeat(pad) + s
			return s
	return str(n)


static func _format_float_pattern(pattern: String, f: float) -> String:
	const MAX_P := 1024
	if pattern == "%e":
		if f == 0.0:
			return "0e+0"
		return "%e" % f
	if pattern.begins_with("%.") and (pattern.ends_with("f") or pattern.ends_with("e")):
		var inner := pattern.substr(2, pattern.length() - 3)
		if inner.is_valid_int():
			var p := min(inner.to_int(), MAX_P)
			if pattern.ends_with("e"):
				return "%.*e" % [p, f]
			return "%.*f" % [p, f]
	return str(f)


# ─── :replace ──

static func _apply_replace(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var idx := markers.find("replace")
	var from_s := String(markers[idx + 1]) if idx + 1 < markers.size() else ""
	var to_s := String(markers[idx + 2]) if idx + 2 < markers.size() else ""
	if from_s.is_empty():
		return
	map[key] = SynxValue.make_string(String(v.data).replace(from_s, to_s))


# ─── :sort / :sort:desc ──

static func _apply_sort(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.ARRAY:
		return
	var idx := markers.find("sort")
	var desc := idx + 1 < markers.size() and String(markers[idx + 1]) == "desc"
	var sorted: Array = v.data.duplicate()
	sorted.sort_custom(func(a, b):
		var an = SynxEngineMarkers._as_number(a)
		var bn = SynxEngineMarkers._as_number(b)
		if an != null and bn != null:
			return float(an) < float(bn)
		return SynxEngineMarkers._value_to_string(a) < SynxEngineMarkers._value_to_string(b)
	)
	if desc:
		sorted.reverse()
	map[key] = SynxValue.make_array(sorted)


# ─── :sum ──

static func _apply_sum(map: Dictionary, key: String) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.ARRAY:
		return
	var total := 0.0
	var all_int := true
	for item in v.data:
		match (item as SynxValue).kind:
			SynxValue.Kind.INT:
				total += int(item.data)
			SynxValue.Kind.FLOAT:
				total += float(item.data)
				if fmod(float(item.data), 1.0) != 0.0:
					all_int = false
			SynxValue.Kind.STRING:
				var s := String(item.data)
				if s.is_valid_float():
					var f := s.to_float()
					total += f
					if fmod(f, 1.0) != 0.0:
						all_int = false
	if all_int and fmod(total, 1.0) == 0.0 and abs(total) < 9.22e18:
		map[key] = SynxValue.make_int(int(total))
	else:
		map[key] = SynxValue.make_float(total)


# ─── :fallback ──

static func _apply_fallback(map: Dictionary, key: String, markers: PackedStringArray, options: SynxOptions) -> void:
	var idx := markers.find("fallback")
	var def_v := String(markers[idx + 1]) if idx + 1 < markers.size() else ""
	var v: SynxValue = map.get(key, SynxValue.make_null())
	var use_fallback := false
	if v.kind == SynxValue.Kind.NULL:
		use_fallback = true
	elif v.kind == SynxValue.Kind.STRING:
		var s := String(v.data)
		if s.is_empty():
			use_fallback = true
		else:
			var jail := SynxEngine.jail_path(SynxEngine._effective_base(options), s)
			if not bool(jail["ok"]):
				use_fallback = true
			elif not FileAccess.file_exists(jail["path"]):
				use_fallback = true
	if use_fallback and not def_v.is_empty():
		map[key] = SynxValue.make_string(def_v)


# ─── :once ──

static func _apply_once(map: Dictionary, key: String, markers: PackedStringArray, options: SynxOptions) -> void:
	var idx := markers.find("once")
	var gen_type := String(markers[idx + 1]) if idx + 1 < markers.size() else "uuid"
	var base := SynxEngine._effective_base(options)
	var lock_path := base.rstrip("/\\") + "/.synx.lock"
	var existing := _read_lock_value(lock_path, key)
	if existing != null:
		map[key] = SynxValue.make_string(existing)
		return
	var generated := ""
	match gen_type:
		"timestamp":
			generated = str(int(Time.get_unix_time_from_system()))
		"random":
			generated = str(SynxRng.random_usize(0xFFFFFFFF))
		_:
			generated = SynxRng.generate_uuid()
	_write_lock_value(lock_path, key, generated)
	map[key] = SynxValue.make_string(generated)


static func _read_lock_value(lock_path: String, key: String) -> Variant:
	if not FileAccess.file_exists(lock_path):
		return null
	var f := FileAccess.open(lock_path, FileAccess.READ)
	if f == null:
		return null
	var text := f.get_as_text()
	f.close()
	for line in text.split("\n"):
		var ls := String(line)
		if ls.begins_with(key + " "):
			return ls.substr(key.length() + 1).strip_edges()
	return null


static func _write_lock_value(lock_path: String, key: String, value: String) -> void:
	var text := ""
	if FileAccess.file_exists(lock_path):
		var f := FileAccess.open(lock_path, FileAccess.READ)
		if f != null:
			text = f.get_as_text()
			f.close()
	var lines := text.split("\n", false)
	var new_line := "%s %s" % [key, value]
	var found := false
	var out_lines: Array[String] = []
	for line in lines:
		var ls := String(line)
		if ls.begins_with(key + " "):
			out_lines.append(new_line)
			found = true
		else:
			out_lines.append(ls)
	if not found:
		out_lines.append(new_line)
	var w := FileAccess.open(lock_path, FileAccess.WRITE)
	if w != null:
		w.store_string("\n".join(out_lines) + "\n")
		w.close()


# ─── :version ──

static func _apply_version(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var idx := markers.find("version")
	var op := String(markers[idx + 1]) if idx + 1 < markers.size() else ">="
	var required := String(markers[idx + 2]) if idx + 2 < markers.size() else ""
	map[key] = SynxValue.make_bool(_compare_versions(String(v.data), op, required))


static func _compare_versions(current: String, op: String, required: String) -> bool:
	var cv: Array[int] = []
	for p in current.split("."):
		if String(p).is_valid_int():
			cv.append(String(p).to_int())
	var rv: Array[int] = []
	for p in required.split("."):
		if String(p).is_valid_int():
			rv.append(String(p).to_int())
	var n := max(cv.size(), rv.size())
	var ord := 0
	for i in n:
		var a := cv[i] if i < cv.size() else 0
		var b := rv[i] if i < rv.size() else 0
		if a != b:
			ord = -1 if a < b else 1
			break
	match op:
		">=": return ord >= 0
		"<=": return ord <= 0
		">":  return ord > 0
		"<":  return ord < 0
		"==", "=": return ord == 0
		"!=": return ord != 0
	return false


# ─── :watch ──

static func _apply_watch(map: Dictionary, key: String, markers: PackedStringArray, options: SynxOptions) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v.kind != SynxValue.Kind.STRING:
		return
	var rel := String(v.data)
	var max_depth := options.max_include_depth if options.max_include_depth > 0 else SynxEngine.DEFAULT_MAX_INCLUDE_DEPTH
	if options._include_depth >= max_depth:
		map[key] = SynxValue.make_string("WATCH_ERR: max include depth (%d) exceeded" % max_depth)
		return
	var jail := SynxEngine.jail_path(SynxEngine._effective_base(options), rel)
	if not bool(jail["ok"]):
		map[key] = SynxValue.make_string("WATCH_ERR: %s" % jail["error"])
		return
	var full: String = jail["path"]
	if not FileAccess.file_exists(full):
		map[key] = SynxValue.make_string("WATCH_ERR: file not found: %s" % rel)
		return
	if SynxEngine._file_size(full) > SynxEngine.MAX_FILE_SIZE:
		map[key] = SynxValue.make_string("WATCH_ERR: file too large")
		return
	var f := FileAccess.open(full, FileAccess.READ)
	if f == null:
		map[key] = SynxValue.make_string("WATCH_ERR: cannot open %s" % rel)
		return
	var content := f.get_as_text()
	f.close()

	var idx := markers.find("watch")
	var key_path := String(markers[idx + 1]) if idx + 1 < markers.size() else ""
	if key_path.is_empty():
		map[key] = SynxValue.make_string(content.strip_edges())
		return

	var ext := ""
	var dot := full.rfind(".")
	if dot >= 0:
		ext = full.substr(dot + 1).to_lower()
	if ext == "json":
		var parsed = JSON.parse_string(content)
		if parsed == null:
			map[key] = SynxValue.make_null()
			return
		var current = parsed
		for part in key_path.split("."):
			if current is Dictionary and current.has(part):
				current = current[part]
			else:
				map[key] = SynxValue.make_null()
				return
		map[key] = SynxValue.from_variant(current)
		return

	# Default: SYNX file.
	var inner := SynxParser.parse(content)
	var cur: SynxValue = inner.root
	for part in key_path.split("."):
		if cur.kind == SynxValue.Kind.OBJECT and cur.data.has(part):
			cur = cur.data[part]
		else:
			map[key] = SynxValue.make_null()
			return
	map[key] = cur


# ─── :prompt ──

static func _apply_prompt(map: Dictionary, key: String, markers: PackedStringArray) -> void:
	var idx := markers.find("prompt")
	var label := String(markers[idx + 1]) if idx + 1 < markers.size() else key
	var v: SynxValue = map.get(key, SynxValue.make_null())
	var synx_text := SynxStringify.stringify(v)
	map[key] = SynxValue.make_string("%s (SYNX):\n```synx\n%s```" % [label, synx_text])


# ─── :inherit pre-pass ──

static func apply_inheritance(root: SynxValue, metadata: Dictionary) -> void:
	if root.kind != SynxValue.Kind.OBJECT:
		return
	if not metadata.has(""):
		return
	var root_meta: Dictionary = metadata[""]
	var root_map: Dictionary = root.data

	var inherits: Array = []  # Array of [child_key, Array[parent_name]]
	for key in root_meta.keys():
		var m: SynxMeta = root_meta[key]
		if not m.has_marker("inherit"):
			continue
		var idx := m.marker_index("inherit")
		var parents: Array[String] = []
		for j in range(idx + 1, m.markers.size()):
			parents.append(String(m.markers[j]))
		if parents.is_empty() and m.args.size() > 0:
			for a in m.args:
				parents.append(String(a))
		if parents.size() > 0:
			inherits.append([String(key), parents])

	for entry in inherits:
		var child_key: String = entry[0]
		var parents: Array = entry[1]
		var merged: Dictionary = {}
		for parent_name in parents:
			if root_map.has(parent_name):
				var p: SynxValue = root_map[parent_name]
				if p.kind == SynxValue.Kind.OBJECT:
					for k in p.data.keys():
						merged[k] = p.data[k]
		if root_map.has(child_key):
			var c: SynxValue = root_map[child_key]
			if c.kind == SynxValue.Kind.OBJECT:
				for k in c.data.keys():
					merged[k] = c.data[k]
		root_map[child_key] = SynxValue.make_object(merged)


# ─── Per-key constraint validation ──

static func validate_per_key_constraints(map: Dictionary, key: String, meta: SynxMeta) -> void:
	var v: SynxValue = map.get(key, SynxValue.make_null())
	if v == null or v.kind == SynxValue.Kind.NULL:
		if meta.c_required:
			map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' is required" % key)
		return
	if meta.c_required:
		var empty := false
		if v.kind == SynxValue.Kind.NULL:
			empty = true
		elif v.kind == SynxValue.Kind.STRING and String(v.data).is_empty():
			empty = true
		if empty:
			map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' is required" % key)
			return
	if not meta.c_type_name.is_empty():
		if not _value_matches_type(v, meta.c_type_name):
			map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' expected type '%s'" % [key, meta.c_type_name])
			return
	if meta.c_has_enum:
		var s := _value_to_string(v)
		var matched := false
		for ev in meta.c_enum:
			if String(ev) == s:
				matched = true
				break
		if not matched:
			map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' must be one of [%s]" % [key, "|".join(meta.c_enum)])
			return
	if not is_nan(meta.c_min) or not is_nan(meta.c_max):
		var num: Variant = null
		match v.kind:
			SynxValue.Kind.INT, SynxValue.Kind.FLOAT:
				num = float(v.data)
			SynxValue.Kind.STRING:
				num = float(String(v.data).length())
		if num != null:
			if not is_nan(meta.c_min) and float(num) < meta.c_min:
				map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' value %s is below min %s" % [key, _format_number(num), _format_number(meta.c_min)])
				return
			if not is_nan(meta.c_max) and float(num) > meta.c_max:
				map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' value %s exceeds max %s" % [key, _format_number(num), _format_number(meta.c_max)])
				return
	if not meta.c_pattern.is_empty() and meta.c_pattern.length() <= 256:
		if v.kind == SynxValue.Kind.STRING:
			var rx := RegEx.new()
			if rx.compile(meta.c_pattern) == OK:
				if rx.search(String(v.data)) == null:
					map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' does not match pattern /%s/" % [key, meta.c_pattern])
					return


# ─── Whole-tree constraint validation (global by field name) ──

static func build_constraint_registry(metadata: Dictionary) -> Dictionary:
	var registry: Dictionary = {}
	for path in metadata.keys():
		var mm: Dictionary = metadata[path]
		for key in mm.keys():
			var meta: SynxMeta = mm[key]
			if not meta.has_constraints:
				continue
			if not registry.has(key):
				registry[key] = meta.clone()
			else:
				var existing: SynxMeta = registry[key]
				_merge_constraints(existing, meta)
	return registry


static func _merge_constraints(base: SynxMeta, incoming: SynxMeta) -> void:
	if incoming.c_required: base.c_required = true
	if incoming.c_readonly: base.c_readonly = true
	if not is_nan(incoming.c_min):
		base.c_min = max(base.c_min, incoming.c_min) if not is_nan(base.c_min) else incoming.c_min
	if not is_nan(incoming.c_max):
		base.c_max = min(base.c_max, incoming.c_max) if not is_nan(base.c_max) else incoming.c_max
	if base.c_type_name.is_empty(): base.c_type_name = incoming.c_type_name
	if base.c_pattern.is_empty(): base.c_pattern = incoming.c_pattern
	if not base.c_has_enum and incoming.c_has_enum:
		base.c_enum = incoming.c_enum.duplicate()
		base.c_has_enum = true


static func validate_field_constraints(value: SynxValue, registry: Dictionary) -> void:
	if value.kind != SynxValue.Kind.OBJECT:
		return
	var map: Dictionary = value.data
	for key in map.keys():
		if registry.has(key):
			var v: SynxValue = map[key]
			var already := v.kind == SynxValue.Kind.STRING and (String(v.data).begins_with("CONSTRAINT_ERR:") or String(v.data).begins_with("TYPE_ERR:"))
			if not already:
				validate_per_key_constraints(map, String(key), registry[key])
		var child: SynxValue = map[key]
		match child.kind:
			SynxValue.Kind.OBJECT:
				validate_field_constraints(child, registry)
			SynxValue.Kind.ARRAY:
				for item in child.data:
					if item is SynxValue and item.kind == SynxValue.Kind.OBJECT:
						validate_field_constraints(item, registry)


# ─── Whole-tree type validation ──

static func build_type_registry(metadata: Dictionary) -> Dictionary:
	var registry: Dictionary = {}
	for path in metadata.keys():
		var mm: Dictionary = metadata[path]
		for key in mm.keys():
			var meta: SynxMeta = mm[key]
			if not meta.type_hint.is_empty() and not registry.has(key):
				registry[key] = meta.type_hint
	return registry


static func validate_field_types(value: SynxValue, registry: Dictionary) -> void:
	if value.kind != SynxValue.Kind.OBJECT:
		return
	var map: Dictionary = value.data
	for key in map.keys():
		if registry.has(key):
			var expected: String = registry[key]
			var v: SynxValue = map[key]
			if not _value_matches_type(v, expected):
				map[key] = SynxValue.make_string("TYPE_ERR: '%s' expected %s but got %s" % [key, expected, v.type_name()])
		var child: SynxValue = map[key]
		match child.kind:
			SynxValue.Kind.OBJECT:
				validate_field_types(child, registry)
			SynxValue.Kind.ARRAY:
				for item in child.data:
					if item is SynxValue and item.kind == SynxValue.Kind.OBJECT:
						validate_field_types(item, registry)


static func _value_matches_type(v: SynxValue, expected: String) -> bool:
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


# ─── Misc helpers ──

static func _as_number(v) -> Variant:
	if v == null or not (v is SynxValue):
		return null
	if v.kind == SynxValue.Kind.INT:
		return float(v.data)
	if v.kind == SynxValue.Kind.FLOAT:
		return float(v.data)
	return null


static func _value_to_string(v) -> String:
	if v == null:
		return ""
	if not (v is SynxValue):
		return String(v)
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
	return ""


static func _format_number(n) -> String:
	var f: float = float(n)
	if absf(f - floorf(f)) < 1e-12 and absf(f) < 9.22e18:
		return str(int(f))
	return str(f)


static func _coerce_calc_number(f: float) -> SynxValue:
	if absf(f - floorf(f)) < 1e-12 and absf(f) < 9.22e18:
		return SynxValue.make_int(int(f))
	return SynxValue.make_float(f)


static func _cast_primitive(s: String) -> SynxValue:
	if s.length() >= 2:
		var c0 := s.unicode_at(0)
		var cN := s.unicode_at(s.length() - 1)
		if (c0 == 34 and cN == 34) or (c0 == 39 and cN == 39):
			return SynxValue.make_string(s.substr(1, s.length() - 2))
	if s == "true": return SynxValue.make_bool(true)
	if s == "false": return SynxValue.make_bool(false)
	if s == "null": return SynxValue.make_null()
	if s.is_valid_int(): return SynxValue.make_int(s.to_int())
	if s.is_valid_float(): return SynxValue.make_float(s.to_float())
	return SynxValue.make_string(s)


static func _is_word_char(b: int) -> bool:
	return (b >= 48 and b <= 57) or (b >= 65 and b <= 90) or (b >= 97 and b <= 122) or b == 95


# Replace whole-word occurrences (word boundary = !word_char on both sides).
static func _replace_word(haystack: String, word: String, replacement: String) -> String:
	var hb := haystack.length()
	var wb := word.length()
	if wb > hb or wb == 0:
		return haystack
	var out := ""
	var i := 0
	while i <= hb - wb:
		if haystack.substr(i, wb) == word:
			var before_ok := i == 0 or not _is_word_char(haystack.unicode_at(i - 1))
			var after_ok := i + wb >= hb or not _is_word_char(haystack.unicode_at(i + wb))
			if before_ok and after_ok:
				out += replacement
				i += wb
				continue
		out += haystack.substr(i, 1)
		i += 1
	while i < hb:
		out += haystack.substr(i, 1)
		i += 1
	return out
