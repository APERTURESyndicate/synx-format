// Constraint and type enforcement for the `!active` engine.
//
// Failures are written into the value (`CONSTRAINT_ERR: ...` / `TYPE_ERR: ...`)
// exactly as in crates/synx-core/src/engine.rs.
import 'meta.dart';
import 'num.dart';
import 'pattern.dart';
import 'value.dart';

const int _maxPatternBytes = 256;

/// The error a value earns against its constraints, or null when it passes.
String? constraintViolation(String key, SynxValue val, SynxConstraints c) {
  // A key written without a value parses as an empty group; for `[required]`
  // that is the missing value, not a present object.
  if (c.required &&
      (val is SynxNull || (val is SynxStr && val.value.isEmpty) || (val is SynxObj && val.map.isEmpty))) {
    return "CONSTRAINT_ERR: '$key' is required";
  }

  // spec: an absent value (null) is checked against `enum` only, read as an
  // empty string; type, range and pattern apply to values that exist.
  if (val is SynxNull) {
    final ev = c.enumValues;
    if (ev != null && !ev.contains('')) {
      return "CONSTRAINT_ERR: '$key' must be one of [${ev.join('|')}]";
    }
    return null;
  }

  final typeName = c.typeName;
  if (typeName != null) {
    final ok = switch (typeName) {
      'int' => val is SynxInt,
      'float' => val is SynxFloat || val is SynxInt,
      'bool' => val is SynxBool,
      // A `:secret` is a masked string: `[type:string]:secret:env` must accept it.
      'string' => val is SynxStr || val is SynxSecret,
      _ => true,
    };
    if (!ok) return "CONSTRAINT_ERR: '$key' expected type '$typeName'";
  }

  final ev = c.enumValues;
  if (ev != null) {
    final text = switch (val) {
      // A secret is checked by its text; no message quotes the value.
      SynxStr(value: var s) => s,
      SynxSecret(value: var s) => s,
      SynxInt(value: var n) => n.toString(),
      SynxFloat(value: var f) => rustDisplay(f),
      SynxBool(value: var b) => b ? 'true' : 'false',
      _ => '',
    };
    if (!ev.contains(text)) return "CONSTRAINT_ERR: '$key' must be one of [${ev.join('|')}]";
  }

  // Numbers compare by value; a string's size is its count of Unicode scalar values.
  final num = switch (val) {
    SynxInt(value: var n) => n.toDouble(),
    SynxFloat(value: var f) => f,
    SynxStr(value: var s) when c.min != null || c.max != null => s.runes.length.toDouble(),
    SynxSecret(value: var s) when c.min != null || c.max != null => s.runes.length.toDouble(),
    _ => null,
  };
  if (num != null) {
    final min = c.min;
    if (min != null && num < min) {
      return "CONSTRAINT_ERR: '$key' value ${rustDisplay(num)} is below min ${rustDisplay(min)}";
    }
    final max = c.max;
    if (max != null && num > max) {
      return "CONSTRAINT_ERR: '$key' value ${rustDisplay(num)} exceeds max ${rustDisplay(max)}";
    }
  }

  // Over-long, invalid and nested-quantifier patterns are skipped silently.
  final pat = c.pattern;
  final text = switch (val) {
    SynxStr(value: var s) => s,
    SynxSecret(value: var s) => s,
    _ => null,
  };
  if (pat != null && text != null && utf8Len(pat) <= _maxPatternBytes && patternRejects(pat, text)) {
    return "CONSTRAINT_ERR: '$key' does not match pattern /$pat/";
  }
  return null;
}

final Map<String, SynxPattern?> _patterns = {};

/// Whether [value] fails `[pattern:…]` (unanchored, like Rust `is_match`).
/// A pattern the `regex` crate would not compile and a backtracking-prone one
/// never reject, as in synx-core.
/// perf: compiled patterns are cached, at most 256.
bool patternRejects(String pattern, String value) {
  SynxPattern? re;
  if (_patterns.containsKey(pattern)) {
    re = _patterns[pattern];
  } else {
    if (_patterns.length >= 256) _patterns.clear();
    re = hasNestedQuantifier(pattern) ? null : SynxPattern.compile(pattern);
    _patterns[pattern] = re;
  }
  return re != null && !re.hasMatch(value);
}

/// The same field declared twice: the stricter bounds win, the first
/// type/pattern/enum stays (Rust `merge_constraints`).
void mergeConstraints(SynxConstraints base, SynxConstraints incoming) {
  if (incoming.required) base.required = true;
  if (incoming.readonly) base.readonly = true;
  final imn = incoming.min;
  if (imn != null) base.min = base.min == null ? imn : _rustMax(base.min!, imn);
  final imx = incoming.max;
  if (imx != null) base.max = base.max == null ? imx : _rustMin(base.max!, imx);
  base.typeName ??= incoming.typeName;
  base.pattern ??= incoming.pattern;
  base.enumValues ??= incoming.enumValues?.toList();
}

/// Rust `f64::max` / `f64::min`: a NaN operand yields the other.
double _rustMax(double a, double b) => a.isNaN ? b : b.isNaN ? a : (a > b ? a : b);
double _rustMin(double a, double b) => a.isNaN ? b : b.isNaN ? a : (a < b ? a : b);

SynxConstraints cloneConstraints(SynxConstraints c) => SynxConstraints()
  ..min = c.min
  ..max = c.max
  ..typeName = c.typeName
  ..required = c.required
  ..readonly = c.readonly
  ..pattern = c.pattern
  ..enumValues = c.enumValues?.toList();

bool matchesType(SynxValue v, String want) => switch (want) {
      'int' => v is SynxInt,
      'float' => v is SynxFloat || v is SynxInt,
      'bool' => v is SynxBool,
      'string' => v is SynxStr || v is SynxSecret,
      'array' => v is SynxArr,
      'object' => v is SynxObj,
      _ => true,
    };

/// Prints like Rust's f64 `Display`: shortest round-trip digits, never an
/// exponent (`5`, `0.25`, `70000`).
String formatRustNumber(double f) => rustDisplay(f);

/// Does the pattern repeat something that already repeats — the `(a+)+` shape
/// behind exponential backtracking? Dart's RegExp backtracks, so such a pattern
/// is refused (skipped) instead of run, as in the reference.
/// Mirrors `has_nested_quantifier` in engine.rs.
bool hasNestedQuantifier(String pattern) {
  final chars = pattern.runes.toList();
  const backslash = 0x5C;
  final opens = <int>[];
  for (var i = 0; i < chars.length; i++) {
    final ch = chars[i];
    if (ch == backslash) {
      i++;
    } else if (ch == 0x5B) {
      // A character class is skipped wholesale; quantifiers in it are literal.
      while (i < chars.length && chars[i] != 0x5D) {
        if (chars[i] == backslash) i++;
        i++;
      }
    } else if (ch == 0x28) {
      opens.add(i);
    } else if (ch == 0x29) {
      if (opens.isEmpty) continue;
      final start = opens.removeLast();
      if (i + 1 < chars.length) {
        final next = chars[i + 1];
        if (next == 0x2A || next == 0x2B || next == 0x7B) {
          final body = <int>[];
          var j = start + 1;
          while (j < i) {
            if (chars[j] == backslash) {
              j += 2;
              continue;
            }
            body.add(chars[j]);
            j++;
          }
          for (var k = 0; k < body.length; k++) {
            final b = body[k];
            if (b == 0x2A ||
                b == 0x2B ||
                (b == 0x7B && k + 1 < body.length && body[k + 1] >= 0x30 && body[k + 1] <= 0x39)) {
              return true;
            }
          }
        }
      }
    }
  }
  return false;
}
