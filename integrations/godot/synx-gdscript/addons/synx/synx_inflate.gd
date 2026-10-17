@tool
class_name SynxInflate
extends RefCounted

# Raw DEFLATE (RFC 1951) decoder for `.synxb` payloads.
#
# synx-core writes raw DEFLATE; Godot's own inflater only accepts zlib or
# gzip streams and verifies their checksum, so it cannot read those files.
# Huffman codes are decoded through a full lookup table per block.

const _LEN_BASE := [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258]
const _LEN_EXTRA := [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0]
const _DIST_BASE := [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577]
const _DIST_EXTRA := [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13]
const _CL_ORDER := [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15]


# Inflate src[start..]; at most `limit` output bytes.
# Returns { "ok": bool, "data": PackedByteArray, "error": String }.
static func inflate(src: PackedByteArray, start: int, limit: int) -> Dictionary:
	var n := src.size()
	var pos := start
	var bitbuf := 0
	var bitcnt := 0
	var out := PackedByteArray()
	out.resize(maxi(1024, mini((n - start) * 4, limit)))
	var olen := 0
	var final := false
	while not final:
		# header: BFINAL, BTYPE
		while bitcnt < 3:
			if pos >= n:
				return _fail("unexpected end of stream")
			bitbuf |= src[pos] << bitcnt
			pos += 1
			bitcnt += 8
		final = (bitbuf & 1) == 1
		var btype := (bitbuf >> 1) & 3
		bitbuf >>= 3
		bitcnt -= 3
		if btype == 0:
			# stored: skip to a byte boundary, LEN, NLEN, raw bytes. Whole
			# bytes still in the bit buffer are given back to the input.
			bitcnt -= bitcnt % 8
			pos -= bitcnt / 8
			bitbuf = 0
			bitcnt = 0
			if pos + 4 > n:
				return _fail("unexpected end of stream")
			var blen := src[pos] | (src[pos + 1] << 8)
			var nlen := src[pos + 2] | (src[pos + 3] << 8)
			pos += 4
			if blen != (~nlen & 0xFFFF):
				return _fail("invalid stored block length")
			if pos + blen > n:
				return _fail("unexpected end of stream")
			if olen + blen > limit:
				return _fail("output exceeds limit")
			while olen + blen > out.size():
				out.resize(mini(out.size() * 2 + blen, limit))
			for i in blen:
				out[olen + i] = src[pos + i]
			olen += blen
			pos += blen
			continue
		var lit_table: PackedInt64Array
		var lit_bits: int
		var dist_table: PackedInt64Array
		var dist_bits: int
		if btype == 1:
			var ll := PackedInt64Array()
			ll.resize(288)
			for i in 288:
				ll[i] = 8 if i < 144 else (9 if i < 256 else (7 if i < 280 else 8))
			var lt := _build(ll, 288)
			lit_table = lt[0]
			lit_bits = lt[1]
			var dl := PackedInt64Array()
			dl.resize(30)
			dl.fill(5)
			var dt := _build(dl, 30)
			dist_table = dt[0]
			dist_bits = dt[1]
		elif btype == 2:
			while bitcnt < 14:
				if pos >= n:
					return _fail("unexpected end of stream")
				bitbuf |= src[pos] << bitcnt
				pos += 1
				bitcnt += 8
			var hlit := (bitbuf & 31) + 257
			var hdist := ((bitbuf >> 5) & 31) + 1
			var hclen := ((bitbuf >> 10) & 15) + 4
			bitbuf >>= 14
			bitcnt -= 14
			var cl := PackedInt64Array()
			cl.resize(19)
			for i in hclen:
				while bitcnt < 3:
					if pos >= n:
						return _fail("unexpected end of stream")
					bitbuf |= src[pos] << bitcnt
					pos += 1
					bitcnt += 8
				cl[_CL_ORDER[i]] = bitbuf & 7
				bitbuf >>= 3
				bitcnt -= 3
			var clt := _build(cl, 19)
			var cl_table: PackedInt64Array = clt[0]
			var cl_bits: int = clt[1]
			if cl_bits == 0:
				return _fail("invalid code lengths")
			var lens := PackedInt64Array()
			lens.resize(hlit + hdist)
			var li := 0
			while li < hlit + hdist:
				while bitcnt < cl_bits and pos < n:
					bitbuf |= src[pos] << bitcnt
					pos += 1
					bitcnt += 8
				var e := cl_table[bitbuf & ((1 << cl_bits) - 1)]
				var elen := e & 15
				if elen == 0 or elen > bitcnt:
					return _fail("invalid code lengths")
				bitbuf >>= elen
				bitcnt -= elen
				var sym := e >> 4
				if sym < 16:
					lens[li] = sym
					li += 1
					continue
				var rep := 0
				var val := 0
				var eb := 2 if sym == 16 else (3 if sym == 17 else 7)
				while bitcnt < eb:
					if pos >= n:
						return _fail("unexpected end of stream")
					bitbuf |= src[pos] << bitcnt
					pos += 1
					bitcnt += 8
				var extra := bitbuf & ((1 << eb) - 1)
				bitbuf >>= eb
				bitcnt -= eb
				if sym == 16:
					if li == 0:
						return _fail("invalid code lengths")
					val = lens[li - 1]
					rep = 3 + extra
				elif sym == 17:
					rep = 3 + extra
				else:
					rep = 11 + extra
				if li + rep > hlit + hdist:
					return _fail("invalid code lengths")
				for _r in rep:
					lens[li] = val
					li += 1
			if lens[256] == 0:
				return _fail("missing end-of-block code")
			var lt2 := _build(lens.slice(0, hlit), hlit)
			lit_table = lt2[0]
			lit_bits = lt2[1]
			var dt2 := _build(lens.slice(hlit), hdist)
			dist_table = dt2[0]
			dist_bits = dt2[1]
		else:
			return _fail("invalid block type")

		var lit_mask := (1 << lit_bits) - 1
		var dist_mask := (1 << dist_bits) - 1
		while true:
			while bitcnt < lit_bits and pos < n:
				bitbuf |= src[pos] << bitcnt
				pos += 1
				bitcnt += 8
			var le := lit_table[bitbuf & lit_mask]
			var llen := le & 15
			if llen == 0 or llen > bitcnt:
				return _fail("invalid literal/length code")
			bitbuf >>= llen
			bitcnt -= llen
			var lsym := le >> 4
			if lsym < 256:
				if olen >= out.size():
					if olen >= limit:
						return _fail("output exceeds limit")
					out.resize(mini(out.size() * 2, limit))
				out[olen] = lsym
				olen += 1
				continue
			if lsym == 256:
				break
			var li2 := lsym - 257
			if li2 >= 29:
				return _fail("invalid length code")
			var leb: int = _LEN_EXTRA[li2]
			while bitcnt < leb:
				if pos >= n:
					return _fail("unexpected end of stream")
				bitbuf |= src[pos] << bitcnt
				pos += 1
				bitcnt += 8
			var length: int = _LEN_BASE[li2] + (bitbuf & ((1 << leb) - 1))
			bitbuf >>= leb
			bitcnt -= leb
			if dist_bits == 0:
				return _fail("invalid distance code")
			while bitcnt < dist_bits and pos < n:
				bitbuf |= src[pos] << bitcnt
				pos += 1
				bitcnt += 8
			var de := dist_table[bitbuf & dist_mask]
			var dlen := de & 15
			if dlen == 0 or dlen > bitcnt:
				return _fail("invalid distance code")
			bitbuf >>= dlen
			bitcnt -= dlen
			var dsym := de >> 4
			if dsym >= 30:
				return _fail("invalid distance code")
			var deb: int = _DIST_EXTRA[dsym]
			while bitcnt < deb:
				if pos >= n:
					return _fail("unexpected end of stream")
				bitbuf |= src[pos] << bitcnt
				pos += 1
				bitcnt += 8
			var dist: int = _DIST_BASE[dsym] + (bitbuf & ((1 << deb) - 1))
			bitbuf >>= deb
			bitcnt -= deb
			if dist > olen:
				return _fail("distance too far back")
			if olen + length > limit:
				return _fail("output exceeds limit")
			while olen + length > out.size():
				out.resize(mini(out.size() * 2 + length, limit))
			var from := olen - dist
			for i in length:
				out[olen + i] = out[from + i]
			olen += length
	out.resize(olen)
	return {"ok": true, "data": out, "error": ""}


static func _fail(msg: String) -> Dictionary:
	return {"ok": false, "data": PackedByteArray(), "error": msg}


# Canonical Huffman code → [table, bits]: table[next `bits` input bits] is
# (symbol << 4) | code length, 0 where no code matches.
static func _build(lengths: PackedInt64Array, count: int) -> Array:
	var max_bits := 0
	var bl_count := PackedInt64Array()
	bl_count.resize(16)
	for i in count:
		var l := lengths[i]
		if l > 0:
			bl_count[l] += 1
			if l > max_bits:
				max_bits = l
	var table := PackedInt64Array()
	if max_bits == 0:
		return [table, 0]
	table.resize(1 << max_bits)
	var next_code := PackedInt64Array()
	next_code.resize(16)
	var code := 0
	for b in range(1, 16):
		code = (code + bl_count[b - 1]) << 1
		next_code[b] = code
	var size := 1 << max_bits
	for sym in count:
		var l := lengths[sym]
		if l == 0:
			continue
		var c := next_code[l]
		next_code[l] += 1
		var rev := 0
		for _b in l:
			rev = (rev << 1) | (c & 1)
			c >>= 1
		var entry := (sym << 4) | l
		var step := 1 << l
		var idx := rev
		while idx < size:
			table[idx] = entry
			idx += step
	return [table, max_bits]
