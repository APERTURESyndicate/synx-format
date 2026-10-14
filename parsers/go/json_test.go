package synx

import (
	"strings"
	"testing"
)

func TestJSONPrimitives(t *testing.T) {
	if ToJSON(Null) != "null" {
		t.Fatalf("null")
	}
	if ToJSON(Bool(true)) != "true" {
		t.Fatalf("bool")
	}
	if ToJSON(Int(42)) != "42" {
		t.Fatalf("int")
	}
	if ToJSON(String("hi")) != "\"hi\"" {
		t.Fatalf("string")
	}
}

func TestJSONSecretRedacted(t *testing.T) {
	if ToJSON(Secret("xxx")) != "\"[SECRET]\"" {
		t.Fatalf("secret not redacted")
	}
}

func TestJSONSortedKeys(t *testing.T) {
	o := NewObjectMap()
	o.Set("b", Int(2))
	o.Set("a", Int(1))
	j := ToJSON(Object_(o))
	pa := strings.Index(j, "\"a\"")
	pb := strings.Index(j, "\"b\"")
	if pa < 0 || pb < 0 || pa >= pb {
		t.Fatalf("keys not sorted: %q", j)
	}
}

func TestJSONEscapes(t *testing.T) {
	j := ToJSON(String("line\nbreak\ttab\"quote\\back"))
	for _, s := range []string{"\\n", "\\t", "\\\"", "\\\\"} {
		if !strings.Contains(j, s) {
			t.Fatalf("escape %q missing in %q", s, j)
		}
	}
}

// Expected strings come from the Rust reference (`ryu`) via `synx parse`.
func TestJSONFloatShortestRoundTrip(t *testing.T) {
	cases := []struct {
		in   float64
		want string
	}{
		{3.14, "3.14"},
		{100, "100.0"},
		{0.0001, "0.0001"},
		{0.00001, "0.00001"},
		{0.000001, "1e-6"},
		{0.000123, "0.000123"},
		{1234567890123456.5, "1234567890123456.5"},
		{12345678901234567.0, "1.2345678901234568e16"},
		{123456789012345678901.5, "1.2345678901234568e20"},
		{-2.5, "-2.5"},
		{1e21, "1e21"},
	}
	for _, c := range cases {
		if got := ToJSON(Float(c.in)); got != c.want {
			t.Errorf("ToJSON(Float(%v)) = %s, want %s", c.in, got, c.want)
		}
	}
}
