/// SYNX — native Dart parser. Parity with crates/synx-core 3.6.x.
library synx;

import 'src/binary.dart' as binmod;
import 'src/diff.dart' as diffmod;
import 'src/engine.dart' as enginemod;
import 'src/formatter.dart' as formatmod;
import 'src/json.dart' as jsonmod;
import 'src/meta.dart';
import 'src/options.dart';
import 'src/parser.dart' as parsermod;
import 'src/stringify.dart' as stringmod;
import 'src/value.dart';

export 'src/binary.dart'
    show
        SynxBinaryError,
        SynxCompileResult,
        SynxDecompileResult,
        isSynxb;
export 'src/diff.dart'
    show
        SynxDiffChange,
        SynxDiffEntry,
        SynxDiffResult,
        diff,
        diffToValue;
export 'src/meta.dart';
export 'src/options.dart';
export 'src/value.dart';

/// Static parse — top-level object only.
SynxObject parse(String text) {
  final r = parsermod.parse(text);
  final root = r.root;
  return root is SynxObj ? root.map : SynxObject();
}

/// Parse with `!active` resolution applied.
SynxObject parseActive(String text, [SynxOptions? opts]) {
  final r = parsermod.parse(text);
  if (r.mode == SynxMode.active) {
    enginemod.resolve(r, opts ?? SynxOptions());
  }
  final root = r.root;
  return root is SynxObj ? root.map : SynxObject();
}

/// Full ParseResult (mode, metadata, includes, …).
SynxParseResult parseFull(String text) => parsermod.parse(text);

SynxParseResult parseFullActive(String text, [SynxOptions? opts]) {
  final r = parsermod.parse(text);
  if (r.mode == SynxMode.active) {
    enginemod.resolve(r, opts ?? SynxOptions());
  }
  return r;
}

/// Parse a `!tool` envelope: `{ tool, params }` or `{ tools: [...] }`.
SynxObject parseTool(String text, [SynxOptions? opts]) {
  final r = parsermod.parse(text);
  if (r.mode == SynxMode.active) {
    enginemod.resolve(r, opts ?? SynxOptions());
  }
  final shaped = parsermod.reshapeToolOutput(r.root, r.schema);
  return shaped is SynxObj ? shaped.map : SynxObject();
}

String toJson(SynxValue v) => jsonmod.toJson(v);

String stringify(SynxValue v) => stringmod.stringify(v);

String format(String text) => formatmod.format(text);

binmod.SynxCompileResult compile(String text, {bool resolved = false}) {
  final r = parsermod.parse(text);
  if (resolved && r.mode == SynxMode.active) {
    enginemod.resolve(r, SynxOptions());
  }
  return binmod.compile(r, resolved);
}

binmod.SynxDecompileResult decompile(List<int> bytes) => binmod.decompile(bytes);

String? decompileToText(List<int> bytes) {
  final res = binmod.decompile(bytes);
  if (!res.ok) return null;
  final pr = res.result!;
  final sb = StringBuffer();
  if (pr.tool) sb.write('!tool\n');
  if (pr.schema) sb.write('!schema\n');
  if (pr.llm) sb.write('!llm\n');
  if (pr.mode == SynxMode.active) sb.write('!active\n');
  if (pr.locked) sb.write('!lock\n');
  if (sb.length > 0) sb.write('\n');
  sb.write(stringmod.stringify(pr.root));
  return sb.toString();
}

diffmod.SynxDiffResult diffObjects(SynxObject a, SynxObject b) =>
    diffmod.diff(a, b);
