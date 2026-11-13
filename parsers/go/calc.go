package synx

import (
	"math"
	"strconv"
	"strings"
	"unicode/utf8"
)

// CalcResult is the outcome of evaluating a `:calc` expression.
type CalcResult struct {
	OK    bool
	Value float64
	Error string
}

// SafeCalc evaluates an arithmetic expression with no variable side-effects:
// +, -, *, /, %, parentheses, integers, floats and unary minus. Variable
// references must be substituted with literals before calling. Error texts
// are those of crates/synx-core/src/calc.rs, character for character.
func SafeCalc(expr string) CalcResult {
	t := trimWS(expr)
	if t == "" {
		return CalcResult{OK: true}
	}
	tokens, err := calcTokenize(t)
	if err != "" {
		return CalcResult{Error: err}
	}
	if len(tokens) == 0 {
		return CalcResult{OK: true}
	}
	v, err := calcEvaluate(tokens)
	if err != "" {
		return CalcResult{Error: err}
	}
	return CalcResult{OK: true, Value: v}
}

type calcKind uint8

const (
	calcNumber calcKind = iota
	calcOp
	calcLParen
	calcRParen
)

type calcToken struct {
	kind   calcKind
	number float64
	op     byte
}

func calcTokenize(expr string) ([]calcToken, string) {
	tokens := make([]calcToken, 0, 16)
	i := 0
	for i < len(expr) {
		c := expr[i]
		if c == ' ' || c == '\t' {
			i++
			continue
		}
		isDigit := c >= '0' && c <= '9'
		isDotNum := c == '.' && i+1 < len(expr) && expr[i+1] >= '0' && expr[i+1] <= '9'
		isUnary := false
		if c == '-' {
			if len(tokens) == 0 {
				isUnary = true
			} else if last := tokens[len(tokens)-1]; last.kind == calcOp || last.kind == calcLParen {
				isUnary = true
			}
		}
		if isDigit || isDotNum || isUnary {
			start := i
			if c == '-' {
				i++
			}
			for i < len(expr) && ((expr[i] >= '0' && expr[i] <= '9') || expr[i] == '.') {
				i++
			}
			s := expr[start:i]
			d, ok := parseRustFloat(s)
			if !ok {
				return nil, "SYNX :calc — invalid number: '" + s + "'"
			}
			tokens = append(tokens, calcToken{kind: calcNumber, number: d})
			continue
		}
		switch c {
		case '+', '-', '*', '/', '%':
			tokens = append(tokens, calcToken{kind: calcOp, op: c})
			i++
			continue
		case '(':
			tokens = append(tokens, calcToken{kind: calcLParen})
			i++
			continue
		case ')':
			tokens = append(tokens, calcToken{kind: calcRParen})
			i++
			continue
		}
		// The whole character, not its first byte read as Latin-1.
		r, _ := utf8.DecodeRuneInString(expr[i:])
		return nil, "SYNX :calc — unexpected character: '" + string(r) + "' in expression"
	}
	return tokens, ""
}

// tokenText is a token as written, for error messages.
func tokenText(t calcToken) string {
	switch t.kind {
	case calcNumber:
		return rustFloatDisplay(t.number)
	case calcOp:
		return string(rune(t.op))
	case calcLParen:
		return "("
	}
	return ")"
}

// calcFrame is one open `expr` of the grammar — `expr → term (('+'|'-') term)*`,
// `term → factor (('*'|'/'|'%') factor)*`, `factor → NUMBER | '(' expr ')'` —
// the top level, or the inside of a pair of parentheses.
type calcFrame struct {
	// Sum so far and the `+` / `-` waiting for its right-hand term (0: none).
	exprLeft float64
	exprOp   byte
	// Product so far and the `*` / `/` / `%` waiting for its factor.
	termLeft float64
	termOp   byte
}

// calcEvaluate evaluates a token stream.
//
// safety: parentheses used to recurse through expr → term → factor, three
// calls per `(`. A goroutine stack grows to 1 GiB and then the runtime ends
// the process — no recover reaches it; `:ref:calc` passes an expression of
// any length. Open parentheses now live in a slice; the order of evaluation,
// and so every result and error text, is the one the recursive parser gave
// (and the one synx-core gives).
func calcEvaluate(t []calcToken) (float64, string) {
	stack := make([]calcFrame, 1, 8)
	pos := 0
	for {
		// A factor is expected here.
		if pos >= len(t) {
			return 0, "SYNX :calc — unexpected end of expression"
		}
		tok := t[pos]
		if tok.kind == calcLParen {
			pos++
			stack = append(stack, calcFrame{})
			continue
		}
		if tok.kind != calcNumber {
			return 0, "SYNX :calc — unexpected token: '" + tokenText(tok) + "'"
		}
		pos++
		value := tok.number

		// Fold the factor into its term, the term into its expr, and close
		// every pair of parentheses that ends here.
		for {
			f := &stack[len(stack)-1]
			termOp := f.termOp
			f.termOp = 0
			switch termOp {
			case 0:
				f.termLeft = value
			case '*':
				f.termLeft *= value
			default:
				if value == 0 {
					return 0, "SYNX :calc — division by zero"
				}
				if termOp == '/' {
					f.termLeft /= value
				} else {
					f.termLeft = math.Mod(f.termLeft, value)
				}
			}
			if pos < len(t) && t[pos].kind == calcOp && (t[pos].op == '*' || t[pos].op == '/' || t[pos].op == '%') {
				f.termOp = t[pos].op
				pos++
				break
			}

			exprOp := f.exprOp
			f.exprOp = 0
			switch exprOp {
			case 0:
				f.exprLeft = f.termLeft
			case '+':
				f.exprLeft += f.termLeft
			default:
				f.exprLeft -= f.termLeft
			}
			if pos < len(t) && t[pos].kind == calcOp && (t[pos].op == '+' || t[pos].op == '-') {
				f.exprOp = t[pos].op
				pos++
				break
			}

			// This expr is complete.
			result := f.exprLeft
			if len(stack) == 1 {
				if pos < len(t) {
					return 0, "SYNX :calc — unexpected token at position " + strconv.Itoa(pos)
				}
				return result, ""
			}
			stack = stack[:len(stack)-1]
			if pos >= len(t) || t[pos].kind != calcRParen {
				return 0, "SYNX :calc — missing closing parenthesis"
			}
			pos++
			value = result
		}
	}
}

// isFinite: neither an infinity nor NaN.
func isFinite(f float64) bool { return !math.IsInf(f, 0) && !math.IsNaN(f) }

// ─── Rust number text ───────────────────────────────────────────────────────

// rustFloatDisplay prints a float like Rust's f64 `Display`: shortest
// round-trip digits in plain decimal, never an exponent (`0.0000001`,
// `1000000000000000000000`), `-0`, `NaN`, `inf`.
func rustFloatDisplay(f float64) string {
	switch {
	case math.IsNaN(f):
		return "NaN"
	case math.IsInf(f, 1):
		return "inf"
	case math.IsInf(f, -1):
		return "-inf"
	}
	return strconv.FormatFloat(f, 'f', -1, 64)
}

// i64MaxF is `i64::MAX as f64` (2^63).
const i64MaxF = 9223372036854775807.0

// f64ToI64 is Rust's `f as i64`: truncation toward zero, saturating at the
// bounds, NaN → 0.
func f64ToI64(f float64) int64 {
	switch {
	case f != f:
		return 0
	case f >= i64MaxF:
		return math.MaxInt64
	case f <= -i64MaxF:
		return math.MinInt64
	}
	return int64(f)
}

// isWhole is Rust's `f.fract() == 0.0`.
func isWhole(f float64) bool {
	return !math.IsInf(f, 0) && f == math.Trunc(f)
}

// formatNumber is the engine's number text: a whole float within i64 prints
// as an integer, anything else as Rust `Display` would.
func formatNumber(n float64) string {
	if isWhole(n) && math.Abs(n) < i64MaxF {
		return strconv.FormatInt(f64ToI64(n), 10)
	}
	return rustFloatDisplay(n)
}

// numberValue turns a computed float into the value Rust stores: an Int when
// it is whole and within i64, a Float otherwise.
func numberValue(f float64) Value {
	if isWhole(f) && math.Abs(f) < i64MaxF {
		return Int(f64ToI64(f))
	}
	return Float(f)
}

// trimFloatText trims a decimal expansion's trailing zeros, keeping `x.0`.
func trimFloatText(s string) string {
	if !strings.Contains(s, ".") {
		return s
	}
	t := strings.TrimRight(s, "0")
	if strings.HasSuffix(t, ".") {
		return t + "0"
	}
	return t
}
