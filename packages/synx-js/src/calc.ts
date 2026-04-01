/**
 * SYNX Safe Calculator — @aperturesyndicate/synx-format
 *
 * Evaluates arithmetic expressions WITHOUT eval() or new Function().
 * Supports: +, -, *, /, %, parentheses, and numeric literals.
 * Variable references are resolved before this stage.
 */

type Token =
  | { type: 'number'; value: number }
  | { type: 'op'; value: string }
  | { type: 'paren'; value: '(' | ')' };

/**
 * Tokenize an arithmetic expression string.
 */
function tokenize(expr: string): Token[] {
  const tokens: Token[] = [];
  let i = 0;

  while (i < expr.length) {
    const ch = expr[i];

    // Skip whitespace
    if (ch === ' ' || ch === '\t') {
      i++;
      continue;
    }

    // Number (integer or float, including negative after operator/start)
    if (
      (ch >= '0' && ch <= '9') ||
      (ch === '.' && i + 1 < expr.length && expr[i + 1] >= '0' && expr[i + 1] <= '9') ||
      (ch === '-' && (tokens.length === 0 || tokens[tokens.length - 1].type === 'op' || (tokens[tokens.length - 1].type === 'paren' && tokens[tokens.length - 1].value === '(')))
    ) {
      let num = '';
      if (ch === '-') {
        num += '-';
        i++;
      }
      while (i < expr.length && ((expr[i] >= '0' && expr[i] <= '9') || expr[i] === '.')) {
        num += expr[i];
        i++;
      }
      tokens.push({ type: 'number', value: parseFloat(num) });
      continue;
    }

    // Operators
    if ('+-*/%'.includes(ch)) {
      tokens.push({ type: 'op', value: ch });
      i++;
      continue;
    }

    // Parentheses
    if (ch === '(' || ch === ')') {
      tokens.push({ type: 'paren', value: ch });
      i++;
      continue;
    }

    throw new Error(`SYNX :calc — unexpected character: '${ch}' in expression "${expr}"`);
  }

  return tokens;
}

/**
 * Recursive descent parser for arithmetic.
 * Grammar:
 *   expr       → term (('+' | '-') term)*
 *   term       → factor (('*' | '/' | '%') factor)*
 *   factor     → NUMBER | '(' expr ')'
 */
class ExprParser {
  private tokens: Token[];
  private pos: number;

  constructor(tokens: Token[]) {
    this.tokens = tokens;
    this.pos = 0;
  }

  parse(): number {
    const result = this.expr();
    if (this.pos < this.tokens.length) {
      throw new Error(`SYNX :calc — unexpected token at position ${this.pos}`);
    }
    return result;
  }

  private expr(): number {
    let left = this.term();
    while (this.pos < this.tokens.length && this.tokens[this.pos].type === 'op' && (this.tokens[this.pos].value === '+' || this.tokens[this.pos].value === '-')) {
      const op = this.tokens[this.pos].value;
      this.pos++;
      const right = this.term();
      left = op === '+' ? left + right : left - right;
    }
    return left;
  }

  private term(): number {
    let left = this.factor();
    while (this.pos < this.tokens.length && this.tokens[this.pos].type === 'op' && ('*/%'.includes(this.tokens[this.pos].value as string))) {
      const op = this.tokens[this.pos].value;
      this.pos++;
      const right = this.factor();
      if (op === '*') left = left * right;
      else if (op === '/') {
        if (right === 0) throw new Error('SYNX :calc — division by zero');
        left = left / right;
      }
      else {
        if (right === 0) throw new Error('SYNX :calc — division by zero');
        left = left % right;
      }
    }
    return left;
  }

  private factor(): number {
    const tok = this.tokens[this.pos];

    if (!tok) {
      throw new Error('SYNX :calc — unexpected end of expression');
    }

    if (tok.type === 'number') {
      this.pos++;
      return tok.value;
    }

    if (tok.type === 'paren' && tok.value === '(') {
      this.pos++; // skip '('
      const val = this.expr();
      if (!this.tokens[this.pos] || this.tokens[this.pos].type !== 'paren' || this.tokens[this.pos].value !== ')') {
        throw new Error('SYNX :calc — missing closing parenthesis');
      }
      this.pos++; // skip ')'
      return val;
    }

    throw new Error(`SYNX :calc — unexpected token: ${JSON.stringify(tok)}`);
  }
}

/**
 * Safely evaluate an arithmetic expression.
 * All variable names must be substituted with numbers before calling this.
 *
 * @param expr - e.g. "100 * 5 + (20 / 4)"
 * @returns The computed number
 */
export function safeCalc(expr: string): number {
  const tokens = tokenize(expr.trim());
  if (tokens.length === 0) return 0;
  return new ExprParser(tokens).parse();
}
