@tool
class_name SynxEngineMarkers
extends RefCounted

# Marker dispatch and constraint checks — synx-core `apply_markers`,
# `constraint_violation` and their helpers, in the same order and with the
# same texts. `obj` is the object that holds `key`; markers replace
# `obj.data[key]`.

const MAX_CALC_EXPR_LEN: int = 4096
const MAX_CALC_RESOLVED_LEN: int = 64 * 1024

const _BUILTIN_MARKERS := [
	"spam", "include", "import", "env", "random", "ref", "i18n", "calc",
	"alias", "secret", "unique", "geo", "split", "join", "default", "clamp",
	"round", "map", "format", "replace", "sort", "sum", "fallback", "once",
	"version", "watch", "prompt", "vision", "audio", "inherit", "template",
]

static var _pattern_cache: Dictionary = {}


static func apply_markers(obj: SynxValue, key: String, meta: SynxMeta, root: SynxValue, options: SynxOptions, path: String, includes: Dictionary, calc_special: PackedStringArray) -> void:
	var map: Dictionary = obj.data
	var markers := meta.markers
	var mb := meta.bits

	# ── :spam ──
	if (mb & SynxMeta.MK_SPAM) != 0:
		var si := markers.find("spam")
		var max_calls := SynxNum.parse_u64(markers[si + 1]) if si + 1 < markers.size() else -1
		if max_calls < 0:
			max_calls = 0
		var window := SynxNum.parse_u64(markers[si + 2]) if si + 2 < markers.size() else -1
		if window < 0:
			window = 1
		if max_calls == 0:
			map[key] = SynxValue.make_string("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]")
			return
		var cur: SynxValue = map.get(key)
		var target := SynxEngine.value_to_string(cur) if cur != null else key
		if not SynxEngine.allow_spam_access("%s::%s" % [key, target], max_calls, window):
			map[key] = SynxValue.make_string("SPAM_ERR: '%s' exceeded %d calls per %ds" % [target, max_calls, window])
			return
		if cur != null:
			var t := SynxEngine.value_to_string(cur)
			var resolved := SynxEngine.deep_get(root, t, true)
			if resolved == null:
				resolved = _copy(map.get(t))
			if resolved != null:
				map[key] = resolved

	# ── :include / :import ──
	if (mb & (SynxMeta.MK_INCLUDE | SynxMeta.MK_IMPORT)) != 0:
		var iv: SynxValue = map.get(key)
		if iv != null and iv.kind == SynxValue.Kind.STRING:
			_apply_include(map, key, String(iv.data), options)
		return

	# ── :env ──
	if (mb & SynxMeta.MK_ENV) != 0:
		var ev: SynxValue = map.get(key)
		if ev != null and ev.kind == SynxValue.Kind.STRING:
			var var_name: String = ev.data
			var env_val := ""
			if options.env != null:
				env_val = String((options.env as Dictionary).get(var_name, ""))
			else:
				env_val = OS.get_environment(var_name)
			var force_string := wants_string(meta)
			var di := markers.find("default")
			if not env_val.is_empty():
				map[key] = SynxValue.make_string(env_val) if force_string else cast_primitive(env_val)
			elif di >= 0 and markers.size() > di + 1:
				var fallback := ":".join(markers.slice(di + 1))
				map[key] = SynxValue.make_string(fallback) if force_string else cast_primitive(fallback)
			else:
				map[key] = SynxValue.make_null()

	# ── :random ──
	if (mb & SynxMeta.MK_RANDOM) != 0:
		var rv: SynxValue = map.get(key)
		if rv != null and rv.kind == SynxValue.Kind.ARRAY:
			var arr: Array = rv.data
			if arr.is_empty():
				map[key] = SynxValue.make_null()
				return
			if not meta.args.is_empty():
				var weights := PackedFloat64Array()
				for a in meta.args:
					var w: Variant = SynxNum.parse_f64(a)
					if w != null:
						weights.append(w)
				map[key] = _copy(_weighted_random(arr, weights))
			else:
				map[key] = _copy(arr[SynxRng.random_usize(arr.size())])

	# ── :ref ──
	if (mb & SynxMeta.MK_REF) != 0:
		var fv: SynxValue = map.get(key)
		if fv != null and fv.kind == SynxValue.Kind.STRING:
			var target2: String = fv.data
			var resolved2 := SynxEngine.deep_get(root, target2, true)
			if resolved2 == null:
				resolved2 = _copy(map.get(target2))
			if resolved2 == null:
				resolved2 = SynxValue.make_null()
			var done := false
			if (mb & SynxMeta.MK_CALC) != 0 and resolved2.is_number():
				var ci := markers.find("calc")
				if ci + 1 < markers.size():
					var calc_expr := markers[ci + 1]
					if not calc_expr.is_empty() and "+-*/%".contains(calc_expr.substr(0, 1)):
						var expr := SynxNum.format_number(float(resolved2.data)) + " " + calc_expr
						var res := SynxSafeCalc.evaluate(expr)
						if bool(res["ok"]):
							map[key] = _calc_value(res["value"])
						else:
							map[key] = SynxValue.make_string("CALC_ERR: %s" % res["error"])
						done = true
			if not done:
				map[key] = resolved2

	# ── :i18n ──
	if (mb & SynxMeta.MK_I18N) != 0:
		var tv: SynxValue = map.get(key)
		if tv != null and tv.kind == SynxValue.Kind.OBJECT:
			var translations: Dictionary = tv.data
			var lang := options.lang if not options.lang.is_empty() else "en"
			var val: SynxValue = translations.get(lang)
			if val == null:
				val = translations.get("en")
			if val == null and not translations.is_empty():
				val = translations.values()[0]
			val = _copy(val) if val != null else SynxValue.make_null()
			var ii := markers.find("i18n")
			if ii + 1 < markers.size() and val.kind == SynxValue.Kind.OBJECT:
				var cf := markers[ii + 1]
				var count_f := 0.0
				var cv: SynxValue = map.get(cf)
				if cv != null and cv.is_number():
					count_f = float(cv.data)
				else:
					var rc := SynxEngine.deep_get(root, cf)
					if rc != null and rc.is_number():
						count_f = float(rc.data)
				var count := SynxNum.to_i64(count_f)
				var forms: Dictionary = val.data
				var category := plural_category(lang, count)
				var chosen: SynxValue = forms.get(category)
				if chosen == null:
					chosen = forms.get("other")
				if chosen == null and not forms.is_empty():
					chosen = forms.values()[0]
				if chosen == null:
					chosen = SynxValue.make_null()
				if chosen.kind == SynxValue.Kind.STRING:
					map[key] = SynxValue.make_string(String(chosen.data).replace("{count}", str(count)))
				else:
					map[key] = _copy(chosen)
			else:
				map[key] = val

	# ── :calc ──
	if (mb & SynxMeta.MK_CALC) != 0:
		var cv2: SynxValue = map.get(key)
		if cv2 != null and cv2.kind == SynxValue.Kind.STRING:
			var expr2: String = cv2.data
			var expr_bytes := expr2.length()
			if expr2.length() * 4 > MAX_CALC_EXPR_LEN:
				expr_bytes = SynxText.utf8_len(expr2)
			if expr_bytes > MAX_CALC_EXPR_LEN:
				map[key] = SynxValue.make_string("CALC_ERR: expression too long (%d chars, max %d)" % [expr_bytes, MAX_CALC_EXPR_LEN])
				return
			var root_map: Dictionary = root.data if root.kind == SynxValue.Kind.OBJECT else {}
			var sub: Variant = substitute_calc_vars(expr2, expr_bytes, map, key, root_map, root.kind == SynxValue.Kind.OBJECT, obj == root, calc_special)
			if sub == null:
				map[key] = SynxValue.make_string("CALC_ERR: resolved expression too long (max %d bytes)" % MAX_CALC_RESOLVED_LEN)
				return
			var dotted: Variant = _substitute_dot_paths(sub[0], sub[1], root)
			if dotted == null:
				map[key] = SynxValue.make_string("CALC_ERR: resolved expression too long (max %d bytes)" % MAX_CALC_RESOLVED_LEN)
				return
			var res2 := SynxSafeCalc.evaluate(dotted)
			if bool(res2["ok"]):
				map[key] = _calc_value(res2["value"])
			else:
				map[key] = SynxValue.make_string("CALC_ERR: %s" % res2["error"])

	# ── :alias ──
	if (mb & SynxMeta.MK_ALIAS) != 0:
		var av: SynxValue = map.get(key)
		if av != null and av.kind == SynxValue.Kind.STRING:
			var target3: String = av.data
			var current_path := key if path.is_empty() else path + "." + key
			if target3 == key or target3 == current_path:
				map[key] = SynxValue.make_string("ALIAS_ERR: self-referential alias: %s → %s" % [current_path, target3])
			else:
				var target_val := SynxEngine.deep_get(root, target3, true)
				var dot := target3.rfind(".")
				var target_parent := target3.substr(0, dot) if dot >= 0 else ""
				var target_key := target3.substr(dot + 1) if dot >= 0 else target3
				var target_has_alias := false
				var holder := _meta_holder(root, target_parent)
				if holder != null and holder.meta != null:
					var tm: SynxMeta = (holder.meta as Dictionary).get(target_key)
					target_has_alias = tm != null and tm.markers.has("alias")
				var is_cycle := target_has_alias and target_val != null and target_val.kind == SynxValue.Kind.STRING \
					and (String(target_val.data) == key or String(target_val.data) == current_path)
				if is_cycle:
					var a := current_path
					var b := target3
					if b < a:
						a = target3
						b = current_path
					map[key] = SynxValue.make_string("ALIAS_ERR: circular alias detected: %s → %s" % [a, b])
				else:
					map[key] = target_val if target_val != null else SynxValue.make_null()

	# ── :secret ──
	if (mb & SynxMeta.MK_SECRET) != 0:
		var sv: SynxValue = map.get(key)
		if sv != null and sv.kind != SynxValue.Kind.SECRET:
			map[key] = SynxValue.make_secret(SynxEngine.value_to_string(sv))

	# ── :unique ──
	if (mb & SynxMeta.MK_UNIQUE) != 0:
		var uv: SynxValue = map.get(key)
		if uv != null and uv.kind == SynxValue.Kind.ARRAY:
			var seen: Dictionary = {}
			var unique: Array = []
			for item in uv.data:
				var s := SynxEngine.value_to_string(item)
				if not seen.has(s):
					seen[s] = true
					unique.append(item)
			map[key] = SynxValue.make_array(unique)

	# ── :geo ──
	if (mb & SynxMeta.MK_GEO) != 0:
		var gv: SynxValue = map.get(key)
		if gv != null and gv.kind == SynxValue.Kind.ARRAY:
			var garr: Array = gv.data
			var region := options.region if not options.region.is_empty() else "US"
			var prefix := region + " "
			var result: SynxValue = null
			for item in garr:
				var it: SynxValue = item
				if it.kind == SynxValue.Kind.STRING and String(it.data).begins_with(prefix):
					result = SynxValue.make_string(SynxText.trim(String(it.data).substr(prefix.length())))
					break
			if result == null:
				if garr.is_empty():
					result = SynxValue.make_null()
				else:
					var first: SynxValue = garr[0]
					var sp := String(first.data).find(" ") if first.kind == SynxValue.Kind.STRING else -1
					if sp >= 0:
						result = SynxValue.make_string(SynxText.trim(String(first.data).substr(sp + 1)))
					else:
						result = _copy(first)
			map[key] = result

	# ── :split ──
	if (mb & SynxMeta.MK_SPLIT) != 0:
		var spv: SynxValue = map.get(key)
		if spv != null and spv.kind == SynxValue.Kind.STRING:
			var spi := markers.find("split")
			var sep := delimiter_from_keyword(markers[spi + 1]) if spi + 1 < markers.size() else ","
			var items: Array = []
			var parts: PackedStringArray
			if sep.is_empty():
				parts = PackedStringArray()
				var s2: String = spv.data
				for ci2 in s2.length():
					parts.append(s2.substr(ci2, 1))
			else:
				parts = String(spv.data).split(sep)
			for part in parts:
				var p := SynxText.trim(part)
				if not p.is_empty():
					items.append(cast_primitive(p))
			map[key] = SynxValue.make_array(items)

	# ── :join ──
	if (mb & SynxMeta.MK_JOIN) != 0:
		var jv: SynxValue = map.get(key)
		if jv != null and jv.kind == SynxValue.Kind.ARRAY:
			var ji := markers.find("join")
			var jsep := delimiter_from_keyword(markers[ji + 1]) if ji + 1 < markers.size() else ","
			var texts := PackedStringArray()
			for item in jv.data:
				texts.append(SynxEngine.value_to_string(item))
			map[key] = SynxValue.make_string(jsep.join(texts))

	# ── :default (standalone, without :env) ──
	if (mb & SynxMeta.MK_DEFAULT) != 0 and (mb & SynxMeta.MK_ENV) == 0:
		var dv: SynxValue = map.get(key)
		var is_empty := dv == null or dv.kind == SynxValue.Kind.NULL or (dv.kind == SynxValue.Kind.STRING and String(dv.data).is_empty())
		if is_empty:
			var di2 := markers.find("default")
			if markers.size() > di2 + 1:
				var fb := ":".join(markers.slice(di2 + 1))
				map[key] = SynxValue.make_string(fb) if wants_string(meta) else cast_primitive(fb)

	# ── :clamp ──
	if (mb & SynxMeta.MK_CLAMP) != 0:
		var cli := markers.find("clamp")
		var lo: Variant = SynxNum.parse_f64(markers[cli + 1] if cli + 1 < markers.size() else "")
		var hi: Variant = SynxNum.parse_f64(markers[cli + 2] if cli + 2 < markers.size() else "")
		# A bound must be a finite number (`nan`, `inf`, `1e400` skip the marker).
		if lo != null and hi != null and is_finite(lo) and is_finite(hi):
			if float(lo) > float(hi):
				map[key] = SynxValue.make_string("CONSTRAINT_ERR: clamp min (%s) > max (%s)" % [SynxNum.display(lo), SynxNum.display(hi)])
			else:
				var clv: SynxValue = map.get(key)
				if clv != null and clv.is_number():
					var n := float(clv.data)
					var clamped := n
					if n < float(lo):
						clamped = lo
					if n > float(hi):
						clamped = hi
					map[key] = _calc_value(clamped)

	# ── :round ──
	if (mb & SynxMeta.MK_ROUND) != 0:
		var ri := markers.find("round")
		var decimals := SynxNum.parse_u32(markers[ri + 1]) if ri + 1 < markers.size() else -1
		if decimals < 0:
			decimals = 0
		var rov: SynxValue = map.get(key)
		if rov != null and rov.is_number():
			var factor := _powi10(decimals)
			var rounded := roundf(float(rov.data) * factor) / factor
			if decimals == 0:
				map[key] = SynxValue.make_int(SynxNum.to_i64(rounded))
			else:
				map[key] = SynxValue.make_float(rounded)

	# ── :map ──
	if (mb & SynxMeta.MK_MAP) != 0:
		var mv: SynxValue = map.get(key)
		if mv != null and mv.kind == SynxValue.Kind.ARRAY:
			var mi := markers.find("map")
			var source_key := markers[mi + 1] if mi + 1 < markers.size() else ""
			var lookup := ""
			if not source_key.is_empty():
				var lv := SynxEngine.deep_get(root, source_key)
				if lv == null:
					lv = map.get(source_key)
				lookup = SynxEngine.value_to_string(lv) if lv != null else ""
			var found: SynxValue = SynxValue.make_null()
			for item in mv.data:
				var it2: SynxValue = item
				if it2.kind != SynxValue.Kind.STRING:
					continue
				var s3: String = it2.data
				var sp2 := s3.find(" ")
				if sp2 >= 0 and SynxText.trim(s3.substr(0, sp2)) == lookup:
					found = cast_primitive(SynxText.trim(s3.substr(sp2 + 1)))
					break
			map[key] = found

	# ── :format ──
	if (mb & SynxMeta.MK_FORMAT) != 0:
		var foi := markers.find("format")
		var pattern := markers[foi + 1] if foi + 1 < markers.size() else "%s"
		var fov: SynxValue = map.get(key)
		if fov != null:
			map[key] = SynxValue.make_string(apply_format_pattern(pattern, fov))

	# ── :replace:FROM:TO ──
	if (mb & SynxMeta.MK_REPLACE) != 0:
		var rpv: SynxValue = map.get(key)
		if rpv != null and rpv.kind == SynxValue.Kind.STRING:
			var rpi := markers.find("replace")
			var from_s := markers[rpi + 1] if rpi + 1 < markers.size() else ""
			var to_s := markers[rpi + 2] if rpi + 2 < markers.size() else ""
			if not from_s.is_empty():
				map[key] = SynxValue.make_string(String(rpv.data).replace(from_s, to_s))

	# ── :sort / :sort:desc ──
	if (mb & SynxMeta.MK_SORT) != 0:
		var sov: SynxValue = map.get(key)
		if sov != null and sov.kind == SynxValue.Kind.ARRAY:
			var soi := markers.find("sort")
			var desc := soi + 1 < markers.size() and markers[soi + 1] == "desc"
			var sorted := _stable_sort((sov.data as Array).duplicate())
			if desc:
				sorted.reverse()
			map[key] = SynxValue.make_array(sorted)

	# ── :sum ──
	if (mb & SynxMeta.MK_SUM) != 0:
		var suv: SynxValue = map.get(key)
		if suv != null and suv.kind == SynxValue.Kind.ARRAY:
			var total := 0.0
			var all_int := true
			for item in suv.data:
				var it3: SynxValue = item
				match it3.kind:
					SynxValue.Kind.INT:
						total += float(it3.data)
					SynxValue.Kind.FLOAT:
						total += float(it3.data)
						if not SynxNum.is_whole(it3.data):
							all_int = false
					SynxValue.Kind.STRING:
						var pf: Variant = SynxNum.parse_f64(it3.data)
						if pf != null:
							total += float(pf)
							if not SynxNum.is_whole(pf):
								all_int = false
			if all_int and SynxNum.fits_int(total):
				map[key] = SynxValue.make_int(int(total))
			else:
				map[key] = SynxValue.make_float(total)

	# ── :fallback ──
	if (mb & SynxMeta.MK_FALLBACK) != 0:
		var fbi := markers.find("fallback")
		var default_val := markers[fbi + 1] if fbi + 1 < markers.size() else ""
		var fbv: SynxValue = map.get(key)
		var use_fallback := false
		if fbv == null or fbv.kind == SynxValue.Kind.NULL:
			use_fallback = true
		elif fbv.kind == SynxValue.Kind.STRING:
			var fs: String = fbv.data
			if fs.is_empty():
				use_fallback = true
			else:
				var jail := SynxEngine.jail_path(SynxEngine._effective_base(options), fs)
				use_fallback = not bool(jail["ok"]) or (not FileAccess.file_exists(jail["path"]) and not DirAccess.dir_exists_absolute(jail["path"]))
		if use_fallback and not default_val.is_empty():
			map[key] = SynxValue.make_string(default_val)

	# ── :once ──
	if (mb & SynxMeta.MK_ONCE) != 0:
		var oi := markers.find("once")
		var gen_type := markers[oi + 1] if oi + 1 < markers.size() else "uuid"
		var lock_path := SynxEngine._effective_base(options).rstrip("/\\") + "/.synx.lock"
		var existing: Variant = _read_lock_value(lock_path, key)
		if existing != null:
			map[key] = SynxValue.make_string(existing)
		else:
			var generated := ""
			match gen_type:
				"timestamp":
					generated = str(int(Time.get_unix_time_from_system()))
				"random":
					generated = str(SynxRng.random_usize(4294967295))
				_:
					generated = SynxRng.generate_uuid()
			_write_lock_value(lock_path, key, generated)
			map[key] = SynxValue.make_string(generated)

	# ── :version ──
	if (mb & SynxMeta.MK_VERSION) != 0:
		var vv: SynxValue = map.get(key)
		if vv != null and vv.kind == SynxValue.Kind.STRING:
			var vi := markers.find("version")
			var op := markers[vi + 1] if vi + 1 < markers.size() else ">="
			var required := markers[vi + 2] if vi + 2 < markers.size() else ""
			map[key] = SynxValue.make_bool(compare_versions(vv.data, op, required))

	# ── :watch ──
	if (mb & SynxMeta.MK_WATCH) != 0:
		var wv: SynxValue = map.get(key)
		if wv != null and wv.kind == SynxValue.Kind.STRING:
			if not _apply_watch(map, key, String(wv.data), markers, options):
				return

	# ── :prompt ──
	if (mb & SynxMeta.MK_PROMPT) != 0:
		var pi := markers.find("prompt")
		var label := markers[pi + 1] if pi + 1 < markers.size() else key
		var pv: SynxValue = map.get(key)
		if pv != null:
			map[key] = SynxValue.make_string("%s (SYNX):\n```synx\n%s```" % [label, stringify_value(pv, 0)])

	# ── :vision / :audio — metadata only, the value passes through ──

	# ── Custom markers (GDScript Callables in place of WASM packages) ──
	if not options.marker_callables.is_empty():
		for marker in markers:
			if _BUILTIN_MARKERS.has(marker):
				continue
			if options.marker_callables.has(marker):
				var callable: Callable = options.marker_callables[marker]
				var current: SynxValue = map.get(key, SynxValue.make_null())
				var margs := markers.slice(markers.find(marker) + 1)
				var out: Variant = callable.call(current, margs)
				if out is SynxValue:
					map[key] = out
				else:
					map[key] = SynxValue.make_string("CUSTOM_ERR: marker '%s' did not return SynxValue" % marker)
				break

	# ── Constraint validation (always last) ──
	if meta.has_constraints:
		var cur2: SynxValue = map.get(key)
		if cur2 == null:
			if meta.c_required:
				map[key] = SynxValue.make_string("CONSTRAINT_ERR: '%s' is required" % key)
		else:
			var err := constraint_violation(key, cur2, meta)
			if not err.is_empty():
				map[key] = SynxValue.make_string(err)


# ─── Constraints ──

# The error `val` earns against `c`, or "" (synx-core `constraint_violation`).
static func constraint_violation(key: String, val: SynxValue, c: SynxMeta) -> String:
	if c.c_required:
		if val.kind == SynxValue.Kind.NULL or (val.kind == SynxValue.Kind.STRING and String(val.data).is_empty()):
			return "CONSTRAINT_ERR: '%s' is required" % key
	# A null value is checked against [enum] only, read as "".
	if val.kind == SynxValue.Kind.NULL:
		if c.c_enum != null and not (c.c_enum as PackedStringArray).has(""):
			return "CONSTRAINT_ERR: '%s' must be one of [%s]" % [key, "|".join(c.c_enum)]
		return ""
	if c.c_type_name != null:
		var ok := true
		match String(c.c_type_name):
			"int":
				ok = val.kind == SynxValue.Kind.INT
			"float":
				ok = val.kind == SynxValue.Kind.FLOAT or val.kind == SynxValue.Kind.INT
			"bool":
				ok = val.kind == SynxValue.Kind.BOOL
			"string":
				ok = val.kind == SynxValue.Kind.STRING or val.kind == SynxValue.Kind.SECRET
		if not ok:
			return "CONSTRAINT_ERR: '%s' expected type '%s'" % [key, c.c_type_name]
	if c.c_enum != null:
		var vs := ""
		match val.kind:
			SynxValue.Kind.STRING:
				vs = val.data
			SynxValue.Kind.INT:
				vs = str(val.data)
			SynxValue.Kind.FLOAT:
				vs = SynxNum.display(val.data)
			SynxValue.Kind.BOOL:
				vs = "true" if val.data else "false"
		if not (c.c_enum as PackedStringArray).has(vs):
			return "CONSTRAINT_ERR: '%s' must be one of [%s]" % [key, "|".join(c.c_enum)]
	var has_num := false
	var num := 0.0
	match val.kind:
		SynxValue.Kind.INT:
			has_num = true
			num = float(val.data)
		SynxValue.Kind.FLOAT:
			has_num = true
			num = val.data
		SynxValue.Kind.STRING:
			if c.c_min != null or c.c_max != null:
				has_num = true
				num = float(String(val.data).length())
	if has_num:
		if c.c_min != null and num < float(c.c_min):
			return "CONSTRAINT_ERR: '%s' value %s is below min %s" % [key, SynxNum.display(num), SynxNum.display(c.c_min)]
		if c.c_max != null and num > float(c.c_max):
			return "CONSTRAINT_ERR: '%s' value %s exceeds max %s" % [key, SynxNum.display(num), SynxNum.display(c.c_max)]
	if c.c_pattern != null and val.kind == SynxValue.Kind.STRING:
		var pat: String = c.c_pattern
		if SynxText.utf8_len(pat) <= 256 and pattern_rejects(pat, val.data):
			return "CONSTRAINT_ERR: '%s' does not match pattern /%s/" % [key, pat]
	return ""


# Whether `s` fails `[pattern:…]`, with the syntax and matching of the Rust
# `regex` crate (SynxRegex). A pattern the crate does not compile, or one of
# the nested-quantifier shape every backtracking engine must refuse, never
# rejects. Compiled patterns are cached (at most 256).
static func pattern_rejects(pat: String, s: String) -> bool:
	if not _pattern_cache.has(pat):
		if _pattern_cache.size() >= 256:
			_pattern_cache.clear()
		var compiled: RegEx = null
		if not has_nested_quantifier(pat):
			var pcre: Variant = SynxRegex.translate(pat)
			if pcre != null:
				var rx := RegEx.new()
				if rx.compile(pcre, false) == OK:
					compiled = rx
		_pattern_cache[pat] = compiled
	var c: RegEx = _pattern_cache[pat]
	return c != null and c.search(s) == null


# synx-core `has_nested_quantifier`: `(a+)+`-shaped patterns.
static func has_nested_quantifier(pattern: String) -> bool:
	var n := pattern.length()
	var opens := PackedInt64Array()
	var i := 0
	while i < n:
		var c := pattern.unicode_at(i)
		if c == 92:
			i += 1
		elif c == 91:
			while i < n and pattern.unicode_at(i) != 93:
				if pattern.unicode_at(i) == 92:
					i += 1
				i += 1
		elif c == 40:
			opens.append(i)
		elif c == 41:
			if not opens.is_empty():
				var start := opens[opens.size() - 1]
				opens.resize(opens.size() - 1)
				var nx := pattern.unicode_at(i + 1) if i + 1 < n else 0
				if nx == 42 or nx == 43 or nx == 123:
					var body := PackedInt64Array()
					var j := start + 1
					while j < i:
						if pattern.unicode_at(j) == 92:
							j += 2
							continue
						body.append(pattern.unicode_at(j))
						j += 1
					for k in body.size():
						var b := body[k]
						if b == 42 or b == 43:
							return true
						if b == 123 and k + 1 < body.size() and body[k + 1] >= 48 and body[k + 1] <= 57:
							return true
		i += 1
	return false


# ─── :calc ──

# Numeric names substituted into an expression: a key of this object
# first (it shadows the root), then a root key. Token by token; the
# whole-word sequential form only when a "special" key with a number is in
# play. [text, utf8_bytes], or null past MAX_CALC_RESOLVED_LEN.
static func substitute_calc_vars(expr: String, expr_bytes: int, local: Dictionary, self_key: String, root: Dictionary, has_root: bool, local_is_root: bool, calc_special: PackedStringArray) -> Variant:
	var special_in_play := false
	if has_root:
		for k in calc_special:
			var rv: SynxValue = root.get(k)
			if rv != null and rv.is_number():
				special_in_play = true
				break
	if not special_in_play and not local_is_root:
		for k in local:
			if k == self_key:
				continue
			var lv: SynxValue = local[k]
			if lv.is_number() and SynxEngine.is_calc_special_key(k):
				special_in_play = true
				break

	if special_in_play:
		var resolved := expr
		var bytes := expr_bytes
		var maps: Array = [local]
		if has_root and not local_is_root:
			maps.append(root)
		for mi in maps.size():
			var m: Dictionary = maps[mi]
			for rk in m:
				if mi == 0 and rk == self_key:
					continue
				var v: SynxValue = m[rk]
				if not v.is_number():
					continue
				var rep := SynxNum.format_number(float(v.data))
				var rw := _replace_word(resolved, rk, rep)
				resolved = rw[0]
				var count: int = rw[1]
				if count > 0:
					bytes += count * (rep.length() - SynxText.utf8_len(rk))
					if bytes > MAX_CALC_RESOLVED_LEN:
						return null
		return [resolved, bytes]

	# perf: the word-character test is written out in the loops; a call per
	# character was most of the cost of a `:calc`.
	var out := PackedStringArray()
	var bytes2 := expr_bytes
	var n := expr.length()
	var i := 0
	var run := 0
	while i < n:
		var c := expr.unicode_at(i)
		if ((c >= 97 and c <= 122) or (c >= 65 and c <= 90) or (c >= 48 and c <= 57) or c == 95):
			var start := i
			i += 1
			while i < n:
				c = expr.unicode_at(i)
				if not ((c >= 97 and c <= 122) or (c >= 65 and c <= 90) or (c >= 48 and c <= 57) or c == 95):
					break
				i += 1
			var token := expr.substr(start, i - start)
			var num: SynxValue = null
			if token != self_key:
				var lv2: SynxValue = local.get(token)
				if lv2 != null and lv2.is_number():
					num = lv2
			if num == null and has_root:
				var rv2: SynxValue = root.get(token)
				if rv2 != null and rv2.is_number():
					num = rv2
			if num != null:
				if start > run:
					out.append(expr.substr(run, start - run))
				var rep2 := SynxNum.format_number(float(num.data))
				out.append(rep2)
				bytes2 += rep2.length() - token.length()
				run = i
		else:
			i += 1
	if run < n:
		out.append(expr.substr(run))
	if bytes2 > MAX_CALC_RESOLVED_LEN:
		return null
	return ["".join(out), bytes2]


# Dot-path references (`base.hp`) replaced by their numbers; null past the cap.
static func _substitute_dot_paths(expr: String, bytes: int, root: SynxValue) -> Variant:
	if not expr.contains("."):
		return expr
	var out := PackedStringArray()
	var n := expr.length()
	var i := 0
	var run := 0
	while i < n:
		var c := expr.unicode_at(i)
		if ((c >= 97 and c <= 122) or (c >= 65 and c <= 90) or (c >= 48 and c <= 57) or c == 95):
			var start := i
			var has_dot := false
			i += 1
			while i < n:
				c = expr.unicode_at(i)
				if ((c >= 97 and c <= 122) or (c >= 65 and c <= 90) or (c >= 48 and c <= 57) or c == 95):
					i += 1
				elif c == 46:
					has_dot = true
					i += 1
				else:
					break
			if has_dot:
				var token := expr.substr(start, i - start)
				var val := SynxEngine.deep_get(root, token)
				if val != null and val.is_number():
					if start > run:
						out.append(expr.substr(run, start - run))
					var rep := SynxNum.format_number(float(val.data))
					out.append(rep)
					bytes += rep.length() - token.length()
					run = i
		else:
			i += 1
	if run < n:
		out.append(expr.substr(run))
	if bytes > MAX_CALC_RESOLVED_LEN:
		return null
	return "".join(out)


# Whole-word occurrences of `word` replaced: [text, count].
static func _replace_word(haystack: String, word: String, replacement: String) -> Array:
	var wl := word.length()
	if wl == 0 or wl > haystack.length():
		return [haystack, 0]
	var out := PackedStringArray()
	var count := 0
	var i := 0
	var run := 0
	var n := haystack.length()
	while true:
		var idx := haystack.find(word, i)
		if idx < 0:
			break
		var before_ok := idx == 0 or not _is_word_char(haystack.unicode_at(idx - 1))
		var after_ok := idx + wl >= n or not _is_word_char(haystack.unicode_at(idx + wl))
		if before_ok and after_ok:
			if idx > run:
				out.append(haystack.substr(run, idx - run))
			out.append(replacement)
			count += 1
			i = idx + wl
			run = i
		else:
			i = idx + 1
	if count == 0:
		return [haystack, 0]
	if run < n:
		out.append(haystack.substr(run))
	return ["".join(out), count]


static func _is_word_char(c: int) -> bool:
	return (c >= 48 and c <= 57) or (c >= 65 and c <= 90) or (c >= 97 and c <= 122) or c == 95


# A `:calc` result: Int when whole and in i64 range, Float otherwise.
static func _calc_value(f: float) -> SynxValue:
	if SynxNum.fits_int(f):
		return SynxValue.make_int(int(f))
	return SynxValue.make_float(f)


# 10^n as Rust `f64::powi` computes it (compiler-rt `__powidf2`).
static func _powi10(n: int) -> float:
	var b := n
	if b > 2147483647:
		b -= 4294967296
	var recip := b < 0
	var r := 1.0
	var a := 10.0
	var e := absi(b)
	while true:
		if (e & 1) != 0:
			r *= a
		e = e / 2
		if e == 0:
			break
		a *= a
	return 1.0 / r if recip else r


# ─── :format ──

static func apply_format_pattern(pattern: String, value: SynxValue) -> String:
	match value.kind:
		SynxValue.Kind.INT:
			if pattern.contains("d") or pattern.contains("i"):
				return _format_int_pattern(pattern, value.data)
			if pattern.contains("f") or pattern.contains("e"):
				return _format_float_pattern(pattern, float(value.data))
			return str(value.data)
		SynxValue.Kind.FLOAT:
			var f: float = value.data
			if pattern.contains("f") or pattern.contains("e"):
				return _format_float_pattern(pattern, f)
			if (pattern.contains("d") or pattern.contains("i")) and SynxNum.is_whole(f) and absf(f) < 9.0e15:
				return _format_int_pattern(pattern, int(f))
			return SynxNum.format_number(f)
		SynxValue.Kind.STRING:
			return value.data
	return SynxEngine.value_to_string(value)


static func _format_int_pattern(pattern: String, n: int) -> String:
	const MAX_W := 4096
	if pattern.begins_with("%"):
		var s := pattern.substr(1)
		var inner := ""
		var has_inner := false
		if s.ends_with("d") or s.ends_with("i"):
			inner = s.substr(0, s.length() - 1)
			has_inner = true
		if has_inner:
			if inner.begins_with("0"):
				var w := SynxNum.parse_u64(inner.substr(1))
				if w >= 0:
					return str(n).lpad(mini(w, MAX_W), "0")
			var w2 := SynxNum.parse_u64(inner)
			if w2 >= 0:
				return str(n).lpad(mini(w2, MAX_W), " ")
	return str(n)


static func _format_float_pattern(pattern: String, f: float) -> String:
	const MAX_P := 1024
	if pattern.begins_with("%"):
		var s := pattern.substr(1)
		if s == "e":
			if f == 0.0:
				return "0e+0"
			var raw := SynxNum.lower_exp(f)
			var ep := raw.rfind("e")
			var mantissa := raw.substr(0, ep)
			var exp_part := raw.substr(ep + 1)
			var sign := "+"
			var digits := exp_part
			if exp_part.begins_with("-"):
				sign = "-"
				digits = exp_part.substr(1)
			elif exp_part.begins_with("+"):
				digits = exp_part.substr(1)
			if mantissa.ends_with(".0"):
				mantissa = mantissa.substr(0, mantissa.length() - 2)
			return mantissa + "e" + sign + digits
		if s.ends_with("f") or s.ends_with("e"):
			var inner := s.substr(0, s.length() - 1)
			if inner.begins_with("."):
				var prec := SynxNum.parse_u64(inner.substr(1))
				if prec >= 0:
					prec = mini(prec, MAX_P)
					if s.ends_with("e"):
						return SynxNum.exp_prec(f, prec)
					return SynxNum.fixed(f, prec)
	return SynxNum.display(f)


# ─── Helpers ──

static func wants_string(meta: SynxMeta) -> bool:
	return (meta.type_hint != null and String(meta.type_hint) == "string") \
		or (meta.c_type_name != null and String(meta.c_type_name) == "string")


# synx-core `cast_primitive`: text from outside the document (env, defaults).
static func cast_primitive(s: String) -> SynxValue:
	var n := s.length()
	if n >= 2:
		var c0 := s.unicode_at(0)
		var cn := s.unicode_at(n - 1)
		if (c0 == 34 and cn == 34) or (c0 == 39 and cn == 39):
			return SynxValue.make_string(s.substr(1, n - 2))
	if s == "true":
		return SynxValue.make_bool(true)
	if s == "false":
		return SynxValue.make_bool(false)
	if s == "null":
		return SynxValue.make_null()
	var iv: Variant = SynxNum.parse_i64(s)
	if iv != null:
		return SynxValue.make_int(iv)
	var fv: Variant = SynxNum.parse_f64(s)
	if fv != null:
		return SynxValue.make_float(fv)
	return SynxValue.make_string(s)


static func delimiter_from_keyword(kw: String) -> String:
	match kw:
		"space": return " "
		"pipe": return "|"
		"dash": return "-"
		"dot": return "."
		"semi": return ";"
		"tab": return "\t"
		"slash": return "/"
	return kw


static func _copy(v: Variant) -> SynxValue:
	if v == null:
		return null
	var sv: SynxValue = v
	if sv.kind == SynxValue.Kind.OBJECT or sv.kind == SynxValue.Kind.ARRAY:
		return sv.clone()
	return sv


# The object a metadata path names, for the alias cycle check. synx-core
# looks the text up as a metadata path, so only a canonical escaping names
# anything.
static func _meta_holder(root: SynxValue, target_parent: String) -> SynxValue:
	if target_parent.is_empty():
		return root
	var segs := SynxMeta.path_segments(target_parent)
	var canon := PackedStringArray()
	for s in segs:
		canon.append(s if s.begins_with("#") else SynxMeta.key_segment(s))
	if ".".join(canon) != target_parent:
		return null
	return SynxMeta.resolve_path(root, target_parent)


static func _weighted_random(items: Array, weights_in: PackedFloat64Array) -> SynxValue:
	var w := weights_in.duplicate()
	if w.size() < items.size():
		var assigned := 0.0
		for f in w:
			assigned += f
		var per_item: float
		if assigned < 100.0:
			per_item = (100.0 - assigned) / float(items.size() - w.size())
		else:
			per_item = assigned / float(w.size())
		while w.size() < items.size():
			w.append(per_item)
	var total := 0.0
	for f in w:
		total += f
	if total <= 0.0:
		return items[SynxRng.random_usize(items.size())]
	var r := SynxRng.random_f64_01()
	var cumulative := 0.0
	for i in items.size():
		cumulative += w[i] / total
		if r <= cumulative:
			return items[i]
	return items[items.size() - 1]


# `:sort` order: two numbers numerically, any other pair as text by code
# point. Stable (merge sort), as Rust's `sort_by`.
static func _sort_less(a: SynxValue, b: SynxValue) -> int:
	if a.is_number() and b.is_number():
		var x := float(a.data)
		var y := float(b.data)
		if x < y:
			return -1
		if x > y:
			return 1
		return 0
	var sa := SynxEngine.value_to_string(a)
	var sb := SynxEngine.value_to_string(b)
	if sa < sb:
		return -1
	if sa > sb:
		return 1
	return 0


static func _stable_sort(arr: Array) -> Array:
	if arr.size() < 2:
		return arr
	var mid := arr.size() / 2
	var left := _stable_sort(arr.slice(0, mid))
	var right := _stable_sort(arr.slice(mid))
	var out: Array = []
	var i := 0
	var j := 0
	while i < left.size() and j < right.size():
		if _sort_less(right[j], left[i]) < 0:
			out.append(right[j])
			j += 1
		else:
			out.append(left[i])
			i += 1
	while i < left.size():
		out.append(left[i])
		i += 1
	while j < right.size():
		out.append(right[j])
		j += 1
	return out


static func _apply_include(map: Dictionary, key: String, rel: String, options: SynxOptions) -> void:
	var max_depth := options.max_include_depth if options.max_include_depth > 0 else SynxEngine.DEFAULT_MAX_INCLUDE_DEPTH
	if options._include_depth >= max_depth:
		map[key] = SynxValue.make_string("INCLUDE_ERR: max include depth (%d) exceeded" % max_depth)
		return
	var jail := SynxEngine.jail_path(SynxEngine._effective_base(options), rel)
	if not bool(jail["ok"]):
		map[key] = SynxValue.make_string("INCLUDE_ERR: %s" % jail["error"])
		return
	var full: String = jail["path"]
	var size := SynxEngine._file_size(full)
	if size > SynxEngine.MAX_FILE_SIZE:
		map[key] = SynxValue.make_string("INCLUDE_ERR: SECURITY: file too large (%d bytes, max %d)" % [size, SynxEngine.MAX_FILE_SIZE])
		return
	var text: Variant = SynxEngine._read_text(full) if FileAccess.file_exists(full) else null
	if text == null:
		map[key] = SynxValue.make_string("INCLUDE_ERR: " + _read_error(full, rel))
		return
	var included := SynxParser.parse(text)
	if included.mode == SynxParseResult.Mode.ACTIVE:
		var child := options.clone()
		child._include_depth += 1
		child.base_path = SynxEngine._parent_dir(full)
		SynxEngine.resolve(included, child)
	map[key] = included.root


# Returns false when the marker stopped the chain (an error was stored).
static func _apply_watch(map: Dictionary, key: String, rel: String, markers: PackedStringArray, options: SynxOptions) -> bool:
	var max_depth := options.max_include_depth if options.max_include_depth > 0 else SynxEngine.DEFAULT_MAX_INCLUDE_DEPTH
	if options._include_depth >= max_depth:
		map[key] = SynxValue.make_string("WATCH_ERR: max include depth (%d) exceeded" % max_depth)
		return false
	var jail := SynxEngine.jail_path(SynxEngine._effective_base(options), rel)
	if not bool(jail["ok"]):
		map[key] = SynxValue.make_string("WATCH_ERR: %s" % jail["error"])
		return false
	var full: String = jail["path"]
	var size := SynxEngine._file_size(full)
	if size > SynxEngine.MAX_FILE_SIZE:
		map[key] = SynxValue.make_string("WATCH_ERR: SECURITY: file too large (%d bytes, max %d)" % [size, SynxEngine.MAX_FILE_SIZE])
		return false
	var wi := markers.find("watch")
	var content: Variant = SynxEngine._read_text(full) if FileAccess.file_exists(full) else null
	if content == null:
		map[key] = SynxValue.make_string("WATCH_ERR: " + _read_error(full, rel))
		return true
	if wi + 1 >= markers.size():
		map[key] = SynxValue.make_string(SynxText.trim(content))
		return true
	var key_path := markers[wi + 1]
	var dot := full.rfind(".")
	var ext := full.substr(dot + 1) if dot >= 0 else ""
	var found: SynxValue = null
	if ext == "json":
		var parsed: Variant = JSON.parse_string(content)
		if parsed != null:
			var cur: Variant = parsed
			var ok := true
			for part in key_path.split("."):
				if cur is Dictionary and (cur as Dictionary).has(part):
					cur = cur[part]
				else:
					ok = false
					break
			if ok:
				found = _from_json_variant(cur)
	else:
		var inner := SynxParser.parse(content)
		var cur2: SynxValue = inner.root
		for part in key_path.split("."):
			if cur2 != null and cur2.kind == SynxValue.Kind.OBJECT:
				cur2 = (cur2.data as Dictionary).get(part)
			else:
				cur2 = null
		found = cur2
	map[key] = found if found != null else SynxValue.make_null()
	return true


# synx-core `fmt_io_err` for a file that could not be read as text.
static func _read_error(full: String, rel: String) -> String:
	if FileAccess.file_exists(full):
		return "stream did not contain valid UTF-8"
	return "file not found: %s" % rel


static func _from_json_variant(v: Variant) -> SynxValue:
	match typeof(v):
		TYPE_NIL:
			return SynxValue.make_null()
		TYPE_BOOL:
			return SynxValue.make_bool(v)
		TYPE_FLOAT:
			var f: float = v
			if SynxNum.is_whole(f) and absf(f) < 9007199254740992.0:
				return SynxValue.make_int(int(f))
			return SynxValue.make_float(f)
		TYPE_INT:
			return SynxValue.make_int(v)
		TYPE_STRING:
			return SynxValue.make_string(v)
		TYPE_ARRAY:
			var out: Array = []
			for item in v:
				out.append(_from_json_variant(item))
			return SynxValue.make_array(out)
		TYPE_DICTIONARY:
			var d: Dictionary = {}
			for k in v:
				d[String(k)] = _from_json_variant(v[k])
			return SynxValue.make_object(d)
	return SynxValue.make_null()


static func _read_lock_value(lock_path: String, key: String) -> Variant:
	if not FileAccess.file_exists(lock_path):
		return null
	var text: Variant = SynxEngine._read_text(lock_path)
	if text == null:
		return null
	for line in (text as String).split("\n"):
		var l := line.trim_suffix("\r")
		if l.begins_with(key) and l.substr(key.length()).begins_with(" "):
			return SynxText.trim_start(l.substr(key.length()))
	return null


static func _write_lock_value(lock_path: String, key: String, value: String) -> void:
	var lines := PackedStringArray()
	if FileAccess.file_exists(lock_path):
		var text: Variant = SynxEngine._read_text(lock_path)
		if text != null:
			lines = (text as String).split("\n", false)
	var new_line := "%s %s" % [key, value]
	var found := false
	for i in lines.size():
		var l := lines[i].trim_suffix("\r")
		if l.begins_with(key) and l.substr(key.length()).begins_with(" "):
			lines[i] = new_line
			found = true
			break
	if not found:
		lines.append(new_line)
	var w := FileAccess.open(lock_path, FileAccess.WRITE)
	if w != null:
		w.store_string("\n".join(lines) + "\n")
		w.close()


static func _leading_int(s: String) -> int:
	var t := SynxText.trim_start(s)
	var neg := false
	var i := 0
	if t.begins_with("-"):
		neg = true
		i = 1
	elif t.begins_with("+"):
		i = 1
	var n := 0
	while i < t.length():
		var c := t.unicode_at(i)
		if c < 48 or c > 57:
			break
		if n > (SynxNum.I64_MAX - (c - 48)) / 10:
			n = SynxNum.I64_MAX
		else:
			n = n * 10 + (c - 48)
		i += 1
	return -n if neg else n


static func compare_versions(current: String, op: String, required: String) -> bool:
	var cv := PackedInt64Array()
	for p in current.split("."):
		cv.append(_leading_int(p))
	var rv := PackedInt64Array()
	for p in required.split("."):
		rv.append(_leading_int(p))
	var ord := 0
	for i in maxi(cv.size(), rv.size()):
		var a := cv[i] if i < cv.size() else 0
		var b := rv[i] if i < rv.size() else 0
		if a != b:
			ord = -1 if a < b else 1
			break
	match op:
		">=": return ord >= 0
		"<=": return ord <= 0
		">": return ord > 0
		"<": return ord < 0
		"==", "=": return ord == 0
		"!=": return ord != 0
	return false


# CLDR plural category (synx-core `plural_category`).
static func plural_category(lang: String, n: int) -> String:
	var abs_n: int = absi(n)
	var n10: int = abs_n % 10
	var n100: int = abs_n % 100
	match lang:
		"ru", "uk", "be", "pl":
			if n10 == 1 and n100 != 11:
				return "one"
			if n10 >= 2 and n10 <= 4 and not (n100 >= 12 and n100 <= 14):
				return "few"
			return "many"
		"cs", "sk":
			if abs_n == 1:
				return "one"
			if abs_n >= 2 and abs_n <= 4:
				return "few"
			return "other"
		"ar":
			if abs_n == 0:
				return "zero"
			if abs_n == 1:
				return "one"
			if abs_n == 2:
				return "two"
			if n100 >= 3 and n100 <= 10:
				return "few"
			if n100 >= 11 and n100 <= 99:
				return "many"
			return "other"
		"fr", "pt":
			return "one" if abs_n <= 1 else "other"
		"ja", "zh", "ko", "vi", "th":
			return "other"
	return "one" if abs_n == 1 else "other"


# The text `:prompt` embeds (synx-core engine `stringify_value`).
static func stringify_value(value: SynxValue, indent: int) -> String:
	var spaces := " ".repeat(indent)
	match value.kind:
		SynxValue.Kind.OBJECT:
			var out := PackedStringArray()
			var keys: Array = (value.data as Dictionary).keys()
			keys.sort()
			for k in keys:
				var val: SynxValue = value.data[k]
				if val.kind == SynxValue.Kind.OBJECT:
					out.append("%s%s\n" % [spaces, k])
					out.append(stringify_value(val, indent + 2))
				elif val.kind == SynxValue.Kind.ARRAY:
					out.append("%s%s\n" % [spaces, k])
					for item in val.data:
						out.append("%s  - %s\n" % [spaces, SynxEngine.value_to_string(item)])
				else:
					out.append("%s%s %s\n" % [spaces, k, SynxEngine.value_to_string(val)])
			return "".join(out)
		SynxValue.Kind.ARRAY:
			var out2 := PackedStringArray()
			for item in value.data:
				out2.append("%s  - %s\n" % [spaces, SynxEngine.value_to_string(item)])
			return "".join(out2)
	return "%s%s\n" % [spaces, SynxEngine.value_to_string(value)]
