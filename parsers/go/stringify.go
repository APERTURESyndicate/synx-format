package synx

import (
	"slices"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

const MaxSerializeDepth = 128

// Stringify renders a Value as SYNX text that parses back to the same data.
// It writes through the SYNXL §14.1 block writer of crates/synx-core
// (`synxl::write_synx_entry`): strings that would read back as another type
// or be cut as a comment are quoted or carried as `|+`, multi-line text keeps
// its relative indent, a list item leads with a key that fits its `- ` line,
// and secrets are written as [SECRET].
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
	for _, p := range sortedPairs(obj.V) {
		writeSynxEntry(out, p.Key, p.Value, depth*2, depth)
	}
}

// sortedPairs is a copy of the entries in key order (code point order).
func sortedPairs(o *Object) []ObjectPair {
	return sortPairsInto(make([]ObjectPair, 0, len(o.pairs)), o)
}

func sortPairsInto(buf []ObjectPair, o *Object) []ObjectPair {
	buf = append(buf[:0], o.pairs...)
	slices.SortFunc(buf, func(a, b ObjectPair) int { return strings.Compare(a.Key, b.Key) })
	return buf
}

// FormatPrimitive renders a single Value as Rust's `format_primitive` does.
func FormatPrimitive(v Value) string {
	switch x := v.(type) {
	case StringValue:
		return x.V
	case IntValue:
		return strconv.FormatInt(x.V, 10)
	case FloatValue:
		s := rustFloatDisplay(x.V)
		if !strings.Contains(s, ".") {
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

// ─── §14.1 block writer ─────────────────────────────────────────────────────

func pushIndent(out *strings.Builder, n int) {
	for i := 0; i < n; i++ {
		out.WriteByte(' ')
	}
}

func writeSynxEntry(out *strings.Builder, key string, v Value, indent, depth int) {
	if depth > MaxNestingDepth {
		return
	}
	switch x := v.(type) {
	case ObjectValue:
		pushIndent(out, indent)
		out.WriteString(key)
		out.WriteByte('\n')
		for _, p := range sortedPairs(x.V) {
			writeSynxEntry(out, p.Key, p.Value, indent+2, depth+1)
		}
		return
	case ArrayValue:
		pushIndent(out, indent)
		out.WriteString(key)
		if len(x.V) == 0 {
			// A bare key reads back as an empty object; `:join` is the SYNX
			// surface that yields an empty list.
			out.WriteString(":join")
		}
		out.WriteByte('\n')
		for _, item := range x.V {
			writeSynxItem(out, item, indent+2, depth+1)
		}
		return
	case StringValue:
		if strings.IndexByte(x.V, '\n') >= 0 {
			writeSynxMultiline(out, key, x.V, indent)
			return
		}
	}
	if text, ok := synxScalar(v); ok {
		pushIndent(out, indent)
		out.WriteString(key)
		out.WriteByte(' ')
		out.WriteString(text)
		out.WriteByte('\n')
		return
	}
	// Not representable as one token — an indent-preserving block, whose body
	// is neither comment-stripped nor cast.
	s, _ := v.(StringValue)
	writeSynxMultiline(out, key, s.V, indent)
}

func writeSynxItem(out *strings.Builder, item Value, indent, depth int) {
	if depth > MaxNestingDepth {
		return
	}
	obj, ok := item.(ObjectValue)
	if !ok {
		pushIndent(out, indent)
		out.WriteString("- ")
		if text, ok := synxScalar(item); ok {
			out.WriteString(text)
		} else if s, ok := item.(StringValue); ok {
			out.WriteString(s.V)
		}
		out.WriteByte('\n')
		return
	}
	pairs := sortedPairs(obj.V)
	// An empty object in a list is a bare dash.
	if len(pairs) == 0 {
		pushIndent(out, indent)
		out.WriteString("-\n")
		return
	}
	// The `- ` line has to carry a key whose value fits on it; SYNX attaches
	// everything deeper to the item, not to that key.
	head := -1
	for i, p := range pairs {
		if dashLineSafe(p.Value) {
			head = i
			break
		}
	}
	// An item is an object only when a deeper line follows its `- ` line, so a
	// one-key item, or one whose head could not take its value, writes the
	// head again in full below (a repeated key is legal; the last one wins).
	repeatHead := head < 0 || len(pairs) == 1
	if head < 0 {
		head = 0
	}
	pushIndent(out, indent)
	out.WriteString("- ")
	out.WriteString(pairs[head].Key)
	if text, ok := synxScalar(pairs[head].Value); ok && text != "" {
		out.WriteByte(' ')
		out.WriteString(text)
	}
	out.WriteByte('\n')
	for i, p := range pairs {
		if i == head && !repeatHead {
			continue
		}
		writeSynxEntry(out, p.Key, p.Value, indent+2, depth+1)
	}
}

// dashLineSafe: may this value share the `- ` line of a list item?
func dashLineSafe(v Value) bool {
	switch x := v.(type) {
	case ObjectValue:
		return x.V.IsEmpty()
	case ArrayValue:
		return false
	case StringValue:
		if strings.IndexByte(x.V, '\n') >= 0 || x.V == "" {
			return false
		}
		_, ok := synxScalar(v)
		return ok
	}
	return true
}

// writeSynxMultiline writes `key |+` and an indented body.
func writeSynxMultiline(out *strings.Builder, key, body string, indent int) {
	pushIndent(out, indent)
	out.WriteString(key)
	out.WriteString(" |+\n")
	for {
		line := body
		nl := strings.IndexByte(body, '\n')
		if nl >= 0 {
			line = body[:nl]
		}
		pushIndent(out, indent+2)
		out.WriteString(line)
		out.WriteByte('\n')
		if nl < 0 {
			break
		}
		body = body[nl+1:]
	}
}

// synxScalar renders a scalar as one SYNX value token, or false when no token
// can carry it. A value is comment-stripped at ` #` / ` //` before casting,
// so a string containing either cannot be rescued by quoting.
func synxScalar(v Value) (string, bool) {
	switch x := v.(type) {
	case NullValue:
		return "null", true
	case BoolValue:
		if x.V {
			return "true", true
		}
		return "false", true
	case IntValue:
		return strconv.FormatInt(x.V, 10), true
	case FloatValue:
		// A non-finite float has no JSON form either; `null` projects the same.
		if x.V != x.V || x.V > 1.7976931348623157e308 || x.V < -1.7976931348623157e308 {
			return "null", true
		}
		return writeFloat(x.V), true
	case SecretValue:
		return "[SECRET]", true
	case StringValue:
		s := x.V
		if strings.IndexByte(s, '\n') >= 0 || strings.Contains(s, " #") || strings.Contains(s, " //") {
			return "", false
		}
		if !synxValueNeedsQuote(s) {
			return s, true
		}
		if q, ok := pickQuote(s); ok {
			return string(q) + s + string(q), true
		}
		return "", false
	}
	return "", false
}

// writeFloat writes a float so the SYNX cast reads it back as the same float:
// plain decimal digits with a `.0` on a whole number.
func writeFloat(f float64) string {
	s := rustFloatDisplay(f)
	if !strings.Contains(s, ".") {
		s += ".0"
	}
	return s
}

// synxValueNeedsQuote: would this string be misread as a bare SYNX value?
func synxValueNeedsQuote(s string) bool {
	return s == "" || s != strings.TrimFunc(s, unicode.IsSpace) || s == "|" || s == "|+" ||
		isQuoteWrapped(s) || !autoCastKeepsString(s)
}

// autoCastKeepsString: does `cast(s)` give back the string itself?
func autoCastKeepsString(s string) bool {
	if isQuoteWrapped(s) {
		return true
	}
	switch s {
	case "true", "false", "null":
		return false
	}
	start := 0
	if s != "" && s[0] == '-' {
		start = 1
	}
	if len(s) <= start || s[start] < '0' || s[start] > '9' {
		return true
	}
	dot := -1
	for j := start; j < len(s); j++ {
		c := s[j]
		if c == '.' {
			if dot >= 0 {
				return true
			}
			dot = j
		} else if c < '0' || c > '9' {
			return true
		}
	}
	if dot < 0 {
		_, err := strconv.ParseInt(s, 10, 64)
		return err != nil
	}
	if dot > start && dot < len(s)-1 {
		_, ok := parseRustFloat(s)
		return !ok
	}
	return true
}

func isQuoteWrapped(s string) bool {
	return len(s) >= 2 && ((s[0] == '"' && s[len(s)-1] == '"') || (s[0] == '\'' && s[len(s)-1] == '\''))
}

// pickQuote is a quote character that does not occur in s.
func pickQuote(s string) (byte, bool) {
	if strings.IndexByte(s, '"') < 0 {
		return '"', true
	}
	if strings.IndexByte(s, '\'') < 0 {
		return '\'', true
	}
	return 0, false
}

// ─── canonical formatter ────────────────────────────────────────────────────

const MaxFormatParseDepth = 128

// Format canonically reformats SYNX text: keys sorted (stable, by the
// lowercased key, code point order) at every level, two spaces per level, one
// blank line after each top-level block, comments dropped, leading
// directives kept. A `|` / `|+` body is content: never parsed as keys nor
// sorted, and a `|+` body keeps its relative indentation.
func Format(text string) string {
	clamped := ClampText(text)
	lines := rustLines(clamped)

	var directives []string
	bodyStart := 0
	for i, l := range lines {
		t := trimWS(l)
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

	nodes, _ := fmtParse(lines, bodyStart, 0, 0)
	fmtSort(nodes)

	var out strings.Builder
	n := len(clamped)
	if n < 64 {
		n = 64
	}
	out.Grow(n)
	if len(directives) > 0 {
		out.WriteString(strings.Join(directives, "\n"))
		out.WriteString("\n\n")
	}
	fmtEmit(nodes, 0, &out)
	return strings.TrimRightFunc(out.String(), unicode.IsSpace) + "\n"
}

type fmtNode struct {
	header      string
	sortKey     string
	children    []fmtNode
	listItems   []string
	isMultiline bool
}

func fmtParse(lines []string, start, base, depth int) ([]fmtNode, int) {
	if depth > MaxFormatParseDepth {
		// Past the cap the subtree is dropped, but its lines are consumed: up to
		// the next content line indented less than this level.
		i := start
		for i < len(lines) && (trimWS(lines[i]) == "" || indentLen(lines[i]) >= base) {
			i++
		}
		return nil, i
	}
	var nodes []fmtNode
	i := start
	for i < len(lines) {
		raw := lines[i]
		t := trimWS(raw)
		if t == "" {
			i++
			continue
		}
		ind := indentLen(raw)
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
		preserve := strings.HasSuffix(t, " |+") || t == "|+"
		node := fmtNode{header: t, isMultiline: preserve || strings.HasSuffix(t, " |") || t == "|"}
		blockBase := -1
		i++
		for i < len(lines) {
			cr := lines[i]
			ct := trimWS(cr)
			if ct == "" {
				i++
				continue
			}
			ci := indentLen(cr)
			if ci <= base {
				break
			}
			switch {
			case preserve:
				if blockBase < 0 {
					blockBase = ci
				}
				body := trimRightWS(cr)
				strip := blockBase
				if ci < strip {
					strip = ci
				}
				node.listItems = append(node.listItems, body[stripBoundary(body, strip):])
				i++
			case node.isMultiline || strings.HasPrefix(ct, "- "):
				node.listItems = append(node.listItems, ct)
				i++
			case strings.HasPrefix(ct, "#") || strings.HasPrefix(ct, "//"):
				i++
			default:
				subs, ni := fmtParse(lines, i, ci, depth+1)
				node.children = append(node.children, subs...)
				i = ni
			}
		}
		nodes = append(nodes, node)
	}
	return nodes, i
}

// fmtSortKey is the header's key, lowercased the way Rust's
// `str::to_lowercase` does.
func fmtSortKey(header string) string {
	end := len(header)
	for i, r := range header {
		if r == '[' || r == ':' || r == '(' || unicode.IsSpace(r) {
			end = i
			break
		}
	}
	return rustToLower(header[:end])
}

// rustToLower is `str::to_lowercase`: full lowercase mapping, including
// U+0130 → "i̇" and a word-final capital sigma → "ς".
func rustToLower(s string) string {
	ascii := true
	for i := 0; i < len(s); i++ {
		if s[i] >= utf8.RuneSelf {
			ascii = false
			break
		}
	}
	if ascii {
		return strings.ToLower(s)
	}
	var sb strings.Builder
	sb.Grow(len(s))
	for i, r := range s {
		switch r {
		case 'İ':
			sb.WriteString("i̇")
		case 'Σ':
			if finalSigma(s, i) {
				sb.WriteRune('ς')
			} else {
				sb.WriteRune('σ')
			}
		default:
			sb.WriteRune(unicode.ToLower(r))
		}
	}
	return sb.String()
}

// finalSigma is the Final_Sigma condition: a cased letter before (skipping
// case-ignorable characters) and none after.
func finalSigma(s string, at int) bool {
	before := false
	for j := at; j > 0; {
		r, size := utf8.DecodeLastRuneInString(s[:j])
		j -= size
		if isCaseIgnorable(r) {
			continue
		}
		before = isCased(r)
		break
	}
	if !before {
		return false
	}
	for j := at + len("Σ"); j < len(s); {
		r, size := utf8.DecodeRuneInString(s[j:])
		j += size
		if isCaseIgnorable(r) {
			continue
		}
		return !isCased(r)
	}
	return true
}

func isCased(r rune) bool {
	return unicode.IsUpper(r) || unicode.IsLower(r) || unicode.IsTitle(r) ||
		unicode.Is(unicode.Other_Lowercase, r) || unicode.Is(unicode.Other_Uppercase, r)
}

func isCaseIgnorable(r rune) bool {
	switch r {
	case '\'', '.', ':', '^', '`', '­', '·', '‘', '’', '․', '‧':
		return true
	}
	return unicode.In(r, unicode.Mn, unicode.Me, unicode.Cf, unicode.Lm, unicode.Sk)
}

func fmtSort(nodes []fmtNode) {
	for i := range nodes {
		nodes[i].sortKey = fmtSortKey(nodes[i].header)
	}
	sort.SliceStable(nodes, func(a, b int) bool { return nodes[a].sortKey < nodes[b].sortKey })
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
