package synx

import (
	"math"
	"strconv"
	"strings"
)

const MaxJSONDepth = 128

// ToJSON serialises a Value to canonical JSON: object keys sorted by code
// point at every level, secrets redacted, floats in the layout of Rust's
// `ryu` (what the reference `to_json` emits).
func ToJSON(v Value) string {
	var sb strings.Builder
	sb.Grow(2048)
	w := jsonWriter{out: &sb}
	w.write(v, 0)
	return sb.String()
}

// jsonWriter keeps one sort buffer per depth, reused across objects.
type jsonWriter struct {
	out  *strings.Builder
	bufs [][]ObjectPair
}

func (w *jsonWriter) write(v Value, depth int) {
	out := w.out
	if depth > MaxJSONDepth {
		out.WriteString("null")
		return
	}
	switch x := v.(type) {
	case NullValue:
		out.WriteString("null")
	case BoolValue:
		if x.V {
			out.WriteString("true")
		} else {
			out.WriteString("false")
		}
	case IntValue:
		var buf [24]byte
		out.Write(strconv.AppendInt(buf[:0], x.V, 10))
	case FloatValue:
		if math.IsNaN(x.V) || math.IsInf(x.V, 0) {
			out.WriteString("null")
			return
		}
		out.WriteString(formatFloat(x.V))
	case StringValue:
		writeJSONString(out, x.V)
	case SecretValue:
		out.WriteString("\"[SECRET]\"")
	case ArrayValue:
		out.WriteByte('[')
		for i, item := range x.V {
			if i > 0 {
				out.WriteByte(',')
			}
			w.write(item, depth+1)
		}
		out.WriteByte(']')
	case ObjectValue:
		out.WriteByte('{')
		for len(w.bufs) <= depth {
			w.bufs = append(w.bufs, nil)
		}
		pairs := sortPairsInto(w.bufs[depth], x.V)
		w.bufs[depth] = pairs
		for i, p := range pairs {
			if i > 0 {
				out.WriteByte(',')
			}
			writeJSONString(out, p.Key)
			out.WriteByte(':')
			vv := p.Value
			if vv == nil {
				vv = Null
			}
			w.write(vv, depth+1)
		}
		out.WriteByte('}')
	default:
		out.WriteString("null")
	}
}

// formatFloat renders the shortest round-trip form of f with the same layout
// as Rust's `ryu`: plain decimals while the decimal exponent stays within
// [-5, 16), otherwise `d.ddde±N`, and a mandatory `.0` on whole numbers.
func formatFloat(f float64) string {
	if f == 0 {
		if math.Signbit(f) {
			return "-0.0"
		}
		return "0.0"
	}
	var ebuf [32]byte
	e := strconv.AppendFloat(ebuf[:0], math.Abs(f), 'e', -1, 64)
	ePos := 0
	for ePos < len(e) && e[ePos] != 'e' {
		ePos++
	}
	var dbuf [24]byte
	digits := dbuf[:0]
	for _, c := range e[:ePos] {
		if c != '.' {
			digits = append(digits, c)
		}
	}
	exp, _ := strconv.Atoi(string(e[ePos+1:]))
	length := len(digits)
	kk := exp + 1
	k := kk - length

	var sb strings.Builder
	sb.Grow(length + 8)
	if f < 0 {
		sb.WriteByte('-')
	}
	switch {
	case k >= 0 && kk <= 16:
		sb.Write(digits)
		for i := 0; i < k; i++ {
			sb.WriteByte('0')
		}
		sb.WriteString(".0")
	case kk > 0 && kk <= 16:
		sb.Write(digits[:kk])
		sb.WriteByte('.')
		sb.Write(digits[kk:])
	case kk > -5 && kk <= 0:
		sb.WriteString("0.")
		for i := 0; i < -kk; i++ {
			sb.WriteByte('0')
		}
		sb.Write(digits)
	case length == 1:
		sb.Write(digits)
		sb.WriteByte('e')
		sb.WriteString(strconv.Itoa(kk - 1))
	default:
		sb.Write(digits[:1])
		sb.WriteByte('.')
		sb.Write(digits[1:])
		sb.WriteByte('e')
		sb.WriteString(strconv.Itoa(kk - 1))
	}
	return sb.String()
}

// writeJSONString writes s as a JSON string literal. Unescaped runs are
// copied whole; every byte that needs escaping is ASCII, so scanning bytes
// never splits a character.
func writeJSONString(out *strings.Builder, s string) {
	const hex = "0123456789abcdef"
	out.WriteByte('"')
	start := 0
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c >= 0x20 && c != '"' && c != '\\' {
			continue
		}
		out.WriteString(s[start:i])
		switch c {
		case '"':
			out.WriteString(`\"`)
		case '\\':
			out.WriteString(`\\`)
		case '\n':
			out.WriteString(`\n`)
		case '\r':
			out.WriteString(`\r`)
		case '\t':
			out.WriteString(`\t`)
		default:
			out.WriteString(`\u00`)
			out.WriteByte(hex[c>>4])
			out.WriteByte(hex[c&0xf])
		}
		start = i + 1
	}
	out.WriteString(s[start:])
	out.WriteByte('"')
}
