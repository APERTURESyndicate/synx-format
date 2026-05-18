package synx

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestConformanceCorpus replays every `.synx` file in the shared corpus
// (../../tests/conformance/cases) through this parser. Missing corpus → skip.
func TestConformanceCorpus(t *testing.T) {
	dir := findCorpus()
	if dir == "" {
		t.Skip("conformance corpus not present in this checkout")
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Skipf("cannot read corpus: %v", err)
	}
	parsed, failed := 0, 0
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".synx") {
			continue
		}
		text, err := os.ReadFile(filepath.Join(dir, e.Name()))
		if err != nil {
			continue
		}
		r := Parse(string(text))
		if _, ok := r.Root.(ObjectValue); ok {
			parsed++
		} else {
			failed++
			t.Logf("corpus %q did not yield an object", e.Name())
		}
	}
	t.Logf("[corpus] parsed %d files, %d failed", parsed, failed)
	if failed > 0 {
		t.Fatalf("%d corpus files failed", failed)
	}
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
