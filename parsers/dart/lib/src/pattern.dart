// `[pattern:…]`: the syntax of the `regex` crate synx-core checks patterns
// with, matched in time linear in the value.
//
// safety: Dart's RegExp backtracks — `^(a|aa)+$` against sixty `a` and a `!`
// did not answer in 20 s — and reads a different dialect: `{` is a literal,
// `(?i)` a syntax error, `\p{L}` the letter `p`. A pattern synx-core does not
// compile is not checked there, so a dialect gap was a value accepted by one
// implementation and refused by the other. This is a Thompson NFA run as a
// Pike VM over code points: every step costs at most the size of the program.
// Character sets lean on RegExp only for a single code point at a time
// (Unicode properties, simple case folding), which cannot backtrack.
import 'pattern_names.dart' as names;

/// Most instructions a compiled pattern may hold; past it the pattern counts
/// as one synx-core does not compile (its own limit is in bytes of program).
const int _maxProgram = 100000;

/// A `[pattern]` compiled once, or null when the `regex` crate would refuse it.
class SynxPattern {
  final List<int> _op;
  final List<int> _a;
  final List<int> _b;
  final List<_Set> _sets;
  final bool _anchoredStart;

  SynxPattern._(this._op, this._a, this._b, this._sets, this._anchoredStart);

  /// Compile [source], or null when the `regex` crate would refuse it.
  static SynxPattern? compile(String source) {
    try {
      final ast = _Parser(source.runes.toList()).parse();
      return _Compiler().build(ast);
    } on _Refused {
      return null;
    }
  }

  /// Whether the pattern matches anywhere in [text] (Rust `Regex::is_match`).
  bool hasMatch(String text) {
    final cps = text.runes.toList();
    final n = _op.length;
    var clist = _SparseSet(n);
    var nlist = _SparseSet(n);
    final stack = <int>[];
    for (var pos = 0; pos <= cps.length; pos++) {
      if (pos == 0 || !_anchoredStart) {
        if (_close(clist, 0, cps, pos, stack)) return true;
      }
      if (clist.length == 0) {
        if (_anchoredStart) return false;
        continue;
      }
      if (pos == cps.length) break;
      final c = cps[pos];
      nlist.clear();
      for (var k = 0; k < clist.length; k++) {
        final pc = clist.dense[k];
        if (_op[pc] == _opChar && _sets[_a[pc]].has(c)) {
          if (_close(nlist, pc + 1, cps, pos + 1, stack)) return true;
        }
      }
      final t = clist;
      clist = nlist;
      nlist = t;
    }
    return false;
  }

  /// Add [start] and everything reachable from it without reading a code
  /// point to [set]; true when that reaches the end of the program.
  bool _close(_SparseSet set, int start, List<int> cps, int pos, List<int> stack) {
    stack.add(start);
    while (stack.isNotEmpty) {
      final pc = stack.removeLast();
      if (set.contains(pc)) continue;
      set.add(pc);
      switch (_op[pc]) {
        case _opMatch:
          stack.clear();
          return true;
        case _opJmp:
          stack.add(_a[pc]);
        case _opSplit:
          stack.add(_b[pc]);
          stack.add(_a[pc]);
        case _opAssert:
          if (_holds(_a[pc], cps, pos)) stack.add(pc + 1);
      }
    }
    return false;
  }

  static bool _holds(int kind, List<int> cps, int pos) {
    final prev = pos > 0 ? cps[pos - 1] : -1;
    final next = pos < cps.length ? cps[pos] : -1;
    switch (kind) {
      case _aStartText:
        return pos == 0;
      case _aEndText:
        return pos == cps.length;
      case _aStartLine:
        return pos == 0 || prev == 0x0A;
      case _aEndLine:
        return pos == cps.length || next == 0x0A;
      case _aStartLineCrlf:
        return pos == 0 || prev == 0x0A || (prev == 0x0D && next != 0x0A);
      case _aEndLineCrlf:
        return pos == cps.length || next == 0x0D || (next == 0x0A && prev != 0x0D);
    }
    final ascii = kind >= _aAsciiOffset;
    final k = ascii ? kind - _aAsciiOffset : kind;
    final wp = prev >= 0 && (ascii ? _isAsciiWord(prev) : _wordSet.has(prev));
    final wn = next >= 0 && (ascii ? _isAsciiWord(next) : _wordSet.has(next));
    switch (k) {
      case _aWord:
        return wp != wn;
      case _aNotWord:
        return wp == wn;
      case _aWordStart:
        return !wp && wn;
      case _aWordEnd:
        return wp && !wn;
      case _aWordStartHalf:
        return !wp;
      case _aWordEndHalf:
        return !wn;
    }
    return false;
  }
}

bool _isAsciiLetter(int c) => (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A);

bool _isAsciiWord(int c) =>
    (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) || c == 0x5F;

const int _opChar = 0, _opSplit = 1, _opJmp = 2, _opAssert = 3, _opMatch = 4;
const int _aStartText = 0, _aEndText = 1, _aStartLine = 2, _aEndLine = 3, _aStartLineCrlf = 4, _aEndLineCrlf = 5;
const int _aWord = 6, _aNotWord = 7, _aWordStart = 8, _aWordEnd = 9, _aWordStartHalf = 10, _aWordEndHalf = 11;

/// Added to a word assertion that reads ASCII word characters only (`(?-u)`).
const int _aAsciiOffset = 100;

class _SparseSet {
  final List<int> dense;
  final List<int> sparse;
  int length = 0;
  _SparseSet(int n)
      : dense = List<int>.filled(n, 0),
        sparse = List<int>.filled(n, 0);
  bool contains(int v) {
    final i = sparse[v];
    return i < length && dense[i] == v;
  }

  void add(int v) {
    sparse[v] = length;
    dense[length++] = v;
  }

  void clear() => length = 0;
}

class _Refused implements Exception {
  const _Refused();
}

// ─── sets of code points ────────────────────────────────────────────────────

/// A set of code points.
abstract class _Set {
  bool has(int c);
}

class _Range extends _Set {
  final int lo, hi;
  _Range(this.lo, this.hi);
  @override
  bool has(int c) => c >= lo && c <= hi;
}

/// One RegExp character class asked about one code point at a time.
class _Js extends _Set {
  final RegExp re;
  final Map<int, bool> _memo = {};
  _Js(this.re);
  @override
  bool has(int c) {
    final known = _memo[c];
    if (known != null) return known;
    final r = re.hasMatch(String.fromCharCode(c));
    if (_memo.length < 4096) _memo[c] = r;
    return r;
  }
}

class _Union extends _Set {
  final List<_Set> parts;
  _Union(this.parts);
  @override
  bool has(int c) {
    for (final p in parts) {
      if (p.has(c)) return true;
    }
    return false;
  }
}

class _Not extends _Set {
  final _Set inner;
  _Not(this.inner);
  @override
  bool has(int c) => !inner.has(c);
}

class _Op extends _Set {
  final _Set a, b;
  final int kind; // 0 intersection, 1 difference, 2 symmetric difference
  _Op(this.a, this.b, this.kind);
  @override
  bool has(int c) {
    final x = a.has(c);
    final y = b.has(c);
    return kind == 0 ? x && y : (kind == 1 ? x && !y : x != y);
  }
}

/// `\w` with Unicode: Alphabetic, marks, decimal numbers, connector punctuation, Join_Control.
final _Set _wordSet = _Js(RegExp(r'^[\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}]$', unicode: true));

String _hex(int c) => '\\u{${c.toRadixString(16)}}';

/// The code points [lo]..[hi], case-folded when [fold] is set (simple case
/// folding, as the `regex` crate folds; ASCII-only folding without Unicode).
_Set _rangeSet(int lo, int hi, bool fold, bool unicode) {
  if (!fold) return _Range(lo, hi);
  if (!unicode) {
    final parts = <_Set>[_Range(lo, hi)];
    final l = lo < 0x41 ? 0x41 : lo, h = hi > 0x5A ? 0x5A : hi;
    if (l <= h) parts.add(_Range(l + 0x20, h + 0x20));
    final l2 = lo < 0x61 ? 0x61 : lo, h2 = hi > 0x7A ? 0x7A : hi;
    if (l2 <= h2) parts.add(_Range(l2 - 0x20, h2 - 0x20));
    return _Union(parts);
  }
  return _Js(RegExp('^[${_hex(lo)}-${_hex(hi)}]\$', unicode: true, caseSensitive: false));
}

_Set _jsClass(String body, bool fold) => _Js(RegExp('^$body\$', unicode: true, caseSensitive: !fold));

// ─── syntax tree ────────────────────────────────────────────────────────────

sealed class _Node {}

class _Empty extends _Node {}

class _Char extends _Node {
  final _Set set;
  _Char(this.set);
}

class _Assert extends _Node {
  final int kind;
  _Assert(this.kind);
}

class _Concat extends _Node {
  final List<_Node> items;
  _Concat(this.items);
}

class _Alt extends _Node {
  final List<_Node> branches;
  _Alt(this.branches);
}

class _Repeat extends _Node {
  final _Node node;
  final int min;
  final int? max;
  _Repeat(this.node, this.min, this.max);
}

/// `(?flags)` on its own: changes the flags, matches nothing, cannot repeat.
class _FlagsOnly extends _Node {}

class _Flags {
  bool i = false, m = false, s = false, u = true, x = false, r = false;
  _Flags copy() => _Flags()
    ..i = i
    ..m = m
    ..s = s
    ..u = u
    ..x = x
    ..r = r;
}

// ─── parser (regex-syntax 0.8 rules) ────────────────────────────────────────

class _Parser {
  final List<int> p;
  int pos = 0;
  _Flags flags = _Flags();
  final Set<String> groupNames = {};

  _Parser(this.p);

  Never fail() => throw const _Refused();

  bool get eof => pos >= p.length;
  int get c => p[pos];

  _Node parse() {
    final node = _alternation();
    if (!eof) fail(); // an unopened `)`
    return node;
  }

  void _skipSpace() {
    if (!flags.x) return;
    while (!eof) {
      final ch = c;
      if (_isSpace(ch)) {
        pos++;
      } else if (ch == 0x23) {
        while (!eof && c != 0x0A) {
          pos++;
        }
      } else {
        break;
      }
    }
  }

  static bool _isSpace(int ch) => ch == 0x20 || (ch >= 0x09 && ch <= 0x0D) || ch == 0x85 || ch == 0xA0 ||
      ch == 0x1680 || (ch >= 0x2000 && ch <= 0x200A) || ch == 0x2028 || ch == 0x2029 || ch == 0x202F ||
      ch == 0x205F || ch == 0x3000;

  _Node _alternation() {
    final branches = <_Node>[_concat()];
    while (!eof && c == 0x7C) {
      pos++;
      branches.add(_concat());
    }
    return branches.length == 1 ? branches.first : _Alt(branches);
  }

  _Node _concat() {
    final items = <_Node>[];
    while (true) {
      _skipSpace();
      if (eof) break;
      final ch = c;
      if (ch == 0x7C || ch == 0x29) break;
      if (ch == 0x2A || ch == 0x2B || ch == 0x3F) {
        pos++;
        _repeatLast(items, ch == 0x2B ? 1 : 0, ch == 0x2A || ch == 0x2B ? null : 1);
        continue;
      }
      if (ch == 0x7B) {
        _counted(items);
        continue;
      }
      items.add(_atom());
    }
    final real = items.where((n) => n is! _FlagsOnly).toList();
    if (real.isEmpty) return _Empty();
    return real.length == 1 ? real.first : _Concat(real);
  }

  void _repeatLast(List<_Node> items, int min, int? max) {
    if (items.isEmpty || items.last is _FlagsOnly) fail(); // repetition operator missing expression
    items.add(_Repeat(items.removeLast(), min, max));
    _skipSpace();
    if (!eof && c == 0x3F) pos++; // lazy: the same set of matches
  }

  void _counted(List<_Node> items) {
    if (items.isEmpty || items.last is _FlagsOnly) fail();
    pos++; // {
    void space() {
      while (!eof && _isSpace(c)) {
        pos++;
      }
    }

    int decimal() {
      space();
      final start = pos;
      var v = 0;
      while (!eof && c >= 0x30 && c <= 0x39) {
        v = v * 10 + (c - 0x30);
        if (v > 0xFFFFFFFF) fail();
        pos++;
      }
      if (pos == start) fail();
      space();
      return v;
    }

    final min = decimal();
    int? max = min;
    if (!eof && c == 0x2C) {
      pos++;
      space();
      if (!eof && c == 0x7D) {
        max = null;
      } else {
        max = decimal();
      }
    }
    if (eof || c != 0x7D) fail();
    pos++;
    if (max != null && min > max) fail();
    items.add(_Repeat(items.removeLast(), min, max));
    _skipSpace();
    if (!eof && c == 0x3F) pos++;
  }

  _Node _atom() {
    final ch = c;
    switch (ch) {
      case 0x28: // (
        return _group();
      case 0x5B: // [
        pos++;
        return _Char(_bracket());
      case 0x5E: // ^
        pos++;
        return _Assert(flags.m ? (flags.r ? _aStartLineCrlf : _aStartLine) : _aStartText);
      case 0x24: // $
        pos++;
        return _Assert(flags.m ? (flags.r ? _aEndLineCrlf : _aEndLine) : _aEndText);
      case 0x2E: // .
        pos++;
        if (!flags.u) fail(); // could match invalid UTF-8
        if (flags.s) return _Char(_Range(0, 0x10FFFF));
        return _Char(flags.r ? _Not(_Union([_Range(0x0A, 0x0A), _Range(0x0D, 0x0D)])) : _Not(_Range(0x0A, 0x0A)));
      case 0x5C: // \
        pos++;
        return _escape();
    }
    pos++;
    return _Char(_literal(ch));
  }

  _Set _literal(int ch) => _rangeSet(ch, ch, flags.i, flags.u);

  _Node _group() {
    pos++; // (
    final saved = flags.copy();
    if (!eof && c == 0x3F) {
      pos++;
      if (eof) fail();
      if (c == 0x50 || c == 0x3C) {
        // (?P<name>…) / (?<name>…)
        if (c == 0x50) {
          pos++;
          if (eof || c != 0x3C) fail();
        }
        pos++;
        final start = pos;
        while (!eof && c != 0x3E) {
          pos++;
        }
        if (eof) fail();
        final name = String.fromCharCodes(p.sublist(start, pos));
        pos++;
        if (!_validGroupName(name) || !groupNames.add(name)) fail();
        return _groupBody(saved);
      }
      if (c == 0x3A) {
        pos++; // (?:…)
        return _groupBody(saved);
      }
      // Flags: `(?flags)` or `(?flags:…)`.
      final next = flags.copy();
      var negate = false;
      var any = false;
      var dangling = false;
      final seen = <int>{};
      while (true) {
        if (eof) fail();
        final f = c;
        if (f == 0x29 || f == 0x3A) break;
        pos++;
        if (f == 0x2D) {
          if (negate) fail();
          negate = true;
          dangling = true;
          continue;
        }
        if (!seen.add(f)) fail();
        final on = !negate;
        switch (f) {
          case 0x69:
            next.i = on;
          case 0x6D:
            next.m = on;
          case 0x73:
            next.s = on;
          case 0x55: // U — swaps greed; the set of matches is the same
            break;
          case 0x75:
            next.u = on;
          case 0x78:
            next.x = on;
          case 0x52:
            next.r = on;
          default:
            fail(); // lookaround, unknown flag
        }
        any = true;
        dangling = false;
      }
      if (!any || dangling) fail();
      if (c == 0x29) {
        pos++;
        flags = next; // to the end of the enclosing group
        return _FlagsOnly();
      }
      pos++; // :
      flags = next;
      return _groupBody(saved);
    }
    return _groupBody(saved);
  }

  _Node _groupBody(_Flags saved) {
    final inner = _alternation();
    if (eof || c != 0x29) fail();
    pos++;
    flags = saved;
    return _Concat([inner]);
  }

  /// regex-syntax `is_capture_char`: `_` or a letter first, then `_`, `.`,
  /// `[`, `]`, letters and digits.
  static bool _validGroupName(String name) {
    if (name.isEmpty) return false;
    var first = true;
    for (final ch in name.runes) {
      final s = String.fromCharCode(ch);
      final ok = ch == 0x5F ||
          (first ? _alphabetic.hasMatch(s) : (ch == 0x2E || ch == 0x5B || ch == 0x5D || _alphanumeric.hasMatch(s)));
      if (!ok) return false;
      first = false;
    }
    return true;
  }

  static final RegExp _alphabetic = RegExp(r'^\p{Alphabetic}$', unicode: true);
  static final RegExp _alphanumeric = RegExp(r'^[\p{Alphabetic}\p{N}]$', unicode: true);

  // ─── escapes ──────────────────────────────────────────────────────────────

  _Node _escape() {
    if (eof) fail();
    final e = c;
    switch (e) {
      case 0x62: // b
        pos++;
        // `\b{start}` and the like; `\b{2}` is a repeated boundary.
        if (pos + 1 < p.length && c == 0x7B && (_isAsciiLetter(p[pos + 1]) || p[pos + 1] == 0x2D)) {
          final save = pos;
          pos++;
          final start = pos;
          while (!eof && c != 0x7D) {
            pos++;
          }
          if (eof) fail();
          final name = String.fromCharCodes(p.sublist(start, pos));
          pos++;
          final kind = switch (name) {
            'start' => _aWordStart,
            'end' => _aWordEnd,
            'start-half' => _aWordStartHalf,
            'end-half' => _aWordEndHalf,
            _ => -1,
          };
          if (kind < 0) {
            pos = save;
            fail();
          }
          return _Assert(_word(kind));
        }
        return _Assert(_word(_aWord));
      case 0x42: // B
        pos++;
        return _Assert(_word(_aNotWord));
      case 0x41: // A
        pos++;
        return _Assert(_aStartText);
      case 0x7A: // z
        pos++;
        return _Assert(_aEndText);
      case 0x3C: // <
        pos++;
        return _Assert(_word(_aWordStart));
      case 0x3E: // >
        pos++;
        return _Assert(_word(_aWordEnd));
    }
    return _Char(_escapeSet(inClass: false));
  }

  int _word(int kind) => flags.u ? kind : kind + _aAsciiOffset;

  /// An escape that stands for code points, at `\` + 1. Literal escapes give
  /// a one-point set; [literalOut] receives the code point when there is one.
  _Set _escapeSet({required bool inClass, List<int>? literalOut}) {
    final e = c;
    pos++;
    switch (e) {
      case 0x64: // d
      case 0x44:
        return _perl(0, e == 0x44);
      case 0x73: // s
      case 0x53:
        return _perl(1, e == 0x53);
      case 0x77: // w
      case 0x57:
        return _perl(2, e == 0x57);
      case 0x70: // p
      case 0x50:
        return _unicodeClass(e == 0x50);
    }
    final cp = _literalEscape(e);
    literalOut?.add(cp);
    return _literal(cp);
  }

  /// The code point of a literal escape whose letter [e] has been read.
  int _literalEscape(int e) {
    switch (e) {
      case 0x6E:
        return 0x0A;
      case 0x74:
        return 0x09;
      case 0x72:
        return 0x0D;
      case 0x66:
        return 0x0C;
      case 0x76:
        return 0x0B;
      case 0x61:
        return 0x07;
      case 0x78: // x
        return _hexEscape(2);
      case 0x75: // u
        return _hexEscape(4);
      case 0x55: // U
        return _hexEscape(8);
    }
    // Any ASCII character that is not a letter or a digit may be escaped.
    if (e < 0x80 && !((e >= 0x30 && e <= 0x39) || (e >= 0x41 && e <= 0x5A) || (e >= 0x61 && e <= 0x7A))) {
      return e;
    }
    fail();
  }

  int _hexEscape(int digits) {
    var v = 0;
    if (!eof && c == 0x7B) {
      pos++;
      final start = pos;
      while (!eof && c != 0x7D) {
        final d = _hexDigit(c);
        if (d < 0) fail();
        v = v * 16 + d;
        if (v > 0x10FFFF) fail();
        pos++;
      }
      if (eof || pos == start) fail();
      pos++;
    } else {
      for (var k = 0; k < digits; k++) {
        if (eof) fail();
        final d = _hexDigit(c);
        if (d < 0) fail();
        v = v * 16 + d;
        pos++;
      }
    }
    if (v > 0x10FFFF || (v >= 0xD800 && v <= 0xDFFF)) fail();
    // Without Unicode a byte past ASCII is not a character.
    if (!flags.u && v > 0x7F) fail();
    return v;
  }

  static int _hexDigit(int ch) {
    if (ch >= 0x30 && ch <= 0x39) return ch - 0x30;
    if (ch >= 0x61 && ch <= 0x66) return ch - 0x61 + 10;
    if (ch >= 0x41 && ch <= 0x46) return ch - 0x41 + 10;
    return -1;
  }

  /// `\d` (0), `\s` (1), `\w` (2), negated when [neg].
  _Set _perl(int kind, bool neg) {
    _Set set;
    if (flags.u) {
      set = switch (kind) {
        0 => _jsClass(r'\p{Nd}', flags.i),
        1 => _jsClass(r'\p{White_Space}', flags.i),
        _ => _wordSet,
      };
    } else {
      set = switch (kind) {
        0 => _Range(0x30, 0x39),
        1 => _Union([_Range(0x09, 0x0D), _Range(0x20, 0x20)]),
        _ => _Union([_Range(0x30, 0x39), _Range(0x41, 0x5A), _Range(0x61, 0x7A), _Range(0x5F, 0x5F)]),
      };
    }
    if (neg) {
      // A negated class without Unicode could match invalid UTF-8.
      if (!flags.u) fail();
      return _Not(set);
    }
    return set;
  }

  /// `\p…` / `\P…`, at the character after the letter.
  _Set _unicodeClass(bool negated) {
    if (!flags.u) fail();
    if (eof) fail();
    String? name;
    String? value;
    var neg = negated;
    if (c == 0x7B) {
      pos++;
      final start = pos;
      while (!eof && c != 0x7D) {
        pos++;
      }
      if (eof) fail();
      final body = String.fromCharCodes(p.sublist(start, pos));
      pos++;
      final ne = body.indexOf('!=');
      final colon = body.indexOf(':');
      final eq = body.indexOf('=');
      if (ne >= 0) {
        name = body.substring(0, ne);
        value = body.substring(ne + 2);
        neg = !neg;
      } else if (colon >= 0) {
        name = body.substring(0, colon);
        value = body.substring(colon + 1);
      } else if (eq >= 0) {
        name = body.substring(0, eq);
        value = body.substring(eq + 1);
      } else {
        name = body;
      }
    } else {
      if (c == 0x5C) fail();
      name = String.fromCharCode(c);
      pos++;
    }
    final js = value == null ? _binaryQuery(name) : _valueQuery(name, value);
    if (js == null) fail();
    _Set set;
    try {
      set = _jsClass(js, flags.i);
    } on FormatException {
      fail(); // a property this platform's tables do not have
    }
    return neg ? _Not(set) : set;
  }

  // ─── bracketed classes ───────────────────────────────────────────────────

  /// `[…]`, after the `[`.
  _Set _bracket() {
    var negated = false;
    if (!eof && c == 0x5E) {
      pos++;
      negated = true;
    }
    final set = _classOps(true);
    if (eof || c != 0x5D) fail();
    pos++;
    if (negated) {
      if (!flags.u) fail(); // could match invalid UTF-8
      return _Not(set);
    }
    return set;
  }

  void _classSpace() {
    if (!flags.x) return;
    while (!eof && _isSpace(c)) {
      pos++;
    }
  }

  /// Unions joined by `&&`, `--` and `~~`, left to right.
  _Set _classOps(bool first) {
    var acc = _classUnion(first);
    while (true) {
      _classSpace();
      if (pos + 1 < p.length) {
        final a = c, b = p[pos + 1];
        final kind = a == 0x26 && b == 0x26 ? 0 : (a == 0x2D && b == 0x2D ? 1 : (a == 0x7E && b == 0x7E ? 2 : -1));
        if (kind >= 0) {
          pos += 2;
          acc = _Op(acc, _classUnion(false), kind);
          continue;
        }
      }
      return acc;
    }
  }

  bool _atOp() {
    if (pos + 1 >= p.length) return false;
    final a = c, b = p[pos + 1];
    return (a == 0x26 && b == 0x26) || (a == 0x2D && b == 0x2D) || (a == 0x7E && b == 0x7E);
  }

  _Set _classUnion(bool first) {
    final parts = <_Set>[];
    var leading = first;
    while (true) {
      _classSpace();
      if (eof) fail();
      if (c == 0x5D && !leading) break;
      // A `-` that opens the class is itself; any other operator may stand
      // first, with an empty left side.
      if (leading && c == 0x2D) {
        pos++;
        parts.add(_rangeSet(0x2D, 0x2D, flags.i, flags.u));
        leading = false;
        continue;
      }
      if (_atOp()) break;
      leading = false;
      // One item: a nested class, an ASCII class, an escape or a character,
      // and a range when a `-` and another character follow.
      if (c == 0x5B) {
        final ascii = _asciiClass();
        if (ascii != null) {
          parts.add(ascii);
          continue;
        }
        pos++;
        parts.add(_bracket());
        continue;
      }
      int? lo;
      if (c == 0x5C) {
        pos++;
        if (eof) fail();
        final e = c;
        if (e == 0x62 || e == 0x42 || e == 0x41 || e == 0x7A || e == 0x3C || e == 0x3E) fail(); // assertions
        final out = <int>[];
        final set = _escapeSet(inClass: true, literalOut: out);
        if (out.isEmpty) {
          // A class escape cannot start a range.
          _classSpace();
          if (!eof && c == 0x2D && pos + 1 < p.length && p[pos + 1] != 0x5D && p[pos + 1] != 0x2D) fail();
          parts.add(set);
          continue;
        }
        lo = out.first;
      } else {
        lo = c;
        pos++;
      }
      _classSpace();
      if (!eof && c == 0x2D && pos + 1 < p.length && p[pos + 1] != 0x5D && p[pos + 1] != 0x2D) {
        pos++;
        _classSpace();
        int hi;
        if (c == 0x5B) fail();
        if (c == 0x5C) {
          pos++;
          if (eof) fail();
          final out = <int>[];
          _escapeSet(inClass: true, literalOut: out);
          if (out.isEmpty) fail(); // a class escape cannot end a range
          hi = out.first;
        } else {
          hi = c;
          pos++;
        }
        if (lo > hi) fail();
        parts.add(_rangeSet(lo, hi, flags.i, flags.u));
      } else {
        parts.add(_rangeSet(lo, lo, flags.i, flags.u));
      }
    }
    return _Union(parts);
  }

  /// `[:name:]` / `[:^name:]` at a `[`, or null (and nothing read) when the
  /// text there is not one — then it is a nested class.
  _Set? _asciiClass() {
    if (pos + 1 >= p.length || p[pos + 1] != 0x3A) return null;
    var k = pos + 2;
    var neg = false;
    if (k < p.length && p[k] == 0x5E) {
      neg = true;
      k++;
    }
    final start = k;
    while (k < p.length && p[k] != 0x3A) {
      k++;
    }
    if (k + 1 >= p.length || p[k + 1] != 0x5D) return null;
    final name = String.fromCharCodes(p.sublist(start, k));
    final ranges = _asciiRanges[name];
    if (ranges == null) return null;
    pos = k + 2;
    final parts = <_Set>[for (var r = 0; r < ranges.length; r += 2) _rangeSet(ranges[r], ranges[r + 1], flags.i, flags.u)];
    final set = _Union(parts);
    if (neg) {
      if (!flags.u) fail();
      return _Not(set);
    }
    return set;
  }

  static const Map<String, List<int>> _asciiRanges = {
    'alnum': [0x30, 0x39, 0x41, 0x5A, 0x61, 0x7A],
    'alpha': [0x41, 0x5A, 0x61, 0x7A],
    'ascii': [0x00, 0x7F],
    'blank': [0x09, 0x09, 0x20, 0x20],
    'cntrl': [0x00, 0x1F, 0x7F, 0x7F],
    'digit': [0x30, 0x39],
    'graph': [0x21, 0x7E],
    'lower': [0x61, 0x7A],
    'print': [0x20, 0x7E],
    'punct': [0x21, 0x2F, 0x3A, 0x40, 0x5B, 0x60, 0x7B, 0x7E],
    'space': [0x09, 0x0D, 0x20, 0x20],
    'upper': [0x41, 0x5A],
    'word': [0x30, 0x39, 0x41, 0x5A, 0x5F, 0x5F, 0x61, 0x7A],
    'xdigit': [0x30, 0x39, 0x41, 0x46, 0x61, 0x66],
  };
}

// ─── Unicode property names (regex-syntax `ClassQuery::canonicalize`) ──────

Map<String, String> _table(String packed) {
  final out = <String, String>{};
  for (final pair in packed.split(';')) {
    final eq = pair.indexOf('=');
    out[pair.substring(0, eq)] = pair.substring(eq + 1);
  }
  return out;
}

final Map<String, String> _props = _table(names.propertyNames);
final Map<String, String> _gencats = _table(names.generalCategories);
final Map<String, String> _scripts = _table(names.scripts);
final Set<String> _bools = names.booleanProperties.split(';').toSet();

/// UAX44-LM3: no `is` prefix, no space, `_` or `-`, ASCII lower case.
String _normalize(String name) {
  var s = name;
  if (s.length >= 2 && s.substring(0, 2).toLowerCase() == 'is') s = s.substring(2);
  final sb = StringBuffer();
  for (final ch in s.codeUnits) {
    if (ch == 0x20 || ch == 0x5F || ch == 0x2D || ch > 0x7F) continue;
    sb.writeCharCode(ch >= 0x41 && ch <= 0x5A ? ch + 0x20 : ch);
  }
  final out = sb.toString();
  // `isc` is an alias of the Other category, not `c` with the prefix dropped.
  return name.length >= 2 && name.substring(0, 2).toLowerCase() == 'is' && out == 'c' ? 'isc' : out;
}

String? _gencatClass(String canon) => switch (canon) {
      'Any' || 'Assigned' || 'ASCII' => '\\p{$canon}',
      _ => '\\p{General_Category=$canon}',
    };

String? _gencat(String norm) => switch (norm) {
      'any' => 'Any',
      'assigned' => 'Assigned',
      'ascii' => 'ASCII',
      _ => _gencats[norm],
    };

/// `\pL`, `\p{Greek}`, `\p{Alphabetic}`: a RegExp class body, or null.
String? _binaryQuery(String name) {
  final norm = _normalize(name);
  if (norm != 'cf' && norm != 'sc' && norm != 'lc') {
    final prop = _props[norm];
    if (prop != null) {
      if (prop == 'White_Space' || _bools.contains(prop)) return '\\p{$prop}';
      if (prop == 'Decimal_Number') return r'\p{Nd}';
      return null; // a property with values, named alone
    }
  }
  final gc = _gencat(norm);
  if (gc != null) return _gencatClass(gc);
  final sc = _scripts[norm];
  if (sc != null) return '\\p{Script=$sc}';
  return null;
}

/// `\p{name=value}`: a RegExp class body, or null.
String? _valueQuery(String name, String value) {
  final prop = _props[_normalize(name)];
  final v = _normalize(value);
  switch (prop) {
    case 'General_Category':
      final gc = _gencat(v);
      return gc == null ? null : _gencatClass(gc);
    case 'Script':
      final sc = _scripts[v];
      return sc == null ? null : '\\p{Script=$sc}';
    case 'Script_Extensions':
      final sc = _scripts[v];
      return sc == null ? null : '\\p{Script_Extensions=$sc}';
  }
  return null; // Age and the segmentation properties: no RegExp counterpart
}

// ─── compiler ───────────────────────────────────────────────────────────────

class _Compiler {
  final List<int> op = [], a = [], b = [];
  final List<_Set> sets = [];
  final Map<_Set, int> setIds = Map.identity();

  int _emit(int o, [int x = 0, int y = 0]) {
    if (op.length >= _maxProgram) throw const _Refused();
    op.add(o);
    a.add(x);
    b.add(y);
    return op.length - 1;
  }

  SynxPattern build(_Node root) {
    _node(root);
    _emit(_opMatch);
    return SynxPattern._(op, a, b, sets, _startsAnchored(root));
  }

  static bool _startsAnchored(_Node n) => switch (n) {
        _Assert(kind: var k) => k == _aStartText,
        _Concat(items: var items) => items.isNotEmpty && _startsAnchored(items.first),
        _Alt(branches: var bs) => bs.every(_startsAnchored),
        _Repeat(node: var inner, min: var min) => min > 0 && _startsAnchored(inner),
        _ => false,
      };

  void _node(_Node n) {
    switch (n) {
      case _Empty() || _FlagsOnly():
        return;
      case _Char(set: var s):
        _emit(_opChar, setIds.putIfAbsent(s, () {
          sets.add(s);
          return sets.length - 1;
        }));
      case _Assert(kind: var k):
        _emit(_opAssert, k);
      case _Concat(items: var items):
        for (final i in items) {
          _node(i);
        }
      case _Alt(branches: var bs):
        final jumps = <int>[];
        for (var k = 0; k < bs.length; k++) {
          if (k < bs.length - 1) {
            final split = _emit(_opSplit);
            a[split] = op.length;
            _node(bs[k]);
            jumps.add(_emit(_opJmp));
            b[split] = op.length;
          } else {
            _node(bs[k]);
          }
        }
        for (final j in jumps) {
          a[j] = op.length;
        }
      case _Repeat(node: var inner, min: var min, max: var max):
        for (var k = 0; k < min; k++) {
          _node(inner);
        }
        if (max == null) {
          // inner*
          final split = _emit(_opSplit);
          a[split] = op.length;
          _node(inner);
          _emit(_opJmp, split);
          b[split] = op.length;
        } else {
          // (inner (inner …)?)? — each optional copy may stop the rest.
          final exits = <int>[];
          for (var k = min; k < max; k++) {
            final split = _emit(_opSplit);
            a[split] = op.length;
            exits.add(split);
            _node(inner);
          }
          for (final s in exits) {
            b[s] = op.length;
          }
        }
    }
  }
}
