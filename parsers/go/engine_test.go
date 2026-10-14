package synx

import (
	"strings"
	"testing"
)

func TestEngineEnvDefault(t *testing.T) {
	r := Parse("!active\nport:env:default:3000 APP_PORT\n")
	Resolve(&r, Options{Env: map[string]string{"APP_PORT": "9090"}})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("port"); !Equal(v, Int(9090)) {
		t.Fatalf("env override missed: %v", v)
	}
}

func TestEngineEnvFallback(t *testing.T) {
	r := Parse("!active\nport:env:default:3000 NOT_SET\n")
	Resolve(&r, Options{Env: map[string]string{}})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("port"); !Equal(v, Int(3000)) {
		t.Fatalf("fallback missed: %v", v)
	}
}

func TestEngineCalc(t *testing.T) {
	r := Parse("!active\nprice 100\ntax:calc price * 0.2\n")
	Resolve(&r, Options{})
	o, _ := AsObject(r.Root)
	v, _ := o.Get("tax")
	d, ok := AsNumber(v)
	if !ok || d < 19.9 || d > 20.1 {
		t.Fatalf("calc result wrong: %v ok=%v", d, ok)
	}
}

func TestEngineSecretRedactedInJSON(t *testing.T) {
	r := Parse("!active\ntoken:secret abc123\n")
	Resolve(&r, Options{})
	j := ToJSON(r.Root)
	if !strings.Contains(j, "[SECRET]") {
		t.Fatalf("redaction missing: %q", j)
	}
	if strings.Contains(j, "abc123") {
		t.Fatalf("secret leaked: %q", j)
	}
}

func TestEngineClamp(t *testing.T) {
	r := Parse("!active\nx:clamp:0:10 99\n")
	Resolve(&r, Options{})
	o, _ := AsObject(r.Root)
	v, _ := o.Get("x")
	d, _ := AsNumber(v)
	if d != 10 {
		t.Fatalf("clamp wrong: %v", d)
	}
}

func TestEngineFormatPadded(t *testing.T) {
	r := Parse("!active\nnum:format:%05d 42\n")
	Resolve(&r, Options{})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("num"); !Equal(v, String("00042")) {
		t.Fatalf("format wrong: %v", v)
	}
}

func resolveWithEnv(src string, env map[string]string) *Object {
	r := Parse(src)
	Resolve(&r, Options{Env: env})
	o, _ := AsObject(r.Root)
	return o
}

func TestEngineEnvTypeStringConstraintKeepsText(t *testing.T) {
	env := map[string]string{"ZIP": "007", "SNOW": "1550184245153112174"}
	o := resolveWithEnv("!active\nid[type:string]:env SNOW\nzip[type:string]:env ZIP\nfallback[type:string]:env:default:0042 MISSING\nplain:env ZIP\n", env)
	want := map[string]Value{
		"id":       String("1550184245153112174"),
		"zip":      String("007"),
		"fallback": String("0042"),
		"plain":    Int(7),
	}
	for k, w := range want {
		if v, _ := o.Get(k); !Equal(v, w) {
			t.Errorf("%s = %v, want %v", k, v, w)
		}
	}
}

func TestEngineTypeStringConstraintAcceptsSecret(t *testing.T) {
	o := resolveWithEnv("!active\ntoken[type:string]:secret:env SNOW\n", map[string]string{"SNOW": "1550184245153112174"})
	v, _ := o.Get("token")
	if s, ok := v.(SecretValue); !ok || s.V != "1550184245153112174" {
		t.Fatalf("token = %#v, want the resolved value wrapped as a secret", v)
	}
}

func TestEngineEnvKeeps64BitIntegerExact(t *testing.T) {
	o := resolveWithEnv("!active\nraw:env SNOW\nq:env Q\n", map[string]string{"SNOW": "1550184245153112174", "Q": `"12"`})
	if v, _ := o.Get("raw"); !Equal(v, Int(1550184245153112174)) {
		t.Errorf("raw = %v, want exact int64", v)
	}
	if v, _ := o.Get("q"); !Equal(v, String("12")) {
		t.Errorf("q = %v, want quotes stripped", v)
	}
}

func TestEngineDefaultTypeStringKeepsText(t *testing.T) {
	o := resolveWithEnv("!active\na[type:string]:default:007 null\nb:default:007 null\n", map[string]string{})
	for k, w := range map[string]Value{"a": String("007"), "b": Int(7)} {
		if v, _ := o.Get(k); !Equal(v, w) {
			t.Errorf("%s = %v, want %v", k, v, w)
		}
	}
}

func TestEngineEnvDefaultRejoinsColons(t *testing.T) {
	o := resolveWithEnv("!active\nhost:env:default:0.0.0.0 MISSING\n", map[string]string{})
	if v, _ := o.Get("host"); !Equal(v, String("0.0.0.0")) {
		t.Fatalf("host = %v, want 0.0.0.0", v)
	}
}

// Reference output (Rust `synx parse`, env ZIP=007 SNOW=1550184245153112174 Q="12").
func TestEngineEnvStringMatrixMatchesReference(t *testing.T) {
	src := "!active\nz:env ZIP\nzs[type:string]:env ZIP\nid[type:string]:env SNOW\nraw:env SNOW\nd[type:string]:env:default:007 MISSING\nsec[type:string]:secret:env SNOW\nq:env Q\n"
	o := resolveWithEnv(src, map[string]string{"ZIP": "007", "SNOW": "1550184245153112174", "Q": `"12"`})
	want := `{"d":"007","id":"1550184245153112174","q":"12","raw":1550184245153112174,"sec":"[SECRET]","z":7,"zs":"007"}`
	if got := ToJSON(Object_(o)); got != want {
		t.Fatalf("got  %s\nwant %s", got, want)
	}
}

func TestEngineInterpolationMasksSecret(t *testing.T) {
	r := Parse("!active\ntoken:secret abc123\nmsg hello {token} world\nsection\n  inner use {token} now\n")
	Resolve(&r, Options{})
	j := ToJSON(r.Root)
	if strings.Contains(j, "abc123") {
		t.Fatalf("secret leaked: %s", j)
	}
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("msg"); !Equal(v, String("hello [SECRET] world")) {
		t.Errorf("msg = %v", v)
	}
	sec, _ := o.Get("section")
	so, _ := AsObject(sec)
	if v, _ := so.Get("inner"); !Equal(v, String("use [SECRET] now")) {
		t.Errorf("section.inner = %v", v)
	}
}

func TestEngineInterpolationKeepsNonASCII(t *testing.T) {
	r := Parse("!active\nname Wario\nmsg мир {name} 東京\n")
	Resolve(&r, Options{})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("msg"); !Equal(v, String("мир Wario 東京")) {
		t.Fatalf("msg = %v", v)
	}
}

func TestEngineInterpolationNested(t *testing.T) {
	r := Parse("!active\nserver\n  host localhost\n  port 8080\nurl http://{server.host}:{server.port}/api\nloose { server.host } {nope}\n")
	Resolve(&r, Options{})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("url"); !Equal(v, String("http://localhost:8080/api")) {
		t.Errorf("url = %v", v)
	}
	if v, _ := o.Get("loose"); !Equal(v, String("{ server.host } {nope}")) {
		t.Errorf("loose = %v", v)
	}
}

func TestEngineEnvNilReadsProcessEnvironment(t *testing.T) {
	t.Setenv("SYNX_TEST_PROCESS_ENV_PORT", "9191")
	src := "!active\nport:env:default:3000 SYNX_TEST_PROCESS_ENV_PORT\n"
	r := Parse(src)
	Resolve(&r, Options{})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("port"); !Equal(v, Int(9191)) {
		t.Fatalf("nil Env should read the process environment, got %v", v)
	}
	// An explicit empty map means "no variables": the default wins.
	o = resolveWithEnv(src, map[string]string{})
	if v, _ := o.Get("port"); !Equal(v, Int(3000)) {
		t.Fatalf("empty Env map must hide process variables, got %v", v)
	}
}

func TestEngineNestedQuantifierPatternIsSkipped(t *testing.T) {
	src := "!active\nv[pattern:^(a+)+$] " + strings.Repeat("a", 45) + "!\nw[pattern:^(a|b)*c$] abab\n"
	o := resolveWithEnv(src, map[string]string{})
	if v, _ := o.Get("v"); !Equal(v, String(strings.Repeat("a", 45)+"!")) {
		t.Errorf("v = %v, want the value unchanged", v)
	}
	if v, _ := o.Get("w"); !Equal(v, String("CONSTRAINT_ERR: 'w' does not match pattern /^(a|b)*c$/")) {
		t.Errorf("w = %v", v)
	}
}

func TestEngineConstraintMessagesNumbers(t *testing.T) {
	o := resolveWithEnv("!active\nf[min:0.1] 0.05\nbig[max:70000] 70001\n", map[string]string{})
	if v, _ := o.Get("f"); !Equal(v, String("CONSTRAINT_ERR: 'f' value 0.05 is below min 0.1")) {
		t.Errorf("f = %v", v)
	}
	if v, _ := o.Get("big"); !Equal(v, String("CONSTRAINT_ERR: 'big' value 70001 exceeds max 70000")) {
		t.Errorf("big = %v", v)
	}
}

func TestEngineFieldRegistryMergesDeclarations(t *testing.T) {
	o := resolveWithEnv("!active\na\n  port[min:10, max:100] 50\nb\n  port[min:60] 70\nc\n  port 55\n", map[string]string{})
	for _, k := range []string{"a", "c"} {
		sub, _ := o.Get(k)
		so, _ := AsObject(sub)
		if v, _ := so.Get("port"); !Equal(v, String("CONSTRAINT_ERR: 'port' value 50 is below min 60")) && k == "a" {
			t.Errorf("a.port = %v", v)
		}
		if v, _ := so.Get("port"); k == "c" && !Equal(v, String("CONSTRAINT_ERR: 'port' value 55 is below min 60")) {
			t.Errorf("c.port = %v", v)
		}
	}
}
