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
  for (final k in _sortedKeys(v.map)) {
    _writeEntry(out, k, v.map[k]!, 0, 0);
  }
  return out.toString();
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
      for (final k in _sortedKeys(m)) {
        _writeEntry(out, k, m[k]!, indent + 2, depth + 1);
      }
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
    _indent(out, indent);
    out
      ..write('- ')
      ..write(headKey);
    final text = _scalar(m[headKey]!);
    if (text != null && text.isNotEmpty) {
      out
        ..write(' ')
        ..write(text);
    }
    out.write('\n');
    for (var i = 0; i < keys.length; i++) {
      if (i == head && !repeatHead) continue;
      _writeEntry(out, keys[i], m[keys[i]]!, indent + 2, depth + 1);
    }
    return;
  }
  _indent(out, indent);
  out
    ..write('- ')
    ..write(_scalar(item) ?? (item is SynxStr ? item.value : ''))
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
