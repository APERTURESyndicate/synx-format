package synx

import (
	"bytes"
	"compress/flate"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

// Regression tests for the SYNX 3.8.0 defects (CHANGELOG [3.8.0]). Every
// expected JSON string is the output of the reference CLI (`synx parse`) for
// the same document.

func resolvedJSON(t *testing.T, src string) string {
	t.Helper()
	r := Parse(src)
	if r.Mode == ModeActive {
		Resolve(&r, Options{Env: map[string]string{}})
	}
	return ToJSON(r.Root)
}

func expectJSON(t *testing.T, src, want string) {
	t.Helper()
	if got := resolvedJSON(t, src); got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
}

func TestReleaseCalcFollowsDocumentOrder(t *testing.T) {
	for i := 0; i < 20; i++ {
		expectJSON(t, "!active\nprice 100\ntax:calc price * 0.2\ntotal:calc price + tax\n",
			`{"price":100,"tax":20,"total":120}`)
	}
}

func TestReleaseCalcLocalShadowsRootAndSpecialKeys(t *testing.T) {
	expectJSON(t, "!active\nlimit 5\nsvc\n  limit 100\n  price:calc limit * 2\nmax-hp 10\nhp 3\nleft:calc max-hp - hp\n",
		`{"hp":3,"left":7,"limit":5,"max-hp":10,"svc":{"limit":100,"price":200}}`)
}

func TestReleaseCalcErrorsMatchReference(t *testing.T) {
	expectJSON(t, "!active\nbad:calc 1.2.3\nlone:calc -\nuni:calc цена + 1\ndiv:calc 1 / 0\nex:calc (1 + 2\nf:calc 0.1 + 0.2\nbig:calc 1e3\n",
		`{"bad":"CALC_ERR: SYNX :calc — invalid number: '1.2.3'","big":"CALC_ERR: SYNX :calc — unexpected character: 'e' in expression",`+
			`"div":"CALC_ERR: SYNX :calc — division by zero","ex":"CALC_ERR: SYNX :calc — missing closing parenthesis","f":0.30000000000000004,`+
			`"lone":"CALC_ERR: SYNX :calc — invalid number: '-'","uni":"CALC_ERR: SYNX :calc — unexpected character: 'ц' in expression"}`)
}

func TestReleaseNestedMarkerSeesParentKeysAbove(t *testing.T) {
	expectJSON(t, "!active\na:calc 2 * 3\nb\n  x {a}\n", `{"a":6,"b":{"x":"6"}}`)
}

func TestReleaseListItemsKeepTheirOwnMarkers(t *testing.T) {
	expectJSON(t, "!active\nservers\n  - name a\n    port:env:default:7001 SYNX_UNSET_Z\n  - name b\n    port[max:9] 8080\n",
		`{"servers":[{"name":"a","port":"CONSTRAINT_ERR: 'port' value 7001 exceeds max 9"},{"name":"b","port":"CONSTRAINT_ERR: 'port' value 8080 exceeds max 9"}]}`)
	r := Parse("!active\nservers\n  - name a\n    port:env:default:7001 P\n")
	if _, ok := r.Metadata.Get("servers.#0").Get("port"); !ok {
		t.Fatalf("list item metadata not filed under servers.#0: %v", r.Metadata.Paths())
	}
}

func TestReleaseInheritCarriesMarkersAndMergesParents(t *testing.T) {
	expectJSON(t, "!active\n_base\n  port:env:default:5 SYNX_UNSET_Z\n  weight 1\n  nested\n    q:calc 2 * 3\n_extra\n  weight 3\nmulti:inherit:_base:_extra\n  z 1\nlist:inherit:_base\n  - 1\nscalar:inherit:_base 5\n",
		`{"list":[1],"multi":{"nested":{"q":6},"port":5,"weight":3,"z":1},"scalar":{"nested":{"q":6},"port":5,"weight":1}}`)
}

func TestReleaseNullChecksOnlyEnum(t *testing.T) {
	expectJSON(t, "!active\nport[type:int]:env SYNX_UNSET_Z\nmode[enum:a|b]:env SYNX_UNSET_Z\nopt[enum:a|]:env SYNX_UNSET_Z\nlen[min:3] null\n",
		`{"len":null,"mode":"CONSTRAINT_ERR: 'mode' must be one of [a|b]","opt":null,"port":null}`)
}

func TestReleaseOverwrittenAndDottedDeclarations(t *testing.T) {
	expectJSON(t, "!active\nport\n  port[enum:a|b] x\nport\na.b\n  x:env:default:9 SYNX_UNSET_Z\na\n  b\n    x hello\n",
		`{"a":{"b":{"x":"hello"}},"a.b":{"x":9},"port":{}}`)
	r := Parse("!active\na.b\n  x:env Q\n")
	if _, ok := r.Metadata.Get(`a\.b`).Get("x"); !ok {
		t.Fatalf("dotted key not escaped in its metadata path: %v", r.Metadata.Paths())
	}
}

func TestReleaseValuesAsText(t *testing.T) {
	expectJSON(t, "!active\nn 0.0000001\nm 123456789012345678901234.0\nt v={n} m={m}\nf:format:%05d 12.0\ng:format:%d 9007199254740993\nh:format:%.3f \"1.5\"\nj:join\n  - 1.5\n  - 2.0\n  - true\n  - null\nobj\n  k 1\nk {obj}!\n",
		`{"f":"00012","g":"9007199254740993","h":"1.5","j":"1.5,2,true,null","k":"!","m":1.2345678901234569e23,"n":1e-7,"obj":{"k":1},"t":"v=0.0000001 m=123456789012345690000000"}`)
}

func TestReleaseSortVersionAndLengths(t *testing.T) {
	expectJSON(t, "!active\ns:sort\n  - b\n  - B\n  - ä\n  - a\n  - 10\n  - 9\nd:sort:desc\n  - 1\n  - 3\n  - 2\n",
		`{"d":[3,2,1],"s":[9,10,"B","a","b","ä"]}`)
	expectJSON(t, "!active\nshort[max:4] мир\nlong[max:4] мирок\nv1:version:>=:1.2.3 1.2.3-beta\n",
		`{"long":"CONSTRAINT_ERR: 'long' value 5 exceeds max 4","short":"мир","v1":true}`)
}

func TestReleaseUnicodeIndentation(t *testing.T) {
	expectJSON(t, "a\n  x 1\n　y 2\nb\n\tc 3\n d 4\n", `{"a":{"x":1,"y":2},"b":{"c":3,"d":4}}`)
	// `|+` strips the base indent by whole characters: two NBSPs stay.
	expectJSON(t, "v |+\n  line\n    deeper\n  x\n", "{\"v\":\"line\\n  deeper\\nx\"}")
	// A line of only U+00A0 is blank, also to look-ahead.
	expectJSON(t, "k\n  \n  - a\n", `{"k":["a"]}`)
}

func TestReleaseListItemFrames(t *testing.T) {
	expectJSON(t, "items\n  - name a\n    sub\n      k 1\n  - name b\n    v 2\nafter 1\n",
		`{"after":1,"items":[{"name":"a","sub":{"k":1}},{"name":"b","v":2}]}`)
	// An object opened between a list key and its first item is not the item's parent.
	expectJSON(t, "items:join\n x_1\n   y 1\n   - k v\n     z 2\n", `{"items":[{"k":"v","z":2}],"x_1":{"y":1}}`)
	// An item dropped because its list key was re-declared still pops the deeper frames
	// first, so `w` lands in `o`, not in `p`.
	expectJSON(t, "k\n  - x 1\n k 5\n o\n     p\n       q 1\n   - z\n    w 1\n", `{"k":5,"o":{"p":{"q":1},"w":1}}`)
}

func TestReleaseStringifyRoundTrips(t *testing.T) {
	src := "a \"007\"\nb \"true\"\ncode |+\n  if x\n    y()\nempty:join\nitems\n  - only one\n  - k v\n    sub\n      z 1\nf -0.0\n"
	r := Parse(src)
	r.Root.(ObjectValue).V.Set("c", String("x # y"))
	text := Stringify(r.Root)
	want := "a \"007\"\nb \"true\"\nc |+\n  x # y\ncode |+\n  if x\n    y()\nempty:join\nf -0.0\nitems\n  - only one\n  - k v\n    sub\n      z 1\n"
	if text != want {
		t.Fatalf("stringify\n got: %q\nwant: %q", text, want)
	}
	if ToJSON(Parse(text).Root) != ToJSON(r.Root) {
		t.Fatalf("round trip changed the data:\n%s", text)
	}
	one := Object_(objectOf("items", Array([]Value{Object_(objectOf("k", String("v")))})))
	if got := Stringify(one); got != "items\n  - k v\n    k v\n" {
		t.Fatalf("one-key item: %q", got)
	}
	sec := Object_(objectOf("token", Secret("abc")))
	if got := Stringify(sec); got != "token [SECRET]\n" {
		t.Fatalf("secret: %q", got)
	}
}

func objectOf(kv ...interface{}) *Object {
	o := NewObjectMap()
	for i := 0; i < len(kv); i += 2 {
		o.Set(kv[i].(string), kv[i+1].(Value))
	}
	return o
}

func TestReleaseFormatKeepsPreserveBlock(t *testing.T) {
	src := "code |+\n  zeta()\n    alpha()\n  beta\nname x\n"
	out := Format(src)
	if out != "code |+\n  zeta()\n    alpha()\n  beta\n\nname x\n" {
		t.Fatalf("format: %q", out)
	}
	if Format(out) != out {
		t.Fatalf("format is not idempotent")
	}
	// Equal keys keep document order, so the last one still wins.
	stable := Format("b 1\na first\nc 2\na second\n")
	if v, _ := ParseRoot(stable).Get("a"); !Equal(v, String("second")) {
		t.Fatalf("stable sort: %q", stable)
	}
}

func TestReleaseFormatDeepNestingTerminates(t *testing.T) {
	var sb strings.Builder
	for i := 0; i < 200; i++ {
		sb.WriteString(strings.Repeat(" ", i) + "k\n")
	}
	out := Format(sb.String())
	if !strings.HasPrefix(out, "k\n  k\n    k\n") || strings.Count(out, "k\n") > 200 {
		t.Fatalf("unexpected output: %q", out[:40])
	}
}

// Past depth 128 the formatter drops the deeper subtree and goes on with the
// next line at or above the cap — the output of `synx format` for this file.
func TestReleaseFormatDropsSubtreePastDepthCap(t *testing.T) {
	var src, want strings.Builder
	for i := 0; i < 140; i++ {
		src.WriteString(strings.Repeat(" ", i) + "k" + strconv.Itoa(i) + "\n")
	}
	src.WriteString("after 1\n  tail 2\nlast x\n")
	want.WriteString("after 1\n  tail 2\n\n")
	for i := 0; i <= 128; i++ {
		want.WriteString(strings.Repeat(" ", 2*i) + "k" + strconv.Itoa(i) + "\n")
	}
	want.WriteString("\nlast x\n")
	if got := Format(src.String()); got != want.String() {
		t.Fatalf("format past the depth cap:\n got %d lines\nwant %d lines", strings.Count(got, "\n"), strings.Count(want.String(), "\n"))
	}
}

func TestReleaseTruncatedIgnoresBOM(t *testing.T) {
	if Parse("\uFEFFa 1\n").Truncated {
		t.Fatalf("a leading BOM alone set Truncated")
	}
	if Parse("a 1\n").Truncated {
		t.Fatalf("Truncated set without any limit")
	}
}

func TestReleaseClampBoundsMustBeFinite(t *testing.T) {
	expectJSON(t, "!active\na:clamp:nan:5 9\nb:clamp:0:inf 9\nc:clamp:0:Infinity 9\nd:clamp:0:1e400 9\ne:clamp:0:5abc 9\nf:clamp:0:5 9\ng:clamp:-inf:5 9\nh:clamp:1e2:2e2 9\n",
		`{"a":9,"b":9,"c":9,"d":9,"e":9,"f":5,"g":9,"h":100}`)
}

func writeFiles(t *testing.T, root string, files map[string]string) {
	t.Helper()
	for name, content := range files {
		p := filepath.Join(root, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(p), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

// `!use` names and manifest `main` / `entry` paths stay inside the packages
// directory: a bad name skips the package, a bad `main` counts as absent.
func TestReleaseUseStaysInsidePackages(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{
		"outside/src/main.synx":                "leak yes\n",
		"outside2/x.synx":                      "leak2 yes\n",
		"base/synx_packages/@m/synx-pkg.synx":  "main ../../../outside2/x.synx\n",
		"base/synx_packages/@m/src/main.synx":  "fallback yes\n",
		"base/synx_packages/@e/synx-pkg.synx":  "main ../x\nentry lib/ok.synx\n",
		"base/synx_packages/@e/lib/ok.synx":    "entry yes\n",
		"base/synx_packages/@ok/src/main.synx": "plain yes\n",
	})
	src := "!active\n!use @a/../../../outside as p1\n!use @m as p2\n!use @e as p3\n!use @c:x as p4\n!use @ok as p5\napp ok\n"
	r := Parse(src)
	Resolve(&r, Options{BasePath: filepath.Join(dir, "base"), Env: map[string]string{}})
	want := `{"app":"ok","p2":{"fallback":"yes"},"p3":{"entry":"yes"},"p5":{"plain":"yes"}}`
	if got := ToJSON(r.Root); got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
	for _, p := range []string{"/abs", `\abs`, "C:x", "a/../b", `a\..\b`, ""} {
		if isContainedRelative(p) {
			t.Errorf("%q accepted as a contained relative path", p)
		}
	}
	for _, p := range []string{"@scope/name", "src/main.synx", "a/..b/c", "./x"} {
		if !isContainedRelative(p) {
			t.Errorf("%q refused", p)
		}
	}
}

// A file that includes itself stops at the include depth cap, and each level
// is resolved once: the `:include` marker reuses the pre-pass result.
func TestReleaseSelfIncludeStopsAtTheFirstRepeat(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{"self.synx": "!active\nme:include self.synx\nn:tick 1\n"})
	text, _ := os.ReadFile(filepath.Join(dir, "self.synx"))
	calls := 0
	r := Parse(string(text))
	Resolve(&r, Options{BasePath: dir, Env: map[string]string{}, MarkerFns: map[string]MarkerFn{
		"tick": func(key string, args []string, v Value) Value { calls++; return v },
	}})
	// The text handed to Resolve has no path; the file it includes is open
	// once, and including it again from inside itself is the cycle.
	got := ToJSON(r.Root)
	want := `{"me":{"me":"INCLUDE_ERR: circular include: 'self.synx'","n":1},"n":1}`
	if got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
	if calls != 2 {
		t.Fatalf("resolved %d documents, want 2", calls)
	}
}

// The pre-pass results are not include aliases: with one `:include` key,
// `{x:include}` still names that include.
func TestReleaseSingleIncludeKeyServesIncludeScope(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{"common.synx": "site_name Demo\n"})
	r := Parse("!active\ndb:include common.synx\ngreet {site_name:include} / {site_name:db}\n")
	Resolve(&r, Options{BasePath: dir, Env: map[string]string{}})
	if got, want := ToJSON(r.Root), `{"db":{"site_name":"Demo"},"greet":"Demo / Demo"}`; got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
}

func TestReleaseSynxbIsDeterministicAndOrderedByUse(t *testing.T) {
	src := "!active\nz rare\nlist\n  - common\n  - common\n  - common\nbig 9007199254740993\nneg -9000000000000000007\nport:env:default:1 P\n"
	r := Parse(src)
	first, err := Compile(r, false)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 5; i++ {
		again, _ := Compile(Parse(src), false)
		if !bytes.Equal(first, again) {
			t.Fatalf("compile is not deterministic")
		}
	}
	payload, err := io.ReadAll(flate.NewReader(bytes.NewReader(first[11:])))
	if err != nil {
		t.Fatal(err)
	}
	// String table: count, then the most used string ("common", 3 uses) first.
	if payload[1] != byte(len("common")) || string(payload[2:8]) != "common" {
		t.Fatalf("string table not ordered by use: % x", payload[:16])
	}
	back, err := Decompile(first)
	if err != nil {
		t.Fatal(err)
	}
	if ToJSON(back.Root) != ToJSON(r.Root) {
		t.Fatalf("round trip: %s", ToJSON(back.Root))
	}
	if !strings.Contains(ToJSON(back.Root), "9007199254740993") {
		t.Fatalf("big integer not exact")
	}
	if m, ok := back.Metadata.Get("").Get("port"); !ok || m.Markers[0] != "env" {
		t.Fatalf("metadata lost")
	}
}

func TestReleaseBlockCommentHidesDirectives(t *testing.T) {
	r := Parse("###\n!active\n###\nx:calc 1 + 1\n")
	if r.Mode != ModeStatic {
		t.Fatalf("a directive inside ### switched the mode")
	}
}

func TestReleaseSecretAndPromptAndAlias(t *testing.T) {
	expectJSON(t, "!active\ntoken:secret abc123\nmsg hello {token}\npick:alias token\na:alias b\nb:alias a\n",
		`{"a":"ALIAS_ERR: circular alias detected: a → b","b":"ALIAS_ERR: circular alias detected: a → b","msg":"hello [SECRET]","pick":"[SECRET]","token":"[SECRET]"}`)
}
