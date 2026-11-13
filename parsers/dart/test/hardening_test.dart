import 'dart:io';
import 'dart:typed_data';

import 'package:synx/synx.dart';
import 'package:synx/src/pattern.dart';
import 'package:test/test.dart';

/// Every expected value below is synx-core's answer for the same input
/// (`synx parse [--active]` of the working tree, environment as given).
String active(String body, [Map<String, String> env = const {}]) =>
    toJson(parseFullActive('!active\n$body', SynxOptions()..env = env).root);
String static_(String text) => toJson(synxObject(parse(text)));

Uint8List container(List<int> payload, {int? declared}) {
  final deflated = ZLibCodec(raw: true).encode(payload);
  final n = declared ?? payload.length;
  return Uint8List.fromList([
    ...'SYNXB'.codeUnits, 1, 0, n & 0xFF, (n >> 8) & 0xFF, (n >> 16) & 0xFF, (n >> 24) & 0xFF, ...deflated,
  ]);
}

List<int> varint(int v) {
  final out = <int>[];
  while (v >= 0x80) {
    out.add((v & 0x7F) | 0x80);
    v >>= 7;
  }
  out.add(v);
  return out;
}

void main() {
  group('parser', () {
    test('the block under a refused key goes with it (B-076)', () {
      expect(static_('user\n  profile\n    name x\n  __proto__\n    role admin\n    name y\n  age 3\n'),
          '{"user":{"age":3,"profile":{"name":"x"}}}');
      final r = parseFull('a 1\nprototype |\n  !active\n  b:env HOME\n  ###\nc 2\n');
      expect(r.mode, SynxMode.static_);
      expect(toJson(r.root), '{"a":1,"c":2}');
    });

    test('a key without a value ending an item keeps the items after it (B-058)', () {
      expect(static_('servers\n  - name a\n    tags\n  - name b\n    tags\n  - name c\n    port 1\n'),
          '{"servers":[{"name":"a","tags":[]},{"name":"b","tags":[]},{"name":"c","port":1}]}');
    });

    test('a comment line is no content (B-043)', () {
      expect(static_('k\n# note\n  - a\n  - b\nm\n  - x 1\n    # c\n    y 2\n'), '{"k":["a","b"],"m":[{"x":1,"y":2}]}');
    });

    test('the key on a dash line keeps its markers and constraints (B-024)', () {
      expect(active('users\n  - password:secret hunter2\n    name bob\n  - host:env H\n    port[min:10] 1\n', {'H': 'h1'}),
          '{"users":[{"name":"bob","password":"[SECRET]"},'
          '{"host":"h1","port":"CONSTRAINT_ERR: \'port\' value 1 is below min 10"}]}');
    });

    test('a megabyte of digits reads in linear time (B-079)', () {
      final digits = '9' * (1 << 20);
      final sw = Stopwatch()..start();
      final r = parse('x $digits\ny(int) $digits\nz(float) $digits\n');
      expect(sw.elapsedMilliseconds, lessThan(5000));
      expect(r['x']!.asString, digits);
      expect(r['y']!.asInt, 0);
      expect(r['z']!.asFloat, double.infinity);
    });
  });

  group('secrets', () {
    test('a secret is masked before its turn, in groups and lists too (B-017, B-026)', () {
      final out = active('leak:alias password\npassword:secret:default:x hunter2\na\n  x {password}\n'
          'keys:secret\n  - hunter2\n  - other\ncreds:secret\n  user bob\n  pass hunter2\ns:secret\n');
      expect(out,
          '{"a":{"x":"[SECRET]"},"creds":{"pass":"[SECRET]","user":"[SECRET]"},"keys":["[SECRET]","[SECRET]"],'
          '"leak":"[SECRET]","password":"[SECRET]","s":{}}');
    });

    test('an unset secret is null, and required sees it (T3-02, S1-01)', () {
      expect(active('k[required]:secret:env SYNX_UNSET_ZZ\nname[required, min:1]\nopt:secret:env SYNX_UNSET_ZZ\npin:secret:env PIN\n', {'PIN': '00123'}),
          '{"k":"CONSTRAINT_ERR: \'k\' is required","name":"CONSTRAINT_ERR: \'name\' is required","opt":null,"pin":"[SECRET]"}');
      final pin = parseFullActive('!active\npin:secret:env PIN\n', SynxOptions()..env = {'PIN': '00123'}).root.asObject!['pin'];
      expect(pin!.asSecret, '00123');
    });
  });

  group('numbers', () {
    test('inf, nan and out-of-range results stay what they are (B-005.4, B-010, B-015, E5–E7)', () {
      expect(
          active('x(float):round nan\ny(float):round inf\nz:round 1${'0' * 30}.5\np[max:-inf] 5\nq[min:nan] 5\n'
              'a:sum\n  - 1\n  - inf\nb:sum\n  - nan\n  - 2\nh:env HOST\nw:random inf\ns:sort\n  - b\n  - 10\n  - 1a\n  - 9\n  - nan\n',
              {'HOST': 'inf'}),
          '{"a":1,"b":2,"h":"inf","p":5,"q":5,"s":[9,10,"1a","b","nan"],"w":"inf","x":null,"y":null,"z":1e30}');
    });

    test(':ref:calc with 20 000 parentheses does not recurse', () {
      expect(active('base 2\nx:ref:calc:*${'(' * 20000}1${')' * 20000} base\n'), '{"base":2,"x":2}');
    });
  });

  group('limits', () {
    test('copies and interpolation stop at the resolution budget (B-030, E8)', () {
      final sb = StringBuffer('a0 ${'x' * 1000}\n');
      for (var i = 0; i < 28; i++) {
        sb.write('a${i + 1} {a$i}{a$i}\n');
      }
      final root = parseFullActive('!active\n$sb', SynxOptions()..env = {}).root.asObject!;
      expect(root['a12']!.asString!.length, 4096000);
      expect(root['a26']!.asString!.length, 4194304);
      expect(root['a27']!.asString, 'LIMIT_ERR: resolution budget exceeded');
      expect(root['a28']!.asString, 'LIMIT_ERR: resolution budget exceeded');
    });

    test('a file including itself stops at the first repeat', () {
      final dir = Directory.systemTemp.createTempSync('synx-cycle-');
      try {
        File('${dir.path}/a.synx').writeAsStringSync('!active\nb:include b.synx\n');
        File('${dir.path}/b.synx').writeAsStringSync('!active\na:include a.synx\nv 1\n');
        final out = toJson(synxObject(parseActive(File('${dir.path}/a.synx').readAsStringSync(), SynxOptions()..basePath = dir.path)));
        expect(out, '{"b":{"a":{"b":"INCLUDE_ERR: circular include: \'b.synx\'"},"v":1}}');
      } finally {
        dir.deleteSync(recursive: true);
      }
    });

    test('a junction inside the base that leads out is an escape (S5-07)', () {
      if (!Platform.isWindows) return;
      final root = Directory.systemTemp.createTempSync('synx-jail-');
      try {
        final outside = Directory('${root.path}/outside')..createSync();
        File('${outside.path}/secret.synx').writeAsStringSync('token hunter2\n');
        final base = Directory('${root.path}/base')..createSync();
        Link('${base.path}/inner').createSync(outside.path);
        final out = toJson(synxObject(parseActive('!active\nx:include inner/secret.synx\n', SynxOptions()..basePath = base.path)));
        expect(out, '{"x":"INCLUDE_ERR: SECURITY: path escapes base directory: \'inner/secret.synx\'"}');
      } finally {
        root.deleteSync(recursive: true);
      }
    });
  });

  group('patterns', () {
    test('the dialect is the regex crate\'s (B-048)', () {
      expect(
          active('r[pattern:{] y\nc[pattern:(?i)^abc\$] ABC\nd[pattern:(?i)^abc\$] x\ne[pattern:^\\p{L}+\$] мир\n'
              'f[pattern:^\\d+\$] ١٢٣\ng[pattern:\\bfoo\\b] food\n'),
          '{"c":"ABC","d":"CONSTRAINT_ERR: \'d\' does not match pattern /(?i)^abc\$/","e":"мир","f":"١٢٣",'
          '"g":"CONSTRAINT_ERR: \'g\' does not match pattern /\\\\bfoo\\\\b/","r":"y"}');
    });

    test('a pattern that backtracks elsewhere answers in linear time (B-079)', () {
      final re = SynxPattern.compile(r'^(a|aa)+$')!;
      final sw = Stopwatch()..start();
      expect(re.hasMatch('${'a' * 5000}!'), isFalse);
      expect(re.hasMatch('a' * 5000), isTrue);
      expect(sw.elapsedMilliseconds, lessThan(5000));
      expect(SynxPattern.compile('(?=a)'), isNull);
      expect(SynxPattern.compile(r'\1'), isNull);
      expect(SynxPattern.compile(r'(?i)\p{Greek}+')!.hasMatch('ΑΒΓ'), isTrue);
    });
  });

  group('.synxb', () {
    test('a container declaring 256 MiB with an empty stream is refused (Y2-07)', () {
      final bytes = Uint8List.fromList([...'SYNXB'.codeUnits, 1, 0, 0, 0, 0, 0x10, 0x03, 0x00]);
      final r = decompile(bytes);
      expect(r.ok, isFalse);
    });

    test('one long string referenced many times is refused at the budget (B-038)', () {
      final text = List<int>.filled(120 * 1024, 0x78);
      final payload = <int>[...varint(1), ...varint(text.length), ...text, 0x06, ...varint(6000)];
      for (var i = 0; i < 6000; i++) {
        payload.addAll([0x05, 0]);
      }
      final r = decompile(container(payload));
      expect(r.ok, isFalse);
      expect(r.error, contains('refusing to decode'));
    });

    test('a stream that inflates past its declared size is refused before it is inflated', () {
      // 256 MiB of zeros deflate to a quarter of a megabyte.
      final bytes = container(Uint8List(256 << 20), declared: 100);
      final before = ProcessInfo.currentRss;
      final r = decompile(bytes);
      final grown = ProcessInfo.currentRss - before;
      expect(r.ok, isFalse);
      expect(grown, lessThan(64 << 20));
    });
  });

  group('writers', () {
    test('a .synxb whose root is not an object has no text form (B-086)', () {
      // String table ["!active\nx:env PATH"], root: that string.
      const text = '!active\nx:env PATH';
      final bytes = container([...varint(1), ...varint(text.length), ...text.codeUnits, 0x05, 0]);
      expect(decompile(bytes).ok, isTrue);
      expect(decompileToText(bytes), isNull);
    });

    test('a refused key is written under a name the reader keeps', () {
      final o = SynxObject()..set('__proto__', synxString('v'));
      expect(stringify(synxObject(o)), '__proto___ v\n');
      expect(toJson(synxObject(parse(stringify(synxObject(o))))), '{"__proto___":"v"}');
    });

    test('a key that would read back as something else is renamed (B-021)', () {
      final o = SynxObject()
        ..set('a\n!active\nleak:env PATH', synxString('v'))
        ..set('a b', synxInt(1))
        ..set('a_b', synxInt(2))
        ..set('a:b', synxInt(3));
      final text = stringify(synxObject(o));
      expect(text, 'a_!active_leak_env_PATH v\na_b_2 1\na_b_3 3\na_b 2\n');
      final back = parseFull(text);
      expect(back.mode, SynxMode.static_);
      expect(toJson(back.root), '{"a_!active_leak_env_PATH":"v","a_b":2,"a_b_2":1,"a_b_3":3}');
    });
  });
}
