package synx

// MarkerFn is the signature for a user-supplied custom marker.
//
// `key` is the field name, `args` is the parsed argument list, `value` is the
// value currently on the field. Returning `Null` is valid. Builtin markers
// always win over a custom marker with the same name.
type MarkerFn func(key string, args []string, value Value) Value

// Options for active-mode resolution. The zero value is fine for static parsing.
type Options struct {
	Env             map[string]string
	Region          string
	Lang            string
	BasePath        string // defaults to "." when empty at resolve time
	MaxIncludeDepth int    // defaults to 16 when zero
	PackagesPath    string // defaults to "./synx_packages" when empty
	Strict          bool
	MarkerFns       map[string]MarkerFn
	// IncludeDepth is internal; do not set manually.
	IncludeDepth int
}
