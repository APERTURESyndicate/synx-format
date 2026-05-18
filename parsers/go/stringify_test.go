package synx

import (
	"strings"
	"testing"
)

func TestStringifyBasicRoundtrip(t *testing.T) {
	r := Parse("active true\nage 30\nname Wario\n")
	out := Stringify(r.Root)
	for _, s := range []string{"name Wario", "age 30", "active true"} {
		if !strings.Contains(out, s) {
			t.Fatalf("missing %q in %q", s, out)
		}
	}
}

func TestStringifyMultilineUsesPipe(t *testing.T) {
	o := NewObjectMap()
	o.Set("rules", String("a\nb\nc"))
	if !strings.Contains(Stringify(Object_(o)), "rules |") {
		t.Fatalf("multiline pipe missing")
	}
}

func TestFormatterSortsKeys(t *testing.T) {
	out := Format("b 2\na 1\nc 3\n")
	a := strings.Index(out, "a 1")
	b := strings.Index(out, "b 2")
	if a < 0 || b < 0 || a >= b {
		t.Fatalf("keys not sorted: %q", out)
	}
}

func TestFormatterPreservesDirective(t *testing.T) {
	out := Format("!active\nname X\n")
	if !strings.HasPrefix(out, "!active") {
		t.Fatalf("directive not preserved")
	}
}
