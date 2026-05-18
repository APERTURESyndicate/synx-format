package synx

import "sort"

// DiffChange captures one changed key's before/after values.
type DiffChange struct {
	From Value
	To   Value
}

// ChangedEntry is a single change paired with its key.
type ChangedEntry struct {
	Key    string
	Change DiffChange
}

// DiffResult is the structural-diff outcome.
type DiffResult struct {
	Added     *Object
	Removed   *Object
	Changed   []ChangedEntry
	Unchanged []string
}

// Diff produces a structural diff between two top-level objects.
func Diff(a, b *Object) DiffResult {
	added := NewObjectMap()
	removed := NewObjectMap()
	var changed []ChangedEntry
	var unchanged []string

	for _, p := range a.Pairs() {
		if bv, ok := b.Get(p.Key); ok {
			if Equal(p.Value, bv) {
				unchanged = append(unchanged, p.Key)
			} else {
				changed = append(changed, ChangedEntry{Key: p.Key, Change: DiffChange{From: p.Value, To: bv}})
			}
		} else {
			removed.Set(p.Key, p.Value)
		}
	}
	for _, p := range b.Pairs() {
		if !a.Contains(p.Key) {
			added.Set(p.Key, p.Value)
		}
	}
	sort.Strings(unchanged)
	return DiffResult{Added: added, Removed: removed, Changed: changed, Unchanged: unchanged}
}

// DiffToValue converts a DiffResult to a Value suitable for JSON output.
func DiffToValue(d DiffResult) Value {
	root := NewObjectMap()
	root.Set("added", Object_(d.Added))
	root.Set("removed", Object_(d.Removed))

	changed := NewObjectMap()
	for _, e := range d.Changed {
		inner := NewObjectMap()
		inner.Set("from", e.Change.From)
		inner.Set("to", e.Change.To)
		changed.Set(e.Key, Object_(inner))
	}
	root.Set("changed", Object_(changed))

	arr := make([]Value, len(d.Unchanged))
	for i, s := range d.Unchanged {
		arr[i] = String(s)
	}
	root.Set("unchanged", Array(arr))
	return Object_(root)
}
