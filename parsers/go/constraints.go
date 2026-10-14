package synx

import (
	"fmt"
	"os"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"
)

// Constraint and type failures are written into the value, not just logged:
// a failing field becomes `CONSTRAINT_ERR: ...` / `TYPE_ERR: ...` exactly as in
// crates/synx-core/src/engine.rs. With Options.Strict the message is also
// written to stderr.

const (
	constraintErrPrefix = "CONSTRAINT_ERR:"
	typeErrPrefix       = "TYPE_ERR:"
	maxPatternBytes     = 256
)

func (r *resolver) failField(obj *Object, key, msg string) {
	obj.Set(key, String(msg))
	if r.opts.Strict {
		fmt.Fprintln(os.Stderr, "synx: "+msg)
	}
}

// validateConstraints checks one key against `c`; the first failing check wins.
func (r *resolver) validateConstraints(obj *Object, key string, c *Constraints) {
	val, ok := obj.Get(key)
	if !ok {
		if c.Required {
			r.failField(obj, key, fmt.Sprintf("CONSTRAINT_ERR: '%s' is required", key))
		}
		return
	}
	if val == nil {
		val = Null
	}

	if c.Required {
		empty := false
		switch x := val.(type) {
		case NullValue:
			empty = true
		case StringValue:
			empty = x.V == ""
		}
		if empty {
			r.failField(obj, key, fmt.Sprintf("CONSTRAINT_ERR: '%s' is required", key))
			return
		}
	}

	if c.TypeName != "" {
		ok := true
		switch c.TypeName {
		case "int":
			_, ok = val.(IntValue)
		case "float":
			switch val.(type) {
			case FloatValue, IntValue:
			default:
				ok = false
			}
		case "bool":
			_, ok = val.(BoolValue)
		case "string":
			switch val.(type) {
			case StringValue, SecretValue:
			default:
				ok = false
			}
		}
		if !ok {
			r.failField(obj, key, fmt.Sprintf("CONSTRAINT_ERR: '%s' expected type '%s'", key, c.TypeName))
			return
		}
	}

	if c.EnumValues != nil {
		text := ""
		switch x := val.(type) {
		case StringValue:
			text = x.V
		case IntValue:
			text = strconv.FormatInt(x.V, 10)
		case FloatValue:
			text = formatRustNumber(x.V)
		case BoolValue:
			text = strconv.FormatBool(x.V)
		}
		found := false
		for _, e := range c.EnumValues {
			if e == text {
				found = true
				break
			}
		}
		if !found {
			r.failField(obj, key, fmt.Sprintf("CONSTRAINT_ERR: '%s' must be one of [%s]", key, strings.Join(c.EnumValues, "|")))
			return
		}
	}

	// Numbers compare by value; a string's size is its count of Unicode scalar values.
	var num float64
	hasNum := false
	switch x := val.(type) {
	case IntValue:
		num, hasNum = float64(x.V), true
	case FloatValue:
		num, hasNum = x.V, true
	case StringValue:
		if c.Min != nil || c.Max != nil {
			num, hasNum = float64(utf8.RuneCountInString(x.V)), true
		}
	}
	if hasNum {
		if c.Min != nil && num < *c.Min {
			r.failField(obj, key, fmt.Sprintf("CONSTRAINT_ERR: '%s' value %s is below min %s", key, formatRustNumber(num), formatRustNumber(*c.Min)))
			return
		}
		if c.Max != nil && num > *c.Max {
			r.failField(obj, key, fmt.Sprintf("CONSTRAINT_ERR: '%s' value %s exceeds max %s", key, formatRustNumber(num), formatRustNumber(*c.Max)))
			return
		}
	}

	// Over-long, invalid and nested-quantifier patterns are skipped silently.
	if c.Pattern != "" && len(c.Pattern) <= maxPatternBytes {
		if s, ok := val.(StringValue); ok && !hasNestedQuantifier(c.Pattern) {
			if re, err := regexp.Compile(c.Pattern); err == nil && !re.MatchString(s.V) {
				r.failField(obj, key, fmt.Sprintf("CONSTRAINT_ERR: '%s' does not match pattern /%s/", key, c.Pattern))
			}
		}
	}
}

// formatRustNumber prints like Rust's f64 `Display`: shortest round-trip digits,
// never an exponent (`5`, `0.25`, `70000`).
func formatRustNumber(f float64) string {
	return strconv.FormatFloat(f, 'f', -1, 64)
}

// hasNestedQuantifier reports a quantified group whose body also contains a
// quantifier (`(a+)+`), the shape behind exponential backtracking. RE2 is
// linear, but the reference refuses the pattern everywhere so every port
// projects the same document. Mirrors `has_nested_quantifier` in engine.rs.
func hasNestedQuantifier(pattern string) bool {
	chars := []rune(pattern)
	var opens []int
	for i := 0; i < len(chars); i++ {
		switch chars[i] {
		case '\\':
			i++
		case '[':
			for i < len(chars) && chars[i] != ']' {
				if chars[i] == '\\' {
					i++
				}
				i++
			}
		case '(':
			opens = append(opens, i)
		case ')':
			if len(opens) == 0 {
				continue
			}
			start := opens[len(opens)-1]
			opens = opens[:len(opens)-1]
			if i+1 < len(chars) && (chars[i+1] == '*' || chars[i+1] == '+' || chars[i+1] == '{') {
				var body []rune
				for j := start + 1; j < i; {
					if chars[j] == '\\' {
						j += 2
						continue
					}
					body = append(body, chars[j])
					j++
				}
				for k, b := range body {
					if b == '*' || b == '+' || (b == '{' && k+1 < len(body) && body[k+1] >= '0' && body[k+1] <= '9') {
						return true
					}
				}
			}
		}
	}
	return false
}

// ─── document-wide registries ───────────────────────────────────────────────

func (r *resolver) sortedMetadataPaths() []string {
	paths := make([]string, 0, len(r.result.Metadata))
	for p := range r.result.Metadata {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	return paths
}

// buildConstraintRegistry maps a field NAME to its declarations merged across
// the whole document: required/readonly OR, min is the largest, max the
// smallest, the first type/pattern/enum wins.
func (r *resolver) buildConstraintRegistry() map[string]*Constraints {
	reg := map[string]*Constraints{}
	for _, path := range r.sortedMetadataPaths() {
		fields := r.result.Metadata[path]
		names := make([]string, 0, len(fields))
		for name := range fields {
			names = append(names, name)
		}
		sort.Strings(names)
		for _, name := range names {
			c := fields[name].Constraints
			if c == nil {
				continue
			}
			base, ok := reg[name]
			if !ok {
				cp := *c
				reg[name] = &cp
				continue
			}
			mergeConstraints(base, c)
		}
	}
	return reg
}

func mergeConstraints(base, in *Constraints) {
	base.Required = base.Required || in.Required
	base.Readonly = base.Readonly || in.Readonly
	if in.Min != nil && (base.Min == nil || *in.Min > *base.Min) {
		v := *in.Min
		base.Min = &v
	}
	if in.Max != nil && (base.Max == nil || *in.Max < *base.Max) {
		v := *in.Max
		base.Max = &v
	}
	if base.TypeName == "" {
		base.TypeName = in.TypeName
	}
	if base.Pattern == "" {
		base.Pattern = in.Pattern
	}
	if base.EnumValues == nil {
		base.EnumValues = in.EnumValues
	}
}

func (r *resolver) buildTypeRegistry() map[string]string {
	reg := map[string]string{}
	for _, path := range r.sortedMetadataPaths() {
		fields := r.result.Metadata[path]
		names := make([]string, 0, len(fields))
		for name := range fields {
			names = append(names, name)
		}
		sort.Strings(names)
		for _, name := range names {
			if hint := fields[name].TypeHint; hint != "" {
				if _, seen := reg[name]; !seen {
					reg[name] = hint
				}
			}
		}
	}
	return reg
}

func isFieldError(v Value) bool {
	s, ok := v.(StringValue)
	return ok && (strings.HasPrefix(s.V, constraintErrPrefix) || strings.HasPrefix(s.V, typeErrPrefix))
}

// validateFieldConstraints applies the registry to every field of that name,
// wherever it sits, skipping values that already carry an error.
func (r *resolver) validateFieldConstraints(obj *Object, reg map[string]*Constraints, depth int) {
	if obj == nil || depth > MaxResolveDepth {
		return
	}
	for _, key := range obj.Keys() {
		if c, ok := reg[key]; ok {
			if v, _ := obj.Get(key); !isFieldError(v) {
				r.validateConstraints(obj, key, c)
			}
		}
		child, _ := obj.Get(key)
		switch x := child.(type) {
		case ObjectValue:
			r.validateFieldConstraints(x.V, reg, depth+1)
		case ArrayValue:
			for _, item := range x.V {
				if o, ok := item.(ObjectValue); ok {
					r.validateFieldConstraints(o.V, reg, depth+1)
				}
			}
		}
	}
}

func valueMatchesType(v Value, want string) bool {
	switch want {
	case "int":
		_, ok := v.(IntValue)
		return ok
	case "float":
		switch v.(type) {
		case FloatValue, IntValue:
			return true
		}
		return false
	case "bool":
		_, ok := v.(BoolValue)
		return ok
	case "string":
		switch v.(type) {
		case StringValue, SecretValue:
			return true
		}
		return false
	case "array":
		_, ok := v.(ArrayValue)
		return ok
	case "object":
		_, ok := v.(ObjectValue)
		return ok
	}
	return true
}

func valueTypeName(v Value) string {
	switch v.(type) {
	case IntValue:
		return "int"
	case FloatValue:
		return "float"
	case BoolValue:
		return "bool"
	case StringValue:
		return "string"
	case SecretValue:
		return "secret"
	case ArrayValue:
		return "array"
	case ObjectValue:
		return "object"
	}
	return "null"
}

func (r *resolver) validateFieldTypes(obj *Object, reg map[string]string, depth int) {
	if obj == nil || depth > MaxResolveDepth {
		return
	}
	for _, key := range obj.Keys() {
		val, _ := obj.Get(key)
		if val == nil {
			val = Null
		}
		if want, ok := reg[key]; ok && !valueMatchesType(val, want) {
			r.failField(obj, key, fmt.Sprintf("TYPE_ERR: '%s' expected %s but got %s", key, want, valueTypeName(val)))
		}
		child, _ := obj.Get(key)
		switch x := child.(type) {
		case ObjectValue:
			r.validateFieldTypes(x.V, reg, depth+1)
		case ArrayValue:
			for _, item := range x.V {
				if o, ok := item.(ObjectValue); ok {
					r.validateFieldTypes(o.V, reg, depth+1)
				}
			}
		}
	}
}

func (r *resolver) validateAll() {
	root := r.root()
	r.validateFieldConstraints(root, r.buildConstraintRegistry(), 0)
	r.validateFieldTypes(root, r.buildTypeRegistry(), 0)
}
