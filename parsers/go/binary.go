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
	"unicode/utf8"
)

// .synxb compact binary format, wire-compatible with crates/synx-core.
//
// Layout:
//   5 bytes magic "SYNXB"
//   1 byte  version
//   1 byte  flags
//   4 bytes little-endian uint32 uncompressed-size
//   raw DEFLATE payload: string table, value tree, [metadata, includes]
//
// The string table is ordered by use count (ties keep first-seen order), so
// frequent strings get one-byte indexes and one document always compiles to
// the same payload.

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

const (
	// maxDecodedBytes bounds the inflated payload of an untrusted file.
	maxDecodedBytes = 256 * 1024 * 1024
	// maxDecodeDepth bounds ARRAY/OBJECT nesting while decoding.
	maxDecodeDepth = 256
	// decodedValueCost is what one decoded value costs against
	// maxDecodedBytes, text aside.
	decodedValueCost = 64
	// maxPreallocItems caps the capacity reserved for a count read from the file.
	maxPreallocItems = 1024
)

// decodeBudget is what decoding has built so far, in bytes.
//
// safety: a value is one byte on the wire and tens of bytes in memory, a
// string is stored once and referenced any number of times, and deflate
// shrinks a run of either a thousandfold: unbudgeted, 8 KB of file decoded to
// half a gigabyte and 171 bytes to 1.4 GiB. The tree shares the payload's
// limit — every value at a fixed cost, plus the bytes of each key and string
// it refers to.
type decodeBudget struct{ spent int64 }

func (b *decodeBudget) charge(n int) error {
	b.spent += int64(n)
	if b.spent > maxDecodedBytes {
		return fmt.Errorf("decoded document exceeds %d bytes — refusing to decode", maxDecodedBytes)
	}
	return nil
}

func (b *decodeBudget) value() error { return b.charge(decodedValueCost) }

// IsSynxb reports whether `data` starts with the `.synxb` magic prefix.
func IsSynxb(data []byte) bool {
	return len(data) >= 5 && bytes.Equal(data[:5], binaryMagic)
}

// Compile produces `.synxb` bytes for a ParseResult. When `resolved` is true,
// metadata and includes are stripped from the output.
func Compile(result ParseResult, resolved bool) ([]byte, error) {
	st := newStringTable()
	st.collectValue(result.Root)
	hasMeta := !resolved && result.Metadata.Len() > 0
	if hasMeta {
		st.collectMetadata(result.Metadata)
		st.collectIncludes(result.Includes)
	}
	st.orderByUse()

	var payload bytes.Buffer
	payload.Grow(1024)
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
		return pr, fmt.Errorf("unsupported .synxb version: %d (expected %d)", data[5], binaryVersion)
	}
	flags := data[6]
	uncomp := int(binary.LittleEndian.Uint32(data[7:11]))
	if uncomp > maxDecodedBytes {
		return pr, fmt.Errorf("declared size %d exceeds limit %d", uncomp, maxDecodedBytes)
	}
	payload, err := inflateRaw(data[11:], uncomp)
	if err != nil {
		return pr, fmt.Errorf("decompression failed: %w", err)
	}
	if len(payload) != uncomp {
		return pr, fmt.Errorf("size mismatch: expected %d, got %d", uncomp, len(payload))
	}

	cur := &cursor{data: payload}
	reader, err := decodeStringTable(cur)
	if err != nil {
		return pr, err
	}
	root, err := decodeValue(cur, reader, 0)
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
	pr.Metadata = NewMetadataTree()
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

// inflateRaw inflates at most `expected` + 1 bytes: one more than declared, to
// notice a stream that inflates past it.
//
// safety: the buffer was reserved from the declared size before a byte was
// inflated — 13 bytes of file declaring 256 MiB cost 256 MiB each — and the
// stream was read up to the 256 MiB cap whatever it declared. The buffer now
// grows with what the stream actually produces.
func inflateRaw(data []byte, expected int) ([]byte, error) {
	r := flate.NewReader(bytes.NewReader(data))
	defer r.Close()
	if expected < 0 {
		expected = 0
	}
	reserve := expected
	if reserve > 64*1024 {
		reserve = 64 * 1024
	}
	buf := bytes.NewBuffer(make([]byte, 0, reserve))
	if _, err := io.Copy(buf, io.LimitReader(r, int64(expected)+1)); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// ─── string table ───────────────────────────────────────────────────────────

type stringTable struct {
	strings []string
	index   map[string]uint32
	uses    []uint32
}

func newStringTable() *stringTable {
	return &stringTable{index: map[string]uint32{}}
}

func (t *stringTable) intern(s string) {
	if idx, ok := t.index[s]; ok {
		t.uses[idx]++
		return
	}
	t.index[s] = uint32(len(t.strings))
	t.strings = append(t.strings, s)
	t.uses = append(t.uses, 1)
}

// orderByUse renumbers the table by descending use count; ties keep
// first-seen order. The format is unchanged — a decoder reads indices
// against whatever table it is given.
func (t *stringTable) orderByUse() {
	order := make([]uint32, len(t.strings))
	for i := range order {
		order[i] = uint32(i)
	}
	sort.SliceStable(order, func(a, b int) bool {
		return t.uses[order[a]] > t.uses[order[b]]
	})
	ordered := make([]string, len(order))
	for newIdx, old := range order {
		s := t.strings[old]
		t.index[s] = uint32(newIdx)
		ordered[newIdx] = s
	}
	t.strings = ordered
}

func (t *stringTable) indexOf(s string) uint64 {
	return uint64(t.index[s])
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
		for _, p := range x.V.pairs {
			t.intern(p.Key)
			t.collectValue(p.Value)
		}
	}
}

func (t *stringTable) collectMetadata(tree *MetadataTree) {
	for _, path := range tree.paths {
		t.intern(path)
		m := tree.levels[path]
		for _, key := range m.keys {
			meta := m.items[key]
			t.intern(key)
			for _, mk := range meta.Markers {
				t.intern(mk)
			}
			for _, a := range meta.Args {
				t.intern(a)
			}
			if meta.hasTypeHint() {
				t.intern(meta.TypeHint)
			}
			if c := meta.Constraints; c != nil {
				if c.hasTypeName() {
					t.intern(c.TypeName)
				}
				if c.hasPattern() {
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
	r := &stringTableReader{strings: make([]string, 0, safeCapacity(count, cur))}
	for i := uint64(0); i < count; i++ {
		ln, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		if ln > uint64(len(cur.data)-cur.pos) {
			return nil, errors.New("unexpected end of data in string table")
		}
		raw := cur.data[cur.pos : cur.pos+int(ln)]
		if !utf8.Valid(raw) {
			return nil, errors.New("invalid UTF-8 in string table")
		}
		r.strings = append(r.strings, string(raw))
		cur.pos += int(ln)
	}
	return r, nil
}

func (r *stringTableReader) get(idx uint64) (string, error) {
	// An index is a u32 on the wire of synx-core; anything larger is not one.
	if idx > math.MaxUint32 {
		return "", errors.New("string index out of range")
	}
	if idx >= uint64(len(r.strings)) {
		return "", fmt.Errorf("string index %d out of bounds (size %d)", idx, len(r.strings))
	}
	return r.strings[idx], nil
}

// safeCapacity bounds a capacity read from untrusted input: every element
// costs at least one byte, so a count never exceeds the bytes left.
//
// safety: and never more than maxPreallocItems. Capped at the bytes that
// remain alone, an 8 KB file of zeros still reserved 128 MiB for a list it
// did not hold.
func safeCapacity(count uint64, cur *cursor) int {
	remaining := uint64(len(cur.data) - cur.pos)
	if count > remaining {
		count = remaining
	}
	if count > maxPreallocItems {
		count = maxPreallocItems
	}
	return int(count)
}

// ─── varint / zigzag ────────────────────────────────────────────────────────

type cursor struct {
	data   []byte
	pos    int
	budget decodeBudget
}

// text charges a string the tree refers to and returns it.
func (cur *cursor) text(s string, err error) (string, error) {
	if err != nil {
		return "", err
	}
	if err := cur.budget.charge(len(s)); err != nil {
		return "", err
	}
	return s, nil
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

func decodeF64LE(cur *cursor, what string) (float64, error) {
	if cur.pos+8 > len(cur.data) {
		return 0, errors.New(what)
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
		encodeVarint(out, t.indexOf(x.V))
	case SecretValue:
		out.WriteByte(tagSecret)
		encodeVarint(out, t.indexOf(x.V))
	case ArrayValue:
		out.WriteByte(tagArray)
		encodeVarint(out, uint64(len(x.V)))
		for _, item := range x.V {
			encodeValue(out, item, t)
		}
	case ObjectValue:
		out.WriteByte(tagObject)
		pairs := sortedPairs(x.V)
		encodeVarint(out, uint64(len(pairs)))
		for _, p := range pairs {
			encodeVarint(out, t.indexOf(p.Key))
			val := p.Value
			if val == nil {
				val = Null
			}
			encodeValue(out, val, t)
		}
	default:
		out.WriteByte(tagNull)
	}
}

func decodeValue(cur *cursor, t *stringTableReader, depth int) (Value, error) {
	if depth > maxDecodeDepth {
		return nil, fmt.Errorf("nesting exceeds %d — refusing to decode", maxDecodeDepth)
	}
	if err := cur.budget.value(); err != nil {
		return nil, err
	}
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
		f, err := decodeF64LE(cur, "unexpected end of data in float")
		if err != nil {
			return nil, err
		}
		return Float(f), nil
	case tagString, tagSecret:
		idx, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		s, err := cur.text(t.get(idx))
		if err != nil {
			return nil, err
		}
		if tag == tagSecret {
			return Secret(s), nil
		}
		return String(s), nil
	case tagArray:
		count, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		arr := make([]Value, 0, safeCapacity(count, cur))
		for i := uint64(0); i < count; i++ {
			v, err := decodeValue(cur, t, depth+1)
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
		obj := newObjectCap(safeCapacity(count, cur))
		for i := uint64(0); i < count; i++ {
			ki, err := decodeVarint(cur)
			if err != nil {
				return nil, err
			}
			k, err := cur.text(t.get(ki))
			if err != nil {
				return nil, err
			}
			v, err := decodeValue(cur, t, depth+1)
			if err != nil {
				return nil, err
			}
			obj.Set(k, v)
		}
		return Object_(obj), nil
	}
	return nil, fmt.Errorf("unknown type tag: 0x%02x", tag)
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
	if c.hasTypeName() {
		bits |= 0x04
	}
	if c.Required {
		bits |= 0x08
	}
	if c.Readonly {
		bits |= 0x10
	}
	if c.hasPattern() {
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
	if c.hasTypeName() {
		encodeVarint(out, t.indexOf(c.TypeName))
	}
	if c.hasPattern() {
		encodeVarint(out, t.indexOf(c.Pattern))
	}
	if c.EnumValues != nil {
		encodeVarint(out, uint64(len(c.EnumValues)))
		for _, v := range c.EnumValues {
			encodeVarint(out, t.indexOf(v))
		}
	}
}

func decodeConstraints(cur *cursor, t *stringTableReader) (*Constraints, error) {
	if cur.pos >= len(cur.data) {
		return nil, errors.New("unexpected end of data in constraints")
	}
	bits := cur.data[cur.pos]
	cur.pos++
	c := &Constraints{}
	if bits&0x01 != 0 {
		v, err := decodeF64LE(cur, "truncated min")
		if err != nil {
			return nil, err
		}
		c.Min = &v
	}
	if bits&0x02 != 0 {
		v, err := decodeF64LE(cur, "truncated max")
		if err != nil {
			return nil, err
		}
		c.Max = &v
	}
	if bits&0x04 != 0 {
		s, err := decodeIndexed(cur, t)
		if err != nil {
			return nil, err
		}
		c.TypeName, c.typeNameSet = s, true
	}
	c.Required = bits&0x08 != 0
	c.Readonly = bits&0x10 != 0
	if bits&0x20 != 0 {
		s, err := decodeIndexed(cur, t)
		if err != nil {
			return nil, err
		}
		c.Pattern, c.patternSet = s, true
	}
	if bits&0x40 != 0 {
		vals, err := decodeStrings(cur, t)
		if err != nil {
			return nil, err
		}
		c.EnumValues = vals
	}
	return c, nil
}

func decodeIndexed(cur *cursor, t *stringTableReader) (string, error) {
	idx, err := decodeVarint(cur)
	if err != nil {
		return "", err
	}
	return cur.text(t.get(idx))
}

func decodeStrings(cur *cursor, t *stringTableReader) ([]string, error) {
	count, err := decodeVarint(cur)
	if err != nil {
		return nil, err
	}
	out := make([]string, 0, safeCapacity(count, cur))
	for i := uint64(0); i < count; i++ {
		if err := cur.budget.value(); err != nil {
			return nil, err
		}
		s, err := decodeIndexed(cur, t)
		if err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, nil
}

func encodeMetadata(out *bytes.Buffer, tree *MetadataTree, t *stringTable) {
	paths := tree.Paths()
	sort.Strings(paths)
	encodeVarint(out, uint64(len(paths)))
	for _, path := range paths {
		encodeVarint(out, t.indexOf(path))
		m := tree.levels[path]
		keys := m.Keys()
		sort.Strings(keys)
		encodeVarint(out, uint64(len(keys)))
		for _, fk := range keys {
			meta := m.items[fk]
			encodeVarint(out, t.indexOf(fk))
			encodeVarint(out, uint64(len(meta.Markers)))
			for _, mk := range meta.Markers {
				encodeVarint(out, t.indexOf(mk))
			}
			encodeVarint(out, uint64(len(meta.Args)))
			for _, a := range meta.Args {
				encodeVarint(out, t.indexOf(a))
			}
			if meta.hasTypeHint() {
				out.WriteByte(1)
				encodeVarint(out, t.indexOf(meta.TypeHint))
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

func decodeMetadata(cur *cursor, t *stringTableReader) (*MetadataTree, error) {
	outer, err := decodeVarint(cur)
	if err != nil {
		return nil, err
	}
	tree := NewMetadataTree()
	for i := uint64(0); i < outer; i++ {
		if err := cur.budget.value(); err != nil {
			return nil, err
		}
		path, err := decodeIndexed(cur, t)
		if err != nil {
			return nil, err
		}
		inner, err := decodeVarint(cur)
		if err != nil {
			return nil, err
		}
		m := NewMetaMap()
		for j := uint64(0); j < inner; j++ {
			fk, err := decodeIndexed(cur, t)
			if err != nil {
				return nil, err
			}
			if err := cur.budget.value(); err != nil {
				return nil, err
			}
			var meta Meta
			if meta.Markers, err = decodeStrings(cur, t); err != nil {
				return nil, err
			}
			if meta.Args, err = decodeStrings(cur, t); err != nil {
				return nil, err
			}
			if cur.pos >= len(cur.data) {
				return nil, errors.New("unexpected end in meta")
			}
			hasTh := cur.data[cur.pos]
			cur.pos++
			if hasTh != 0 {
				if meta.TypeHint, err = decodeIndexed(cur, t); err != nil {
					return nil, err
				}
				meta.typeHintSet = true
			}
			if cur.pos >= len(cur.data) {
				return nil, errors.New("unexpected end in meta")
			}
			hasC := cur.data[cur.pos]
			cur.pos++
			if hasC != 0 {
				if meta.Constraints, err = decodeConstraints(cur, t); err != nil {
					return nil, err
				}
			}
			m.Set(fk, meta)
		}
		tree.Set(path, m)
	}
	return tree, nil
}

func encodeIncludes(out *bytes.Buffer, incs []IncludeDirective, t *stringTable) {
	encodeVarint(out, uint64(len(incs)))
	for _, inc := range incs {
		encodeVarint(out, t.indexOf(inc.Path))
		encodeVarint(out, t.indexOf(inc.Alias))
	}
}

func decodeIncludes(cur *cursor, t *stringTableReader) ([]IncludeDirective, error) {
	count, err := decodeVarint(cur)
	if err != nil {
		return nil, err
	}
	out := make([]IncludeDirective, 0, safeCapacity(count, cur))
	for i := uint64(0); i < count; i++ {
		if err := cur.budget.value(); err != nil {
			return nil, err
		}
		p, err := decodeIndexed(cur, t)
		if err != nil {
			return nil, err
		}
		a, err := decodeIndexed(cur, t)
		if err != nil {
			return nil, err
		}
		out = append(out, IncludeDirective{Path: p, Alias: a})
	}
	return out, nil
}
