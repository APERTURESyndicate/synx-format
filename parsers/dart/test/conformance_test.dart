import 'dart:convert';
import 'dart:io';

import 'package:synx/synx.dart';
import 'package:synx/src/engine.dart' show resolve;
import 'package:test/test.dart';

/// Replay every `.synx` case of the shared corpus that has a sibling
/// `.expected.json` and compare the canonical JSON byte for byte, exactly like
/// the Rust runner. Skips when the corpus directory is absent.
void main() {
  final dir = _findCorpus();

  test('corpus is present with at least 24 cases', () {
    if (dir == null) {
      markTestSkipped('conformance corpus not present in this checkout');
      return;
    }
    expect(_cases(dir).length, greaterThanOrEqualTo(24));
  });

  if (dir == null) return;

  for (final synxFile in _cases(dir)) {
    final name = synxFile.uri.pathSegments.last;
    test('conformance $name', () {
      final expectedFile = File(
          '${synxFile.path.substring(0, synxFile.path.length - '.synx'.length)}.expected.json');
      final input = _readUtf8KeepingBom(synxFile);
      final expected = utf8.decode(expectedFile.readAsBytesSync()).trim();
      expect(_conformanceJson(input), expected,
          reason: 'conformance FAIL: $name');
    });
  }
}

/// `!tool` documents go through [parseTool]; everything else through the plain
/// parse, resolved with default options when the document is `!active`.
String _conformanceJson(String input) {
  if (input.trimLeft().startsWith('!tool')) {
    return toJson(synxObject(parseTool(input)));
  }
  final r = parseFull(input);
  if (r.mode == SynxMode.active) resolve(r, SynxOptions());
  return toJson(r.root);
}

/// `utf8.decode` silently drops a leading BOM; stripping it is the parser's
/// job, so put it back before handing the text over.
String _readUtf8KeepingBom(File f) {
  final bytes = f.readAsBytesSync();
  final hasBom = bytes.length >= 3 &&
      bytes[0] == 0xEF &&
      bytes[1] == 0xBB &&
      bytes[2] == 0xBF;
  final text = utf8.decode(hasBom ? bytes.sublist(3) : bytes);
  return hasBom ? '${String.fromCharCode(0xFEFF)}$text' : text;
}

List<File> _cases(String dir) {
  final files = Directory(dir)
      .listSync()
      .whereType<File>()
      .where((f) => f.path.endsWith('.synx'))
      .where((f) => File(
              '${f.path.substring(0, f.path.length - '.synx'.length)}.expected.json')
          .existsSync())
      .toList()
    ..sort((a, b) => a.path.compareTo(b.path));
  return files;
}

String? _findCorpus() {
  final candidates = [
    'tests/conformance/cases',
    '../tests/conformance/cases',
    '../../tests/conformance/cases',
    '../../../tests/conformance/cases',
  ];
  for (final c in candidates) {
    final d = Directory(c);
    if (d.existsSync()) return d.absolute.path;
  }
  return null;
}
