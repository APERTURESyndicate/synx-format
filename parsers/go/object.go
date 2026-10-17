package synx

import "sort"

// Object is an insertion-ordered string→Value map — the Go counterpart of
// synx-core's `IndexMap`. Order is load-bearing: active-mode markers resolve
// in the order the document declares them, and the writers sort keys
// themselves where they need a canonical order.
//
// Small objects are scanned linearly; once an object outgrows
// objectIndexThreshold entries a key→position index is built, so a root of
// thousands of keys is not searched once per line (that made parsing a 4 MB
// document quadratic before 3.8.0).
type Object struct {
	pairs []ObjectPair
	index map[string]int
}

// ObjectPair is one key/value entry. Exported so callers can iterate the
// pairs without going through the API surface.
type ObjectPair struct {
	Key   string
	Value Value
}

const objectIndexThreshold = 12

func NewObjectMap() *Object {
	return &Object{}
}

func newObjectCap(n int) *Object {
	return &Object{pairs: make([]ObjectPair, 0, n)}
}

// Pairs returns the underlying ordered entries. Mutating the slice does not
// affect the Object (return is a copy).
func (o *Object) Pairs() []ObjectPair {
	out := make([]ObjectPair, len(o.pairs))
	copy(out, o.pairs)
	return out
}

// Len returns the number of entries.
func (o *Object) Len() int { return len(o.pairs) }

// IsEmpty is shorthand for `Len() == 0`.
func (o *Object) IsEmpty() bool { return len(o.pairs) == 0 }

func (o *Object) find(key string) int {
	if o.index != nil {
		if i, ok := o.index[key]; ok {
			return i
		}
		return -1
	}
	for i := range o.pairs {
		if o.pairs[i].Key == key {
			return i
		}
	}
	return -1
}

// Get looks up a key. Returns (value, true) on hit, (nil, false) on miss.
func (o *Object) Get(key string) (Value, bool) {
	if i := o.find(key); i >= 0 {
		return o.pairs[i].Value, true
	}
	return nil, false
}

// GetOr returns the value for `key`, or `fallback` if absent.
func (o *Object) GetOr(key string, fallback Value) Value {
	if v, ok := o.Get(key); ok {
		return v
	}
	return fallback
}

// Contains reports whether the object holds `key`.
func (o *Object) Contains(key string) bool {
	return o.find(key) >= 0
}

// Set inserts or overwrites a key, preserving insertion order for new keys
// and original position for existing keys.
func (o *Object) Set(key string, value Value) {
	if i := o.find(key); i >= 0 {
		o.pairs[i].Value = value
		return
	}
	if o.pairs == nil {
		o.pairs = make([]ObjectPair, 0, 4)
	}
	o.pairs = append(o.pairs, ObjectPair{Key: key, Value: value})
	if o.index != nil {
		o.index[key] = len(o.pairs) - 1
	} else if len(o.pairs) > objectIndexThreshold {
		o.buildIndex()
	}
}

func (o *Object) buildIndex() {
	o.index = make(map[string]int, len(o.pairs)*2)
	for i := range o.pairs {
		o.index[o.pairs[i].Key] = i
	}
}

// Remove deletes a key, keeping the order of the others. Returns true when
// something was removed.
func (o *Object) Remove(key string) bool {
	i := o.find(key)
	if i < 0 {
		return false
	}
	o.pairs = append(o.pairs[:i], o.pairs[i+1:]...)
	o.reindex()
	return true
}

// retain keeps the entries for which keep returns true, in order.
func (o *Object) retain(keep func(key string) bool) {
	n := 0
	for _, p := range o.pairs {
		if keep(p.Key) {
			o.pairs[n] = p
			n++
		}
	}
	if n == len(o.pairs) {
		return
	}
	for i := n; i < len(o.pairs); i++ {
		o.pairs[i] = ObjectPair{}
	}
	o.pairs = o.pairs[:n]
	o.reindex()
}

func (o *Object) reindex() {
	o.index = nil
	if len(o.pairs) > objectIndexThreshold {
		o.buildIndex()
	}
}

// Keys returns the keys in insertion order.
func (o *Object) Keys() []string {
	out := make([]string, len(o.pairs))
	for i, p := range o.pairs {
		out[i] = p.Key
	}
	return out
}

// SortedKeys returns a freshly sorted copy of the keys (byte order, which is
// code-point order for UTF-8).
func (o *Object) SortedKeys() []string {
	keys := o.Keys()
	sort.Strings(keys)
	return keys
}

// Equal performs order-insensitive deep equality.
func (o *Object) Equal(other *Object) bool {
	if o == nil && other == nil {
		return true
	}
	if o == nil || other == nil {
		return false
	}
	if len(o.pairs) != len(other.pairs) {
		return false
	}
	for _, p := range o.pairs {
		v, ok := other.Get(p.Key)
		if !ok {
			return false
		}
		if !Equal(p.Value, v) {
			return false
		}
	}
	return true
}

// Clone returns a deep copy: nested objects and arrays are copied too, so the
// copy can be resolved or validated without touching the original.
func (o *Object) Clone() *Object {
	out := &Object{pairs: make([]ObjectPair, len(o.pairs))}
	for i, p := range o.pairs {
		out.pairs[i] = ObjectPair{Key: p.Key, Value: CloneValue(p.Value)}
	}
	if o.index != nil {
		out.buildIndex()
	}
	return out
}

// CloneValue deep-copies objects and arrays; scalars are returned as they are.
func CloneValue(v Value) Value {
	switch x := v.(type) {
	case ObjectValue:
		if x.V == nil {
			return x
		}
		return ObjectValue{V: x.V.Clone()}
	case ArrayValue:
		out := make([]Value, len(x.V))
		for i, item := range x.V {
			out[i] = CloneValue(item)
		}
		return ArrayValue{V: out}
	}
	return v
}
