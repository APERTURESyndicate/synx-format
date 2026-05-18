import 'package:synx/src/calc.dart';
import 'package:test/test.dart';

void main() {
  test('basic ops', () {
    expect(safeCalc('2 + 3').value, equals(5.0));
    expect(safeCalc('10 - 4').value, equals(6.0));
    expect(safeCalc('3 * 7').value, equals(21.0));
    expect(safeCalc('20 / 4').value, equals(5.0));
    expect(safeCalc('10 % 3').value, equals(1.0));
  });

  test('precedence and parens', () {
    expect(safeCalc('2 + 3 * 4').value, equals(14.0));
    expect(safeCalc('(2 + 3) * 4').value, equals(20.0));
  });

  test('negatives', () {
    expect(safeCalc('-5 + 3').value, equals(-2.0));
    expect(safeCalc('10 * -2').value, equals(-20.0));
  });

  test('div zero', () {
    final r = safeCalc('10 / 0');
    expect(r.ok, isFalse);
    expect(r.error, isNotEmpty);
  });

  test('empty', () {
    expect(safeCalc('').ok, isTrue);
    expect(safeCalc('').value, equals(0.0));
  });
}
