package synx

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// Benchmarks over the shared inputs: benchmarks/config.synx (a 2.5 KB service
// config), the conformance corpus, and generated documents shaped like the
// release benches (static-mid ≈ 130 KB, static-large ≈ 4 MB, active-mid
// ≈ 40 KB, active-large ≈ 1.2 MB). Run with
//
//	go test -run '^$' -bench . -benchmem
//
// The generators are deterministic, so numbers compare across versions.

// repoRoot is the synx-format checkout: SYNX_REPO, else two levels up.
func repoRoot() string {
	if r := os.Getenv("SYNX_REPO"); r != "" {
		return r
	}
	return filepath.Join("..", "..")
}

func benchConfig(b *testing.B) string {
	b.Helper()
	data, err := os.ReadFile(filepath.Join(repoRoot(), "benchmarks", "config.synx"))
	if err != nil {
		b.Skip("benchmarks/config.synx not present")
	}
	return string(data)
}

func genStatic(groups int) string {
	var sb strings.Builder
	for g := 0; g < groups; g++ {
		fmt.Fprintf(&sb, "service_%d\n", g)
		fmt.Fprintf(&sb, "  name svc-%d\n  port %d\n  ratio %d.%d\n  enabled %v\n", g, 3000+g%1000, g%7, g%100, g%2 == 0)
		fmt.Fprintf(&sb, "  host 10.0.%d.%d\n  note \"quoted %d\"\n", g%256, g%200, g)
		sb.WriteString("  limits\n")
		fmt.Fprintf(&sb, "    cpu %d\n    memory %dMi\n    burst %d.5\n", g%16+1, 128*(g%8+1), g%9)
		sb.WriteString("  tags\n")
		for t := 0; t < 4; t++ {
			fmt.Fprintf(&sb, "    - tag%d\n", (g+t)%50)
		}
		sb.WriteString("  replicas\n")
		for r := 0; r < 2; r++ {
			fmt.Fprintf(&sb, "    - zone z%d\n      weight %d\n", r, (g+r)%10)
		}
		sb.WriteString("  description |\n    First line of text.\n    Second line of text.\n")
	}
	return sb.String()
}

func genActive(groups int) string {
	var sb strings.Builder
	sb.WriteString("!active\nbase 100\nrate 0.2\nregion eu\n")
	for g := 0; g < groups; g++ {
		fmt.Fprintf(&sb, "svc_%d\n", g)
		fmt.Fprintf(&sb, "  port:env:default:%d SVC_%d_PORT\n", 7000+g%1000, g)
		fmt.Fprintf(&sb, "  price %d\n", 10+g%90)
		sb.WriteString("  tax:calc price * rate\n  total:calc price + tax\n")
		fmt.Fprintf(&sb, "  weight[min:1, max:100] %d\n", g%100+1)
		fmt.Fprintf(&sb, "  mode[enum:a|b|c] %c\n", 'a'+rune(g%3))
		sb.WriteString("  url http://{region}.example/{svc_0.price}\n")
		sb.WriteString("  tags:join\n    - x\n    - y\n")
		sb.WriteString("  servers\n")
		fmt.Fprintf(&sb, "    - name a%d\n      port:env:default:%d SVC_A_%d\n", g, 8000+g%100, g)
		fmt.Fprintf(&sb, "    - name b%d\n      port[max:65535] %d\n", g, 9000+g%100)
	}
	return sb.String()
}

func benchCorpus(b *testing.B) []string {
	b.Helper()
	dir := filepath.Join(repoRoot(), "tests", "conformance", "cases")
	names, _ := filepath.Glob(filepath.Join(dir, "*.synx"))
	var docs []string
	for _, n := range names {
		data, err := os.ReadFile(n)
		if err == nil {
			docs = append(docs, string(data))
		}
	}
	return docs
}

func benchParse(b *testing.B, doc string) {
	b.SetBytes(int64(len(doc)))
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		r := Parse(doc)
		if r.Mode == ModeActive {
			Resolve(&r, Options{Env: map[string]string{}})
		}
	}
}

func BenchmarkParseConfig(b *testing.B)      { benchParse(b, benchConfig(b)) }
func BenchmarkParseStaticMid(b *testing.B)   { benchParse(b, genStatic(400)) }
func BenchmarkParseStaticLarge(b *testing.B) { benchParse(b, genStatic(12000)) }
func BenchmarkParseActiveMid(b *testing.B)   { benchParse(b, genActive(100)) }
func BenchmarkParseActiveLarge(b *testing.B) { benchParse(b, genActive(3000)) }

func BenchmarkParseCorpus(b *testing.B) {
	docs := benchCorpus(b)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		for _, d := range docs {
			r := Parse(d)
			if r.Mode == ModeActive {
				Resolve(&r, Options{Env: map[string]string{}})
			}
		}
	}
}

func BenchmarkJSONStaticLarge(b *testing.B) {
	r := Parse(genStatic(12000))
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = ToJSON(r.Root)
	}
}

func BenchmarkCompileStaticLarge(b *testing.B) {
	r := Parse(genStatic(12000))
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := Compile(r, false); err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkDecompileStaticLarge(b *testing.B) {
	bin, err := Compile(Parse(genStatic(12000)), false)
	if err != nil {
		b.Fatal(err)
	}
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := DecompileToText(bin); err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkStringifyStaticLarge(b *testing.B) {
	r := Parse(genStatic(12000))
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = Stringify(r.Root)
	}
}

func BenchmarkFormatStaticMid(b *testing.B) {
	doc := genStatic(400)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = Format(doc)
	}
}
