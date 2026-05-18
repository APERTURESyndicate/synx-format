@tool
extends SceneTree

# Conformance + smoke test runner.
#
# Run with:
#   godot --headless --script tests/test_runner.gd
#
# Returns nonzero exit on any failure so CI can fail.

var failures: Array[String] = []
var passed: int = 0


func _init() -> void:
	print("SYNX GDScript test runner — Godot %s" % Engine.get_version_info()["string"])
	_run_unit_tests()
	_run_conformance()
	print("\n────────────────────────────────")
	print("Passed: %d" % passed)
	print("Failed: %d" % failures.size())
	for f in failures:
		print("  ✗ %s" % f)
	if failures.size() > 0:
		quit(1)
	else:
		quit(0)


# ── Unit tests ──

func _run_unit_tests() -> void:
	_test_scalar_parse()
	_test_nested()
	_test_lists()
	_test_multiline()
	_test_quoted_strings()
	_test_active_env()
	_test_active_calc()
	_test_active_ref()
	_test_active_alias()
	_test_active_alias_cycle()
	_test_active_inherit()
	_test_active_default()
	_test_active_clamp_round()
	_test_active_split_join()
	_test_active_unique()
	_test_active_sort_sum()
	_test_active_random_weighted()
	_test_active_replace()
	_test_active_version()
	_test_active_i18n()
	_test_active_i18n_plural_ru()
	_test_active_interpolation()
	_test_active_format_pattern()
	_test_active_constraints_required()
	_test_active_constraints_min_max()
	_test_active_constraints_enum_pattern()
	_test_active_secret_redaction()
	_test_active_secret_redaction_in_json()
	_test_calc_evaluator()
	_test_diff_basic()
	_test_binary_roundtrip()
	_test_binary_roundtrip_active()
	_test_stringify()
	_test_formatter_canonical()
	_test_tool_call_reshape()
	_test_tool_schema_reshape()
	_test_to_variant()


# ── Conformance runner ──

func _run_conformance() -> void:
	# Conformance cases ship in the parent repo at `tests/conformance/cases/`.
	# Resolve relative to the Godot project root if available.
	var base := _conformance_base()
	if base.is_empty():
		print("· conformance: skipped (no cases directory found)")
		return
	var dir := DirAccess.open(base)
	if dir == null:
		print("· conformance: cannot open %s" % base)
		return
	var ran := 0
	for f in dir.get_files():
		if not f.ends_with(".synx"):
			continue
		var stem := f.substr(0, f.length() - 5)
		var expected_path := base + "/" + stem + ".expected.json"
		if not FileAccess.file_exists(expected_path):
			continue
		ran += 1
		var synx_text := FileAccess.get_file_as_string(base + "/" + f)
		var expected := FileAccess.get_file_as_string(expected_path).strip_edges()
		var parsed_full := SynxParser.parse(synx_text)
		var emit_root: SynxValue
		if parsed_full.tool_directive:
			emit_root = SynxParser.reshape_tool_output(parsed_full.root, parsed_full.schema)
		else:
			emit_root = parsed_full.root
		var actual := SynxJson.to_json(emit_root)
		_assert_eq(actual, expected, "conformance/" + stem)
	if ran == 0:
		print("· conformance: 0 cases (skipped)")


func _conformance_base() -> String:
	# Search up from this script's location for `tests/conformance/cases/`.
	var candidates := [
		"res://../../tests/conformance/cases",
		"res://../../../tests/conformance/cases",
		"res://tests/conformance/cases",
	]
	for c in candidates:
		var dir := DirAccess.open(c)
		if dir != null:
			return c
	# Fallback: ProjectSettings globalize_path on resource path.
	var here := ProjectSettings.globalize_path("res://").rstrip("/")
	# Repo root is two levels up from integrations/godot/synx-gdscript.
	var guess := here.path_join("../../tests/conformance/cases")
	if DirAccess.dir_exists_absolute(guess):
		return guess.simplify_path()
	return ""


# ── Assertion ──

func _assert(cond: bool, msg: String) -> void:
	if cond:
		passed += 1
		print("· %s ✓" % msg)
	else:
		failures.append(msg)
		print("· %s ✗" % msg)


func _assert_eq(actual, expected, msg: String) -> void:
	if actual == expected:
		passed += 1
		print("· %s ✓" % msg)
	else:
		failures.append("%s — got: %s | want: %s" % [msg, str(actual), str(expected)])
		print("· %s ✗ got=%s want=%s" % [msg, str(actual), str(expected)])


# ── Individual tests ──

func _test_scalar_parse() -> void:
	var d := SynxParser.parse("name Alice\nage 30\nactive true\nscore 99.5\nempty null").root.data
	_assert_eq((d["name"] as SynxValue).data, "Alice", "scalar parse: name")
	_assert_eq((d["age"] as SynxValue).data, 30, "scalar parse: age (int)")
	_assert_eq((d["active"] as SynxValue).data, true, "scalar parse: active (bool)")
	_assert_eq((d["score"] as SynxValue).data, 99.5, "scalar parse: score (float)")
	_assert((d["empty"] as SynxValue).is_null(), "scalar parse: empty (null)")


func _test_nested() -> void:
	var d := SynxParser.parse("server\n  host 0.0.0.0\n  port 8080").root.data
	var server: SynxValue = d["server"]
	_assert_eq((server.data["host"] as SynxValue).data, "0.0.0.0", "nested: server.host")
	_assert_eq((server.data["port"] as SynxValue).data, 8080, "nested: server.port")


func _test_lists() -> void:
	var d := SynxParser.parse("inventory\n  - Sword\n  - Shield\n  - Potion").root.data
	var inv: SynxValue = d["inventory"]
	_assert_eq(inv.data.size(), 3, "list: 3 items")
	_assert_eq((inv.data[0] as SynxValue).data, "Sword", "list: first item")


func _test_multiline() -> void:
	var d := SynxParser.parse("rules |\n  Rule one.\n  Rule two.\n  Rule three.").root.data
	_assert_eq((d["rules"] as SynxValue).data, "Rule one.\nRule two.\nRule three.", "multiline block")


func _test_quoted_strings() -> void:
	var d := SynxParser.parse("port \"3000\"\nname 'Alice'").root.data
	_assert_eq((d["port"] as SynxValue).data, "3000", "quoted preserves string")
	_assert_eq((d["port"] as SynxValue).kind, SynxValue.Kind.STRING, "quoted prevents int cast")


func _test_active_env() -> void:
	var r := SynxParser.parse("!active\nport:env:default:3000 SYNX_TEST_PORT")
	var o := SynxOptions.new()
	o.env = {"SYNX_TEST_PORT": "9090"}
	SynxEngine.resolve(r, o)
	_assert_eq((r.root.data["port"] as SynxValue).data, 9090, "env resolves with override")


func _test_active_calc() -> void:
	var r := SynxParser.parse("!active\nprice 100\ntax:calc price * 0.2")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["tax"] as SynxValue).data, 20, "calc: price * 0.2 = 20")


func _test_active_ref() -> void:
	var r := SynxParser.parse("!active\nbase_rate 50\nquick:ref base_rate")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["quick"] as SynxValue).data, 50, "ref resolves")


func _test_active_alias() -> void:
	var r := SynxParser.parse("!active\nname Alice\nbackup:alias name")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["backup"] as SynxValue).data, "Alice", "alias copies value")


func _test_active_alias_cycle() -> void:
	var r := SynxParser.parse("!active\na:alias b\nb:alias a")
	SynxEngine.resolve(r, SynxOptions.new())
	var av := (r.root.data["a"] as SynxValue).data
	_assert(String(av).begins_with("ALIAS_ERR"), "alias cycle reported")


func _test_active_inherit() -> void:
	var r := SynxParser.parse("!active\n_base\n  weight 10\n  stackable true\nsteel:inherit:_base\n  weight 25\n  material metal")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert(not r.root.data.has("_base"), "inherit: _base stripped")
	var steel: SynxValue = r.root.data["steel"]
	_assert_eq((steel.data["weight"] as SynxValue).data, 25, "inherit: child overrides")
	_assert_eq((steel.data["stackable"] as SynxValue).data, true, "inherit: parent field copied")
	_assert_eq((steel.data["material"] as SynxValue).data, "metal", "inherit: child-only field present")


func _test_active_default() -> void:
	var r := SynxParser.parse("!active\nport:default:8080")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["port"] as SynxValue).data, 8080, "default applies when empty")


func _test_active_clamp_round() -> void:
	var r := SynxParser.parse("!active\nhp:clamp:0:100 150\npi:round:2 3.14159")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["hp"] as SynxValue).data, 100, "clamp 0..100 of 150 = 100")
	_assert_eq((r.root.data["pi"] as SynxValue).data, 3.14, "round to 2 dp")


func _test_active_split_join() -> void:
	var r := SynxParser.parse("!active\ntags:split a, b, c\nnames:join\n  - Alice\n  - Bob")
	SynxEngine.resolve(r, SynxOptions.new())
	var tags: SynxValue = r.root.data["tags"]
	_assert_eq(tags.data.size(), 3, "split → 3 items")
	_assert_eq((r.root.data["names"] as SynxValue).data, "Alice,Bob", "join → comma sep")


func _test_active_unique() -> void:
	var r := SynxParser.parse("!active\nseen:unique\n  - 1\n  - 2\n  - 1\n  - 3")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["seen"] as SynxValue).data.size(), 3, "unique drops duplicates")


func _test_active_sort_sum() -> void:
	var r := SynxParser.parse("!active\nasc:sort\n  - 5\n  - 1\n  - 3\ntotal:sum\n  - 10\n  - 20\n  - 30")
	SynxEngine.resolve(r, SynxOptions.new())
	var asc: SynxValue = r.root.data["asc"]
	_assert_eq((asc.data[0] as SynxValue).data, 1, "sort: smallest first")
	_assert_eq((r.root.data["total"] as SynxValue).data, 60, "sum = 60")


func _test_active_random_weighted() -> void:
	SynxRng.set_seed(42)
	var r := SynxParser.parse("!active\ntier:random 100\n  - Common\n  - Rare\n  - Epic")
	SynxEngine.resolve(r, SynxOptions.new())
	var t: SynxValue = r.root.data["tier"]
	_assert(t.kind == SynxValue.Kind.STRING, "weighted random returns an item")


func _test_active_replace() -> void:
	var r := SynxParser.parse("!active\nslug:replace:_:- hello_world_again")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["slug"] as SynxValue).data, "hello-world-again", "replace _ → -")


func _test_active_version() -> void:
	var r := SynxParser.parse("!active\nok:version:>=:1.2.0 1.5.0")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["ok"] as SynxValue).data, true, "version >= passes")


func _test_active_i18n() -> void:
	var r := SynxParser.parse("!active\ntitle:i18n\n  en Hello\n  ru Привет")
	var o := SynxOptions.new()
	o.lang = "ru"
	SynxEngine.resolve(r, o)
	_assert_eq((r.root.data["title"] as SynxValue).data, "Привет", "i18n selects ru")


func _test_active_i18n_plural_ru() -> void:
	var r := SynxParser.parse("!active\nitem_count 3\ntitle:i18n:item_count\n  en\n    one {count} item\n    other {count} items\n  ru\n    one {count} предмет\n    few {count} предмета\n    many {count} предметов\n    other {count} предметов")
	var o := SynxOptions.new()
	o.lang = "ru"
	SynxEngine.resolve(r, o)
	_assert_eq((r.root.data["title"] as SynxValue).data, "3 предмета", "ru plural 'few' for 3")


func _test_active_interpolation() -> void:
	var r := SynxParser.parse("!active\nname Alice\ngreeting Hello, {name}!")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["greeting"] as SynxValue).data, "Hello, Alice!", "{name} interpolation")


func _test_active_format_pattern() -> void:
	var r := SynxParser.parse("!active\nzip:format:%05d 42")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert_eq((r.root.data["zip"] as SynxValue).data, "00042", "format %05d → '00042'")


func _test_active_constraints_required() -> void:
	# An empty-value required string field violates the constraint.
	var r := SynxParser.parse("!active\nname[required, type:string]")
	SynxEngine.resolve(r, SynxOptions.new())
	var v: SynxValue = r.root.data["name"]
	# `name` becomes an empty Object (no value, no nested children) — the
	# type constraint forces a string check, which fails, yielding CONSTRAINT_ERR.
	_assert(v.kind == SynxValue.Kind.STRING and String(v.data).begins_with("CONSTRAINT_ERR"), "required+type:string on empty value → CONSTRAINT_ERR")


func _test_active_constraints_min_max() -> void:
	var r := SynxParser.parse("!active\nhp[min:0, max:100] 200")
	SynxEngine.resolve(r, SynxOptions.new())
	var v: SynxValue = r.root.data["hp"]
	_assert(v.kind == SynxValue.Kind.STRING and String(v.data).begins_with("CONSTRAINT_ERR"), "max violated → CONSTRAINT_ERR")


func _test_active_constraints_enum_pattern() -> void:
	var r1 := SynxParser.parse("!active\nrole[enum:admin|user] guest")
	SynxEngine.resolve(r1, SynxOptions.new())
	var v1: SynxValue = r1.root.data["role"]
	_assert(String(v1.data).begins_with("CONSTRAINT_ERR"), "enum violation")

	var r2 := SynxParser.parse("!active\nzip[pattern:^\\d{5}$] abcde")
	SynxEngine.resolve(r2, SynxOptions.new())
	var v2: SynxValue = r2.root.data["zip"]
	_assert(String(v2.data).begins_with("CONSTRAINT_ERR"), "pattern violation")


func _test_active_secret_redaction() -> void:
	var r := SynxParser.parse("!active\ntoken:secret abc-123")
	SynxEngine.resolve(r, SynxOptions.new())
	var v: SynxValue = r.root.data["token"]
	_assert(v.kind == SynxValue.Kind.SECRET, "secret stored as SECRET kind")
	_assert_eq(v.as_secret(), "abc-123", "secret retains underlying value")


func _test_active_secret_redaction_in_json() -> void:
	var r := SynxParser.parse("!active\ntoken:secret abc-123")
	SynxEngine.resolve(r, SynxOptions.new())
	var json := SynxJson.to_json(r.root)
	_assert("\"[SECRET]\"" in json, "JSON output redacts secrets")
	_assert(not ("abc-123" in json), "JSON output does NOT leak secret")


func _test_calc_evaluator() -> void:
	_assert_eq(SynxSafeCalc.evaluate("2 + 3 * 4")["value"], 14.0, "calc precedence")
	_assert_eq(SynxSafeCalc.evaluate("(2 + 3) * 4")["value"], 20.0, "calc parens")
	_assert(not bool(SynxSafeCalc.evaluate("10 / 0")["ok"]), "calc div by zero")
	_assert_eq(SynxSafeCalc.evaluate("-5 + 3")["value"], -2.0, "calc unary minus")


func _test_diff_basic() -> void:
	var a := {"x": SynxValue.make_int(1), "y": SynxValue.make_int(2)}
	var b := {"x": SynxValue.make_int(1), "z": SynxValue.make_int(3)}
	var d := SynxDiff.diff(a, b)
	_assert_eq(d["added"].size(), 1, "diff: added")
	_assert_eq(d["removed"].size(), 1, "diff: removed")
	_assert_eq(d["unchanged"].size(), 1, "diff: unchanged")


func _test_binary_roundtrip() -> void:
	var text := "name Test\nport 8080\nactive true\ntags\n  - a\n  - b\n"
	var r := SynxParser.parse(text)
	var bin := SynxBinary.compile(r, false)
	_assert(SynxBinary.is_synxb(bin), "binary: magic header")
	var back := SynxBinary.decompile(bin)
	_assert(bool(back["ok"]), "binary: decompile ok")
	var restored: SynxParseResult = back["result"]
	_assert(restored.root.equals(r.root), "binary: roundtrip preserves tree")


func _test_binary_roundtrip_active() -> void:
	var text := "!active\nport[min:1, max:65535]:env:default:3000 PORT\n"
	var r := SynxParser.parse(text)
	var bin := SynxBinary.compile(r, false)
	var back := SynxBinary.decompile(bin)
	_assert(bool(back["ok"]), "binary active: decompile ok")
	var restored: SynxParseResult = back["result"]
	_assert(restored.mode == SynxParseResult.Mode.ACTIVE, "binary active: mode preserved")
	_assert(restored.metadata.size() > 0, "binary active: metadata preserved")


func _test_stringify() -> void:
	var v := SynxValue.make_object({
		"name": SynxValue.make_string("Test"),
		"port": SynxValue.make_int(8080),
	})
	var text := SynxStringify.stringify(v)
	_assert("name Test" in text, "stringify: name Test line")
	_assert("port 8080" in text, "stringify: port 8080 line")


func _test_formatter_canonical() -> void:
	var canonical := SynxFormatter.format("port 8080\nname Test\n# comment\n\n")
	# Keys sorted; comment stripped; trailing newline.
	var lines := canonical.split("\n")
	_assert(lines[0] == "name Test", "canonical: alphabetical (name first)")
	_assert(canonical.ends_with("\n"), "canonical: single trailing newline")


func _test_tool_call_reshape() -> void:
	var r := SynxParser.parse("!tool\nweb_search\n  query test\n  lang ru\n")
	var shaped := SynxParser.reshape_tool_output(r.root, false)
	_assert_eq((shaped.data["tool"] as SynxValue).data, "web_search", "tool reshape: tool name")
	var params: SynxValue = shaped.data["params"]
	_assert_eq((params.data["query"] as SynxValue).data, "test", "tool reshape: params.query")


func _test_tool_schema_reshape() -> void:
	var r := SynxParser.parse("!tool\n!schema\nweb_search\n  query string\nmemory_write\n  path string\n")
	var shaped := SynxParser.reshape_tool_output(r.root, true)
	var tools: SynxValue = shaped.data["tools"]
	_assert_eq(tools.data.size(), 2, "schema reshape: 2 tools")
	_assert_eq((((tools.data[0] as SynxValue).data["name"]) as SynxValue).data, "memory_write", "schema reshape: sorted (memory first)")


func _test_to_variant() -> void:
	var d := Synx.parse_to_variant("name Alice\nage 30\nactive true")
	_assert_eq(d["name"], "Alice", "to_variant: name unwrapped")
	_assert_eq(d["age"], 30, "to_variant: age unwrapped")
	_assert_eq(d["active"], true, "to_variant: active unwrapped")
