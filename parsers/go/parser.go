package synx

import (
	"bytes"
	"math/rand"
	"strconv"
	"strings"
	"unicode"
)

// Resource caps (parity with crates/synx-core/src/parser.rs).
const (
	MaxInputBytes      = 16 * 1024 * 1024
	MaxLineStarts      = 2_000_000
	MaxNestingDepth    = 128
	MaxMultilineBytes  = 1024 * 1024
	MaxListItems       = 1 << 20
	MaxIncludes        = 4096
	MaxEnumParts       = 4096
	MaxMarkerSegments  = 512
)

// ClampText truncates `text` to a UTF-8-safe prefix bounded by MaxInputBytes.
func ClampText(text string) string {
	b := []byte(text)
	if len(b) <= MaxInputBytes {
		return text
	}
	end := MaxInputBytes
	for end > 0 && b[end]&0xC0 == 0x80 {
		end--
	}
	return string(b[:end])
}

// Parse converts SYNX text into a ParseResult.
func Parse(text string) ParseResult {
	b := []byte(ClampText(text))

	// Bound number of indexed newlines.
	maxNl := MaxLineStarts - 1
	{
		seen, scan := 0, 0
		for scan < len(b) {
			if b[scan] == '\n' {
				if seen >= maxNl {
					b = b[:scan]
					break
				}
				seen++
			}
			scan++
		}
	}

	// Index line starts.
	lineStarts := make([]int, 1, 64)
	lineStarts[0] = 0
	for scan := 0; scan < len(b); scan++ {
		if b[scan] == '\n' {
			lineStarts = append(lineStarts, scan+1)
		}
	}
	lineCount := len(lineStarts)

	result := ParseResult{
		Root:     NewObject(),
		Metadata: MetadataTree{},
	}
	rootObj, _ := AsObject(result.Root)
	stack := []stackFrame{{indent: -1, kind: seRoot}}

	var block *blockState
	var list *listState
	inBlockComment := false

	i := 0
	for i < lineCount {
		raw := lineBytes(b, lineStarts, i)
		rawStr := string(raw)
		t := strings.TrimSpace(rawStr)

		// Directives
		switch t {
		case "!active":
			result.Mode = ModeActive
			i++
			continue
		case "!lock":
			result.Locked = true
			i++
			continue
		case "!tool":
			result.Tool = true
			i++
			continue
		case "!schema":
			result.Schema = true
			i++
			continue
		case "!llm":
			result.Llm = true
			i++
			continue
		}
		if strings.HasPrefix(t, "!include ") {
			if len(result.Includes) < MaxIncludes {
				rest := strings.TrimSpace(t[9:])
				path, alias := rest, ""
				if ws := firstWhitespace(rest); ws >= 0 {
					path = rest[:ws]
					alias = strings.TrimSpace(rest[ws:])
				}
				if alias == "" {
					base := path
					if slash := strings.LastIndexAny(base, "/\\"); slash >= 0 {
						base = base[slash+1:]
					}
					if strings.HasSuffix(base, ".synx") || strings.HasSuffix(base, ".SYNX") {
						base = base[:len(base)-5]
					}
					alias = base
				}
				result.Includes = append(result.Includes, IncludeDirective{Path: path, Alias: alias})
			}
			i++
			continue
		}
		if strings.HasPrefix(t, "!use ") {
			rest := strings.TrimSpace(t[5:])
			if len(rest) > 0 && rest[0] == '@' {
				pkg, alias := rest, ""
				if idx := strings.Index(rest, " as "); idx >= 0 {
					pkg = strings.TrimSpace(rest[:idx])
					alias = strings.TrimSpace(rest[idx+4:])
				}
				if alias == "" {
					if slash := strings.LastIndex(pkg, "/"); slash >= 0 {
						alias = pkg[slash+1:]
					} else {
						alias = pkg
					}
				}
				if pkg != "" {
					result.Uses = append(result.Uses, UseDirective{Package: pkg, Alias: alias})
				}
			}
			i++
			continue
		}
		if strings.HasPrefix(t, "#!mode:") {
			declared := strings.TrimSpace(t[7:])
			if declared == "active" {
				result.Mode = ModeActive
			} else {
				result.Mode = ModeStatic
			}
			i++
			continue
		}

		if t == "###" {
			inBlockComment = !inBlockComment
			i++
			continue
		}
		if inBlockComment {
			i++
			continue
		}
		if t == "" || t[0] == '#' || strings.HasPrefix(t, "//") {
			i++
			continue
		}

		indent := indentOf(raw)

		// Continue multiline block
		if block != nil {
			if indent > block.indent {
				if block.content.Len() < MaxMultilineBytes {
					if block.content.Len() > 0 {
						block.content.WriteByte('\n')
					}
					room := MaxMultilineBytes - block.content.Len()
					if room < len(t) {
						block.content.WriteString(t[:room])
					} else {
						block.content.WriteString(t)
					}
				}
				i++
				continue
			}
			insertValue(rootObj, stack, block.stackIdx, block.key, String(block.content.String()))
			block = nil
		}

		// List items
		if strings.HasPrefix(t, "- ") {
			if list != nil && indent > list.indent {
				for len(stack) > 1 {
					back := stack[len(stack)-1]
					if back.kind == seListItem && back.indent >= indent {
						stack = stack[:len(stack)-1]
					} else {
						break
					}
				}
				valStr := stripComment(strings.TrimSpace(t[2:]))
				nested := false
				for peek := i + 1; peek < lineCount; peek++ {
					pl := lineBytes(b, lineStarts, peek)
					pt := strings.TrimSpace(string(pl))
					if pt == "" {
						continue
					}
					pi := indentOf(pl)
					if pi > indent && !strings.HasPrefix(pt, "- ") &&
						!strings.HasPrefix(pt, "#") && !strings.HasPrefix(pt, "//") {
						nested = true
					}
					break
				}

				listKey := list.key
				listStackIdx := list.stackIdx
				newIdx := -1
				mutateArray(rootObj, stack, listStackIdx, listKey, func(arr *[]Value) {
					if len(*arr) >= MaxListItems {
						return
					}
					if nested {
						item := NewObjectMap()
						if p := parseLine(valStr); p != nil {
							var v Value
							if p.typeHint != "" {
								v = castTyped(p.value, p.typeHint)
							} else if p.value == "" {
								v = NewObject()
							} else {
								v = castValue(p.value)
							}
							item.Set(p.key, v)
						} else {
							item.Set("_value", castValue(valStr))
						}
						newIdx = len(*arr)
						*arr = append(*arr, Object_(item))
					} else {
						*arr = append(*arr, castValue(valStr))
					}
				})
				if newIdx >= 0 && len(stack) < MaxNestingDepth {
					stack = append(stack, stackFrame{indent: indent, kind: seListItem, key: listKey, itemIdx: newIdx})
				}
				i++
				continue
			}
		} else if list != nil && indent <= list.indent {
			list = nil
			for len(stack) > 1 {
				back := stack[len(stack)-1]
				if back.kind == seListItem && back.indent >= indent {
					stack = stack[:len(stack)-1]
				} else {
					break
				}
			}
		}

		// Key line
		p := parseLine(t)
		if p == nil {
			i++
			continue
		}
		if p.key == "__proto__" || p.key == "constructor" || p.key == "prototype" {
			i++
			continue
		}
		for len(stack) > 1 && stack[len(stack)-1].indent >= indent {
			stack = stack[:len(stack)-1]
		}
		parentIdx := len(stack) - 1

		if result.Mode == ModeActive && (len(p.markers) > 0 || p.constraints != nil || p.typeHint != "") {
			path := buildPath(stack)
			meta := Meta{
				Markers:     p.markers,
				Args:        p.markerArgs,
				TypeHint:    p.typeHint,
				Constraints: p.constraints,
			}
			if result.Metadata[path] == nil {
				result.Metadata[path] = MetaMap{}
			}
			result.Metadata[path][p.key] = meta
		}

		isBlock := p.value == "|"
		isListMarker := false
		for _, m := range p.markers {
			if m == "random" || m == "unique" || m == "geo" || m == "join" {
				isListMarker = true
				break
			}
		}

		switch {
		case isBlock:
			insertValue(rootObj, stack, parentIdx, p.key, String(""))
			block = &blockState{indent: indent, key: p.key, stackIdx: parentIdx}
		case isListMarker && p.value == "":
			insertValue(rootObj, stack, parentIdx, p.key, NewArray())
			list = &listState{indent: indent, key: p.key, stackIdx: parentIdx}
		case p.value == "":
			becameList := false
			for peek := i + 1; peek < lineCount; peek++ {
				pl := lineBytes(b, lineStarts, peek)
				pt := strings.TrimSpace(string(pl))
				if pt == "" {
					continue
				}
				if strings.HasPrefix(pt, "- ") {
					insertValue(rootObj, stack, parentIdx, p.key, NewArray())
					list = &listState{indent: indent, key: p.key, stackIdx: parentIdx}
					becameList = true
				}
				break
			}
			if !becameList {
				insertValue(rootObj, stack, parentIdx, p.key, NewObject())
				if len(stack) < MaxNestingDepth {
					stack = append(stack, stackFrame{indent: indent, kind: seKey, key: p.key})
				}
			}
		default:
			var v Value
			if p.typeHint != "" {
				v = castTyped(p.value, p.typeHint)
			} else {
				v = castValue(p.value)
			}
			insertValue(rootObj, stack, parentIdx, p.key, v)
		}
		i++
	}

	if block != nil {
		insertValue(rootObj, stack, block.stackIdx, block.key, String(block.content.String()))
	}

	return result
}

// ReshapeToolOutput reshapes the parsed tree for `!tool` mode.
func ReshapeToolOutput(root Value, schema bool) Value {
	obj, ok := AsObject(root)
	if !ok {
		return root
	}
	if schema {
		keys := obj.SortedKeys()
		tools := make([]Value, 0, len(keys))
		for _, k := range keys {
			def := NewObjectMap()
			def.Set("name", String(k))
			v, _ := obj.Get(k)
			if v == nil {
				v = Null
			}
			def.Set("params", v)
			tools = append(tools, Object_(def))
		}
		out := NewObjectMap()
		out.Set("tools", Array(tools))
		return Object_(out)
	}
	if obj.IsEmpty() {
		out := NewObjectMap()
		out.Set("tool", Null)
		out.Set("params", NewObject())
		return Object_(out)
	}
	keys := obj.SortedKeys()
	firstKey := keys[0]
	firstVal, _ := obj.Get(firstKey)
	var params Value
	if _, ok := firstVal.(ObjectValue); ok {
		params = firstVal
	} else {
		params = NewObject()
	}
	out := NewObjectMap()
	out.Set("tool", String(firstKey))
	out.Set("params", params)
	return Object_(out)
}

// ─── internal types ─────────────────────────────────────────────────────────

type stackEntryKind uint8

const (
	seRoot stackEntryKind = iota
	seKey
	seListItem
)

type stackFrame struct {
	indent  int
	kind    stackEntryKind
	key     string // for seKey and seListItem.listKey
	itemIdx int    // for seListItem only
}

type blockState struct {
	indent   int
	key      string
	content  bytes.Buffer
	stackIdx int
}

type listState struct {
	indent   int
	key      string
	stackIdx int
}

type parsedLine struct {
	key         string
	typeHint    string
	value       string
	markers     []string
	markerArgs  []string
	constraints *Constraints
}

// ─── line decomposer ────────────────────────────────────────────────────────

func parseLine(trimmed string) *parsedLine {
	if trimmed == "" {
		return nil
	}
	if trimmed[0] == '#' || strings.HasPrefix(trimmed, "//") || strings.HasPrefix(trimmed, "- ") {
		return nil
	}
	first := trimmed[0]
	if first == '[' || first == ':' || first == '-' || first == '/' || first == '(' {
		return nil
	}
	b := []byte(trimmed)
	pos := 0
	for pos < len(b) {
		ch := b[pos]
		if ch == ' ' || ch == '\t' || ch == '[' || ch == ':' || ch == '(' {
			break
		}
		pos++
	}
	out := &parsedLine{key: string(b[:pos])}

	if pos < len(b) && b[pos] == '(' {
		start := pos + 1
		scan := start
		for scan < len(b) && b[scan] != ')' {
			scan++
		}
		if scan < len(b) {
			out.typeHint = string(b[start:scan])
			pos = scan + 1
		} else {
			pos = start
		}
	}

	if pos < len(b) && b[pos] == '[' {
		cstart := pos + 1
		depth := 1
		scan := cstart
		for scan < len(b) && depth > 0 {
			switch b[scan] {
			case '[':
				depth++
			case ']':
				depth--
				if depth == 0 {
				}
			}
			if depth == 0 {
				break
			}
			scan++
		}
		if depth == 0 {
			c := parseConstraints(string(b[cstart:scan]))
			out.constraints = &c
			pos = scan + 1
		} else {
			sweep := cstart
			for sweep < len(b) && b[sweep] != ']' {
				sweep++
			}
			if sweep < len(b) {
				c := parseConstraints(string(b[cstart:sweep]))
				out.constraints = &c
				pos = sweep + 1
			} else {
				c := parseConstraints(string(b[cstart:]))
				out.constraints = &c
				pos = len(b)
			}
		}
	}

	if pos < len(b) && b[pos] == ':' {
		mstart := pos + 1
		mend := mstart
		for mend < len(b) && b[mend] != ' ' && b[mend] != '\t' {
			mend++
		}
		chain := string(b[mstart:mend])
		segs := 0
		for _, seg := range splitN(chain, ':', MaxMarkerSegments) {
			out.markers = append(out.markers, seg)
			segs++
			if segs >= MaxMarkerSegments {
				break
			}
		}
		pos = mend
	}

	for pos < len(b) && (b[pos] == ' ' || b[pos] == '\t') {
		pos++
	}
	value := ""
	if pos < len(b) {
		value = stripComment(string(b[pos:]))
	}
	out.value = value

	if containsString(out.markers, "random") && out.value != "" {
		var nums []string
		for _, tok := range strings.Fields(out.value) {
			if _, err := strconv.ParseFloat(tok, 64); err == nil {
				nums = append(nums, tok)
			}
		}
		if len(nums) > 0 {
			out.markerArgs = nums
			out.value = ""
		}
	}
	if containsString(out.markers, "inherit") && out.value != "" {
		out.markerArgs = []string{strings.TrimSpace(out.value)}
		out.value = ""
	}
	return out
}

func parseConstraints(raw string) Constraints {
	var c Constraints
	for _, rawPart := range strings.Split(raw, ",") {
		part := strings.TrimSpace(rawPart)
		if part == "" {
			continue
		}
		if part == "required" {
			c.Required = true
			continue
		}
		if part == "readonly" {
			c.Readonly = true
			continue
		}
		colon := strings.IndexByte(part, ':')
		if colon < 0 {
			continue
		}
		k := strings.TrimSpace(part[:colon])
		v := strings.TrimSpace(part[colon+1:])
		switch k {
		case "min":
			if d, err := strconv.ParseFloat(v, 64); err == nil {
				c.Min = &d
			}
		case "max":
			if d, err := strconv.ParseFloat(v, 64); err == nil {
				c.Max = &d
			}
		case "type":
			c.TypeName = v
		case "pattern":
			c.Pattern = v
		case "enum":
			vals := []string{}
			count := 0
			for _, piece := range strings.Split(v, "|") {
				if count >= MaxEnumParts {
					break
				}
				vals = append(vals, strings.TrimSpace(piece))
				count++
			}
			c.EnumValues = vals
		}
	}
	return c
}

// ─── casting ────────────────────────────────────────────────────────────────

func castValue(val string) Value {
	if len(val) >= 2 {
		f, l := val[0], val[len(val)-1]
		if (f == '"' && l == '"') || (f == '\'' && l == '\'') {
			return String(val[1 : len(val)-1])
		}
	}
	switch val {
	case "true":
		return Bool(true)
	case "false":
		return Bool(false)
	case "null":
		return Null
	}
	if val == "" {
		return String("")
	}
	start := 0
	if val[0] == '-' {
		if len(val) == 1 {
			return String(val)
		}
		start = 1
	}
	if val[start] < '0' || val[start] > '9' {
		return String(val)
	}
	seenDot, allNumeric := false, true
	dotPos := -1
	for j := start; j < len(val); j++ {
		c := val[j]
		if c == '.' {
			if seenDot {
				allNumeric = false
				break
			}
			seenDot = true
			dotPos = j
		} else if c < '0' || c > '9' {
			allNumeric = false
			break
		}
	}
	if !allNumeric {
		return String(val)
	}
	if seenDot {
		if dotPos > start && dotPos < len(val)-1 {
			if d, err := strconv.ParseFloat(val, 64); err == nil {
				return Float(d)
			}
		}
		return String(val)
	}
	if n, err := strconv.ParseInt(val, 10, 64); err == nil {
		return Int(n)
	}
	return String(val)
}

func castTyped(val, hint string) Value {
	switch hint {
	case "int":
		n, _ := strconv.ParseInt(val, 10, 64)
		return Int(n)
	case "float":
		f, _ := strconv.ParseFloat(val, 64)
		return Float(f)
	case "bool":
		return Bool(strings.TrimSpace(val) == "true")
	case "string":
		return String(val)
	case "random", "random:int":
		return Int(rand.Int63())
	case "random:float":
		return Float(rand.Float64())
	case "random:bool":
		return Bool(rand.Intn(2) == 1)
	}
	return castValue(val)
}

// ─── helpers ────────────────────────────────────────────────────────────────

func lineBytes(b []byte, starts []int, i int) []byte {
	s := starts[i]
	var e int
	if i+1 < len(starts) {
		e = starts[i+1] - 1
	} else {
		e = len(b)
	}
	if e > s && b[e-1] == '\r' {
		e--
	}
	return b[s:e]
}

func indentOf(line []byte) int {
	i := 0
	for i < len(line) && (line[i] == ' ' || line[i] == '\t') {
		i++
	}
	return i
}

func firstWhitespace(s string) int {
	for i := 0; i < len(s); i++ {
		if s[i] == ' ' || s[i] == '\t' {
			return i
		}
	}
	return -1
}

func stripComment(val string) string {
	r := val
	if p := strings.Index(r, " //"); p >= 0 {
		r = r[:p]
	}
	if p := strings.Index(r, " #"); p >= 0 {
		r = r[:p]
	}
	for len(r) > 0 {
		last := r[len(r)-1]
		if last == ' ' || last == '\t' || last == '\r' {
			r = r[:len(r)-1]
			continue
		}
		break
	}
	return r
}

func splitN(s string, sep byte, max int) []string {
	out := make([]string, 0, 4)
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] == sep {
			out = append(out, s[start:i])
			start = i + 1
			if len(out) >= max {
				break
			}
		}
	}
	out = append(out, s[start:])
	return out
}

func containsString(arr []string, s string) bool {
	for _, x := range arr {
		if x == s {
			return true
		}
	}
	return false
}

// ─── path navigation (tree mutation helpers) ────────────────────────────────

func buildPath(stack []stackFrame) string {
	var parts []string
	for _, f := range stack[1:] {
		if f.kind == seKey {
			parts = append(parts, f.key)
		}
	}
	return strings.Join(parts, ".")
}

func insertValue(root *Object, stack []stackFrame, parentIdx int, key string, value Value) {
	if parentIdx == 0 {
		root.Set(key, value)
		return
	}
	path := stack[1 : parentIdx+1]
	setValueAtPath(root, path, key, value)
}

func setValueAtPath(obj *Object, path []stackFrame, key string, value Value) {
	if len(path) == 0 {
		obj.Set(key, value)
		return
	}
	head := path[0]
	rest := path[1:]
	switch head.kind {
	case seKey:
		v, ok := obj.Get(head.key)
		if !ok {
			return
		}
		child, ok := AsObject(v)
		if !ok {
			return
		}
		setValueAtPath(child, rest, key, value)
	case seListItem:
		v, ok := obj.Get(head.key)
		if !ok {
			return
		}
		arr, ok := AsArray(v)
		if !ok || head.itemIdx >= len(arr) {
			return
		}
		item, ok := AsObject(arr[head.itemIdx])
		if !ok {
			return
		}
		setValueAtPath(item, rest, key, value)
	}
}

func mutateArray(root *Object, stack []stackFrame, parentIdx int, listKey string, transform func(*[]Value)) {
	path := stack[1 : parentIdx+1]
	mutateArrayPath(root, path, listKey, transform)
}

func mutateArrayPath(obj *Object, path []stackFrame, listKey string, transform func(*[]Value)) {
	if len(path) == 0 {
		v, _ := obj.Get(listKey)
		arr, _ := AsArray(v)
		if arr == nil {
			arr = []Value{}
		}
		transform(&arr)
		obj.Set(listKey, Array(arr))
		return
	}
	head := path[0]
	rest := path[1:]
	switch head.kind {
	case seKey:
		v, ok := obj.Get(head.key)
		if !ok {
			return
		}
		child, ok := AsObject(v)
		if !ok {
			return
		}
		mutateArrayPath(child, rest, listKey, transform)
	case seListItem:
		v, ok := obj.Get(head.key)
		if !ok {
			return
		}
		arr, ok := AsArray(v)
		if !ok || head.itemIdx >= len(arr) {
			return
		}
		item, ok := AsObject(arr[head.itemIdx])
		if !ok {
			return
		}
		mutateArrayPath(item, rest, listKey, transform)
	}
}

// suppress "unused" for unicode import in some build configurations.
var _ = unicode.IsSpace
