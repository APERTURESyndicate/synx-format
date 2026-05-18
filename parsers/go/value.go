// Package synx provides a native Go parser for the SYNX (Active Data Format).
// Parity with crates/synx-core 3.6.x — no cgo, no external dependencies.
package synx

// Kind tags every SynxValue concrete type. Stable wire numbers (used by
// .synxb).
type Kind uint8

const (
	KindNull Kind = iota
	KindBool
	KindInt
	KindFloat
	KindString
	KindArray
	KindObject
	KindSecret
)

func (k Kind) String() string {
	switch k {
	case KindNull:
		return "null"
	case KindBool:
		return "bool"
	case KindInt:
		return "int"
	case KindFloat:
		return "float"
	case KindString:
		return "string"
	case KindArray:
		return "array"
	case KindObject:
		return "object"
	case KindSecret:
		return "secret"
	}
	return "unknown"
}

// Value is the sealed sum type for SYNX scalars and composites.
// "Sealed" via the unexported isSynxValue marker — only this package can
// implement Value.
type Value interface {
	Kind() Kind
	isSynxValue()
}

// NullValue is the canonical singleton for SYNX null. Use Null instead of
// creating new instances.
type NullValue struct{}

func (NullValue) Kind() Kind   { return KindNull }
func (NullValue) isSynxValue() {}

// Null is the canonical SYNX null literal.
var Null Value = NullValue{}

type BoolValue struct{ V bool }

func (BoolValue) Kind() Kind   { return KindBool }
func (BoolValue) isSynxValue() {}

type IntValue struct{ V int64 }

func (IntValue) Kind() Kind   { return KindInt }
func (IntValue) isSynxValue() {}

type FloatValue struct{ V float64 }

func (FloatValue) Kind() Kind   { return KindFloat }
func (FloatValue) isSynxValue() {}

type StringValue struct{ V string }

func (StringValue) Kind() Kind   { return KindString }
func (StringValue) isSynxValue() {}

type ArrayValue struct{ V []Value }

func (ArrayValue) Kind() Kind   { return KindArray }
func (ArrayValue) isSynxValue() {}

// ObjectValue wraps a pointer to *Object so multiple references share the
// same underlying ordered map. This matches the Rust / Swift / Java ports
// where the object is reference-semantic.
type ObjectValue struct{ V *Object }

func (ObjectValue) Kind() Kind   { return KindObject }
func (ObjectValue) isSynxValue() {}

// SecretValue is the redacted variant — never appears literally in JSON output.
type SecretValue struct{ V string }

func (SecretValue) Kind() Kind   { return KindSecret }
func (SecretValue) isSynxValue() {}

// ─── Constructors ───────────────────────────────────────────────────────────

func Bool(b bool) Value      { return BoolValue{V: b} }
func Int(n int64) Value      { return IntValue{V: n} }
func Float(f float64) Value  { return FloatValue{V: f} }
func String(s string) Value  { return StringValue{V: s} }
func Secret(s string) Value  { return SecretValue{V: s} }
func Array(v []Value) Value  { return ArrayValue{V: v} }
func Object_(o *Object) Value { return ObjectValue{V: o} }

// NewArray returns an empty array Value.
func NewArray() Value { return ArrayValue{V: []Value{}} }

// NewObject returns an ObjectValue wrapping a fresh ordered map.
func NewObject() Value { return ObjectValue{V: NewObjectMap()} }

// ─── Typed accessors ────────────────────────────────────────────────────────

// IsNull is the conventional null check (avoids type assertion).
func IsNull(v Value) bool { _, ok := v.(NullValue); return ok }

// AsBool returns (val, true) when v is a Bool; otherwise (false, false).
func AsBool(v Value) (bool, bool) {
	if b, ok := v.(BoolValue); ok {
		return b.V, true
	}
	return false, false
}

func AsInt(v Value) (int64, bool) {
	if i, ok := v.(IntValue); ok {
		return i.V, true
	}
	return 0, false
}

func AsFloat(v Value) (float64, bool) {
	if f, ok := v.(FloatValue); ok {
		return f.V, true
	}
	return 0, false
}

func AsString(v Value) (string, bool) {
	if s, ok := v.(StringValue); ok {
		return s.V, true
	}
	return "", false
}

func AsSecret(v Value) (string, bool) {
	if s, ok := v.(SecretValue); ok {
		return s.V, true
	}
	return "", false
}

func AsArray(v Value) ([]Value, bool) {
	if a, ok := v.(ArrayValue); ok {
		return a.V, true
	}
	return nil, false
}

func AsObject(v Value) (*Object, bool) {
	if o, ok := v.(ObjectValue); ok {
		return o.V, true
	}
	return nil, false
}

// AsNumber returns a float64 representation of v when it is numeric.
// Int → float64, Float passes through, Bool → 0/1, otherwise (0, false).
func AsNumber(v Value) (float64, bool) {
	switch t := v.(type) {
	case IntValue:
		return float64(t.V), true
	case FloatValue:
		return t.V, true
	case BoolValue:
		if t.V {
			return 1, true
		}
		return 0, true
	}
	return 0, false
}

// Equal performs deep value equality. Objects compare order-insensitively,
// matching the Rust HashMap == HashMap contract.
func Equal(a, b Value) bool {
	if a == nil && b == nil {
		return true
	}
	if a == nil || b == nil {
		return false
	}
	if a.Kind() != b.Kind() {
		return false
	}
	switch x := a.(type) {
	case NullValue:
		return true
	case BoolValue:
		return x.V == b.(BoolValue).V
	case IntValue:
		return x.V == b.(IntValue).V
	case FloatValue:
		return x.V == b.(FloatValue).V
	case StringValue:
		return x.V == b.(StringValue).V
	case SecretValue:
		return x.V == b.(SecretValue).V
	case ArrayValue:
		other := b.(ArrayValue).V
		if len(x.V) != len(other) {
			return false
		}
		for i := range x.V {
			if !Equal(x.V[i], other[i]) {
				return false
			}
		}
		return true
	case ObjectValue:
		return x.V.Equal(b.(ObjectValue).V)
	}
	return false
}
