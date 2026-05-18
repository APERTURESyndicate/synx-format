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
