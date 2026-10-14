package synx

import (
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
	"unicode"
)

// TestConformanceCorpus replays every `.synx` case of the shared corpus
// (../../tests/conformance/cases) that has a sibling `.expected.json` and
// compares the canonical JSON byte for byte, exactly like the Rust runner.
// Missing corpus → skip.
func TestConformanceCorpus(t *testing.T) {
	dir := findCorpus()
	if dir == "" {
		t.Skip("conformance corpus not present in this checkout")
	}
	names, err := filepath.Glob(filepath.Join(dir, "*.synx"))
	if err != nil || len(names) == 0 {
		t.Fatalf("no .synx cases found in %s", dir)
	}
	sort.Strings(names)

	checked, failed := 0, 0
	for _, synxPath := range names {
		expectedPath := strings.TrimSuffix(synxPath, ".synx") + ".expected.json"
		expectedRaw, err := os.ReadFile(expectedPath)
		if err != nil {
			continue
		}
		// The BOM is deliberately not stripped here: that is the parser's job.
		input, err := os.ReadFile(synxPath)
		if err != nil {
			t.Fatalf("%s: %v", synxPath, err)
		}
		expected := strings.TrimSpace(string(expectedRaw))
		got := conformanceJSON(string(input))
		checked++
		if got != expected {
			failed++
			t.Errorf("conformance FAIL: %s\n  got:      %s\n  expected: %s",
				filepath.Base(synxPath), got, expected)
		}
	}
	if checked < 24 {
		t.Errorf("expected at least 24 conformance cases, ran %d", checked)
	}
	t.Logf("[corpus] %d cases checked, %d failed", checked, failed)
}

// conformanceJSON routes `!tool` documents through ParseTool and everything
// else through the plain parse (resolved with default options when the
// document is `!active`), then serialises canonically.
func conformanceJSON(input string) string {
	if strings.HasPrefix(strings.TrimLeftFunc(input, unicode.IsSpace), "!tool") {
		return ToJSON(Object_(ParseTool(input, Options{})))
	}
	r := Parse(input)
	if r.Mode == ModeActive {
		Resolve(&r, Options{})
	}
	return ToJSON(r.Root)
}

func findCorpus() string {
	candidates := []string{
		"tests/conformance/cases",
		"../tests/conformance/cases",
		"../../tests/conformance/cases",
		"../../../tests/conformance/cases",
	}
	cwd, _ := os.Getwd()
	for _, c := range candidates {
		p := filepath.Join(cwd, c)
		if fi, err := os.Stat(p); err == nil && fi.IsDir() {
			return p
		}
	}
	return ""
}
