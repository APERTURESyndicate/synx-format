package synx

import (
	"testing"
)

func TestParseSimpleKV(t *testing.T) {
	r := Parse("name Wario\nage 30\nactive true\nscore 99.5\nempty null")
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("name"); !Equal(v, String("Wario")) {
		t.Fatalf("name mismatch: %v", v)
	}
	if v, _ := o.Get("age"); !Equal(v, Int(30)) {
		t.Fatalf("age mismatch: %v", v)
	}
	if v, _ := o.Get("active"); !Equal(v, Bool(true)) {
		t.Fatalf("active mismatch: %v", v)
	}
	if v, _ := o.Get("score"); !Equal(v, Float(99.5)) {
		t.Fatalf("score mismatch: %v", v)
	}
	if v, _ := o.Get("empty"); !IsNull(v) {
		t.Fatalf("empty should be null: %v", v)
	}
	if r.Mode != ModeStatic {
		t.Fatalf("mode mismatch")
	}
}

func TestParseNestedObjects(t *testing.T) {
	r := Parse("server\n  host 0.0.0.0\n  port 8080\n  ssl\n    enabled true")
	o, _ := AsObject(r.Root)
	sv, _ := o.Get("server")
	server, _ := AsObject(sv)
	if v, _ := server.Get("port"); !Equal(v, Int(8080)) {
		t.Fatalf("port mismatch")
	}
	ssl, _ := AsObject(server.GetOr("ssl", Null))
	if v, _ := ssl.Get("enabled"); !Equal(v, Bool(true)) {
		t.Fatalf("ssl.enabled mismatch")
	}
}

func TestParseLists(t *testing.T) {
	r := Parse("inventory\n  - Sword\n  - Shield\n  - Potion")
	o, _ := AsObject(r.Root)
	arr, _ := AsArray(o.GetOr("inventory", Null))
	if len(arr) != 3 {
		t.Fatalf("expected 3 items, got %d", len(arr))
	}
}

func TestParseMultilineBlock(t *testing.T) {
	r := Parse("rules |\n  Rule one.\n  Rule two.\n  Rule three.")
	o, _ := AsObject(r.Root)
	s, _ := AsString(o.GetOr("rules", Null))
	if s == "" {
		t.Fatalf("multiline string empty")
	}
}

func TestParseActiveMetadata(t *testing.T) {
	r := Parse("!active\nprice 100\ntax:calc price * 0.2")
	if r.Mode != ModeActive {
		t.Fatalf("mode != active")
	}
	if r.Metadata[""] == nil || r.Metadata[""]["tax"].Markers[0] != "calc" {
		t.Fatalf("missing calc metadata")
	}
}

func TestParsePrototypePollutionRejected(t *testing.T) {
	r := Parse("__proto__ evil\nconstructor evil\nprototype evil\nname safe\n")
	o, _ := AsObject(r.Root)
	if o.Contains("__proto__") || o.Contains("constructor") || o.Contains("prototype") {
		t.Fatalf("prototype pollution not rejected")
	}
	if !o.Contains("name") {
		t.Fatalf("legitimate key dropped")
	}
}

func TestParseConstraints(t *testing.T) {
	r := Parse("!active\nname[min:3, max:30, required] Wario")
	c := r.Metadata[""]["name"].Constraints
	if c == nil || c.Min == nil || *c.Min != 3 {
		t.Fatalf("min")
	}
	if c.Max == nil || *c.Max != 30 {
		t.Fatalf("max")
	}
	if !c.Required {
		t.Fatalf("required")
	}
}

func TestParseTypeHintStringKeepsString(t *testing.T) {
	r := Parse("zip(string) 90210")
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("zip"); !Equal(v, String("90210")) {
		t.Fatalf("type hint string not honoured: %v", v)
	}
}

func TestParseToolDirective(t *testing.T) {
	r := Parse("!tool\nweb_search\n  query test\n  lang ru\n")
	if !r.Tool {
		t.Fatalf("tool flag not set")
	}
	shaped := ReshapeToolOutput(r.Root, false)
	o, _ := AsObject(shaped)
	if v, _ := o.Get("tool"); !Equal(v, String("web_search")) {
		t.Fatalf("tool name mismatch")
	}
}
