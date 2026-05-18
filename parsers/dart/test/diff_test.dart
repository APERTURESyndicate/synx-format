import 'package:synx/synx.dart';
import 'package:test/test.dart';

void main() {
  test('identical', () {
    final a = SynxObject()
      ..set('x', synxInt(1))
      ..set('y', synxInt(2));
    final b = SynxObject()
      ..set('x', synxInt(1))
      ..set('y', synxInt(2));
    final d = diffObjects(a, b);
    expect(d.added.isEmpty, isTrue);
    expect(d.removed.isEmpty, isTrue);
    expect(d.changed, isEmpty);
    expect(d.unchanged.length, equals(2));
  });

  test('added/removed', () {
    final a = SynxObject()..set('x', synxInt(1));
    final b = SynxObject()..set('y', synxInt(2));
    final d = diffObjects(a, b);
    expect(d.added.length, equals(1));
    expect(d.removed.length, equals(1));
  });

  test('changed', () {
    final a = SynxObject()..set('name', synxString('Alice'));
    final b = SynxObject()..set('name', synxString('Bob'));
    final d = diffObjects(a, b);
    expect(d.changed.length, equals(1));
    expect(d.changed.first.key, equals('name'));
  });
}
