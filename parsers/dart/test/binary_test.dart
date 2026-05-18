import 'package:synx/synx.dart';
import 'package:test/test.dart';

void main() {
  test('static roundtrip', () {
    final r = parseFull('name App\nport 8080\n');
    final compiled = compile('name App\nport 8080\n');
    expect(compiled.ok, isTrue, reason: compiled.error);
    expect(isSynxb(compiled.bytes!), isTrue);
    final restored = decompile(compiled.bytes!);
    expect(restored.ok, isTrue, reason: restored.error);
    expect(restored.result!.root, equals(r.root));
  });

  test('magic check', () {
    expect(isSynxb([0x53, 0x59, 0x4E, 0x58, 0x42, 1, 0]), isTrue);
    expect(isSynxb([0x4A, 0x53, 0x4F, 0x4E]), isFalse);
  });

  test('invalid magic rejected', () {
    final bad = List<int>.filled(11, 0);
    bad[0] = 'W'.codeUnitAt(0);
    bad[1] = 'R'.codeUnitAt(0);
    bad[2] = 'O'.codeUnitAt(0);
    bad[3] = 'N'.codeUnitAt(0);
    bad[4] = 'G'.codeUnitAt(0);
    final r = decompile(bad);
    expect(r.ok, isFalse);
  });
}
