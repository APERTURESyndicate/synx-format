@tool
class_name Synx
extends Node

# Top-level SYNX facade — entry points matching synx-core::Synx and the C# SynxFormat.
#
# This Node is registered as an autoload (singleton) by `plugin.gd`, so user code
# can call:
#   var data := Synx.parse(text)
#   var json := Synx.to_json_value(Synx.parse_active(text, opts))
# without instantiating anything.
#
# Every function is also exposed as `static` to allow `Synx.<fn>` use from scripts
# that don't depend on the autoload. Internally we just forward to the static
# member.

# ─── Parse ──

static func parse(text: String) -> Dictionary:
	# Returns the top-level Dictionary[String, SynxValue] (static mode — no engine).
	var r := SynxParser.parse(text)
	if r.root.kind == SynxValue.Kind.OBJECT:
		return r.root.data
	return {}


static func parse_active(text: String, options: SynxOptions = null) -> Dictionary:
	var r := SynxParser.parse(text)
	if r.mode == SynxParseResult.Mode.ACTIVE:
		SynxEngine.resolve(r, options if options != null else SynxOptions.new())
	if r.root.kind == SynxValue.Kind.OBJECT:
		return r.root.data
	return {}


static func parse_full(text: String) -> SynxParseResult:
	return SynxParser.parse(text)


static func parse_full_active(text: String, options: SynxOptions = null) -> SynxParseResult:
	var r := SynxParser.parse(text)
	if r.mode == SynxParseResult.Mode.ACTIVE:
		SynxEngine.resolve(r, options if options != null else SynxOptions.new())
	return r


# Parse a !tool file and reshape to { tool, params } or { tools: [...] }.
static func parse_tool(text: String, options: SynxOptions = null) -> Dictionary:
	var r := SynxParser.parse(text)
	if r.mode == SynxParseResult.Mode.ACTIVE:
		SynxEngine.resolve(r, options if options != null else SynxOptions.new())
	var shaped := SynxParser.reshape_tool_output(r.root, r.schema)
	if shaped.kind == SynxValue.Kind.OBJECT:
		return shaped.data
	return {}


# ─── Variants helpers (Godot-idiomatic) ──

# Parse and immediately collapse to plain Variant values (no SynxValue wrappers).
# Useful when callers don't care about secrets / metadata round-trips.
static func parse_to_variant(text: String) -> Variant:
	var r := SynxParser.parse(text)
	return r.root.to_variant()


static func parse_active_to_variant(text: String, options: SynxOptions = null) -> Variant:
	var r := SynxParser.parse(text)
	if r.mode == SynxParseResult.Mode.ACTIVE:
		SynxEngine.resolve(r, options if options != null else SynxOptions.new())
	return r.root.to_variant()


# ─── JSON ──

static func to_json(value) -> String:
	if value is SynxValue:
		return SynxJson.to_json(value)
	if value is Dictionary:
		return SynxJson.to_json(SynxValue.make_object(value))
	if value is SynxParseResult:
		return SynxJson.to_json(value.root)
	if value is Array:
		var arr: Array = []
		for item in value:
			arr.append(SynxValue.from_variant(item))
		return SynxJson.to_json(SynxValue.make_array(arr))
	return SynxJson.to_json(SynxValue.from_variant(value))


# ─── Stringify / format (SYNX text out) ──

static func stringify(value) -> String:
	if value is SynxValue:
		return SynxStringify.stringify(value)
	if value is Dictionary:
		# Promote a plain Dictionary[String, SynxValue|Variant] to SynxValue.
		var promoted: Dictionary = {}
		for k in value.keys():
			var v = value[k]
			promoted[k] = v if v is SynxValue else SynxValue.from_variant(v)
		return SynxStringify.stringify(SynxValue.make_object(promoted))
	if value is SynxParseResult:
		return SynxStringify.stringify(value.root)
	return SynxStringify.stringify(SynxValue.from_variant(value))


static func format(text: String) -> String:
	return SynxFormatter.format(text)


# ─── .synxb binary ──

static func compile(text: String, resolved: bool = false) -> PackedByteArray:
	var r := SynxParser.parse(text)
	if resolved and r.mode == SynxParseResult.Mode.ACTIVE:
		SynxEngine.resolve(r, SynxOptions.new())
	return SynxBinary.compile(r, resolved)


# Returns { "ok": bool, "text": String, "error": String }.
static func decompile(data: PackedByteArray) -> Dictionary:
	var r := SynxBinary.decompile(data)
	if not bool(r["ok"]):
		return r
	var pr: SynxParseResult = r["result"]
	var out := ""
	if pr.tool_directive:
		out += "!tool\n"
	if pr.schema:
		out += "!schema\n"
	if pr.llm:
		out += "!llm\n"
	if pr.mode == SynxParseResult.Mode.ACTIVE:
		out += "!active\n"
	if pr.locked:
		out += "!lock\n"
	if not out.is_empty():
		out += "\n"
	out += SynxStringify.stringify(pr.root)
	return {"ok": true, "text": out, "error": ""}


static func is_synxb(data: PackedByteArray) -> bool:
	return SynxBinary.is_synxb(data)


# ─── Diff ──

# `a` / `b` accept Dictionary[String, SynxValue] OR SynxParseResult.
static func diff(a, b) -> Dictionary:
	var da: Dictionary = _as_dict(a)
	var db: Dictionary = _as_dict(b)
	return SynxDiff.diff(da, db)


static func diff_to_value(d: Dictionary) -> SynxValue:
	return SynxDiff.diff_to_value(d)


static func _as_dict(v) -> Dictionary:
	if v is Dictionary:
		return v
	if v is SynxParseResult:
		if v.root.kind == SynxValue.Kind.OBJECT:
			return v.root.data
	if v is SynxValue and v.kind == SynxValue.Kind.OBJECT:
		return v.data
	return {}


# ─── File helpers ──

static func load_file(path: String) -> SynxParseResult:
	var f := FileAccess.open(path, FileAccess.READ)
	if f == null:
		var pr := SynxParseResult.new()
		return pr
	var text := f.get_as_text()
	f.close()
	return SynxParser.parse(text)
