// Package synx wraps the synx-c native library (Rust synx-core 3.6.x) via cgo.
// Requires: CGO_ENABLED=1, a C toolchain, and libsynx_c in the link path (see README).
package synx

/*
#cgo CFLAGS: -I${SRCDIR}/../c-header/include

// Default link for Linux/macOS (libsynx_c.so / libsynx_c.dylib next to repo target/release).
#cgo linux LDFLAGS: -L${SRCDIR}/../../target/release -lsynx_c
#cgo darwin LDFLAGS: -L${SRCDIR}/../../target/release -lsynx_c

// Windows (MSVC-built synx_c): MinGW ld must not pick synx_c.lib (static CRT). Link synx_c.dll.lib via CGO_LDFLAGS, e.g.
//   set CGO_LDFLAGS=path\\to\\target\\release\\synx_c.dll.lib
//   set CGO_LDFLAGS_ALLOW=.*
// Or build synx-c for the GNU target and use -lsynx_c as on Linux.

#include <stdlib.h>
#include <synx.h>
*/
import "C"

import (
	"errors"
	"unsafe"
)

// ErrSynx indicates the native engine rejected input or hit an internal error (NULL from FFI).
var ErrSynx = errors.New("synx: native operation failed")

func takeString(p *C.char) (string, error) {
	if p == nil {
		return "", ErrSynx
	}
	out := C.GoString(p)
	C.synx_free(p)
	return out, nil
}

// Parse parses SYNX text to canonical JSON (static mode).
func Parse(input string) (string, error) {
	cs := C.CString(input)
	defer C.free(unsafe.Pointer(cs))
	return takeString(C.synx_parse(cs))
}

// ParseActive parses SYNX with !active engine resolution; result is JSON.
func ParseActive(input string) (string, error) {
	cs := C.CString(input)
	defer C.free(unsafe.Pointer(cs))
	return takeString(C.synx_parse_active(cs))
}

// Stringify converts JSON text (UTF-8) to SYNX.
func Stringify(jsonUTF8 string) (string, error) {
	cs := C.CString(jsonUTF8)
	defer C.free(unsafe.Pointer(cs))
	return takeString(C.synx_stringify(cs))
}

// Format returns canonically formatted SYNX text.
func Format(input string) (string, error) {
	cs := C.CString(input)
	defer C.free(unsafe.Pointer(cs))
	return takeString(C.synx_format(cs))
}

// ParseTool parses a !tool document and returns reshaped JSON.
func ParseTool(input string) (string, error) {
	cs := C.CString(input)
	defer C.free(unsafe.Pointer(cs))
	return takeString(C.synx_parse_tool(cs))
}

// Diff returns structural diff JSON for two SYNX documents.
func Diff(a, b string) (string, error) {
	ca := C.CString(a)
	cb := C.CString(b)
	defer C.free(unsafe.Pointer(ca))
	defer C.free(unsafe.Pointer(cb))
	return takeString(C.synx_diff(ca, cb))
}

// Compile encodes SYNX as .synxb bytes.
func Compile(input string, resolved bool) ([]byte, error) {
	cs := C.CString(input)
	defer C.free(unsafe.Pointer(cs))
	var n C.size_t
	r := C.int(0)
	if resolved {
		r = 1
	}
	p := C.synx_compile(cs, r, &n)
	if p == nil {
		return nil, ErrSynx
	}
	out := C.GoBytes(unsafe.Pointer(p), C.int(n))
	C.synx_free_bytes(p, n)
	return out, nil
}

// Decompile decodes .synxb bytes to SYNX text.
func Decompile(data []byte) (string, error) {
	if len(data) == 0 {
		return "", ErrSynx
	}
	p := C.synx_decompile((*C.uchar)(unsafe.Pointer(&data[0])), C.size_t(len(data)))
	return takeString(p)
}

// IsSynxb reports whether data begins with the .synxb magic header.
func IsSynxb(data []byte) bool {
	if len(data) == 0 {
		return false
	}
	return C.synx_is_synxb((*C.uchar)(unsafe.Pointer(&data[0])), C.size_t(len(data))) != 0
}
