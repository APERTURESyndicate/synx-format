@tool
class_name SynxText
extends RefCounted

# Text helpers with the exact semantics of synx-core.
#
# SYNX §4 measures and trims *Unicode* whitespace (Rust `char::is_whitespace`),
# and every size cap is in UTF-8 bytes. Godot's `strip_edges` strips every
# character up to U+0020 and nothing above it, so it is not used on document
# text.


# Rust `char::is_whitespace` (Unicode White_Space).
static func is_ws(c: int) -> bool:
	if c <= 32:
		return c == 32 or (c >= 9 and c <= 13)
	if c < 0x85:
		return false
	return c == 0x85 or c == 0xA0 or c == 0x1680 or (c >= 0x2000 and c <= 0x200A) \
		or c == 0x2028 or c == 0x2029 or c == 0x202F or c == 0x205F or c == 0x3000


# UTF-8 width of one code point.
static func utf8_width(c: int) -> int:
	if c < 0x80:
		return 1
	if c < 0x800:
		return 2
	if c < 0x10000:
		return 3
	return 4


# UTF-8 byte length of `s`.
static func utf8_len(s: String) -> int:
	return s.to_utf8_buffer().size()


# Strict UTF-8 decoding that keeps a leading U+FEFF (Godot's
# `get_string_from_utf8` drops it); null when the bytes are not valid UTF-8.
static func utf8_decode(bytes: PackedByteArray) -> Variant:
	var boms := 0
	var n := bytes.size()
	while n >= 3 * boms + 3 and bytes[3 * boms] == 0xEF and bytes[3 * boms + 1] == 0xBB and bytes[3 * boms + 2] == 0xBF:
		boms += 1
	var rest := bytes.slice(3 * boms) if boms > 0 else bytes
	var s := rest.get_string_from_utf8()
	if s.to_utf8_buffer() != rest:
		return null
	if boms > 0:
		s = String.chr(0xFEFF).repeat(boms) + s
	return s


# Rust `str::trim`.
static func trim(s: String) -> String:
	var n := s.length()
	if n == 0:
		return s
	var a := 0
	while a < n and is_ws(s.unicode_at(a)):
		a += 1
	if a == n:
		return ""
	var b := n
	while is_ws(s.unicode_at(b - 1)):
		b -= 1
	if a == 0 and b == n:
		return s
	return s.substr(a, b - a)


# Rust `str::trim_end`.
static func trim_end(s: String) -> String:
	var b := s.length()
	while b > 0 and is_ws(s.unicode_at(b - 1)):
		b -= 1
	if b == s.length():
		return s
	return s.substr(0, b)


# Rust `str::trim_start`.
static func trim_start(s: String) -> String:
	var n := s.length()
	var a := 0
	while a < n and is_ws(s.unicode_at(a)):
		a += 1
	if a == 0:
		return s
	return s.substr(a)


# The longest prefix of `s` that fits in `max_bytes` of UTF-8.
static func utf8_prefix(s: String, max_bytes: int) -> String:
	if max_bytes <= 0:
		return ""
	if s.length() * 4 <= max_bytes:
		return s
	var bytes := 0
	var n := s.length()
	for i in n:
		var w := utf8_width(s.unicode_at(i))
		if bytes + w > max_bytes:
			return s.substr(0, i)
		bytes += w
	return s


# Rust `char::is_alphanumeric` for every character of `s` (true for "").
static var _rx_alnum: RegEx = null

static func all_alnum_or(s: String, allow_dot: bool) -> bool:
	var n := s.length()
	var ascii := true
	for i in n:
		var c := s.unicode_at(i)
		if (c >= 48 and c <= 57) or (c >= 65 and c <= 90) or (c >= 97 and c <= 122) or c == 95:
			continue
		if c == 46 and allow_dot:
			continue
		if c < 128:
			return false
		ascii = false
	if ascii:
		return true
	if _rx_alnum == null:
		_rx_alnum = RegEx.new()
		_rx_alnum.compile("^[\\p{Alphabetic}\\p{N}_.]*$")
	if not allow_dot and s.contains("."):
		return false
	return _rx_alnum.search(s) != null


# Byte-order comparison of two strings (UTF-8 byte order is code point order).
static func less(a: String, b: String) -> bool:
	return a < b
