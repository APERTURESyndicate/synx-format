package synx

import "sort"

// Object is an insertion-ordered string→Value map. Used wherever the Rust /
// Java port would use a `HashMap<String, Value>`: iteration order is stable
// for stringify and canonical reformat, while JSON output re-sorts for
// byte-stable diffs.
//
// Lookup is O(n); for the typical config sizes (<100 keys per object) this
// is faster than a hash table because of cache locality.
type Object struct {
	pairs []ObjectPair
}

// ObjectPair is one key/value entry. Exported so callers can iterate the
// pairs without going through the API surface.
type ObjectPair struct {
	Key   string
	Value Value
}

func NewObjectMap() *Object {
	return &Object{}
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

// Get looks up a key. Returns (value, true) on hit, (nil, false) on miss.
func (o *Object) Get(key string) (Value, bool) {
	for _, p := range o.pairs {
		if p.Key == key {
			return p.Value, true
		}
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
	_, ok := o.Get(key)
	return ok
}

// Set inserts or overwrites a key, preserving insertion order for new keys
// and original position for existing keys.
func (o *Object) Set(key string, value Value) {
	for i := range o.pairs {
		if o.pairs[i].Key == key {
			o.pairs[i].Value = value
			return
		}
	}
	o.pairs = append(o.pairs, ObjectPair{Key: key, Value: value})
}

// Remove deletes a key. Returns true when something was removed.
func (o *Object) Remove(key string) bool {
	for i := range o.pairs {
		if o.pairs[i].Key == key {
			o.pairs = append(o.pairs[:i], o.pairs[i+1:]...)
			return true
		}
	}
	return false
}

// Keys returns the keys in insertion order.
func (o *Object) Keys() []string {
	out := make([]string, len(o.pairs))
	for i, p := range o.pairs {
		out[i] = p.Key
	}
	return out
}

// SortedKeys returns a freshly sorted copy of the keys (used for canonical
// JSON / stringify output).
func (o *Object) SortedKeys() []string {
	keys := o.Keys()
	sort.Strings(keys)
	return keys
}

// Equal performs order-insensitive deep equality (matches Rust HashMap == HashMap).
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
