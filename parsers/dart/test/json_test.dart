import 'package:synx/synx.dart';
import 'package:test/test.dart';

void main() {
  test('primitives', () {
    expect(toJson(synxNull()), equals('null'));
    expect(toJson(synxBool(true)), equals('true'));
    expect(toJson(synxInt(42)), equals('42'));
    expect(toJson(synxString('hi')), equals('"hi"'));
  });

  test('secret redacted', () {
    expect(toJson(synxSecret('xxx')), equals('"[SECRET]"'));
  });

  test('object sorted keys', () {
    final o = SynxObject()
      ..set('b', synxInt(2))
      ..set('a', synxInt(1));
    final j = toJson(synxObject(o));
    final pa = j.indexOf('"a"');
    final pb = j.indexOf('"b"');
    expect(pa, greaterThanOrEqualTo(0));
    expect(pb, greaterThan(pa));
  });

  test('escapes', () {
    final j = toJson(synxString('line\nbreak\ttab"quote\\back'));
    expect(j, contains(r'\n'));
    expect(j, contains(r'\t'));
    expect(j, contains(r'\"'));
    expect(j, contains(r'\\'));
  });
}
