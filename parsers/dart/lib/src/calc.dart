// Safe arithmetic evaluator for `:calc`. Mirrors crates/synx-core/src/calc.rs.

class SynxCalcResult {
  final bool ok;
  final double value;
  final String error;
  const SynxCalcResult.success(this.value)
      : ok = true,
        error = '';
  const SynxCalcResult.failure(this.error)
      : ok = false,
        value = 0;
}

SynxCalcResult safeCalc(String expr) {
  final t = expr.trim();
  if (t.isEmpty) return const SynxCalcResult.success(0);
  final tokens = <_Tok>[];
  final err = _tokenize(t, tokens);
  if (err != null) return SynxCalcResult.failure(err);
  if (tokens.isEmpty) return const SynxCalcResult.success(0);
  final p = _ExprParser(tokens);
  final out = p.expr();
  if (out.error != '') return SynxCalcResult.failure(out.error);
  if (p.pos < tokens.length) {
    return SynxCalcResult.failure(
        'SYNX :calc - unexpected token at position ${p.pos}');
  }
  return SynxCalcResult.success(out.value);
}

enum _Kind { number, op, lParen, rParen }

class _Tok {
  final _Kind kind;
  final double number;
  final String op;
  const _Tok.num(this.number)
      : kind = _Kind.number,
        op = '';
  const _Tok.opc(this.op)
      : kind = _Kind.op,
        number = 0;
  const _Tok.lp()
      : kind = _Kind.lParen,
        number = 0,
        op = '';
  const _Tok.rp()
      : kind = _Kind.rParen,
        number = 0,
        op = '';
}

String? _tokenize(String expr, List<_Tok> tokens) {
  var i = 0;
  while (i < expr.length) {
    final c = expr[i];
    if (c == ' ' || c == '\t') {
      i++;
      continue;
    }
    final cc = expr.codeUnitAt(i);
    final isDigit = cc >= 0x30 && cc <= 0x39;
    final isDotNum = c == '.' &&
        i + 1 < expr.length &&
        expr.codeUnitAt(i + 1) >= 0x30 &&
        expr.codeUnitAt(i + 1) <= 0x39;
    var isUnary = false;
    if (c == '-') {
      if (tokens.isEmpty) {
        isUnary = true;
      } else {
        final last = tokens.last;
        if (last.kind == _Kind.op || last.kind == _Kind.lParen) isUnary = true;
      }
    }
    if (isDigit || isDotNum || isUnary) {
      final start = i;
      if (c == '-') i++;
      while (i < expr.length) {
        final x = expr.codeUnitAt(i);
        if ((x >= 0x30 && x <= 0x39) || x == 0x2E) {
          i++;
        } else {
          break;
        }
      }
      final s = expr.substring(start, i);
      final d = double.tryParse(s);
      if (d == null) {
        return "SYNX :calc - invalid number: '$s'";
      }
      tokens.add(_Tok.num(d));
      continue;
    }
    if (c == '+' || c == '-' || c == '*' || c == '/' || c == '%') {
      tokens.add(_Tok.opc(c));
      i++;
      continue;
    }
    if (c == '(') {
      tokens.add(const _Tok.lp());
      i++;
      continue;
    }
    if (c == ')') {
      tokens.add(const _Tok.rp());
      i++;
      continue;
    }
    return "SYNX :calc - unexpected character: '$c'";
  }
  return null;
}

class _ParseStep {
  final double value;
  final String error;
  const _ParseStep(this.value, [this.error = '']);
}

class _ExprParser {
  final List<_Tok> tokens;
  int pos = 0;
  _ExprParser(this.tokens);

  _ParseStep expr() {
    var left = term();
    if (left.error != '') return left;
    var leftV = left.value;
    while (pos < tokens.length) {
      final t = tokens[pos];
      if (t.kind == _Kind.op && t.op == '+') {
        pos++;
        final r = term();
        if (r.error != '') return r;
        leftV += r.value;
      } else if (t.kind == _Kind.op && t.op == '-') {
        pos++;
        final r = term();
        if (r.error != '') return r;
        leftV -= r.value;
      } else {
        break;
      }
    }
    return _ParseStep(leftV);
  }

  _ParseStep term() {
    var left = factor();
    if (left.error != '') return left;
    var leftV = left.value;
    while (pos < tokens.length) {
      final t = tokens[pos];
      if (t.kind != _Kind.op) break;
      switch (t.op) {
        case '*':
          pos++;
          final r = factor();
          if (r.error != '') return r;
          leftV *= r.value;
          break;
        case '/':
          pos++;
          final r = factor();
          if (r.error != '') return r;
          if (r.value == 0) {
            return const _ParseStep(0, 'SYNX :calc - division by zero');
          }
          leftV /= r.value;
          break;
        case '%':
          pos++;
          final r = factor();
          if (r.error != '') return r;
          if (r.value == 0) {
            return const _ParseStep(0, 'SYNX :calc - division by zero');
          }
          leftV = leftV % r.value;
          break;
        default:
          return _ParseStep(leftV);
      }
    }
    return _ParseStep(leftV);
  }

  _ParseStep factor() {
    if (pos >= tokens.length) {
      return const _ParseStep(0, 'SYNX :calc - unexpected end of expression');
    }
    final t = tokens[pos];
    if (t.kind == _Kind.number) {
      pos++;
      return _ParseStep(t.number);
    }
    if (t.kind == _Kind.lParen) {
      pos++;
      final v = expr();
      if (v.error != '') return v;
      if (pos >= tokens.length || tokens[pos].kind != _Kind.rParen) {
        return const _ParseStep(0, 'SYNX :calc - missing closing parenthesis');
      }
      pos++;
      return _ParseStep(v.value);
    }
    return const _ParseStep(0, 'SYNX :calc - unexpected token');
  }
}
