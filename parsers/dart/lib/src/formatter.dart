// Canonical SYNX text reformatter. Mirrors fmt_canonical in synx-core lib.rs.

import 'parser.dart' show clampText;

const int maxFormatParseDepth = 128;

String format(String text) {
  final clamped = clampText(text);
  final lines = clamped.split('\n');
  for (var i = 0; i < lines.length; i++) {
    if (lines[i].isNotEmpty && lines[i].endsWith('\r')) {
      lines[i] = lines[i].substring(0, lines[i].length - 1);
    }
  }

  final directives = <String>[];
  var bodyStart = 0;
  for (var i = 0; i < lines.length; i++) {
    final t = lines[i].trim();
    if (t == '!active' ||
        t == '!lock' ||
        t == '!tool' ||
        t == '!schema' ||
        t == '!llm' ||
        t == '#!mode:active') {
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
  _fmtSort(nodes);

  final out = StringBuffer();
  if (directives.isNotEmpty) {
    out.write(directives.join('\n'));
    out.write('\n\n');
  }
  _fmtEmit(nodes, 0, out);
  var s = out.toString();
  while (s.isNotEmpty && (s.endsWith('\n') || s.endsWith(' ') || s.endsWith('\t'))) {
    s = s.substring(0, s.length - 1);
  }
  return '$s\n';
}

class _FmtNode {
  String header;
  List<_FmtNode> children = [];
  List<String> listItems = [];
  bool isMultiline = false;
  _FmtNode(this.header);
}

int _indentOf(String line) {
  var i = 0;
  while (i < line.length && (line[i] == ' ' || line[i] == '\t')) {
    i++;
  }
  return i;
}

int _fmtParse(List<String> lines, int start, int base, int depth,
    List<_FmtNode> out) {
  if (depth > maxFormatParseDepth) return start;
  var i = start;
  while (i < lines.length) {
    final raw = lines[i];
    final t = raw.trim();
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
    final node = _FmtNode(t);
    node.isMultiline = t == '|' || t.endsWith(' |');
    i++;
    while (i < lines.length) {
      final cr = lines[i];
      final ct = cr.trim();
      if (ct.isEmpty) {
        i++;
        continue;
      }
      final ci = _indentOf(cr);
      if (ci <= base) break;
      if (node.isMultiline || ct.startsWith('- ')) {
        node.listItems.add(ct);
        i++;
      } else if (ct.startsWith('#') || ct.startsWith('//')) {
        i++;
      } else {
        final subs = <_FmtNode>[];
        i = _fmtParse(lines, i, ci, depth + 1, subs);
        node.children.addAll(subs);
      }
    }
    out.add(node);
  }
  return i;
}

String _sortKey(String header) {
  var end = 0;
  while (end < header.length) {
    final c = header[end];
    if (c == ' ' || c == '\t' || c == '[' || c == ':' || c == '(') break;
    end++;
  }
  return header.substring(0, end).toLowerCase();
}

void _fmtSort(List<_FmtNode> nodes) {
  nodes.sort((a, b) => _sortKey(a.header).compareTo(_sortKey(b.header)));
  for (final n in nodes) {
    _fmtSort(n.children);
  }
}

void _fmtEmit(List<_FmtNode> nodes, int indent, StringBuffer out) {
  final sp = ' ' * indent;
  final itemSp = ' ' * (indent + 2);
  for (final n in nodes) {
    out.write(sp);
    out.write(n.header);
    out.write('\n');
    if (n.children.isNotEmpty) _fmtEmit(n.children, indent + 2, out);
    for (final li in n.listItems) {
      out.write(itemSp);
      out.write(li);
      out.write('\n');
    }
    if (indent == 0 && (n.children.isNotEmpty || n.listItems.isNotEmpty)) {
      out.write('\n');
    }
  }
}

