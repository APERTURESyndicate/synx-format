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
		s := fmt.Sprintf("%.17g", x.V)
		if !strings.ContainsAny(s, ".eE") {
			s += ".0"
		}
		out.WriteString(s)
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
