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

/// Shortest round-trip form with the same layout as Rust's `ryu` (what the
/// reference `to_json` emits): plain decimals while the decimal exponent stays
/// within [-5, 16), otherwise `d.ddde<N>`, and a mandatory `.0` on whole
/// numbers. Dart's own `toString` switches layout at different magnitudes
/// (`1e16` prints as `10000000000000000.0`, `1e-6` as `0.000001`).
String _floatString(double f) {
  if (f == 0) return f.isNegative ? '-0.0' : '0.0';
  final e = f.abs().toStringAsExponential();
  final ePos = e.indexOf('e');
  final digits = e.substring(0, ePos).replaceFirst('.', '');
  final exp = int.parse(e.substring(ePos + 1));
  final length = digits.length;
  final kk = exp + 1;
  final k = kk - length;

  final sb = StringBuffer();
  if (f < 0) sb.write('-');
  if (k >= 0 && kk <= 16) {
    sb.write(digits);
    sb.write('0' * k);
    sb.write('.0');
  } else if (kk > 0 && kk <= 16) {
    sb.write(digits.substring(0, kk));
    sb.write('.');
    sb.write(digits.substring(kk));
  } else if (kk > -5 && kk <= 0) {
    sb.write('0.');
    sb.write('0' * -kk);
    sb.write(digits);
  } else if (length == 1) {
    sb.write(digits);
    sb.write('e');
    sb.write(kk - 1);
  } else {
    sb.write(digits.substring(0, 1));
    sb.write('.');
    sb.write(digits.substring(1));
    sb.write('e');
    sb.write(kk - 1);
  }
  return sb.toString();
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
