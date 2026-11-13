// Value → SYNX text that parses back to the same data — the SYNXL §14.1 block
// writer of synx-core (`synxl::write_synx_entry`): strings that would read as
// another type or be cut as a comment are quoted or carried as `|+`, multi-line
// text keeps its relative indent, a list item's head is repeated when needed,
// and secrets are written as their mask.

import 'num.dart';
import 'parser.dart' show maxNestingDepth;
import 'value.dart';

const int maxSerializeDepth = 128;

String stringify(SynxValue v) {
  if (v is! SynxObj) return formatPrimitive(v);
  final out = StringBuffer();
  _writeObject(out, v.map, 0, 0);
  return out.toString();
}

/// The entries of one object at [indent] columns, sorted by key.
void _writeObject(StringBuffer out, SynxObject m, int indent, int depth) {
  final names = _keyNames(m);
  for (final k in _sortedKeys(m)) {
    _writeEntry(out, names?[k] ?? k, m[k]!, indent, depth);
  }
}

/// Does [key], written at the start of a line, read back as itself?
bool keyReadsBack(String key) {
  if (key.isEmpty) return false;
  final first = key.codeUnitAt(0);
  // - # / [ : (
  if (first == 0x2D || first == 0x23 || first == 0x2F || first == 0x5B || first == 0x3A || first == 0x28) return false;
  // Lines the parser takes for directives.
  if (const {'!active', '!lock', '!tool', '!schema', '!llm', '!include', '!use'}.contains(key)) return false;
  // Keys the parser drops, with everything under them.
  if (key == '__proto__' || key == 'constructor' || key == 'prototype') return false;
  // Whitespace at an edge is trimmed away with the line; inside, only a space
  // or a tab ends a key (U+00A0 is a letter like any other).
  if (isWs(first) || isWs(key.codeUnitAt(key.length - 1))) return false;
  for (final c in key.runes) {
    if (_isControl(c) || c == 0x20 || c == 0x5B || c == 0x3A || c == 0x28) return false;
  }
  return true;
}

bool _isControl(int c) => c < 0x20 || (c >= 0x7F && c <= 0x9F);

/// [key] with every character that would end it or start something else
/// written as `_`.
///
/// safety: SYNX has no quoting for keys, and a key was written as it came.
/// `port:env` gave the next reader a marker, `!active` on a line of its own
/// switched the document to active mode, a line break began new lines — a map
/// with caller-chosen keys, saved and loaded, could read the environment.
String _writableKey(String key) {
  final cps = key.runes.toList();
  final sb = StringBuffer();
  final last = cps.length - 1;
  for (var i = 0; i < cps.length; i++) {
    final c = cps[i];
    final edge = i == 0 || i == last;
    final breaks = _isControl(c) ||
        c == 0x20 ||
        c == 0x5B ||
        c == 0x3A ||
        c == 0x28 ||
        (edge && c <= 0xFFFF && isWs(c)) ||
        (i == 0 && (c == 0x2D || c == 0x23 || c == 0x2F || c == 0x21));
    if (breaks) {
      sb.write('_');
    } else {
      sb.writeCharCode(c);
    }
  }
  final out = sb.isEmpty ? '_' : sb.toString();
  // safety: `constructor`, `prototype` and `__proto__` are keys the reader
  // refuses with what hangs under them; such a key gets a `_` after it.
  return out == '__proto__' || out == 'constructor' || out == 'prototype' ? '${out}_' : out;
}

/// The names the keys of [m] are written under, or null when every key reads
/// back as itself. Two keys never share a name: one already taken in the
/// object gets a number — `a_b`, `a_b_2`, `a_b_3`, in the order of its keys.
Map<String, String>? _keyNames(SynxObject m) {
  if (m.keyView.every(keyReadsBack)) return null;
  final renamed = <String, String>{};
  final taken = <String>{};
  final next = <String, int>{};
  for (final key in m.keyView) {
    if (keyReadsBack(key)) continue;
    final base = _writableKey(key);
    var name = base;
    while (m.contains(name) || taken.contains(name)) {
      final n = next[base] ?? 2;
      name = '${base}_$n';
      next[base] = n + 1;
    }
    taken.add(name);
    renamed[key] = name;
  }
  return renamed;
}

List<String> _sortedKeys(SynxObject m) => sortCodePoints(m.keys);

final List<String> _indents = List<String>.generate(64, (i) => ' ' * i);

void _indent(StringBuffer out, int n) {
  if (n > 0) out.write(n < _indents.length ? _indents[n] : ' ' * n);
}

/// Write `key value` as SYNX at [indent] columns.
void _writeEntry(StringBuffer out, String key, SynxValue value, int indent, int depth) {
  if (depth > maxNestingDepth) return;
  switch (value) {
    case SynxObj(map: var m):
      _indent(out, indent);
      out
        ..write(key)
        ..write('\n');
      _writeObject(out, m, indent + 2, depth + 1);
    case SynxArr(values: var items):
      _indent(out, indent);
      out.write(key);
      // A bare key parses back as an empty object; `:join` is the only SYNX
      // surface that yields an empty array.
      if (items.isEmpty) out.write(':join');
      out.write('\n');
      for (final item in items) {
        _writeItem(out, item, indent + 2, depth + 1);
      }
    case SynxStr(value: var s) when s.contains('\n'):
      _writeMultiline(out, key, s, indent);
    default:
      final text = _scalar(value);
      if (text != null) {
        _indent(out, indent);
        out
          ..write(key)
          ..write(' ')
          ..write(text)
          ..write('\n');
      } else {
        // Not one SYNX token: an indent-preserving block, never comment-stripped nor cast.
        _writeMultiline(out, key, value is SynxStr ? value.value : (value is SynxSecret ? value.value : ''), indent);
      }
  }
}

/// Write one `- item` list entry.
void _writeItem(StringBuffer out, SynxValue item, int indent, int depth) {
  if (depth > maxNestingDepth) return;
  if (item is SynxObj) {
    final m = item.map;
    final keys = _sortedKeys(m);
    // An empty object in a list has no key for the `- ` line: a bare dash.
    if (keys.isEmpty) {
      _indent(out, indent);
      out.write('-\n');
      return;
    }
    // The `- ` line carries a key whose value fits on it; SYNX attaches
    // everything deeper to the item, not to that key.
    final headSafe = keys.indexWhere((k) => _dashLineSafe(m[k]!));
    final head = headSafe < 0 ? 0 : headSafe;
    // §14.1 — an item is an object only when a deeper line follows its `- `
    // line; writing the head again in full covers a one-key item and an item
    // whose every value is too big for the `- ` line (a repeated key is legal,
    // the last one wins).
    final repeatHead = headSafe < 0 || keys.length == 1;
    final headKey = keys[head];
    final names = _keyNames(m);
    _indent(out, indent);
    out
      ..write('- ')
      ..write(names?[headKey] ?? headKey);
    final text = _scalar(m[headKey]!);
    if (text != null && text.isNotEmpty) {
      out
        ..write(' ')
        ..write(text);
    }
    out.write('\n');
    for (var i = 0; i < keys.length; i++) {
      if (i == head && !repeatHead) continue;
      _writeEntry(out, names?[keys[i]] ?? keys[i], m[keys[i]]!, indent + 2, depth + 1);
    }
    return;
  }
  _indent(out, indent);
  // safety: a list item is one line and SYNX has no block form for it, so a
  // line break is written as a space; written as it was, the rest of the text
  // became lines of their own.
  out
    ..write('- ')
    ..write(_scalar(item) ?? (item is SynxStr ? item.value.replaceAll('\n', ' ').replaceAll('\r', ' ') : ''))
    ..write('\n');
}

/// May this value share the `- ` line of a list item?
bool _dashLineSafe(SynxValue v) => switch (v) {
      SynxObj(map: var m) => m.isEmpty,
      SynxArr() => false,
      SynxStr(value: var s) => !s.contains('\n') && _scalar(v) != null && s.isNotEmpty,
      _ => true,
    };

/// `key |+` plus an indented body (§14.3).
void _writeMultiline(StringBuffer out, String key, String body, int indent) {
  _indent(out, indent);
  out
    ..write(key)
    ..write(' |+\n');
  for (final line in body.split('\n')) {
    _indent(out, indent + 2);
    out
      ..write(line)
      ..write('\n');
  }
}

/// A scalar as one SYNX value token, or null when none can carry it. A value is
/// comment-stripped at ` #` / ` //` before casting, so quoting cannot rescue those.
String? _scalar(SynxValue v) {
  switch (v) {
    case SynxNull():
      return 'null';
    case SynxBool(value: var b):
      return b ? 'true' : 'false';
    case SynxInt(value: var n):
      return n.toString();
    case SynxFloat(value: var f):
      // A non-finite float has no JSON form either; `null` is the projection-preserving token.
      if (!f.isFinite) return 'null';
      return _float(f);
    // safety: a secret is written as its mask, never as the plaintext.
    case SynxSecret():
      return '[SECRET]';
    case SynxStr(value: var s):
      if (s.contains('\n') || s.contains(' #') || s.contains(' //')) return null;
      if (!_needsQuote(s)) return s;
      if (!s.contains('"')) return '"$s"';
      if (!s.contains("'")) return "'$s'";
      return null;
    default:
      return null;
  }
}

/// A float the SYNX cast reads back as the same float: Rust `Display`, `.0` when whole.
String _float(double f) {
  final s = rustDisplay(f);
  return s.contains('.') ? s : '$s.0';
}

/// Would this string be misread as a bare SYNX value?
bool _needsQuote(String s) =>
    s.isEmpty ||
    isWs(s.codeUnitAt(0)) ||
    isWs(s.codeUnitAt(s.length - 1)) ||
    s == '|' ||
    s == '|+' ||
    _isQuoteWrapped(s) ||
    !_autoCastKeepsString(s);

bool _isQuoteWrapped(String s) {
  if (s.length < 2) return false;
  final a = s.codeUnitAt(0);
  final b = s.codeUnitAt(s.length - 1);
  return (a == 0x22 && b == 0x22) || (a == 0x27 && b == 0x27);
}

/// Does SYNX §8.3 casting leave [s] a string?
bool _autoCastKeepsString(String s) {
  if (_isQuoteWrapped(s)) return true;
  if (s == 'true' || s == 'false' || s == 'null') return false;
  final start = s.isNotEmpty && s.codeUnitAt(0) == 0x2D ? 1 : 0;
  if (s.length <= start) return true;
  final c0 = s.codeUnitAt(start);
  if (c0 < 0x30 || c0 > 0x39) return true;
  var dot = -1;
  for (var j = start; j < s.length; j++) {
    final c = s.codeUnitAt(j);
    if (c == 0x2E) {
      if (dot >= 0) return true;
      dot = j;
    } else if (c < 0x30 || c > 0x39) {
      return true;
    }
  }
  if (dot < 0) return parseI64(s) == null;
  return !(dot > start && dot < s.length - 1);
}

/// A root that is not an object (Rust `format_primitive`).
String formatPrimitive(SynxValue v) => switch (v) {
      SynxStr(value: var s) => s,
      SynxInt(value: var n) => n.toString(),
      SynxFloat(value: var f) => _float(f),
      SynxBool(value: var b) => b ? 'true' : 'false',
      SynxNull() => 'null',
      SynxArr(values: var arr) => '[${arr.map(formatPrimitive).join(', ')}]',
      SynxObj() => '[Object]',
      SynxSecret() => '[SECRET]',
    };
