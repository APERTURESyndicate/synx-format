package synx

import "testing"

func TestCalcBasicOps(t *testing.T) {
	cases := []struct {
		expr string
		want float64
	}{
		{"2 + 3", 5},
		{"10 - 4", 6},
		{"3 * 7", 21},
		{"20 / 4", 5},
		{"10 % 3", 1},
	}
	for _, c := range cases {
		r := SafeCalc(c.expr)
		if !r.OK || r.Value != c.want {
			t.Fatalf("%q: ok=%v val=%v err=%q", c.expr, r.OK, r.Value, r.Error)
		}
	}
}

func TestCalcPrecedence(t *testing.T) {
	if r := SafeCalc("2 + 3 * 4"); r.Value != 14 {
		t.Fatalf("got %v", r.Value)
	}
	if r := SafeCalc("(2 + 3) * 4"); r.Value != 20 {
		t.Fatalf("got %v", r.Value)
	}
}

func TestCalcNegatives(t *testing.T) {
	if r := SafeCalc("-5 + 3"); r.Value != -2 {
		t.Fatalf("got %v", r.Value)
	}
	if r := SafeCalc("10 * -2"); r.Value != -20 {
		t.Fatalf("got %v", r.Value)
	}
}

func TestCalcDivZero(t *testing.T) {
	r := SafeCalc("10 / 0")
	if r.OK {
		t.Fatalf("expected error")
	}
	if r.Error == "" {
		t.Fatalf("missing error message")
	}
}

func TestCalcEmpty(t *testing.T) {
	r := SafeCalc("")
	if !r.OK || r.Value != 0 {
		t.Fatalf("empty calc failed: %+v", r)
	}
}
