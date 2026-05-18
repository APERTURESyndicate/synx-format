// Value → SYNX text. Mirrors crates/synx-core/src/lib.rs::serialize.

import 'value.dart';

const int maxSerializeDepth = 128;

String stringify(SynxValue v) {
  final sb = StringBuffer();
  _serialize(v, 0, sb);
  return sb.toString();
}

void _serialize(SynxValue v, int depth, StringBuffer out) {
  if (depth > maxSerializeDepth) {
    out.write('[synx:max-depth]\n');
    return;
  }
  if (v is! SynxObj) {
    out.write(formatPrimitive(v));
    return;
  }
  final map = v.map;
  final indent = ' ' * (depth * 2);
  for (final k in map.sortedKeys) {
    final val = map[k]!;
    if (val is SynxArr) {
      out.write(indent);
      out.write(k);
      out.write('\n');
      for (final item in val.values) {
        if (item is SynxObj && item.map.isNotEmpty) {
          final entries = item.map.entries;
          out.write(indent);
          out.write('  - ');
          out.write(entries[0].key);
          out.write(' ');
          out.write(formatPrimitive(entries[0].value));
          out.write('\n');
          for (var j = 1; j < entries.length; j++) {
            out.write(indent);
            out.write('    ');
            out.write(entries[j].key);
            out.write(' ');
            out.write(formatPrimitive(entries[j].value));
            out.write('\n');
          }
        } else {
          out.write(indent);
          out.write('  - ');
          out.write(formatPrimitive(item));
          out.write('\n');
        }
      }
    } else if (val is SynxObj) {
      out.write(indent);
      out.write(k);
      out.write('\n');
      _serialize(val, depth + 1, out);
    } else if (val is SynxStr && val.value.contains('\n')) {
      out.write(indent);
      out.write(k);
      out.write(' |\n');
      for (final line in val.value.split('\n')) {
        out.write(indent);
        out.write('  ');
        out.write(line);
        out.write('\n');
      }
    } else {
      out.write(indent);
      out.write(k);
      out.write(' ');
      out.write(formatPrimitive(val));
      out.write('\n');
    }
  }
}

String formatPrimitive(SynxValue v) => switch (v) {
      SynxStr(value: var s) => s,
      SynxInt(value: var n) => n.toString(),
      SynxFloat(value: var f) => () {
          if (f.isNaN || f.isInfinite) return 'null';
          var s = f.toString();
          if (!s.contains('.') && !s.contains('e') && !s.contains('E')) {
            s = '$s.0';
          }
          return s;
        }(),
      SynxBool(value: var b) => b ? 'true' : 'false',
      SynxNull() => 'null',
      SynxArr(values: var arr) =>
        '[${arr.map(formatPrimitive).join(', ')}]',
      SynxObj() => '[Object]',
      SynxSecret() => '[SECRET]',
    };
