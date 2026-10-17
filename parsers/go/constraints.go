package synx

import (
	"fmt"
	"math"
	"os"
	"regexp"
	"strconv"
	"strings"
	"sync"
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
	maxCachedPatterns   = 256
)

func (r *resolver) report(msg string) {
	if r.opts.Strict {
		fmt.Fprintln(os.Stderr, "synx: "+msg)
	}
}

// validateConstraints checks one key against its own declaration.
func (r *resolver) validateConstraints(obj *Object, key string, c *Constraints) {
	i := obj.find(key)
	if i < 0 {
		if c.Required {
			msg := "CONSTRAINT_ERR: '" + key + "' is required"
			obj.Set(key, String(msg))
			r.report(msg)
		}
		return
	}
	if msg, bad := constraintViolation(key, obj.pairs[i].Value, c); bad {
		obj.pairs[i].Value = String(msg)
		r.report(msg)
	}
}

// constraintViolation is the error a value earns against its constraints;
// the first failing check wins.
func constraintViolation(key string, val Value, c *Constraints) (string, bool) {
	if c.Required {
		empty := false
		switch x := val.(type) {
		case NullValue:
			empty = true
		case StringValue:
			empty = x.V == ""
		}
		if empty {
			return "CONSTRAINT_ERR: '" + key + "' is required", true
		}
	}

	// An absent value (null) is checked against `enum` only, read as an empty
	// string; type, range and pattern apply to values that exist.
	if _, isNull := val.(NullValue); isNull {
		if c.EnumValues != nil {
			for _, e := range c.EnumValues {
				if e == "" {
					return "", false
				}
			}
			return enumError(key, c), true
		}
		return "", false
	}

	if c.hasTypeName() {
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
			// A `:secret` is a masked string: `[type:string]:secret:env` accepts it.
			switch val.(type) {
			case StringValue, SecretValue:
			default:
				ok = false
			}
		}
		if !ok {
			return "CONSTRAINT_ERR: '" + key + "' expected type '" + c.TypeName + "'", true
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
			text = rustFloatDisplay(x.V)
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
			return enumError(key, c), true
		}
	}

	// Numbers compare by value; a string's size is its count of Unicode
	// scalar values.
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
			return fmt.Sprintf("CONSTRAINT_ERR: '%s' value %s is below min %s", key, rustFloatDisplay(num), rustFloatDisplay(*c.Min)), true
		}
		if c.Max != nil && num > *c.Max {
			return fmt.Sprintf("CONSTRAINT_ERR: '%s' value %s exceeds max %s", key, rustFloatDisplay(num), rustFloatDisplay(*c.Max)), true
		}
	}

	// Over-long, invalid and backtracking-prone patterns never reject.
	if c.hasPattern() && len(c.Pattern) <= maxPatternBytes {
		if s, ok := val.(StringValue); ok && patternRejects(c.Pattern, s.V) {
			return "CONSTRAINT_ERR: '" + key + "' does not match pattern /" + c.Pattern + "/", true
		}
	}
	return "", false
}

func enumError(key string, c *Constraints) string {
	return "CONSTRAINT_ERR: '" + key + "' must be one of [" + strings.Join(c.EnumValues, "|") + "]"
}

var (
	patternMu    sync.Mutex
	patternCache = map[string]*regexp.Regexp{}
)

// patternRejects reports whether `s` fails `[pattern:…]`. Compiled patterns
// are cached (bounded), so a pattern declared on a field name is compiled
// once, not once per value checked against it.
func patternRejects(pat, s string) bool {
	patternMu.Lock()
	re, ok := patternCache[pat]
	if !ok {
		if len(patternCache) >= maxCachedPatterns {
			patternCache = map[string]*regexp.Regexp{}
		}
		if !hasNestedQuantifier(pat) {
			if compiled, err := regexp.Compile(pat); err == nil {
				re = compiled
			}
		}
		patternCache[pat] = re
	}
	patternMu.Unlock()
	return re != nil && !re.MatchString(s)
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

// collectRegistries fills the type and constraint registries from one object
// and its subtree: the object's own declarations first, then each child in
// document order. The first type for a field name wins; constraints for one
// name are merged.
func (r *resolver) collectRegistries(v Value, types map[string]string, cons map[string]*Constraints, depth int) {
	if depth > MaxResolveDepth {
		return
	}
	obj, ok := v.(ObjectValue)
	if !ok {
		return
	}
	if mm := r.meta.levels[string(r.path)]; mm != nil {
		for _, key := range mm.keys {
			meta := mm.items[key]
			if meta.hasTypeHint() {
				if _, seen := types[key]; !seen {
					types[key] = meta.TypeHint
				}
			}
			if meta.Constraints != nil {
				if existing, ok := cons[key]; ok {
					mergeConstraints(existing, meta.Constraints)
				} else {
					cp := *meta.Constraints
					cons[key] = &cp
				}
			}
		}
	}
	baseLen := len(r.path)
	for _, p := range obj.V.pairs {
		switch child := p.Value.(type) {
		case ObjectValue:
			r.pushKey(p.Key)
			r.collectRegistries(child, types, cons, depth+1)
			r.path = r.path[:baseLen]
		case ArrayValue:
			r.pushKey(p.Key)
			listLen := len(r.path)
			for idx, item := range child.V {
				if o, ok := item.(ObjectValue); ok {
					r.pushItem(idx)
					r.collectRegistries(o, types, cons, depth+1)
					r.path = r.path[:listLen]
				}
			}
			r.path = r.path[:baseLen]
		}
	}
}

// mergeConstraints merges a repeated declaration: required/readonly OR, the
// larger min, the smaller max, the first type/pattern/enum.
func mergeConstraints(base, in *Constraints) {
	base.Required = base.Required || in.Required
	base.Readonly = base.Readonly || in.Readonly
	if in.Min != nil {
		v := *in.Min
		if base.Min != nil {
			v = rustMax(*base.Min, v)
		}
		base.Min = &v
	}
	if in.Max != nil {
		v := *in.Max
		if base.Max != nil {
			v = rustMin(*base.Max, v)
		}
		base.Max = &v
	}
	if !base.hasTypeName() {
		base.TypeName, base.typeNameSet = in.TypeName, in.typeNameSet
	}
	if !base.hasPattern() {
		base.Pattern, base.patternSet = in.Pattern, in.patternSet
	}
	if base.EnumValues == nil {
		base.EnumValues = in.EnumValues
	}
}

// rustMax / rustMin are f64::max / f64::min: a NaN operand loses.
func rustMax(a, b float64) float64 {
	switch {
	case math.IsNaN(a):
		return b
	case math.IsNaN(b):
		return a
	case a > b:
		return a
	}
	return b
}

func rustMin(a, b float64) float64 {
	switch {
	case math.IsNaN(a):
		return b
	case math.IsNaN(b):
		return a
	case a < b:
		return a
	}
	return b
}

func isFieldError(v Value) bool {
	s, ok := v.(StringValue)
	return ok && (strings.HasPrefix(s.V, constraintErrPrefix) || strings.HasPrefix(s.V, typeErrPrefix))
}

// validateFields applies both registries to every field of that name,
// wherever it sits: constraints, then type. A value that already carries an
// error is not measured against constraints again.
func (r *resolver) validateFields(v Value, cons map[string]*Constraints, types map[string]string) {
	if len(cons) == 0 && len(types) == 0 {
		return
	}
	obj, ok := v.(ObjectValue)
	if !ok {
		return
	}
	for i := range obj.V.pairs {
		key := obj.V.pairs[i].Key
		val := obj.V.pairs[i].Value
		if c, ok := cons[key]; ok && !isFieldError(val) {
			if msg, bad := constraintViolation(key, val, c); bad {
				val = String(msg)
				obj.V.pairs[i].Value = val
				r.report(msg)
			}
		}
		if want, ok := types[key]; ok && !valueMatchesType(val, want) {
			msg := "TYPE_ERR: '" + key + "' expected " + want + " but got " + valueTypeName(val)
			val = String(msg)
			obj.V.pairs[i].Value = val
			r.report(msg)
		}
		switch x := val.(type) {
		case ObjectValue:
			r.validateFields(x, cons, types)
		case ArrayValue:
			for _, item := range x.V {
				if o, ok := item.(ObjectValue); ok {
					r.validateFields(o, cons, types)
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
