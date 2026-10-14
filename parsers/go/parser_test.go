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

// SYNX 3.7 — `|+` keeps each continuation line's indent relative to the
// first non-empty content line (see spec §8.4.1).
func TestParseMultilineBlockPreserveIndent(t *testing.T) {
	src := "prompt |+\n  Top\n    Indented two\n      Indented four\n    Back to two\n  Top again"
	r := Parse(src)
	o, _ := AsObject(r.Root)
	got, _ := AsString(o.GetOr("prompt", Null))
	want := "Top\n  Indented two\n    Indented four\n  Back to two\nTop again"
	if got != want {
		t.Fatalf("|+ mismatch\n got: %q\nwant: %q", got, want)
	}
}

func TestParseMultilineBlockPreserveIndentLocksBase(t *testing.T) {
	src := "code |+\n    function foo() {\n      return 1;\n    }"
	r := Parse(src)
	o, _ := AsObject(r.Root)
	got, _ := AsString(o.GetOr("code", Null))
	want := "function foo() {\n  return 1;\n}"
	if got != want {
		t.Fatalf("|+ base-indent lock failed\n got: %q\nwant: %q", got, want)
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

func TestParseLeadingBOMDoesNotHideActive(t *testing.T) {
	r := Parse("\U0000FEFF!active\nname synx\n")
	if r.Mode != ModeActive {
		t.Fatalf("mode = %v, want active", r.Mode)
	}
	o, _ := AsObject(r.Root)
	if _, ok := o.Get("name"); !ok || len(o.Keys()) != 1 {
		t.Fatalf("keys = %v, want just name", o.Keys())
	}
	o = ParseRoot("\U0000FEFFname synx\n")
	if v, _ := o.Get("name"); !Equal(v, String("synx")) {
		t.Fatalf("BOM glued to first key: %v", o.Keys())
	}
}

func TestParseBlockBodyIsRawText(t *testing.T) {
	o := ParseRoot("code |+\n  # keep\n  ### fence\n  !active\nnext 1\n")
	if v, _ := o.Get("code"); !Equal(v, String("# keep\n### fence\n!active")) {
		t.Fatalf("code = %v", v)
	}
	if v, _ := o.Get("next"); !Equal(v, Int(1)) {
		t.Fatalf("next = %v", v)
	}
}

func TestParseParenthesisedValuesArePlainText(t *testing.T) {
	src := "a (aa)\nb (int)5\nc (random:bool)\ne (aa) tail\nf[enum:x|y] (x)\ng(aa) 5\nh(int)\ni(int)\n  child 1\nj(string) |\n  kept\nlist\n  - (int)5\n  - (aa)\n  - m(int) 5\n    n 1\n"
	want := `{"a":"(aa)","b":"(int)5","c":"(random:bool)","e":"(aa) tail","f":"(x)","g":5,"h":{},"i":{"child":1},"j":"kept","list":["(int)5","(aa)",{"m":5,"n":1}]}`
	if got := ToJSON(Parse(src).Root); got != want {
		t.Fatalf("got  %s\nwant %s", got, want)
	}
}

func TestParseTypeHintOnlyAttachedToKey(t *testing.T) {
	o := ParseRoot("a (int) 5\nb(int) 5\n")
	if v, _ := o.Get("a"); !Equal(v, String("(int) 5")) {
		t.Errorf("a = %v", v)
	}
	if v, _ := o.Get("b"); !Equal(v, Int(5)) {
		t.Errorf("b = %v", v)
	}
}

func TestParseUnicodeIndentInPreservedBlock(t *testing.T) {
	// Indent is Unicode whitespace; the base-indent cut lands on a rune boundary.
	o := ParseRoot("body |+\n\U000000A0\U000000A0a\n\U000000A0\U000000A0\U000000A0b\nafter 1\n")
	if v, _ := o.Get("body"); !Equal(v, String("a\n\U000000A0b")) {
		t.Fatalf("body = %q", v)
	}
	if v, _ := o.Get("after"); !Equal(v, Int(1)) {
		t.Fatalf("after = %v", v)
	}
}
