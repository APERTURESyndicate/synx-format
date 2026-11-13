/// SYNX — native Dart parser. Parity with crates/synx-core 3.8.1.
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
  // safety: a document is an object, and the compiler writes no other root. A
  // file whose root was a string came back as that string — text of its
  // author's choosing, `!active` lines included, out of a file flagged static.
  // Such a root has no text form.
  if (pr.root is! SynxObj) return null;
  final sb = StringBuffer();
  if (pr.tool) sb.write('!tool\n');
  if (pr.schema) sb.write('!schema\n');
  if (pr.llm) sb.write('!llm\n');
  // A resolved file is the static document it now is: its values are final,
  // and `!active` would interpolate `{braces}` in them a second time.
  final resolved = bytes.length > 6 && (bytes[6] & 0x08) != 0;
  if (pr.mode == SynxMode.active && !resolved) sb.write('!active\n');
  if (pr.locked) sb.write('!lock\n');
  for (final inc in pr.includes) {
    sb.write('!include ${inc.path} ${inc.alias}\n');
  }
  if (sb.length > 0) sb.write('\n');
  sb.write(stringmod.stringify(pr.root));
  return sb.toString();
}

diffmod.SynxDiffResult diffObjects(SynxObject a, SynxObject b) =>
    diffmod.diff(a, b);
