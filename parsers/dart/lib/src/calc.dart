// Safe arithmetic evaluator for `:calc`. Mirrors crates/synx-core/src/calc.rs:
// the same grammar and the same error texts.
import 'num.dart';

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

class _CalcError implements Exception {
  final String message;
  const _CalcError(this.message);
}

SynxCalcResult safeCalc(String expr) {
  final t = trimWs(expr);
  if (t.isEmpty) return const SynxCalcResult.success(0);
  try {
    final tokens = _tokenize(t);
    if (tokens.isEmpty) return const SynxCalcResult.success(0);
    return SynxCalcResult.success(_evaluate(tokens));
  } on _CalcError catch (e) {
    return SynxCalcResult.failure(e.message);
  }
}

enum _Kind { number, op, lParen, rParen }

class _Tok {
  final _Kind kind;
  final double number;
  final String op;
  const _Tok(this.kind, [this.number = 0, this.op = '']);
}

bool _isDigit(int c) => c >= 0x30 && c <= 0x39;

List<_Tok> _tokenize(String expr) {
  final tokens = <_Tok>[];
  var i = 0;
  while (i < expr.length) {
    final c = expr.codeUnitAt(i);
    if (c == 0x20 || c == 0x09) {
      i++;
      continue;
    }
    // A number, including a negative one after an operator, `(` or the start.
    final unary = c == 0x2D && (tokens.isEmpty || tokens.last.kind == _Kind.op || tokens.last.kind == _Kind.lParen);
    if (_isDigit(c) || (c == 0x2E && i + 1 < expr.length && _isDigit(expr.codeUnitAt(i + 1))) || unary) {
      final start = i;
      if (c == 0x2D) i++;
      while (i < expr.length && (_isDigit(expr.codeUnitAt(i)) || expr.codeUnitAt(i) == 0x2E)) {
        i++;
      }
      final s = expr.substring(start, i);
      final d = parseF64(s);
      if (d == null) throw _CalcError("SYNX :calc — invalid number: '$s'");
      tokens.add(_Tok(_Kind.number, d));
      continue;
    }
    if (c == 0x2B || c == 0x2D || c == 0x2A || c == 0x2F || c == 0x25) {
      tokens.add(_Tok(_Kind.op, 0, String.fromCharCode(c)));
      i++;
      continue;
    }
    if (c == 0x28) {
      tokens.add(const _Tok(_Kind.lParen));
      i++;
      continue;
    }
    if (c == 0x29) {
      tokens.add(const _Tok(_Kind.rParen));
      i++;
      continue;
    }
    // The whole character, a surrogate pair included.
    final shown = c >= 0xD800 && c <= 0xDBFF && i + 1 < expr.length ? expr.substring(i, i + 2) : expr[i];
    throw _CalcError("SYNX :calc — unexpected character: '$shown' in expression");
  }
  return tokens;
}

/// A token as written, for error messages (identical in every implementation).
String _tokenText(_Tok t) => switch (t.kind) {
      _Kind.number => rustDisplay(t.number),
      _Kind.op => t.op,
      _Kind.lParen => '(',
      _Kind.rParen => ')',
    };

/// `expr → term (('+'|'-') term)*`, `term → factor (('*'|'/'|'%') factor)*`,
/// `factor → NUMBER | '(' expr ')'`, one frame per open parenthesis.
///
/// safety: parentheses used to recurse through expr → term → factor; 20 000 of
/// them through `:ref:calc` overflowed the stack. Open parentheses now live in
/// a list; the order of evaluation, and so every result and error, is the one
/// the recursive parser produced (synx-core `calc.rs evaluate`).
class _Frame {
  double exprLeft = 0;
  String? exprOp;
  double termLeft = 0;
  String? termOp;
}

double _evaluate(List<_Tok> tokens) {
  final stack = <_Frame>[_Frame()];
  var pos = 0;
  while (true) {
    // A factor is expected here.
    if (pos >= tokens.length) throw const _CalcError('SYNX :calc — unexpected end of expression');
    final t = tokens[pos];
    double value;
    if (t.kind == _Kind.number) {
      pos++;
      value = t.number;
    } else if (t.kind == _Kind.lParen) {
      pos++;
      stack.add(_Frame());
      continue;
    } else {
      throw _CalcError("SYNX :calc — unexpected token: '${_tokenText(t)}'");
    }

    // Fold the factor into its term, the term into its expr, and close every
    // pair of parentheses that ends here.
    while (true) {
      final frame = stack.last;
      final top = frame.termOp;
      frame.termOp = null;
      if (top == null) {
        frame.termLeft = value;
      } else if (top == '*') {
        frame.termLeft *= value;
      } else {
        if (value == 0) throw const _CalcError('SYNX :calc — division by zero');
        // Truncated remainder, as Rust's `%` (Dart's `%` is Euclidean).
        frame.termLeft = top == '/' ? frame.termLeft / value : frame.termLeft.remainder(value);
      }
      if (pos < tokens.length && tokens[pos].kind == _Kind.op && '*/%'.contains(tokens[pos].op)) {
        frame.termOp = tokens[pos].op;
        pos++;
        break;
      }
      final eop = frame.exprOp;
      frame.exprOp = null;
      if (eop == null) {
        frame.exprLeft = frame.termLeft;
      } else if (eop == '+') {
        frame.exprLeft += frame.termLeft;
      } else {
        frame.exprLeft -= frame.termLeft;
      }
      if (pos < tokens.length && tokens[pos].kind == _Kind.op && (tokens[pos].op == '+' || tokens[pos].op == '-')) {
        frame.exprOp = tokens[pos].op;
        pos++;
        break;
      }
      // This expr is complete.
      final result = frame.exprLeft;
      if (stack.length == 1) {
        if (pos < tokens.length) throw _CalcError('SYNX :calc — unexpected token at position $pos');
        return result;
      }
      stack.removeLast();
      if (pos < tokens.length && tokens[pos].kind == _Kind.rParen) {
        pos++;
      } else {
        throw const _CalcError('SYNX :calc — missing closing parenthesis');
      }
      value = result;
    }
  }
}
