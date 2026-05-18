@tool
class_name SynxBinary
extends RefCounted

# .synxb compact binary format — wire-compatible with synx-core::binary.
#
# Layout (header is uncompressed, payload is raw DEFLATE):
#   5 bytes  magic "SYNXB"
#   1 byte   format version (1)
#   1 byte   flags (active / locked / has_metadata / resolved / tool / schema / llm)
#   4 bytes  uncompressed payload length (LE)
#   N bytes  DEFLATE-compressed payload
#
# Payload:
#   • string table (varint count + (varint len + UTF-8 bytes) ×N)
#   • root value (recursive, strings encoded as table indices)
#   • [optional] metadata + includes when FLAG_HAS_META is set
#
# Godot's `PackedByteArray.compress(File.COMPRESSION_DEFLATE)` produces raw
# DEFLATE (RFC 1951), matching `miniz_oxide::deflate::compress_to_vec`.

const MAGIC := "SYNXB"
const FORMAT_VERSION := 1

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


# ─── Varint (LEB128 unsigned) ──

static func _encode_varint(out: PackedByteArray, val: int) -> void:
	var v := val
	while true:
		var byte := v & 0x7f
		v >>= 7
		if v == 0:
			out.append(byte)
			return
		out.append(byte | 0x80)


# Returns { "value": int, "next": int, "ok": bool, "error": String }.
static func _decode_varint(data: PackedByteArray, pos: int) -> Dictionary:
	var result := 0
	var shift := 0
	var p := pos
	while true:
		if p >= data.size():
			return {"ok": false, "error": "unexpected end of data in varint"}
		var byte := data[p]
		p += 1
		result |= (byte & 0x7f) << shift
		if (byte & 0x80) == 0:
			return {"ok": true, "value": result, "next": p}
		shift += 7
		if shift >= 64:
			return {"ok": false, "error": "varint overflow"}
	return {"ok": false, "error": "unreachable"}


static func _zigzag_encode(n: int) -> int:
	return ((n << 1) ^ (n >> 63)) & 0x7fffffffffffffff if n >= 0 else (((n << 1) ^ (n >> 63)))


static func _zigzag_decode(n: int) -> int:
	return (n >> 1) ^ -(n & 1)


# ─── String table ──

class _StringTable extends RefCounted:
	var strings: Array[String] = []
	var index: Dictionary = {}

	func intern(s: String) -> int:
		if index.has(s):
			return int(index[s])
		var i := strings.size()
		strings.append(s)
		index[s] = i
		return i

	func collect_value(v: SynxValue) -> void:
		match v.kind:
			SynxValue.Kind.STRING, SynxValue.Kind.SECRET:
				intern(v.data)
			SynxValue.Kind.ARRAY:
				for item in v.data:
					collect_value(item)
			SynxValue.Kind.OBJECT:
				for k in v.data.keys():
					intern(String(k))
					collect_value(v.data[k])

	func collect_metadata(metadata: Dictionary) -> void:
		for path in metadata.keys():
			intern(String(path))
			var mm: Dictionary = metadata[path]
			for key in mm.keys():
				intern(String(key))
				var meta: SynxMeta = mm[key]
				for m in meta.markers:
					intern(String(m))
				for a in meta.args:
					intern(String(a))
				if not meta.type_hint.is_empty():
					intern(meta.type_hint)
				if meta.has_constraints:
					if not meta.c_type_name.is_empty():
						intern(meta.c_type_name)
					if not meta.c_pattern.is_empty():
						intern(meta.c_pattern)
					if meta.c_has_enum:
						for ev in meta.c_enum:
							intern(String(ev))

	func collect_includes(includes: Array) -> void:
		for inc in includes:
			intern(inc.path)
			intern(inc.alias)

	func encode(out: PackedByteArray) -> void:
		_encode_varint(out, strings.size())
		for s in strings:
			var bytes := s.to_utf8_buffer()
			_encode_varint(out, bytes.size())
			out.append_array(bytes)


class _StringTableReader extends RefCounted:
	var strings: Array[String] = []

	static func decode(data: PackedByteArray, pos_ref: Array) -> Dictionary:
		var pos: int = int(pos_ref[0])
		var hdr := _decode_varint(data, pos)
		if not bool(hdr["ok"]):
			return hdr
		var count: int = int(hdr["value"])
		pos = int(hdr["next"])
		var sr := _StringTableReader.new()
		for i in count:
			var lh := _decode_varint(data, pos)
			if not bool(lh["ok"]):
				return lh
			var l: int = int(lh["value"])
			pos = int(lh["next"])
			if pos + l > data.size():
				return {"ok": false, "error": "unexpected end in string table"}
			var slice := data.slice(pos, pos + l)
			sr.strings.append(slice.get_string_from_utf8())
			pos += l
		pos_ref[0] = pos
		return {"ok": true, "table": sr}

	func get_at(idx: int) -> Dictionary:
		if idx < 0 or idx >= strings.size():
			return {"ok": false, "error": "string index %d out of bounds (size %d)" % [idx, strings.size()]}
		return {"ok": true, "value": strings[idx]}


# ─── Encoding ──

static func _encode_value(out: PackedByteArray, v: SynxValue, st: _StringTable) -> void:
	match v.kind:
		SynxValue.Kind.NULL:
			out.append(TAG_NULL)
		SynxValue.Kind.BOOL:
			out.append(TAG_TRUE if v.data else TAG_FALSE)
		SynxValue.Kind.INT:
			out.append(TAG_INT)
			_encode_varint(out, _zigzag_encode(int(v.data)))
		SynxValue.Kind.FLOAT:
			out.append(TAG_FLOAT)
			var buf := PackedByteArray()
			buf.resize(8)
			buf.encode_double(0, float(v.data))
			out.append_array(buf)
		SynxValue.Kind.STRING:
			out.append(TAG_STRING)
			_encode_varint(out, st.intern(String(v.data)))
		SynxValue.Kind.ARRAY:
			out.append(TAG_ARRAY)
			var arr: Array = v.data
			_encode_varint(out, arr.size())
			for item in arr:
				_encode_value(out, item, st)
		SynxValue.Kind.OBJECT:
			out.append(TAG_OBJECT)
			var map: Dictionary = v.data
			var keys: Array = map.keys()
			keys.sort()
			_encode_varint(out, keys.size())
			for k in keys:
				_encode_varint(out, st.intern(String(k)))
				_encode_value(out, map[k], st)
		SynxValue.Kind.SECRET:
			out.append(TAG_SECRET)
			_encode_varint(out, st.intern(String(v.data)))


# Returns { "ok": bool, "value": SynxValue, "next": int, "error": String }.
static func _decode_value(data: PackedByteArray, pos: int, st: _StringTableReader) -> Dictionary:
	if pos >= data.size():
		return {"ok": false, "error": "unexpected end of data"}
	var tag := data[pos]
	pos += 1
	match tag:
		TAG_NULL:
			return {"ok": true, "value": SynxValue.make_null(), "next": pos}
		TAG_FALSE:
			return {"ok": true, "value": SynxValue.make_bool(false), "next": pos}
		TAG_TRUE:
			return {"ok": true, "value": SynxValue.make_bool(true), "next": pos}
		TAG_INT:
			var h := _decode_varint(data, pos)
			if not bool(h["ok"]): return h
			return {"ok": true, "value": SynxValue.make_int(_zigzag_decode(int(h["value"]))), "next": int(h["next"])}
		TAG_FLOAT:
			if pos + 8 > data.size():
				return {"ok": false, "error": "unexpected end of data in float"}
			var f := data.slice(pos, pos + 8).decode_double(0)
			return {"ok": true, "value": SynxValue.make_float(f), "next": pos + 8}
		TAG_STRING:
			var hs := _decode_varint(data, pos)
			if not bool(hs["ok"]): return hs
			var sv := st.get_at(int(hs["value"]))
			if not bool(sv["ok"]): return sv
			return {"ok": true, "value": SynxValue.make_string(String(sv["value"])), "next": int(hs["next"])}
		TAG_ARRAY:
			var hc := _decode_varint(data, pos)
			if not bool(hc["ok"]): return hc
			var count: int = int(hc["value"])
			pos = int(hc["next"])
			var arr: Array = []
			for i in count:
				var ir := _decode_value(data, pos, st)
				if not bool(ir["ok"]): return ir
				arr.append(ir["value"])
				pos = int(ir["next"])
			return {"ok": true, "value": SynxValue.make_array(arr), "next": pos}
		TAG_OBJECT:
			var hc2 := _decode_varint(data, pos)
			if not bool(hc2["ok"]): return hc2
			var count2: int = int(hc2["value"])
			pos = int(hc2["next"])
			var map: Dictionary = {}
			for i in count2:
				var kr := _decode_varint(data, pos)
				if not bool(kr["ok"]): return kr
				var ks := st.get_at(int(kr["value"]))
				if not bool(ks["ok"]): return ks
				pos = int(kr["next"])
				var vr := _decode_value(data, pos, st)
				if not bool(vr["ok"]): return vr
				map[String(ks["value"])] = vr["value"]
				pos = int(vr["next"])
			return {"ok": true, "value": SynxValue.make_object(map), "next": pos}
		TAG_SECRET:
			var hs2 := _decode_varint(data, pos)
			if not bool(hs2["ok"]): return hs2
			var sv2 := st.get_at(int(hs2["value"]))
			if not bool(sv2["ok"]): return sv2
			return {"ok": true, "value": SynxValue.make_secret(String(sv2["value"])), "next": int(hs2["next"])}
	return {"ok": false, "error": "unknown type tag: 0x%02x" % tag}


# ─── Metadata / includes ──

static func _encode_constraints(out: PackedByteArray, meta: SynxMeta, st: _StringTable) -> void:
	var bits := 0
	var has_min := not is_nan(meta.c_min)
	var has_max := not is_nan(meta.c_max)
	if has_min: bits |= 0x01
	if has_max: bits |= 0x02
	if not meta.c_type_name.is_empty(): bits |= 0x04
	if meta.c_required: bits |= 0x08
	if meta.c_readonly: bits |= 0x10
	if not meta.c_pattern.is_empty(): bits |= 0x20
	if meta.c_has_enum: bits |= 0x40
	out.append(bits)
	if has_min:
		var b := PackedByteArray(); b.resize(8); b.encode_double(0, meta.c_min); out.append_array(b)
	if has_max:
		var b2 := PackedByteArray(); b2.resize(8); b2.encode_double(0, meta.c_max); out.append_array(b2)
	if not meta.c_type_name.is_empty():
		_encode_varint(out, st.intern(meta.c_type_name))
	if not meta.c_pattern.is_empty():
		_encode_varint(out, st.intern(meta.c_pattern))
	if meta.c_has_enum:
		_encode_varint(out, meta.c_enum.size())
		for v in meta.c_enum:
			_encode_varint(out, st.intern(String(v)))


static func _decode_constraints(data: PackedByteArray, pos: int, st: _StringTableReader, meta: SynxMeta) -> Dictionary:
	if pos >= data.size():
		return {"ok": false, "error": "unexpected end in constraints"}
	var bits := data[pos]
	pos += 1
	meta.has_constraints = true
	if (bits & 0x01) != 0:
		if pos + 8 > data.size():
			return {"ok": false, "error": "truncated min"}
		meta.c_min = data.slice(pos, pos + 8).decode_double(0)
		pos += 8
	if (bits & 0x02) != 0:
		if pos + 8 > data.size():
			return {"ok": false, "error": "truncated max"}
		meta.c_max = data.slice(pos, pos + 8).decode_double(0)
		pos += 8
	if (bits & 0x04) != 0:
		var h := _decode_varint(data, pos)
		if not bool(h["ok"]): return h
		var sv := st.get_at(int(h["value"]))
		if not bool(sv["ok"]): return sv
		meta.c_type_name = String(sv["value"])
		pos = int(h["next"])
	if (bits & 0x08) != 0:
		meta.c_required = true
	if (bits & 0x10) != 0:
		meta.c_readonly = true
	if (bits & 0x20) != 0:
		var h2 := _decode_varint(data, pos)
		if not bool(h2["ok"]): return h2
		var sv2 := st.get_at(int(h2["value"]))
		if not bool(sv2["ok"]): return sv2
		meta.c_pattern = String(sv2["value"])
		pos = int(h2["next"])
	if (bits & 0x40) != 0:
		var hc := _decode_varint(data, pos)
		if not bool(hc["ok"]): return hc
		var cnt: int = int(hc["value"])
		pos = int(hc["next"])
		meta.c_has_enum = true
		for _i in cnt:
			var hi := _decode_varint(data, pos)
			if not bool(hi["ok"]): return hi
			var ss := st.get_at(int(hi["value"]))
			if not bool(ss["ok"]): return ss
			meta.c_enum.append(String(ss["value"]))
			pos = int(hi["next"])
	return {"ok": true, "next": pos}


static func _encode_meta(out: PackedByteArray, meta: SynxMeta, st: _StringTable) -> void:
	_encode_varint(out, meta.markers.size())
	for m in meta.markers:
		_encode_varint(out, st.intern(String(m)))
	_encode_varint(out, meta.args.size())
	for a in meta.args:
		_encode_varint(out, st.intern(String(a)))
	if not meta.type_hint.is_empty():
		out.append(1)
		_encode_varint(out, st.intern(meta.type_hint))
	else:
		out.append(0)
	if meta.has_constraints:
		out.append(1)
		_encode_constraints(out, meta, st)
	else:
		out.append(0)


static func _decode_meta(data: PackedByteArray, pos: int, st: _StringTableReader) -> Dictionary:
	var meta := SynxMeta.new()
	var mc := _decode_varint(data, pos)
	if not bool(mc["ok"]): return mc
	var cnt: int = int(mc["value"])
	pos = int(mc["next"])
	for _i in cnt:
		var h := _decode_varint(data, pos)
		if not bool(h["ok"]): return h
		var sv := st.get_at(int(h["value"]))
		if not bool(sv["ok"]): return sv
		meta.markers.append(String(sv["value"]))
		pos = int(h["next"])
	var ac := _decode_varint(data, pos)
	if not bool(ac["ok"]): return ac
	cnt = int(ac["value"])
	pos = int(ac["next"])
	for _i in cnt:
		var h2 := _decode_varint(data, pos)
		if not bool(h2["ok"]): return h2
		var sv2 := st.get_at(int(h2["value"]))
		if not bool(sv2["ok"]): return sv2
		meta.args.append(String(sv2["value"]))
		pos = int(h2["next"])

	if pos >= data.size():
		return {"ok": false, "error": "unexpected end in meta"}
	var has_th := data[pos]
	pos += 1
	if has_th != 0:
		var ht := _decode_varint(data, pos)
		if not bool(ht["ok"]): return ht
		var st_h := st.get_at(int(ht["value"]))
		if not bool(st_h["ok"]): return st_h
		meta.type_hint = String(st_h["value"])
		pos = int(ht["next"])

	if pos >= data.size():
		return {"ok": false, "error": "unexpected end in meta"}
	var has_c := data[pos]
	pos += 1
	if has_c != 0:
		var dc := _decode_constraints(data, pos, st, meta)
		if not bool(dc["ok"]): return dc
		pos = int(dc["next"])

	return {"ok": true, "meta": meta, "next": pos}


static func _encode_metadata(out: PackedByteArray, metadata: Dictionary, st: _StringTable) -> void:
	var paths: Array = metadata.keys()
	paths.sort()
	_encode_varint(out, paths.size())
	for path in paths:
		_encode_varint(out, st.intern(String(path)))
		var mm: Dictionary = metadata[path]
		var keys: Array = mm.keys()
		keys.sort()
		_encode_varint(out, keys.size())
		for k in keys:
			_encode_varint(out, st.intern(String(k)))
			_encode_meta(out, mm[k], st)


static func _decode_metadata(data: PackedByteArray, pos: int, st: _StringTableReader) -> Dictionary:
	var oc := _decode_varint(data, pos)
	if not bool(oc["ok"]): return oc
	var outer: int = int(oc["value"])
	pos = int(oc["next"])
	var metadata: Dictionary = {}
	for _i in outer:
		var pkh := _decode_varint(data, pos)
		if not bool(pkh["ok"]): return pkh
		var ps := st.get_at(int(pkh["value"]))
		if not bool(ps["ok"]): return ps
		var path := String(ps["value"])
		pos = int(pkh["next"])

		var ich := _decode_varint(data, pos)
		if not bool(ich["ok"]): return ich
		var inner: int = int(ich["value"])
		pos = int(ich["next"])
		var meta_map: Dictionary = {}
		for _j in inner:
			var kh := _decode_varint(data, pos)
			if not bool(kh["ok"]): return kh
			var ks := st.get_at(int(kh["value"]))
			if not bool(ks["ok"]): return ks
			pos = int(kh["next"])
			var mh := _decode_meta(data, pos, st)
			if not bool(mh["ok"]): return mh
			meta_map[String(ks["value"])] = mh["meta"]
			pos = int(mh["next"])
		metadata[path] = meta_map
	return {"ok": true, "metadata": metadata, "next": pos}


static func _encode_includes(out: PackedByteArray, includes: Array, st: _StringTable) -> void:
	_encode_varint(out, includes.size())
	for inc in includes:
		_encode_varint(out, st.intern(inc.path))
		_encode_varint(out, st.intern(inc.alias))


static func _decode_includes(data: PackedByteArray, pos: int, st: _StringTableReader) -> Dictionary:
	var ch := _decode_varint(data, pos)
	if not bool(ch["ok"]): return ch
	var cnt: int = int(ch["value"])
	pos = int(ch["next"])
	var includes: Array = []
	for _i in cnt:
		var ph := _decode_varint(data, pos)
		if not bool(ph["ok"]): return ph
		var ps := st.get_at(int(ph["value"]))
		if not bool(ps["ok"]): return ps
		pos = int(ph["next"])
		var ah := _decode_varint(data, pos)
		if not bool(ah["ok"]): return ah
		var asv := st.get_at(int(ah["value"]))
		if not bool(asv["ok"]): return asv
		pos = int(ah["next"])
		includes.append(SynxParseResult.IncludeDirective.new(String(ps["value"]), String(asv["value"])))
	return {"ok": true, "includes": includes, "next": pos}


# ─── Public API ──

static func compile(result: SynxParseResult, resolved: bool) -> PackedByteArray:
	var st := _StringTable.new()
	st.collect_value(result.root)
	var has_meta := not resolved and result.metadata.size() > 0
	if has_meta:
		st.collect_metadata(result.metadata)
		st.collect_includes(result.includes)

	var payload := PackedByteArray()
	st.encode(payload)
	_encode_value(payload, result.root, st)
	if has_meta:
		_encode_metadata(payload, result.metadata, st)
		_encode_includes(payload, result.includes, st)

	# DEFLATE — raw, no zlib/gzip wrapper.
	var compressed := payload.compress(FileAccess.COMPRESSION_DEFLATE)

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

	# Uncompressed payload length, LE u32.
	var size_buf := PackedByteArray(); size_buf.resize(4); size_buf.encode_u32(0, payload.size())
	out.append_array(size_buf)
	out.append_array(compressed)
	return out


# Returns { "ok": bool, "result": SynxParseResult, "error": String }.
static func decompile(data: PackedByteArray) -> Dictionary:
	if data.size() < 11:
		return {"ok": false, "error": "file too small for .synxb header"}
	if data.slice(0, 5).get_string_from_ascii() != MAGIC:
		return {"ok": false, "error": "invalid .synxb magic (expected SYNXB)"}
	var version := data[5]
	if version != FORMAT_VERSION:
		return {"ok": false, "error": "unsupported .synxb version: %d (expected %d)" % [version, FORMAT_VERSION]}
	var flags := data[6]
	var uncomp_size := data.slice(7, 11).decode_u32(0)

	var payload := data.slice(11, data.size()).decompress(uncomp_size, FileAccess.COMPRESSION_DEFLATE)
	if payload.size() != uncomp_size:
		return {"ok": false, "error": "size mismatch: expected %d, got %d" % [uncomp_size, payload.size()]}

	var pos_ref := [0]
	var st_h := _StringTableReader.decode(payload, pos_ref)
	if not bool(st_h["ok"]):
		return st_h
	var st: _StringTableReader = st_h["table"]
	var pos: int = int(pos_ref[0])

	var rv := _decode_value(payload, pos, st)
	if not bool(rv["ok"]): return rv
	var root: SynxValue = rv["value"]
	pos = int(rv["next"])

	var result := SynxParseResult.new()
	result.root = root
	result.mode = SynxParseResult.Mode.ACTIVE if (flags & FLAG_ACTIVE) != 0 else SynxParseResult.Mode.STATIC
	result.locked = (flags & FLAG_LOCKED) != 0
	result.tool_directive = (flags & FLAG_TOOL) != 0
	result.schema = (flags & FLAG_SCHEMA) != 0
	result.llm = (flags & FLAG_LLM) != 0

	if (flags & FLAG_HAS_META) != 0:
		var mh := _decode_metadata(payload, pos, st)
		if not bool(mh["ok"]): return mh
		result.metadata = mh["metadata"]
		pos = int(mh["next"])
		var ih := _decode_includes(payload, pos, st)
		if not bool(ih["ok"]): return ih
		result.includes = ih["includes"]

	return {"ok": true, "result": result}


static func is_synxb(data: PackedByteArray) -> bool:
	if data.size() < 5:
		return false
	return data.slice(0, 5).get_string_from_ascii() == MAGIC
