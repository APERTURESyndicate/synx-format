package synx

import (
	"bytes"
	"compress/flate"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"math"
	"sort"
)

// .synxb compact binary format. Wire-compatible with crates/synx-core 3.6.x.
//
// Layout:
//   5 bytes magic "SYNXB"
//   1 byte  version
//   1 byte  flags
//   4 bytes little-endian uint32 uncompressed-size
//   raw DEFLATE payload (compress/flate.NewWriter level 9 = matches Rust miniz_oxide).

var (
	binaryMagic   = []byte{'S', 'Y', 'N', 'X', 'B'}
	binaryVersion = byte(1)
)

const (
	flagActive   = 0x01
	flagLocked   = 0x02
	flagHasMeta  = 0x04
	flagResolved = 0x08
	flagTool     = 0x10
	flagSchema   = 0x20
	flagLLM      = 0x40
)

const (
	tagNull   = 0x00
	tagFalse  = 0x01
	tagTrue   = 0x02
	tagInt    = 0x03
	tagFloat  = 0x04
	tagString = 0x05
	tagArray  = 0x06
	tagObject = 0x07
	tagSecret = 0x08
)

// IsSynxb reports whether `data` starts with the `.synxb` magic prefix.
func IsSynxb(data []byte) bool {
	return len(data) >= 5 && bytes.Equal(data[:5], binaryMagic)
}

// Compile produces `.synxb` bytes for a ParseResult. When `resolved` is true,
// metadata and includes are stripped from the output.
func Compile(result ParseResult, resolved bool) ([]byte, error) {
	st := newStringTable()
	st.collectValue(result.Root)
	hasMeta := !resolved && len(result.Metadata) > 0
	if hasMeta {
		st.collectMetadata(result.Metadata)
		st.collectIncludes(result.Includes)
	}

	var payload bytes.Buffer
	st.encode(&payload)
	encodeValue(&payload, result.Root, st)
	if hasMeta {
		encodeMetadata(&payload, result.Metadata, st)
		encodeIncludes(&payload, result.Includes, st)
	}

	compressed, err := deflateRaw(payload.Bytes())
	if err != nil {
		return nil, err
	}

	out := make([]byte, 0, 11+len(compressed))
	out = append(out, binaryMagic...)
	out = append(out, binaryVersion)
	flags := byte(0)
	if result.Mode == ModeActive {
		flags |= flagActive
	}
	if result.Locked {
		flags |= flagLocked
	}
	if hasMeta {
		flags |= flagHasMeta
	}
	if resolved {
		flags |= flagResolved
	}
	if result.Tool {
		flags |= flagTool
	}
	if result.Schema {
		flags |= flagSchema
	}
	if result.Llm {
		flags |= flagLLM
	}
	out = append(out, flags)
	var sz [4]byte
	binary.LittleEndian.PutUint32(sz[:], uint32(payload.Len()))
	out = append(out, sz[:]...)
	out = append(out, compressed...)
	return out, nil
}

// Decompile parses `.synxb` bytes back into a ParseResult.
func Decompile(data []byte) (ParseResult, error) {
	var pr ParseResult
	if len(data) < 11 {
		return pr, errors.New("file too small for .synxb header")
	}
	if !IsSynxb(data) {
		return pr, errors.New("invalid .synxb magic (expected SYNXB)")
	}
	if data[5] != binaryVersion {
		return pr, fmt.Errorf("unsupported .synxb version: %d", data[5])
	}
	flags := data[6]
	uncomp := binary.LittleEndian.Uint32(data[7:11])
	payload, err := inflateRaw(data[11:], int(uncomp))
	if err != nil {
		return pr, fmt.Errorf("decompression failed: %w", err)
	}
	if uint32(len(payload)) != uncomp {
		return pr, errors.New("size mismatch in decompressed payload")
	}

	cur := &cursor{data: payload}
	reader, err := decodeStringTable(cur)
	if err != nil {
		return pr, err
	}
	root, err := decodeValue(cur, reader)
	if err != nil {
		return pr, err
	}
	pr.Root = root
	if flags&flagActive != 0 {
		pr.Mode = ModeActive
	}
	pr.Locked = flags&flagLocked != 0
	pr.Tool = flags&flagTool != 0
	pr.Schema = flags&flagSchema != 0
	pr.Llm = flags&flagLLM != 0
	pr.Metadata = MetadataTree{}
	if flags&flagHasMeta != 0 {
		meta, err := decodeMetadata(cur, reader)
		if err != nil {
			return pr, err
		}
		pr.Metadata = meta
		incs, err := decodeIncludes(cur, reader)
		if err != nil {
			return pr, err
		}
		pr.Includes = incs
	}
	return pr, nil
}

// ─── DEFLATE wrappers ───────────────────────────────────────────────────────

func deflateRaw(data []byte) ([]byte, error) {
	var buf bytes.Buffer
	w, err := flate.NewWriter(&buf, flate.BestCompression)
	if err != nil {
		return nil, err
	}
	if _, err := w.Write(data); err != nil {
		_ = w.Close()
		return nil, err
	}
	if err := w.Close(); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

func inflateRaw(data []byte, expected int) ([]byte, error) {
	r := flate.NewReader(bytes.NewReader(data))
	defer r.Close()
	if expected <= 0 {
		expected = 1024
	}
	buf := bytes.NewBuffer(make([]byte, 0, expected))
	if _, err := io.Copy(buf, r); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// ─── string table ───────────────────────────────────────────────────────────

type stringTable struct {
	strings []string
	index   map[string]uint32
}

func newStringTable() *stringTable {
	return &stringTable{index: map[string]uint32{}}
}

func (t *stringTable) intern(s string) uint32 {
	if idx, ok := t.index[s]; ok {
		return idx
	}
	idx := uint32(len(t.strings))
	t.strings = append(t.strings, s)
	t.index[s] = idx
	return idx
}

func (t *stringTable) indexOf(s string) uint32 {
	return t.index[s]
}

func (t *stringTable) collectValue(v Value) {
	switch x := v.(type) {
	case StringValue:
		t.intern(x.V)
	case SecretValue:
		t.intern(x.V)
	case ArrayValue:
		for _, item := range x.V {
			t.collectValue(item)
		}
	case ObjectValue:
		for _, p := range x.V.Pairs() {
			t.intern(p.Key)
			t.collectValue(p.Value)
		}
	}
}

func (t *stringTable) collectMetadata(tree MetadataTree) {
	for path, m := range tree {
		t.intern(path)
		for key, meta := range m {
			t.intern(key)
			for _, mk := range meta.Markers {
				t.intern(mk)
			}
			for _, a := range meta.Args {
				t.intern(a)
			}
			if meta.TypeHint != "" {
				t.intern(meta.TypeHint)
			}
			if meta.Constraints != nil {
				c := meta.Constraints
				if c.TypeName != "" {
					t.intern(c.TypeName)
				}
				if c.Pattern != "" {
					t.intern(c.Pattern)
				}
				for _, e := range c.EnumValues {
					t.intern(e)
				}
			}
		}
	}
}

func (t *stringTable) collectIncludes(incs []IncludeDirective) {
	for _, inc := range incs {
		t.intern(inc.Path)
		t.intern(inc.Alias)
	}
}

func (t *stringTable) encode(out *bytes.Buffer) {
	encodeVarint(out, uint64(len(t.strings)))
	for _, s := range t.strings {
		encodeVarint(out, uint64(len(s)))
		out.WriteString(s)
	}
}

type stringTableReader struct {
	strings []string
}

func decodeStringTable(cur *cursor) (*stringTableReader, error) {
	count, err := decodeVarint(cur)
	if err != nil {
		return nil, err
	}
	r := &stringTableReader{strings: make([]string, 0, count)}
	for i := uint64(0); i < count; i++ {
		ln, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		if cur.pos+int(ln) > len(cur.data) {
			return nil, errors.New("unexpected end of data in string table")
		}
		r.strings = append(r.strings, string(cur.data[cur.pos:cur.pos+int(ln)]))
		cur.pos += int(ln)
	}
	return r, nil
}

func (r *stringTableReader) get(idx uint32) (string, error) {
	if int(idx) >= len(r.strings) {
		return "", errors.New("string index out of bounds")
	}
	return r.strings[idx], nil
}

// ─── varint / zigzag ────────────────────────────────────────────────────────

type cursor struct {
	data []byte
	pos  int
}

func encodeVarint(out *bytes.Buffer, value uint64) {
	for {
		b := byte(value & 0x7F)
		value >>= 7
		if value == 0 {
			out.WriteByte(b)
			return
		}
		out.WriteByte(b | 0x80)
	}
}

func decodeVarint(cur *cursor) (uint64, error) {
	var result uint64
	var shift uint
	for {
		if cur.pos >= len(cur.data) {
			return 0, errors.New("unexpected end of data in varint")
		}
		b := cur.data[cur.pos]
		cur.pos++
		result |= uint64(b&0x7F) << shift
		if b&0x80 == 0 {
			return result, nil
		}
		shift += 7
		if shift >= 64 {
			return 0, errors.New("varint overflow")
		}
	}
}

func zigzagEncode(n int64) uint64 { return uint64((n << 1) ^ (n >> 63)) }
func zigzagDecode(n uint64) int64 { return int64(n>>1) ^ -int64(n&1) }

func encodeF64LE(out *bytes.Buffer, f float64) {
	var buf [8]byte
	binary.LittleEndian.PutUint64(buf[:], math.Float64bits(f))
	out.Write(buf[:])
}

func decodeF64LE(cur *cursor) (float64, error) {
	if cur.pos+8 > len(cur.data) {
		return 0, errors.New("unexpected end of data in float")
	}
	v := math.Float64frombits(binary.LittleEndian.Uint64(cur.data[cur.pos : cur.pos+8]))
	cur.pos += 8
	return v, nil
}

// ─── value encode / decode ──────────────────────────────────────────────────

func encodeValue(out *bytes.Buffer, v Value, t *stringTable) {
	switch x := v.(type) {
	case NullValue:
		out.WriteByte(tagNull)
	case BoolValue:
		if x.V {
			out.WriteByte(tagTrue)
		} else {
			out.WriteByte(tagFalse)
		}
	case IntValue:
		out.WriteByte(tagInt)
		encodeVarint(out, zigzagEncode(x.V))
	case FloatValue:
		out.WriteByte(tagFloat)
		encodeF64LE(out, x.V)
	case StringValue:
		out.WriteByte(tagString)
		encodeVarint(out, uint64(t.indexOf(x.V)))
	case SecretValue:
		out.WriteByte(tagSecret)
		encodeVarint(out, uint64(t.indexOf(x.V)))
	case ArrayValue:
		out.WriteByte(tagArray)
		encodeVarint(out, uint64(len(x.V)))
		for _, item := range x.V {
			encodeValue(out, item, t)
		}
	case ObjectValue:
		out.WriteByte(tagObject)
		keys := x.V.Keys()
		sort.Strings(keys)
		encodeVarint(out, uint64(len(keys)))
		for _, k := range keys {
			encodeVarint(out, uint64(t.indexOf(k)))
			val, _ := x.V.Get(k)
			if val == nil {
				val = Null
			}
			encodeValue(out, val, t)
		}
	}
}

func decodeValue(cur *cursor, t *stringTableReader) (Value, error) {
	if cur.pos >= len(cur.data) {
		return nil, errors.New("unexpected end of data")
	}
	tag := cur.data[cur.pos]
	cur.pos++
	switch tag {
	case tagNull:
		return Null, nil
	case tagFalse:
		return Bool(false), nil
	case tagTrue:
		return Bool(true), nil
	case tagInt:
		raw, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		return Int(zigzagDecode(raw)), nil
	case tagFloat:
		f, err := decodeF64LE(cur)
		if err != nil {
			return nil, err
		}
		return Float(f), nil
	case tagString:
		idx, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		s, err := t.get(uint32(idx))
		if err != nil {
			return nil, err
		}
		return String(s), nil
	case tagSecret:
		idx, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		s, err := t.get(uint32(idx))
		if err != nil {
			return nil, err
		}
		return Secret(s), nil
	case tagArray:
		count, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		arr := make([]Value, 0, count)
		for i := uint64(0); i < count; i++ {
			v, err := decodeValue(cur, t)
			if err != nil {
				return nil, err
			}
			arr = append(arr, v)
		}
		return Array(arr), nil
	case tagObject:
		count, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		obj := NewObjectMap()
		for i := uint64(0); i < count; i++ {
			ki, err := decodeVarint(cur)
			if err != nil {
				return nil, err
			}
			k, err := t.get(uint32(ki))
			if err != nil {
				return nil, err
			}
			v, err := decodeValue(cur, t)
			if err != nil {
				return nil, err
			}
			obj.Set(k, v)
		}
		return Object_(obj), nil
	}
	return nil, fmt.Errorf("unknown type tag 0x%02x", tag)
}

// ─── metadata encode / decode ───────────────────────────────────────────────

func encodeConstraints(out *bytes.Buffer, c *Constraints, t *stringTable) {
	var bits byte
	if c.Min != nil {
		bits |= 0x01
	}
	if c.Max != nil {
		bits |= 0x02
	}
	if c.TypeName != "" {
		bits |= 0x04
	}
	if c.Required {
		bits |= 0x08
	}
	if c.Readonly {
		bits |= 0x10
	}
	if c.Pattern != "" {
		bits |= 0x20
	}
	if c.EnumValues != nil {
		bits |= 0x40
	}
	out.WriteByte(bits)
	if c.Min != nil {
		encodeF64LE(out, *c.Min)
	}
	if c.Max != nil {
		encodeF64LE(out, *c.Max)
	}
	if c.TypeName != "" {
		encodeVarint(out, uint64(t.indexOf(c.TypeName)))
	}
	if c.Pattern != "" {
		encodeVarint(out, uint64(t.indexOf(c.Pattern)))
	}
	if c.EnumValues != nil {
		encodeVarint(out, uint64(len(c.EnumValues)))
		for _, v := range c.EnumValues {
			encodeVarint(out, uint64(t.indexOf(v)))
		}
	}
}

func decodeConstraints(cur *cursor, t *stringTableReader) (*Constraints, error) {
	if cur.pos >= len(cur.data) {
		return nil, errors.New("unexpected end in constraints")
	}
	bits := cur.data[cur.pos]
	cur.pos++
	c := &Constraints{}
	if bits&0x01 != 0 {
		v, err := decodeF64LE(cur)
		if err != nil {
			return nil, err
		}
		c.Min = &v
	}
	if bits&0x02 != 0 {
		v, err := decodeF64LE(cur)
		if err != nil {
			return nil, err
		}
		c.Max = &v
	}
	if bits&0x04 != 0 {
		idx, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		s, err := t.get(uint32(idx))
		if err != nil {
			return nil, err
		}
		c.TypeName = s
	}
	if bits&0x08 != 0 {
		c.Required = true
	}
	if bits&0x10 != 0 {
		c.Readonly = true
	}
	if bits&0x20 != 0 {
		idx, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		s, err := t.get(uint32(idx))
		if err != nil {
			return nil, err
		}
		c.Pattern = s
	}
	if bits&0x40 != 0 {
		count, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		vals := make([]string, 0, count)
		for i := uint64(0); i < count; i++ {
			idx, err := decodeVarint(cur)
			if err != nil {
				return nil, err
			}
			s, err := t.get(uint32(idx))
			if err != nil {
				return nil, err
			}
			vals = append(vals, s)
		}
		c.EnumValues = vals
	}
	return c, nil
}

func encodeMetadata(out *bytes.Buffer, tree MetadataTree, t *stringTable) {
	outerKeys := make([]string, 0, len(tree))
	for k := range tree {
		outerKeys = append(outerKeys, k)
	}
	sort.Strings(outerKeys)
	encodeVarint(out, uint64(len(outerKeys)))
	for _, path := range outerKeys {
		encodeVarint(out, uint64(t.indexOf(path)))
		m := tree[path]
		innerKeys := make([]string, 0, len(m))
		for k := range m {
			innerKeys = append(innerKeys, k)
		}
		sort.Strings(innerKeys)
		encodeVarint(out, uint64(len(innerKeys)))
		for _, fk := range innerKeys {
			meta := m[fk]
			encodeVarint(out, uint64(t.indexOf(fk)))
			encodeVarint(out, uint64(len(meta.Markers)))
			for _, mk := range meta.Markers {
				encodeVarint(out, uint64(t.indexOf(mk)))
			}
			encodeVarint(out, uint64(len(meta.Args)))
			for _, a := range meta.Args {
				encodeVarint(out, uint64(t.indexOf(a)))
			}
			if meta.TypeHint != "" {
				out.WriteByte(1)
				encodeVarint(out, uint64(t.indexOf(meta.TypeHint)))
			} else {
				out.WriteByte(0)
			}
			if meta.Constraints != nil {
				out.WriteByte(1)
				encodeConstraints(out, meta.Constraints, t)
			} else {
				out.WriteByte(0)
			}
		}
	}
}

func decodeMetadata(cur *cursor, t *stringTableReader) (MetadataTree, error) {
	outer, err := decodeVarint(cur)
	if err != nil {
		return nil, err
	}
	tree := make(MetadataTree, outer)
	for i := uint64(0); i < outer; i++ {
		pi, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		path, err := t.get(uint32(pi))
		if err != nil {
			return nil, err
		}
		inner, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		m := make(MetaMap, inner)
		for j := uint64(0); j < inner; j++ {
			fki, err := decodeVarint(cur)
			if err != nil {
				return nil, err
			}
			fk, err := t.get(uint32(fki))
			if err != nil {
				return nil, err
			}
			var meta Meta
			mc, err := decodeVarint(cur)
			if err != nil {
				return nil, err
			}
			for k := uint64(0); k < mc; k++ {
				idx, err := decodeVarint(cur)
				if err != nil {
					return nil, err
				}
				s, err := t.get(uint32(idx))
				if err != nil {
					return nil, err
				}
				meta.Markers = append(meta.Markers, s)
			}
			ac, err := decodeVarint(cur)
			if err != nil {
				return nil, err
			}
			for k := uint64(0); k < ac; k++ {
				idx, err := decodeVarint(cur)
				if err != nil {
					return nil, err
				}
				s, err := t.get(uint32(idx))
				if err != nil {
					return nil, err
				}
				meta.Args = append(meta.Args, s)
			}
			if cur.pos >= len(cur.data) {
				return nil, errors.New("unexpected end in meta (type_hint flag)")
			}
			hasTh := cur.data[cur.pos]
			cur.pos++
			if hasTh != 0 {
				idx, err := decodeVarint(cur)
				if err != nil {
					return nil, err
				}
				s, err := t.get(uint32(idx))
				if err != nil {
					return nil, err
				}
				meta.TypeHint = s
			}
			if cur.pos >= len(cur.data) {
				return nil, errors.New("unexpected end in meta (constraints flag)")
			}
			hasC := cur.data[cur.pos]
			cur.pos++
			if hasC != 0 {
				c, err := decodeConstraints(cur, t)
				if err != nil {
					return nil, err
				}
				meta.Constraints = c
			}
			m[fk] = meta
		}
		tree[path] = m
	}
	return tree, nil
}

func encodeIncludes(out *bytes.Buffer, incs []IncludeDirective, t *stringTable) {
	encodeVarint(out, uint64(len(incs)))
	for _, inc := range incs {
		encodeVarint(out, uint64(t.indexOf(inc.Path)))
		encodeVarint(out, uint64(t.indexOf(inc.Alias)))
	}
}

func decodeIncludes(cur *cursor, t *stringTableReader) ([]IncludeDirective, error) {
	count, err := decodeVarint(cur)
	if err != nil {
		return nil, err
	}
	out := make([]IncludeDirective, 0, count)
	for i := uint64(0); i < count; i++ {
		pi, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		ai, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		p, err := t.get(uint32(pi))
		if err != nil {
			return nil, err
		}
		a, err := t.get(uint32(ai))
		if err != nil {
			return nil, err
		}
		out = append(out, IncludeDirective{Path: p, Alias: a})
	}
	return out, nil
}
