import 'package:synx/synx.dart';
import 'package:test/test.dart';

void main() {
  test('parse simple kv', () {
    final r = parseFull('name Wario\nage 30\nactive true\nscore 99.5\nempty null');
    final o = (r.root as SynxObj).map;
    expect(o['name'], equals(synxString('Wario')));
    expect(o['age'], equals(synxInt(30)));
    expect(o['active'], equals(synxBool(true)));
    expect(o['score'], equals(synxFloat(99.5)));
    expect(o['empty']!.isNull, isTrue);
    expect(r.mode, equals(SynxMode.static_));
  });

  test('parse nested objects', () {
    final r = parseFull('server\n  host 0.0.0.0\n  port 8080\n  ssl\n    enabled true');
    final o = (r.root as SynxObj).map;
    final server = (o['server'] as SynxObj).map;
    expect(server['port'], equals(synxInt(8080)));
    final ssl = (server['ssl'] as SynxObj).map;
    expect(ssl['enabled'], equals(synxBool(true)));
  });

  test('parse lists', () {
    final r = parseFull('inventory\n  - Sword\n  - Shield\n  - Potion');
    final o = (r.root as SynxObj).map;
    final arr = (o['inventory'] as SynxArr).values;
    expect(arr.length, equals(3));
  });

  test('parse multiline block', () {
    final r = parseFull('rules |\n  Rule one.\n  Rule two.\n  Rule three.');
    final o = (r.root as SynxObj).map;
    final s = (o['rules'] as SynxStr).value;
    expect(s.contains('\n'), isTrue);
  });

  test('parse active metadata', () {
    final r = parseFull('!active\nprice 100\ntax:calc price * 0.2');
    expect(r.mode, equals(SynxMode.active));
    expect(r.metadata['']?['tax']?.markers, equals(['calc']));
  });

  test('parse prototype-pollution rejected', () {
    final r = parseFull('__proto__ evil\nconstructor evil\nprototype evil\nname safe\n');
    final o = (r.root as SynxObj).map;
    expect(o.contains('__proto__'), isFalse);
    expect(o.contains('constructor'), isFalse);
    expect(o.contains('prototype'), isFalse);
    expect(o.contains('name'), isTrue);
  });

  test('parse constraints', () {
    final r = parseFull('!active\nname[min:3, max:30, required] Wario');
    final c = r.metadata['']?['name']?.constraints;
    expect(c?.min, equals(3.0));
    expect(c?.max, equals(30.0));
    expect(c?.required, isTrue);
  });

  test('type hint string keeps string', () {
    final r = parseFull('zip(string) 90210');
    final o = (r.root as SynxObj).map;
    expect(o['zip'], equals(synxString('90210')));
  });
}
