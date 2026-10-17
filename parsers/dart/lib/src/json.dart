// Canonical JSON encoder — synx-core `write_json`: keys sorted by code point,
// secrets redacted, floats in the ryu layout.

import 'num.dart';
import 'value.dart';

const int maxJsonDepth = 128;

String toJson(SynxValue v) {
  final sb = StringBuffer();
  _write(sb, v, 0);
  return sb.toString();
}

void _write(StringBuffer out, SynxValue v, int depth) {
  if (depth > maxJsonDepth) {
    out.write('null');
    return;
  }
  switch (v) {
    case SynxNull():
      out.write('null');
    case SynxBool(value: var b):
      out.write(b ? 'true' : 'false');
    case SynxInt(value: var n):
      out.write(n);
    case SynxFloat(value: var f):
      // JSON has no NaN / ±Infinity — emit null so output stays valid.
      if (f.isNaN || f.isInfinite) {
        out.write('null');
      } else {
        writeRyu(out, f);
      }
    case SynxStr(value: var s):
      _string(out, s);
    case SynxSecret():
      out.write('"[SECRET]"');
    case SynxArr(values: var arr):
      out.write('[');
      for (var i = 0; i < arr.length; i++) {
        if (i > 0) out.write(',');
        _write(out, arr[i], depth + 1);
      }
      out.write(']');
    case SynxObj(map: var map):
      out.write('{');
      final keys = sortCodePoints(map.keys);
      for (var i = 0; i < keys.length; i++) {
        if (i > 0) out.write(',');
        _string(out, keys[i]);
        out.write(':');
        _write(out, map[keys[i]]!, depth + 1);
      }
      out.write('}');
  }
}

const String _hex = '0123456789abcdef';

/// A JSON string literal. perf: unescaped runs are copied whole.
void _string(StringBuffer out, String s) {
  out.write('"');
  var start = 0;
  for (var i = 0; i < s.length; i++) {
    final c = s.codeUnitAt(i);
    if (c >= 0x20 && c != 0x22 && c != 0x5C) continue;
    if (i > start) out.write(s.substring(start, i));
    switch (c) {
      case 0x22:
        out.write('\\"');
      case 0x5C:
        out.write('\\\\');
      case 0x0A:
        out.write('\\n');
      case 0x0D:
        out.write('\\r');
      case 0x09:
        out.write('\\t');
      default:
        out
          ..write('\\u00')
          ..write(_hex[c >> 4])
          ..write(_hex[c & 0xF]);
    }
    start = i + 1;
  }
  out.write(start == 0 ? s : s.substring(start));
  out.write('"');
}
