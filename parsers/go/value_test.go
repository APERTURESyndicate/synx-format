package synx

import "testing"

func TestObjectSetGetRemove(t *testing.T) {
	o := NewObjectMap()
	o.Set("a", Int(1))
	o.Set("b", String("two"))
	if v, ok := o.Get("a"); !ok || !Equal(v, Int(1)) {
		t.Fatalf("get a failed")
	}
	if !o.Contains("b") {
		t.Fatalf("contains b failed")
	}
	if !o.Remove("a") {
		t.Fatalf("remove a failed")
	}
	if o.Contains("a") {
		t.Fatalf("a still present after remove")
	}
}

func TestObjectEqualityOrderInsensitive(t *testing.T) {
	a := NewObjectMap()
	a.Set("x", Int(1))
	a.Set("y", Int(2))
	b := NewObjectMap()
	b.Set("y", Int(2))
	b.Set("x", Int(1))
	if !Equal(Object_(a), Object_(b)) {
		t.Fatalf("order-insensitive equality failed")
	}
}

func TestTypeHelpers(t *testing.T) {
	if !IsNull(Null) {
		t.Fatalf("IsNull")
	}
	if n, ok := AsInt(Int(5)); !ok || n != 5 {
		t.Fatalf("AsInt")
	}
	if d, ok := AsNumber(Float(3.14)); !ok || d != 3.14 {
		t.Fatalf("AsNumber float")
	}
	if _, ok := AsInt(String("x")); ok {
		t.Fatalf("AsInt should fail on string")
	}
}
