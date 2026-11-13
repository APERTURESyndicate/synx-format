/**
 * SYNX Safe Calculator — @aperturesyndicate/synx-format
 *
 * Evaluates arithmetic expressions WITHOUT eval() or new Function().
 * Supports: +, -, *, /, %, parentheses, and numeric literals.
 * Variable references are resolved before this stage.
 *
 * A port of `calc.rs` in synx-core: the same tokens, the same order of
 * evaluation, the same error for the same expression.
 */

import { trimSpace } from './parser';

/** Token kinds: a number, or the character code of an operator or parenthesis. */
const NUMBER = 0;
const PLUS = 43;
const MINUS = 45;
const STAR = 42;
const SLASH = 47;
const PERCENT = 37;
const LPAREN = 40;
const RPAREN = 41;
/** No operator is waiting. */
const NONE = -1;

/**
 * Tokenize an arithmetic expression into `kinds` and, for numbers, `values`.
 */
function tokenize(expr: string, kinds: number[], values: number[]): void {
  const len = expr.length;
  let i = 0;

  while (i < len) {
    const ch = expr.charCodeAt(i);

    // Skip whitespace
    if (ch === 32 || ch === 9) {
      i++;
      continue;
    }

    // Number (integer or float, including negative after operator/start/lparen)
    let negative = false;
    if (ch === MINUS) {
      const last = kinds.length === 0 ? PLUS : kinds[kinds.length - 1];
      negative = last !== NUMBER && last !== RPAREN;
    }
    if (
      (ch >= 48 && ch <= 57) ||
      (ch === 46 && i + 1 < len && expr.charCodeAt(i + 1) >= 48 && expr.charCodeAt(i + 1) <= 57) ||
      negative
    ) {
      const start = i;
      if (ch === MINUS) i++;
      let digits = 0;
      let dots = 0;
      while (i < len) {
        const c = expr.charCodeAt(i);
        if (c >= 48 && c <= 57) digits++;
        else if (c === 46) dots++;
        else break;
        i++;
      }
      const text = expr.substring(start, i);
      // spec: a malformed literal is an error, as in synx-core — `parseFloat`
      // read a lone `-` as NaN and `1.2.3` as 1.2 without a word.
      // safety: counted, not matched. The pattern this replaces
      // (`\d+\.?\d*`) split a run of digits in two at every position, so a
      // long run ahead of a second dot cost the square of its length.
      if (digits === 0 || dots > 1) throw new Error(`SYNX :calc — invalid number: '${text}'`);
      kinds.push(NUMBER);
      values.push(parseFloat(text));
      continue;
    }

    // Operators and parentheses
    if (ch === PLUS || ch === MINUS || ch === STAR || ch === SLASH || ch === PERCENT || ch === LPAREN || ch === RPAREN) {
      kinds.push(ch);
      values.push(0);
      i++;
      continue;
    }

    throw new Error(`SYNX :calc — unexpected character: '${String.fromCodePoint(expr.codePointAt(i)!)}' in expression`);
  }
}

/**
 * Evaluate a token stream.
 *
 * Grammar:
 *   expr   → term (('+' | '-') term)*
 *   term   → factor (('*' | '/' | '%') factor)*
 *   factor → NUMBER | '(' expr ')'
 *
 * safety: parentheses used to recurse through `expr → term → factor`, so
 * `x:calc ((((…` ended in "Maximum call stack size exceeded" — an error of the
 * runtime, at a depth that differs between Node and Bun, where synx-core
 * computes the value. Open parentheses now live in arrays: one frame per `(`
 * holds the sum so far with the `+` / `-` waiting for its term, and the
 * product so far with the `*` / `/` / `%` waiting for its factor.
 */
function evaluate(kinds: number[], values: number[]): number {
  const count = kinds.length;
  const exprLeft: number[] = [0];
  const exprOp: number[] = [NONE];
  const termLeft: number[] = [0];
  const termOp: number[] = [NONE];
  let top = 0;
  let pos = 0;

  for (;;) {
    // A factor is expected here.
    if (pos >= count) throw new Error('SYNX :calc — unexpected end of expression');
    const kind = kinds[pos];
    if (kind === LPAREN) {
      pos++;
      top++;
      exprLeft[top] = 0;
      exprOp[top] = NONE;
      termLeft[top] = 0;
      termOp[top] = NONE;
      continue;
    }
    if (kind !== NUMBER) {
      throw new Error(`SYNX :calc — unexpected token: '${String.fromCharCode(kind)}'`);
    }
    let value = values[pos];
    pos++;

    // Fold the factor into its term, the term into its expr, and close every
    // pair of parentheses that ends here.
    for (;;) {
      const op = termOp[top];
      termOp[top] = NONE;
      if (op === NONE) termLeft[top] = value;
      else if (op === STAR) termLeft[top] *= value;
      else {
        if (value === 0) throw new Error('SYNX :calc — division by zero');
        if (op === SLASH) termLeft[top] /= value;
        else termLeft[top] %= value;
      }
      const next = pos < count ? kinds[pos] : NONE;
      if (next === STAR || next === SLASH || next === PERCENT) {
        termOp[top] = next;
        pos++;
        break;
      }

      const sign = exprOp[top];
      exprOp[top] = NONE;
      if (sign === NONE) exprLeft[top] = termLeft[top];
      else if (sign === PLUS) exprLeft[top] += termLeft[top];
      else exprLeft[top] -= termLeft[top];
      if (next === PLUS || next === MINUS) {
        exprOp[top] = next;
        pos++;
        break;
      }

      // This expr is complete.
      const result = exprLeft[top];
      if (top === 0) {
        if (pos < count) throw new Error(`SYNX :calc — unexpected token at position ${pos}`);
        return result;
      }
      top--;
      if (next !== RPAREN) throw new Error('SYNX :calc — missing closing parenthesis');
      pos++;
      value = result;
    }
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
  const kinds: number[] = [];
  const values: number[] = [];
  tokenize(trimSpace(expr), kinds, values);
  if (kinds.length === 0) return 0;
  return evaluate(kinds, values);
}
