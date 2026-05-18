package synx

import "strings"

// ParseRoot is sugar for `Parse(text)` returning only the top-level object.
// Static mode only — call ParseActive to resolve `!active` markers.
func ParseRoot(text string) *Object {
	r := Parse(text)
	if o, ok := AsObject(r.Root); ok {
		return o
	}
	return NewObjectMap()
}

// ParseActive parses and runs the `!active` engine, returning the top-level object.
func ParseActive(text string, opts Options) *Object {
	r := Parse(text)
	if r.Mode == ModeActive {
		Resolve(&r, opts)
	}
	if o, ok := AsObject(r.Root); ok {
		return o
	}
	return NewObjectMap()
}

// ParseFull returns the full ParseResult (mode, metadata, includes, …).
func ParseFull(text string) ParseResult { return Parse(text) }

// ParseFullActive runs the engine on the parsed tree and returns the full result.
func ParseFullActive(text string, opts Options) ParseResult {
	r := Parse(text)
	if r.Mode == ModeActive {
		Resolve(&r, opts)
	}
	return r
}

// ParseTool parses a `!tool` envelope into `{ tool, params }` or
// `{ tools: [...] }` (schema mode).
func ParseTool(text string, opts Options) *Object {
	r := Parse(text)
	if r.Mode == ModeActive {
		Resolve(&r, opts)
	}
	shaped := ReshapeToolOutput(r.Root, r.Schema)
	if o, ok := AsObject(shaped); ok {
		return o
	}
	return NewObjectMap()
}

// CompileText is the convenience wrapper: text → ParseResult → bytes.
func CompileText(text string, resolved bool) ([]byte, error) {
	r := Parse(text)
	if resolved && r.Mode == ModeActive {
		Resolve(&r, Options{})
	}
	return Compile(r, resolved)
}

// DecompileToText is the convenience wrapper: bytes → ParseResult → SYNX text.
func DecompileToText(bytes []byte) (string, error) {
	pr, err := Decompile(bytes)
	if err != nil {
		return "", err
	}
	var sb strings.Builder
	if pr.Tool {
		sb.WriteString("!tool\n")
	}
	if pr.Schema {
		sb.WriteString("!schema\n")
	}
	if pr.Llm {
		sb.WriteString("!llm\n")
	}
	if pr.Mode == ModeActive {
		sb.WriteString("!active\n")
	}
	if pr.Locked {
		sb.WriteString("!lock\n")
	}
	if sb.Len() > 0 {
		sb.WriteByte('\n')
	}
	sb.WriteString(Stringify(pr.Root))
	return sb.String(), nil
}
