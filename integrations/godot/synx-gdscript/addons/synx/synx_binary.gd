@tool
class_name SynxBinary
extends RefCounted

# .synxb compact binary format — wire-compatible with synx-core::binary.
#
# Layout (the header is uncompressed, the payload is raw DEFLATE, RFC 1951):
#   5 bytes  magic "SYNXB"
#   1 byte   format version (1)
#   1 byte   flags (active / locked / has_metadata / resolved / tool / schema / llm)
#   4 bytes  uncompressed payload length (LE)
#   N bytes  raw DEFLATE payload
#
# Payload:
#   • string table (varint count + (varint len + UTF-8 bytes) ×N), ordered by
#     use count (ties in first-seen order), so the same document always gives
#     the same bytes and frequent strings get one-byte indexes
#   • root value (recursive, strings encoded as table indices, object keys
#     sorted, integers as zigzag varints — all 64 bits exact)
#   • [optional] metadata (synx-core dot paths) + includes when FLAG_HAS_META
#
# Godot's DEFLATE mode writes a zlib stream (2-byte header, Adler-32
# trailer) and its inflater insists on that trailer, while synx-core writes
# and reads raw DEFLATE. Compile strips the zlib wrapper; decompile inflates
# with the decoder below.

const MAGIC := "SYNXB"
const FORMAT_VERSION := 1
const MAX_DECODED_BYTES := 256 * 1024 * 1024
const MAX_DECODE_DEPTH := 256

const FLAG_ACTIVE := 0x01
const FLAG_LOCKED := 0x02
const FLAG_HAS_META := 0x04
const FLAG_RESOLVED := 0x08
const FLAG_TOOL := 0x10
const FLAG_SCHEMA := 0x20
const FLAG_LLM := 0x40

const TAG_NULL := 0x00
const TAG_FALSE := 0x01
const TAG_TRUE := 0x02
const TAG_INT := 0x03
const TAG_FLOAT := 0x04
const TAG_STRING := 0x05
const TAG_ARRAY := 0x06
const TAG_OBJECT := 0x07
const TAG_SECRET := 0x08


# ─── Varint (LEB128 of a u64 held in an int) / zigzag ──

static func _encode_varint(out: PackedByteArray, val: int) -> void:
	var v := val
	while true:
		var byte := v & 0x7f
		v = (v >> 7) & 0x01FFFFFFFFFFFFFF
		if v == 0:
			out.append(byte)
			return
		out.append(byte | 0x80)


static func _zigzag_encode(n: int) -> int:
	return (n << 1) ^ (n >> 63)


static func _zigzag_decode(u: int) -> int:
	return ((u >> 1) & 0x7FFFFFFFFFFFFFFF) ^ -(u & 1)


static func _encode_f64(out: PackedByteArray, f: float) -> void:
	var b := PackedByteArray()
	b.resize(8)
	b.encode_double(0, f)
	out.append_array(b)


# ─── String table ──

class _StringTable extends RefCounted:
	var strings := PackedStringArray()
	var index: Dictionary = {}
	var uses := PackedInt64Array()

	func intern(s: String) -> void:
		var i: Variant = index.get(s)
		if i != null:
			uses[i] += 1
			return
		index[s] = strings.size()
		strings.append(s)
		uses.append(1)

	# Descending use count, ties in first-seen order (synx-core `order_by_use`).
	func order_by_use() -> void:
		var order: Array = []
		for i in strings.size():
			order.append(i)
		var u := uses
		order.sort_custom(func(a: int, b: int) -> bool:
			if u[a] != u[b]:
				return u[a] > u[b]
			return a < b)
		var sorted := PackedStringArray()
		for new_idx in order.size():
			var s := strings[order[new_idx]]
			index[s] = new_idx
			sorted.append(s)
		strings = sorted

	func collect_value(v: SynxValue) -> void:
		match v.kind:
			SynxValue.Kind.STRING, SynxValue.Kind.SECRET:
				intern(v.data)
			SynxValue.Kind.ARRAY:
				for item in v.data:
					collect_value(item)
			SynxValue.Kind.OBJECT:
				for k in v.data:
					intern(k)
					collect_value(v.data[k])

	func collect_metadata(metadata: Dictionary) -> void:
		for path in metadata:
			intern(path)
			var mm: Dictionary = metadata[path]
			for key in mm:
				intern(key)
				var meta: SynxMeta = mm[key]
				for m in meta.markers:
					intern(m)
				for a in meta.args:
					intern(a)
				if meta.type_hint != null:
					intern(meta.type_hint)
				if meta.has_constraints:
					if meta.c_type_name != null:
						intern(meta.c_type_name)
					if meta.c_pattern != null:
						intern(meta.c_pattern)
					if meta.c_enum != null:
						for ev in meta.c_enum:
							intern(ev)

	func collect_includes(includes: Array) -> void:
		for inc in includes:
			intern(inc.path)
			intern(inc.alias)

	func encode(out: PackedByteArray) -> void:
		SynxBinary._encode_varint(out, strings.size())
		for s in strings:
			var bytes := s.to_utf8_buffer()
			SynxBinary._encode_varint(out, bytes.size())
			out.append_array(bytes)

	func idx(s: String) -> int:
		return index[s]


# ─── Encoding ──

static func _encode_value(out: PackedByteArray, v: SynxValue, st: _StringTable) -> void:
	match v.kind:
		SynxValue.Kind.NULL:
			out.append(TAG_NULL)
		SynxValue.Kind.BOOL:
			out.append(TAG_TRUE if v.data else TAG_FALSE)
		SynxValue.Kind.INT:
			out.append(TAG_INT)
			_encode_varint(out, _zigzag_encode(v.data))
		SynxValue.Kind.FLOAT:
			out.append(TAG_FLOAT)
			_encode_f64(out, v.data)
		SynxValue.Kind.STRING:
			out.append(TAG_STRING)
			_encode_varint(out, st.idx(v.data))
		SynxValue.Kind.SECRET:
			out.append(TAG_SECRET)
			_encode_varint(out, st.idx(v.data))
		SynxValue.Kind.ARRAY:
			out.append(TAG_ARRAY)
			var arr: Array = v.data
			_encode_varint(out, arr.size())
			for item in arr:
				_encode_value(out, item, st)
		SynxValue.Kind.OBJECT:
			out.append(TAG_OBJECT)
			var keys: Array = (v.data as Dictionary).keys()
			keys.sort()
			_encode_varint(out, keys.size())
			for k in keys:
				_encode_varint(out, st.idx(k))
				_encode_value(out, v.data[k], st)


static func _encode_meta(out: PackedByteArray, meta: SynxMeta, st: _StringTable) -> void:
	_encode_varint(out, meta.markers.size())
	for m in meta.markers:
		_encode_varint(out, st.idx(m))
	_encode_varint(out, meta.args.size())
	for a in meta.args:
		_encode_varint(out, st.idx(a))
	if meta.type_hint != null:
		out.append(1)
		_encode_varint(out, st.idx(meta.type_hint))
	else:
		out.append(0)
	if not meta.has_constraints:
		out.append(0)
		return
	out.append(1)
	var bits := 0
	if meta.c_min != null: bits |= 0x01
	if meta.c_max != null: bits |= 0x02
	if meta.c_type_name != null: bits |= 0x04
	if meta.c_required: bits |= 0x08
	if meta.c_readonly: bits |= 0x10
	if meta.c_pattern != null: bits |= 0x20
	if meta.c_enum != null: bits |= 0x40
	out.append(bits)
	if meta.c_min != null:
		_encode_f64(out, meta.c_min)
	if meta.c_max != null:
		_encode_f64(out, meta.c_max)
	if meta.c_type_name != null:
		_encode_varint(out, st.idx(meta.c_type_name))
	if meta.c_pattern != null:
		_encode_varint(out, st.idx(meta.c_pattern))
	if meta.c_enum != null:
		var ev: PackedStringArray = meta.c_enum
		_encode_varint(out, ev.size())
		for v in ev:
			_encode_varint(out, st.idx(v))


static func _encode_metadata(out: PackedByteArray, metadata: Dictionary, st: _StringTable) -> void:
	var paths: Array = metadata.keys()
	paths.sort()
	_encode_varint(out, paths.size())
	for path in paths:
		_encode_varint(out, st.idx(path))
		var mm: Dictionary = metadata[path]
		var keys: Array = mm.keys()
		keys.sort()
		_encode_varint(out, keys.size())
		for k in keys:
			_encode_varint(out, st.idx(k))
			_encode_meta(out, mm[k], st)


# ─── Decoding ──

class _Reader extends RefCounted:
	var data: PackedByteArray
	var pos: int = 0
	var err: String = ""
	var strings := PackedStringArray()

	func _init(d: PackedByteArray) -> void:
		data = d

	func fail(msg: String) -> void:
		if err.is_empty():
			err = msg

	func varint() -> int:
		var result := 0
		var shift := 0
		while true:
			if pos >= data.size():
				fail("unexpected end of data in varint")
				return 0
			var byte := data[pos]
			pos += 1
			result |= (byte & 0x7f) << shift
			if (byte & 0x80) == 0:
				return result
			shift += 7
			if shift >= 64:
				fail("varint overflow")
				return 0
		return 0

	func byte_at() -> int:
		if pos >= data.size():
			return -1
		var b := data[pos]
		pos += 1
		return b

	func f64(what: String) -> float:
		if pos + 8 > data.size():
			fail(what)
			return 0.0
		var f := data.decode_double(pos)
		pos += 8
		return f

	func string_at(i: int) -> String:
		if i < 0 or i >= strings.size():
			fail("string index %d out of bounds (size %d)" % [i, strings.size()])
			return ""
		return strings[i]

	func read_table() -> void:
		var count := varint()
		for _i in count:
			if not err.is_empty():
				return
			var n := varint()
			if not err.is_empty():
				return
			if n < 0 or pos + n > data.size():
				fail("unexpected end of data in string table")
				return
			var s: Variant = SynxText.utf8_decode(data.slice(pos, pos + n))
			if s == null:
				fail("invalid UTF-8 in string table")
				return
			strings.append(s)
			pos += n

	func value(depth: int) -> SynxValue:
		if depth > SynxBinary.MAX_DECODE_DEPTH:
			fail("nesting exceeds %d — refusing to decode" % SynxBinary.MAX_DECODE_DEPTH)
			return SynxValue.make_null()
		var tag := byte_at()
		if tag < 0:
			fail("unexpected end of data")
			return SynxValue.make_null()
		match tag:
			SynxBinary.TAG_NULL:
				return SynxValue.make_null()
			SynxBinary.TAG_FALSE:
				return SynxValue.make_bool(false)
			SynxBinary.TAG_TRUE:
				return SynxValue.make_bool(true)
			SynxBinary.TAG_INT:
				return SynxValue.make_int(SynxBinary._zigzag_decode(varint()))
			SynxBinary.TAG_FLOAT:
				return SynxValue.make_float(f64("unexpected end of data in float"))
			SynxBinary.TAG_STRING:
				return SynxValue.make_string(string_at(varint()))
			SynxBinary.TAG_SECRET:
				return SynxValue.make_secret(string_at(varint()))
			SynxBinary.TAG_ARRAY:
				var count := varint()
				var arr: Array = []
				for _i in count:
					if not err.is_empty():
						break
					arr.append(value(depth + 1))
				return SynxValue.make_array(arr)
			SynxBinary.TAG_OBJECT:
				var count2 := varint()
				var map: Dictionary = {}
				for _i in count2:
					if not err.is_empty():
						break
					var k := string_at(varint())
					map[k] = value(depth + 1)
				return SynxValue.make_object(map)
		fail("unknown type tag: 0x%02x" % tag)
		return SynxValue.make_null()

	func meta() -> SynxMeta:
		var m := SynxMeta.new()
		var markers := PackedStringArray()
		for _i in varint():
			if not err.is_empty():
				return m
			markers.append(string_at(varint()))
		m.set_markers(markers)
		var args := PackedStringArray()
		for _i in varint():
			if not err.is_empty():
				return m
			args.append(string_at(varint()))
		m.args = args
		var has_th := byte_at()
		if has_th < 0:
			fail("unexpected end in meta")
			return m
		if has_th != 0:
			m.type_hint = string_at(varint())
		var has_c := byte_at()
		if has_c < 0:
			fail("unexpected end in meta")
			return m
		if has_c == 0:
			return m
		m.has_constraints = true
		var bits := byte_at()
		if bits < 0:
			fail("unexpected end of data in constraints")
			return m
		if bits & 0x01:
			m.c_min = f64("truncated min")
		if bits & 0x02:
			m.c_max = f64("truncated max")
		if bits & 0x04:
			m.c_type_name = string_at(varint())
		m.c_required = (bits & 0x08) != 0
		m.c_readonly = (bits & 0x10) != 0
		if bits & 0x20:
			m.c_pattern = string_at(varint())
		if bits & 0x40:
			var ev := PackedStringArray()
			for _i in varint():
				if not err.is_empty():
					break
				ev.append(string_at(varint()))
			m.c_enum = ev
		return m

	func metadata() -> Dictionary:
		var out: Dictionary = {}
		for _i in varint():
			if not err.is_empty():
				return out
			var path := string_at(varint())
			var mm: Dictionary = {}
			for _j in varint():
				if not err.is_empty():
					return out
				var k := string_at(varint())
				mm[k] = meta()
			out[path] = mm
		return out

	func includes() -> Array:
		var out: Array = []
		for _i in varint():
			if not err.is_empty():
				return out
			var p := string_at(varint())
			var a := string_at(varint())
			out.append(SynxParseResult.IncludeDirective.new(p, a))
		return out


# ─── Public API ──

# Compile a parse result. With `resolved`, metadata and includes are left out.
static func compile(result: SynxParseResult, resolved: bool) -> PackedByteArray:
	var st := _StringTable.new()
	st.collect_value(result.root)
	var metadata: Dictionary = {} if resolved else result.metadata
	var has_meta := not metadata.is_empty()
	if has_meta:
		st.collect_metadata(metadata)
		st.collect_includes(result.includes)
	st.order_by_use()

	var payload := PackedByteArray()
	st.encode(payload)
	_encode_value(payload, result.root, st)
	if has_meta:
		_encode_metadata(payload, metadata, st)
		_encode_varint(payload, result.includes.size())
		for inc in result.includes:
			_encode_varint(payload, st.idx(inc.path))
			_encode_varint(payload, st.idx(inc.alias))

	var zlib := payload.compress(FileAccess.COMPRESSION_DEFLATE)
	var raw := zlib.slice(2, zlib.size() - 4)

	var out := PackedByteArray()
	out.append_array(MAGIC.to_ascii_buffer())
	out.append(FORMAT_VERSION)
	var flags := 0
	if result.mode == SynxParseResult.Mode.ACTIVE: flags |= FLAG_ACTIVE
	if result.locked: flags |= FLAG_LOCKED
	if has_meta: flags |= FLAG_HAS_META
	if resolved: flags |= FLAG_RESOLVED
	if result.tool_directive: flags |= FLAG_TOOL
	if result.schema: flags |= FLAG_SCHEMA
	if result.llm: flags |= FLAG_LLM
	out.append(flags)
	var size_buf := PackedByteArray()
	size_buf.resize(4)
	size_buf.encode_u32(0, payload.size())
	out.append_array(size_buf)
	out.append_array(raw)
	return out


# Returns { "ok": bool, "result": SynxParseResult, "error": String }.
static func decompile(data: PackedByteArray) -> Dictionary:
	if data.size() < 11:
		return _err("file too small for .synxb header")
	if data.slice(0, 5).get_string_from_ascii() != MAGIC:
		return _err("invalid .synxb magic (expected SYNXB)")
	var version := data[5]
	if version != FORMAT_VERSION:
		return _err("unsupported .synxb version: %d (expected %d)" % [version, FORMAT_VERSION])
	var flags := data[6]
	var uncomp_size := data.decode_u32(7)
	if uncomp_size > MAX_DECODED_BYTES:
		return _err("declared size %d exceeds limit %d" % [uncomp_size, MAX_DECODED_BYTES])
	var inflated := SynxInflate.inflate(data, 11, MAX_DECODED_BYTES)
	if not bool(inflated["ok"]):
		return _err("decompression failed: %s" % inflated["error"])
	var payload: PackedByteArray = inflated["data"]
	if payload.size() != uncomp_size:
		return _err("size mismatch: expected %d, got %d" % [uncomp_size, payload.size()])

	var rd := _Reader.new(payload)
	rd.read_table()
	if not rd.err.is_empty():
		return _err(rd.err)
	var root := rd.value(0)
	if not rd.err.is_empty():
		return _err(rd.err)

	var pr := SynxParseResult.new()
	pr.root = root
	pr.mode = SynxParseResult.Mode.ACTIVE if (flags & FLAG_ACTIVE) != 0 else SynxParseResult.Mode.STATIC
	pr.locked = (flags & FLAG_LOCKED) != 0
	pr.tool_directive = (flags & FLAG_TOOL) != 0
	pr.schema = (flags & FLAG_SCHEMA) != 0
	pr.llm = (flags & FLAG_LLM) != 0
	if (flags & FLAG_HAS_META) != 0:
		var md := rd.metadata()
		var inc := rd.includes()
		if not rd.err.is_empty():
			return _err(rd.err)
		pr.metadata = md
		pr.includes = inc
	return {"ok": true, "result": pr, "error": ""}


static func is_synxb(data: PackedByteArray) -> bool:
	return data.size() >= 5 and data.slice(0, 5).get_string_from_ascii() == MAGIC


static func _err(msg: String) -> Dictionary:
	return {"ok": false, "result": null, "error": msg}
