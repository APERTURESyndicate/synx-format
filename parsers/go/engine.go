package synx

import (
	"fmt"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
)

const MaxResolveDepth = 512

// builtinMarkers — names of builtin markers; custom markers cannot shadow these.
var builtinMarkers = map[string]struct{}{
	"env": {}, "default": {}, "calc": {}, "ref": {}, "alias": {}, "secret": {},
	"random": {}, "unique": {}, "geo": {}, "i18n": {}, "split": {}, "join": {},
	"clamp": {}, "round": {}, "map": {}, "format": {}, "replace": {}, "sort": {},
	"sum": {}, "fallback": {}, "once": {}, "version": {}, "watch": {}, "prompt": {},
	"vision": {}, "audio": {}, "include": {}, "import": {}, "inherit": {}, "spam": {},
}

func isBuiltinMarker(name string) bool {
	_, ok := builtinMarkers[name]
	return ok
}

// Process-wide spam rate-limit buckets (one resolution per (process, key)).
var (
	spamMu      sync.Mutex
	spamBuckets = map[string]struct{}{}
)

// Resolve applies markers and constraints in-place. No-op on Static mode.
func Resolve(result *ParseResult, opts Options) {
	if result.Mode != ModeActive {
		return
	}
	if _, ok := result.Root.(ObjectValue); !ok {
		return
	}
	r := &resolver{result: result, opts: opts}
	r.namespaces = map[string]*Object{}
	r.onceKeys = map[string]struct{}{}
	r.onceNewKeys = map[string]struct{}{}
	seed := int64(0)
	if opts.Env != nil {
		if s, ok := opts.Env["SYNX_SEED"]; ok {
			if v, err := strconv.ParseInt(s, 10, 64); err == nil {
				seed = v
			}
		}
	}
	if seed == 0 {
		seed = rand.Int63()
	}
	r.rng = rand.New(rand.NewSource(seed))
	r.run()
}

type resolver struct {
	result      *ParseResult
	opts        Options
	namespaces  map[string]*Object
	rng         *rand.Rand
	onceLoaded  bool
	onceKeys    map[string]struct{}
	onceNewKeys map[string]struct{}
}

func (r *resolver) root() *Object {
	o, _ := AsObject(r.result.Root)
	return o
}

func (r *resolver) run() {
	r.loadPackages()
	r.loadIncludes()
	r.applyInheritPass()
	r.stripUnderscoreKeys()
	r.walk("", 0)
	r.validateAll()
	r.flushOnce()
}

// ─── top-level passes ───────────────────────────────────────────────────────

func (r *resolver) loadPackages() {
	if len(r.result.Uses) == 0 {
		return
	}
	base := r.opts.PackagesPath
	if base == "" {
		base = "./synx_packages"
	}
	for _, use := range r.result.Uses {
		if strings.Contains(use.Package, "..") {
			continue
		}
		path := filepath.Join(base, use.Package, "synx.synx")
		text, err := os.ReadFile(path)
		if err != nil {
			continue
		}
		sub := Parse(string(text))
		obj, ok := AsObject(sub.Root)
		if !ok {
			continue
		}
		r.namespaces[use.Alias] = obj
		r.root().Set(use.Alias, Object_(obj))
	}
}

func (r *resolver) loadIncludes() {
	if len(r.result.Includes) == 0 {
		return
	}
	max := r.opts.MaxIncludeDepth
	if max == 0 {
		max = 16
	}
	if r.opts.IncludeDepth >= max {
		return
	}
	base := r.opts.BasePath
	if base == "" {
		base = "."
	}
	for _, inc := range r.result.Includes {
		safe := r.jailPath(base, inc.Path)
		if safe == "" {
			continue
		}
		text, err := os.ReadFile(safe)
		if err != nil {
			continue
		}
		sub := Parse(string(text))
		if sub.Mode == ModeActive {
			subOpts := r.opts
			subOpts.BasePath = filepath.Dir(safe)
			subOpts.IncludeDepth = r.opts.IncludeDepth + 1
			Resolve(&sub, subOpts)
		}
		obj, ok := AsObject(sub.Root)
		if !ok {
			continue
		}
		r.namespaces[inc.Alias] = obj
		r.root().Set(inc.Alias, Object_(obj))
	}
}

func (r *resolver) applyInheritPass() {
	for path, fields := range r.result.Metadata {
		for key, meta := range fields {
			if !meta.HasMarker("inherit") || len(meta.Args) == 0 {
				continue
			}
			r.inheritMerge(path, key, meta.Args)
		}
	}
}

func (r *resolver) inheritMerge(path, key string, parentNames []string) {
	parent := r.getObjectAt(path)
	if parent == nil {
		return
	}
	child, _ := parent.Get(key)
	co, ok := AsObject(child)
	if !ok {
		return
	}
	for _, name := range parentNames {
		pv, _ := parent.Get(name)
		if po, ok := AsObject(pv); ok {
			mergeMissing(co, po)
		}
	}
}

func mergeMissing(dst, src *Object) {
	for _, p := range src.Pairs() {
		existing, ok := dst.Get(p.Key)
		if ok {
			if eo, ok := AsObject(existing); ok {
				if so, ok := AsObject(p.Value); ok {
					mergeMissing(eo, so)
				}
			}
			continue
		}
		dst.Set(p.Key, p.Value)
	}
}

func (r *resolver) stripUnderscoreKeys() {
	root := r.root()
	for _, k := range root.Keys() {
		if len(k) > 0 && k[0] == '_' {
			root.Remove(k)
		}
	}
}

func (r *resolver) walk(path string, depth int) {
	if depth > MaxResolveDepth {
		return
	}
	if fields, ok := r.result.Metadata[path]; ok {
		// Snapshot keys — mutation can re-shape the container.
		keys := make([]string, 0, len(fields))
		for k := range fields {
			keys = append(keys, k)
		}
		for _, k := range keys {
			r.applyMarkers(fields[k], k, path)
		}
	}
	container := r.getObjectAt(path)
	if container == nil {
		return
	}
	for _, p := range container.Pairs() {
		if _, ok := p.Value.(ObjectValue); ok {
			sub := p.Key
			if path != "" {
				sub = path + "." + p.Key
			}
			r.walk(sub, depth+1)
		}
	}
}

func (r *resolver) applyMarkers(meta Meta, key, path string) {
	parent := r.getObjectAt(path)
	if parent == nil {
		return
	}
	value, _ := parent.Get(key)
	if value == nil {
		value = Null
	}

	for _, marker := range meta.Markers {
		switch marker {
		case "env":
			value = r.applyEnv(value, meta)
		case "default":
			value = r.applyDefault(value, meta)
		case "calc":
			value = r.applyCalc(value)
		case "ref", "alias":
			value = r.applyRef(value)
		case "secret":
			value = r.applySecret(value)
		case "random":
			value = r.applyRandom(value, meta)
		case "unique":
			value = r.applyUnique(value)
		case "geo":
			value = r.applyGeo(value, meta)
		case "i18n":
			value = r.applyI18n(value, meta)
		case "split":
			value = r.applySplit(value, meta)
		case "join":
			value = r.applyJoin(value, meta)
		case "clamp":
			value = r.applyClamp(value, meta)
		case "round":
			value = r.applyRound(value, meta)
		case "map":
			value = r.applyMap(value, meta)
		case "format":
			value = r.applyFormat(value, meta)
		case "replace":
			value = r.applyReplace(value, meta)
		case "sort":
			value = r.applySort(value, meta)
		case "sum":
			value = r.applySum(value)
		case "fallback":
			value = r.applyFallback(value, meta)
		case "once":
			value = r.applyOnce(value, path, key)
		case "version":
			value = r.applyVersion(value)
		case "watch":
			value = r.applyWatch(value)
		case "prompt":
			value = r.applyPrompt(value)
		case "vision", "audio":
			// passthrough envelopes
		case "spam":
			value = r.applySpam(value, key)
		case "inherit", "include", "import":
			// handled in pre-pass / directives
		default:
			if !isBuiltinMarker(marker) {
				if fn, ok := r.opts.MarkerFns[marker]; ok {
					value = fn(key, meta.Args, value)
				}
			}
		}
	}

	if meta.TypeHint != "" {
		value = r.coerceTypeHint(value, meta.TypeHint)
	}
	parent.Set(key, value)
}

func (r *resolver) validateAll() {
	for path, fields := range r.result.Metadata {
		container := r.getObjectAt(path)
		if container == nil {
			continue
		}
		for fk, meta := range fields {
			c := meta.Constraints
			if c == nil {
				continue
			}
			fv, ok := container.Get(fk)
			if !ok {
				if c.Required && r.opts.Strict {
					fmt.Fprintf(os.Stderr, "synx: required '%s.%s' missing\n", path, fk)
				}
				continue
			}
			if c.TypeName != "" {
				match := false
				switch c.TypeName {
				case "int":
					_, match = fv.(IntValue)
				case "float":
					_, match = fv.(FloatValue)
				case "bool":
					_, match = fv.(BoolValue)
				case "string":
					_, match = fv.(StringValue)
				case "array":
					_, match = fv.(ArrayValue)
				case "object":
					_, match = fv.(ObjectValue)
				}
				if !match && r.opts.Strict {
					fmt.Fprintf(os.Stderr, "synx: type mismatch '%s.%s' want %s, got %s\n",
						path, fk, c.TypeName, fv.Kind())
				}
			}
			if d, ok := AsNumber(fv); ok {
				if c.Min != nil && d < *c.Min && r.opts.Strict {
					fmt.Fprintf(os.Stderr, "synx: '%s.%s' below min\n", path, fk)
				}
				if c.Max != nil && d > *c.Max && r.opts.Strict {
					fmt.Fprintf(os.Stderr, "synx: '%s.%s' above max\n", path, fk)
				}
			}
			if c.EnumValues != nil {
				if s, ok := AsString(fv); ok {
					found := false
					for _, e := range c.EnumValues {
						if e == s {
							found = true
							break
						}
					}
					if !found && r.opts.Strict {
						fmt.Fprintf(os.Stderr, "synx: '%s.%s' '%s' not in enum\n", path, fk, s)
					}
				}
			}
			if c.Pattern != "" {
				if s, ok := AsString(fv); ok {
					if !regexMatches(s, c.Pattern) && r.opts.Strict {
						fmt.Fprintf(os.Stderr, "synx: '%s.%s' fails pattern '%s'\n", path, fk, c.Pattern)
					}
				}
			}
		}
	}
}

// ─── once persistence ──────────────────────────────────────────────────────

func (r *resolver) applyOnce(v Value, path, key string) Value {
	if !r.onceLoaded {
		r.onceLoaded = true
		base := r.opts.BasePath
		if base == "" {
			base = "."
		}
		data, err := os.ReadFile(filepath.Join(base, ".synx.lock"))
		if err == nil {
			for _, line := range strings.Split(string(data), "\n") {
				s := strings.TrimSpace(line)
				if s != "" {
					r.onceKeys[s] = struct{}{}
				}
			}
		}
	}
	lockKey := key
	if path != "" {
		lockKey = path + "." + key
	}
	if _, ok := r.onceKeys[lockKey]; ok {
		return Null
	}
	r.onceNewKeys[lockKey] = struct{}{}
	return v
}

func (r *resolver) flushOnce() {
	if len(r.onceNewKeys) == 0 {
		return
	}
	base := r.opts.BasePath
	if base == "" {
		base = "."
	}
	all := map[string]struct{}{}
	for k := range r.onceKeys {
		all[k] = struct{}{}
	}
	for k := range r.onceNewKeys {
		all[k] = struct{}{}
	}
	keys := make([]string, 0, len(all))
	for k := range all {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	_ = os.WriteFile(filepath.Join(base, ".synx.lock"), []byte(strings.Join(keys, "\n")+"\n"), 0644)
}

// ─── path / lookup / interpolation ──────────────────────────────────────────

func (r *resolver) getObjectAt(path string) *Object {
	if path == "" {
		return r.root()
	}
	current := r.root()
	for _, seg := range strings.Split(path, ".") {
		v, ok := current.Get(seg)
		if !ok {
			return nil
		}
		o, ok := AsObject(v)
		if !ok {
			return nil
		}
		current = o
	}
	return current
}

func (r *resolver) lookup(path string) Value {
	if dot := strings.IndexByte(path, '.'); dot >= 0 {
		ns := path[:dot]
		rest := path[dot+1:]
		if nsRoot, ok := r.namespaces[ns]; ok {
			if v := deepGet(rest, nsRoot); v != nil {
				return v
			}
		}
	}
	return deepGet(path, r.root())
}

func deepGet(path string, from *Object) Value {
	if path == "" || from == nil {
		return nil
	}
	current := from
	parts := strings.Split(path, ".")
	for i, p := range parts {
		v, ok := current.Get(p)
		if !ok {
			return nil
		}
		if i == len(parts)-1 {
			return v
		}
		o, ok := AsObject(v)
		if !ok {
			return nil
		}
		current = o
	}
	return nil
}

func (r *resolver) interpolate(s string) string {
	if strings.IndexByte(s, '{') < 0 {
		return s
	}
	var sb strings.Builder
	sb.Grow(len(s))
	i := 0
	for i < len(s) {
		c := s[i]
		if c == '{' {
			end := strings.IndexByte(s[i+1:], '}')
			if end < 0 {
				sb.WriteByte('{')
				i++
				continue
			}
			end += i + 1
			inner := strings.TrimSpace(s[i+1 : end])
			v := r.lookup(inner)
			if v != nil {
				sb.WriteString(valueToString(v))
			} else {
				sb.WriteByte('{')
				sb.WriteString(inner)
				sb.WriteByte('}')
			}
			i = end + 1
			continue
		}
		sb.WriteByte(c)
		i++
	}
	return sb.String()
}

func (r *resolver) jailPath(base, rel string) string {
	if rel == "" {
		return ""
	}
	if rel[0] == '/' || rel[0] == '\\' {
		return ""
	}
	if len(rel) >= 2 && rel[1] == ':' {
		return "" // Windows drive letter
	}
	if strings.HasPrefix(rel, "res://") || strings.HasPrefix(rel, "user://") {
		return ""
	}
	normalized := strings.ReplaceAll(rel, "\\", "/")
	for _, seg := range strings.Split(normalized, "/") {
		if seg == ".." || seg == "..." {
			return ""
		}
	}
	full := filepath.Join(base, normalized)
	// Resolve symlinks so a link inside `base` pointing outside the jail is
	// rejected. EvalSymlinks fails if the target does not exist; in that case
	// fall back to lexical containment of the absolute paths.
	baseCanon, err := filepath.EvalSymlinks(base)
	if err != nil {
		baseCanon, err = filepath.Abs(base)
		if err != nil {
			return ""
		}
	}
	fullCanon, err := filepath.EvalSymlinks(full)
	if err != nil {
		fullAbs, absErr := filepath.Abs(full)
		if absErr != nil {
			return ""
		}
		if !strings.HasPrefix(fullAbs, baseCanon+string(filepath.Separator)) && fullAbs != baseCanon {
			return ""
		}
		return full
	}
	if !strings.HasPrefix(fullCanon, baseCanon+string(filepath.Separator)) && fullCanon != baseCanon {
		return ""
	}
	return fullCanon
}

func (r *resolver) coerceTypeHint(v Value, hint string) Value {
	switch hint {
	case "int":
		if _, ok := v.(IntValue); ok {
			return v
		}
		if d, ok := AsNumber(v); ok {
			return Int(int64(d))
		}
	case "float":
		if _, ok := v.(FloatValue); ok {
			return v
		}
		if d, ok := AsNumber(v); ok {
			return Float(d)
		}
	case "string":
		if _, ok := v.(StringValue); ok {
			return v
		}
		return String(valueToString(v))
	case "bool":
		if _, ok := v.(BoolValue); ok {
			return v
		}
		s := valueToString(v)
		return Bool(s == "true" || s == "1")
	}
	return v
}

// ─── shared helpers ────────────────────────────────────────────────────────

func valueToString(v Value) string {
	switch x := v.(type) {
	case NullValue:
		return "null"
	case BoolValue:
		if x.V {
			return "true"
		}
		return "false"
	case IntValue:
		return strconv.FormatInt(x.V, 10)
	case FloatValue:
		if math.IsNaN(x.V) || math.IsInf(x.V, 0) {
			return "null"
		}
		s := fmt.Sprintf("%.17g", x.V)
		if !strings.ContainsAny(s, ".eE") {
			s += ".0"
		}
		return s
	case StringValue:
		return x.V
	case SecretValue:
		return x.V
	case ArrayValue:
		parts := make([]string, len(x.V))
		for i, item := range x.V {
			parts[i] = valueToString(item)
		}
		return "[" + strings.Join(parts, ", ") + "]"
	case ObjectValue:
		return "[Object]"
	}
	return ""
}

func regexMatches(value, pattern string) bool {
	re, err := regexp.Compile(pattern)
	if err != nil {
		return true // invalid pattern — do not reject value
	}
	return re.MatchString(value)
}
