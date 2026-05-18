package synx

import (
	"fmt"
	"math"
	"os"
	"sort"
	"strconv"
	"strings"
	"unicode"
)

// All 27 marker bodies live here as methods on *resolver. The dispatch
// table is in engine.go.

func (r *resolver) applyEnv(v Value, meta Meta) Value {
	if r.opts.Env == nil {
		return v
	}
	varName := valueToString(v)
	fallback := ""
	idx := meta.MarkerIndex("env")
	if idx >= 0 && idx+1 < len(meta.Markers) && meta.Markers[idx+1] == "default" && idx+2 < len(meta.Markers) {
		fallback = meta.Markers[idx+2]
	}
	if val, ok := r.opts.Env[varName]; ok {
		return String(val)
	}
	if fallback != "" {
		return String(fallback)
	}
	return Null
}

func (r *resolver) applyDefault(v Value, meta Meta) Value {
	if meta.HasMarker("env") {
		return v
	}
	empty := false
	if _, ok := v.(NullValue); ok {
		empty = true
	} else if s, ok := v.(StringValue); ok && s.V == "" {
		empty = true
	}
	if !empty {
		return v
	}
	idx := meta.MarkerIndex("default")
	if idx >= 0 && idx+1 < len(meta.Markers) {
		return String(meta.Markers[idx+1])
	}
	return v
}

func (r *resolver) applyCalc(v Value) Value {
	expr := valueToString(v)
	if expr == "" {
		return v
	}
	expr = r.interpolate(expr)
	// Collect numeric sibling identifiers from the root scope.
	pairs := make([]struct {
		key string
		val float64
	}, 0)
	for _, p := range r.root().Pairs() {
		if d, ok := AsNumber(p.Value); ok {
			pairs = append(pairs, struct {
				key string
				val float64
			}{p.Key, d})
		}
	}
	sort.Slice(pairs, func(a, b int) bool { return len(pairs[a].key) > len(pairs[b].key) })
	for _, p := range pairs {
		expr = replaceWord(expr, p.key, fmt.Sprintf("%.17g", p.val))
	}

	res := SafeCalc(expr)
	if !res.OK {
		return v
	}
	if math.Floor(res.Value) == res.Value && math.Abs(res.Value) < 9.2233720368547758e18 {
		return Int(int64(res.Value))
	}
	return Float(res.Value)
}

func (r *resolver) applyRef(v Value) Value {
	path := valueToString(v)
	if path == "" {
		return v
	}
	if target := r.lookup(path); target != nil {
		return target
	}
	return v
}

func (r *resolver) applySecret(v Value) Value {
	if _, ok := v.(SecretValue); ok {
		return v
	}
	if _, ok := v.(NullValue); ok {
		return v
	}
	if s, ok := v.(StringValue); ok {
		return Secret(s.V)
	}
	return Secret(valueToString(v))
}

func (r *resolver) applyRandom(v Value, meta Meta) Value {
	var opts []string
	if arr, ok := AsArray(v); ok {
		for _, item := range arr {
			opts = append(opts, valueToString(item))
		}
	} else if s, ok := AsString(v); ok {
		for _, part := range strings.Split(s, ",") {
			opts = append(opts, strings.TrimSpace(part))
		}
	}
	if len(opts) == 0 {
		return v
	}
	weights := make([]float64, 0, len(meta.Args))
	for _, a := range meta.Args {
		w, err := strconv.ParseFloat(a, 64)
		if err != nil {
			w = 1
		}
		weights = append(weights, w)
	}
	for len(weights) < len(opts) {
		weights = append(weights, 1)
	}
	total := 0.0
	for _, w := range weights {
		total += w
	}
	if total <= 0 {
		return v
	}
	pick := r.rng.Float64() * total
	acc := 0.0
	for i, w := range weights {
		acc += w
		if pick <= acc {
			return String(opts[i])
		}
	}
	return String(opts[len(opts)-1])
}

func (r *resolver) applyUnique(v Value) Value {
	arr, ok := AsArray(v)
	if !ok {
		return v
	}
	out := make([]Value, 0, len(arr))
	for _, item := range arr {
		dup := false
		for _, seen := range out {
			if Equal(seen, item) {
				dup = true
				break
			}
		}
		if !dup {
			out = append(out, item)
		}
	}
	return Array(out)
}

func (r *resolver) applyGeo(v Value, meta Meta) Value {
	if r.opts.Region == "" {
		return v
	}
	for _, a := range meta.Args {
		colon := strings.IndexByte(a, ':')
		if colon < 0 {
			continue
		}
		if a[:colon] == r.opts.Region {
			return String(a[colon+1:])
		}
	}
	return v
}

func (r *resolver) applyI18n(v Value, meta Meta) Value {
	lang := r.opts.Lang
	if lang == "" {
		lang = "en"
	}
	lang2 := lang
	if len(lang) >= 2 {
		lang2 = lang[:2]
	}
	n, isNum := AsNumber(v)
	category := "other"
	if isNum {
		category = pluralCategory(lang, n)
	}
	keys := []string{
		lang + "." + category,
		lang2 + "." + category,
		lang,
		lang2,
		"other",
	}
	for _, key := range keys {
		for _, a := range meta.Args {
			colon := strings.IndexByte(a, ':')
			if colon < 0 {
				continue
			}
			if a[:colon] == key {
				return String(a[colon+1:])
			}
		}
	}
	return v
}

func (r *resolver) applySplit(v Value, meta Meta) Value {
	s, ok := AsString(v)
	if !ok {
		return v
	}
	sep := ","
	idx := meta.MarkerIndex("split")
	if idx >= 0 && idx+1 < len(meta.Markers) {
		sep = meta.Markers[idx+1]
	}
	out := []Value{}
	if sep == "" {
		for _, c := range s {
			out = append(out, String(string(c)))
		}
	} else {
		for _, part := range strings.Split(s, sep) {
			out = append(out, String(strings.TrimSpace(part)))
		}
	}
	return Array(out)
}

func (r *resolver) applyJoin(v Value, meta Meta) Value {
	arr, ok := AsArray(v)
	if !ok {
		return v
	}
	sep := ","
	idx := meta.MarkerIndex("join")
	if idx >= 0 && idx+1 < len(meta.Markers) {
		sep = meta.Markers[idx+1]
	}
	parts := make([]string, len(arr))
	for i, x := range arr {
		parts[i] = valueToString(x)
	}
	return String(strings.Join(parts, sep))
}

func (r *resolver) applyClamp(v Value, meta Meta) Value {
	d, ok := AsNumber(v)
	if !ok {
		return v
	}
	lo, hi := math.Inf(-1), math.Inf(1)
	idx := meta.MarkerIndex("clamp")
	if idx >= 0 && idx+2 < len(meta.Markers) {
		if x, err := strconv.ParseFloat(meta.Markers[idx+1], 64); err == nil {
			lo = x
		}
		if x, err := strconv.ParseFloat(meta.Markers[idx+2], 64); err == nil {
			hi = x
		}
	}
	if d < lo {
		d = lo
	}
	if d > hi {
		d = hi
	}
	if _, ok := v.(IntValue); ok {
		return Int(int64(d))
	}
	return Float(d)
}

func (r *resolver) applyRound(v Value, meta Meta) Value {
	d, ok := AsNumber(v)
	if !ok {
		return v
	}
	digits := 0
	idx := meta.MarkerIndex("round")
	if idx >= 0 && idx+1 < len(meta.Markers) {
		if n, err := strconv.Atoi(meta.Markers[idx+1]); err == nil {
			digits = n
		}
	}
	factor := math.Pow(10, float64(digits))
	rounded := math.Round(d*factor) / factor
	if digits == 0 {
		return Int(int64(rounded))
	}
	return Float(rounded)
}

func (r *resolver) applyMap(v Value, meta Meta) Value {
	key := valueToString(v)
	for _, a := range meta.Args {
		colon := strings.IndexByte(a, ':')
		if colon < 0 {
			continue
		}
		if a[:colon] == key {
			return String(a[colon+1:])
		}
	}
	return v
}

func (r *resolver) applyFormat(v Value, meta Meta) Value {
	idx := meta.MarkerIndex("format")
	if idx < 0 || idx+1 >= len(meta.Markers) {
		return v
	}
	pattern := r.interpolate(meta.Markers[idx+1])
	n, _ := AsNumber(v)
	sIn := valueToString(v)
	return String(applyPrintf(pattern, n, sIn))
}

func (r *resolver) applyReplace(v Value, meta Meta) Value {
	s, ok := AsString(v)
	if !ok {
		return v
	}
	idx := meta.MarkerIndex("replace")
	if idx < 0 || idx+2 >= len(meta.Markers) {
		return v
	}
	from := meta.Markers[idx+1]
	to := meta.Markers[idx+2]
	if from == "" {
		return v
	}
	return String(strings.ReplaceAll(s, from, to))
}

func (r *resolver) applySort(v Value, meta Meta) Value {
	arr, ok := AsArray(v)
	if !ok {
		return v
	}
	desc := false
	idx := meta.MarkerIndex("sort")
	if idx >= 0 && idx+1 < len(meta.Markers) {
		desc = meta.Markers[idx+1] == "desc"
	}
	out := make([]Value, len(arr))
	copy(out, arr)
	sort.SliceStable(out, func(a, b int) bool {
		da, na := AsNumber(out[a])
		db, nb := AsNumber(out[b])
		if na && nb {
			if desc {
				return da > db
			}
			return da < db
		}
		sa := valueToString(out[a])
		sb := valueToString(out[b])
		if desc {
			return sa > sb
		}
		return sa < sb
	})
	return Array(out)
}

func (r *resolver) applySum(v Value) Value {
	arr, ok := AsArray(v)
	if !ok {
		return v
	}
	total := 0.0
	anyFloat := false
	for _, item := range arr {
		if d, ok := AsNumber(item); ok {
			total += d
			if _, ok := item.(FloatValue); ok {
				anyFloat = true
			}
		}
	}
	if anyFloat {
		return Float(total)
	}
	return Int(int64(total))
}

func (r *resolver) applyFallback(v Value, meta Meta) Value {
	empty := false
	if _, ok := v.(NullValue); ok {
		empty = true
	} else if s, ok := v.(StringValue); ok && s.V == "" {
		empty = true
	}
	if !empty {
		return v
	}
	idx := meta.MarkerIndex("fallback")
	if idx >= 0 && idx+1 < len(meta.Markers) {
		return String(meta.Markers[idx+1])
	}
	return v
}

func (r *resolver) applyVersion(v Value) Value {
	if _, ok := v.(StringValue); ok {
		return v
	}
	return String(valueToString(v))
}

func (r *resolver) applyWatch(v Value) Value {
	rel, ok := AsString(v)
	if !ok {
		return v
	}
	base := r.opts.BasePath
	if base == "" {
		base = "."
	}
	safe := r.jailPath(base, rel)
	if safe == "" {
		return v
	}
	data, err := readFile(safe)
	if err != nil {
		return v
	}
	return String(data)
}

func (r *resolver) applyPrompt(v Value) Value {
	s, ok := AsString(v)
	if !ok {
		return v
	}
	return String(r.interpolate(s))
}

func (r *resolver) applySpam(v Value, key string) Value {
	spamMu.Lock()
	defer spamMu.Unlock()
	if _, ok := spamBuckets[key]; ok {
		return Null
	}
	spamBuckets[key] = struct{}{}
	return v
}

// ─── shared utilities ───────────────────────────────────────────────────────

func readFile(p string) (string, error) {
	data, err := os.ReadFile(p)
	if err != nil {
		return "", err
	}
	return string(data), nil
}

func replaceWord(s, word, repl string) string {
	if word == "" {
		return s
	}
	var sb strings.Builder
	sb.Grow(len(s) + 16)
	i := 0
	for i < len(s) {
		idx := strings.Index(s[i:], word)
		if idx < 0 {
			sb.WriteString(s[i:])
			break
		}
		found := i + idx
		sb.WriteString(s[i:found])
		leftOK := found == 0 || !isWordRune(rune(s[found-1]))
		after := found + len(word)
		rightOK := after == len(s) || !isWordRune(rune(s[after]))
		if leftOK && rightOK {
			sb.WriteString(repl)
			i = after
		} else {
			sb.WriteByte(s[found])
			i = found + 1
		}
	}
	return sb.String()
}

func isWordRune(r rune) bool {
	return unicode.IsLetter(r) || unicode.IsDigit(r) || r == '_'
}

func applyPrintf(pattern string, number float64, sIn string) string {
	var sb strings.Builder
	sb.Grow(len(pattern) + 16)
	i := 0
	for i < len(pattern) {
		c := pattern[i]
		if c != '%' {
			sb.WriteByte(c)
			i++
			continue
		}
		if i+1 < len(pattern) && pattern[i+1] == '%' {
			sb.WriteByte('%')
			i += 2
			continue
		}
		end := i + 1
		for end < len(pattern) {
			k := pattern[end]
			if k == 'd' || k == 'i' || k == 'f' || k == 'e' || k == 'g' || k == 's' {
				break
			}
			end++
		}
		if end >= len(pattern) {
			sb.WriteString(pattern[i:])
			break
		}
		// Convert C-style printf to Go's fmt directives.
		spec := pattern[i : end+1]
		kind := pattern[end]
		goSpec := spec
		if kind == 'i' {
			goSpec = spec[:len(spec)-1] + "d"
		}
		switch kind {
		case 'd', 'i':
			sb.WriteString(fmt.Sprintf(goSpec, int64(number)))
		case 'f', 'e', 'g':
			sb.WriteString(fmt.Sprintf(goSpec, number))
		case 's':
			sb.WriteString(fmt.Sprintf(goSpec, sIn))
		}
		i = end + 1
	}
	return sb.String()
}

func pluralCategory(lang string, n float64) string {
	two := lang
	if len(lang) >= 2 {
		two = lang[:2]
	}
	intN := int64(math.Floor(math.Abs(n)))
	mod10 := intN % 10
	mod100 := intN % 100
	intLike := n == math.Floor(n)

	switch two {
	case "ru", "uk", "be":
		if intLike && mod10 == 1 && mod100 != 11 {
			return "one"
		}
		if intLike && mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14) {
			return "few"
		}
		if intLike && (mod10 == 0 || (mod10 >= 5 && mod10 <= 9) || (mod100 >= 11 && mod100 <= 14)) {
			return "many"
		}
		return "other"
	case "pl":
		if intLike && n == 1 {
			return "one"
		}
		if intLike && mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14) {
			return "few"
		}
		if intLike && n != 1 && (mod10 == 0 || mod10 == 1 ||
			(mod10 >= 5 && mod10 <= 9) || (mod100 >= 12 && mod100 <= 14)) {
			return "many"
		}
		return "other"
	case "cs", "sk":
		if intLike && n == 1 {
			return "one"
		}
		if intLike && intN >= 2 && intN <= 4 {
			return "few"
		}
		if !intLike {
			return "many"
		}
		return "other"
	case "ar":
		if n == 0 {
			return "zero"
		}
		if n == 1 {
			return "one"
		}
		if n == 2 {
			return "two"
		}
		if intLike && mod100 >= 3 && mod100 <= 10 {
			return "few"
		}
		if intLike && mod100 >= 11 {
			return "many"
		}
		return "other"
	case "fr", "pt":
		if n >= 0 && n < 2 {
			return "one"
		}
		return "other"
	case "ja", "zh", "ko", "vi", "th":
		return "other"
	}
	if n == 1 {
		return "one"
	}
	return "other"
}
