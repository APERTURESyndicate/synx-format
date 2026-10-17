@tool
class_name SynxNum
extends RefCounted

# Numbers as synx-core reads and writes them, bit for bit.
#
# Godot's own conversions cannot be used for this: `String.to_float` is not
# correctly rounded (about one 17-digit decimal in four comes back one ULP
# off, and values near 1e-308 come back as 0), and `var_to_str` round-trips
# but is not always the shortest form. This module reads decimals with the
# grammar and rounding of Rust's `f64::from_str`, and writes the shortest
# round-trip digits in the layouts of ryu (JSON) and of Rust `Display`.
# Exact work is done with small big integers (base 2^24 limbs).

const I64_MAX: int = 9223372036854775807
const I64_MIN: int = -9223372036854775807 - 1
# `i64::MAX as f64` — the bound synx-core uses to decide "fits an integer".
const I64_MAX_F: float = 9223372036854775808.0

const _FRAC_MASK: int = (1 << 52) - 1
const _HIDDEN: int = 1 << 52
const _BITS: int = 24
const _BASE: int = 1 << 24
const _MASK: int = (1 << 24) - 1
const _MAX_FINITE_BITS: int = 0x7FEFFFFFFFFFFFFF

# Never write the literal `-0.0` in GDScript: the compiler pools it with `0.0`
# (they compare equal), so every `0.0` in that function becomes negative.
# Negate a zero at run time instead.

static var _buf: PackedByteArray = PackedByteArray([0, 0, 0, 0, 0, 0, 0, 0])
static var _p10: PackedFloat64Array = PackedFloat64Array()
static var _rx_f64: RegEx = null
static var _pow5_cache: Dictionary = {}


# ─── Bits ──

static func bits_of(x: float) -> int:
	_buf.encode_double(0, x)
	return _buf.decode_s64(0)


static func from_bits(b: int) -> float:
	_buf.encode_s64(0, b)
	return _buf.decode_double(0)


static func is_negative(x: float) -> bool:
	return bits_of(x) < 0


static func _pow10f() -> PackedFloat64Array:
	if _p10.is_empty():
		var p := 1.0
		for i in 23:
			_p10.append(p)
			p *= 10.0
	return _p10


# ─── Integer conversions ──

# Rust `f64::fract() == 0.0`.
static func is_whole(x: float) -> bool:
	return x - trunc(x) == 0.0


static func trunc(x: float) -> float:
	return floorf(x) if x >= 0.0 else ceilf(x)


# Rust `x as i64` (saturating, NaN → 0).
static func to_i64(x: float) -> int:
	if is_nan(x):
		return 0
	if x >= I64_MAX_F:
		return I64_MAX
	if x <= -I64_MAX_F:
		return I64_MIN
	return int(x)


# The integer a whole float stands for, as synx-core stores a `:calc` result:
# `Int` when `fract() == 0 && abs() < i64::MAX as f64`, `Float` otherwise.
static func fits_int(x: float) -> bool:
	return is_whole(x) and absf(x) < I64_MAX_F


# Rust `i64::from_str`: `[+-]?[0-9]+`, in range. null when it is not.
static func parse_i64(s: String) -> Variant:
	if not s.is_valid_int():
		return null
	var c0 := s.unicode_at(0)
	var neg := c0 == 45
	var digits := s.substr(1) if (c0 == 45 or c0 == 43) else s
	digits = digits.lstrip("0")
	if digits.is_empty():
		return 0
	if digits.length() > 19:
		return null
	if digits.length() == 19:
		if neg:
			if digits > "9223372036854775808":
				return null
			if digits == "9223372036854775808":
				return I64_MIN
		elif digits > "9223372036854775807":
			return null
	var v := digits.to_int()
	return -v if neg else v


# Rust `u64::from_str` (`usize` on 64-bit): `+?[0-9]+`. -1 when invalid; a
# value above i64::MAX saturates there.
static func parse_u64(s: String) -> int:
	var n := s.length()
	if n == 0:
		return -1
	var start := 1 if s.unicode_at(0) == 43 else 0
	if start == n:
		return -1
	for i in range(start, n):
		var c := s.unicode_at(i)
		if c < 48 or c > 57:
			return -1
	var digits := s.substr(start).lstrip("0")
	if digits.is_empty():
		return 0
	if digits.length() > 20 or (digits.length() == 20 and digits > "18446744073709551615"):
		return -1
	if digits.length() > 19 or (digits.length() == 19 and digits > "9223372036854775807"):
		return I64_MAX
	return digits.to_int()


# Rust `u32::from_str`. -1 when invalid.
static func parse_u32(s: String) -> int:
	var v := parse_u64(s)
	if v < 0 or v > 4294967295:
		return -1
	return v


# ─── Float parsing ──

# Whether `s` is a Rust `f64` literal (sign, decimal or exponent form, inf,
# infinity, nan — case-insensitive), the grammar `str::parse::<f64>` accepts.
static func is_f64_text(s: String) -> bool:
	if _rx_f64 == null:
		_rx_f64 = RegEx.new()
		_rx_f64.compile("^[+-]?(?:(?:[0-9]+\\.?[0-9]*|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?|[iI][nN][fF](?:[iI][nN][iI][tT][yY])?|[nN][aA][nN])$")
	return _rx_f64.search(s) != null


# Rust `str::parse::<f64>()`: the correctly rounded value, or null.
static func parse_f64(s: String) -> Variant:
	var fast: Variant = _fast_decimal(s)
	if fast != null:
		return fast
	if s.is_empty():
		return null
	# perf: a literal starts with a digit, a sign, a dot, `inf` or `nan`.
	var c := s.unicode_at(0)
	if not ((c >= 48 and c <= 57) or c == 43 or c == 45 or c == 46 or c == 105 or c == 73 or c == 110 or c == 78):
		return null
	if not is_f64_text(s):
		return null
	return parse_f64_valid(s)


# perf: `[+-]?digits[.digits]` with at most 15 digits is one exact division
# (Clinger's fast path) — no grammar regex, no big integers. null when the
# text is anything else; the general path decides it then.
static func _fast_decimal(s: String) -> Variant:
	var n := s.length()
	if n == 0 or n > 17:
		return null
	var c := s.unicode_at(0)
	var start := 1 if (c == 45 or c == 43) else 0
	if start >= n:
		return null
	var d0 := s.unicode_at(start)
	if d0 < 48 or d0 > 57:
		return null
	var dot := s.find(".")
	var ip: String
	var fp := ""
	if dot < 0:
		ip = s.substr(start)
	else:
		ip = s.substr(start, dot - start)
		fp = s.substr(dot + 1)
		if not fp.is_empty():
			var f0 := fp.unicode_at(0)
			if f0 < 48 or f0 > 57 or not fp.is_valid_int():
				return null
	if not ip.is_valid_int():
		return null
	var digits := ip + fp
	if digits.length() > 15:
		return null
	var v := float(digits.to_int()) / _pow10f()[fp.length()]
	return -v if c == 45 else v


# `parse_f64` for text already known to be in the grammar.
static func parse_f64_valid(s: String) -> float:
	var n := s.length()
	var i := 0
	var neg := false
	var c := s.unicode_at(0)
	if c == 43 or c == 45:
		neg = c == 45
		i = 1
	c = s.unicode_at(i)
	if c == 105 or c == 73:
		return -INF if neg else INF
	if c == 110 or c == 78:
		return NAN
	var e_pos := s.findn("e", i)
	var mant := s.substr(i, (e_pos - i) if e_pos >= 0 else (n - i))
	var exp10 := 0
	if e_pos >= 0:
		var es := s.substr(e_pos + 1)
		var eneg := es.begins_with("-")
		if es.begins_with("-") or es.begins_with("+"):
			es = es.substr(1)
		es = es.lstrip("0")
		if es.length() > 9:
			exp10 = 999999999
		elif not es.is_empty():
			exp10 = es.to_int()
		if eneg:
			exp10 = -exp10
	var dot := mant.find(".")
	var all := mant
	var frac_len := 0
	if dot >= 0:
		frac_len = mant.length() - dot - 1
		all = mant.substr(0, dot) + mant.substr(dot + 1)
	var q := exp10 - frac_len
	all = all.lstrip("0")
	var full := all.length()
	all = all.rstrip("0")
	q += full - all.length()
	var v := 0.0 if all.is_empty() else decimal_to_f64(all, q)
	return -v if neg else v


# The double nearest `d × 10^q` (ties to even). `d` is a non-empty digit
# string without leading or trailing zeros.
static func decimal_to_f64(d: String, q: int) -> float:
	var n := d.length()
	var p10 := _pow10f()
	# Clinger's fast path: an exact integer times an exact power of ten is
	# rounded once, which is the correct rounding.
	if n <= 15:
		if q >= 0 and q <= 22:
			return float(d.to_int()) * p10[q]
		if q < 0 and q >= -22:
			return float(d.to_int()) / p10[-q]
		if q > 22 and q <= 22 + 15 - n:
			return (float(d.to_int()) * p10[q - 22]) * p10[22]
	var mag := n + q
	if mag > 310:
		return INF
	if mag < -324:
		return 0.0
	if n > 800:
		# Digits past the 800th only matter as "something is there": every
		# rounding boundary of a double has fewer significant digits.
		d = d.substr(0, 800) + "1"
		q += n - 801
		n = 801
	var z := _estimate(d, q)
	if is_inf(z):
		z = from_bits(_MAX_FINITE_BITS)
	var big_d := _from_decimal(d)
	# q >= 0: the decimal side is d·5^q (fixed); q < 0: the binary side gets 5^-q.
	var dec_side := _mul(big_d, _pow5(q)) if q >= 0 else big_d
	var p5 := _pow5(-q) if q < 0 else PackedInt64Array()
	for _guard in 4000:
		var b := bits_of(z)
		var biased := (b >> 52) & 0x7FF
		var frac := b & _FRAC_MASK
		var m: int
		var e: int
		if biased == 0:
			m = frac
			e = -1074
		else:
			m = frac | _HIDDEN
			e = biased - 1075
		var c := _cmp_dec_bin(dec_side, p5, q, 2 * m + 1, e - 1)
		if c > 0 or (c == 0 and (m & 1) == 1):
			if b == _MAX_FINITE_BITS:
				return INF
			z = from_bits(b + 1)
			continue
		if m == 0:
			return z
		var lo_n: int
		var lo_e: int
		if frac == 0 and biased > 1:
			lo_n = 4 * m - 1
			lo_e = e - 2
		else:
			lo_n = 2 * m - 1
			lo_e = e - 1
		c = _cmp_dec_bin(dec_side, p5, q, lo_n, lo_e)
		if c < 0 or (c == 0 and (m & 1) == 1):
			z = from_bits(b - 1)
			continue
		return z
	return z


# A float within a few ULP of d × 10^q.
static func _estimate(d: String, q: int) -> float:
	var head := d.substr(0, 17)
	var k := q + d.length() - head.length()
	var v := float(head.to_int())
	var p10 := _pow10f()
	while k > 22:
		v *= p10[22]
		k -= 22
	while k < -22:
		v /= p10[22]
		k += 22
	if k >= 0:
		v *= p10[k]
	else:
		v /= p10[-k]
	if v == 0.0:
		v = from_bits(1)
	return v


# Compare d·10^q with n·2^j, d·5^q given as `dec_side` (q >= 0) or d given
# with `p5` = 5^-q (q < 0).
static func _cmp_dec_bin(dec_side: PackedInt64Array, p5: PackedInt64Array, q: int, n: int, j: int) -> int:
	var lhs := dec_side
	var rhs := _big(n)
	if q >= 0:
		var t := q - j
		if t >= 0:
			lhs = _shl(lhs, t)
		else:
			rhs = _shl(rhs, -t)
	else:
		rhs = _mul(rhs, p5)
		var t2 := j - q
		if t2 >= 0:
			rhs = _shl(rhs, t2)
		else:
			lhs = _shl(lhs, -t2)
	return _cmp(lhs, rhs)


# ─── Float writing ──

# Rust `Display` for f64: shortest round-trip digits, plain decimal, no
# exponent ("0.0000001", "1000000000000000000000", "1" for 1.0, "-0").
static func display(x: float) -> String:
	if is_nan(x):
		return "NaN"
	if is_inf(x):
		return "inf" if x > 0.0 else "-inf"
	var neg := bits_of(x) < 0
	if x == 0.0:
		return "-0" if neg else "0"
	var sd := shortest(absf(x), false)
	var d: String = sd[0]
	var k: int = sd[1]
	var n := d.length()
	var s: String
	if k <= 0:
		s = "0." + "0".repeat(-k) + d
	elif k < n:
		s = d.substr(0, k) + "." + d.substr(k)
	else:
		s = d + "0".repeat(k - n)
	return "-" + s if neg else s


# synx-core `format_number`: a whole float in i64 range as an integer, any
# other through `Display`. perf: fractional values are cached — `:calc`
# substitutes the same operands over and over.
static var _fmt_cache: Dictionary = {}

static func format_number(x: float) -> String:
	if fits_int(x):
		return str(int(x))
	if is_nan(x) or is_inf(x):
		return display(x)
	var hit: Variant = _fmt_cache.get(x)
	if hit != null:
		return hit
	var s := display(x)
	if _fmt_cache.size() >= 4096:
		_fmt_cache.clear()
	_fmt_cache[x] = s
	return s


# The ryu layout synx-core writes JSON floats in ("1.0", "0.001", "1e21",
# "1.5e-7"). NaN and infinities are the caller's business (JSON null).
static func ryu(x: float) -> String:
	var neg := bits_of(x) < 0
	if x == 0.0:
		return "-0.0" if neg else "0.0"
	var sd := shortest(absf(x), true)
	var d: String = sd[0]
	var kk: int = sd[1]
	var length := d.length()
	var k := kk - length
	var s: String
	if k >= 0 and kk <= 16:
		s = d + "0".repeat(k) + ".0"
	elif kk > 0 and kk <= 16:
		s = d.substr(0, kk) + "." + d.substr(kk)
	elif kk > -5 and kk <= 0:
		s = "0." + "0".repeat(-kk) + d
	elif length == 1:
		s = d + "e" + str(kk - 1)
	else:
		s = d.substr(0, 1) + "." + d.substr(1) + "e" + str(kk - 1)
	return "-" + s if neg else s


# Rust `{:e}`: shortest digits in exponent form ("1.23456789e5", "1e0").
static func lower_exp(x: float) -> String:
	if is_nan(x):
		return "NaN"
	if is_inf(x):
		return "inf" if x > 0.0 else "-inf"
	var neg := bits_of(x) < 0
	if x == 0.0:
		return "-0e0" if neg else "0e0"
	var sd := shortest(absf(x), false)
	var d: String = sd[0]
	var kk: int = sd[1]
	var s := d.substr(0, 1)
	if d.length() > 1:
		s += "." + d.substr(1)
	s += "e" + str(kk - 1)
	return "-" + s if neg else s


# Rust `{:.prec$}`: exact decimal value rounded half-to-even.
static func fixed(x: float, prec: int) -> String:
	if is_nan(x):
		return "NaN"
	if is_inf(x):
		return "inf" if x > 0.0 else "-inf"
	var neg := bits_of(x) < 0
	var ex := _exact_decimal(absf(x))
	var r := _round_decimal(ex[0], ex[1], prec)
	var digits: String = r
	if digits.length() < prec + 1:
		digits = "0".repeat(prec + 1 - digits.length()) + digits
	var ip := digits.substr(0, digits.length() - prec).lstrip("0")
	if ip.is_empty():
		ip = "0"
	var s := ip
	if prec > 0:
		s += "." + digits.substr(digits.length() - prec)
	return "-" + s if neg else s


# Rust `{:.prec$e}`: exact value rounded half-to-even to prec+1 digits.
static func exp_prec(x: float, prec: int) -> String:
	if is_nan(x):
		return "NaN"
	if is_inf(x):
		return "inf" if x > 0.0 else "-inf"
	var neg := bits_of(x) < 0
	var s: String
	if x == 0.0:
		s = "0"
		if prec > 0:
			s += "." + "0".repeat(prec)
		s += "e0"
		return "-" + s if neg else s
	var ex := _exact_decimal(absf(x))
	var digits: String = ex[0]
	var t: int = ex[1]
	var e10 := digits.length() - 1 - t
	var kept: String
	if digits.length() <= prec + 1:
		kept = digits + "0".repeat(prec + 1 - digits.length())
	else:
		kept = _round_decimal(digits, digits.length() - prec - 1, 0)
		if kept.length() > prec + 1:
			kept = kept.substr(0, prec + 1)
			e10 += 1
	s = kept.substr(0, 1)
	if prec > 0:
		s += "." + kept.substr(1)
	s += "e" + str(e10)
	return "-" + s if neg else s


# Shortest round-trip digits of a positive finite double: [digits, k] with
# value = 0.digits × 10^k, the candidate nearest the exact value.
# `half_even` decides an exact tie between two candidates (ryu rounds to
# even, Rust's Dragon fallback rounds up).
#
# perf: Godot's `var_to_str` is tried first. When its digits (at most 15)
# parse back to x exactly, they are the answer: at 15 digits or fewer a
# decimal is the only one of its length inside the rounding interval, and a
# correctly rounded P-digit print (P <= 15) of a value that has a shorter
# representation is that representation padded with zeros. Anything else —
# 16 or 17 digits, or text that does not round-trip — goes to the exact
# algorithm.
static func shortest(x: float, half_even: bool) -> Array:
	var g := _godot_digits(var_to_str(x))
	if not g.is_empty():
		var d: String = g[0]
		var kk: int = g[1]
		if d.length() <= 15 and _fast_is(d, kk, x):
			return [d, kk]
	return _dragon(x, half_even)


# 0.d × 10^k through the fast path equals x exactly.
static func _fast_is(d: String, k: int, x: float) -> bool:
	var n := d.length()
	if n == 0 or n > 15:
		return false
	var q := k - n
	var p10 := _pow10f()
	var v: float
	if q >= 0 and q <= 22:
		v = float(d.to_int()) * p10[q]
	elif q < 0 and q >= -22:
		v = float(d.to_int()) / p10[-q]
	else:
		return false
	return v == x


# 0.d + one unit in the last place: [digits, k].
static func _increment(d: String, k: int) -> Array:
	var chars := d.to_utf8_buffer()
	var i := chars.size() - 1
	while i >= 0:
		if chars[i] == 57:
			chars[i] = 48
			i -= 1
		else:
			chars[i] += 1
			break
	var s := chars.get_string_from_utf8()
	if i < 0:
		return ["1", k + 1]
	return [s.rstrip("0"), k]


# Digits and k of Godot's `var_to_str` text ("1.2345e+20", "0.001", "100.0").
static func _godot_digits(s: String) -> Array:
	var e_pos := s.find("e")
	if e_pos < 0:
		# perf: the common plain form "123.456".
		var pdot := s.find(".")
		if pdot > 0 and s.unicode_at(0) != 45:
			var pall := s.substr(0, pdot) + s.substr(pdot + 1)
			if pall.is_valid_int():
				var pstripped := pall.lstrip("0")
				var ppoint := pdot - (pall.length() - pstripped.length())
				pstripped = pstripped.rstrip("0")
				if not pstripped.is_empty():
					return [pstripped, ppoint]
	var mant := s if e_pos < 0 else s.substr(0, e_pos)
	var exp10 := 0
	if e_pos >= 0:
		var es := s.substr(e_pos + 1)
		if not es.is_valid_int():
			return []
		exp10 = es.to_int()
	var dot := mant.find(".")
	var ip := mant if dot < 0 else mant.substr(0, dot)
	var fp := "" if dot < 0 else mant.substr(dot + 1)
	if not ip.is_valid_int() or ip.begins_with("-") or ip.begins_with("+"):
		return []
	if not fp.is_empty() and (not fp.is_valid_int() or fp.begins_with("-") or fp.begins_with("+")):
		return []
	var all := ip + fp
	var point := ip.length() + exp10
	var stripped := all.lstrip("0")
	point -= all.length() - stripped.length()
	stripped = stripped.rstrip("0")
	if stripped.is_empty():
		return []
	return [stripped, point]


# Steele & White / Burger & Dybvig free-format shortest digits, exact.
static func _dragon(x: float, half_even: bool) -> Array:
	var b := bits_of(x)
	var biased := (b >> 52) & 0x7FF
	var frac := b & _FRAC_MASK
	var m: int
	var e: int
	if biased == 0:
		m = frac
		e = -1074
	else:
		m = frac | _HIDDEN
		e = biased - 1075
	var even := (m & 1) == 0
	var boundary := frac == 0 and biased > 1
	var r: PackedInt64Array
	var s: PackedInt64Array
	var mp: PackedInt64Array
	var mm: PackedInt64Array
	if e >= 0:
		if not boundary:
			r = _shl(_big(m), e + 1)
			s = _big(2)
			mp = _shl(_big(1), e)
			mm = mp
		else:
			r = _shl(_big(m), e + 2)
			s = _big(4)
			mp = _shl(_big(1), e + 1)
			mm = _shl(_big(1), e)
	else:
		if not boundary:
			r = _big(m * 2)
			s = _shl(_big(1), 1 - e)
			mp = _big(1)
			mm = mp
		else:
			r = _big(m * 4)
			s = _shl(_big(1), 2 - e)
			mp = _big(2)
			mm = _big(1)
	var k := int(ceil(log(x) / 2.302585092994046 - 1e-10))
	if k >= 0:
		s = _mul_pow10(s, k)
	else:
		r = _mul_pow10(r, -k)
		mp = _mul_pow10(mp, -k)
		mm = _mul_pow10(mm, -k)
	while true:
		var hc := _cmp(_add(r, mp), s)
		if hc > 0 or (hc == 0 and even):
			s = _mul_small(s, 10)
			k += 1
		else:
			break
	var digits := PackedByteArray()
	for _guard in 1100:
		r = _mul_small(r, 10)
		mp = _mul_small(mp, 10)
		mm = _mul_small(mm, 10)
		var dgt := 0
		while _cmp(r, s) >= 0:
			r = _sub(r, s)
			dgt += 1
		if digits.is_empty() and dgt == 0:
			# The estimate of k was one too high; this position is a 0.
			k -= 1
			continue
		var c1 := _cmp(r, mm)
		var tc1 := c1 < 0 or (c1 == 0 and even)
		var c2 := _cmp(_add(r, mp), s)
		var tc2 := c2 > 0 or (c2 == 0 and even)
		if not tc1 and not tc2:
			digits.append(48 + dgt)
			continue
		if tc1 and tc2:
			var c3 := _cmp(_shl(r, 1), s)
			if c3 > 0 or (c3 == 0 and (not half_even or (dgt & 1) == 1)):
				dgt += 1
		elif tc2:
			dgt += 1
		if dgt == 10:
			var inc := _increment(digits.get_string_from_utf8() + "9", k)
			return inc
		digits.append(48 + dgt)
		break
	return [digits.get_string_from_utf8().rstrip("0"), k]


# Exact decimal expansion of a non-negative double: [digits, t] with
# value = digits × 10^-t ("" for zero).
static func _exact_decimal(x: float) -> Array:
	if x == 0.0:
		return ["", 0]
	var b := bits_of(x)
	var biased := (b >> 52) & 0x7FF
	var frac := b & _FRAC_MASK
	var m: int
	var e: int
	if biased == 0:
		m = frac
		e = -1074
	else:
		m = frac | _HIDDEN
		e = biased - 1075
	if e >= 0:
		return [_to_decimal(_shl(_big(m), e)), 0]
	return [_to_decimal(_mul(_big(m), _pow5(-e))), -e]


# digits × 10^-t rounded half-to-even to `keep` fractional digits; the
# result is the digit string of value × 10^keep.
static func _round_decimal(digits: String, t: int, keep: int) -> String:
	if t <= keep:
		return digits + "0".repeat(keep - t)
	var drop := t - keep
	if digits.length() < drop + 1:
		digits = "0".repeat(drop + 1 - digits.length()) + digits
	var kept := digits.substr(0, digits.length() - drop)
	var rest := digits.substr(digits.length() - drop)
	var first := rest.unicode_at(0)
	var tail_nonzero := not rest.substr(1).lstrip("0").is_empty()
	var last_odd := (kept.unicode_at(kept.length() - 1) & 1) == 1
	if first > 53 or (first == 53 and (tail_nonzero or last_odd)):
		var chars := kept.to_utf8_buffer()
		var i := chars.size() - 1
		while i >= 0:
			if chars[i] == 57:
				chars[i] = 48
				i -= 1
			else:
				chars[i] += 1
				break
		kept = chars.get_string_from_utf8()
		if i < 0:
			kept = "1" + kept
	return kept


# ─── Big integers (little-endian base 2^24 limbs, no leading zero limb) ──

static func _big(n: int) -> PackedInt64Array:
	var a := PackedInt64Array()
	while n > 0:
		a.append(n & _MASK)
		n = n >> _BITS
	return a


static func _norm(a: PackedInt64Array) -> PackedInt64Array:
	var n := a.size()
	while n > 0 and a[n - 1] == 0:
		n -= 1
	if n != a.size():
		a.resize(n)
	return a


static func _cmp(a: PackedInt64Array, b: PackedInt64Array) -> int:
	var na := a.size()
	var nb := b.size()
	if na != nb:
		return 1 if na > nb else -1
	var i := na - 1
	while i >= 0:
		var x := a[i]
		var y := b[i]
		if x != y:
			return 1 if x > y else -1
		i -= 1
	return 0


static func _add(a: PackedInt64Array, b: PackedInt64Array) -> PackedInt64Array:
	var na := a.size()
	var nb := b.size()
	var n := maxi(na, nb)
	var out := PackedInt64Array()
	out.resize(n)
	var carry := 0
	for i in n:
		var t := carry
		if i < na:
			t += a[i]
		if i < nb:
			t += b[i]
		out[i] = t & _MASK
		carry = t >> _BITS
	if carry > 0:
		out.append(carry)
	return out


# a - b for a >= b.
static func _sub(a: PackedInt64Array, b: PackedInt64Array) -> PackedInt64Array:
	var out := a.duplicate()
	var nb := b.size()
	var borrow := 0
	for i in out.size():
		var t := out[i] - borrow
		if i < nb:
			t -= b[i]
		if t < 0:
			t += _BASE
			borrow = 1
		else:
			borrow = 0
		out[i] = t
		if i >= nb and borrow == 0:
			break
	return _norm(out)


# a × k + add, for 0 <= k, add < 2^31.
static func _mul_small(a: PackedInt64Array, k: int, add: int = 0) -> PackedInt64Array:
	var n := a.size()
	var out := PackedInt64Array()
	out.resize(n)
	var carry := add
	for i in n:
		var t := a[i] * k + carry
		out[i] = t & _MASK
		carry = t >> _BITS
	while carry > 0:
		out.append(carry & _MASK)
		carry = carry >> _BITS
	return _norm(out)


static func _mul(a: PackedInt64Array, b: PackedInt64Array) -> PackedInt64Array:
	var na := a.size()
	var nb := b.size()
	if na == 0 or nb == 0:
		return PackedInt64Array()
	if nb == 1:
		return _mul_small(a, b[0])
	if na == 1:
		return _mul_small(b, a[0])
	var out := PackedInt64Array()
	out.resize(na + nb)
	for i in na:
		var ai := a[i]
		if ai == 0:
			continue
		var carry := 0
		for j in nb:
			var t := out[i + j] + ai * b[j] + carry
			out[i + j] = t & _MASK
			carry = t >> _BITS
		var k := i + nb
		while carry > 0:
			var t2 := out[k] + carry
			out[k] = t2 & _MASK
			carry = t2 >> _BITS
			k += 1
	return _norm(out)


static func _shl(a: PackedInt64Array, bits: int) -> PackedInt64Array:
	if a.is_empty() or bits == 0:
		return a
	var limbs := bits / _BITS
	var rem := bits % _BITS
	var out := PackedInt64Array()
	out.resize(limbs)
	if rem == 0:
		out.append_array(a)
		return out
	var carry := 0
	for i in a.size():
		var t := (a[i] << rem) | carry
		out.append(t & _MASK)
		carry = t >> _BITS
	if carry > 0:
		out.append(carry)
	return out


static func _pow5(k: int) -> PackedInt64Array:
	if _pow5_cache.has(k):
		return _pow5_cache[k]
	var r := _big(1)
	var left := k
	while left >= 13:
		r = _mul_small(r, 1220703125)
		left -= 13
	var tail := 1
	for i in left:
		tail *= 5
	r = _mul_small(r, tail)
	if _pow5_cache.size() < 2048:
		_pow5_cache[k] = r
	return r


static func _mul_pow10(a: PackedInt64Array, k: int) -> PackedInt64Array:
	return _shl(_mul(a, _pow5(k)), k)


static func _from_decimal(s: String) -> PackedInt64Array:
	var n := s.length()
	var first := n % 7
	if first == 0:
		first = 7
	var out := _big(s.substr(0, first).to_int())
	var i := first
	while i < n:
		out = _mul_small(out, 10000000, s.substr(i, 7).to_int())
		i += 7
	return out


static func _to_decimal(a: PackedInt64Array) -> String:
	if a.is_empty():
		return "0"
	var parts := PackedStringArray()
	var cur := a
	while not cur.is_empty():
		var q := PackedInt64Array()
		q.resize(cur.size())
		var rem := 0
		var i := cur.size() - 1
		while i >= 0:
			var t := (rem << _BITS) | cur[i]
			q[i] = t / 10000000
			rem = t % 10000000
			i -= 1
		cur = _norm(q)
		parts.append(str(rem))
	var out := parts[parts.size() - 1]
	var j := parts.size() - 2
	while j >= 0:
		out += parts[j].lpad(7, "0")
		j -= 1
	return out
