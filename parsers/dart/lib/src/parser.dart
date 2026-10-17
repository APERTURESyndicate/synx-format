// SYNX text-to-tree parser. Mirrors crates/synx-core/src/parser.rs.
//
// perf: lines are walked as offsets into the text; a slice is copied only when
// it is stored (a key, a value, a block body).
import 'dart:math';

import 'meta.dart';
import 'num.dart';
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

/// Drop a leading U+FEFF (SYNX §2.1) and cut the text to 16 MiB of UTF-8
/// without splitting a character.
String clampText(String text) => _clamp(text).$1;

/// [clampText], and whether anything was cut. A dropped BOM is not truncation:
/// synx-core measures the original length after it.
(String, bool) _clamp(String text) {
  if (text.isNotEmpty && text.codeUnitAt(0) == 0xFEFF) text = text.substring(1);
  if (text.length <= maxInputBytes ~/ 3 || utf8Len(text) <= maxInputBytes) return (text, false);
  var bytes = 0;
  var i = 0;
  while (i < text.length) {
    final c = text.codeUnitAt(i);
    final pair = c >= 0xD800 && c <= 0xDBFF && i + 1 < text.length;
    final b = pair ? 4 : utf8LenUnit(c);
    if (bytes + b > maxInputBytes) break;
    bytes += b;
    i += pair ? 2 : 1;
  }
  return (text.substring(0, i), true);
}

bool _isReservedJsKey(String key) =>
    key == '__proto__' || key == 'constructor' || key == 'prototype';

bool _eqAt(String s, int start, int end, String lit) {
  if (end - start != lit.length) return false;
  return s.startsWith(lit, start);
}

bool _startsAt(String s, int start, int end, String lit) =>
    end - start >= lit.length && s.startsWith(lit, start);

/// Parse a SYNX text into a [SynxParseResult].
SynxParseResult parse(String rawText) {
  var (text, truncated) = _clamp(rawText);
  // Keep at most maxLineStarts lines: cut before the newline that would start one more.
  {
    var seen = 0;
    var scan = 0;
    while (true) {
      final nl = text.indexOf('\n', scan);
      if (nl < 0) break;
      if (seen >= maxLineStarts - 1) {
        text = text.substring(0, nl);
        truncated = true;
        break;
      }
      seen++;
      scan = nl + 1;
    }
  }

  final result = SynxParseResult();
  final root = (result.root as SynxObj).map;
  final metadata = result.metadata;
  final stack = <_Frame>[_Frame(-1, _Kind.root, '', 0, 0, root)..path = ''];

  _BlockState? block;
  _ListState? list;
  var inBlockComment = false;

  final len = text.length;
  var pos = 0;
  while (pos <= len) {
    final start = pos;
    final nlIdx = text.indexOf('\n', start);
    final nl = nlIdx < 0 ? len : nlIdx;
    pos = nl + 1;
    final end = nl > start && text.codeUnitAt(nl - 1) == 0x0D ? nl - 1 : nl;

    var ts = start;
    while (ts < end && isWs(text.codeUnitAt(ts))) {
      ts++;
    }
    // A blank line is never structural and never body (SYNX §8.4.1).
    if (ts == end) continue;
    var te = end;
    while (te > ts && isWs(text.codeUnitAt(te - 1))) {
      te--;
    }

    // Indentation is the UTF-8 byte length of the leading whitespace (SYNX §4).
    var indent = 0;
    for (var k = start; k < ts; k++) {
      indent += utf8LenUnit(text.codeUnitAt(k));
    }

    // Block continuation runs before every line-class rule below: a `|` / `|+`
    // body is raw text, so `#`, `//`, `###` and `!…` are ordinary content there.
    final blk = block;
    if (blk != null) {
      if (indent > blk.indent) {
        if (blk.bytes < maxMultilineBytes) {
          if (blk.content.isNotEmpty) {
            blk.content.write('\n');
            blk.bytes++;
          }
          final room = maxMultilineBytes - blk.bytes;
          if (room > 0) {
            int sliceStart;
            int sliceEnd;
            if (blk.preserveIndent) {
              // `|+` (SYNX 3.7): lock the strip prefix to the first continuation
              // line's indent and cut that many bytes of whitespace — by whole
              // characters — from each line. Spec §8.4.1.
              if (blk.baseIndent < 0) blk.baseIndent = indent;
              final strip = min(blk.baseIndent, indent);
              var rawEnd = end;
              while (rawEnd > start && isWs(text.codeUnitAt(rawEnd - 1))) {
                rawEnd--;
              }
              var cut = start;
              var off = 0;
              while (cut < rawEnd && off < strip && isWs(text.codeUnitAt(cut))) {
                off += utf8LenUnit(text.codeUnitAt(cut));
                cut++;
              }
              sliceStart = cut;
              sliceEnd = rawEnd;
            } else {
              sliceStart = ts;
              sliceEnd = te;
            }
            var take = sliceStart;
            var used = 0;
            while (take < sliceEnd) {
              final c = text.codeUnitAt(take);
              final pair = c >= 0xD800 && c <= 0xDBFF && take + 1 < sliceEnd;
              final b = pair ? 4 : utf8LenUnit(c);
              if (used + b > room) break;
              used += b;
              take += pair ? 2 : 1;
            }
            blk.content.write(text.substring(sliceStart, take));
            blk.bytes += used;
            if (take < sliceEnd) truncated = true;
          }
        } else {
          truncated = true;
        }
        continue;
      }
      block = null;
      _insert(stack, blk.stackIdx, blk.key, synxString(blk.content.toString()));
    }

    if (_eqAt(text, ts, te, '###')) {
      inBlockComment = !inBlockComment;
      continue;
    }
    if (inBlockComment) continue;

    final first = text.codeUnitAt(ts);
    // Directive lines (§6). Unknown `!…` lines fall through to the generic handling.
    if (first == 0x21 /* ! */ || _startsAt(text, ts, te, '#!mode:')) {
      if (_eqAt(text, ts, te, '!active')) {
        result.mode = SynxMode.active;
        continue;
      }
      if (_eqAt(text, ts, te, '!lock')) {
        result.locked = true;
        continue;
      }
      if (_eqAt(text, ts, te, '!tool')) {
        result.tool = true;
        continue;
      }
      if (_eqAt(text, ts, te, '!schema')) {
        result.schema = true;
        continue;
      }
      if (_eqAt(text, ts, te, '!llm')) {
        result.llm = true;
        continue;
      }
      if (_startsAt(text, ts, te, '!include ')) {
        if (result.includes.length < maxIncludes) {
          final rest = trimWs(text.substring(ts + 9, te));
          var ws = 0;
          while (ws < rest.length && !isWs(rest.codeUnitAt(ws))) {
            ws++;
          }
          final path = rest.substring(0, ws);
          final alias = ws < rest.length ? trimWs(rest.substring(ws + 1)) : _deriveIncludeAlias(path);
          result.includes.add(SynxIncludeDirective(path, alias));
        }
        continue;
      }
      if (_startsAt(text, ts, te, '!use ')) {
        final rest = trimWs(text.substring(ts + 5, te));
        if (rest.startsWith('@')) {
          // Parse: !use @scope/name [as alias]
          final asIdx = rest.indexOf(' as ');
          final pkg = trimWs(asIdx < 0 ? rest : rest.substring(0, asIdx));
          final alias = asIdx >= 0 ? trimWs(rest.substring(asIdx + 4)) : pkg.substring(pkg.lastIndexOf('/') + 1);
          if (pkg.isNotEmpty) result.uses.add(SynxUseDirective(pkg, alias));
        }
        continue;
      }
      if (_startsAt(text, ts, te, '#!mode:')) {
        final declared = trimWs(text.substring(ts + 7, te));
        result.mode = declared == 'active' ? SynxMode.active : SynxMode.static_;
        continue;
      }
    }

    if (first == 0x23 /* # */ || _startsAt(text, ts, te, '//')) continue;

    final lst = list;
    if (_startsAt(text, ts, te, '- ')) {
      if (lst != null && indent > lst.indent) {
        // Pop every frame of the previous item — its own frame and any object
        // nested inside it; popping only item frames stopped at a nested object,
        // so the next item's keys were looked up through it and dropped.
        while (stack.length > 1 && stack.last.indent >= indent) {
          stack.removeLast();
        }

        final valStr = _stripComment(trimWs(text.substring(ts + 2, te)));

        // The item is an object when the next content line is deeper and not
        // another item or a comment.
        var nested = false;
        final next = _nextContentLine(text, pos);
        if (next != null) {
          var pts = next.$1;
          var pi = 0;
          while (pts < next.$2 && isWs(text.codeUnitAt(pts))) {
            pi += utf8LenUnit(text.codeUnitAt(pts));
            pts++;
          }
          if (pi > indent &&
              !_startsAt(text, pts, next.$2, '- ') &&
              text.codeUnitAt(pts) != 0x23 &&
              !_startsAt(text, pts, next.$2, '//')) {
            nested = true;
          }
        }

        final listParent = stack[lst.stackIdx].map;
        var existing = listParent[lst.key];
        if (existing == null) {
          existing = synxArray();
          listParent[lst.key] = existing;
        }
        // A key re-declared under the open list as something else takes the
        // items no more: they are dropped, never written into a detached array.
        if (existing is SynxArr && existing.values.length < maxListItems) {
          final items = existing.values;
          if (nested) {
            final itemObj = SynxObject();
            final parsed = _parseLine(valStr);
            if (parsed != null) {
              final SynxValue v = parsed.typeHint != null
                  ? _castTyped(parsed.value, parsed.typeHint!)
                  : parsed.value.isNotEmpty
                      ? castValue(parsed.value)
                      : synxObject();
              if (!_isReservedJsKey(parsed.key)) itemObj[parsed.key] = v;
            } else {
              itemObj['_value'] = castValue(valStr);
            }
            final itemIdx = items.length;
            items.add(SynxObj(itemObj));
            if (stack.length < maxNestingDepth) {
              stack.add(_Frame(indent, _Kind.listItem, lst.key, itemIdx, lst.stackIdx, itemObj));
            }
          } else {
            items.add(castValue(valStr));
          }
        }
        continue;
      }
    } else if (lst != null && indent <= lst.indent) {
      list = null;
      while (stack.length > 1 && stack.last.kind == _Kind.listItem && stack.last.indent >= indent) {
        stack.removeLast();
      }
    }

    final parsed = _parseLine(text.substring(ts, te));
    if (parsed == null) continue;

    // Reject prototype-polluting keys so JSON consumers (notably JS) are never
    // exposed to __proto__ / constructor / prototype injection.
    if (_isReservedJsKey(parsed.key)) continue;

    while (stack.length > 1 && stack.last.indent >= indent) {
      stack.removeLast();
    }
    final parentIdx = stack.length - 1;
    final parentMap = stack[parentIdx].map;

    if (result.mode == SynxMode.active && metadata.isNotEmpty) {
      final replaced = parentMap[parsed.key];
      if (replaced is SynxObj || replaced is SynxArr) {
        // A key that replaces an object or a list replaces its declarations too:
        // metadata is filed by path, so the old subtree's markers and constraints
        // used to apply to whatever took its place.
        final parentPath = _pathOf(stack, parentIdx);
        final prefix = parentPath.isEmpty ? keySegment(parsed.key) : '$parentPath.${keySegment(parsed.key)}';
        metadata.removeWhere((p, _) =>
            p == prefix || (p.length > prefix.length && p.codeUnitAt(prefix.length) == 0x2E && p.startsWith(prefix)));
      }
    }

    if (result.mode == SynxMode.active &&
        (parsed.markers.isNotEmpty || parsed.constraints != null || parsed.typeHint != null)) {
      final path = _pathOf(stack, parentIdx);
      metadata.putIfAbsent(path, () => <String, SynxMeta>{})[parsed.key] = SynxMeta()
        ..markers = parsed.markers
        ..args = parsed.markerArgs
        ..typeHint = parsed.typeHint
        ..constraints = parsed.constraints;
    }

    // `|` (3.6) trims each continuation line; `|+` (3.7) keeps the indent
    // relative to the first one. Spec §8.4.1.
    final isBlock = parsed.value == '|' || parsed.value == '|+';
    final isListMarker =
        parsed.markers.any((m) => m == 'random' || m == 'unique' || m == 'geo' || m == 'join');

    if (isBlock) {
      parentMap[parsed.key] = synxString('');
      block = _BlockState(indent, parsed.key, parentIdx, parsed.value == '|+');
    } else if (isListMarker && parsed.value.isEmpty) {
      // Insert an empty array now so the key exists even if the list stays empty.
      parentMap[parsed.key] = synxArray();
      list = _ListState(indent, parsed.key, parentIdx);
    } else if (parsed.value.isEmpty) {
      final next = _nextContentLine(text, pos);
      if (next != null && trimWs(text.substring(next.$1, next.$2)).startsWith('- ')) {
        parentMap[parsed.key] = synxArray();
        list = _ListState(indent, parsed.key, parentIdx);
        continue;
      }
      final obj = SynxObject();
      parentMap[parsed.key] = SynxObj(obj);
      // Past the cap the object is still inserted, but nesting stops increasing.
      if (stack.length < maxNestingDepth) {
        stack.add(_Frame(indent, _Kind.key, parsed.key, 0, parentIdx, obj));
      }
    } else {
      parentMap[parsed.key] =
          parsed.typeHint != null ? _castTyped(parsed.value, parsed.typeHint!) : castValue(parsed.value);
    }
  }

  final blkEnd = block;
  if (blkEnd != null) {
    _insert(stack, blkEnd.stackIdx, blkEnd.key, synxString(blkEnd.content.toString()));
  }
  result.truncated = truncated;
  return result;
}

void _insert(List<_Frame> stack, int idx, String key, SynxValue value) {
  if (idx < stack.length) stack[idx].map[key] = value;
}

/// The first non-blank line at or after [from], CR excluded.
(int, int)? _nextContentLine(String text, int from) {
  var p = from;
  while (p <= text.length) {
    final nlIdx = p < text.length ? text.indexOf('\n', p) : -1;
    final nl = nlIdx < 0 ? text.length : nlIdx;
    final end = nl > p && text.codeUnitAt(nl - 1) == 0x0D ? nl - 1 : nl;
    for (var k = p; k < end; k++) {
      if (!isWs(text.codeUnitAt(k))) return (p, end);
    }
    p = nl + 1;
  }
  return null;
}

String _deriveIncludeAlias(String path) {
  final name = path.substring(max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
  if (name.endsWith('.synx') || name.endsWith('.SYNX')) return name.substring(0, name.length - 5);
  return name;
}

/// Reshape parsed tree for `!tool` mode.
SynxValue reshapeToolOutput(SynxValue root, bool schema) {
  if (root is! SynxObj) return root;
  final map = root.map;
  final keys = sortCodePoints(map.keys);

  if (schema) {
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
  final firstKey = keys.first;
  final firstVal = map[firstKey] ?? synxNull();
  final params = SynxObject();
  if (firstVal is SynxObj) {
    for (final e in firstVal.map.entryView) {
      params[e.key] = e.value;
    }
  }
  final out = SynxObject();
  out.set('tool', synxString(firstKey));
  out.set('params', synxObject(params));
  return synxObject(out);
}

// ─── Metadata paths ──────────────────────────────────────────────────────────

/// The metadata path of frame [idx]: object keys, and for a list item the
/// list's key followed by `#<index>`. Follows the frame chain, so an unrelated
/// object opened between a list key and its item stays out of it.
String _pathOf(List<_Frame> stack, int idx) {
  final f = stack[idx];
  final cached = f.path;
  if (cached != null) return cached;
  final parentPath = _pathOf(stack, f.kind == _Kind.listItem ? f.parent : idx - 1);
  final seg = keySegment(f.key);
  var p = parentPath.isEmpty ? seg : '$parentPath.$seg';
  if (f.kind == _Kind.listItem) p = '$p.#${f.itemIdx}';
  f.path = p;
  return p;
}

/// [key] as one path segment: `.` and `\` are escaped with `\`, so the object
/// `a.b` and `b` inside `a` never share a path.
String keySegment(String key) {
  if (!key.contains('.') && !key.contains('\\')) return key;
  final sb = StringBuffer();
  for (var i = 0; i < key.length; i++) {
    final c = key.codeUnitAt(i);
    if (c == 0x2E || c == 0x5C) sb.write('\\');
    sb.writeCharCode(c);
  }
  return sb.toString();
}

/// The unescaped segments of a metadata path: `a\.b.c` is `["a.b", "c"]`.
List<String> metaPathSegments(String path) {
  final out = <String>[];
  if (path.isEmpty) return out;
  final seg = StringBuffer();
  for (var i = 0; i < path.length; i++) {
    final c = path.codeUnitAt(i);
    if (c == 0x5C) {
      if (i + 1 < path.length) seg.writeCharCode(path.codeUnitAt(++i));
    } else if (c == 0x2E) {
      out.add(seg.toString());
      seg.clear();
    } else {
      seg.writeCharCode(c);
    }
  }
  out.add(seg.toString());
  return out;
}

// ─── internal types ──────────────────────────────────────────────────────────

enum _Kind { root, key, listItem }

/// One open object. [map] is the object itself: a frame lives only while its
/// object is attached (anything that could replace it pops the frame first),
/// so holding it equals navigating to it from the root.
class _Frame {
  final int indent;
  final _Kind kind;
  final String key;
  final int itemIdx;

  /// For a list item, the frame holding the list (not necessarily the one below).
  final int parent;
  final SynxObject map;
  String? path;
  _Frame(this.indent, this.kind, this.key, this.itemIdx, this.parent, this.map);
}

class _BlockState {
  final int indent;
  final String key;
  final int stackIdx;
  final StringBuffer content = StringBuffer();

  /// SYNX 3.7 `|+`: keep indent relative to the first continuation line.
  final bool preserveIndent;

  /// Locked on the first content line; -1 means "unlocked".
  int baseIndent = -1;

  /// UTF-8 bytes in [content] (the block cap is in bytes).
  int bytes = 0;
  _BlockState(this.indent, this.key, this.stackIdx, this.preserveIndent);
}

class _ListState {
  final int indent;
  final String key;
  final int stackIdx;
  _ListState(this.indent, this.key, this.stackIdx);
}

class _ParsedLine {
  final String key;
  final String? typeHint;
  final String value;
  final List<String> markers;
  final List<String> markerArgs;
  final SynxConstraints? constraints;
  _ParsedLine(this.key, this.typeHint, this.value, this.markers, this.markerArgs, this.constraints);
}

// ─── line parser ─────────────────────────────────────────────────────────────

/// Cut an inline ` //` or ` #` comment and trailing whitespace off a value.
String _stripComment(String val) {
  var r = val;
  if (r.contains(' ')) {
    final p1 = r.indexOf(' //');
    if (p1 >= 0) r = r.substring(0, p1);
    final p2 = r.indexOf(' #');
    if (p2 >= 0) r = r.substring(0, p2);
  }
  return trimWsEnd(r);
}

_ParsedLine? _parseLine(String trimmed) {
  if (trimmed.isEmpty) return null;
  final first = trimmed.codeUnitAt(0);
  // [ : - # / (
  if (first == 0x5B || first == 0x3A || first == 0x2D || first == 0x23 || first == 0x2F || first == 0x28) {
    return null;
  }
  final len = trimmed.length;
  var pos = 0;
  while (pos < len) {
    final ch = trimmed.codeUnitAt(pos);
    if (ch == 0x20 || ch == 0x09 || ch == 0x5B || ch == 0x3A || ch == 0x28) break;
    pos++;
  }
  final key = trimmed.substring(0, pos);

  String? typeHint;
  if (pos < len && trimmed.codeUnitAt(pos) == 0x28) {
    final start = pos + 1;
    final close = trimmed.indexOf(')', start);
    if (close >= 0) {
      typeHint = trimmed.substring(start, close);
      pos = close + 1;
    } else {
      pos++;
    }
  }

  SynxConstraints? constraints;
  if (pos < len && trimmed.codeUnitAt(pos) == 0x5B) {
    // Balanced-bracket scan so patterns like `^[a-z]+$` keep their own `]`.
    final cstart = pos + 1;
    var depth = 1;
    var scan = cstart;
    while (scan < len && depth > 0) {
      final b = trimmed.codeUnitAt(scan);
      if (b == 0x5B) {
        depth++;
      } else if (b == 0x5D) {
        depth--;
        if (depth == 0) break;
      }
      scan++;
    }
    if (depth == 0) {
      constraints = parseConstraints(trimmed.substring(cstart, scan));
      pos = scan + 1;
    } else {
      // Unbalanced: fall back to the first `]`, if any.
      final rel = trimmed.indexOf(']', cstart);
      if (rel >= 0) {
        constraints = parseConstraints(trimmed.substring(cstart, rel));
        pos = rel + 1;
      } else {
        constraints = parseConstraints(trimmed.substring(cstart));
        pos = len;
      }
    }
  }

  var markers = <String>[];
  if (pos < len && trimmed.codeUnitAt(pos) == 0x3A) {
    final mstart = pos + 1;
    var mend = mstart;
    while (mend < len && trimmed.codeUnitAt(mend) != 0x20 && trimmed.codeUnitAt(mend) != 0x09) {
      mend++;
    }
    // Empty segments are kept (`a::b` is three), as in synx-core.
    var segStart = mstart;
    while (markers.length < maxMarkerSegments) {
      final c = trimmed.indexOf(':', segStart);
      if (c < 0 || c >= mend) {
        markers.add(trimmed.substring(segStart, mend));
        break;
      }
      markers.add(trimmed.substring(segStart, c));
      segStart = c + 1;
    }
    pos = mend;
  }

  while (pos < len && (trimmed.codeUnitAt(pos) == 0x20 || trimmed.codeUnitAt(pos) == 0x09)) {
    pos++;
  }
  var value = pos < len ? _stripComment(trimmed.substring(pos)) : '';
  var args = <String>[];

  if (markers.isNotEmpty && value.isNotEmpty) {
    // :random — weight percentages ride in the value.
    if (markers.contains('random')) {
      final nums = <String>[];
      var i = 0;
      while (i < value.length) {
        while (i < value.length && isWs(value.codeUnitAt(i))) {
          i++;
        }
        final s = i;
        while (i < value.length && !isWs(value.codeUnitAt(i))) {
          i++;
        }
        if (i > s) {
          final tok = value.substring(s, i);
          if (parseF64(tok) != null) nums.add(tok);
        }
      }
      if (nums.isNotEmpty) {
        args = nums;
        value = '';
      }
    }
    // :inherit — a value names the parent key; it opens a group, not a leaf.
    if (value.isNotEmpty && markers.contains('inherit')) {
      args = [trimWs(value)];
      value = '';
    }
  }
  return _ParsedLine(key, typeHint, value, markers, args, constraints);
}

/// The body of a `[…]` block; present even when nothing in it is recognised.
SynxConstraints parseConstraints(String raw) {
  final c = SynxConstraints();
  for (final rawPart in raw.split(',')) {
    final part = trimWs(rawPart);
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
    final k = trimWs(part.substring(0, colon));
    final v = trimWs(part.substring(colon + 1));
    switch (k) {
      case 'min':
        c.min = parseF64(v);
      case 'max':
        c.max = parseF64(v);
      case 'type':
        c.typeName = v;
      case 'pattern':
        c.pattern = v;
      case 'enum':
        final vals = <String>[];
        for (final piece in v.split('|')) {
          if (vals.length >= maxEnumParts) break;
          vals.add(piece);
        }
        c.enumValues = vals;
    }
  }
  return c;
}

/// SYNX §8.3 automatic casting.
SynxValue castValue(String val) {
  if (val.length >= 2) {
    final f = val.codeUnitAt(0);
    final l = val.codeUnitAt(val.length - 1);
    if ((f == 0x22 && l == 0x22) || (f == 0x27 && l == 0x27)) {
      return synxString(val.substring(1, val.length - 1));
    }
  }
  switch (val) {
    case 'true':
      return const SynxBool(true);
    case 'false':
      return const SynxBool(false);
    case 'null':
      return SynxNull.instance;
    case '':
      return const SynxStr('');
  }
  var start = 0;
  if (val.codeUnitAt(0) == 0x2D) {
    if (val.length == 1) return synxString(val);
    start = 1;
  }
  final c0 = val.codeUnitAt(start);
  if (c0 < 0x30 || c0 > 0x39) return synxString(val);
  var dotPos = -1;
  for (var j = start; j < val.length; j++) {
    final c = val.codeUnitAt(j);
    if (c == 0x2E) {
      if (dotPos >= 0) return synxString(val);
      dotPos = j;
    } else if (c < 0x30 || c > 0x39) {
      return synxString(val);
    }
  }
  if (dotPos >= 0) {
    if (dotPos > start && dotPos < val.length - 1) return synxFloat(double.parse(val));
    return synxString(val);
  }
  final n = parseI64(val);
  return n != null ? synxInt(n) : synxString(val);
}

final Random _rng = Random();

/// SYNX §8.3 typed casting.
SynxValue _castTyped(String val, String hint) {
  switch (hint) {
    case 'int':
      return synxInt(parseI64(val) ?? 0);
    case 'float':
      return synxFloat(parseF64(val) ?? 0);
    case 'bool':
      return synxBool(trimWs(val) == 'true');
    case 'string':
      return synxString(val);
    case 'random':
    case 'random:int':
      // Full signed 64-bit range, as Rust `rng::random_i64()`.
      final hi = _rng.nextInt(1 << 32);
      final lo = _rng.nextInt(1 << 32);
      return synxInt((hi << 32) | lo);
    case 'random:float':
      return synxFloat(_rng.nextDouble());
    case 'random:bool':
      return synxBool(_rng.nextBool());
  }
  return castValue(val);
}
