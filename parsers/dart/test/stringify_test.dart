import 'package:synx/synx.dart';
import 'package:test/test.dart';

void main() {
  test('basic roundtrip', () {
    final r = parseFull('active true\nage 30\nname Wario\n');
    final out = stringify(r.root);
    expect(out, contains('name Wario'));
    expect(out, contains('age 30'));
    expect(out, contains('active true'));
  });

  test('multiline uses pipe', () {
    final o = SynxObject()..set('rules', synxString('a\nb\nc'));
    final out = stringify(synxObject(o));
    expect(out, contains('rules |'));
  });

  test('formatter sorts keys', () {
    final out = format('b 2\na 1\nc 3\n');
    final a = out.indexOf('a 1');
    final b = out.indexOf('b 2');
    expect(a, greaterThanOrEqualTo(0));
    expect(b, greaterThan(a));
  });

  test('formatter preserves directive', () {
    expect(format('!active\nname X\n'), startsWith('!active'));
  });
}
