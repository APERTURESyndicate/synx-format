package synx

import (
	"strings"
	"testing"
)

func TestParse(t *testing.T) {
	j, err := Parse("name Demo\nversion 3.6.0\n")
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if !strings.Contains(j, `"name"`) || !strings.Contains(j, "Demo") {
		t.Fatalf("unexpected json: %s", j)
	}
}

func TestParseActive(t *testing.T) {
	j, err := ParseActive("!active\nport:env:default:8080 PORT\n")
	if err != nil {
		t.Fatalf("ParseActive: %v", err)
	}
	if !strings.Contains(j, "8080") {
		t.Fatalf("unexpected: %s", j)
	}
}

func TestRoundTripCompile(t *testing.T) {
	const text = "a 1\nb 2\n"
	bin, err := Compile(text, false)
	if err != nil {
		t.Fatalf("Compile: %v", err)
	}
	if !IsSynxb(bin) {
		t.Fatal("IsSynxb expected true")
	}
	back, err := Decompile(bin)
	if err != nil {
		t.Fatalf("Decompile: %v", err)
	}
	if !strings.Contains(back, "a") || !strings.Contains(back, "b") {
		t.Fatalf("decompiled: %q", back)
	}
}

func TestParseTool(t *testing.T) {
	j, err := ParseTool("!tool\nweb_search\n  query test\n")
	if err != nil {
		t.Fatalf("ParseTool: %v", err)
	}
	if !strings.Contains(j, "web_search") {
		t.Fatalf("unexpected: %s", j)
	}
}
