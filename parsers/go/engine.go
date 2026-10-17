package synx

import (
	"fmt"
	"math"
	"math/rand"
	"os"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

// MaxResolveDepth caps object nesting during active-mode resolution.
const MaxResolveDepth = 512

const (
	maxCalcExprLen         = 4096
	maxCalcResolvedLen     = 64 * 1024
	maxFileSize            = 10 * 1024 * 1024
	defaultMaxIncludeDepth = 16
	maxEngineScratch       = 4 * 1024 * 1024
)

// builtinMarkers — names a custom marker (Options.MarkerFns) cannot take.
var builtinMarkers = map[string]struct{}{
	"env": {}, "default": {}, "calc": {}, "ref": {}, "alias": {}, "secret": {},
	"random": {}, "unique": {}, "geo": {}, "i18n": {}, "split": {}, "join": {},
	"clamp": {}, "round": {}, "map": {}, "format": {}, "replace": {}, "sort": {},
	"sum": {}, "fallback": {}, "once": {}, "version": {}, "watch": {}, "prompt": {},
	"vision": {}, "audio": {}, "include": {}, "import": {}, "inherit": {}, "spam": {},
	"template": {},
}

func isBuiltinMarker(name string) bool {
	_, ok := builtinMarkers[name]
	return ok
}

// Bits for the built-in markers applyMarkers dispatches on.
const (
	mkAlias uint32 = 1 << iota
	mkCalc
	mkClamp
	mkDefault
	mkEnv
	mkFallback
	mkFormat
	mkGeo
	mkI18n
	mkImport
	mkInclude
	mkJoin
	mkMap
	mkOnce
	mkPrompt
	mkRandom
	mkRef
	mkReplace
	mkRound
	mkSecret
	mkSort
	mkSpam
	mkSplit
	mkSum
	mkUnique
	mkVersion
	mkWatch
)

// markerBits classifies a marker chain once, so every marker test in
// applyMarkers is a bit test instead of a scan with string compares.
func markerBits(markers []string) uint32 {
	var bits uint32
	for _, m := range markers {
		switch m {
		case "alias":
			bits |= mkAlias
		case "calc":
			bits |= mkCalc
		case "clamp":
			bits |= mkClamp
		case "default":
			bits |= mkDefault
		case "env":
			bits |= mkEnv
		case "fallback":
			bits |= mkFallback
		case "format":
			bits |= mkFormat
		case "geo":
			bits |= mkGeo
		case "i18n":
			bits |= mkI18n
		case "import":
			bits |= mkImport
		case "include":
			bits |= mkInclude
		case "join":
			bits |= mkJoin
		case "map":
			bits |= mkMap
		case "once":
			bits |= mkOnce
		case "prompt":
			bits |= mkPrompt
		case "random":
			bits |= mkRandom
		case "ref":
			bits |= mkRef
		case "replace":
			bits |= mkReplace
		case "round":
			bits |= mkRound
		case "secret":
			bits |= mkSecret
		case "sort":
			bits |= mkSort
		case "spam":
			bits |= mkSpam
		case "split":
			bits |= mkSplit
		case "sum":
			bits |= mkSum
		case "unique":
			bits |= mkUnique
		case "version":
			bits |= mkVersion
		case "watch":
			bits |= mkWatch
		}
	}
	return bits
}

func markerIndex(markers []string, name string) int {
	for i, m := range markers {
		if m == name {
			return i
		}
	}
	return -1
}

// markerArg is markers[idx+off] when it exists.
func markerArg(markers []string, idx, off int) (string, bool) {
	if idx >= 0 && idx+off < len(markers) {
		return markers[idx+off], true
	}
	return "", false
}

type resolver struct {
	opts        Options
	meta        *MetadataTree
	includes    *Object
	rootValue   Value
	root        *Object
	rootSpecial []string
	path        []byte
	rng         *rand.Rand
	// preIncludes holds what the `:include` pre-pass resolved for a root key.
	// It is kept apart from the `{key:alias}` map, so `{x:include}` still
	// sees exactly the includes the document declares.
	preIncludes map[string]preInclude
}

type preInclude struct {
	path string
	root Value
}

// Resolve applies markers and constraints in place, as crates/synx-core's
// engine does. No-op on a static document.
//
// Resolution is one pass per object in document order: for each key, first
// its child object (or each object item of its list), then the key's own
// markers; after all keys, `{…}` interpolation. A marker therefore sees every
// key above it already resolved, at any depth.
func Resolve(result *ParseResult, opts Options) {
	if result.Mode != ModeActive {
		return
	}
	meta := result.Metadata
	if meta == nil {
		meta = NewMetadataTree()
	}
	r := &resolver{opts: opts, meta: meta, rootValue: result.Root, preIncludes: map[string]preInclude{}}
	r.root, _ = AsObject(result.Root)
	r.rng = newRNG(opts)

	packages := r.loadPackages(result.Uses)
	r.includes = r.loadIncludes(result.Includes)

	// `:include` / `:import` keys also feed the `{key:alias}` map.
	if r.root != nil {
		if rootMeta := meta.Get(""); rootMeta != nil {
			for i := 0; i < len(r.root.pairs); i++ {
				key := r.root.pairs[i].Key
				m, ok := rootMeta.items[key]
				if !ok || !(hasMarker(m.Markers, "include") || hasMarker(m.Markers, "import")) {
					continue
				}
				// The included document runs its own pre-pass: without the depth
				// cap a file that includes itself recursed until the stack ran out.
				if r.opts.IncludeDepth >= r.maxIncludeDepth() {
					continue
				}
				path, ok := r.root.pairs[i].Value.(StringValue)
				if !ok {
					continue
				}
				full, errText := jailPath(r.basePath(), path.V)
				if errText != "" || checkFileSize(full) != "" {
					continue
				}
				text, err := readUTF8(full)
				if err != nil {
					continue
				}
				included := r.parseIncluded(text, full)
				// The key's `:include` marker takes this result instead of reading
				// and resolving the file a second time (2^depth for a chain).
				r.preIncludes[key] = preInclude{path: path.V, root: included}
				if !r.includes.Contains(key) {
					r.includes.Set(key, included)
				}
			}
		}
		for _, p := range packages.pairs {
			if !r.root.Contains(p.Key) {
				r.root.Set(p.Key, CloneValue(p.Value))
			}
		}
	}

	// Type and constraint registries: the live tree before inheritance, each
	// object's own declarations first, then its children in document order.
	types := map[string]string{}
	cons := map[string]*Constraints{}
	r.collectRegistries(result.Root, types, cons, 0)

	applyInheritance(result.Root, meta)
	if r.root != nil {
		r.root.retain(func(k string) bool { return !strings.HasPrefix(k, "_") })
	}

	// Literal `:secret` values are masked before anything can interpolate them.
	r.path = r.path[:0]
	r.maskSecretLiterals(result.Root)

	if r.root != nil {
		for _, p := range r.root.pairs {
			if isCalcSpecialKey(p.Key) {
				r.rootSpecial = append(r.rootSpecial, p.Key)
			}
		}
		r.path = r.path[:0]
		r.resolveObject(r.root, 0)
	}

	r.validateFields(result.Root, cons, types)

	result.Metadata = meta
	result.Uses = nil
}

func newRNG(opts Options) *rand.Rand {
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
	return rand.New(rand.NewSource(seed))
}

func (r *resolver) basePath() string {
	if r.opts.BasePath == "" {
		return "."
	}
	return r.opts.BasePath
}

func (r *resolver) maxIncludeDepth() int {
	if r.opts.MaxIncludeDepth == 0 {
		return defaultMaxIncludeDepth
	}
	return r.opts.MaxIncludeDepth
}

// parseIncluded parses an included file and resolves it when it is active.
func (r *resolver) parseIncluded(text, full string) Value {
	included := Parse(text)
	if included.Mode == ModeActive {
		child := r.opts
		child.IncludeDepth++
		child.BasePath = parentDir(full)
		Resolve(&included, child)
	}
	return included.Root
}

func (r *resolver) pushKey(key string) {
	if len(r.path) > 0 {
		r.path = append(r.path, '.')
	}
	if strings.IndexByte(key, '.') < 0 && strings.IndexByte(key, '\\') < 0 {
		r.path = append(r.path, key...)
		return
	}
	for i := 0; i < len(key); i++ {
		if key[i] == '.' || key[i] == '\\' {
			r.path = append(r.path, '\\')
		}
		r.path = append(r.path, key[i])
	}
}

func (r *resolver) pushItem(idx int) {
	r.path = append(r.path, '.', '#')
	r.path = strconv.AppendInt(r.path, int64(idx), 10)
}

func (r *resolver) resolveObject(obj *Object, depth int) {
	if depth >= MaxResolveDepth {
		for i := range obj.pairs {
			obj.pairs[i].Value = String("NESTING_ERR: maximum object nesting depth exceeded")
		}
		return
	}
	mm := r.meta.levels[string(r.path)]
	baseLen := len(r.path)
	n := len(obj.pairs)
	for i := 0; i < n; i++ {
		if i >= len(obj.pairs) {
			break
		}
		switch child := obj.pairs[i].Value.(type) {
		case ObjectValue:
			r.pushKey(obj.pairs[i].Key)
			r.resolveObject(child.V, depth+1)
			r.path = r.path[:baseLen]
		case ArrayValue:
			r.pushKey(obj.pairs[i].Key)
			listLen := len(r.path)
			for idx, item := range child.V {
				if o, ok := item.(ObjectValue); ok {
					r.pushItem(idx)
					r.resolveObject(o.V, depth+1)
					r.path = r.path[:listLen]
				}
			}
			r.path = r.path[:baseLen]
		}
		if mm == nil {
			continue
		}
		key := obj.pairs[i].Key
		meta, ok := mm.items[key]
		if !ok {
			continue
		}
		r.applyMarkers(obj, key, meta, string(r.path))
	}

	// Then `{…}` interpolation on every string value of this object.
	for i := 0; i < len(obj.pairs); i++ {
		s, ok := obj.pairs[i].Value.(StringValue)
		if !ok || strings.IndexByte(s.V, '{') < 0 {
			continue
		}
		if out := r.resolveInterpolation(s.V, obj); out != s.V {
			obj.pairs[i].Value = String(out)
		}
	}
}

func (r *resolver) lookupEnv(name string) (string, bool) {
	if r.opts.Env != nil {
		v, ok := r.opts.Env[name]
		return v, ok
	}
	return os.LookupEnv(name)
}

func calcErr(msg string) Value { return String("CALC_ERR: " + msg) }

const calcTooLong = "CALC_ERR: resolved expression too long (max 65536 bytes)"

func (r *resolver) applyMarkers(obj *Object, key string, meta *Meta, path string) {
	markers := meta.Markers
	mb := markerBits(markers)
	get := func() (Value, bool) { return obj.Get(key) }
	set := func(v Value) { obj.Set(key, v) }

	// ── :spam:MAX[:WINDOW_SEC] target ──
	if mb&mkSpam != 0 {
		idx := markerIndex(markers, "spam")
		maxCalls := uint64(0)
		if s, ok := markerArg(markers, idx, 1); ok {
			if n, ok := parseRustUint(s); ok {
				maxCalls = n
			}
		}
		windowSec := uint64(1)
		if s, ok := markerArg(markers, idx, 2); ok {
			if n, ok := parseRustUint(s); ok {
				windowSec = n
			}
		}
		if maxCalls == 0 {
			set(String("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]"))
			return
		}
		target := key
		if v, ok := get(); ok {
			target = valueToString(v)
		}
		if !allowSpamAccess(key+"::"+target, maxCalls, windowSec) {
			set(String(fmt.Sprintf("SPAM_ERR: '%s' exceeded %d calls per %ds", target, maxCalls, windowSec)))
			return
		}
		if v, ok := get(); ok {
			t := valueToString(v)
			if res, ok := deepGet(r.rootValue, t); ok {
				set(CloneValue(res))
			} else if res, ok := obj.Get(t); ok {
				set(CloneValue(res))
			}
		}
	}

	// ── :include / :import ──
	if mb&(mkInclude|mkImport) != 0 {
		if v, ok := get(); ok {
			if filePath, ok := v.(StringValue); ok {
				maxDepth := r.maxIncludeDepth()
				if r.opts.IncludeDepth >= maxDepth {
					set(String(fmt.Sprintf("INCLUDE_ERR: max include depth (%d) exceeded", maxDepth)))
					return
				}
				if path == "" {
					if pre, ok := r.preIncludes[key]; ok && pre.path == filePath.V {
						set(CloneValue(pre.root))
						return
					}
				}
				full, errText := jailPath(r.basePath(), filePath.V)
				if errText != "" {
					set(String("INCLUDE_ERR: " + errText))
					return
				}
				if errText := checkFileSize(full); errText != "" {
					set(String("INCLUDE_ERR: " + errText))
					return
				}
				text, err := readUTF8(full)
				if err != nil {
					set(String("INCLUDE_ERR: " + ioErrText(err, filePath.V)))
					return
				}
				set(r.parseIncluded(text, full))
			}
		}
		return
	}

	// ── :env ──
	if mb&mkEnv != 0 {
		if v, ok := get(); ok {
			if name, ok := v.(StringValue); ok {
				envVal, found := r.lookupEnv(name.V)
				forceString := wantsString(meta)
				di := markerIndex(markers, "default")
				switch {
				case found && envVal != "":
					set(castEngineText(envVal, forceString))
				case di >= 0 && len(markers) > di+1:
					set(castEngineText(strings.Join(markers[di+1:], ":"), forceString))
				default:
					set(Null)
				}
			}
		}
	}

	// ── :random ──
	if mb&mkRandom != 0 {
		if v, ok := get(); ok {
			if arr, ok := v.(ArrayValue); ok {
				if len(arr.V) == 0 {
					set(Null)
					return
				}
				var picked Value
				if len(meta.Args) > 0 {
					weights := make([]float64, 0, len(meta.Args))
					for _, a := range meta.Args {
						if w, ok := parseRustFloat(a); ok {
							weights = append(weights, w)
						}
					}
					picked = r.weightedRandom(arr.V, weights)
				} else {
					picked = arr.V[r.rng.Intn(len(arr.V))]
				}
				set(picked)
			}
		}
	}

	// ── :ref (feeds the resolved value into later markers; :ref:calc:*2) ──
	if mb&mkRef != 0 {
		if v, ok := get(); ok {
			if target, ok := v.(StringValue); ok {
				var resolved Value = Null
				if res, ok := deepGet(r.rootValue, target.V); ok {
					resolved = CloneValue(res)
				} else if res, ok := obj.Get(target.V); ok {
					resolved = CloneValue(res)
				}
				done := false
				if mb&mkCalc != 0 {
					if n, ok := valueAsNumber(resolved); ok {
						if calcExpr, ok := markerArg(markers, markerIndex(markers, "calc"), 1); ok {
							first := ' '
							if calcExpr != "" {
								first, _ = utf8.DecodeRuneInString(calcExpr)
							}
							if strings.ContainsRune("+-*/%", first) {
								res := SafeCalc(formatNumber(n) + " " + calcExpr)
								if res.OK {
									set(numberValue(res.Value))
								} else {
									set(calcErr(res.Error))
								}
								done = true
							}
						}
					}
				}
				if !done {
					set(resolved)
				}
			}
		}
	}

	// ── :i18n[:COUNT_FIELD] ──
	if mb&mkI18n != 0 {
		if v, ok := get(); ok {
			if translations, ok := v.(ObjectValue); ok {
				lang := r.opts.Lang
				if lang == "" {
					lang = "en"
				}
				val, found := translations.V.Get(lang)
				if !found {
					val, found = translations.V.Get("en")
				}
				if !found && len(translations.V.pairs) > 0 {
					val, found = translations.V.pairs[0].Value, true
				}
				if !found {
					val = Null
				}
				val = CloneValue(val)
				countField, hasCount := markerArg(markers, markerIndex(markers, "i18n"), 1)
				if forms, ok := val.(ObjectValue); ok && hasCount {
					count := 0.0
					if cv, ok := obj.Get(countField); ok {
						if n, ok := valueAsNumber(cv); ok {
							count = n
						} else if rv, ok := deepGet(r.rootValue, countField); ok {
							count, _ = valueAsNumber(rv)
						}
					} else if rv, ok := deepGet(r.rootValue, countField); ok {
						count, _ = valueAsNumber(rv)
					}
					n := f64ToI64(count)
					category := pluralCategory(lang, n)
					chosen, found := forms.V.Get(category)
					if !found {
						chosen, found = forms.V.Get("other")
					}
					if !found && len(forms.V.pairs) > 0 {
						chosen, found = forms.V.pairs[0].Value, true
					}
					if !found {
						chosen = Null
					}
					if s, ok := chosen.(StringValue); ok {
						set(String(strings.ReplaceAll(s.V, "{count}", strconv.FormatInt(n, 10))))
					} else {
						set(chosen)
					}
				} else {
					set(val)
				}
			}
		}
	}

	// ── :calc ──
	if mb&mkCalc != 0 {
		if v, ok := get(); ok {
			if expr, ok := v.(StringValue); ok {
				if len(expr.V) > maxCalcExprLen {
					set(String(fmt.Sprintf("CALC_ERR: expression too long (%d chars, max %d)", len(expr.V), maxCalcExprLen)))
					return
				}
				resolved, ok := r.substituteCalcVars(expr.V, obj, key)
				if !ok {
					set(String(calcTooLong))
					return
				}
				resolved, ok = r.substituteCalcPaths(resolved)
				if !ok {
					set(String(calcTooLong))
					return
				}
				res := SafeCalc(resolved)
				if res.OK {
					set(numberValue(res.Value))
				} else {
					set(calcErr(res.Error))
				}
			}
		}
	}

	// ── :alias ──
	if mb&mkAlias != 0 {
		if v, ok := get(); ok {
			if t, ok := v.(StringValue); ok {
				target := t.V
				// A list item's `#<index>` segment is a metadata detail, not part
				// of a path a document can name.
				var cp strings.Builder
				for _, seg := range MetaPathSegments(path) {
					if strings.HasPrefix(seg, "#") {
						continue
					}
					cp.WriteString(seg)
					cp.WriteByte('.')
				}
				cp.WriteString(key)
				currentPath := cp.String()
				if target == key || target == currentPath {
					set(String(fmt.Sprintf("ALIAS_ERR: self-referential alias: %s → %s", currentPath, target)))
				} else {
					targetVal, found := deepGet(r.rootValue, target)
					targetParent, targetKey := "", target
					if dot := strings.LastIndexByte(target, '.'); dot >= 0 {
						targetParent, targetKey = target[:dot], target[dot+1:]
					}
					targetHasAlias := false
					if mm := r.meta.Get(targetParent); mm != nil {
						if m, ok := mm.items[targetKey]; ok {
							targetHasAlias = hasMarker(m.Markers, "alias")
						}
					}
					isCycle := false
					if targetHasAlias && found {
						if s, ok := targetVal.(StringValue); ok {
							isCycle = s.V == key || s.V == currentPath
						}
					}
					switch {
					case isCycle:
						a, b := currentPath, target
						if b < a {
							a, b = b, a
						}
						set(String(fmt.Sprintf("ALIAS_ERR: circular alias detected: %s → %s", a, b)))
					case found:
						set(CloneValue(targetVal))
					default:
						set(Null)
					}
				}
			}
		}
	}

	// ── :secret (idempotent: the pre-pass may have masked it already) ──
	if mb&mkSecret != 0 {
		if v, ok := get(); ok {
			if _, isSecret := v.(SecretValue); !isSecret {
				set(Secret(valueToString(v)))
			}
		}
	}

	// ── :unique ──
	if mb&mkUnique != 0 {
		if v, ok := get(); ok {
			if arr, ok := v.(ArrayValue); ok {
				seen := make(map[string]struct{}, len(arr.V))
				unique := make([]Value, 0, len(arr.V))
				for _, item := range arr.V {
					s := valueToString(item)
					if _, dup := seen[s]; !dup {
						seen[s] = struct{}{}
						unique = append(unique, item)
					}
				}
				set(Array(unique))
			}
		}
	}

	// ── :geo ──
	if mb&mkGeo != 0 {
		if v, ok := get(); ok {
			if arr, ok := v.(ArrayValue); ok {
				region := r.opts.Region
				if region == "" {
					region = "US"
				}
				prefix := region + " "
				var result Value = Null
				found := false
				for _, item := range arr.V {
					if s, ok := item.(StringValue); ok && strings.HasPrefix(s.V, prefix) {
						result = String(strings.TrimSpace(s.V[len(prefix):]))
						found = true
						break
					}
				}
				if !found && len(arr.V) > 0 {
					first := arr.V[0]
					result = first
					if s, ok := first.(StringValue); ok {
						if sp := strings.IndexByte(s.V, ' '); sp >= 0 {
							result = String(strings.TrimSpace(s.V[sp+1:]))
						}
					}
				}
				set(result)
			}
		}
	}

	// ── :split[:DELIM] ──
	if mb&mkSplit != 0 {
		if v, ok := get(); ok {
			if s, ok := v.(StringValue); ok {
				sep := ","
				if d, ok := markerArg(markers, markerIndex(markers, "split"), 1); ok {
					sep = delimiterFromKeyword(d)
				}
				var items []Value
				for _, part := range strings.Split(s.V, sep) {
					if p := strings.TrimSpace(part); p != "" {
						items = append(items, castPrimitive(p))
					}
				}
				if items == nil {
					items = []Value{}
				}
				set(Array(items))
			}
		}
	}

	// ── :join[:DELIM] ──
	if mb&mkJoin != 0 {
		if v, ok := get(); ok {
			if arr, ok := v.(ArrayValue); ok {
				sep := ","
				if d, ok := markerArg(markers, markerIndex(markers, "join"), 1); ok {
					sep = delimiterFromKeyword(d)
				}
				parts := make([]string, len(arr.V))
				for i, x := range arr.V {
					parts[i] = valueToString(x)
				}
				set(String(strings.Join(parts, sep)))
			}
		}
	}

	// ── :default (standalone, without :env) ──
	if mb&mkDefault != 0 && mb&mkEnv == 0 {
		empty := true
		if v, ok := get(); ok {
			switch x := v.(type) {
			case NullValue:
			case StringValue:
				empty = x.V == ""
			default:
				empty = false
			}
		}
		if empty {
			di := markerIndex(markers, "default")
			if len(markers) > di+1 {
				set(castEngineText(strings.Join(markers[di+1:], ":"), wantsString(meta)))
			}
		}
	}

	// ── :clamp:MIN:MAX ──
	if mb&mkClamp != 0 {
		idx := markerIndex(markers, "clamp")
		minS, _ := markerArg(markers, idx, 1)
		maxS, _ := markerArg(markers, idx, 2)
		// A bound must be a finite number: `nan`, `inf` and `1e400` skip the marker.
		lo, okLo := parseRustFloat(minS)
		hi, okHi := parseRustFloat(maxS)
		if okLo && okHi && !math.IsInf(lo, 0) && !math.IsNaN(lo) && !math.IsInf(hi, 0) && !math.IsNaN(hi) {
			if lo > hi {
				set(String(fmt.Sprintf("CONSTRAINT_ERR: clamp min (%s) > max (%s)", rustFloatDisplay(lo), rustFloatDisplay(hi))))
			} else if v, ok := get(); ok {
				if n, ok := valueAsNumber(v); ok {
					if n < lo {
						n = lo
					}
					if n > hi {
						n = hi
					}
					set(numberValue(n))
				}
			}
		}
	}

	// ── :round[:N] ──
	if mb&mkRound != 0 {
		decimals := uint64(0)
		if s, ok := markerArg(markers, markerIndex(markers, "round"), 1); ok {
			if n, ok := parseRustUint(s); ok && n <= math.MaxUint32 {
				decimals = n
			}
		}
		if v, ok := get(); ok {
			if n, ok := valueAsNumber(v); ok {
				factor := powi(10, int32(uint32(decimals)))
				rounded := math.Round(n*factor) / factor
				if decimals == 0 {
					set(Int(f64ToI64(rounded)))
				} else {
					set(Float(rounded))
				}
			}
		}
	}

	// ── :map[:SOURCE_KEY] — `- lookup result` entries ──
	if mb&mkMap != 0 {
		if v, ok := get(); ok {
			if arr, ok := v.(ArrayValue); ok {
				sourceKey, _ := markerArg(markers, markerIndex(markers, "map"), 1)
				lookup := ""
				if sourceKey != "" {
					if res, ok := deepGet(r.rootValue, sourceKey); ok {
						lookup = valueToString(res)
					} else if res, ok := obj.Get(sourceKey); ok {
						lookup = valueToString(res)
					}
				}
				var result Value = Null
				for _, item := range arr.V {
					s, ok := item.(StringValue)
					if !ok {
						continue
					}
					if sp := strings.IndexByte(s.V, ' '); sp >= 0 && strings.TrimSpace(s.V[:sp]) == lookup {
						result = castPrimitive(strings.TrimSpace(s.V[sp+1:]))
						break
					}
				}
				set(result)
			}
		}
	}

	// ── :format:PATTERN ──
	if mb&mkFormat != 0 {
		pattern, ok := markerArg(markers, markerIndex(markers, "format"), 1)
		if !ok {
			pattern = "%s"
		}
		if v, ok := get(); ok {
			set(String(applyFormatPattern(pattern, v)))
		}
	}

	// ── :replace:FROM:TO ──
	if mb&mkReplace != 0 {
		if v, ok := get(); ok {
			if s, ok := v.(StringValue); ok {
				idx := markerIndex(markers, "replace")
				from, _ := markerArg(markers, idx, 1)
				to, _ := markerArg(markers, idx, 2)
				if from != "" {
					set(String(strings.ReplaceAll(s.V, from, to)))
				}
			}
		}
	}

	// ── :sort / :sort:desc — numbers numerically, anything else by code point ──
	if mb&mkSort != 0 {
		if v, ok := get(); ok {
			if arr, ok := v.(ArrayValue); ok {
				desc := false
				if d, ok := markerArg(markers, markerIndex(markers, "sort"), 1); ok {
					desc = d == "desc"
				}
				sorted := make([]Value, len(arr.V))
				copy(sorted, arr.V)
				sort.SliceStable(sorted, func(a, b int) bool {
					an, aok := valueAsNumber(sorted[a])
					bn, bok := valueAsNumber(sorted[b])
					if aok && bok {
						return an < bn
					}
					return valueToString(sorted[a]) < valueToString(sorted[b])
				})
				if desc {
					for i, j := 0, len(sorted)-1; i < j; i, j = i+1, j-1 {
						sorted[i], sorted[j] = sorted[j], sorted[i]
					}
				}
				set(Array(sorted))
			}
		}
	}

	// ── :sum ──
	if mb&mkSum != 0 {
		if v, ok := get(); ok {
			if arr, ok := v.(ArrayValue); ok {
				total := 0.0
				allInt := true
				for _, item := range arr.V {
					switch x := item.(type) {
					case IntValue:
						total += float64(x.V)
					case FloatValue:
						total += x.V
						if !isWhole(x.V) {
							allInt = false
						}
					case StringValue:
						if f, ok := parseRustFloat(x.V); ok {
							total += f
							if !isWhole(f) {
								allInt = false
							}
						}
					}
				}
				if allInt && isWhole(total) && math.Abs(total) < i64MaxF {
					set(Int(f64ToI64(total)))
				} else {
					set(Float(total))
				}
			}
		}
	}

	// ── :fallback:DEFAULT — when the value (a path) is empty or missing ──
	if mb&mkFallback != 0 {
		defaultVal, _ := markerArg(markers, markerIndex(markers, "fallback"), 1)
		useFallback := true
		if v, ok := get(); ok {
			switch x := v.(type) {
			case NullValue:
			case StringValue:
				if x.V != "" {
					if safe, errText := jailPath(r.basePath(), x.V); errText == "" {
						_, err := os.Stat(safe)
						useFallback = err != nil
					}
				}
			default:
				useFallback = false
			}
		}
		if useFallback && defaultVal != "" {
			set(String(defaultVal))
		}
	}

	// ── :once[:uuid|timestamp|random] ──
	if mb&mkOnce != 0 {
		genType, ok := markerArg(markers, markerIndex(markers, "once"), 1)
		if !ok {
			genType = "uuid"
		}
		lockPath := ".synx.lock"
		if r.opts.BasePath != "" {
			lockPath = joinPath(r.opts.BasePath, ".synx.lock")
		}
		if locked, ok := readLockValue(lockPath, key); ok {
			set(String(locked))
		} else {
			generated := r.generateOnce(genType)
			writeLockValue(lockPath, key, generated)
			set(String(generated))
		}
	}

	// ── :version:OP:REQUIRED ──
	if mb&mkVersion != 0 {
		if v, ok := get(); ok {
			if cur, ok := v.(StringValue); ok {
				idx := markerIndex(markers, "version")
				op, ok := markerArg(markers, idx, 1)
				if !ok {
					op = ">="
				}
				required, _ := markerArg(markers, idx, 2)
				set(Bool(compareVersions(cur.V, op, required)))
			}
		}
	}

	// ── :watch[:KEY_PATH] ──
	if mb&mkWatch != 0 {
		if v, ok := get(); ok {
			if filePath, ok := v.(StringValue); ok {
				maxDepth := r.maxIncludeDepth()
				if r.opts.IncludeDepth >= maxDepth {
					set(String(fmt.Sprintf("WATCH_ERR: max include depth (%d) exceeded", maxDepth)))
					return
				}
				full, errText := jailPath(r.basePath(), filePath.V)
				if errText != "" {
					set(String("WATCH_ERR: " + errText))
					return
				}
				if errText := checkFileSize(full); errText != "" {
					set(String("WATCH_ERR: " + errText))
					return
				}
				keyPath, hasKeyPath := markerArg(markers, markerIndex(markers, "watch"), 1)
				content, err := readUTF8(full)
				if err != nil {
					set(String("WATCH_ERR: " + ioErrText(err, filePath.V)))
				} else if hasKeyPath {
					if val, ok := extractFromFileContent(content, keyPath, fileExtension(full)); ok {
						set(val)
					} else {
						set(Null)
					}
				} else {
					set(String(strings.TrimSpace(content)))
				}
			}
		}
	}

	// ── :prompt[:LABEL] ──
	if mb&mkPrompt != 0 {
		label, ok := markerArg(markers, markerIndex(markers, "prompt"), 1)
		if !ok {
			label = key
		}
		if v, ok := get(); ok {
			set(String(label + " (SYNX):\n```synx\n" + stringifyPrompt(v, 0) + "```"))
		}
	}

	// ── custom markers (Options.MarkerFns; stand-in for WASM markers) ──
	if len(r.opts.MarkerFns) > 0 {
		for _, m := range markers {
			if isBuiltinMarker(m) {
				continue
			}
			if fn, ok := r.opts.MarkerFns[m]; ok {
				cur, ok := get()
				if !ok {
					cur = Null
				}
				out := fn(key, meta.Args, cur)
				if out == nil {
					out = Null
				}
				set(out)
			}
		}
	}

	// ── constraints, always last ──
	if meta.Constraints != nil {
		r.validateConstraints(obj, key, meta.Constraints)
	}
}

// ─── :calc substitution ─────────────────────────────────────────────────────

// isCalcSpecialKey: a key whole-word substitution can reach in ways a token
// lookup cannot — one with a non-word byte (`max-hp`, `a.b`, `цена`), or one a
// substituted number can itself spell (`2`, `NaN`, `inf`).
func isCalcSpecialKey(k string) bool {
	allWord, allDigit := true, true
	for i := 0; i < len(k); i++ {
		if !isWordByte(k[i]) {
			allWord = false
		}
		if k[i] < '0' || k[i] > '9' {
			allDigit = false
		}
	}
	return !allWord || allDigit || k == "NaN" || k == "inf"
}

func isWordByte(b byte) bool {
	return (b >= 'a' && b <= 'z') || (b >= 'A' && b <= 'Z') || (b >= '0' && b <= '9') || b == '_'
}

// substituteCalcVars puts numbers into a `:calc` expression: a key of the
// current object first (it shadows the root), then a root key — one pass over
// the expression's words. Only when a key a word cannot name holds a number
// does it fall back to the exact sequential whole-word replacement. false
// when the result exceeds maxCalcResolvedLen.
func (r *resolver) substituteCalcVars(expr string, local *Object, selfKey string) (string, bool) {
	root := r.root
	localIsRoot := root != nil && root == local
	numeric := func(m *Object, k string) (float64, bool) {
		if v, ok := m.Get(k); ok {
			return valueAsNumber(v)
		}
		return 0, false
	}

	special := false
	if root != nil {
		for _, k := range r.rootSpecial {
			if _, ok := numeric(root, k); ok {
				special = true
				break
			}
		}
	}
	if !special && !localIsRoot {
		for _, p := range local.pairs {
			if p.Key == selfKey {
				continue
			}
			if _, ok := valueAsNumber(p.Value); ok && isCalcSpecialKey(p.Key) {
				special = true
				break
			}
		}
	}

	if special {
		resolved := expr
		maps := []*Object{local}
		if !localIsRoot && root != nil {
			maps = append(maps, root)
		}
		for _, m := range maps {
			for _, p := range m.pairs {
				if m == local && p.Key == selfKey {
					continue
				}
				if n, ok := valueAsNumber(p.Value); ok {
					resolved = replaceWord(resolved, p.Key, formatNumber(n))
					if len(resolved) > maxCalcResolvedLen {
						return "", false
					}
				}
			}
		}
		return resolved, true
	}

	var out strings.Builder
	out.Grow(len(expr) + 16)
	i := 0
	for i < len(expr) {
		if isWordByte(expr[i]) {
			start := i
			for i < len(expr) && isWordByte(expr[i]) {
				i++
			}
			token := expr[start:i]
			n, found := 0.0, false
			if token != selfKey {
				n, found = numeric(local, token)
			}
			if !found && root != nil {
				n, found = numeric(root, token)
			}
			if found {
				out.WriteString(formatNumber(n))
			} else {
				out.WriteString(token)
			}
		} else {
			_, size := utf8.DecodeRuneInString(expr[i:])
			out.WriteString(expr[i : i+size])
			i += size
		}
		if out.Len() > maxCalcResolvedLen {
			return "", false
		}
	}
	return out.String(), true
}

// substituteCalcPaths replaces dot-path references (`stats.base_hp`) with
// the number they name.
func (r *resolver) substituteCalcPaths(resolved string) (string, bool) {
	if strings.IndexByte(resolved, '.') < 0 {
		return resolved, true
	}
	var out strings.Builder
	out.Grow(len(resolved))
	i := 0
	for i < len(resolved) {
		if isWordByte(resolved[i]) {
			start := i
			hasDot := false
			for i < len(resolved) && (isWordByte(resolved[i]) || resolved[i] == '.') {
				if resolved[i] == '.' {
					hasDot = true
				}
				i++
			}
			token := resolved[start:i]
			replaced := false
			if hasDot {
				if val, ok := deepGet(r.rootValue, token); ok {
					if n, ok := valueAsNumber(val); ok {
						out.WriteString(formatNumber(n))
						replaced = true
					}
				}
			}
			if !replaced {
				out.WriteString(token)
			}
		} else {
			_, size := utf8.DecodeRuneInString(resolved[i:])
			out.WriteString(resolved[i : i+size])
			i += size
		}
		if out.Len() > maxCalcResolvedLen {
			return "", false
		}
	}
	return out.String(), true
}

// replaceWord replaces whole-word occurrences of `word` with `repl`.
func replaceWord(hay, word, repl string) string {
	if len(word) > len(hay) {
		return hay
	}
	var out strings.Builder
	out.Grow(len(hay))
	i := 0
	for i <= len(hay)-len(word) {
		if out.Len() >= maxEngineScratch {
			break
		}
		if hay[i:i+len(word)] == word {
			beforeOK := i == 0 || !isWordByte(hay[i-1])
			afterOK := i+len(word) >= len(hay) || !isWordByte(hay[i+len(word)])
			if beforeOK && afterOK {
				if room := maxEngineScratch - out.Len(); room > 0 {
					take := len(repl)
					if take > room {
						take = room
					}
					out.WriteString(repl[:floorCharBoundary(repl, take)])
				}
				i += len(word)
				continue
			}
		}
		_, size := utf8.DecodeRuneInString(hay[i:])
		if out.Len() < maxEngineScratch {
			out.WriteString(hay[i : i+size])
		}
		i += size
	}
	if i < len(hay) {
		room := maxEngineScratch - out.Len()
		if room < 0 {
			room = 0
		}
		if room > len(hay)-i {
			room = len(hay) - i
		}
		rest := hay[i:]
		out.WriteString(rest[:floorCharBoundary(rest, room)])
	}
	return out.String()
}

// floorCharBoundary is the largest index ≤ n that starts a rune.
func floorCharBoundary(s string, n int) int {
	if n >= len(s) {
		return len(s)
	}
	for n > 0 && !utf8.RuneStart(s[n]) {
		n--
	}
	return n
}

// ─── lookup and interpolation ───────────────────────────────────────────────

// deepGet looks `path` up as a key of the root first (so a key holding a dot
// is found), then as a dot-path. The value is shared; callers that store it
// elsewhere clone it.
func deepGet(root Value, path string) (Value, bool) {
	obj, ok := root.(ObjectValue)
	if !ok {
		return nil, false
	}
	if v, ok := obj.V.Get(path); ok {
		return v, true
	}
	cur := obj.V
	for {
		dot := strings.IndexByte(path, '.')
		part := path
		if dot >= 0 {
			part = path[:dot]
		}
		v, ok := cur.Get(part)
		if !ok {
			return nil, false
		}
		if dot < 0 {
			return v, true
		}
		path = path[dot+1:]
		o, ok := v.(ObjectValue)
		if !ok {
			return nil, false
		}
		cur = o.V
	}
}

// isRefName: every character alphanumeric (Unicode), `_` or `.` — vacuously
// true for the empty name, as in the reference.
func isRefName(name string) bool {
	for _, c := range name {
		if c != '_' && c != '.' && !isAlphanumeric(c) {
			return false
		}
	}
	return true
}

// isAlphanumeric is Rust's `char::is_alphanumeric`: Unicode Alphabetic or
// Numeric.
func isAlphanumeric(c rune) bool {
	if c < utf8.RuneSelf {
		return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')
	}
	return unicode.IsLetter(c) || unicode.IsNumber(c) || unicode.Is(unicode.Other_Alphabetic, c)
}

// resolveInterpolation expands `{key}`, `{a.b}`, `{key:alias}` and
// `{key:include}` in a string value.
func (r *resolver) resolveInterpolation(tpl string, local *Object) string {
	var out strings.Builder
	if len(tpl) < maxEngineScratch {
		out.Grow(len(tpl))
	}
	writeValue := func(v Value) {
		s := valueToString(v)
		if room := maxEngineScratch - out.Len(); room > 0 {
			take := len(s)
			if take > room {
				take = room
			}
			out.WriteString(s[:floorCharBoundary(s, take)])
		}
	}
	writeLiteral := func(inner string) {
		out.WriteByte('{')
		if rem := maxEngineScratch - (out.Len() + 1); rem > 0 {
			take := len(inner)
			if take > rem {
				take = rem
			}
			out.WriteString(inner[:floorCharBoundary(inner, take)])
		}
		if out.Len() < maxEngineScratch {
			out.WriteByte('}')
		}
	}
	i := 0
	for i < len(tpl) {
		if out.Len() >= maxEngineScratch {
			break
		}
		if tpl[i] == '{' {
			if end := strings.IndexByte(tpl[i+1:], '}'); end >= 0 {
				inner := tpl[i+1 : i+1+end]
				if colon := strings.IndexByte(inner, ':'); colon >= 0 {
					name, scope := inner[:colon], inner[colon+1:]
					if isRefName(name) {
						var val Value
						found := false
						if scope == "include" {
							if r.includes.Len() == 1 {
								val, found = deepGet(r.includes.pairs[0].Value, name)
							}
						} else if inc, ok := r.includes.Get(scope); ok {
							val, found = deepGet(inc, name)
						}
						if found {
							writeValue(val)
						} else {
							writeLiteral(inner)
						}
						i += 2 + end
						continue
					}
				} else if isRefName(inner) {
					val, found := deepGet(r.rootValue, inner)
					if !found {
						val, found = local.Get(inner)
					}
					if found {
						writeValue(val)
					} else {
						writeLiteral(inner)
					}
					i += 2 + end
					continue
				}
			}
		}
		_, size := utf8.DecodeRuneInString(tpl[i:])
		if out.Len() < maxEngineScratch {
			out.WriteString(tpl[i : i+size])
		}
		i += size
	}
	return out.String()
}

// ─── inheritance and secrets ────────────────────────────────────────────────

// applyInheritance merges `child:inherit:p1:p2` left to right: each parent
// overrides the one before, the child overrides all. Each field keeps the
// declarations of the object its value came from; inherited nested subtrees
// are copies carrying their own declarations; a list or scalar child is left
// as written.
func applyInheritance(rootV Value, md *MetadataTree) {
	rootMeta := md.Get("")
	if rootMeta == nil {
		return
	}
	type inherit struct {
		child   string
		parents []string
	}
	var inherits []inherit
	for _, key := range rootMeta.keys {
		meta := rootMeta.items[key]
		idx := markerIndex(meta.Markers, "inherit")
		if idx < 0 {
			continue
		}
		parents := append([]string(nil), meta.Markers[idx+1:]...)
		if len(parents) == 0 && len(meta.Args) > 0 {
			parents = append([]string(nil), meta.Args...)
		}
		if len(parents) > 0 {
			inherits = append(inherits, inherit{key, parents})
		}
	}
	root, ok := rootV.(ObjectValue)
	if !ok {
		return
	}
	for _, in := range inherits {
		cv, ok := root.V.Get(in.child)
		if !ok {
			continue
		}
		child, ok := cv.(ObjectValue)
		if !ok {
			continue
		}
		merged := NewObjectMap()
		source := map[string]string{} // field → parent it came from; absent = the child
		for _, pn := range in.parents {
			pv, ok := root.V.Get(pn)
			if !ok {
				continue
			}
			parent, ok := pv.(ObjectValue)
			if !ok {
				continue
			}
			for _, p := range parent.V.pairs {
				merged.Set(p.Key, CloneValue(p.Value))
				source[p.Key] = pn
			}
		}
		for _, p := range child.V.pairs {
			merged.Set(p.Key, p.Value)
			delete(source, p.Key)
		}

		top := NewMetaMap()
		type nestedMeta struct {
			path string
			mm   *MetaMap
		}
		var nested []nestedMeta
		childSeg := keySegment(in.child)
		for _, p := range merged.pairs {
			k := p.Key
			parentName, inherited := source[k]
			from := in.child
			if inherited {
				from = parentName
			}
			if mm := md.Get(keySegment(from)); mm != nil {
				if m, ok := mm.items[k]; ok {
					top.Set(k, *m)
				}
			}
			if inherited {
				prefix := keySegment(parentName) + "." + keySegment(k)
				for _, path := range md.paths {
					if !strings.HasPrefix(path, prefix) {
						continue
					}
					rest := path[len(prefix):]
					if rest != "" && rest[0] != '.' {
						continue
					}
					nested = append(nested, nestedMeta{childSeg + "." + keySegment(k) + rest, md.levels[path].Clone()})
				}
			}
		}
		root.V.Set(in.child, Object_(merged))
		if top.Len() == 0 {
			md.Remove(childSeg)
		} else {
			md.Set(childSeg, top)
		}
		for _, n := range nested {
			md.Set(n.path, n.mm)
		}
	}
}

// needsResolutionBeforeSecret: markers whose value must be resolved before it
// becomes a secret; those are masked by `:secret` in place.
func needsResolutionBeforeSecret(markers []string) bool {
	for _, m := range markers {
		switch m {
		case "env", "include", "import", "watch", "ref", "alias", "calc", "map", "i18n", "default",
			"fallback", "once", "split", "join", "format", "replace":
			return true
		}
	}
	return false
}

func (r *resolver) maskSecretLiterals(v Value) {
	obj, ok := v.(ObjectValue)
	if !ok {
		return
	}
	if mm := r.meta.levels[string(r.path)]; mm != nil {
		for _, key := range mm.keys {
			meta := mm.items[key]
			if !hasMarker(meta.Markers, "secret") || needsResolutionBeforeSecret(meta.Markers) {
				continue
			}
			if i := obj.V.find(key); i >= 0 {
				switch val := obj.V.pairs[i].Value.(type) {
				case SecretValue, ObjectValue, ArrayValue:
				default:
					obj.V.pairs[i].Value = Secret(valueToString(val))
				}
			}
		}
	}
	baseLen := len(r.path)
	for _, p := range obj.V.pairs {
		switch child := p.Value.(type) {
		case ObjectValue:
			r.pushKey(p.Key)
			r.maskSecretLiterals(child)
			r.path = r.path[:baseLen]
		case ArrayValue:
			r.pushKey(p.Key)
			listLen := len(r.path)
			for idx, item := range child.V {
				if o, ok := item.(ObjectValue); ok {
					r.pushItem(idx)
					r.maskSecretLiterals(o)
					r.path = r.path[:listLen]
				}
			}
			r.path = r.path[:baseLen]
		}
	}
}

// ─── shared helpers ─────────────────────────────────────────────────────────

func hasMarker(markers []string, name string) bool {
	return markerIndex(markers, name) >= 0
}

// wantsString reports whether text taken from outside the document (`:env`,
// `:default`) must stay a string: `(string)` or `[type:string]` asks for it.
func wantsString(meta *Meta) bool {
	return (meta.hasTypeHint() && meta.TypeHint == "string") ||
		(meta.Constraints != nil && meta.Constraints.hasTypeName() && meta.Constraints.TypeName == "string")
}

func castEngineText(text string, forceString bool) Value {
	if forceString {
		return String(text)
	}
	return castPrimitive(text)
}

// castPrimitive types engine-sourced text: quoted text is a string, then
// literals, an i64, an f64 (Rust grammar), else the string.
func castPrimitive(val string) Value {
	if len(val) >= 2 {
		f, l := val[0], val[len(val)-1]
		if (f == '"' && l == '"') || (f == '\'' && l == '\'') {
			return String(val[1 : len(val)-1])
		}
	}
	switch val {
	case "true":
		return Bool(true)
	case "false":
		return Bool(false)
	case "null":
		return Null
	}
	if n, err := strconv.ParseInt(val, 10, 64); err == nil {
		return Int(n)
	}
	if f, ok := parseRustFloat(val); ok {
		return Float(f)
	}
	return String(val)
}

func delimiterFromKeyword(k string) string {
	switch k {
	case "space":
		return " "
	case "pipe":
		return "|"
	case "dash":
		return "-"
	case "dot":
		return "."
	case "semi":
		return ";"
	case "tab":
		return "\t"
	case "slash":
		return "/"
	}
	return k
}

// valueAsNumber: Int and Float only (a bool is not a number to the engine).
func valueAsNumber(v Value) (float64, bool) {
	switch x := v.(type) {
	case IntValue:
		return float64(x.V), true
	case FloatValue:
		return x.V, true
	}
	return 0, false
}

// valueToString is a value as text: an int as an integer, a float as Rust
// `Display` (a whole float as an integer, never an exponent), true/false,
// "null", "" for a group or a list, and "[SECRET]" for a secret, which never
// reaches a string sink as plaintext.
func valueToString(v Value) string {
	switch x := v.(type) {
	case StringValue:
		return x.V
	case IntValue:
		return strconv.FormatInt(x.V, 10)
	case FloatValue:
		return formatNumber(x.V)
	case BoolValue:
		if x.V {
			return "true"
		}
		return "false"
	case NullValue:
		return "null"
	case SecretValue:
		return "[SECRET]"
	}
	return ""
}

func (r *resolver) weightedRandom(items []Value, weights []float64) Value {
	w := append([]float64(nil), weights...)
	if len(w) < len(items) {
		assigned := 0.0
		for _, x := range w {
			assigned += x
		}
		var perItem float64
		if assigned < 100 {
			perItem = (100 - assigned) / float64(len(items)-len(w))
		} else {
			perItem = assigned / float64(len(w))
		}
		for len(w) < len(items) {
			w = append(w, perItem)
		}
	}
	total := 0.0
	for _, x := range w {
		total += x
	}
	if total <= 0 {
		return items[r.rng.Intn(len(items))]
	}
	roll := float64(r.rng.Int63n(10000)) / 10000
	cumulative := 0.0
	for i, item := range items {
		cumulative += w[i] / total
		if roll <= cumulative {
			return item
		}
	}
	return items[len(items)-1]
}

func (r *resolver) generateOnce(genType string) string {
	switch genType {
	case "timestamp":
		return strconv.FormatInt(nowUnix(), 10)
	case "random":
		return strconv.FormatUint(r.rng.Uint64()%math.MaxUint32, 10)
	}
	hi, lo := r.rng.Uint64(), r.rng.Uint64()
	timeHi := ((hi >> 16) & 0x0FFF) | 0x4000
	clkSeq := ((lo >> 48) & 0x3FFF) | 0x8000
	return fmt.Sprintf("%08x-%04x-%04x-%04x-%012x",
		uint32(hi>>32), uint16(hi>>16), uint16(timeHi), uint16(clkSeq), lo&0x0000_FFFF_FFFF_FFFF)
}
