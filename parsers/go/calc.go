package synx

import (
	"fmt"
	"math"
	"strconv"
	"strings"
)

// CalcResult is the outcome of evaluating a `:calc` expression.
type CalcResult struct {
	OK    bool
	Value float64
	Error string
}

// SafeCalc evaluates an arithmetic expression with no variable side-effects.
// Supports +, -, *, /, %, parentheses, integers, floats, unary minus.
// Variable references must be substituted with literals before calling.
func SafeCalc(expr string) CalcResult {
	t := strings.TrimSpace(expr)
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
	p := &calcParser{tokens: tokens}
	v, err := p.expr()
	if err != "" {
		return CalcResult{Error: err}
	}
	if p.pos < len(p.tokens) {
		return CalcResult{Error: fmt.Sprintf("SYNX :calc - unexpected token at position %d", p.pos)}
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
			} else {
				last := tokens[len(tokens)-1]
				if last.kind == calcOp || last.kind == calcLParen {
					isUnary = true
				}
			}
		}
		if isDigit || isDotNum || isUnary {
			start := i
			if c == '-' {
				i++
			}
			for i < len(expr) {
				x := expr[i]
				if (x >= '0' && x <= '9') || x == '.' {
					i++
				} else {
					break
				}
			}
			s := expr[start:i]
			d, err := strconv.ParseFloat(s, 64)
			if err != nil {
				return nil, fmt.Sprintf("SYNX :calc - invalid number: '%s'", s)
			}
			tokens = append(tokens, calcToken{kind: calcNumber, number: d})
			continue
		}
		if c == '+' || c == '-' || c == '*' || c == '/' || c == '%' {
			tokens = append(tokens, calcToken{kind: calcOp, op: c})
			i++
			continue
		}
		if c == '(' {
			tokens = append(tokens, calcToken{kind: calcLParen})
			i++
			continue
		}
		if c == ')' {
			tokens = append(tokens, calcToken{kind: calcRParen})
			i++
			continue
		}
		return nil, fmt.Sprintf("SYNX :calc - unexpected character: '%c'", c)
	}
	return tokens, ""
}

type calcParser struct {
	tokens []calcToken
	pos    int
}

func (p *calcParser) expr() (float64, string) {
	left, err := p.term()
	if err != "" {
		return 0, err
	}
	for p.pos < len(p.tokens) {
		t := p.tokens[p.pos]
		if t.kind == calcOp && t.op == '+' {
			p.pos++
			r, err := p.term()
			if err != "" {
				return 0, err
			}
			left += r
		} else if t.kind == calcOp && t.op == '-' {
			p.pos++
			r, err := p.term()
			if err != "" {
				return 0, err
			}
			left -= r
		} else {
			break
		}
	}
	return left, ""
}

func (p *calcParser) term() (float64, string) {
	left, err := p.factor()
	if err != "" {
		return 0, err
	}
	for p.pos < len(p.tokens) {
		t := p.tokens[p.pos]
		if t.kind != calcOp {
			break
		}
		switch t.op {
		case '*':
			p.pos++
			r, err := p.factor()
			if err != "" {
				return 0, err
			}
			left *= r
		case '/':
			p.pos++
			r, err := p.factor()
			if err != "" {
				return 0, err
			}
			if r == 0 {
				return 0, "SYNX :calc - division by zero"
			}
			left /= r
		case '%':
			p.pos++
			r, err := p.factor()
			if err != "" {
				return 0, err
			}
			if r == 0 {
				return 0, "SYNX :calc - division by zero"
			}
			left = math.Mod(left, r)
		default:
			return left, ""
		}
	}
	return left, ""
}

func (p *calcParser) factor() (float64, string) {
	if p.pos >= len(p.tokens) {
		return 0, "SYNX :calc - unexpected end of expression"
	}
	t := p.tokens[p.pos]
	switch t.kind {
	case calcNumber:
		p.pos++
		return t.number, ""
	case calcLParen:
		p.pos++
		v, err := p.expr()
		if err != "" {
			return 0, err
		}
		if p.pos >= len(p.tokens) || p.tokens[p.pos].kind != calcRParen {
			return 0, "SYNX :calc - missing closing parenthesis"
		}
		p.pos++
		return v, ""
	}
	return 0, "SYNX :calc - unexpected token"
}
