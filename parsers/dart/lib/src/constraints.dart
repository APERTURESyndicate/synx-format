// Constraint and type enforcement for the `!active` engine.
//
// Failures are written into the value (`CONSTRAINT_ERR: ...` / `TYPE_ERR: ...`)
// exactly as in crates/synx-core/src/engine.rs; with `strict` the message is
// also written to stderr.
import 'dart:convert';
import 'dart:io';

import 'meta.dart';
import 'value.dart';

const int _maxPatternBytes = 256;
const String _constraintErr = 'CONSTRAINT_ERR:';
const String _typeErr = 'TYPE_ERR:';

class ConstraintChecker {
  final bool strict;
  ConstraintChecker({required this.strict});

  void _fail(SynxObject obj, String key, String msg) {
    obj.set(key, synxString(msg));
    if (strict) stderr.writeln('synx: $msg');
  }

  /// Checks one key against [c]; the first failing check wins.
  void validate(SynxObject obj, String key, SynxConstraints c) {
    final val = obj[key];
    if (val == null) {
      if (c.required) _fail(obj, key, "CONSTRAINT_ERR: '$key' is required");
      return;
    }

    if (c.required) {
      final empty = val is SynxNull || (val is SynxStr && val.value.isEmpty);
      if (empty) {
        _fail(obj, key, "CONSTRAINT_ERR: '$key' is required");
        return;
      }
    }

    final typeName = c.typeName;
    if (typeName != null) {
      final ok = switch (typeName) {
        'int' => val is SynxInt,
        'float' => val is SynxFloat || val is SynxInt,
        'bool' => val is SynxBool,
        'string' => val is SynxStr || val is SynxSecret,
        _ => true,
      };
      if (!ok) {
        _fail(obj, key, "CONSTRAINT_ERR: '$key' expected type '$typeName'");
        return;
      }
    }

    final enumValues = c.enumValues;
    if (enumValues != null) {
      final text = switch (val) {
        SynxStr(value: var s) => s,
        SynxInt(value: var n) => n.toString(),
        SynxFloat(value: var f) => formatRustNumber(f),
        SynxBool(value: var b) => b ? 'true' : 'false',
        _ => '',
      };
      if (!enumValues.contains(text)) {
        _fail(obj, key,
            "CONSTRAINT_ERR: '$key' must be one of [${enumValues.join('|')}]");
        return;
      }
    }

    // Numbers compare by value; a string's size is its count of Unicode scalar values.
    final num = switch (val) {
      SynxInt(value: var n) => n.toDouble(),
      SynxFloat(value: var f) => f,
      SynxStr(value: var s) when c.min != null || c.max != null =>
        s.runes.length.toDouble(),
      _ => null,
    };
    if (num != null) {
      final min = c.min;
      if (min != null && num < min) {
        _fail(obj, key,
            "CONSTRAINT_ERR: '$key' value ${formatRustNumber(num)} is below min ${formatRustNumber(min)}");
        return;
      }
      final max = c.max;
      if (max != null && num > max) {
        _fail(obj, key,
            "CONSTRAINT_ERR: '$key' value ${formatRustNumber(num)} exceeds max ${formatRustNumber(max)}");
        return;
      }
    }

    // Over-long, invalid and nested-quantifier patterns are skipped silently.
    final pat = c.pattern;
    if (pat != null && val is SynxStr && utf8.encode(pat).length <= _maxPatternBytes) {
      if (!hasNestedQuantifier(pat)) {
        RegExp re;
        try {
          re = RegExp(pat);
        } on FormatException {
          return;
        }
        if (!re.hasMatch(val.value)) {
          _fail(obj, key,
              "CONSTRAINT_ERR: '$key' does not match pattern /$pat/");
        }
      }
    }
  }

  /// Applies the document-wide registries after the whole tree is resolved.
  void validateDocument(SynxObject root, Map<String, Map<String, SynxMeta>> metadata) {
    _fieldConstraints(root, _constraintRegistry(metadata), 0);
    _fieldTypes(root, _typeRegistry(metadata), 0);
  }

  void _fieldConstraints(
      SynxObject obj, Map<String, SynxConstraints> reg, int depth) {
    if (depth > 512) return;
    for (final key in obj.keys) {
      final c = reg[key];
      if (c != null && !_isFieldError(obj[key])) validate(obj, key, c);
      final child = obj[key];
      if (child is SynxObj) {
        _fieldConstraints(child.map, reg, depth + 1);
      } else if (child is SynxArr) {
        for (final item in child.values) {
          if (item is SynxObj) _fieldConstraints(item.map, reg, depth + 1);
        }
      }
    }
  }

  void _fieldTypes(SynxObject obj, Map<String, String> reg, int depth) {
    if (depth > 512) return;
    for (final key in obj.keys) {
      final want = reg[key];
      final val = obj[key] ?? synxNull();
      if (want != null && !_matchesType(val, want)) {
        _fail(obj, key,
            "TYPE_ERR: '$key' expected $want but got ${val.typeName}");
      }
      final child = obj[key];
      if (child is SynxObj) {
        _fieldTypes(child.map, reg, depth + 1);
      } else if (child is SynxArr) {
        for (final item in child.values) {
          if (item is SynxObj) _fieldTypes(item.map, reg, depth + 1);
        }
      }
    }
  }
}

bool _isFieldError(SynxValue? v) =>
    v is SynxStr &&
    (v.value.startsWith(_constraintErr) || v.value.startsWith(_typeErr));

bool _matchesType(SynxValue v, String want) => switch (want) {
      'int' => v is SynxInt,
      'float' => v is SynxFloat || v is SynxInt,
      'bool' => v is SynxBool,
      'string' => v is SynxStr || v is SynxSecret,
      'array' => v is SynxArr,
      'object' => v is SynxObj,
      _ => true,
    };

/// A field NAME maps to its declarations merged across the whole document:
/// required/readonly OR, min is the largest, max the smallest, the first
/// type/pattern/enum wins.
Map<String, SynxConstraints> _constraintRegistry(
    Map<String, Map<String, SynxMeta>> metadata) {
  final reg = <String, SynxConstraints>{};
  for (final fields in metadata.values) {
    fields.forEach((name, meta) {
      final c = meta.constraints;
      if (c == null) return;
      final base = reg[name];
      if (base == null) {
        reg[name] = SynxConstraints()
          ..min = c.min
          ..max = c.max
          ..typeName = c.typeName
          ..required = c.required
          ..readonly = c.readonly
          ..pattern = c.pattern
          ..enumValues = c.enumValues;
        return;
      }
      base.required = base.required || c.required;
      base.readonly = base.readonly || c.readonly;
      if (c.min != null && (base.min == null || c.min! > base.min!)) {
        base.min = c.min;
      }
      if (c.max != null && (base.max == null || c.max! < base.max!)) {
        base.max = c.max;
      }
      base.typeName ??= c.typeName;
      base.pattern ??= c.pattern;
      base.enumValues ??= c.enumValues;
    });
  }
  return reg;
}

Map<String, String> _typeRegistry(Map<String, Map<String, SynxMeta>> metadata) {
  final reg = <String, String>{};
  for (final fields in metadata.values) {
    fields.forEach((name, meta) {
      final hint = meta.typeHint;
      if (hint != null) reg.putIfAbsent(name, () => hint);
    });
  }
  return reg;
}

/// Prints like Rust's f64 `Display`: shortest round-trip digits, never an
/// exponent (`5`, `0.25`, `70000`).
String formatRustNumber(double f) {
  if (f == 0) return f.isNegative ? '-0' : '0';
  if (f.isNaN) return 'NaN';
  if (f.isInfinite) return f.isNegative ? '-inf' : 'inf';
  final e = f.abs().toStringAsExponential();
  final ePos = e.indexOf('e');
  final digits = e.substring(0, ePos).replaceFirst('.', '');
  final kk = int.parse(e.substring(ePos + 1)) + 1;
  final sb = StringBuffer();
  if (f < 0) sb.write('-');
  if (kk <= 0) {
    sb.write('0.');
    sb.write('0' * -kk);
    sb.write(digits);
  } else if (kk >= digits.length) {
    sb.write(digits);
    sb.write('0' * (kk - digits.length));
  } else {
    sb.write(digits.substring(0, kk));
    sb.write('.');
    sb.write(digits.substring(kk));
  }
  return sb.toString();
}

/// Does the pattern repeat something that already repeats — the `(a+)+` shape
/// behind exponential backtracking? Dart's RegExp backtracks, so such a pattern
/// is refused (skipped) instead of run, as in the reference.
/// Mirrors `has_nested_quantifier` in engine.rs / `hasNestedQuantifier` in
/// packages/synx-js/src/synxl.ts.
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
                (b == 0x7B &&
                    k + 1 < body.length &&
                    body[k + 1] >= 0x30 &&
                    body[k + 1] <= 0x39)) {
              return true;
            }
          }
        }
      }
    }
  }
  return false;
}
