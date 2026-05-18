import 'dart:io';

import 'package:synx/synx.dart';
import 'package:test/test.dart';

/// Replay every `.synx` in the shared corpus through this parser.
/// Skips when the corpus directory is absent.
void main() {
  test('corpus parses without error', () {
    final dir = _findCorpus();
    if (dir == null) {
      // Skip silently when corpus isn't bundled.
      return;
    }
    final files = Directory(dir)
        .listSync()
        .whereType<File>()
        .where((f) => f.path.endsWith('.synx'));
    var parsed = 0;
    var failed = 0;
    for (final f in files) {
      final text = f.readAsStringSync();
      final r = parseFull(text);
      if (r.root is SynxObj) {
        parsed++;
      } else {
        failed++;
        print('corpus ${f.path} did not yield an object');
      }
    }
    print('[corpus] parsed $parsed files, $failed failed');
    expect(failed, equals(0));
  });
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
