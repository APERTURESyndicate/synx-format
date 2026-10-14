package synx

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
)

const MaxJSONDepth = 128

// ToJSON serialises a Value to canonical JSON (sorted object keys, secrets
// redacted, floats with mandatory decimal marker).
func ToJSON(v Value) string {
	var sb strings.Builder
	sb.Grow(2048)
	writeJSON(&sb, v, 0)
	return sb.String()
}

func writeJSON(out *strings.Builder, v Value, depth int) {
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
		out.WriteString(strconv.FormatInt(x.V, 10))
	case FloatValue:
		if math.IsNaN(x.V) || math.IsInf(x.V, 0) {
			out.WriteString("null")
			return
		}
		out.WriteString(formatFloat(x.V))
	case StringValue:
		out.WriteByte('"')
		escapeJSON(out, x.V)
		out.WriteByte('"')
	case SecretValue:
		out.WriteString("\"[SECRET]\"")
	case ArrayValue:
		out.WriteByte('[')
		for i, item := range x.V {
			if i > 0 {
				out.WriteByte(',')
			}
			writeJSON(out, item, depth+1)
		}
		out.WriteByte(']')
	case ObjectValue:
		out.WriteByte('{')
		obj := x.V
		keys := obj.Keys()
		sort.Strings(keys)
		first := true
		for _, k := range keys {
			if !first {
				out.WriteByte(',')
			}
			first = false
			out.WriteByte('"')
			escapeJSON(out, k)
			out.WriteString("\":")
			vv, _ := obj.Get(k)
			if vv == nil {
				vv = Null
			}
			writeJSON(out, vv, depth+1)
		}
		out.WriteByte('}')
	}
}

// formatFloat renders the shortest round-trip form of f with the same layout
// as Rust's `ryu` (what the reference `to_json` emits): plain decimals while
// the decimal exponent stays within [-5, 16), otherwise `d.ddde±N`, and a
// mandatory `.0` on whole numbers.
func formatFloat(f float64) string {
	if f == 0 {
		if math.Signbit(f) {
			return "-0.0"
		}
		return "0.0"
	}
	e := strconv.FormatFloat(math.Abs(f), 'e', -1, 64)
	mant, expStr, _ := strings.Cut(e, "e")
	digits := strings.Replace(mant, ".", "", 1)
	exp, _ := strconv.Atoi(expStr)
	length := len(digits)
	kk := exp + 1
	k := kk - length

	var sb strings.Builder
	if f < 0 {
		sb.WriteByte('-')
	}
	switch {
	case k >= 0 && kk <= 16:
		sb.WriteString(digits)
		sb.WriteString(strings.Repeat("0", k))
		sb.WriteString(".0")
	case kk > 0 && kk <= 16:
		sb.WriteString(digits[:kk])
		sb.WriteByte('.')
		sb.WriteString(digits[kk:])
	case kk > -5 && kk <= 0:
		sb.WriteString("0.")
		sb.WriteString(strings.Repeat("0", -kk))
		sb.WriteString(digits)
	case length == 1:
		sb.WriteString(digits)
		sb.WriteByte('e')
		sb.WriteString(strconv.Itoa(kk - 1))
	default:
		sb.WriteString(digits[:1])
		sb.WriteByte('.')
		sb.WriteString(digits[1:])
		sb.WriteByte('e')
		sb.WriteString(strconv.Itoa(kk - 1))
	}
	return sb.String()
}

func escapeJSON(out *strings.Builder, s string) {
	for i := 0; i < len(s); i++ {
		c := s[i]
		switch c {
		case '"':
			out.WriteString("\\\"")
		case '\\':
			out.WriteString("\\\\")
		case '\n':
			out.WriteString("\\n")
		case '\r':
			out.WriteString("\\r")
		case '\t':
			out.WriteString("\\t")
		default:
			if c < 0x20 {
				fmt.Fprintf(out, "\\u%04x", c)
			} else {
				out.WriteByte(c)
			}
		}
	}
}
