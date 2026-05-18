// SYNX text-to-tree parser. Mirrors crates/synx-core/src/parser.rs.
//
// Byte-level scanning over `Uint8List` (the UTF-8 encoding of the input) keeps
// performance close to the Rust/Go ports while staying in pure Dart.
import 'dart:convert';
import 'dart:math';
import 'dart:typed_data';

import 'meta.dart';
import 'value.dart';

// Resource caps (parity with parser.rs).
const int maxInputBytes = 16 * 1024 * 1024;
const int maxLineStarts = 2000000;
const int maxNestingDepth = 128;
const int maxMultilineBytes = 1024 * 1024;
const int maxListItems = 1 << 20;
const int maxIncludes = 4096;
const int maxEnumParts = 4096;
const int maxMarkerSegments = 512;

/// Truncate `text` to a UTF-8-safe prefix bounded by [maxInputBytes].
String clampText(String text) {
  final bytes = utf8.encode(text);
  if (bytes.length <= maxInputBytes) return text;
  var end = maxInputBytes;
  while (end > 0 && (bytes[end] & 0xC0) == 0x80) {
    end--;
  }
  return utf8.decode(bytes.sublist(0, end));
}

/// Parse a SYNX text into a [SynxParseResult].
SynxParseResult parse(String rawText) {
  Uint8List bytes = Uint8List.fromList(utf8.encode(clampText(rawText)));

  // Bound the number of indexed newlines.
  {
    final maxNl = maxLineStarts - 1;
    var seen = 0;
    var scan = 0;
    while (scan < bytes.length) {
      if (bytes[scan] == 0x0A) {
        if (seen >= maxNl) {
          bytes = bytes.sublist(0, scan);
          break;
        }
        seen++;
      }
      scan++;
    }
  }

  final lineStarts = <int>[0];
  for (var scan = 0; scan < bytes.length; scan++) {
    if (bytes[scan] == 0x0A) lineStarts.add(scan + 1);
  }
  final lineCount = lineStarts.length;

  final result = SynxParseResult();
  final rootObj = (result.root as SynxObj).map;
  final stack = <_StackFrame>[_StackFrame(-1, _StackKind.root, '', 0)];

  _BlockState? block;
  _ListState? list;
  var inBlockComment = false;

  var i = 0;
  while (i < lineCount) {
    final raw = _lineBytes(bytes, lineStarts, i);
    final rawStr = utf8.decode(raw, allowMalformed: true);
    final t = rawStr.trim();

    switch (t) {
      case '!active':
        result.mode = SynxMode.active;
        i++;
        continue;
      case '!lock':
        result.locked = true;
        i++;
        continue;
      case '!tool':
        result.tool = true;
        i++;
        continue;
      case '!schema':
        result.schema = true;
        i++;
        continue;
      case '!llm':
        result.llm = true;
        i++;
        continue;
    }
    if (t.startsWith('!include ')) {
      if (result.includes.length < maxIncludes) {
        final rest = t.substring(9).trim();
        var path = rest;
        var alias = '';
        final ws = _firstWs(rest);
        if (ws >= 0) {
          path = rest.substring(0, ws);
          alias = rest.substring(ws).trim();
        }
        if (alias.isEmpty) {
          var base = path;
          final slash = max(base.lastIndexOf('/'), base.lastIndexOf('\\'));
          if (slash >= 0) base = base.substring(slash + 1);
          if (base.endsWith('.synx') || base.endsWith('.SYNX')) {
            base = base.substring(0, base.length - 5);
          }
          alias = base;
        }
        result.includes.add(SynxIncludeDirective(path, alias));
      }
      i++;
      continue;
    }
    if (t.startsWith('!use ')) {
      final rest = t.substring(5).trim();
      if (rest.isNotEmpty && rest[0] == '@') {
        var pkg = rest;
        var alias = '';
        final asPos = rest.indexOf(' as ');
        if (asPos >= 0) {
          pkg = rest.substring(0, asPos).trim();
          alias = rest.substring(asPos + 4).trim();
        }
        if (alias.isEmpty) {
          final slash = pkg.lastIndexOf('/');
          alias = slash >= 0 ? pkg.substring(slash + 1) : pkg;
        }
        if (pkg.isNotEmpty) {
          result.uses.add(SynxUseDirective(pkg, alias));
        }
      }
      i++;
      continue;
    }
    if (t.startsWith('#!mode:')) {
      final declared = t.substring(7).trim();
      result.mode = declared == 'active' ? SynxMode.active : SynxMode.static_;
      i++;
      continue;
    }

    if (t == '###') {
      inBlockComment = !inBlockComment;
      i++;
      continue;
    }
    if (inBlockComment) {
      i++;
      continue;
    }
    if (t.isEmpty || t.startsWith('#') || t.startsWith('//')) {
      i++;
      continue;
    }

    final indent = _indentOf(raw);

    // Continue multiline block
    if (block != null) {
      if (indent > block.indent) {
        if (block.content.length < maxMultilineBytes) {
          if (block.content.isNotEmpty) block.content.write('\n');
          final room = maxMultilineBytes - block.content.length;
          final n = t.length < room ? t.length : room;
          block.content.write(t.substring(0, n));
        }
        i++;
        continue;
      }
      _insertValue(rootObj, stack, block.stackIdx, block.key,
          synxString(block.content.toString()));
      block = null;
    }

    // List items
    if (t.startsWith('- ')) {
      if (list != null && indent > list.indent) {
        while (stack.length > 1 &&
            stack.last.kind == _StackKind.listItem &&
            stack.last.indent >= indent) {
          stack.removeLast();
        }
        final valStr = _stripComment(t.substring(2).trim());

        var nested = false;
        for (var peek = i + 1; peek < lineCount; peek++) {
          final pl = _lineBytes(bytes, lineStarts, peek);
          final pt = utf8.decode(pl, allowMalformed: true).trim();
          if (pt.isEmpty) continue;
          final pi = _indentOf(pl);
          if (pi > indent &&
              !pt.startsWith('- ') &&
              !pt.startsWith('#') &&
              !pt.startsWith('//')) {
            nested = true;
          }
          break;
        }

        final listKey = list.key;
        final listStackIdx = list.stackIdx;
        var newIdx = -1;
        _mutateArray(rootObj, stack, listStackIdx, listKey, (arr) {
          if (arr.length >= maxListItems) return;
          if (nested) {
            final item = SynxObject();
            final parsed = _parseLine(valStr);
            if (parsed != null) {
              SynxValue v;
              if (parsed.typeHint != null) {
                v = _castTyped(parsed.value, parsed.typeHint!);
              } else if (parsed.value.isEmpty) {
                v = synxObject();
              } else {
                v = _castValue(parsed.value);
              }
              item.set(parsed.key, v);
            } else {
              item.set('_value', _castValue(valStr));
            }
            newIdx = arr.length;
            arr.add(synxObject(item));
          } else {
            arr.add(_castValue(valStr));
          }
        });
        if (newIdx >= 0 && stack.length < maxNestingDepth) {
          stack.add(_StackFrame(indent, _StackKind.listItem, listKey, newIdx));
        }
        i++;
        continue;
      }
    } else if (list != null && indent <= list.indent) {
      list = null;
      while (stack.length > 1 &&
          stack.last.kind == _StackKind.listItem &&
          stack.last.indent >= indent) {
        stack.removeLast();
      }
    }

    final parsed = _parseLine(t);
    if (parsed == null) {
      i++;
      continue;
    }
    if (parsed.key == '__proto__' ||
        parsed.key == 'constructor' ||
        parsed.key == 'prototype') {
      i++;
      continue;
    }
    while (stack.length > 1 && stack.last.indent >= indent) {
      stack.removeLast();
    }
    final parentIdx = stack.length - 1;

    if (result.mode == SynxMode.active &&
        (parsed.markers.isNotEmpty ||
            parsed.constraints != null ||
            parsed.typeHint != null)) {
      final path = _buildPath(stack);
      final meta = SynxMeta()
        ..markers = parsed.markers
        ..args = parsed.markerArgs
        ..typeHint = parsed.typeHint
        ..constraints = parsed.constraints;
      result.metadata.putIfAbsent(path, () => {})[parsed.key] = meta;
    }

    final isBlock = parsed.value == '|';
    final isListMarker = parsed.markers
        .any((m) => m == 'random' || m == 'unique' || m == 'geo' || m == 'join');

    if (isBlock) {
      _insertValue(rootObj, stack, parentIdx, parsed.key, synxString(''));
      block = _BlockState(indent, parsed.key, parentIdx);
    } else if (isListMarker && parsed.value.isEmpty) {
      _insertValue(rootObj, stack, parentIdx, parsed.key, synxArray());
      list = _ListState(indent, parsed.key, parentIdx);
    } else if (parsed.value.isEmpty) {
      var becameList = false;
      for (var peek = i + 1; peek < lineCount; peek++) {
        final pl = _lineBytes(bytes, lineStarts, peek);
        final pt = utf8.decode(pl, allowMalformed: true).trim();
        if (pt.isEmpty) continue;
        if (pt.startsWith('- ')) {
          _insertValue(rootObj, stack, parentIdx, parsed.key, synxArray());
          list = _ListState(indent, parsed.key, parentIdx);
          becameList = true;
        }
        break;
      }
      if (!becameList) {
        _insertValue(rootObj, stack, parentIdx, parsed.key, synxObject());
        if (stack.length < maxNestingDepth) {
          stack.add(_StackFrame(indent, _StackKind.key, parsed.key, 0));
        }
      }
    } else {
      final v = parsed.typeHint != null
          ? _castTyped(parsed.value, parsed.typeHint!)
          : _castValue(parsed.value);
      _insertValue(rootObj, stack, parentIdx, parsed.key, v);
    }
    i++;
  }

  if (block != null) {
    _insertValue(rootObj, stack, block.stackIdx, block.key,
        synxString(block.content.toString()));
  }
  return result;
}

/// Reshape parsed tree for `!tool` mode.
SynxValue reshapeToolOutput(SynxValue root, bool schema) {
  if (root is! SynxObj) return root;
  final map = root.map;

  if (schema) {
    final keys = map.sortedKeys;
    final tools = <SynxValue>[];
    for (final k in keys) {
      final def = SynxObject();
      def.set('name', synxString(k));
      def.set('params', map[k] ?? synxNull());
      tools.add(synxObject(def));
    }
    final out = SynxObject();
    out.set('tools', synxArray(tools));
    return synxObject(out);
  }

  if (map.isEmpty) {
    final out = SynxObject();
    out.set('tool', synxNull());
    out.set('params', synxObject());
    return synxObject(out);
  }
  final keys = map.sortedKeys;
  final firstKey = keys.first;
  final firstVal = map[firstKey] ?? synxNull();
  final params = firstVal is SynxObj ? firstVal : synxObject();
  final out = SynxObject();
  out.set('tool', synxString(firstKey));
  out.set('params', params);
  return synxObject(out);
}

// ─── internal types ─────────────────────────────────────────────────────────

enum _StackKind { root, key, listItem }

class _StackFrame {
  final int indent;
  final _StackKind kind;
  final String key;
  final int itemIdx;
  const _StackFrame(this.indent, this.kind, this.key, this.itemIdx);
}

class _BlockState {
  final int indent;
  final String key;
  final int stackIdx;
  final StringBuffer content = StringBuffer();
  _BlockState(this.indent, this.key, this.stackIdx);
}

class _ListState {
  final int indent;
  final String key;
  final int stackIdx;
  _ListState(this.indent, this.key, this.stackIdx);
}

class _ParsedLine {
  String key = '';
  String? typeHint;
  String value = '';
  List<String> markers = [];
  List<String> markerArgs = [];
  SynxConstraints? constraints;
}

// ─── helpers ────────────────────────────────────────────────────────────────

List<int> _lineBytes(Uint8List bytes, List<int> starts, int i) {
  final s = starts[i];
  var e = (i + 1 < starts.length) ? starts[i + 1] - 1 : bytes.length;
  if (e > s && bytes[e - 1] == 0x0D) e--;
  return bytes.sublist(s, e);
}

int _indentOf(List<int> line) {
  var i = 0;
  while (i < line.length && (line[i] == 0x20 || line[i] == 0x09)) {
    i++;
  }
  return i;
}

int _firstWs(String s) {
  for (var i = 0; i < s.length; i++) {
    final c = s[i];
    if (c == ' ' || c == '\t') return i;
  }
  return -1;
}

String _stripComment(String val) {
  var r = val;
  final p1 = r.indexOf(' //');
  if (p1 >= 0) r = r.substring(0, p1);
  final p2 = r.indexOf(' #');
  if (p2 >= 0) r = r.substring(0, p2);
  while (r.isNotEmpty &&
      (r[r.length - 1] == ' ' || r[r.length - 1] == '\t' || r[r.length - 1] == '\r')) {
    r = r.substring(0, r.length - 1);
  }
  return r;
}

_ParsedLine? _parseLine(String trimmed) {
  if (trimmed.isEmpty) return null;
  final first = trimmed[0];
  if (first == '#' || trimmed.startsWith('//') || trimmed.startsWith('- ')) {
    return null;
  }
  if (first == '[' ||
      first == ':' ||
      first == '-' ||
      first == '/' ||
      first == '(') {
    return null;
  }
  final len = trimmed.length;
  var pos = 0;
  while (pos < len) {
    final ch = trimmed[pos];
    if (ch == ' ' || ch == '\t' || ch == '[' || ch == ':' || ch == '(') break;
    pos++;
  }
  final out = _ParsedLine()..key = trimmed.substring(0, pos);

  if (pos < len && trimmed[pos] == '(') {
    final start = pos + 1;
    var scan = start;
    while (scan < len && trimmed[scan] != ')') {
      scan++;
    }
    if (scan < len) {
      out.typeHint = trimmed.substring(start, scan);
      pos = scan + 1;
    } else {
      pos = start;
    }
  }

  if (pos < len && trimmed[pos] == '[') {
    final cstart = pos + 1;
    var depth = 1;
    var scan = cstart;
    while (scan < len && depth > 0) {
      final b = trimmed[scan];
      if (b == '[') {
        depth++;
      } else if (b == ']') {
        depth--;
        if (depth == 0) break;
      }
      scan++;
    }
    if (depth == 0) {
      out.constraints = _parseConstraints(trimmed.substring(cstart, scan));
      pos = scan + 1;
    } else {
      var sweep = cstart;
      while (sweep < len && trimmed[sweep] != ']') {
        sweep++;
      }
      if (sweep < len) {
        out.constraints = _parseConstraints(trimmed.substring(cstart, sweep));
        pos = sweep + 1;
      } else {
        out.constraints = _parseConstraints(trimmed.substring(cstart));
        pos = len;
      }
    }
  }

  if (pos < len && trimmed[pos] == ':') {
    final mstart = pos + 1;
    var mend = mstart;
    while (mend < len && trimmed[mend] != ' ' && trimmed[mend] != '\t') {
      mend++;
    }
    final chain = trimmed.substring(mstart, mend);
    var segs = 0;
    for (final seg in chain.split(':')) {
      if (segs >= maxMarkerSegments) break;
      out.markers.add(seg);
      segs++;
    }
    pos = mend;
  }

  while (pos < len && (trimmed[pos] == ' ' || trimmed[pos] == '\t')) {
    pos++;
  }
  out.value = pos < len ? _stripComment(trimmed.substring(pos)) : '';

  if (out.markers.contains('random') && out.value.isNotEmpty) {
    final nums = <String>[];
    for (final tok in out.value.split(RegExp(r'\s+'))) {
      if (tok.isEmpty) continue;
      if (double.tryParse(tok) != null) nums.add(tok);
    }
    if (nums.isNotEmpty) {
      out.markerArgs = nums;
      out.value = '';
    }
  }
  if (out.markers.contains('inherit') && out.value.isNotEmpty) {
    out.markerArgs = [out.value.trim()];
    out.value = '';
  }
  return out;
}

SynxConstraints _parseConstraints(String raw) {
  final c = SynxConstraints();
  for (final rawPart in raw.split(',')) {
    final part = rawPart.trim();
    if (part.isEmpty) continue;
    if (part == 'required') {
      c.required = true;
      continue;
    }
    if (part == 'readonly') {
      c.readonly = true;
      continue;
    }
    final colon = part.indexOf(':');
    if (colon < 0) continue;
    final k = part.substring(0, colon).trim();
    final v = part.substring(colon + 1).trim();
    switch (k) {
      case 'min':
        final d = double.tryParse(v);
        if (d != null) c.min = d;
        break;
      case 'max':
        final d = double.tryParse(v);
        if (d != null) c.max = d;
        break;
      case 'type':
        c.typeName = v;
        break;
      case 'pattern':
        c.pattern = v;
        break;
      case 'enum':
        final vals = <String>[];
        var count = 0;
        for (final piece in v.split('|')) {
          if (count >= maxEnumParts) break;
          vals.add(piece);
          count++;
        }
        c.enumValues = vals;
        break;
    }
  }
  return c;
}

SynxValue _castValue(String val) {
  if (val.length >= 2) {
    final f = val[0];
    final l = val[val.length - 1];
    if ((f == '"' && l == '"') || (f == "'" && l == "'")) {
      return synxString(val.substring(1, val.length - 1));
    }
  }
  switch (val) {
    case 'true':
      return synxBool(true);
    case 'false':
      return synxBool(false);
    case 'null':
      return synxNull();
  }
  if (val.isEmpty) return synxString('');
  var start = 0;
  if (val[0] == '-') {
    if (val.length == 1) return synxString(val);
    start = 1;
  }
  final c0 = val.codeUnitAt(start);
  if (c0 < 0x30 || c0 > 0x39) return synxString(val);
  var seenDot = false;
  var dotPos = -1;
  var allNumeric = true;
  for (var j = start; j < val.length; j++) {
    final c = val.codeUnitAt(j);
    if (c == 0x2E) {
      if (seenDot) {
        allNumeric = false;
        break;
      }
      seenDot = true;
      dotPos = j;
    } else if (c < 0x30 || c > 0x39) {
      allNumeric = false;
      break;
    }
  }
  if (!allNumeric) return synxString(val);
  if (seenDot) {
    if (dotPos > start && dotPos < val.length - 1) {
      final d = double.tryParse(val);
      if (d != null) return synxFloat(d);
    }
    return synxString(val);
  }
  final n = int.tryParse(val);
  if (n != null) return synxInt(n);
  return synxString(val);
}

SynxValue _castTyped(String val, String hint) {
  switch (hint) {
    case 'int':
      return synxInt(int.tryParse(val) ?? 0);
    case 'float':
      return synxFloat(double.tryParse(val) ?? 0);
    case 'bool':
      return synxBool(val.trim() == 'true');
    case 'string':
      return synxString(val);
    case 'random':
    case 'random:int':
      // Match Rust `rng::random_i64()`: full signed 64-bit range including
      // negative values. `Random.nextInt` caps at 2^32, so combine two
      // 32-bit draws; on Dart Native `int` is 64-bit signed so the high bit
      // naturally produces negative results too.
      final r = Random();
      final hi = r.nextInt(1 << 32);
      final lo = r.nextInt(1 << 32);
      return synxInt((hi << 32) | lo);
    case 'random:float':
      return synxFloat(Random().nextDouble());
    case 'random:bool':
      return synxBool(Random().nextBool());
  }
  return _castValue(val);
}

// ─── tree mutation ──────────────────────────────────────────────────────────

String _buildPath(List<_StackFrame> stack) {
  final parts = <String>[];
  for (var i = 1; i < stack.length; i++) {
    if (stack[i].kind == _StackKind.key) parts.add(stack[i].key);
  }
  return parts.join('.');
}

void _insertValue(SynxObject root, List<_StackFrame> stack, int parentIdx,
    String key, SynxValue value) {
  if (parentIdx == 0) {
    root.set(key, value);
    return;
  }
  final path = stack.sublist(1, parentIdx + 1);
  _setValueAtPath(root, path, 0, key, value);
}

void _setValueAtPath(SynxObject obj, List<_StackFrame> path, int idx,
    String key, SynxValue value) {
  if (idx >= path.length) {
    obj.set(key, value);
    return;
  }
  final head = path[idx];
  switch (head.kind) {
    case _StackKind.root:
      _setValueAtPath(obj, path, idx + 1, key, value);
      return;
    case _StackKind.key:
      final v = obj[head.key];
      if (v is! SynxObj) return;
      _setValueAtPath(v.map, path, idx + 1, key, value);
      return;
    case _StackKind.listItem:
      final v = obj[head.key];
      if (v is! SynxArr) return;
      if (head.itemIdx >= v.values.length) return;
      final item = v.values[head.itemIdx];
      if (item is! SynxObj) return;
      _setValueAtPath(item.map, path, idx + 1, key, value);
      return;
  }
}

void _mutateArray(SynxObject root, List<_StackFrame> stack, int parentIdx,
    String listKey, void Function(List<SynxValue> arr) transform) {
  final path = stack.sublist(1, parentIdx + 1);
  _mutateArrayPath(root, path, 0, listKey, transform);
}

void _mutateArrayPath(SynxObject obj, List<_StackFrame> path, int idx,
    String listKey, void Function(List<SynxValue>) transform) {
  if (idx >= path.length) {
    final cur = obj[listKey];
    List<SynxValue> arr;
    if (cur is SynxArr) {
      arr = cur.values;
    } else {
      arr = <SynxValue>[];
    }
    transform(arr);
    obj.set(listKey, synxArray(arr));
    return;
  }
  final head = path[idx];
  switch (head.kind) {
    case _StackKind.root:
      _mutateArrayPath(obj, path, idx + 1, listKey, transform);
      return;
    case _StackKind.key:
      final v = obj[head.key];
      if (v is! SynxObj) return;
      _mutateArrayPath(v.map, path, idx + 1, listKey, transform);
      return;
    case _StackKind.listItem:
      final v = obj[head.key];
      if (v is! SynxArr) return;
      if (head.itemIdx >= v.values.length) return;
      final item = v.values[head.itemIdx];
      if (item is! SynxObj) return;
      _mutateArrayPath(item.map, path, idx + 1, listKey, transform);
      return;
  }
}
