package synx

import (
	"fmt"
	"math"
	"sort"
	"strconv"
	"strings"
)

const MaxSerializeDepth = 128

// Stringify renders a Value as SYNX text.
func Stringify(v Value) string {
	var sb strings.Builder
	sb.Grow(2048)
	serialize(&sb, v, 0)
	return sb.String()
}

func serialize(out *strings.Builder, v Value, depth int) {
	if depth > MaxSerializeDepth {
		out.WriteString("[synx:max-depth]\n")
		return
	}
	obj, ok := v.(ObjectValue)
	if !ok {
		out.WriteString(FormatPrimitive(v))
		return
	}
	indent := strings.Repeat(" ", depth*2)
	keys := obj.V.SortedKeys()
	for _, k := range keys {
		val, _ := obj.V.Get(k)
		switch x := val.(type) {
		case ArrayValue:
			out.WriteString(indent)
			out.WriteString(k)
			out.WriteByte('\n')
			for _, item := range x.V {
				if inner, ok := item.(ObjectValue); ok && inner.V.Len() > 0 {
					pairs := inner.V.Pairs()
					out.WriteString(indent)
					out.WriteString("  - ")
					out.WriteString(pairs[0].Key)
					out.WriteByte(' ')
					out.WriteString(FormatPrimitive(pairs[0].Value))
					out.WriteByte('\n')
					for j := 1; j < len(pairs); j++ {
						out.WriteString(indent)
						out.WriteString("    ")
						out.WriteString(pairs[j].Key)
						out.WriteByte(' ')
						out.WriteString(FormatPrimitive(pairs[j].Value))
						out.WriteByte('\n')
					}
				} else {
					out.WriteString(indent)
					out.WriteString("  - ")
					out.WriteString(FormatPrimitive(item))
					out.WriteByte('\n')
				}
			}
		case ObjectValue:
			out.WriteString(indent)
			out.WriteString(k)
			out.WriteByte('\n')
			serialize(out, val, depth+1)
		case StringValue:
			if strings.Contains(x.V, "\n") {
				out.WriteString(indent)
				out.WriteString(k)
				out.WriteString(" |\n")
				for _, line := range strings.Split(x.V, "\n") {
					out.WriteString(indent)
					out.WriteString("  ")
					out.WriteString(line)
					out.WriteByte('\n')
				}
			} else {
				out.WriteString(indent)
				out.WriteString(k)
				out.WriteByte(' ')
				out.WriteString(FormatPrimitive(val))
				out.WriteByte('\n')
			}
		default:
			out.WriteString(indent)
			out.WriteString(k)
			out.WriteByte(' ')
			out.WriteString(FormatPrimitive(val))
			out.WriteByte('\n')
		}
	}
}

// FormatPrimitive renders a single Value as a SYNX scalar literal.
func FormatPrimitive(v Value) string {
	switch x := v.(type) {
	case StringValue:
		return x.V
	case IntValue:
		return strconv.FormatInt(x.V, 10)
	case FloatValue:
		if math.IsNaN(x.V) || math.IsInf(x.V, 0) {
			return "null"
		}
		s := fmt.Sprintf("%.17g", x.V)
		if !strings.ContainsAny(s, ".eE") {
			s += ".0"
		}
		return s
	case BoolValue:
		if x.V {
			return "true"
		}
		return "false"
	case NullValue:
		return "null"
	case ArrayValue:
		parts := make([]string, len(x.V))
		for i, item := range x.V {
			parts[i] = FormatPrimitive(item)
		}
		return "[" + strings.Join(parts, ", ") + "]"
	case ObjectValue:
		return "[Object]"
	case SecretValue:
		return "[SECRET]"
	}
	return ""
}

// ─── canonical formatter ────────────────────────────────────────────────────

const MaxFormatParseDepth = 128

// Format canonically reformats SYNX text — sorts keys, normalises indent,
// preserves leading directives.
func Format(text string) string {
	clamped := ClampText(text)
	lines := strings.Split(clamped, "\n")
	for i, l := range lines {
		if len(l) > 0 && l[len(l)-1] == '\r' {
			lines[i] = l[:len(l)-1]
		}
	}

	var directives []string
	bodyStart := 0
	for i, l := range lines {
		t := strings.TrimSpace(l)
		switch t {
		case "!active", "!lock", "!tool", "!schema", "!llm", "#!mode:active":
			directives = append(directives, t)
			bodyStart = i + 1
			continue
		}
		if t == "" || strings.HasPrefix(t, "#") || strings.HasPrefix(t, "//") {
			bodyStart = i + 1
			continue
		}
		break
	}

	var nodes []fmtNode
	fmtParse(lines, bodyStart, 0, 0, &nodes)
	fmtSort(nodes)

	var out strings.Builder
	if len(directives) > 0 {
		out.WriteString(strings.Join(directives, "\n"))
		out.WriteString("\n\n")
	}
	fmtEmit(nodes, 0, &out)

	r := out.String()
	r = strings.TrimRight(r, " \t\n") + "\n"
	return r
}

type fmtNode struct {
	header      string
	children    []fmtNode
	listItems   []string
	isMultiline bool
}

func fmtParseIndent(line string) int {
	i := 0
	for i < len(line) && (line[i] == ' ' || line[i] == '\t') {
		i++
	}
	return i
}

func fmtParse(lines []string, start, base, depth int, out *[]fmtNode) int {
	if depth > MaxFormatParseDepth {
		return start
	}
	i := start
	for i < len(lines) {
		raw := lines[i]
		t := strings.TrimSpace(raw)
		if t == "" {
			i++
			continue
		}
		ind := fmtParseIndent(raw)
		if ind < base {
			break
		}
		if ind > base {
			i++
			continue
		}
		if strings.HasPrefix(t, "- ") || strings.HasPrefix(t, "#") || strings.HasPrefix(t, "//") {
			i++
			continue
		}
		n := fmtNode{header: t}
		n.isMultiline = t == "|" || strings.HasSuffix(t, " |")
		i++
		for i < len(lines) {
			cr := lines[i]
			ct := strings.TrimSpace(cr)
			if ct == "" {
				i++
				continue
			}
			ci := fmtParseIndent(cr)
			if ci <= base {
				break
			}
			if n.isMultiline || strings.HasPrefix(ct, "- ") {
				n.listItems = append(n.listItems, ct)
				i++
			} else if strings.HasPrefix(ct, "#") || strings.HasPrefix(ct, "//") {
				i++
			} else {
				var subs []fmtNode
				i = fmtParse(lines, i, ci, depth+1, &subs)
				n.children = append(n.children, subs...)
			}
		}
		*out = append(*out, n)
	}
	return i
}

func fmtSortKey(header string) string {
	end := 0
	for end < len(header) {
		c := header[end]
		if c == ' ' || c == '\t' || c == '[' || c == ':' || c == '(' {
			break
		}
		end++
	}
	return strings.ToLower(header[:end])
}

func fmtSort(nodes []fmtNode) {
	sort.SliceStable(nodes, func(a, b int) bool {
		return fmtSortKey(nodes[a].header) < fmtSortKey(nodes[b].header)
	})
	for i := range nodes {
		fmtSort(nodes[i].children)
	}
}

func fmtEmit(nodes []fmtNode, indent int, out *strings.Builder) {
	sp := strings.Repeat(" ", indent)
	itemSp := strings.Repeat(" ", indent+2)
	for _, n := range nodes {
		out.WriteString(sp)
		out.WriteString(n.header)
		out.WriteByte('\n')
		if len(n.children) > 0 {
			fmtEmit(n.children, indent+2, out)
		}
		for _, li := range n.listItems {
			out.WriteString(itemSp)
			out.WriteString(li)
			out.WriteByte('\n')
		}
		if indent == 0 && (len(n.children) > 0 || len(n.listItems) > 0) {
			out.WriteByte('\n')
		}
	}
}
