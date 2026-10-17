@tool
class_name SynxRegex
extends RefCounted

# `[pattern:…]` with the syntax and the match language of the Rust `regex`
# crate that synx-core uses, on top of Godot's PCRE2.
#
# `translate(p)` parses `p` as regex-syntax does and returns an equivalent
# PCRE2 pattern, or null when the Rust crate would refuse to compile it (the
# constraint is then skipped, as in synx-core). Only whether a match exists
# matters for a constraint, so greediness and capture groups are dropped.
# The dialects differ in many places — `$` is the end of the text in Rust,
# `\v` is a vertical tab, `\e`, `\Q`, `\1`, `\Z`, look-around and a `{` that
# is not a counted repetition are errors, `\d \w \s \b` are Unicode, classes
# nest and support `&&`, `--`, `~~` — so nothing is passed through as is.

const _NEST_LIMIT := 250
const _W_U := "[\\p{Alphabetic}\\p{M}\\p{Nd}\\p{Pc}\\x{200C}\\x{200D}]"
const _W_A := "[0-9A-Za-z_]"
const _ASCII_CLASSES := {
	"alnum": "0-9A-Za-z", "alpha": "A-Za-z", "ascii": "\\x{00}-\\x{7F}", "blank": "\\t ",
	"cntrl": "\\x{00}-\\x{1F}\\x{7F}", "digit": "0-9", "graph": "!-~", "lower": "a-z",
	"print": " -~", "punct": "!-/:-@\\[-`{-~", "space": "\\t\\n\\x{0B}\\f\\r ", "upper": "A-Z",
	"word": "0-9A-Za-z_", "xdigit": "0-9A-Fa-f",
}

var _p: String
var _n: int
var _i: int = 0
var _err: bool = false
var _depth: int = 0
var _names: Dictionary = {}
# Flags in force: case-insensitive, multi-line, dot-all, Unicode, verbose, CRLF.
var _fi := false
var _fm := false
var _fs := false
var _fu := true
var _fx := false
var _fr := false


static func translate(pattern: String) -> Variant:
	var t := SynxRegex.new()
	t._p = pattern
	t._n = pattern.length()
	var out := t._alternation(false)
	if t._err or t._i < t._n:
		return null
	return out


func _peek(off: int = 0) -> int:
	return _p.unicode_at(_i + off) if _i + off < _n else -1


func _fail() -> String:
	_err = true
	return ""


func _skip_space() -> void:
	if not _fx:
		return
	while _i < _n:
		var c := _p.unicode_at(_i)
		if SynxText.is_ws(c):
			_i += 1
		elif c == 35:
			while _i < _n and _p.unicode_at(_i) != 10:
				_i += 1
		else:
			break


# alternation := concat ('|' concat)*  — stops at ')' when inside a group.
func _alternation(in_group: bool) -> String:
	var saved := [_fi, _fm, _fs, _fu, _fx, _fr]
	var parts := PackedStringArray()
	var cur := PackedStringArray()
	var last_atom := -1
	while not _err:
		_skip_space()
		if _i >= _n:
			break
		var c := _p.unicode_at(_i)
		if c == 41:
			if not in_group:
				return _fail()
			break
		if c == 124:
			_i += 1
			parts.append("".join(cur))
			cur = PackedStringArray()
			last_atom = -1
			continue
		if c == 42 or c == 43 or c == 63 or c == 123:
			if last_atom < 0:
				return _fail()
			var q := _quantifier()
			if _err:
				return ""
			cur[last_atom] = "(?:" + cur[last_atom] + ")" + q
			continue
		var atom := _atom()
		if _err:
			return ""
		if atom == "\u0001FLAGS":
			# A bare `(?flags)` group changes the flags for the rest of the
			# alternation; it is not a repeatable item.
			cur.append(_flag_text())
			last_atom = -1
			continue
		cur.append(atom)
		last_atom = cur.size() - 1
	parts.append("".join(cur))
	_fi = saved[0]
	_fm = saved[1]
	_fs = saved[2]
	_fu = saved[3]
	_fx = saved[4]
	_fr = saved[5]
	return "|".join(parts)


func _flag_text() -> String:
	return "(?i)" if _fi else "(?-i)"


# One repetition operator after an item: "*", "+", "?", "{n}", "{n,}", "{n,m}".
func _quantifier() -> String:
	var c := _p.unicode_at(_i)
	_i += 1
	var q := ""
	if c == 42:
		q = "*"
	elif c == 43:
		q = "+"
	elif c == 63:
		q = "?"
	else:
		# Whitespace is allowed around the numbers in any mode.
		_skip_ws()
		var lo := _decimal()
		if lo < 0:
			return _fail()
		_skip_ws()
		if _peek() == 44:
			_i += 1
			_skip_ws()
			if _peek() == 125:
				q = "{%d,}" % lo
			else:
				var hi := _decimal()
				if hi < 0 or hi < lo:
					return _fail()
				q = "{%d,%d}" % [lo, hi]
			_skip_ws()
		else:
			q = "{%d}" % lo
		if _peek() != 125:
			return _fail()
		_i += 1
		if q.length() > 0 and (lo > 65535):
			return _fail()
	_skip_space()
	if _peek() == 63:
		_i += 1
	return q


func _skip_ws() -> void:
	while _i < _n and SynxText.is_ws(_p.unicode_at(_i)):
		_i += 1


func _decimal() -> int:
	var start := _i
	while _i < _n and _p.unicode_at(_i) >= 48 and _p.unicode_at(_i) <= 57:
		_i += 1
	if _i == start or _i - start > 9:
		return -1
	return _p.substr(start, _i - start).to_int()


func _atom() -> String:
	var c := _p.unicode_at(_i)
	if c == 40:
		return _group()
	if c == 91:
		var cls := _class()
		return "" if _err else cls
	if c == 92:
		return _escape(false)
	_i += 1
	if c == 46:
		if not _fu:
			return _fail()
		if _fs:
			return "(?s:.)" if not _fr else "[\\s\\S]"
		return "[^\\n\\r]" if _fr else "[^\\n]"
	if c == 94:
		if not _fm:
			return "\\A"
		return "(?:\\A|(?<=\\n))" if not _fr else "(?:\\A|(?<=\\n)|(?<=\\r)(?!\\n))"
	if c == 36:
		if not _fm:
			return "\\z"
		return "(?:\\z|(?=\\n))" if not _fr else "(?:\\z|(?=\\r)|(?<!\\r)(?=\\n))"
	return _literal(c)


func _literal(c: int) -> String:
	return "\\x{%X}" % c


func _group() -> String:
	_i += 1
	_depth += 1
	if _depth > _NEST_LIMIT:
		return _fail()
	var saved := [_fi, _fm, _fs, _fu, _fx, _fr]
	var prefix_i := ""
	if _peek() == 63:
		_i += 1
		var c := _peek()
		if c == 80 and _peek(1) == 60:
			_i += 2
			if not _group_name():
				return _fail()
		elif c == 60:
			_i += 1
			if not _group_name():
				return _fail()
		else:
			# flags: (?flags) or (?flags:...)
			var neg := false
			var any := false
			var seen := {}
			var dangling := false
			while true:
				var f := _peek()
				if f == -1:
					return _fail()
				if f == 41 or f == 58:
					break
				_i += 1
				if f == 45:
					if neg:
						return _fail()
					neg = true
					dangling = true
					continue
				if seen.has(f):
					return _fail()
				seen[f] = true
				var on := not neg
				dangling = false
				any = true
				match f:
					105:
						_fi = on
					109:
						_fm = on
					115:
						_fs = on
					85:
						pass
					117:
						_fu = on
					120:
						_fx = on
					82:
						_fr = on
					_:
						return _fail()
			# `(?:` needs no flag; `(?)` and a dangling `-` are errors.
			if (not any and _peek() != 58) or dangling:
				return _fail()
			if _peek() == 41:
				_i += 1
				_depth -= 1
				return "\u0001FLAGS"
			_i += 1
			prefix_i = _flag_text()
	var body := _alternation(true)
	if _err or _peek() != 41:
		return _fail()
	_i += 1
	_depth -= 1
	_fi = saved[0]
	_fm = saved[1]
	_fs = saved[2]
	_fu = saved[3]
	_fx = saved[4]
	_fr = saved[5]
	return "(?:" + prefix_i + body + ")"


# A capture-group name, then `>`: a letter or `_`, then word characters,
# `.`, `[` and `]`; each name once.
func _group_name() -> bool:
	var start := _i
	while _i < _n and _p.unicode_at(_i) != 62:
		var c := _p.unicode_at(_i)
		var ch := _p.substr(_i, 1)
		var letter := c == 95 or SynxText.all_alnum_or(ch, false) and not (c >= 48 and c <= 57)
		var ok := letter
		if _i > start:
			ok = letter or SynxText.all_alnum_or(ch, false) or c == 46 or c == 91 or c == 93
		if not ok:
			return false
		_i += 1
	if _i >= _n or _i == start:
		return false
	var name := _p.substr(start, _i - start)
	if _names.has(name):
		return false
	_names[name] = true
	_i += 1
	return true


# An escape outside (in_class = false) or inside a class. Returns PCRE text
# for one item; inside a class a "single character" item (literal or class).
func _escape(in_class: bool) -> String:
	_i += 1
	if _i >= _n:
		return _fail()
	var c := _p.unicode_at(_i)
	_i += 1
	# meta and superfluous escapes (any ASCII character but a letter, a
	# digit, `<` and `>`) are literals
	if c < 128 and not ((c >= 48 and c <= 57) or (c >= 65 and c <= 90) or (c >= 97 and c <= 122)) and c != 60 and c != 62:
		return _literal(c)
	match c:
		97: return _literal(7)
		102: return _literal(12)
		116: return _literal(9)
		110: return _literal(10)
		114: return _literal(13)
		118: return _literal(11)
		120, 117, 85:
			return _hex(c)
		112, 80:
			return _unicode_class(c == 80)
		100:
			return "\\p{Nd}" if _fu else "[0-9]"
		68:
			if not _fu:
				return _fail()
			return "\\P{Nd}"
		115:
			return "[\\p{White_Space}]" if _fu else "[\\t\\n\\x{0B}\\f\\r ]"
		83:
			if not _fu:
				return _fail()
			return "[^\\p{White_Space}]"
		119:
			return _W_U if _fu else _W_A
		87:
			if not _fu:
				return _fail()
			return "[^" + _W_U.substr(1)
	if in_class:
		return _fail()
	var w := _W_U if _fu else _W_A
	match c:
		65: return "\\A"
		122: return "\\z"
		60: return "(?<!%s)(?=%s)" % [w, w]
		62: return "(?<=%s)(?!%s)" % [w, w]
		66: return "(?:(?<=%s)(?=%s)|(?<!%s)(?!%s))" % [w, w, w, w]
		98:
			# `{start}` and friends: only a brace of letters and `-` is a
			# boundary kind; `{2}` is `` repeated.
			if _peek() == 123:
				var j := _i + 1
				while j < _n:
					var k := _p.unicode_at(j)
					if not ((k >= 65 and k <= 90) or (k >= 97 and k <= 122) or k == 45):
						break
					j += 1
				if j < _n and _p.unicode_at(j) == 125 and j > _i + 1:
					var kind := _p.substr(_i + 1, j - _i - 1)
					_i = j + 1
					match kind:
						"start": return "(?<!%s)(?=%s)" % [w, w]
						"end": return "(?<=%s)(?!%s)" % [w, w]
						"start-half": return "(?<!%s)" % w
						"end-half": return "(?!%s)" % w
					return _fail()
			return "(?:(?<=%s)(?!%s)|(?<!%s)(?=%s))" % [w, w, w, w]
	return _fail()


func _hex(kind: int) -> String:
	var digits := 2 if kind == 120 else (4 if kind == 117 else 8)
	var hex := ""
	if _peek() == 123:
		var close := _p.find("}", _i)
		if close < 0:
			return _fail()
		hex = _p.substr(_i + 1, close - _i - 1)
		_i = close + 1
		if hex.is_empty() or hex.length() > 8:
			return _fail()
	else:
		if _i + digits > _n:
			return _fail()
		hex = _p.substr(_i, digits)
		_i += digits
	for k in hex.length():
		var h := hex.unicode_at(k)
		if not ((h >= 48 and h <= 57) or (h >= 65 and h <= 70) or (h >= 97 and h <= 102)):
			return _fail()
	var v := hex.hex_to_int()
	if v > 0x10FFFF or (v >= 0xD800 and v <= 0xDFFF):
		return _fail()
	if not _fu and v > 0x7F:
		return _fail()
	return _literal(v)


func _unicode_class(negated: bool) -> String:
	if not _fu:
		return _fail()
	var name := ""
	if _peek() == 123:
		var close := _p.find("}", _i)
		if close < 0:
			return _fail()
		name = _p.substr(_i + 1, close - _i - 1)
		_i = close + 1
		# `\p{^…}` is not regex-syntax.
		if name.begins_with("^"):
			return _fail()
	else:
		if _i >= _n:
			return _fail()
		name = _p.substr(_i, 1)
		_i += 1
	name = name.strip_edges()
	if name.is_empty():
		return _fail()
	var eq := name.find("=")
	if eq < 0:
		eq = name.find(":")
	if eq >= 0:
		name = name.substr(eq + 1).strip_edges()
	var rx := RegEx.new()
	if rx.compile("\\p{" + name + "}", false) != OK:
		return _fail()
	return ("\\P{%s}" if negated else "\\p{%s}") % name


# ─── Character classes ──

# A class as one PCRE item that matches a single character. Each member is a
# self-contained single-character matcher; members are combined with `|`.
func _class() -> String:
	_i += 1
	_depth += 1
	if _depth > _NEST_LIMIT:
		return _fail()
	var negated := false
	if _peek() == 94:
		negated = true
		_i += 1
	var expr := _class_ops()
	if _err:
		return ""
	if _peek() != 93:
		return _fail()
	_i += 1
	_depth -= 1
	if negated:
		if not _fu:
			return _fail()
		return "(?!" + expr + ")[\\s\\S]"
	return expr


# Set operations, left to right, all of the same precedence.
func _class_ops() -> String:
	var left := _class_union(true)
	while not _err:
		_skip_space()
		var c := _peek()
		var op := ""
		if c == 38 and _peek(1) == 38:
			op = "&&"
		elif c == 45 and _peek(1) == 45:
			op = "--"
		elif c == 126 and _peek(1) == 126:
			op = "~~"
		else:
			break
		_i += 2
		var right := _class_union(false)
		if _err:
			return ""
		match op:
			"&&":
				left = "(?:(?=%s)%s)" % [left, right]
			"--":
				left = "(?:(?!%s)%s)" % [right, left]
			"~~":
				left = "(?:(?!%s)%s|(?!%s)%s)" % [right, left, left, right]
	return left


# Juxtaposed items up to `]` or a set operator.
func _class_union(first: bool) -> String:
	var members := PackedStringArray()
	var start_i := _i
	while not _err:
		_skip_space()
		if _i >= _n:
			return _fail()
		var c := _p.unicode_at(_i)
		if c == 93 and not (first and _i == start_i):
			break
		if (c == 38 and _peek(1) == 38) or (c == 126 and _peek(1) == 126):
			break
		if c == 45 and _peek(1) == 45:
			break
		if c == 91:
			# ASCII class [:name:] or a nested class
			if _peek(1) == 58:
				var close := _p.find(":]", _i + 2)
				if close >= 0:
					var name := _p.substr(_i + 2, close - _i - 2)
					var neg := name.begins_with("^")
					if neg:
						name = name.substr(1)
					if _ASCII_CLASSES.has(name):
						_i = close + 2
						var body: String = _ASCII_CLASSES[name]
						if neg and not _fu:
							return _fail()
						members.append(("[^%s]" if neg else "[%s]") % body)
						continue
			var nested := _class()
			if _err:
				return ""
			members.append(nested)
			continue
		var lo := _class_char()
		if _err:
			return ""
		if lo is String:
			# A class escape cannot open a range.
			if _peek() == 45 and _peek(1) != 93 and _peek(1) != 45 and _peek(1) != -1:
				return _fail()
			members.append(lo)
			continue
		# range?
		if _peek() == 45 and _peek(1) != 93 and _peek(1) != 45 and _peek(1) != -1:
			_i += 1
			_skip_space()
			var hi := _class_char()
			if _err:
				return ""
			if hi is String:
				return _fail()
			if int(hi) < int(lo):
				return _fail()
			members.append("[\\x{%X}-\\x{%X}]" % [lo, hi])
			continue
		members.append("[\\x{%X}]" % lo)
	if members.is_empty():
		return _fail()
	if members.size() == 1:
		return members[0]
	return "(?:" + "|".join(members) + ")"


# One class character: an int code point for a literal, or a String item for
# an escape that stands for a set (\d, \p{L}, …).
func _class_char() -> Variant:
	var c := _p.unicode_at(_i)
	if c != 92:
		_i += 1
		return c
	var item := _escape(true)
	if _err:
		return null
	# A literal escape came back as \x{HEX}: turn it into a code point.
	if item.begins_with("\\x{") and item.ends_with("}") and item.count("{") == 1:
		return item.substr(3, item.length() - 4).hex_to_int()
	return item
