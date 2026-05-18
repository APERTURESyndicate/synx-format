// Canonical JSON encoder — sorted keys, secrets redacted, floats with marker.

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
      return;
    case SynxBool(value: var b):
      out.write(b ? 'true' : 'false');
      return;
    case SynxInt(value: var n):
      out.write(n.toString());
      return;
    case SynxFloat(value: var f):
      if (f.isNaN || f.isInfinite) {
        out.write('null');
        return;
      }
      out.write(_floatString(f));
      return;
    case SynxStr(value: var s):
      out.write('"');
      _escape(out, s);
      out.write('"');
      return;
    case SynxSecret():
      out.write('"[SECRET]"');
      return;
    case SynxArr(values: var arr):
      out.write('[');
      for (var i = 0; i < arr.length; i++) {
        if (i > 0) out.write(',');
        _write(out, arr[i], depth + 1);
      }
      out.write(']');
      return;
    case SynxObj(map: var map):
      out.write('{');
      final keys = map.sortedKeys;
      var first = true;
      for (final k in keys) {
        if (!first) out.write(',');
        first = false;
        out.write('"');
        _escape(out, k);
        out.write('":');
        _write(out, map[k] ?? synxNull(), depth + 1);
      }
      out.write('}');
      return;
  }
}

String _floatString(double f) {
  // Dart's `toStringAsPrecision(17)` keeps integer values as `1.0000000000000000`,
  // which differs from Rust ryu. Match Rust shortest-round-trip by using
  // `toString()` (returns "1.0" for 1.0, "99.5" for 99.5, "1.5e-10" for tiny).
  var s = f.toString();
  if (!s.contains('.') && !s.contains('e') && !s.contains('E')) {
    s = '$s.0';
  }
  return s;
}

void _escape(StringBuffer out, String s) {
  for (var i = 0; i < s.length; i++) {
    final c = s.codeUnitAt(i);
    switch (c) {
      case 0x22:
        out.write('\\"');
        break;
      case 0x5C:
        out.write('\\\\');
        break;
      case 0x0A:
        out.write('\\n');
        break;
      case 0x0D:
        out.write('\\r');
        break;
      case 0x09:
        out.write('\\t');
        break;
      default:
        if (c < 0x20) {
          out.write('\\u');
          out.write(c.toRadixString(16).padLeft(4, '0'));
        } else {
          out.writeCharCode(c);
        }
    }
  }
}
