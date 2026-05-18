import 'package:synx/synx.dart';
import 'package:test/test.dart';

void main() {
  test('object set/get/remove', () {
    final o = SynxObject();
    o.set('a', synxInt(1));
    o.set('b', synxString('two'));
    expect(o['a'], equals(synxInt(1)));
    expect(o.contains('b'), isTrue);
    expect(o.remove('a'), isTrue);
    expect(o.contains('a'), isFalse);
  });

  test('object equality is order-insensitive', () {
    final a = SynxObject()
      ..set('x', synxInt(1))
      ..set('y', synxInt(2));
    final b = SynxObject()
      ..set('y', synxInt(2))
      ..set('x', synxInt(1));
    expect(synxObject(a), equals(synxObject(b)));
  });

  test('type helpers', () {
    expect(synxNull().isNull, isTrue);
    expect(synxInt(5).asInt, equals(5));
    expect(synxFloat(3.14).asDouble, equals(3.14));
    expect(synxString('x').asInt, isNull);
  });
}
