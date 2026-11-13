package synx

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"
)

// Helpers behind the marker bodies in engine.go, each a port of the function
// of the same name in crates/synx-core/src/engine.rs.

// ─── :format ────────────────────────────────────────────────────────────────

func applyFormatPattern(pattern string, v Value) string {
	switch x := v.(type) {
	case IntValue:
		switch {
		case strings.ContainsAny(pattern, "di"):
			return formatIntPattern(pattern, x.V)
		case strings.ContainsAny(pattern, "fe"):
			return formatFloatPattern(pattern, float64(x.V))
		}
		return strconv.FormatInt(x.V, 10)
	case FloatValue:
		switch {
		case strings.ContainsAny(pattern, "fe"):
			return formatFloatPattern(pattern, x.V)
		case strings.ContainsAny(pattern, "di") && isWhole(x.V) && x.V < 9.0e15 && x.V > -9.0e15:
			// A whole float takes an integer pattern: `12.0` under `%05d` is `00012`.
			return formatIntPattern(pattern, f64ToI64(x.V))
		}
		return formatNumber(x.V)
	case StringValue:
		return x.V
	}
	return valueToString(v)
}

func formatIntPattern(pattern string, n int64) string {
	const maxWidth = 4096
	if s, ok := strings.CutPrefix(pattern, "%"); ok {
		inner, ok := strings.CutSuffix(s, "d")
		if !ok {
			inner, ok = strings.CutSuffix(s, "i")
		}
		if ok {
			text := strconv.FormatInt(n, 10)
			if w, ok := strings.CutPrefix(inner, "0"); ok {
				if width, ok := parseRustUint(w); ok {
					return padLeft(text, '0', clampWidth(width, maxWidth))
				}
			}
			if width, ok := parseRustUint(inner); ok {
				return padLeft(text, ' ', clampWidth(width, maxWidth))
			}
		}
	}
	return strconv.FormatInt(n, 10)
}

func clampWidth(w uint64, max int) int {
	if w > uint64(max) {
		return max
	}
	return int(w)
}

// padLeft right-aligns text in `width` characters (Rust `{:0>w$}` / `{:>w$}`:
// the fill goes before a minus sign too).
func padLeft(text string, fill byte, width int) string {
	n := utf8.RuneCountInString(text)
	if n >= width {
		return text
	}
	return strings.Repeat(string(fill), width-n) + text
}

func formatFloatPattern(pattern string, f float64) string {
	const maxPrec = 1024
	if s, ok := strings.CutPrefix(pattern, "%"); ok {
		if s == "e" {
			if f == 0 {
				return "0e+0"
			}
			return jsExponential(f)
		}
		inner, ok := strings.CutSuffix(s, "f")
		if !ok {
			inner, ok = strings.CutSuffix(s, "e")
		}
		if ok {
			if precS, ok := strings.CutPrefix(inner, "."); ok {
				if p, ok := parseRustUint(precS); ok {
					prec := clampWidth(p, maxPrec)
					if strings.HasSuffix(s, "e") {
						return rustLowerExp(f, prec)
					}
					return rustFixed(f, prec)
				}
			}
		}
	}
	return rustFloatDisplay(f)
}

// jsExponential is `%e`: shortest digits with a signed exponent and no
// leading zeros (`1.23456789e+5`), the shape of JS `toExponential()`.
func jsExponential(f float64) string {
	if special, ok := nonFinite(f); ok {
		return special
	}
	raw := strconv.FormatFloat(f, 'e', -1, 64)
	mant, exp, _ := strings.Cut(raw, "e")
	sign := byte('+')
	if exp != "" && (exp[0] == '+' || exp[0] == '-') {
		sign = exp[0]
		exp = exp[1:]
	}
	exp = strings.TrimLeft(exp, "0")
	if exp == "" {
		exp = "0"
	}
	mant = strings.TrimSuffix(mant, ".0")
	return mant + "e" + string(sign) + exp
}

// rustLowerExp is Rust `{:.prec$e}`: `1.23e3`, `1.00e-3`.
func rustLowerExp(f float64, prec int) string {
	if special, ok := nonFinite(f); ok {
		return special
	}
	raw := strconv.FormatFloat(f, 'e', prec, 64)
	mant, exp, _ := strings.Cut(raw, "e")
	neg := strings.HasPrefix(exp, "-")
	exp = strings.TrimLeft(strings.TrimLeft(exp, "+-"), "0")
	if exp == "" {
		exp = "0"
	}
	if neg {
		exp = "-" + exp
	}
	return mant + "e" + exp
}

// rustFixed is Rust `{:.prec$}`.
func rustFixed(f float64, prec int) string {
	if special, ok := nonFinite(f); ok {
		return special
	}
	return strconv.FormatFloat(f, 'f', prec, 64)
}

func nonFinite(f float64) (string, bool) {
	switch {
	case f != f:
		return "NaN", true
	case f > 1.7976931348623157e308:
		return "inf", true
	case f < -1.7976931348623157e308:
		return "-inf", true
	}
	return "", false
}

// powi is LLVM's `__powidf2` — what Rust's `f64::powi` with a run-time
// exponent compiles to — so `:round` scales by exactly the same factor.
func powi(a float64, b int32) float64 {
	recip := b < 0
	r := 1.0
	for {
		if b&1 != 0 {
			r *= a
		}
		b /= 2
		if b == 0 {
			break
		}
		a *= a
	}
	if recip {
		return 1 / r
	}
	return r
}

// ─── :version ───────────────────────────────────────────────────────────────

// leadingInt is the integer at the start of s (after whitespace, with an
// optional sign), or 0 — JavaScript's `parseInt(s, 10) || 0`.
func leadingInt(s string) int64 {
	t := strings.TrimLeftFunc(s, isSpaceRune)
	neg := false
	if t != "" && (t[0] == '-' || t[0] == '+') {
		neg = t[0] == '-'
		t = t[1:]
	}
	var n int64
	for i := 0; i < len(t) && t[i] >= '0' && t[i] <= '9'; i++ {
		d := int64(t[i] - '0')
		if n > (1<<63-1-d)/10 {
			n = 1<<63 - 1
		} else {
			n = n*10 + d
		}
	}
	if neg {
		return -n
	}
	return n
}

// compareVersions reads each dot-separated part for its leading integer, a
// part with none counting as 0, so positions never shift (`1.2.3-beta` is
// 1.2.3).
func compareVersions(current, op, required string) bool {
	cv := strings.Split(current, ".")
	rv := strings.Split(required, ".")
	n := len(cv)
	if len(rv) > n {
		n = len(rv)
	}
	ord := 0
	for i := 0; i < n; i++ {
		var a, b int64
		if i < len(cv) {
			a = leadingInt(cv[i])
		}
		if i < len(rv) {
			b = leadingInt(rv[i])
		}
		if a != b {
			if a < b {
				ord = -1
			} else {
				ord = 1
			}
			break
		}
	}
	switch op {
	case ">=":
		return ord >= 0
	case "<=":
		return ord <= 0
	case ">":
		return ord > 0
	case "<":
		return ord < 0
	case "==", "=":
		return ord == 0
	case "!=":
		return ord != 0
	}
	return false
}

// ─── :i18n plural rules ─────────────────────────────────────────────────────

// pluralCategory is the CLDR plural category for a language and an integer.
func pluralCategory(lang string, n int64) string {
	abs := uint64(n)
	if n < 0 {
		abs = uint64(-(n + 1)) + 1
	}
	n10, n100 := abs%10, abs%100
	switch lang {
	case "ru", "uk", "be", "pl":
		switch {
		case n10 == 1 && n100 != 11:
			return "one"
		case n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14):
			return "few"
		}
		return "many"
	case "cs", "sk":
		switch {
		case abs == 1:
			return "one"
		case abs >= 2 && abs <= 4:
			return "few"
		}
		return "other"
	case "ar":
		switch {
		case abs == 0:
			return "zero"
		case abs == 1:
			return "one"
		case abs == 2:
			return "two"
		case n100 >= 3 && n100 <= 10:
			return "few"
		case n100 >= 11 && n100 <= 99:
			return "many"
		}
		return "other"
	case "fr", "pt":
		if abs <= 1 {
			return "one"
		}
		return "other"
	case "ja", "zh", "ko", "vi", "th":
		return "other"
	}
	if abs == 1 {
		return "one"
	}
	return "other"
}

// ─── :prompt ────────────────────────────────────────────────────────────────

// stringifyPrompt renders a value as SYNX text for `:prompt`.
func stringifyPrompt(v Value, indent int) string {
	spaces := strings.Repeat(" ", indent)
	var sb strings.Builder
	switch x := v.(type) {
	case ObjectValue:
		for _, key := range x.V.SortedKeys() {
			val, _ := x.V.Get(key)
			switch inner := val.(type) {
			case ObjectValue:
				sb.WriteString(spaces + key + "\n")
				sb.WriteString(stringifyPrompt(val, indent+2))
			case ArrayValue:
				sb.WriteString(spaces + key + "\n")
				for _, item := range inner.V {
					sb.WriteString(spaces + "  - " + valueToString(item) + "\n")
				}
			default:
				sb.WriteString(spaces + key + " " + valueToString(val) + "\n")
			}
		}
	case ArrayValue:
		for _, item := range x.V {
			sb.WriteString(spaces + "  - " + valueToString(item) + "\n")
		}
	default:
		sb.WriteString(spaces + valueToString(v) + "\n")
	}
	return sb.String()
}

// ─── :spam ──────────────────────────────────────────────────────────────────

var (
	spamMu      sync.Mutex
	spamBuckets = map[string][]time.Time{}
)

func allowSpamAccess(bucket string, maxCalls, windowSec uint64) bool {
	now := time.Now()
	if windowSec < 1 {
		windowSec = 1
	}
	window := time.Duration(windowSec) * time.Second
	if windowSec > uint64(1<<62)/uint64(time.Second) {
		window = time.Duration(1<<63 - 1)
	}
	spamMu.Lock()
	defer spamMu.Unlock()
	calls := spamBuckets[bucket]
	kept := calls[:0]
	for _, ts := range calls {
		if now.Sub(ts) <= window {
			kept = append(kept, ts)
		}
	}
	if uint64(len(kept)) >= maxCalls {
		spamBuckets[bucket] = kept
		return false
	}
	spamBuckets[bucket] = append(kept, now)
	return true
}

func nowUnix() int64 { return time.Now().Unix() }

// ─── :once lock file ────────────────────────────────────────────────────────

func readLockValue(lockPath, key string) (string, bool) {
	content, err := os.ReadFile(lockPath)
	if err != nil || !utf8.Valid(content) {
		return "", false
	}
	for _, line := range rustLines(string(content)) {
		if rest, ok := strings.CutPrefix(line, key); ok && strings.HasPrefix(rest, " ") {
			return strings.TrimLeftFunc(rest, isSpaceRune), true
		}
	}
	return "", false
}

func writeLockValue(lockPath, key, value string) {
	var lines []string
	if content, err := os.ReadFile(lockPath); err == nil && utf8.Valid(content) {
		lines = rustLines(string(content))
	}
	newLine := key + " " + value
	found := false
	for i, line := range lines {
		if strings.HasPrefix(line, key) && strings.HasPrefix(line[len(key):], " ") {
			lines[i] = newLine
			found = true
			break
		}
	}
	if !found {
		lines = append(lines, newLine)
	}
	_ = os.WriteFile(lockPath, []byte(strings.Join(lines, "\n")+"\n"), 0o644)
}

// rustLines is Rust's `str::lines`: split on `\n`, drop one trailing `\r`
// per line, no empty last line.
func rustLines(s string) []string {
	if s == "" {
		return nil
	}
	s = strings.TrimSuffix(s, "\n")
	parts := strings.Split(s, "\n")
	for i, p := range parts {
		parts[i] = strings.TrimSuffix(p, "\r")
	}
	return parts
}

// ─── files: jail, size, reads ───────────────────────────────────────────────

// jailPath keeps `file` inside `base`. It returns the path to read, or the
// reason it was refused (the text of the reference's SECURITY errors).
func jailPath(base, file string) (string, string) {
	if file != "" && (file[0] == '/' || file[0] == '\\') {
		return "", fmt.Sprintf("SECURITY: rooted paths are not allowed: '%s'", file)
	}
	if filepath.IsAbs(file) {
		return "", fmt.Sprintf("SECURITY: absolute paths are not allowed: '%s'", file)
	}
	// safety: `NUL`, `con.txt` or `COM1` in any directory opens a device on
	// Windows — `:include NUL` read it as an empty document, and a console or
	// a serial port blocks the read. synx-core reaches the disk through a
	// verbatim path, where these are ordinary missing files.
	if runtime.GOOS == "windows" && hasDeviceName(file) {
		return "", "file not found: " + file
	}
	// A base that cannot be resolved refuses instead of jailing nothing.
	if base == "" {
		base = "."
	}
	baseCanon, err := canonicalize(base)
	if err != nil {
		return "", fmt.Sprintf("SECURITY: base directory not found: '%s'", base)
	}
	full := joinPath(baseCanon, file)
	fullCanon, err := canonicalize(full)
	if err != nil {
		// The file may not exist yet — at least verify no `..` escapes. Judged
		// by the path as the document wrote it, segment by segment: a base
		// directory named `a..b` is no traversal.
		for _, seg := range strings.FieldsFunc(file, func(r rune) bool { return r == '/' || r == '\\' }) {
			if seg == ".." {
				return "", fmt.Sprintf("SECURITY: path traversal detected: '%s'", file)
			}
		}
		if !pathHasPrefix(full, baseCanon) {
			return "", fmt.Sprintf("SECURITY: path escapes base directory: '%s'", file)
		}
		return full, ""
	}
	if !pathHasPrefix(fullCanon, baseCanon) {
		return "", fmt.Sprintf("SECURITY: path escapes base directory: '%s'", file)
	}
	return fullCanon, ""
}

// hasDeviceName reports a path component that names a DOS device, with or
// without an extension or a stream: `NUL`, `con.txt`, `sub/COM1`, `aux:x`.
func hasDeviceName(file string) bool {
	for _, part := range strings.FieldsFunc(file, func(r rune) bool { return r == '/' || r == '\\' }) {
		stem := part
		if i := strings.IndexAny(stem, ".:"); i >= 0 {
			stem = stem[:i]
		}
		stem = strings.ToUpper(strings.TrimRight(stem, " "))
		switch stem {
		case "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$":
			return true
		}
		if strings.HasPrefix(stem, "COM") || strings.HasPrefix(stem, "LPT") {
			switch stem[3:] {
			case "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "\u00b9", "\u00b2", "\u00b3":
				return true
			}
		}
	}
	return false
}

// canonicalize is `fs::canonicalize`: absolute, symlinks resolved, and an
// error when the path does not exist.
func canonicalize(p string) (string, error) {
	abs, err := filepath.Abs(p)
	if err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(abs)
}

// joinPath is `Path::join`: no cleaning, and an absolute second part
// replaces the first.
func joinPath(a, b string) string {
	// A drive-relative path (`C:x`) replaces the first part as well.
	if filepath.IsAbs(b) || a == "" || (runtime.GOOS == "windows" && filepath.VolumeName(b) != "") {
		return b
	}
	if strings.HasSuffix(a, "/") || strings.HasSuffix(a, string(filepath.Separator)) {
		return a + b
	}
	return a + string(filepath.Separator) + b
}

func isPathSep(c byte) bool {
	return c == '/' || (runtime.GOOS == "windows" && c == '\\')
}

// pathComponents splits a path the way `Path::components` compares it:
// empty and `.` components (other than a leading one) are dropped.
func pathComponents(p string) []string {
	var out []string
	start := 0
	for i := 0; i <= len(p); i++ {
		if i == len(p) || isPathSep(p[i]) {
			c := p[start:i]
			switch {
			case i == 0 && len(p) > 0:
				out = append(out, "/")
			case c == "" || (c == "." && len(out) > 0):
			default:
				out = append(out, c)
			}
			start = i + 1
		}
	}
	return out
}

// pathHasPrefix is `Path::starts_with`: whole components.
func pathHasPrefix(p, base string) bool {
	pc, bc := pathComponents(p), pathComponents(base)
	if len(bc) > len(pc) {
		return false
	}
	for i := range bc {
		same := pc[i] == bc[i]
		if !same && runtime.GOOS == "windows" {
			same = strings.EqualFold(pc[i], bc[i])
		}
		if !same {
			return false
		}
	}
	return true
}

func parentDir(p string) string {
	return filepath.Dir(p)
}

// fileExtension is `Path::extension`: none for a dot-file without another dot.
func fileExtension(p string) string {
	name := filepath.Base(p)
	dot := strings.LastIndexByte(name, '.')
	if dot <= 0 {
		return ""
	}
	return name[dot+1:]
}

func checkFileSize(p string) string {
	if fi, err := os.Stat(p); err == nil && fi.Size() > maxFileSize {
		return fmt.Sprintf("SECURITY: file too large (%d bytes, max %d)", fi.Size(), maxFileSize)
	}
	return ""
}

var errInvalidUTF8 = errors.New("stream did not contain valid UTF-8")

// readUTF8 is `fs::read_to_string`: the file must be valid UTF-8.
func readUTF8(p string) (string, error) {
	data, err := os.ReadFile(p)
	if err != nil {
		return "", err
	}
	if !utf8.Valid(data) {
		return "", errInvalidUTF8
	}
	return string(data), nil
}

// ioErrText is the reference's portable I/O error text.
//
// spec: it never carries the operating system's own wording, so the same
// document resolves to the same string on every machine. A name the platform
// cannot hold at all (a NUL, or `|` on Windows) is a file that is not there.
func ioErrText(err error, ctx string) string {
	switch {
	case errors.Is(err, os.ErrNotExist), invalidFileName(err):
		return "file not found: " + ctx
	case errors.Is(err, os.ErrPermission):
		return "permission denied: " + ctx
	}
	return "cannot read file: " + ctx
}

func isRegularFile(p string) bool {
	fi, err := os.Stat(p)
	return err == nil && fi.Mode().IsRegular()
}

// ─── !include / !use ────────────────────────────────────────────────────────

func (r *resolver) loadIncludes(directives []IncludeDirective) *Object {
	out := NewObjectMap()
	if r.opts.IncludeDepth >= r.maxIncludeDepth() {
		return out
	}
	for _, inc := range directives {
		full, errText := jailPath(r.basePath(), inc.Path)
		if errText != "" || checkFileSize(full) != "" {
			continue
		}
		if included, errText := r.includeFile(full, full, inc.Path); errText == "" {
			out.Set(inc.Alias, included)
		}
	}
	return out
}

// loadPackages reads `<base>/<packages>/@scope/name` for each `!use`: the
// manifest's `main` (or legacy `entry`), else `src/main.synx`.
func (r *resolver) loadPackages(directives []UseDirective) *Object {
	out := NewObjectMap()
	// safety: a package may `!use` another. The depth was counted but never
	// checked, and nothing noticed a package reaching itself: packages using
	// one another recursed until the runtime ended the process.
	if len(directives) == 0 || r.opts.IncludeDepth >= r.maxIncludeDepth() {
		return out
	}
	pkgBase := r.opts.PackagesPath
	if pkgBase == "" {
		pkgBase = "./synx_packages"
	}
	pkgRoot := joinPath(r.basePath(), pkgBase)
	for _, ud := range directives {
		if !isContainedRelative(ud.Package) {
			continue
		}
		pkgDir := joinPath(pkgRoot, ud.Package)
		entry, ok := readManifestMain(pkgDir)
		if !ok {
			entry = joinPath(joinPath(pkgDir, "src"), "main.synx")
		}
		if !isRegularFile(entry) || checkFileSize(entry) != "" {
			continue
		}
		opened, err := canonicalize(entry)
		if err != nil {
			opened = entry
		}
		if included, errText := r.includeFile(opened, entry, ud.Package); errText == "" {
			out.Set(ud.Alias, included)
		}
	}
	return out
}

func readManifestMain(pkgDir string) (string, bool) {
	text, err := readUTF8(joinPath(pkgDir, "synx-pkg.synx"))
	if err != nil {
		return "", false
	}
	for _, line := range rustLines(text) {
		t := trimWS(line)
		if rest, ok := strings.CutPrefix(t, "main "); ok {
			if p := trimWS(rest); isContainedRelative(p) {
				return joinPath(pkgDir, p), true
			}
		}
		if rest, ok := strings.CutPrefix(t, "entry "); ok {
			if p := trimWS(rest); isContainedRelative(p) {
				return joinPath(pkgDir, p), true
			}
		}
	}
	return "", false
}

// isContainedRelative: a relative path with no root, drive, `:` or `..`
// segment, judged by its text, so a package directory that is itself a symlink
// (a linked local package) keeps working. A `!use` name or manifest `main`
// that fails it would otherwise read files outside the packages directory.
func isContainedRelative(p string) bool {
	if p == "" || p[0] == '/' || p[0] == '\\' || strings.IndexByte(p, ':') >= 0 || filepath.IsAbs(p) {
		return false
	}
	for _, seg := range strings.FieldsFunc(p, func(r rune) bool { return r == '/' || r == '\\' }) {
		if seg == ".." {
			return false
		}
	}
	return true
}

// ─── :watch key paths ───────────────────────────────────────────────────────

// extractFromFileContent follows a dot-path through a JSON or SYNX file.
func extractFromFileContent(content, keyPath, ext string) (Value, bool) {
	if ext == "json" {
		dec := json.NewDecoder(strings.NewReader(content))
		dec.UseNumber()
		var parsed interface{}
		if err := dec.Decode(&parsed); err != nil {
			return nil, false
		}
		if _, err := dec.Token(); err != io.EOF {
			return nil, false
		}
		cur := parsed
		for _, part := range strings.Split(keyPath, ".") {
			m, ok := cur.(map[string]interface{})
			if !ok {
				return nil, false
			}
			if cur, ok = m[part]; !ok {
				return nil, false
			}
		}
		return jsonToValue(cur), true
	}
	parsed := Parse(content)
	cur := parsed.Root
	for _, part := range strings.Split(keyPath, ".") {
		obj, ok := cur.(ObjectValue)
		if !ok {
			return nil, false
		}
		if cur, ok = obj.V.Get(part); !ok {
			return nil, false
		}
	}
	return cur, true
}

func jsonToValue(v interface{}) Value {
	switch x := v.(type) {
	case nil:
		return Null
	case bool:
		return Bool(x)
	case json.Number:
		s := x.String()
		if !strings.ContainsAny(s, ".eE") {
			if n, err := strconv.ParseInt(s, 10, 64); err == nil {
				return Int(n)
			}
		}
		if f, err := strconv.ParseFloat(s, 64); err == nil {
			return Float(f)
		}
		return Null
	case string:
		return String(x)
	case []interface{}:
		out := make([]Value, len(x))
		for i, item := range x {
			out[i] = jsonToValue(item)
		}
		return Array(out)
	case map[string]interface{}:
		keys := make([]string, 0, len(x))
		for k := range x {
			keys = append(keys, k)
		}
		sort.Strings(keys)
		obj := newObjectCap(len(keys))
		for _, k := range keys {
			obj.Set(k, jsonToValue(x[k]))
		}
		return Object_(obj)
	}
	return Null
}

func isSpaceRune(r rune) bool { return unicode.IsSpace(r) }
