extends SceneTree

# SYNX GDScript test runner — regression cases, unit tests and the SYNX
# conformance suite of the repository (tests/conformance/cases).
#
#   godot --headless --path integrations/godot/synx-gdscript --import
#   godot --headless --path integrations/godot/synx-gdscript -s tests/test_runner.gd
#
# `--import` registers the addon's `class_name` scripts; run it once after
# adding a script. Exits non-zero on any failure. A script error leaves a
# headless SceneTree running, so CI should wrap the run in a timeout.
#
# Use the Godot executable itself: the WinGet `godot` link loses the
# GodotSharp directory of the .NET build and hangs in ".NET: Initializing".

var failures: Array[String] = []
var passed: int = 0


func _init() -> void:
	print("SYNX GDScript test runner — Godot %s" % Engine.get_version_info()["string"])
	_run_regression()
	_run_unit_tests()
	_run_conformance()
	print("· SYNXL (tests/conformance-synxl): not run — this port has no SYNXL reader")
	print("\n────────────────────────────────")
	print("Passed: %d" % passed)
	print("Failed: %d" % failures.size())
	for f in failures:
		print("  ✗ %s" % f)
	quit(1 if failures.size() > 0 else 0)


# ── Regression cases (expected JSON produced by synx-core 3.8.0) ──

const REGRESSION_CASES := [
	["order: calc chain", "!active\nprice 100\ntax:calc price * 0.2\ntotal:calc price + tax\n", "{\"price\":100,\"tax\":20,\"total\":120}"],
	["order: nested sees resolved parent key", "!active\na:calc 2 * 3\nb\n  x got {a}\n  y:calc a + 1\n", "{\"a\":6,\"b\":{\"x\":\"got 6\",\"y\":7}}"],
	["order: alias of env is the value", "!active\nport:env:default:8080 SYNX_T_UNSET\ncopy:alias port\n", "{\"copy\":8080,\"port\":8080}"],
	["order: first type declaration wins", "!active\nx(int) 5\nsub\n  x(string) hello\n", "{\"sub\":{\"x\":\"TYPE_ERR: 'x' expected int but got string\"},\"x\":5}"],
	["decl: list items keep their own markers", "!active\nservers\n  - name a\n    port:env:default:7001 SYNX_T_UNSET\n  - name b\n    port:clamp:1:5 9\n  - name c\n    port 3\n", "{\"servers\":[{\"name\":\"a\",\"port\":7001},{\"name\":\"b\",\"port\":5},{\"name\":\"c\",\"port\":3}]}"],
	["decl: dotted key does not share with nested key", "!active\na.b\n  x:calc 1 + 1\na\n  b\n    x 1 + 1\n", "{\"a\":{\"b\":{\"x\":\"1 + 1\"}},\"a.b\":{\"x\":2}}"],
	["decl: replacing an object drops its declarations", "!active\nport\n  inner[enum:a] zzz\nport\n  inner other\n", "{\"port\":{\"inner\":\"other\"}}"],
	["decl: overwritten object no longer registers", "!active\na\n  x(int) 1\na 5\nb\n  x text\n", "{\"a\":5,\"b\":{\"x\":\"text\"}}"],
	["inherit: parents left to right, child overrides", "!active\n_a\n  w 1\n  v a\n_b\n  w 2\n  u b\nc:inherit:_a:_b\n  v own\n", "{\"c\":{\"u\":\"b\",\"v\":\"own\",\"w\":2}}"],
	["inherit: fields keep the markers of their source", "!active\n_base\n  port:env:default:5 SYNX_T_UNSET\n  hp:calc 2 * 5\nchild:inherit:_base\n  hp 1\n", "{\"child\":{\"hp\":1,\"port\":5}}"],
	["inherit: nested subtrees are copies", "!active\n_base\n  obj\n    n:calc 1 + 1\na:inherit:_base\n  k 1\nb:inherit:_base\n  k 2\n", "{\"a\":{\"k\":1,\"obj\":{\"n\":2}},\"b\":{\"k\":2,\"obj\":{\"n\":2}}}"],
	["inherit: list or scalar child is left as written", "!active\n_base\n  w 1\nlst:inherit:_base\n  - 1\n  - 2\nsc:inherit:_base 5\n", "{\"lst\":[1,2],\"sc\":{\"w\":1}}"],
	["calc: local key shadows root", "!active\nrate 2\nsvc\n  rate 10\n  cost:calc rate * 3\ncost:calc rate * 3\n", "{\"cost\":6,\"rate\":2,\"svc\":{\"cost\":30,\"rate\":10}}"],
	["calc: special keys", "!active\nmax-hp 50\nцена 7\nboss:calc max-hp * 2\nvat:calc цена * 2\n", "{\"boss\":100,\"max-hp\":50,\"vat\":14,\"цена\":7}"],
	["calc: error texts", "!active\nd:calc 1 / 0\nm:calc 1.2.3 + 1\nl:calc - 5\nu:calc цена_x + 1\np:calc (1 + 2\ne:calc 1 +\nt:calc 2 3\n", "{\"d\":\"CALC_ERR: SYNX :calc — division by zero\",\"e\":\"CALC_ERR: SYNX :calc — unexpected end of expression\",\"l\":\"CALC_ERR: SYNX :calc — invalid number: '-'\",\"m\":\"CALC_ERR: SYNX :calc — invalid number: '1.2.3'\",\"p\":\"CALC_ERR: SYNX :calc — missing closing parenthesis\",\"t\":\"CALC_ERR: SYNX :calc — unexpected token at position 1\",\"u\":\"CALC_ERR: SYNX :calc — unexpected character: 'ц' in expression\"}"],
	["calc: modulo and unary minus", "!active\na:calc -7 % 3\nb:calc 10 * -2\nc:calc (2 + 3) * 4\nd:calc 0.1 * 3\n", "{\"a\":-1,\"b\":-20,\"c\":20,\"d\":0.30000000000000004}"],
	["null: only enum is checked", "!active\nport[type:int]:env SYNX_T_UNSET\nname[min:3]:env SYNX_T_UNSET\nmode[enum:a|b]:env SYNX_T_UNSET\nok[enum:a|]:env SYNX_T_UNSET\n", "{\"mode\":\"CONSTRAINT_ERR: 'mode' must be one of [a|b]\",\"name\":null,\"ok\":null,\"port\":null}"],
	["text: numbers, groups, null, secrets", "!active\na 0.0000001\nb 123456789012345678901234.5\nobj\n  k 1\nn null\ns:secret hush\nt a={a} b={b} o={obj} n={n} s={s}\nj:join\n  - 1.5\n  - 2.0\n  - 0.00000025\n", "{\"a\":1e-7,\"b\":1.2345678901234569e23,\"j\":\"1.5,2,0.00000025\",\"n\":null,\"obj\":{\"k\":1},\"s\":\"[SECRET]\",\"t\":\"a=0.0000001 b=123456789012345690000000 o= n=null s=[SECRET]\"}"],
	["format: whole float under an integer pattern", "!active\na:format:%05d 12.0\nb:format:%5d 7\nc:format:%d 9007199254740993\nd:format:%.2f \"3.14159\"\ne:format:%.3f 2.0005\nf:format:%e 123456.789\ng:format:%.1e 0.25\nh:format:%05d -5\n", "{\"a\":\"00012\",\"b\":\"    7\",\"c\":\"9007199254740993\",\"d\":\"3.14159\",\"e\":\"2.001\",\"f\":\"1.23456789e+5\",\"g\":\"2.5e-1\",\"h\":\"000-5\"}"],
	["sort: numbers numeric, text by code point, stable", "!active\nn:sort\n  - 10\n  - 9\n  - 2.5\ns:sort\n  - b\n  - B\n  - é\n  - a\nd:sort:desc\n  - 1\n  - 3\n  - 2\n", "{\"d\":[3,2,1],\"n\":[2.5,9,10],\"s\":[\"B\",\"a\",\"b\",\"é\"]}"],
	["version: positional leading integers", "!active\na:version:>=:1.2.3 1.2.3-beta\nb:version:>:1.2 \"1.10\"\nc:version:<:2.0 1.x.9\n", "{\"a\":true,\"b\":true,\"c\":true}"],
	["min/max: strings count characters", "!active\na[max:3] мир\nb[max:3] мирок\nc[min:2] 😀😀\nd[min:3] 😀😀\n", "{\"a\":\"мир\",\"b\":\"CONSTRAINT_ERR: 'b' value 5 exceeds max 3\",\"c\":\"😀😀\",\"d\":\"CONSTRAINT_ERR: 'd' value 2 is below min 3\"}"],
	["indent: NBSP counts 2, U+3000 counts 3", "root\n a 1\n　b 2\n\tc 3\n", "{\"root\":{\"a\":1,\"b\":2,\"c\":3}}"],
	["indent: |+ keeps relative indentation", "code |+\n    def f():\n        return 1\n    f()\nnext 1\n", "{\"code\":\"def f():\\n    return 1\\nf()\",\"next\":1}"],
	["indent: line of NBSP is blank", "a\n  \n  b 1\n", "{\"a\":{\"b\":1}}"],
	["list: item after a nested object keeps its keys", "items\n  - name a\n    sub\n      k 1\n  - name b\n    port 2\n", "{\"items\":[{\"name\":\"a\",\"sub\":{\"k\":1}},{\"name\":\"b\",\"port\":2}]}"],
	["list: deeper non-item key belongs to the parent", "root\n  items\n    - 1\n    extra 2\n    - 3\n", "{\"root\":{\"extra\":2,\"items\":[1,3]}}"],
	["list: object between list key and first item", "items:join\n  x_1\n    - k v\n      w 1\n    - z\n", "{\"items\":[],\"x_1\":[{\"k\":\"v\",\"w\":1},\"z\"]}"],
	["list: object opened between list key and item is not its parent", "items:join\n  x_1\n    y 1\n    - k v\n      w 2\n", "{\"items\":[{\"k\":\"v\",\"w\":2}],\"x_1\":{\"y\":1}}"],
	["list: scalar item closes the previous item", "items\n  - name a\n    k 1\n  - plain\n    # note\n    deep 2\n", "{\"deep\":2,\"items\":[{\"k\":1,\"name\":\"a\"},\"plain\"]}"],
	["list: items after a re-declared key", "lst\n  - a\nlst 5\n  - b\n", "{\"lst\":5}"],
	["list: a dropped item still closes the frames above it", "lst\n  - 1\n lst 5\n   obj\n     k 1\n  - 2\n     z 9\nfull\n", "{\"full\":{},\"lst\":5,\"obj\":{\"k\":1},\"z\":9}"],
	["list: frames popped before the item is checked", "a\n\t- base_hp 's'\n  a\n      x_1 [x]\n  - {cfg.v}\n   name\n", "{\"a\":[{\"a\":{\"x_1\":\"[x]\"},\"base_hp\":\"s\"},{\"name\":{},\"{cfg.v}\":{}}]}"],
	["parse: type hints", "a(int) 42.99\nb(float) 2 + 2\nc(bool) true\nd(string) 007\ne() 5\n", "{\"a\":0,\"b\":0.0,\"c\":true,\"d\":\"007\",\"e\":5}"],
	["parse: quoted and big numbers", "a \"007\"\nb 9223372036854775807\nc 9223372036854775808\nd -0.0\ne 0.30000000000000004\nf 1.\n", "{\"a\":\"007\",\"b\":9223372036854775807,\"c\":\"9223372036854775808\",\"d\":-0.0,\"e\":0.30000000000000004,\"f\":\"1.\"}"],
	["pattern: Rust regex syntax and matching", "!active\nbad_brace[pattern:^(ab){2$] abab!\nbad_esc[pattern:\\e] x\nneg_p[pattern:\\p{^Lu}] A\nascii_dot[pattern:(?-u).] x\nspaced[pattern:^a{ 2 }$] aaa\nwb[pattern:\\b{2}x] x\nempty_group[pattern:^(?:)x$] y\nend_is_text_end[pattern:^a$] |\n  a\n  b\nuni_digit[pattern:^\\d$] ١\nset_ops[pattern:^[a-z&&[^aeiou]]+$] bca\nnested[pattern:^[[ab]c]+$] abcd\n", "{\"ascii_dot\":\"x\",\"bad_brace\":\"abab!\",\"bad_esc\":\"x\",\"empty_group\":\"CONSTRAINT_ERR: 'empty_group' does not match pattern /^(?:)x$/\",\"end_is_text_end\":\"CONSTRAINT_ERR: 'end_is_text_end' does not match pattern /^a$/\",\"neg_p\":\"A\",\"nested\":\"CONSTRAINT_ERR: 'nested' does not match pattern /^[[ab]c]+$/\",\"set_ops\":\"CONSTRAINT_ERR: 'set_ops' does not match pattern /^[a-z&&[^aeiou]]+$/\",\"spaced\":\"CONSTRAINT_ERR: 'spaced' does not match pattern /^a{ 2 }$/\",\"uni_digit\":\"١\",\"wb\":\"x\"}"],
	["clamp: bounds must be finite numbers", "!active\nv:clamp:nan:5 3\nx:clamp:1:inf 7\ny:clamp:2:9 12\nz:clamp:Infinity:5 3\nw:clamp:1e400:5 3\nq:clamp:5abc:9 3\nr:clamp:-inf:1 0\nok:clamp:1e2:2e2 500\n", "{\"ok\":200,\"q\":3,\"r\":0,\"v\":3,\"w\":3,\"x\":7,\"y\":9,\"z\":3}"],
	["calc: price * 0.2", "!active\nprice 100\ntax:calc price * 0.2", "{\"price\":100,\"tax\":20}"],
	["ref and ref:calc shorthand", "!active\nbase_rate 50\nquick:ref base_rate\ndouble:ref:calc:*2 base_rate", "{\"base_rate\":50,\"double\":100,\"quick\":50}"],
	["alias copies value", "!active\nname Alice\nbackup:alias name", "{\"backup\":\"Alice\",\"name\":\"Alice\"}"],
	["alias cycle", "!active\na:alias b\nb:alias a\nc:alias c", "{\"a\":\"ALIAS_ERR: circular alias detected: a → b\",\"b\":\"ALIAS_ERR: circular alias detected: a → b\",\"c\":\"ALIAS_ERR: self-referential alias: c → c\"}"],
	["default applies to null, not to a group", "!active\nport:default:8080 null\ngroup:default:8080", "{\"group\":{},\"port\":8080}"],
	["clamp / round", "!active\nhp:clamp:0:100 150\npi:round:2 3.14159\nbad:clamp:5:1 3\nr0:round 2.5", "{\"bad\":\"CONSTRAINT_ERR: clamp min (5) > max (1)\",\"hp\":100,\"pi\":3.14,\"r0\":3}"],
	["split / join", "!active\ntags:split a, b, c\nnames:join\n  - Alice\n  - Bob\nsp:split:pipe 1|2.5|x", "{\"names\":\"Alice,Bob\",\"sp\":[1,2.5,\"x\"],\"tags\":[\"a\",\"b\",\"c\"]}"],
	["unique drops duplicates", "!active\nseen:unique\n  - 1\n  - 2\n  - 1\n  - 3", "{\"seen\":[1,2,3]}"],
	["sort / sum", "!active\nasc:sort\n  - 5\n  - 1\n  - 3\ntotal:sum\n  - 10\n  - 20\n  - 30\nfl:sum\n  - 1.5\n  - 2", "{\"asc\":[1,3,5],\"fl\":3.5,\"total\":60}"],
	["replace", "!active\nslug:replace:_:- hello_world_again", "{\"slug\":\"hello-world-again\"}"],
	["version >= passes", "!active\nok:version:>=:1.2.0 1.5.0", "{\"ok\":true}"],
	["interpolation", "!active\nname Alice\ngreeting Hello, {name}!\nmiss {nope} {a b}\npath {svc.port}\nsvc\n  port 80", "{\"greeting\":\"Hello, Alice!\",\"miss\":\"{nope} {a b}\",\"name\":\"Alice\",\"path\":\"80\",\"svc\":{\"port\":80}}"],
	["format %05d", "!active\nzip:format:%05d 42", "{\"zip\":\"00042\"}"],
	["required / type", "!active\nname[required, type:string]\nblank[required] \"\"", "{\"blank\":\"CONSTRAINT_ERR: 'blank' is required\",\"name\":\"CONSTRAINT_ERR: 'name' expected type 'string'\"}"],
	["max violated", "!active\nhp[min:0, max:100] 200.5", "{\"hp\":\"CONSTRAINT_ERR: 'hp' value 200.5 exceeds max 100\"}"],
	["enum violation", "!active\nrole[enum:admin|user] guest", "{\"role\":\"CONSTRAINT_ERR: 'role' must be one of [admin|user]\"}"],
	["pattern violation; nested quantifier skipped", "!active\nzip[pattern:^\\d{5}$] abcde\nok[pattern:^\\d{5}$] \"01234\"\nnested[pattern:^(a+)+$] b", "{\"nested\":\"b\",\"ok\":\"01234\",\"zip\":\"CONSTRAINT_ERR: 'zip' does not match pattern /^\\\\d{5}$/\"}"],
	["secret never reaches text", "!active\ntoken:secret abc-123\nmsg key={token}\nj:join\n  - {token}", "{\"j\":\"[SECRET]\",\"msg\":\"key=[SECRET]\",\"token\":\"[SECRET]\"}"],
	["zero keeps its sign", "a 0.0\nb -0.0\nc -0", "{\"a\":0.0,\"b\":-0.0,\"c\":0}"],
]


func _run_regression() -> void:
	for case in REGRESSION_CASES:
		_assert_eq(_parse_json(case[1]), case[2], "3.8.0 " + String(case[0]))


func _parse_json(text: String, opts: SynxOptions = null) -> String:
	var r := SynxParser.parse(text)
	if r.mode == SynxParseResult.Mode.ACTIVE:
		SynxEngine.resolve(r, opts if opts != null else SynxOptions.new())
	return SynxJson.to_json(r.root)


# ── Conformance ──

func _run_conformance() -> void:
	var base := _conformance_base()
	if base.is_empty():
		failures.append("conformance: tests/conformance/cases not found above the project")
		return
	var files := DirAccess.get_files_at(base)
	files.sort()
	var ran := 0
	for f in files:
		if not f.ends_with(".synx"):
			continue
		var stem := f.substr(0, f.length() - 5)
		var expected_path := base.path_join(stem + ".expected.json")
		if not FileAccess.file_exists(expected_path):
			continue
		ran += 1
		var text: String = SynxText.utf8_decode(FileAccess.get_file_as_bytes(base.path_join(f)))
		var expected := FileAccess.get_file_as_bytes(expected_path).get_string_from_utf8().strip_edges()
		var r := SynxParser.parse(text)
		if r.mode == SynxParseResult.Mode.ACTIVE:
			SynxEngine.resolve(r, SynxOptions.new())
		var root := r.root
		if SynxText.trim_start(text).begins_with("!tool"):
			root = SynxParser.reshape_tool_output(root, r.schema)
		_assert_eq(SynxJson.to_json(root), expected, "conformance/" + stem)
	if ran == 0:
		failures.append("conformance: no cases in " + base)


# The repository's tests/conformance/cases, searched upwards from the project.
func _conformance_base() -> String:
	var dir := ProjectSettings.globalize_path("res://").rstrip("/")
	for _i in 6:
		var candidate := dir.path_join("tests/conformance/cases")
		if DirAccess.dir_exists_absolute(candidate):
			return candidate
		dir = dir.get_base_dir()
	return ""


# ── Assertions ──

func _assert(cond: bool, msg: String) -> void:
	if cond:
		passed += 1
		print("· %s ✓" % msg)
	else:
		failures.append(msg)
		print("· %s ✗" % msg)


func _assert_eq(actual: Variant, expected: Variant, msg: String) -> void:
	if typeof(actual) == typeof(expected) and actual == expected:
		passed += 1
		print("· %s ✓" % msg)
	else:
		failures.append("%s — got: %s | want: %s" % [msg, str(actual), str(expected)])
		print("· %s ✗ got=%s want=%s" % [msg, str(actual), str(expected)])


static func _bits(x: float) -> String:
	var b := PackedByteArray()
	b.resize(8)
	b.encode_double(0, x)
	return b.hex_encode()


# ── Unit tests ──

func _run_unit_tests() -> void:
	_test_scalar_parse()
	_test_nested()
	_test_lists()
	_test_multiline()
	_test_quoted_strings()
	_test_active_env()
	_test_active_inherit()
	_test_active_random_weighted()
	_test_active_i18n()
	_test_active_i18n_plural_ru()
	_test_active_secret_redaction()
	_test_active_secret_redaction_in_json()
	_test_calc_evaluator()
	_test_calc_deep_parentheses()
	_test_numbers_parse_correctly_rounded()
	_test_numbers_shortest_text()
	_test_metadata_paths()
	_test_diff_basic()
	_test_binary_roundtrip()
	_test_binary_roundtrip_active()
	_test_binary_deterministic_and_exact()
	_test_binary_reads_synx_core()
	_test_stringify()
	_test_stringify_round_trip()
	_test_formatter_canonical()
	_test_formatter_keeps_block_body()
	_test_formatter_depth_cap()
	_test_include_self_depth()
	_test_include_interpolation()
	_test_use_packages_contained()
	_test_tool_call_reshape()
	_test_tool_schema_reshape()
	_test_to_variant()


func _test_scalar_parse() -> void:
	var d: Dictionary = SynxParser.parse("name Alice\nage 30\nactive true\nscore 99.5\nempty null").root.data
	_assert_eq((d["name"] as SynxValue).data, "Alice", "scalar parse: name")
	_assert_eq((d["age"] as SynxValue).data, 30, "scalar parse: age (int)")
	_assert_eq((d["active"] as SynxValue).data, true, "scalar parse: active (bool)")
	_assert_eq((d["score"] as SynxValue).data, 99.5, "scalar parse: score (float)")
	_assert((d["empty"] as SynxValue).is_null(), "scalar parse: empty (null)")


func _test_nested() -> void:
	var d: Dictionary = SynxParser.parse("server\n  host 0.0.0.0\n  port 8080").root.data
	var server: SynxValue = d["server"]
	_assert_eq((server.data["host"] as SynxValue).data, "0.0.0.0", "nested: server.host")
	_assert_eq((server.data["port"] as SynxValue).data, 8080, "nested: server.port")


func _test_lists() -> void:
	var d: Dictionary = SynxParser.parse("inventory\n  - Sword\n  - Shield\n  - Potion").root.data
	var inv: SynxValue = d["inventory"]
	_assert_eq((inv.data as Array).size(), 3, "list: 3 items")
	_assert_eq((inv.data[0] as SynxValue).data, "Sword", "list: first item")


func _test_multiline() -> void:
	var d: Dictionary = SynxParser.parse("rules |\n  Rule one.\n  Rule two.\n  Rule three.").root.data
	_assert_eq((d["rules"] as SynxValue).data, "Rule one.\nRule two.\nRule three.", "multiline block")


func _test_quoted_strings() -> void:
	var d: Dictionary = SynxParser.parse("port \"3000\"\nname 'Alice'").root.data
	_assert_eq((d["port"] as SynxValue).data, "3000", "quoted preserves string")
	_assert_eq((d["port"] as SynxValue).kind, SynxValue.Kind.STRING, "quoted prevents int cast")


func _test_active_env() -> void:
	var r := SynxParser.parse("!active\nport:env:default:3000 SYNX_TEST_PORT")
	var o := SynxOptions.new()
	o.env = {"SYNX_TEST_PORT": "9090"}
	SynxEngine.resolve(r, o)
	_assert_eq((r.root.data["port"] as SynxValue).data, 9090, "env resolves with override")






func _test_active_inherit() -> void:
	var r := SynxParser.parse("!active\n_base\n  weight 10\n  stackable true\nsteel:inherit:_base\n  weight 25\n  material metal")
	SynxEngine.resolve(r, SynxOptions.new())
	_assert(not (r.root.data as Dictionary).has("_base"), "inherit: _base stripped")
	var steel: SynxValue = r.root.data["steel"]
	_assert_eq((steel.data["weight"] as SynxValue).data, 25, "inherit: child overrides")
	_assert_eq((steel.data["stackable"] as SynxValue).data, true, "inherit: parent field copied")
	_assert_eq((steel.data["material"] as SynxValue).data, "metal", "inherit: child-only field present")







func _test_active_random_weighted() -> void:
	SynxRng.set_seed(42)
	var r := SynxParser.parse("!active\ntier:random 100\n  - Common\n  - Rare\n  - Epic")
	SynxEngine.resolve(r, SynxOptions.new())
	var t: SynxValue = r.root.data["tier"]
	_assert(t.kind == SynxValue.Kind.STRING, "weighted random returns an item")




func _test_active_i18n() -> void:
	var o := SynxOptions.new()
	o.lang = "ru"
	_assert_eq(_parse_json("!active\ntitle:i18n\n  en Hello\n  ru Привет", o), "{\"title\":\"Привет\"}", "i18n selects ru")


func _test_active_i18n_plural_ru() -> void:
	var o := SynxOptions.new()
	o.lang = "ru"
	var doc := "!active\nitem_count 3\ntitle:i18n:item_count\n  en\n    one {count} item\n    other {count} items\n  ru\n    one {count} предмет\n    few {count} предмета\n    many {count} предметов\n    other {count} предметов"
	_assert_eq(_parse_json(doc, o), "{\"item_count\":3,\"title\":\"3 предмета\"}", "ru plural 'few' for 3")







func _test_active_secret_redaction() -> void:
	var r := SynxParser.parse("!active\ntoken:secret abc-123")
	SynxEngine.resolve(r, SynxOptions.new())
	var v: SynxValue = r.root.data["token"]
	_assert(v.kind == SynxValue.Kind.SECRET, "secret stored as SECRET kind")
	_assert_eq(v.as_secret(), "abc-123", "secret retains underlying value")


func _test_active_secret_redaction_in_json() -> void:
	var r := SynxParser.parse("!active\ntoken:secret abc-123\nmsg key={token}")
	SynxEngine.resolve(r, SynxOptions.new())
	var json := SynxJson.to_json(r.root)
	_assert_eq(json, "{\"msg\":\"key=[SECRET]\",\"token\":\"[SECRET]\"}", "JSON and interpolation never see a secret")
	_assert(not SynxStringify.stringify(r.root).contains("abc-123"), "stringify masks a secret")


func _test_calc_evaluator() -> void:
	_assert_eq(SynxSafeCalc.evaluate("2 + 3 * 4")["value"], 14.0, "calc precedence")
	_assert_eq(SynxSafeCalc.evaluate("(2 + 3) * 4")["value"], 20.0, "calc parens")
	_assert_eq(SynxSafeCalc.evaluate("10 / 0")["error"], "SYNX :calc — division by zero", "calc div by zero")
	_assert_eq(SynxSafeCalc.evaluate("-5 + 3")["value"], -2.0, "calc unary minus")
	_assert_eq(SynxSafeCalc.evaluate("2 цена")["error"], "SYNX :calc — unexpected character: 'ц' in expression", "calc keeps a non-ASCII character whole")


func _test_calc_deep_parentheses() -> void:
	var expr := "(".repeat(2000) + "1" + ")".repeat(2000)
	_assert_eq(SynxSafeCalc.evaluate(expr)["value"], 1.0, "calc: 2000 nested parentheses do not overflow the stack")


func _test_numbers_parse_correctly_rounded() -> void:
	# Godot's String.to_float is one ULP off on these; Rust is correctly rounded.
	_assert_eq(_bits(SynxNum.parse_f64("7503036609.1172905")), "6ce0110c74f3fb41", "f64 parse: 17 digits rounded correctly")
	_assert_eq(_bits(SynxNum.parse_f64("2.2250738585072011e-308")), "ffffffffffff0f00", "f64 parse: below DBL_MIN")
	_assert_eq(_bits(SynxNum.parse_f64("4.9406564584124654e-324")), "0100000000000000", "f64 parse: smallest subnormal")
	_assert_eq(SynxNum.parse_f64("1.7976931348623159e308"), INF, "f64 parse: overflow is inf")
	_assert(SynxNum.parse_f64("1.2.3") == null and SynxNum.parse_f64("-") == null and SynxNum.parse_f64(" 1") == null, "f64 parse: Rust grammar")
	_assert(SynxNum.parse_i64("9223372036854775808") == null and SynxNum.parse_i64("-9223372036854775808") == -9223372036854775807 - 1, "i64 parse: range")


func _test_numbers_shortest_text() -> void:
	_assert_eq(SynxNum.ryu(0.1 + 0.2), "0.30000000000000004", "ryu: shortest digits")
	_assert_eq(SynxNum.ryu(1e21), "1e21", "ryu: exponent form")
	_assert_eq(SynxNum.ryu(1e-7), "1e-7", "ryu: small exponent form")
	_assert_eq(SynxNum.ryu(100.0), "100.0", "ryu: whole float keeps .0")
	_assert_eq(SynxNum.display(1e21), "1000000000000000000000", "Display: no exponent")
	_assert_eq(SynxNum.display(0.0000001), "0.0000001", "Display: plain small decimal")
	_assert_eq(SynxNum.display(1108830226480205.25), "1108830226480205.3", "Display: an exact tie rounds up (Rust Dragon)")
	_assert_eq(SynxNum.ryu(1108830226480205.25), "1108830226480205.2", "ryu: an exact tie rounds to even")
	_assert_eq(SynxNum.fixed(2.5, 0), "2", "fixed: ties to even")
	_assert_eq(SynxNum.fixed(0.1, 20), "0.10000000000000000555", "fixed: exact expansion")



func _test_metadata_paths() -> void:
	var r := SynxParser.parse("!active\na.b:env X\nlist\n  - name a\n    port:env P\n")
	var md: Dictionary = r.metadata
	_assert(md.has("") and (md[""] as Dictionary).has("a.b"), "metadata: dotted key in the root map")
	_assert(md.has("list.#0") and (md["list.#0"] as Dictionary).has("port"), "metadata: list item path list.#0")
	_assert_eq(SynxMeta.key_segment("a.b\\c"), "a\\.b\\\\c", "metadata: key escaping")
	_assert_eq(SynxMeta.path_segments("a\\.b.c"), PackedStringArray(["a.b", "c"]), "metadata: path segments")


func _test_diff_basic() -> void:
	var a := {"x": SynxValue.make_int(1), "y": SynxValue.make_int(2)}
	var b := {"x": SynxValue.make_int(1), "z": SynxValue.make_int(3)}
	var d := SynxDiff.diff(a, b)
	_assert_eq((d["added"] as Dictionary).size(), 1, "diff: added")
	_assert_eq((d["removed"] as Dictionary).size(), 1, "diff: removed")
	_assert_eq((d["unchanged"] as Array).size(), 1, "diff: unchanged")


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
	var back := SynxBinary.decompile(SynxBinary.compile(r, false))
	_assert(bool(back["ok"]), "binary active: decompile ok")
	var restored: SynxParseResult = back["result"]
	_assert(restored.mode == SynxParseResult.Mode.ACTIVE, "binary active: mode preserved")
	var meta: SynxMeta = (restored.root.meta as Dictionary)["port"]
	_assert(meta.markers == PackedStringArray(["env", "default", "3000"]) and meta.c_max == 65535.0, "binary active: declarations preserved")


func _test_binary_deterministic_and_exact() -> void:
	var text := "b 9007199254740993\na -9223372036854775808\nc 9223372036854775807\nd x\ne x\nf y\n"
	var r1 := SynxParser.parse(text)
	var bin1 := SynxBinary.compile(r1, false)
	var bin2 := SynxBinary.compile(SynxParser.parse(text), false)
	_assert(bin1 == bin2, "binary: the same document compiles to the same bytes")
	var restored: SynxParseResult = SynxBinary.decompile(bin1)["result"]
	_assert_eq(SynxJson.to_json(restored.root), "{\"a\":-9223372036854775808,\"b\":9007199254740993,\"c\":9223372036854775807,\"d\":\"x\",\"e\":\"x\",\"f\":\"y\"}", "binary: 64-bit integers stay exact")


# Files written by synx-core 3.8.0 (`synx compile`): raw DEFLATE, which
# Godot's own inflater cannot read.
const SYNXB_FROM_RUST_STATIC := "53594e584201005700000035c0d1098020100660cfff4eab291aaa050c4c82b2481f7a0a6992362d28fa1a8e6ef6dcf994795db68c7e0c883ed04089764c2993b31638cbab15569f52192d35580931ee1f695c8711fb00"
const SYNXB_FROM_RUST_ACTIVE := "53594e58420105760000001d894d0ac2301046e7cbcc34f577e115dc4b445ceb0d14f1024123084d2d1af5babd417b8396f62d1e3cde52aad73be12fe7d3e5aacf14e2474a1f033c71287ff61e1efe5b24d939e7f2716fd64e6f858f15b6d85bc319ac1155f08ad4980cc4369f108169a43d0caebbe6380578365f0cd903"

func _test_binary_reads_synx_core() -> void:
	var a := SynxBinary.decompile(SYNXB_FROM_RUST_STATIC.hex_decode())
	_assert(bool(a["ok"]), "binary: reads a synx-core .synxb")
	if bool(a["ok"]):
		_assert_eq(SynxJson.to_json((a["result"] as SynxParseResult).root), "{\"big\":9007199254740993,\"f\":-0.0,\"lst\":[\"a\",2],\"name\":\"Test\",\"neg\":-9223372036854775808,\"port\":8080,\"s\":\"x\"}", "binary: synx-core values")
	var b := Synx.decompile(SYNXB_FROM_RUST_ACTIVE.hex_decode())
	_assert_eq(b["text"], "!active\n\nitems\n  - name a\n    w 9\nport PORT\n", "binary: synx-core active file decompiles to the same text")
	var c := SynxBinary.decompile(SYNXB_FROM_RUST_ACTIVE.hex_decode())
	if bool(c["ok"]):
		var md: Dictionary = (c["result"] as SynxParseResult).metadata
		_assert(md.has("items.#0") and md.has(""), "binary: synx-core metadata paths attach to the tree")


func _test_stringify() -> void:
	var v := SynxValue.make_object({
		"name": SynxValue.make_string("Test"),
		"port": SynxValue.make_int(8080),
	})
	_assert_eq(SynxStringify.stringify(v), "name Test\nport 8080\n", "stringify: plain keys")


func _test_stringify_round_trip() -> void:
	var doc := "a \"007\"\nb \"true\"\nc |+\n  x # y\n    indented\nd\n  - k 1\n  - only 1\n    deep 2\n  - z\ne:join\nf \"\"\ng 1.0\n"
	var r := SynxParser.parse(doc)
	var text := SynxStringify.stringify(r.root)
	# The text synx-core writes for this document.
	_assert_eq(text, "a \"007\"\nb \"true\"\nc |+\n  x # y\n    indented\nd\n  - k 1\n  - deep 2\n    only 1\n  - z\ne:join\nf \"\"\ng 1.0\n", "stringify: quotes what would cast, keeps |+")
	_assert_eq(SynxJson.to_json(SynxParser.parse(text).root), SynxJson.to_json(r.root), "stringify: parses back to the same data")
	var empty_list := SynxValue.make_object({"l": SynxValue.make_array([]), "one": SynxValue.make_array([SynxValue.make_object({"k": SynxValue.make_int(1)})])})
	var t2 := SynxStringify.stringify(empty_list)
	_assert_eq(t2, "l:join\none\n  - k 1\n    k 1\n", "stringify: empty list is key:join, one-key item repeats its head")
	_assert_eq(SynxJson.to_json(SynxParser.parse(t2).root), "{\"l\":[],\"one\":[{\"k\":1}]}", "stringify: those read back")


func _test_formatter_canonical() -> void:
	var canonical := SynxFormatter.format("port 8080\nname Test\n# comment\n\n")
	_assert_eq(canonical, "name Test\nport 8080\n", "format: sorted, comments stripped, one trailing newline")


func _test_formatter_keeps_block_body() -> void:
	var src := "code |+\n  zeta()\n    alpha()\n  beta\nname x\nb 1\na first\nB 2\na second\n"
	var out := SynxFormatter.format(src)
	_assert_eq(out, "a first\na second\nb 1\nB 2\ncode |+\n  zeta()\n    alpha()\n  beta\n\nname x\n", "format: |+ body kept, stable sort")
	_assert_eq(SynxFormatter.format(out), out, "format: idempotent")


# Expected text as `synx format` writes it: past depth 128 the subtree is
# dropped and formatting goes on with the next line above the cap.
func _test_formatter_depth_cap() -> void:
	var lines := PackedStringArray()
	for i in 140:
		lines.append(" ".repeat(i) + "k%d" % i)
	var want := ""
	for i in 129:
		want += "  ".repeat(i) + "k%d\n" % i
	_assert_eq(SynxFormatter.format("\n".join(lines) + "\n"), want, "format: a subtree deeper than 128 is dropped")
	lines = PackedStringArray()
	for i in 140:
		lines.append("  ".repeat(i) + "k%d" % i)
	lines.append_array(PackedStringArray([" ".repeat(258) + "deep_sibling 1", "", "    back 1", "  b2 2", "top 3", "a 0"]))
	want = "a 0\nk0\n  b2 2\n  k1\n    back 1\n"
	for i in range(2, 129):
		want += "  ".repeat(i) + "k%d\n" % i
	want += "\ntop 3\n"
	_assert_eq(SynxFormatter.format("\n".join(lines) + "\n"), want, "format: lines above the cap after a dropped subtree are kept")


# A scratch directory under user:// for the file-based tests.
func _scratch_dir(name: String) -> String:
	var dir := ProjectSettings.globalize_path("user://synx_tests").path_join(name)
	_remove_tree(dir)
	DirAccess.make_dir_recursive_absolute(dir)
	return dir


func _write(path: String, text: String) -> void:
	DirAccess.make_dir_recursive_absolute(path.get_base_dir())
	var f := FileAccess.open(path, FileAccess.WRITE)
	f.store_string(text)
	f.close()


func _remove_tree(dir: String) -> void:
	if not DirAccess.dir_exists_absolute(dir):
		return
	for sub in DirAccess.get_directories_at(dir):
		_remove_tree(dir.path_join(sub))
	for f in DirAccess.get_files_at(dir):
		DirAccess.remove_absolute(dir.path_join(f))
	DirAccess.remove_absolute(dir)


func _parse_file_json(path: String) -> String:
	var r := SynxParser.parse(SynxText.utf8_decode(FileAccess.get_file_as_bytes(path)))
	var o := SynxOptions.new()
	o.base_path = path.get_base_dir()
	if r.mode == SynxParseResult.Mode.ACTIVE:
		SynxEngine.resolve(r, o)
	return SynxJson.to_json(r.root)


# A file that includes itself nests 16 levels, then reports the depth cap
# (the reference's pre-pass recursed without a limit).
func _test_include_self_depth() -> void:
	var dir := _scratch_dir("include_self")
	_write(dir.path_join("self.synx"), "!active\nme:include ./self.synx\nv 1\n")
	var want := "\"INCLUDE_ERR: max include depth (16) exceeded\""
	for i in 17:
		want = "{\"me\":" + want + ",\"v\":1}"
	_assert_eq(_parse_file_json(dir.path_join("self.synx")), want, "include: a file including itself stops at depth 16")
	_remove_tree(dir)


# `{key:include}` reads the one included file; with two it stays as written.
func _test_include_interpolation() -> void:
	var dir := _scratch_dir("include_interp")
	_write(dir.path_join("common.synx"), "site_name Demo\n")
	_write(dir.path_join("one.synx"), "!active\ndb:include ./common.synx\ngreet {site_name:include}\nby {site_name:db}\n")
	_write(dir.path_join("two.synx"), "!active\n!include ./common.synx\ndb:include ./common.synx\ngreet {site_name:include}\n")
	_assert_eq(_parse_file_json(dir.path_join("one.synx")), "{\"by\":\"Demo\",\"db\":{\"site_name\":\"Demo\"},\"greet\":\"Demo\"}", "include: {key:include} with one :include")
	_assert_eq(_parse_file_json(dir.path_join("two.synx")), "{\"db\":{\"site_name\":\"Demo\"},\"greet\":\"{site_name:include}\"}", "include: {key:include} with two includes stays")
	_remove_tree(dir)


# `!use` names and manifest entries must stay inside the packages directory;
# a bad name skips the package, a bad manifest field counts as absent.
func _test_use_packages_contained() -> void:
	var dir := _scratch_dir("packages")
	var pkg := dir.path_join("synx_packages/@scope")
	_write(pkg.path_join("good/src/main.synx"), "v good\n")
	_write(pkg.path_join("manifest/synx-pkg.synx"), "main lib/entry.synx\n")
	_write(pkg.path_join("manifest/lib/entry.synx"), "v manifest\n")
	_write(pkg.path_join("badmain/synx-pkg.synx"), "main ../../../outside/src/main.synx\nentry ok.synx\n")
	_write(pkg.path_join("badmain/ok.synx"), "v entry\n")
	_write(pkg.path_join("badmain2/synx-pkg.synx"), "main C:/x.synx\n")
	_write(pkg.path_join("badmain2/src/main.synx"), "v src\n")
	_write(dir.path_join("outside/src/main.synx"), "v ESCAPED\n")
	_write(dir.path_join("main.synx"), "!active\n!use @scope/good\n!use @scope/manifest\n!use @scope/badmain\n!use @scope/badmain2\n!use @scope/../../outside as esc\nx 1\n")
	_assert_eq(_parse_file_json(dir.path_join("main.synx")), "{\"badmain\":{\"v\":\"entry\"},\"badmain2\":{\"v\":\"src\"},\"good\":{\"v\":\"good\"},\"manifest\":{\"v\":\"manifest\"},\"x\":1}", "!use: packages and manifest entries stay inside the packages directory")
	_remove_tree(dir)


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
	_assert_eq((tools.data as Array).size(), 2, "schema reshape: 2 tools")
	_assert_eq((((tools.data[0] as SynxValue).data["name"]) as SynxValue).data, "memory_write", "schema reshape: sorted (memory first)")


func _test_to_variant() -> void:
	var d: Dictionary = Synx.parse_to_variant("name Alice\nage 30\nactive true")
	_assert_eq(d["name"], "Alice", "to_variant: name unwrapped")
	_assert_eq(d["age"], 30, "to_variant: age unwrapped")
	_assert_eq(d["active"], true, "to_variant: active unwrapped")
