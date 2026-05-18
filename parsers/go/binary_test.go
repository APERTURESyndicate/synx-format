package synx

import "testing"

func TestBinaryStaticRoundtrip(t *testing.T) {
	r := Parse("name App\nport 8080\n")
	bytes, err := Compile(r, false)
	if err != nil {
		t.Fatalf("compile: %v", err)
	}
	if !IsSynxb(bytes) {
		t.Fatalf("magic missing")
	}
	restored, err := Decompile(bytes)
	if err != nil {
		t.Fatalf("decompile: %v", err)
	}
	if !Equal(r.Root, restored.Root) {
		t.Fatalf("roundtrip mismatch")
	}
}

func TestBinaryMagicCheck(t *testing.T) {
	if !IsSynxb([]byte{'S', 'Y', 'N', 'X', 'B', 1, 0}) {
		t.Fatalf("magic mismatch")
	}
	if IsSynxb([]byte{'J', 'S', 'O', 'N'}) {
		t.Fatalf("false positive")
	}
}

func TestBinaryInvalidMagicRejected(t *testing.T) {
	bad := make([]byte, 11)
	copy(bad, []byte("WRONG"))
	if _, err := Decompile(bad); err == nil {
		t.Fatalf("expected error")
	}
}
