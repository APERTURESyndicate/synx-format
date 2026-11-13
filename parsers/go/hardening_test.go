package synx

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"encoding/xml"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"runtime/debug"
	"strings"
	"testing"
	"time"
)

// Inputs that ended the process, hung it, printed a secret or disagreed with
// synx-core. Every expected JSON string is the reference CLI's output for the
// same document.

func activeJSON(t *testing.T, src, base string) string {
	t.Helper()
	r := Parse(src)
	Resolve(&r, Options{Env: map[string]string{}, BasePath: base})
	return ToJSON(r.Root)
}

// ─── secrets ────────────────────────────────────────────────────────────────

func TestSecretIsPrintedNowhere(t *testing.T) {
	root := ParseActive("!active\npassword:secret hunter2\nport 8080\nlist\n  - a\n", Options{Env: map[string]string{}})
	secret, _ := root.Get("password")
	if s, ok := AsSecret(secret); !ok || s != "hunter2" {
		t.Fatalf("AsSecret = %q, %v", s, ok)
	}
	type holder struct {
		Name  string
		Value Value
	}
	type xmlHolder struct{ P SecretValue }
	printed := []string{
		fmt.Sprint(secret), fmt.Sprintf("%v", secret), fmt.Sprintf("%+v", secret), fmt.Sprintf("%#v", secret),
		fmt.Sprintf("%s", secret), fmt.Sprintf("%q", secret), fmt.Sprintf("%x", secret), fmt.Sprintf("%d", secret),
		fmt.Sprintf("%v", &secret), fmt.Sprintf("%v", []Value{secret}), fmt.Sprintf("%+v", map[string]Value{"k": secret}),
		fmt.Sprintf("%+v", holder{Name: "n", Value: secret}),
		fmt.Sprint(root), fmt.Sprintf("%v", root), fmt.Sprintf("%+v", root), fmt.Sprintf("%#v", root), fmt.Sprintf("%s", root),
		fmt.Sprintf("%v", Object_(root)), fmt.Sprintf("%+v", root.Pairs()),
		fmt.Errorf("config: %v", root).Error(),
		root.String(), ToJSON(Object_(root)), Stringify(Object_(root)),
	}
	for _, enc := range []func() ([]byte, error){
		func() ([]byte, error) { return json.Marshal(secret) },
		func() ([]byte, error) { return json.Marshal(root.Pairs()) },
		func() ([]byte, error) { return json.Marshal(holder{Value: secret}) },
		func() ([]byte, error) { return xml.Marshal(xmlHolder{secret.(SecretValue)}) },
	} {
		out, err := enc()
		if err != nil {
			t.Fatal(err)
		}
		printed = append(printed, string(out))
	}
	for i, s := range printed {
		if strings.Contains(s, "hunter2") {
			t.Errorf("output %d shows the secret: %s", i, s)
		}
	}
	if got := fmt.Sprint(secret); got != "[SECRET]" {
		t.Errorf("Sprint(secret) = %q", got)
	}
	if got := fmt.Sprint(root); got != `{"list":["a"],"password":"[SECRET]","port":8080}` {
		t.Errorf("Sprint(root) = %s", got)
	}
}

func TestUnsetSecretIsNullAndRequiredNoticesIt(t *testing.T) {
	root := ParseActive("!active\ns:secret hunter2\nu:secret:env SYNX_UNSET_Q\nr[required]:secret:env SYNX_UNSET_Q\ng[required]\n",
		Options{Env: map[string]string{}})
	want := `{"g":"CONSTRAINT_ERR: 'g' is required","r":"CONSTRAINT_ERR: 'r' is required","s":"[SECRET]","u":null}`
	if got := ToJSON(Object_(root)); got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
	if u, _ := root.Get("u"); !IsNull(u) {
		t.Fatalf("u = %#v, want null", u)
	}
}

// ─── parity with synx-core: secrets in order, list items, lists ─────────────

func TestParityWithCore(t *testing.T) {
	dir := t.TempDir()
	for _, c := range parityCases {
		if got := activeJSON(t, c.input, dir); got != c.want {
			t.Errorf("%s\n got: %s\nwant: %s", c.name, got, c.want)
		}
	}
}

func TestSecretIncludeIsMaskedInTheAliasScopeToo(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{"creds.synx": "password hunter2\nuser bob\n"})
	got := activeJSON(t, "!active\ndb:secret:include creds.synx\nx {password:db}\ny {password:include}\nz {db.password}\n", dir)
	want := `{"db":{"password":"[SECRET]","user":"[SECRET]"},"x":"[SECRET]","y":"[SECRET]","z":"[SECRET]"}`
	if got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
}

func TestListCapIsReportedAsTruncation(t *testing.T) {
	var sb strings.Builder
	sb.WriteString("l\n")
	for i := 0; i < MaxListItems+3; i++ {
		sb.WriteString("  - x\n")
	}
	r := Parse(sb.String())
	arr, _ := AsArray(r.Root.(ObjectValue).V.GetOr("l", Null))
	if len(arr) != MaxListItems || !r.Truncated {
		t.Fatalf("items %d, truncated %v", len(arr), r.Truncated)
	}
}

// ─── :calc ──────────────────────────────────────────────────────────────────

func TestCalcParenthesesNeverUseTheCallStack(t *testing.T) {
	// The runtime ends the process when a goroutine's stack passes its limit;
	// with the limit lowered, the recursive parser died on this expression.
	defer debug.SetMaxStack(debug.SetMaxStack(4 << 20))
	done := make(chan CalcResult, 2)
	go func() {
		done <- SafeCalc(strings.Repeat("(", 200000) + "7" + strings.Repeat(")", 200000))
		done <- SafeCalc(strings.Repeat("(", 200000) + "1")
	}()
	if r := <-done; !r.OK || r.Value != 7 {
		t.Fatalf("%+v", r)
	}
	if r := <-done; r.OK || r.Error != "SYNX :calc — missing closing parenthesis" {
		t.Fatalf("%+v", r)
	}
	expectJSON(t, "!active\nbase 5\nz:ref:calc:+"+strings.Repeat("(", 20000)+"1"+strings.Repeat(")", 20000)+" base\n", `{"base":5,"z":6}`)
}

func TestCalcWithoutRecursionGivesTheSameAnswers(t *testing.T) {
	expectJSON(t, "!active\na:calc ((1+2)*3)\nb:calc -(2)\nc:calc (1 + 2\nd:calc 2 * (3 + 4) - 5 % 3\ne:calc 1 / (2 - 2)\nf:calc ()\n"+
		"g:calc ((((((((((1))))))))))\nh:calc 2 * (3\ni:calc (2))\n",
		`{"a":9,"b":"CALC_ERR: SYNX :calc — invalid number: '-'","c":"CALC_ERR: SYNX :calc — missing closing parenthesis",`+
			`"d":12,"e":"CALC_ERR: SYNX :calc — division by zero","f":"CALC_ERR: SYNX :calc — unexpected token: ')'","g":1,`+
			`"h":"CALC_ERR: SYNX :calc — missing closing parenthesis","i":"CALC_ERR: SYNX :calc — unexpected token at position 3"}`)
}

// ─── files ──────────────────────────────────────────────────────────────────

func TestIncludeCycleStopsAtItsFirstRepeat(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{
		// Three keys including the file they are in: 3^16 resolutions under the depth cap alone.
		"self.synx": "!active\na:include self.synx\nb:include self.synx\nc:include self.synx\nv 1\n",
		"ping.synx": "!active\nnext:include pong.synx\np 1\n",
		"pong.synx": "!active\nnext:include ping.synx\nq 2\n",
	})
	started := time.Now()
	level := `{"a":"INCLUDE_ERR: circular include: 'self.synx'","b":"INCLUDE_ERR: circular include: 'self.synx'","c":"INCLUDE_ERR: circular include: 'self.synx'","v":1}`
	got := activeJSON(t, "!active\na:include self.synx\nb:include self.synx\nc:include self.synx\n", dir)
	if want := `{"a":` + level + `,"b":` + level + `,"c":` + level + `}`; got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
	if took := time.Since(started); took > 3*time.Second {
		t.Fatalf("took %v", took)
	}
	for i := 0; i < 2; i++ { // the same document twice: nothing is left open
		got = activeJSON(t, "!active\nx:include ping.synx\n", dir)
		if want := `{"x":{"next":{"next":"INCLUDE_ERR: circular include: 'ping.synx'","q":2},"p":1}}`; got != want {
			t.Fatalf("\n got: %s\nwant: %s", got, want)
		}
	}
}

func TestOneDocumentReadsAtMost1024IncludedFiles(t *testing.T) {
	dir := t.TempDir()
	var body strings.Builder
	body.WriteString("!active\n")
	for i := 0; i < 1100; i++ {
		if err := os.WriteFile(filepath.Join(dir, fmt.Sprintf("inc%d.synx", i)), []byte(fmt.Sprintf("v %d\n", i)), 0o644); err != nil {
			t.Fatal(err)
		}
		fmt.Fprintf(&body, "k%04d:include inc%d.synx\n", i, i)
	}
	root := ParseActive(body.String(), Options{Env: map[string]string{}, BasePath: dir})
	if got := ToJSON(root.GetOr("k1023", Null)); got != `{"v":1023}` {
		t.Fatalf("k1023 = %s", got)
	}
	if got := ToJSON(root.GetOr("k1024", Null)); got != `"INCLUDE_ERR: include budget (1024 files) exceeded"` {
		t.Fatalf("k1024 = %s", got)
	}
	// The budget belongs to one document.
	if got := activeJSON(t, "!active\nk:include inc0.synx\n", dir); got != `{"k":{"v":0}}` {
		t.Fatalf("second document: %s", got)
	}
}

func TestPackagesThatUseEachOtherDoNotRecurseForever(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{
		"synx_packages/@a/one/src/main.synx": "!active\n!use @a/two\nv 1\n",
		"synx_packages/@a/two/src/main.synx": "!active\n!use @a/one\nw 2\n",
	})
	r := Parse("!active\n!use @a/one\nx 1\n")
	Resolve(&r, Options{Env: map[string]string{}, BasePath: dir, PackagesPath: filepath.Join(dir, "synx_packages")})
	if got, want := ToJSON(r.Root), `{"one":{"two":{"w":2},"v":1},"x":1}`; got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
}

// linkDirectory makes a directory link that needs no privilege: a junction on
// Windows, a symlink elsewhere.
func linkDirectory(link, target string) bool {
	if runtime.GOOS == "windows" {
		return exec.Command("cmd", "/C", "mklink", "/J", link, target).Run() == nil
	}
	return os.Symlink(target, link) == nil
}

func TestLinkInsideTheBaseDoesNotLeadOutOfIt(t *testing.T) {
	root := t.TempDir()
	base := filepath.Join(root, "base")
	outside := filepath.Join(root, "outside")
	writeFiles(t, root, map[string]string{"base/sub/part.synx": "v 1\n", "outside/secret.synx": "token OUTSIDE\n"})
	link := filepath.Join(base, "link")
	if !linkDirectory(link, outside) {
		t.Skip("cannot create a directory link here")
	}
	defer os.Remove(link) // the link itself, before the temp dir is swept through it
	got := activeJSON(t, "!active\na:include link/secret.synx\nb:watch link/secret.synx\nc:fallback:none link/secret.synx\nd:include sub/part.synx\n", base)
	want := `{"a":"INCLUDE_ERR: SECURITY: path escapes base directory: 'link/secret.synx'",` +
		`"b":"WATCH_ERR: SECURITY: path escapes base directory: 'link/secret.synx'","c":"none","d":{"v":1}}`
	if got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
}

func TestFileErrorsCarryNoHostPathAndReadTheSameEverywhere(t *testing.T) {
	dir := t.TempDir()
	writeFiles(t, dir, map[string]string{"sub/part.synx": "v 1\n", "bad.synx": "v \xff\xfe\n"})
	long := strings.Repeat("x", 300) + ".synx"
	got := activeJSON(t, "!active\na:include nope.synx\nd:include sub\ne:include .\nf:include "+long+"\ng:include sub/../sub/part.synx\n"+
		"h:include sub/../nope.synx\nk:watch sub\nl:watch nope.json\nu:include bad.synx\n", dir)
	want := `{"a":"INCLUDE_ERR: file not found: nope.synx","d":"INCLUDE_ERR: not a file: sub","e":"INCLUDE_ERR: not a file: .",` +
		`"f":"INCLUDE_ERR: file not found: ` + long + `","g":{"v":1},` +
		`"h":"INCLUDE_ERR: SECURITY: path traversal detected: 'sub/../nope.synx'","k":"WATCH_ERR: not a file: sub",` +
		`"l":"WATCH_ERR: file not found: nope.json","u":"INCLUDE_ERR: file is not valid UTF-8: bad.synx"}`
	if runtime.GOOS != "windows" {
		// A name longer than the file system holds is "not found" only where the
		// reference says so; elsewhere it is an unreadable file.
		want = strings.Replace(want, `"f":"INCLUDE_ERR: file not found: `, `"f":"INCLUDE_ERR: cannot read file: `, 1)
	}
	if got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
	if strings.Contains(got, filepath.Base(dir)) {
		t.Fatalf("the host path is in the output: %s", got)
	}

	for _, name := range []string{"NUL", "nul", "con", "CON.txt", "sub/COM1", "lpt9.log", "aux", "PRN", "nul:x", "CONIN$"} {
		if !hasDeviceName(name) {
			t.Errorf("%q is a device name", name)
		}
	}
	for _, name := range []string{"null.synx", "console.synx", "com.synx", "sub/part.synx", "COM10", "lpt"} {
		if hasDeviceName(name) {
			t.Errorf("%q is not a device name", name)
		}
	}
	if runtime.GOOS == "windows" {
		got := activeJSON(t, "!active\ni:include NUL\nj:include con.txt\nm:watch sub/COM1\n", dir)
		want := `{"i":"INCLUDE_ERR: file not found: NUL","j":"INCLUDE_ERR: file not found: con.txt","m":"WATCH_ERR: file not found: sub/COM1"}`
		if got != want {
			t.Fatalf("\n got: %s\nwant: %s", got, want)
		}
	}

	// A base directory that does not exist jails nothing, so nothing is read through it.
	gone := filepath.Join(dir, "gone")
	got = activeJSON(t, "!active\nx:include part.synx\n", gone)
	if want := ToJSON(Object_(func() *Object {
		o := NewObjectMap()
		o.Set("x", String("INCLUDE_ERR: SECURITY: base directory not found: '"+gone+"'"))
		return o
	}())); got != want {
		t.Fatalf("\n got: %s\nwant: %s", got, want)
	}
}

// ─── numbers ────────────────────────────────────────────────────────────────

func TestRoundSortSumAndOutsideNumbersFollowCore(t *testing.T) {
	expectJSON(t, "!active\nx(float):round nan\ny(float):round inf\nz:round:99999999999 1.5\nv:round:-1 5\nw:round 1e30\nq:round abc\nr:round 2.5\ns:round -2.5\n"+
		"t:round:2 1.005\nu:round 1e18\nbig:round 9223372036854775807\n",
		`{"big":9.223372036854776e18,"q":"abc","r":3,"s":-3,"t":1.0,"u":"1e18","v":5,"w":"1e30","x":null,"y":null,"z":2}`)

	expectJSON(t, "!active\ns:sort\n  - b\n  - 10\n  - 9\n  - 1a\n  - a\n  - 2.5\n  - true\n  - null\n  - B\nd:sort:desc\n  - b\n  - 10\n  - 9\n  - 1a\n  - 2.5\n"+
		"sum:sum\n  - 1\n  - inf\n  - nan\n  - 2.5\n  - 1e400\nhost:env:default:inf SYNX_PROBE_UNSET\nn:env:default:NaN SYNX_PROBE_UNSET\n"+
		"big:env:default:1e400 SYNX_PROBE_UNSET\nok:env:default:1e3 SYNX_PROBE_UNSET\nparts:split inf,nan,1,2.5\n"+
		"g[min:1e400] 5\nh[max:-inf] 5\ni[min:inf] 5\nj[max:nan] 5\nk[min:3] 2\n",
		`{"big":"1e400","d":["b","1a",10,9,2.5],"g":5,"h":5,"host":"inf","i":5,"j":5,`+
			`"k":"CONSTRAINT_ERR: 'k' value 2 is below min 3","n":"NaN","ok":1000.0,"parts":["inf","nan",1,2.5],`+
			`"s":[2.5,9,10,"1a","B","a","b",null,true],"sum":3.5}`)

	// A weight is digits[.digits]; anything else is the value.
	expectJSON(t, "!active\na:random inf\nb:random -5\nc:random 1e2\nd:random .5\ne:random +3\nf:random nan\n",
		`{"a":"inf","b":-5,"c":"1e2","d":".5","e":"+3","f":"nan"}`)
}

// ─── .synxb ─────────────────────────────────────────────────────────────────

func testVarint(n uint64) []byte {
	var buf bytes.Buffer
	encodeVarint(&buf, n)
	return buf.Bytes()
}

func testTable(strs ...string) []byte {
	out := testVarint(uint64(len(strs)))
	for _, s := range strs {
		out = append(out, testVarint(uint64(len(s)))...)
		out = append(out, s...)
	}
	return out
}

func testContainer(t *testing.T, payload []byte, declared int) []byte {
	t.Helper()
	packed, err := deflateRaw(payload)
	if err != nil {
		t.Fatal(err)
	}
	if declared < 0 {
		declared = len(payload)
	}
	head := append([]byte("SYNXB"), 1, 0, 0, 0, 0, 0)
	binary.LittleEndian.PutUint32(head[7:], uint32(declared))
	return append(head, packed...)
}

func refusal(t *testing.T, file []byte) string {
	t.Helper()
	_, err := Decompile(file)
	if err == nil {
		t.Fatal("the file was accepted")
	}
	return err.Error()
}

func TestSynxbHeaderAloneReservesNothing(t *testing.T) {
	// 13 bytes declaring 256 MiB: the buffer used to be reserved before a byte was inflated.
	file := append([]byte("SYNXB"), 1, 0, 0, 0, 0, 0x10, 0x03, 0x00)
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	msg := refusal(t, file)
	runtime.ReadMemStats(&after)
	if msg != "size mismatch: expected 268435456, got 0" {
		t.Fatalf("refusal: %s", msg)
	}
	if grew := after.TotalAlloc - before.TotalAlloc; grew > 4<<20 {
		t.Fatalf("allocated %d bytes", grew)
	}
}

func TestSynxbBombsAreRefusedByTheDecodeBudget(t *testing.T) {
	const want = "decoded document exceeds 268435456 bytes — refusing to decode"
	// Eight million nulls: 8 KiB of file.
	nulls := make([]byte, 8<<20)
	payload := append(append(testTable(), tagArray), testVarint(uint64(len(nulls)))...)
	if msg := refusal(t, testContainer(t, append(payload, nulls...), -1)); msg != want {
		t.Fatalf("nulls: %s", msg)
	}
	// One 120 KB string referenced 6000 times: 720 MB of text from 200 bytes of file.
	payload = append(append(testTable(strings.Repeat("x", 120000)), tagArray), testVarint(6000)...)
	for i := 0; i < 6000; i++ {
		payload = append(payload, tagString, 0)
	}
	if msg := refusal(t, testContainer(t, payload, -1)); msg != want {
		t.Fatalf("references: %s", msg)
	}
	// A document within the budget still reads.
	payload = append(append(testTable("k", "v"), tagObject), 1, 0, tagString, 1)
	if text, err := DecompileToText(testContainer(t, payload, -1)); err != nil || text != "k v\n" {
		t.Fatalf("%q, %v", text, err)
	}
}

func TestSynxbLengthsAndIndexesAreChecked(t *testing.T) {
	cases := []struct {
		payload []byte
		want    string
	}{
		{append(append(testVarint(1), testVarint(^uint64(0))...), 1), "unexpected end of data in string table"},
		{append(append(testTable("zero", "k"), tagObject, 1, 1, tagString), testVarint(1<<32)...), "string index out of range"},
	}
	for _, c := range cases {
		if msg := refusal(t, testContainer(t, c.payload, -1)); msg != c.want {
			t.Errorf("got %q, want %q", msg, c.want)
		}
	}
	// A stream that inflates past what the header declares stops one byte over.
	if msg := refusal(t, testContainer(t, make([]byte, 8<<20), 1024)); msg != "size mismatch: expected 1024, got 1025" {
		t.Errorf("declared small: %s", msg)
	}
}

// B-076: what stands under a refused key goes with it.
func TestTheBlockOfARefusedKeyIsDropped(t *testing.T) {
	r := Parse("user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n")
	if got := ToJSON(r.Root); got != `{"user":{"age":3,"profile":{"name":"x"}}}` {
		t.Fatalf("got %s", got)
	}
	r = Parse("a 1\nprototype |\n  !active\n  b:env HOME\n  ###\nc 2\n")
	if r.Mode != ModeStatic || ToJSON(r.Root) != `{"a":1,"c":2}` {
		t.Fatalf("mode %v, %s", r.Mode, ToJSON(r.Root))
	}
}
