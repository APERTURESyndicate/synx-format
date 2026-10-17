package synx

import (
	"math"
	"math/rand"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

// Resource caps (parity with crates/synx-core/src/parser.rs).
const (
	MaxInputBytes     = 16 * 1024 * 1024
	MaxLineStarts     = 2_000_000
	MaxNestingDepth   = 128
	MaxMultilineBytes = 1024 * 1024
	MaxListItems      = 1 << 20
	MaxIncludes       = 4096
	MaxEnumParts      = 4096
	MaxMarkerSegments = 512
)

// ClampText truncates `text` to a UTF-8-safe prefix bounded by MaxInputBytes.
//
// A single leading U+FEFF byte-order mark is dropped first (SYNX §2.1). It is
// not whitespace, so left in place it would glue itself to the first key and
// silently turn a BOM-prefixed `!active` into a plain key.
func ClampText(text string) string {
	text = strings.TrimPrefix(text, "\uFEFF")
	if len(text) <= MaxInputBytes {
		return text
	}
	end := MaxInputBytes
	for end > 0 && !utf8.RuneStart(text[end]) {
		end--
	}
	return text[:end]
}

// parseEnd is the byte length to parse: the whole text, or the prefix before
// the newline that would exceed MaxLineStarts lines.
func parseEnd(text string) int {
	maxNewlines := MaxLineStarts - 1
	seen, scan := 0, 0
	for scan < len(text) {
		rel := strings.IndexByte(text[scan:], '\n')
		if rel < 0 {
			break
		}
		if seen >= maxNewlines {
			return scan + rel
		}
		seen++
		scan += rel + 1
	}
	return len(text)
}

// ─── internal types ─────────────────────────────────────────────────────────

type frameKind uint8

const (
	frameRoot frameKind = iota
	frameKey
	frameListItem
)

// stackFrame is one open object. obj is the object itself: objects are
// reference values, so a frame needs no re-navigation from the root, and a
// frame is popped before anything can replace its object in the parent.
type stackFrame struct {
	indent  int
	kind    frameKind
	key     string // frameKey: the key; frameListItem: the list's key
	itemIdx int    // frameListItem only
	parent  int    // frameListItem: stack index of the object holding the list
	obj     *Object
}

type blockState struct {
	indent  int
	key     string
	content strings.Builder
	parent  *Object
	// `|+` (SYNX 3.7) keeps each continuation line's indent relative to the
	// first one; baseIndent is locked on the first content line (-1 = unset).
	preserveIndent bool
	baseIndent     int
}

type listState struct {
	indent   int
	key      string
	stackIdx int
	parent   *Object
	pos      int // position of key in parent, -1 until known
}

// parsedLine is one key line; every string is a slice of the line.
type parsedLine struct {
	key         string
	typeHint    string
	hasTypeHint bool
	value       string
	markers     []string
	markerArgs  []string
	constraints *Constraints
}

type parser struct {
	text          string
	stack         []stackFrame
	metaPath      string
	metaPathValid bool
	result        ParseResult
}

func (p *parser) push(f stackFrame) {
	p.stack = append(p.stack, f)
	p.metaPathValid = false
}

func (p *parser) pop() {
	p.stack[len(p.stack)-1] = stackFrame{}
	p.stack = p.stack[:len(p.stack)-1]
	p.metaPathValid = false
}

// currentPath is the metadata path of the top frame. It follows the frame
// chain rather than the whole stack: a list item's parent is the object that
// holds the list, which need not be the frame below it.
func (p *parser) currentPath() string {
	if p.metaPathValid {
		return p.metaPath
	}
	p.metaPathValid = true
	if len(p.stack) <= 1 {
		p.metaPath = ""
		return ""
	}
	var chain [MaxNestingDepth + 1]int
	n := 0
	for idx := len(p.stack) - 1; idx > 0 && n < len(chain); {
		chain[n] = idx
		n++
		if p.stack[idx].kind == frameListItem {
			idx = p.stack[idx].parent
		} else {
			idx--
		}
	}
	var sb strings.Builder
	for i := n - 1; i >= 0; i-- {
		f := &p.stack[chain[i]]
		if sb.Len() > 0 {
			sb.WriteByte('.')
		}
		writeKeySegment(&sb, f.key)
		if f.kind == frameListItem {
			sb.WriteString(".#")
			sb.WriteString(strconv.Itoa(f.itemIdx))
		}
	}
	p.metaPath = sb.String()
	return p.metaPath
}

// Parse converts SYNX text into a ParseResult.
func Parse(text string) ParseResult {
	// A leading BOM is not input that a limit dropped.
	originalLen := len(strings.TrimPrefix(text, "\uFEFF"))
	text = ClampText(text)
	text = text[:parseEnd(text)]

	root := NewObjectMap()
	p := &parser{text: text}
	p.result = ParseResult{Root: Object_(root), Metadata: NewMetadataTree()}
	p.result.Truncated = len(text) < originalLen
	p.stack = make([]stackFrame, 1, 16)
	p.stack[0] = stackFrame{indent: -1, kind: frameRoot, obj: root}
	r := &p.result

	var block *blockState
	var list *listState
	inBlockComment := false

	pos := 0
	for pos <= len(text) {
		start := pos
		nl := strings.IndexByte(text[start:], '\n')
		if nl < 0 {
			nl = len(text)
		} else {
			nl += start
		}
		pos = nl + 1
		end := nl
		if end > start && text[end-1] == '\r' {
			end--
		}
		raw := text[start:end]
		trimmed := trimWS(raw)

		// A blank line is never structural and never body: SYNX §8.4.1 does not
		// preserve blank lines inside a multiline body.
		if trimmed == "" {
			continue
		}
		indent := indentLen(raw)

		// Continue a multiline block. This runs before every line-class rule
		// below: a `|` / `|+` body is raw text (SYNX §8.4.1), so `#`, `//`,
		// `###` and `!…` lines are ordinary content there.
		if block != nil {
			if indent > block.indent {
				if block.content.Len() < MaxMultilineBytes {
					if block.content.Len() > 0 {
						block.content.WriteByte('\n')
					}
					room := MaxMultilineBytes - block.content.Len()
					if room > 0 {
						var slice string
						if block.preserveIndent {
							if block.baseIndent < 0 {
								block.baseIndent = indent
							}
							strip := block.baseIndent
							if indent < strip {
								strip = indent
							}
							rawTrimEnd := trimRightWS(raw)
							if cut := stripBoundary(rawTrimEnd, strip); cut < len(rawTrimEnd) {
								slice = rawTrimEnd[cut:]
							}
						} else {
							slice = trimmed
						}
						n := utf8PrefixLen(slice, room)
						block.content.WriteString(slice[:n])
						if n < len(slice) {
							r.Truncated = true
						}
					}
				} else {
					r.Truncated = true
				}
				continue
			}
			block.parent.Set(block.key, String(block.content.String()))
			block = nil
		}

		if trimmed == "###" {
			inBlockComment = !inBlockComment
			continue
		}
		if inBlockComment {
			continue
		}

		// Directive lines (§6) all start with `!` or `#!mode:`.
		if trimmed[0] == '!' || strings.HasPrefix(trimmed, "#!mode:") {
			if p.directive(trimmed) {
				continue
			}
		}

		if trimmed[0] == '#' || strings.HasPrefix(trimmed, "//") {
			continue
		}

		if strings.HasPrefix(trimmed, "- ") {
			if list != nil && indent > list.indent {
				// Pop every frame of the previous item — its own frame and any
				// object nested inside it.
				for len(p.stack) > 1 && p.stack[len(p.stack)-1].indent >= indent {
					p.pop()
				}
				valStr := stripComment(trimWS(trimmed[2:]))

				// The item is an object when the next content line is deeper
				// and is neither another item nor a comment.
				nested := false
				if ps, pe, ok := nextContentLine(text, pos); ok {
					pl := text[ps:pe]
					pt := trimWS(pl)
					if indentLen(pl) > indent && !strings.HasPrefix(pt, "- ") &&
						pt[0] != '#' && !strings.HasPrefix(pt, "//") {
						nested = true
					}
				}

				parent := list.parent
				if list.pos < 0 {
					list.pos = parent.find(list.key)
					if list.pos < 0 {
						parent.Set(list.key, ArrayValue{V: []Value{}})
						list.pos = parent.find(list.key)
					}
				}
				av, isArr := parent.pairs[list.pos].Value.(ArrayValue)
				if !isArr || len(av.V) >= MaxListItems {
					continue
				}
				if nested {
					item := NewObjectMap()
					if pl, ok := parseLine(valStr); ok {
						var v Value
						switch {
						case pl.hasTypeHint:
							v = castTyped(pl.value, pl.typeHint)
						case pl.value != "":
							v = cast(pl.value)
						default:
							v = NewObject()
						}
						if !isReservedJSKey(pl.key) {
							item.Set(pl.key, v)
						}
					} else {
						item.Set("_value", cast(valStr))
					}
					itemIdx := len(av.V)
					av.V = append(av.V, Object_(item))
					parent.pairs[list.pos].Value = av
					if len(p.stack) < MaxNestingDepth {
						p.push(stackFrame{indent: indent, kind: frameListItem, key: list.key,
							itemIdx: itemIdx, parent: list.stackIdx, obj: item})
					}
				} else {
					av.V = append(av.V, cast(valStr))
					parent.pairs[list.pos].Value = av
				}
				continue
			}
		} else if list != nil && indent <= list.indent {
			// A non-item line at or below the list's indent closes it.
			list = nil
			for len(p.stack) > 1 {
				top := p.stack[len(p.stack)-1]
				if top.kind == frameListItem && top.indent >= indent {
					p.pop()
				} else {
					break
				}
			}
		}

		pl, ok := parseLine(trimmed)
		if !ok || isReservedJSKey(pl.key) {
			continue
		}
		for len(p.stack) > 1 && p.stack[len(p.stack)-1].indent >= indent {
			p.pop()
		}
		parentIdx := len(p.stack) - 1
		parent := p.stack[parentIdx].obj

		// A key that replaces an object or list replaces its declarations too.
		if r.Mode == ModeActive && r.Metadata.Len() > 0 {
			if old, exists := parent.Get(pl.key); exists && isContainer(old) {
				prefix := p.currentPath()
				var sb strings.Builder
				sb.WriteString(prefix)
				if prefix != "" {
					sb.WriteByte('.')
				}
				writeKeySegment(&sb, pl.key)
				pfx := sb.String()
				r.Metadata.retain(func(path string) bool {
					return !(path == pfx || (len(path) > len(pfx) && strings.HasPrefix(path, pfx) && path[len(pfx)] == '.'))
				})
			}
		}

		if r.Mode == ModeActive && (len(pl.markers) > 0 || pl.constraints != nil || pl.hasTypeHint) {
			r.Metadata.level(p.currentPath()).Set(pl.key, Meta{
				Markers:     pl.markers,
				Args:        pl.markerArgs,
				TypeHint:    pl.typeHint,
				Constraints: pl.constraints,
				typeHintSet: pl.hasTypeHint,
			})
		}

		isBlock := pl.value == "|" || pl.value == "|+"
		isListMarker := false
		for _, m := range pl.markers {
			if m == "random" || m == "unique" || m == "geo" || m == "join" {
				isListMarker = true
				break
			}
		}

		switch {
		case isBlock:
			parent.Set(pl.key, String(""))
			block = &blockState{indent: indent, key: pl.key, parent: parent,
				preserveIndent: pl.value == "|+", baseIndent: -1}
		case isListMarker && pl.value == "":
			parent.Set(pl.key, ArrayValue{V: []Value{}})
			list = &listState{indent: indent, key: pl.key, stackIdx: parentIdx, parent: parent, pos: -1}
		case pl.value == "":
			if ps, pe, ok := nextContentLine(text, pos); ok && strings.HasPrefix(trimWS(text[ps:pe]), "- ") {
				parent.Set(pl.key, ArrayValue{V: []Value{}})
				list = &listState{indent: indent, key: pl.key, stackIdx: parentIdx, parent: parent, pos: -1}
				continue
			}
			child := NewObjectMap()
			parent.Set(pl.key, Object_(child))
			if len(p.stack) < MaxNestingDepth {
				p.push(stackFrame{indent: indent, kind: frameKey, key: pl.key, obj: child})
			}
		default:
			var v Value
			if pl.hasTypeHint {
				v = castTyped(pl.value, pl.typeHint)
			} else {
				v = cast(pl.value)
			}
			parent.Set(pl.key, v)
		}
	}

	if block != nil {
		block.parent.Set(block.key, String(block.content.String()))
	}
	return p.result
}

// directive applies one directive line and reports whether it was one.
// Unknown `!…` lines fall through to the generic line handling.
func (p *parser) directive(t string) bool {
	r := &p.result
	switch t {
	case "!active":
		r.Mode = ModeActive
		return true
	case "!lock":
		r.Locked = true
		return true
	case "!tool":
		r.Tool = true
		return true
	case "!schema":
		r.Schema = true
		return true
	case "!llm":
		r.Llm = true
		return true
	}
	if strings.HasPrefix(t, "!include ") {
		if len(r.Includes) < MaxIncludes {
			rest := strings.TrimSpace(t[9:])
			path, alias, hasAlias := rest, "", false
			if ws := strings.IndexFunc(rest, unicode.IsSpace); ws >= 0 {
				path = rest[:ws]
				_, size := utf8.DecodeRuneInString(rest[ws:])
				alias = strings.TrimSpace(rest[ws+size:])
				hasAlias = true
			}
			if !hasAlias {
				name := path
				if slash := strings.LastIndexAny(name, "/\\"); slash >= 0 {
					name = name[slash+1:]
				}
				if strings.HasSuffix(name, ".synx") {
					name = name[:len(name)-5]
				} else if strings.HasSuffix(name, ".SYNX") {
					name = name[:len(name)-5]
				}
				alias = name
			}
			r.Includes = append(r.Includes, IncludeDirective{Path: path, Alias: alias})
		}
		return true
	}
	if strings.HasPrefix(t, "!use ") {
		rest := strings.TrimSpace(t[5:])
		if strings.HasPrefix(rest, "@") {
			pkg, alias := rest, ""
			hasAlias := false
			if idx := strings.Index(rest, " as "); idx >= 0 {
				pkg = rest[:idx]
				alias = strings.TrimSpace(rest[idx+4:])
				hasAlias = true
			}
			pkg = strings.TrimSpace(pkg)
			if !hasAlias {
				alias = pkg
				if slash := strings.LastIndexByte(pkg, '/'); slash >= 0 {
					alias = pkg[slash+1:]
				}
			}
			if pkg != "" {
				r.Uses = append(r.Uses, UseDirective{Package: pkg, Alias: alias})
			}
		}
		return true
	}
	if strings.HasPrefix(t, "#!mode:") {
		if strings.TrimSpace(t[7:]) == "active" {
			r.Mode = ModeActive
		} else {
			r.Mode = ModeStatic
		}
		return true
	}
	return false
}

// nextContentLine finds the first non-blank line at or after byte `from`,
// as (start, end) with a trailing CR excluded.
func nextContentLine(text string, from int) (int, int, bool) {
	p := from
	for p <= len(text) {
		nl := strings.IndexByte(text[p:], '\n')
		if nl < 0 {
			nl = len(text)
		} else {
			nl += p
		}
		end := nl
		if end > p && text[end-1] == '\r' {
			end--
		}
		if trimWS(text[p:end]) != "" {
			return p, end, true
		}
		p = nl + 1
	}
	return 0, 0, false
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
			def.Set("params", CloneValue(v))
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
		params = CloneValue(firstVal)
	} else {
		params = NewObject()
	}
	out := NewObjectMap()
	out.Set("tool", String(firstKey))
	out.Set("params", params)
	return Object_(out)
}

// ─── line decomposer ────────────────────────────────────────────────────────

func parseLine(trimmed string) (parsedLine, bool) {
	var out parsedLine
	if trimmed == "" || trimmed[0] == '#' || strings.HasPrefix(trimmed, "//") || strings.HasPrefix(trimmed, "- ") {
		return out, false
	}
	switch trimmed[0] {
	case '[', ':', '-', '/', '(':
		return out, false
	}
	n := len(trimmed)
	pos := 0
	for pos < n {
		ch := trimmed[pos]
		if ch == ' ' || ch == '\t' || ch == '[' || ch == ':' || ch == '(' {
			break
		}
		pos++
	}
	out.key = trimmed[:pos]

	if pos < n && trimmed[pos] == '(' {
		start := pos + 1
		if c := strings.IndexByte(trimmed[start:], ')'); c >= 0 {
			out.typeHint = trimmed[start : start+c]
			out.hasTypeHint = true
			pos = start + c + 1
		} else {
			pos++
		}
	}

	// Balanced scan so `^[A-Z]{2}$` does not close the block at its own `]`.
	if pos < n && trimmed[pos] == '[' {
		cstart := pos + 1
		depth := 1
		scan := cstart
		for scan < n && depth > 0 {
			switch trimmed[scan] {
			case '[':
				depth++
			case ']':
				depth--
			}
			if depth == 0 {
				break
			}
			scan++
		}
		var c Constraints
		if depth == 0 {
			c = parseConstraints(trimmed[cstart:scan])
			pos = scan + 1
		} else if rel := strings.IndexByte(trimmed[cstart:], ']'); rel >= 0 {
			c = parseConstraints(trimmed[cstart : cstart+rel])
			pos = cstart + rel + 1
		} else {
			c = parseConstraints(trimmed[cstart:])
			pos = n
		}
		out.constraints = &c
	}

	if pos < n && trimmed[pos] == ':' {
		mstart := pos + 1
		mend := mstart
		for mend < n && trimmed[mend] != ' ' && trimmed[mend] != '\t' {
			mend++
		}
		chain := trimmed[mstart:mend]
		count := strings.Count(chain, ":") + 1
		if count > MaxMarkerSegments {
			count = MaxMarkerSegments
		}
		out.markers = make([]string, 0, count)
		for len(out.markers) < MaxMarkerSegments {
			i := strings.IndexByte(chain, ':')
			if i < 0 {
				out.markers = append(out.markers, chain)
				break
			}
			out.markers = append(out.markers, chain[:i])
			chain = chain[i+1:]
		}
		pos = mend
	}

	for pos < n && (trimmed[pos] == ' ' || trimmed[pos] == '\t') {
		pos++
	}
	if pos < n {
		out.value = stripComment(trimmed[pos:])
	}

	if len(out.markers) > 0 && out.value != "" {
		// `:random` reads its weights from the value.
		if containsString(out.markers, "random") {
			var nums []string
			for _, tok := range strings.FieldsFunc(out.value, unicode.IsSpace) {
				if _, ok := parseRustFloat(tok); ok {
					nums = append(nums, tok)
				}
			}
			if len(nums) > 0 {
				out.markerArgs = nums
				out.value = ""
			}
		}
		// `:inherit` with a value names the parent instead of a scalar.
		if out.value != "" && containsString(out.markers, "inherit") {
			out.markerArgs = []string{strings.TrimSpace(out.value)}
			out.value = ""
		}
	}
	return out, true
}

// parseConstraints parses the body of a `[…]` block.
func parseConstraints(raw string) Constraints {
	var c Constraints
	for raw != "" {
		var part string
		if i := strings.IndexByte(raw, ','); i >= 0 {
			part, raw = raw[:i], raw[i+1:]
		} else {
			part, raw = raw, ""
		}
		part = strings.TrimSpace(part)
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
			c.Min = nil
			if d, ok := parseRustFloat(v); ok {
				c.Min = &d
			}
		case "max":
			c.Max = nil
			if d, ok := parseRustFloat(v); ok {
				c.Max = &d
			}
		case "type":
			c.TypeName = v
			c.typeNameSet = true
		case "pattern":
			c.Pattern = v
			c.patternSet = true
		case "enum":
			vals := make([]string, 0, strings.Count(v, "|")+1)
			for len(vals) < MaxEnumParts {
				i := strings.IndexByte(v, '|')
				if i < 0 {
					vals = append(vals, v)
					break
				}
				vals = append(vals, v[:i])
				v = v[i+1:]
			}
			c.EnumValues = vals
		}
	}
	return c
}

// ─── casting ────────────────────────────────────────────────────────────────

// cast is SYNX §8.3 automatic casting.
func cast(val string) Value {
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
	case "":
		return String("")
	}
	start := 0
	if val[0] == '-' {
		if len(val) == 1 {
			return String(val)
		}
		start = 1
	}
	if val[start] >= '0' && val[start] <= '9' {
		dotPos := -1
		allNumeric := true
		for j := start; j < len(val); j++ {
			c := val[j]
			if c == '.' {
				if dotPos >= 0 {
					allNumeric = false
					break
				}
				dotPos = j
			} else if c < '0' || c > '9' {
				allNumeric = false
				break
			}
		}
		if allNumeric {
			if dotPos >= 0 {
				if dotPos > start && dotPos < len(val)-1 {
					if d, ok := parseRustFloat(val); ok {
						return Float(d)
					}
				}
			} else if n, err := strconv.ParseInt(val, 10, 64); err == nil {
				return Int(n)
			}
		}
	}
	return String(val)
}

// castTyped is SYNX §8.3 typed casting.
func castTyped(val, hint string) Value {
	switch hint {
	case "int":
		n, err := strconv.ParseInt(val, 10, 64)
		if err != nil {
			n = 0
		}
		return Int(n)
	case "float":
		f, ok := parseRustFloat(val)
		if !ok {
			f = 0
		}
		return Float(f)
	case "bool":
		return Bool(strings.TrimSpace(val) == "true")
	case "string":
		return String(val)
	case "random", "random:int":
		return Int(rand.Int63n(2147483647))
	case "random:float":
		return Float(float64(rand.Int63n(10000)) / 10000)
	case "random:bool":
		return Bool(rand.Intn(2) == 0)
	}
	return cast(val)
}

// parseRustFloat reads a float with Rust's `str::parse::<f64>` grammar:
// optional sign, then digits with an optional fraction and exponent, or
// `inf` / `infinity` / `nan` in any case. Go's ParseFloat also takes hex
// floats and underscores, which Rust rejects; out-of-range values are ±inf
// (or zero), not errors.
func parseRustFloat(s string) (float64, bool) {
	if s == "" {
		return 0, false
	}
	i := 0
	if s[0] == '+' || s[0] == '-' {
		i = 1
	}
	if i >= len(s) {
		return 0, false
	}
	body := s[i:]
	switch strings.ToLower(body) {
	case "inf", "infinity":
		if s[0] == '-' {
			return math.Inf(-1), true
		}
		return math.Inf(1), true
	case "nan":
		return math.NaN(), true
	}
	digits := 0
	j := 0
	for j < len(body) && body[j] >= '0' && body[j] <= '9' {
		j++
		digits++
	}
	if j < len(body) && body[j] == '.' {
		j++
		for j < len(body) && body[j] >= '0' && body[j] <= '9' {
			j++
			digits++
		}
	}
	if digits == 0 {
		return 0, false
	}
	if j < len(body) && (body[j] == 'e' || body[j] == 'E') {
		j++
		if j < len(body) && (body[j] == '+' || body[j] == '-') {
			j++
		}
		expDigits := 0
		for j < len(body) && body[j] >= '0' && body[j] <= '9' {
			j++
			expDigits++
		}
		if expDigits == 0 {
			return 0, false
		}
	}
	if j != len(body) {
		return 0, false
	}
	f, err := strconv.ParseFloat(s, 64)
	if err != nil {
		if ne, ok := err.(*strconv.NumError); !ok || ne.Err != strconv.ErrRange {
			return 0, false
		}
	}
	return f, true
}

// parseRustUint reads an unsigned integer like Rust's `str::parse::<u64>`,
// which accepts one leading `+`.
func parseRustUint(s string) (uint64, bool) {
	if strings.HasPrefix(s, "+") {
		s = s[1:]
		if s == "" || s[0] == '+' || s[0] == '-' {
			return 0, false
		}
	}
	n, err := strconv.ParseUint(s, 10, 64)
	return n, err == nil
}

// ─── helpers ────────────────────────────────────────────────────────────────

func isReservedJSKey(key string) bool {
	return key == "__proto__" || key == "constructor" || key == "prototype"
}

func isContainer(v Value) bool {
	switch v.(type) {
	case ObjectValue, ArrayValue:
		return true
	}
	return false
}

// isASCIIWS is SYNX whitespace inside the ASCII range — what Unicode
// White_Space accepts below U+0080.
func isASCIIWS(b byte) bool {
	return b == ' ' || (b >= '\t' && b <= '\r')
}

// trimWS trims Unicode whitespace from both ends, with an ASCII fast path.
func trimWS(s string) string {
	start := 0
	for start < len(s) && isASCIIWS(s[start]) {
		start++
	}
	if start < len(s) && s[start] >= utf8.RuneSelf {
		return strings.TrimFunc(s[start:], unicode.IsSpace)
	}
	end := len(s)
	for end > start && isASCIIWS(s[end-1]) {
		end--
	}
	if end > start && s[end-1] >= utf8.RuneSelf {
		return strings.TrimRightFunc(s[start:end], unicode.IsSpace)
	}
	return s[start:end]
}

// trimRightWS trims trailing Unicode whitespace.
func trimRightWS(s string) string {
	end := len(s)
	for end > 0 && isASCIIWS(s[end-1]) {
		end--
	}
	if end > 0 && s[end-1] >= utf8.RuneSelf {
		return strings.TrimRightFunc(s[:end], unicode.IsSpace)
	}
	return s[:end]
}

// indentLen is the UTF-8 byte length of the leading Unicode whitespace
// (SYNX §4): U+00A0 counts 2, U+3000 counts 3, a tab or a space 1.
func indentLen(s string) int {
	i := 0
	for i < len(s) && isASCIIWS(s[i]) {
		i++
	}
	if i < len(s) && s[i] >= utf8.RuneSelf {
		return len(s) - len(strings.TrimLeftFunc(s, unicode.IsSpace))
	}
	return i
}

// stripBoundary returns the byte offset of the first rune at or past `strip`
// bytes of leading whitespace; it never lands inside a multi-byte rune.
func stripBoundary(s string, strip int) int {
	off := 0
	for _, r := range s {
		if off >= strip || !unicode.IsSpace(r) {
			break
		}
		off += utf8.RuneLen(r)
	}
	return off
}

// utf8PrefixLen is the longest prefix of s that fits in limit bytes without
// splitting a rune.
func utf8PrefixLen(s string, limit int) int {
	if limit < 0 {
		limit = 0
	}
	if len(s) <= limit {
		return len(s)
	}
	end := limit
	for end > 0 && !utf8.RuneStart(s[end]) {
		end--
	}
	return end
}

// stripComment cuts an inline ` //` or ` #` comment and trailing whitespace.
func stripComment(val string) string {
	if strings.IndexByte(val, ' ') >= 0 {
		if p := strings.Index(val, " //"); p >= 0 {
			val = val[:p]
		}
		if p := strings.Index(val, " #"); p >= 0 {
			val = val[:p]
		}
	}
	return trimRightWS(val)
}

func containsString(arr []string, s string) bool {
	for _, x := range arr {
		if x == s {
			return true
		}
	}
	return false
}

// writeKeySegment appends `key` to a metadata path as one segment: `.` and
// `\` inside the key are escaped with `\`, so the object `a.b` and the object
// `b` inside `a` never share a path.
func writeKeySegment(sb *strings.Builder, key string) {
	if strings.IndexByte(key, '.') < 0 && strings.IndexByte(key, '\\') < 0 {
		sb.WriteString(key)
		return
	}
	for i := 0; i < len(key); i++ {
		if key[i] == '.' || key[i] == '\\' {
			sb.WriteByte('\\')
		}
		sb.WriteByte(key[i])
	}
}

// keySegment is `key` as one metadata-path segment (see writeKeySegment).
func keySegment(key string) string {
	if strings.IndexByte(key, '.') < 0 && strings.IndexByte(key, '\\') < 0 {
		return key
	}
	var sb strings.Builder
	writeKeySegment(&sb, key)
	return sb.String()
}

// MetaPathSegments splits a metadata path into its unescaped segments:
// `a\.b.c` is ["a.b", "c"].
func MetaPathSegments(path string) []string {
	if path == "" {
		return nil
	}
	var out []string
	var seg strings.Builder
	for i := 0; i < len(path); i++ {
		switch path[i] {
		case '\\':
			if i+1 < len(path) {
				i++
				seg.WriteByte(path[i])
			}
		case '.':
			out = append(out, seg.String())
			seg.Reset()
		default:
			seg.WriteByte(path[i])
		}
	}
	return append(out, seg.String())
}
