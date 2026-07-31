import 'package:synx/synx.dart';
import 'package:test/test.dart';

/// SYNX 3.7 §8.4.1 — the `|+` indent-preserving multiline opener.
///
/// These exist because 3.7.0 shipped without them: this parser did not even
/// compile under `|+` (string methods were being called on the line's byte
/// list) and nothing caught it, since only Rust, JS and Go covered the
/// operator.
String _str(String src, String key) {
  final r = parseFull(src);
  return ((r.root as SynxObj).map[key] as SynxStr).value;
}

void main() {
  test('|+ preserves indent relative to the first continuation line', () {
    // The worked example from the normative text.
    final v = _str(
      'prompt |+\n'
      '  Outline:\n'
      '    - step one\n'
      '    - step two\n'
      '      sub-step\n'
      '  End.\n',
      'prompt',
    );
    expect(v, equals('Outline:\n  - step one\n  - step two\n    sub-step\nEnd.'));
  });

  test('plain | still trims every continuation line', () {
    final v = _str(
      'prompt |\n'
      '  Outline:\n'
      '    - step one\n'
      '  End.\n',
      'prompt',
    );
    expect(v, equals('Outline:\n- step one\nEnd.'));
  });

  test('base indent is locked on the first non-empty continuation line', () {
    // The base comes from the first content line (4 spaces); the shallower line
    // after it loses all indentation and no padding is invented.
    final v = _str('k |+\n    deep\n  shallow\n', 'k');
    expect(v, equals('deep\nshallow'));
  });

  test('trailing whitespace is stripped per line', () {
    final v = _str('k |+\n  a   \n  b\t\n', 'k');
    expect(v, equals('a\nb'));
  });

  test('block ends at the opener indent', () {
    final r = parseFull('k |+\n  body\nother value\n');
    final o = (r.root as SynxObj).map;
    expect((o['k'] as SynxStr).value, equals('body'));
    expect(o['other'], equals(synxString('value')));
  });

  test('non-ASCII survives the slice', () {
    // The slice is taken at a byte offset, so a multi-byte character right
    // after the stripped indent is exactly what breaks when the offset is
    // applied to the wrong unit — which is how this parser was broken.
    final v = _str('k |+\n  Привет\n    мир\n', 'k');
    expect(v, equals('Привет\n  мир'));
  });
}
