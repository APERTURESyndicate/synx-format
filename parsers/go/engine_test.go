package synx

import (
	"strings"
	"testing"
)

func TestEngineEnvDefault(t *testing.T) {
	r := Parse("!active\nport:env:default:3000 APP_PORT\n")
	Resolve(&r, Options{Env: map[string]string{"APP_PORT": "9090"}})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("port"); !Equal(v, String("9090")) {
		t.Fatalf("env override missed: %v", v)
	}
}

func TestEngineEnvFallback(t *testing.T) {
	r := Parse("!active\nport:env:default:3000 NOT_SET\n")
	Resolve(&r, Options{Env: map[string]string{}})
	o, _ := AsObject(r.Root)
	if v, _ := o.Get("port"); !Equal(v, String("3000")) {
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
