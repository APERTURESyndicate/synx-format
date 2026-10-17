// Number text exactly as synx-core writes and reads it: Rust f64 `Display`, the
// ryu layout of `write_json`, `{:.N}` / `{:.Ne}` with exact round-half-to-even,
// and the strict `str::parse` grammars.
import 'dart:typed_data';

/// Shortest round-trip digits of a finite [d]: the value is `0.digits × 10^point`.
/// Zero is `('0', 1)`.
(String, int, bool) _shortest(double d) {
  final neg = d < 0 || (d == 0 && d.isNegative);
  if (d == 0) return ('0', 1, neg);
  // Without an argument Dart writes the shortest round-trip digits: "1.2345e+2".
  final e = d.abs().toStringAsExponential();
  final ePos = e.indexOf('e');
  var digits = e.substring(0, ePos).replaceFirst('.', '');
  final point = int.parse(e.substring(ePos + 1)) + 1;
  var end = digits.length;
  while (end > 1 && digits.codeUnitAt(end - 1) == 0x30) {
    end--;
  }
  digits = digits.substring(0, end);
  return (digits, point, neg);
}

/// Rust f64 `Display`: shortest round-trip digits, plain decimal, never an exponent.
String rustDisplay(double d) {
  if (d.isNaN) return 'NaN';
  if (d.isInfinite) return d > 0 ? 'inf' : '-inf';
  final (digits, point, neg) = _shortest(d);
  final sb = StringBuffer();
  if (neg) sb.write('-');
  if (point <= 0) {
    sb
      ..write('0.')
      ..write('0' * -point)
      ..write(digits);
  } else if (point >= digits.length) {
    sb
      ..write(digits)
      ..write('0' * (point - digits.length));
  } else {
    sb
      ..write(digits.substring(0, point))
      ..write('.')
      ..write(digits.substring(point));
  }
  return sb.toString();
}

const double _i64MaxAsF64 = 9223372036854775807.0;

/// synx-core `format_number`: a whole value below 2^63 prints as an integer,
/// anything else as [rustDisplay].
String formatNumber(double n) {
  if (n.isFinite && n % 1 == 0 && n.abs() < _i64MaxAsF64) return n.toInt().toString();
  return rustDisplay(n);
}

/// The ryu layout `write_json` uses for a finite float (`1.0`, `1e-7`, `1.5e300`).
void writeRyu(StringBuffer o, double d) {
  final (digits, point, neg) = _shortest(d);
  if (neg) o.write('-');
  final len = digits.length;
  final k = point - len;
  if (k >= 0 && point <= 16) {
    o
      ..write(digits)
      ..write('0' * k)
      ..write('.0');
  } else if (point > 0 && point <= 16) {
    o
      ..write(digits.substring(0, point))
      ..write('.')
      ..write(digits.substring(point));
  } else if (point > -5 && point <= 0) {
    o
      ..write('0.')
      ..write('0' * -point)
      ..write(digits);
  } else {
    o.write(digits[0]);
    if (len > 1) {
      o
        ..write('.')
        ..write(digits.substring(1));
    }
    o
      ..write('e')
      ..write(point - 1);
  }
}

/// Rust `{:e}`: shortest digits in exponent form (`1.5e-7`, `1e0`).
String lowerExpShortest(double d) {
  if (d.isNaN) return 'NaN';
  if (d.isInfinite) return d > 0 ? 'inf' : '-inf';
  final (digits, point, neg) = _shortest(d);
  final sb = StringBuffer();
  if (neg) sb.write('-');
  sb.write(digits[0]);
  if (digits.length > 1) {
    sb
      ..write('.')
      ..write(digits.substring(1));
  }
  sb
    ..write('e')
    ..write(digits == '0' ? 0 : point - 1);
  return sb.toString();
}

// ─── Exact decimal expansion (for {:.N} and {:.Ne}) ──────────────────────────

/// The exact decimal value of a finite |d| as `digits × 10^-scale`.
(BigInt, int) _exact(double d) {
  final bd = ByteData(8)..setFloat64(0, d.abs());
  final bits = bd.getInt64(0);
  var exp = (bits >> 52) & 0x7FF;
  var mant = bits & 0xFFFFFFFFFFFFF;
  if (exp == 0) {
    exp = 1;
  } else {
    mant |= 1 << 52;
  }
  final e2 = exp - 1075;
  if (e2 >= 0) return (BigInt.from(mant) << e2, 0);
  return (BigInt.from(mant) * BigInt.from(5).pow(-e2), -e2);
}

/// Round the decimal string [s] to its first [keep] digits, ties to even.
(String, bool) _roundDigits(String s, int keep) {
  if (keep >= s.length) return (s, false);
  if (keep < 0) return ('', false);
  var up = false;
  final first = s.codeUnitAt(keep);
  if (first > 0x35) {
    up = true;
  } else if (first == 0x35) {
    var rest = false;
    for (var i = keep + 1; i < s.length; i++) {
      if (s.codeUnitAt(i) != 0x30) {
        rest = true;
        break;
      }
    }
    up = rest || (keep > 0 && (s.codeUnitAt(keep - 1) - 0x30).isOdd);
  }
  final kept = s.substring(0, keep).codeUnits.toList();
  var carry = false;
  if (up) {
    var i = kept.length - 1;
    while (i >= 0 && kept[i] == 0x39) {
      kept[i] = 0x30;
      i--;
    }
    if (i >= 0) {
      kept[i]++;
    } else {
      carry = true;
    }
  }
  return (String.fromCharCodes(kept), carry);
}

/// Rust `format!("{:.prec$}", d)`.
String rustFixed(double d, int prec) {
  if (d.isNaN) return 'NaN';
  if (d.isInfinite) return d > 0 ? 'inf' : '-inf';
  final neg = d < 0 || (d == 0 && d.isNegative);
  final (big, scale) = _exact(d);
  var s = big.toString();
  if (s.length <= scale) s = '${'0' * (scale - s.length + 1)}$s';
  String intPart;
  String frac;
  if (scale <= prec) {
    intPart = s.substring(0, s.length - scale);
    frac = s.substring(s.length - scale) + '0' * (prec - scale);
  } else {
    final keep = s.length - scale + prec;
    var (r, carry) = _roundDigits(s, keep);
    if (carry) r = '1$r';
    intPart = r.substring(0, r.length - prec);
    frac = r.substring(r.length - prec);
  }
  final sb = StringBuffer();
  if (neg) sb.write('-');
  sb.write(intPart);
  if (prec > 0) {
    sb
      ..write('.')
      ..write(frac);
  }
  return sb.toString();
}

/// Rust `format!("{:.prec$e}", d)`.
String rustExpFixed(double d, int prec) {
  if (d.isNaN) return 'NaN';
  if (d.isInfinite) return d > 0 ? 'inf' : '-inf';
  final neg = d < 0 || (d == 0 && d.isNegative);
  final sb = StringBuffer();
  if (neg) sb.write('-');
  if (d == 0) {
    sb.write('0');
    if (prec > 0) {
      sb
        ..write('.')
        ..write('0' * prec);
    }
    sb.write('e0');
    return sb.toString();
  }
  final (big, scale) = _exact(d);
  final s = big.toString();
  var exp = s.length - 1 - scale;
  var (r, carry) = _roundDigits(s, prec + 1);
  if (carry) {
    r = '1${r.substring(0, r.length - 1)}';
    exp++;
  }
  if (r.length < prec + 1) r += '0' * (prec + 1 - r.length);
  sb.write(r[0]);
  if (prec > 0) {
    sb
      ..write('.')
      ..write(r.substring(1, prec + 1));
  }
  sb
    ..write('e')
    ..write(exp);
  return sb.toString();
}

// ─── Rust str::parse grammars ────────────────────────────────────────────────

bool _isDigit(int c) => c >= 0x30 && c <= 0x39;

/// Rust `str::parse::<f64>`: `[+-]? (digits[.digits?] | .digits) ([eE][+-]?digits)?`,
/// or inf / infinity / nan in any case. Null when the text is not that.
double? parseF64(String s) {
  if (s.isEmpty) return null;
  var i = 0;
  var neg = false;
  final c0 = s.codeUnitAt(0);
  if (c0 == 0x2B || c0 == 0x2D) {
    neg = c0 == 0x2D;
    i = 1;
  }
  final body = s.substring(i).toLowerCase();
  if (body == 'inf' || body == 'infinity') return neg ? double.negativeInfinity : double.infinity;
  if (body == 'nan') return double.nan;
  final intStart = i;
  while (i < s.length && _isDigit(s.codeUnitAt(i))) {
    i++;
  }
  final intDigits = i - intStart;
  var fracStart = i;
  var fracDigits = 0;
  if (i < s.length && s.codeUnitAt(i) == 0x2E) {
    i++;
    fracStart = i;
    while (i < s.length && _isDigit(s.codeUnitAt(i))) {
      i++;
    }
    fracDigits = i - fracStart;
  }
  if (intDigits == 0 && fracDigits == 0) return null;
  var expPart = '';
  if (i < s.length && (s.codeUnitAt(i) == 0x65 || s.codeUnitAt(i) == 0x45)) {
    final expStart = i + 1;
    i++;
    if (i < s.length && (s.codeUnitAt(i) == 0x2B || s.codeUnitAt(i) == 0x2D)) i++;
    final digitsStart = i;
    while (i < s.length && _isDigit(s.codeUnitAt(i))) {
      i++;
    }
    if (i == digitsStart) return null;
    expPart = 'e${s.substring(expStart, i)}';
  }
  if (i != s.length) return null;
  // Canonical spelling for double.parse: no bare `.`, no empty integer part.
  final intText = intDigits == 0 ? '0' : s.substring(intStart, intStart + intDigits);
  final fracText = fracDigits == 0 ? '' : '.${s.substring(fracStart, fracStart + fracDigits)}';
  return double.parse('${neg ? '-' : ''}$intText$fracText$expPart');
}

final BigInt _i64Min = BigInt.parse('-9223372036854775808');
final BigInt _i64Max = BigInt.parse('9223372036854775807');

/// Rust `str::parse::<i64>`: an optional sign and ASCII digits, no overflow.
int? parseI64(String s) {
  var i = 0;
  if (s.isNotEmpty && (s.codeUnitAt(0) == 0x2B || s.codeUnitAt(0) == 0x2D)) i = 1;
  if (i >= s.length) return null;
  for (var j = i; j < s.length; j++) {
    if (!_isDigit(s.codeUnitAt(j))) return null;
  }
  if (s.length - i <= 18) return int.parse(s);
  final b = BigInt.parse(s);
  if (b < _i64Min || b > _i64Max) return null;
  return b.toInt();
}

/// Rust `str::parse` of an unsigned type with maximum [max] (u64, usize, u32):
/// optional `+`, ASCII digits. The result is capped at the largest Dart int.
int? parseUnsigned(String s, BigInt max) {
  var i = 0;
  if (s.isNotEmpty && s.codeUnitAt(0) == 0x2B) i = 1;
  if (i >= s.length) return null;
  for (var j = i; j < s.length; j++) {
    if (!_isDigit(s.codeUnitAt(j))) return null;
  }
  final b = BigInt.parse(s.substring(i));
  if (b > max) return null;
  return b > _i64Max ? _i64Max.toInt() : b.toInt();
}

final BigInt u64Max = BigInt.parse('18446744073709551615');
final BigInt u32Max = BigInt.from(4294967295);

/// Rust `f as i64`: saturating, NaN is 0.
int saturatingI64(double d) {
  if (d.isNaN) return 0;
  if (d >= 9223372036854775807.0) return 9223372036854775807;
  if (d <= -9223372036854775808.0) return -9223372036854775807 - 1;
  return d.toInt();
}

/// UTF-8 byte length of one UTF-16 unit; a surrogate pair counts 4 over its two units.
int utf8LenUnit(int c) => c < 0x80 ? 1 : c < 0x800 ? 2 : (c >= 0xD800 && c <= 0xDFFF) ? 2 : 3;

/// UTF-8 byte length of a string.
int utf8Len(String s) {
  var n = 0;
  for (var i = 0; i < s.length; i++) {
    n += utf8LenUnit(s.codeUnitAt(i));
  }
  return n;
}

/// Order by Unicode code point — Rust's byte order of UTF-8 strings
/// (`String.compareTo` compares UTF-16 units, which differ above the surrogates).
int compareCodePoints(String a, String b) {
  final n = a.length < b.length ? a.length : b.length;
  for (var i = 0; i < n; i++) {
    final x = a.codeUnitAt(i);
    final y = b.codeUnitAt(i);
    if (x == y) continue;
    final xs = x >= 0xD800 && x <= 0xDFFF;
    final ys = y >= 0xD800 && y <= 0xDFFF;
    if (xs != ys) return xs ? 1 : -1;
    return x - y;
  }
  return a.length - b.length;
}

/// SYNX whitespace: Unicode White_Space, the set Rust `char::is_whitespace` accepts.
bool isWs(int c) {
  if (c <= 0x20) return c == 0x20 || (c >= 0x09 && c <= 0x0D);
  if (c < 0x85) return false;
  return c == 0x85 ||
      c == 0xA0 ||
      c == 0x1680 ||
      (c >= 0x2000 && c <= 0x200A) ||
      c == 0x2028 ||
      c == 0x2029 ||
      c == 0x202F ||
      c == 0x205F ||
      c == 0x3000;
}

/// `str::trim` over White_Space (Dart's `trim` also strips U+FEFF; Rust does not).
String trimWs(String s) {
  var a = 0;
  var b = s.length;
  while (a < b && isWs(s.codeUnitAt(a))) {
    a++;
  }
  while (b > a && isWs(s.codeUnitAt(b - 1))) {
    b--;
  }
  return (a == 0 && b == s.length) ? s : s.substring(a, b);
}

String trimWsStart(String s) {
  var a = 0;
  while (a < s.length && isWs(s.codeUnitAt(a))) {
    a++;
  }
  return a == 0 ? s : s.substring(a);
}

String trimWsEnd(String s) {
  var b = s.length;
  while (b > 0 && isWs(s.codeUnitAt(b - 1))) {
    b--;
  }
  return b == s.length ? s : s.substring(0, b);
}

/// Sort [keys] by code point. perf: when no key holds a unit at or above
/// U+D800 — nearly always — UTF-16 order is code-point order and the native
/// comparison is used.
List<String> sortCodePoints(List<String> keys) {
  for (final k in keys) {
    for (var i = 0; i < k.length; i++) {
      if (k.codeUnitAt(i) >= 0xD800) return keys..sort(compareCodePoints);
    }
  }
  return keys..sort();
}
