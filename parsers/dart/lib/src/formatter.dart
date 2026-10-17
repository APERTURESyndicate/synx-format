// Canonical SYNX text reformatter. Mirrors fmt_canonical in synx-core lib.rs
// byte for byte: keys sorted by code point (stable), 2-space indentation,
// comments dropped, `|+` bodies kept verbatim with their relative indent.

import 'num.dart';
import 'parser.dart' show clampText;

const int maxFormatParseDepth = 128;

String format(String text) {
  final lines = _rustLines(clampText(text));

  final directives = <String>[];
  var bodyStart = 0;
  for (var i = 0; i < lines.length; i++) {
    final t = trimWs(lines[i]);
    if (t == '!active' || t == '!lock' || t == '!tool' || t == '!schema' || t == '!llm' || t == '#!mode:active') {
      directives.add(t);
      bodyStart = i + 1;
    } else if (t.isEmpty || t.startsWith('#') || t.startsWith('//')) {
      bodyStart = i + 1;
    } else {
      break;
    }
  }

  final nodes = <_FmtNode>[];
  _fmtParse(lines, bodyStart, 0, 0, nodes);
  _fmtSort(nodes, 0);

  final out = StringBuffer();
  if (directives.isNotEmpty) {
    out
      ..write(directives.join('\n'))
      ..write('\n\n');
  }
  _fmtEmit(nodes, 0, out);
  return '${trimWsEnd(out.toString())}\n';
}

/// Rust `str::lines`: split at `\n`, drop a trailing `\r`, no final empty line.
List<String> _rustLines(String text) {
  final lines = <String>[];
  var start = 0;
  while (start < text.length) {
    final nl = text.indexOf('\n', start);
    final end = nl < 0 ? text.length : nl;
    final e = end > start && text.codeUnitAt(end - 1) == 0x0D ? end - 1 : end;
    lines.add(text.substring(start, e));
    if (nl < 0) break;
    start = nl + 1;
  }
  return lines;
}

class _FmtNode {
  final String header;
  final String sortKey;
  final List<_FmtNode> children = [];
  final List<String> listItems = [];
  final bool isMultiline;
  _FmtNode(this.header, this.sortKey, this.isMultiline);
}

/// Indentation in UTF-8 bytes of leading whitespace, as in the parser.
int _indentOf(String line) {
  var n = 0;
  for (var i = 0; i < line.length; i++) {
    final c = line.codeUnitAt(i);
    if (!isWs(c)) break;
    n += utf8LenUnit(c);
  }
  return n;
}

/// UTF-16 length of at most [limit] UTF-8 bytes of leading whitespace, by whole characters.
int _wsPrefixLen(String s, int limit) {
  var off = 0;
  var i = 0;
  while (i < s.length && off < limit && isWs(s.codeUnitAt(i))) {
    off += utf8LenUnit(s.codeUnitAt(i));
    i++;
  }
  return i;
}

/// The key of a header, lowercased: up to the first whitespace, `[`, `:` or `(`.
String _sortKey(String header) {
  var end = 0;
  while (end < header.length) {
    final c = header.codeUnitAt(end);
    if (isWs(c) || c == 0x5B || c == 0x3A || c == 0x28) break;
    end++;
  }
  return header.substring(0, end).toLowerCase();
}

int _fmtParse(List<String> lines, int start, int base, int depth, List<_FmtNode> nodes) {
  if (depth > maxFormatParseDepth) {
    // Past the cap the subtree is dropped, but its lines are consumed: everything
    // up to the next non-blank line shallower than [base] (Rust `fmt_parse`).
    var j = start;
    while (j < lines.length && (trimWs(lines[j]).isEmpty || _indentOf(lines[j]) >= base)) {
      j++;
    }
    return j;
  }
  var i = start;
  while (i < lines.length) {
    final raw = lines[i];
    final t = trimWs(raw);
    if (t.isEmpty) {
      i++;
      continue;
    }
    final ind = _indentOf(raw);
    if (ind < base) break;
    if (ind > base) {
      i++;
      continue;
    }
    if (t.startsWith('- ') || t.startsWith('#') || t.startsWith('//')) {
      i++;
      continue;
    }
    // spec: `|+` opens a block too; its body is text with its relative indent
    // kept, never child keys to sort.
    final preserve = t.endsWith(' |+') || t == '|+';
    final node = _FmtNode(t, _sortKey(t), preserve || t.endsWith(' |') || t == '|');
    var blockBase = -1;
    i++;
    while (i < lines.length) {
      final cr = lines[i];
      final ct = trimWs(cr);
      if (ct.isEmpty) {
        i++;
        continue;
      }
      final ci = _indentOf(cr);
      if (ci <= base) break;
      if (preserve) {
        if (blockBase < 0) blockBase = ci;
        final body = trimWsEnd(cr);
        node.listItems.add(body.substring(_wsPrefixLen(body, blockBase < ci ? blockBase : ci)));
        i++;
      } else if (node.isMultiline || ct.startsWith('- ')) {
        node.listItems.add(ct);
        i++;
      } else if (ct.startsWith('#') || ct.startsWith('//')) {
        i++;
      } else {
        i = _fmtParse(lines, i, ci, depth + 1, node.children);
      }
    }
    nodes.add(node);
  }
  return i;
}

void _fmtSort(List<_FmtNode> nodes, int depth) {
  if (depth > maxFormatParseDepth) return;
  // Stable, by code point: equal keys (a repeated key) keep document order, so
  // the last one still wins.
  if (nodes.length > 1) {
    final indexed = [for (var i = 0; i < nodes.length; i++) (nodes[i], i)];
    indexed.sort((a, b) {
      final c = compareCodePoints(a.$1.sortKey, b.$1.sortKey);
      return c != 0 ? c : a.$2 - b.$2;
    });
    for (var i = 0; i < indexed.length; i++) {
      nodes[i] = indexed[i].$1;
    }
  }
  for (final n in nodes) {
    _fmtSort(n.children, depth + 1);
  }
}

void _fmtEmit(List<_FmtNode> nodes, int indent, StringBuffer out) {
  final sp = ' ' * indent;
  final itemSp = ' ' * (indent + 2);
  for (final n in nodes) {
    out
      ..write(sp)
      ..write(n.header)
      ..write('\n');
    if (n.children.isNotEmpty) _fmtEmit(n.children, indent + 2, out);
    for (final li in n.listItems) {
      out
        ..write(itemSp)
        ..write(li)
        ..write('\n');
    }
    if (indent == 0 && (n.children.isNotEmpty || n.listItems.isNotEmpty)) out.write('\n');
  }
}
