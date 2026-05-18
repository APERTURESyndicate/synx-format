package synx

import "testing"

func TestDiffIdentical(t *testing.T) {
	a := NewObjectMap()
	a.Set("x", Int(1))
	a.Set("y", Int(2))
	b := NewObjectMap()
	b.Set("y", Int(2))
	b.Set("x", Int(1))
	d := Diff(a, b)
	if !d.Added.IsEmpty() || !d.Removed.IsEmpty() || len(d.Changed) != 0 {
		t.Fatalf("identical mismatch: %+v", d)
	}
	if len(d.Unchanged) != 2 {
		t.Fatalf("unchanged len: %d", len(d.Unchanged))
	}
}

func TestDiffAddedRemoved(t *testing.T) {
	a := NewObjectMap()
	a.Set("x", Int(1))
	b := NewObjectMap()
	b.Set("y", Int(2))
	d := Diff(a, b)
	if d.Added.Len() != 1 || d.Removed.Len() != 1 {
		t.Fatalf("added/removed mismatch: %+v", d)
	}
}

func TestDiffChanged(t *testing.T) {
	a := NewObjectMap()
	a.Set("name", String("Alice"))
	b := NewObjectMap()
	b.Set("name", String("Bob"))
	d := Diff(a, b)
	if len(d.Changed) != 1 || d.Changed[0].Key != "name" {
		t.Fatalf("changed mismatch: %+v", d)
	}
}
