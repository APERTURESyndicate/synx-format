import 'dart:io' show Platform;

import 'package:synx/synx.dart';
import 'package:synx/src/engine.dart' show resolve;
import 'package:test/test.dart';

/// SYNX 3.7.2 - 3.7.4 behaviour ported from the Rust reference. Expected
/// strings come from `synx parse` (crates/synx-core).
final String _bom = String.fromCharCode(0xFEFF);
final String _nbsp = String.fromCharCode(0xA0);

SynxObject _resolved(String src, [Map<String, String>? env]) {
  final r = parseFull(src);
  resolve(r, SynxOptions()..env = env);
  return (r.root as SynxObj).map;
}

void main() {
  group('parser', () {
    test('leading BOM does not hide !active or glue itself to the first key', () {
      final r = parseFull('$_bom!active\nname synx\n');
      expect(r.mode, SynxMode.active);
      expect((r.root as SynxObj).map.keys, equals(['name']));
      expect(parse('${_bom}name synx\n')['name'], equals(synxString('synx')));
    });

    test('a mid-document U+FEFF is not whitespace (Rust str::trim keeps it)', () {
      final o = parse('k2 v$_bom\n$_bom\nk v\n');
      expect(o['k2'], equals(synxString('v$_bom')));
      expect(o.keys, contains(_bom));
      expect(o['k'], equals(synxString('v')));
    });

    test('block body is raw text: comments, ### and directives are content', () {
      final o = parse('code |+\n  # keep\n  ### fence\n  !active\nnext 1\n');
      expect(o['code'], equals(synxString('# keep\n### fence\n!active')));
      expect(o['next'], equals(synxInt(1)));
    });

    test('a value that merely starts with (word) is plain text', () {
      final o = parse('a (aa)\nb (int)5\nc (random:bool)\ne (aa) tail\nf[enum:x|y] (x)\n'
          'g(aa) 5\nh(int)\ni(int)\n  child 1\nj(string) |\n  kept\n'
          'list\n  - (int)5\n  - (aa)\n  - m(int) 5\n    n 1\n');
      expect(
        toJson(synxObject(o)),
        '{"a":"(aa)","b":"(int)5","c":"(random:bool)","e":"(aa) tail","f":"(x)","g":5,'
        '"h":{},"i":{"child":1},"j":"kept","list":["(int)5","(aa)",{"m":5,"n":1}]}',
      );
    });

    test('type hint applies only when attached to the key', () {
      final o = parse('a (int) 5\nb(int) 5\n');
      expect(o['a'], equals(synxString('(int) 5')));
      expect(o['b'], equals(synxInt(5)));
    });

    test('|+ strips the base indent on a character boundary', () {
      final o = parse('body |+\n$_nbsp$_nbsp'
          'a\n$_nbsp$_nbsp$_nbsp'
          'b\nafter 1\n');
      expect(o['body'], equals(synxString('a\n${_nbsp}b')));
      expect(o['after'], equals(synxInt(1)));
    });
  });

  group('json', () {
    test('floats use the ryu layout', () {
      final cases = <double, String>{
        3.14: '3.14',
        100: '100.0',
        0.0001: '0.0001',
        0.00001: '0.00001',
        0.000001: '1e-6',
        0.000123: '0.000123',
        1234567890123456.5: '1234567890123456.5',
        12345678901234567.0: '1.2345678901234568e16',
        123456789012345678901.5: '1.2345678901234568e20',
        -2.5: '-2.5',
        1e21: '1e21',
      };
      cases.forEach((f, want) {
        expect(toJson(synxFloat(f)), want, reason: 'float $f');
      });
    });
  });

  group('engine', () {
    const snow = '1550184245153112174';

    test(':env with [type:string] keeps text, plain :env casts', () {
      final o = _resolved(
        '!active\nid[type:string]:env SNOW\nzip[type:string]:env ZIP\n'
        'fallback[type:string]:env:default:0042 MISSING\nplain:env ZIP\n',
        {'SNOW': snow, 'ZIP': '007'},
      );
      expect(o['id'], equals(synxString(snow)));
      expect(o['zip'], equals(synxString('007')));
      expect(o['fallback'], equals(synxString('0042')));
      expect(o['plain'], equals(synxInt(7)));
    });

    test('[type:string] accepts a :secret', () {
      final o = _resolved('!active\ntoken[type:string]:secret:env SNOW\n', {'SNOW': snow});
      expect(o['token'], equals(synxSecret(snow)));
    });

    test(':env keeps 64-bit integers exact and strips quotes', () {
      final o = _resolved('!active\nraw:env SNOW\nq:env Q\n', {'SNOW': snow, 'Q': '"12"'});
      expect(o['raw'], equals(synxInt(1550184245153112174)));
      expect(o['q'], equals(synxString('12')));
    });

    test('standalone :default honours [type:string]', () {
      final o = _resolved('!active\na[type:string]:default:007 null\nb:default:007 null\n', {});
      expect(o['a'], equals(synxString('007')));
      expect(o['b'], equals(synxInt(7)));
    });

    test(':env:default rejoins the colon-split fallback', () {
      final o = _resolved('!active\nhost:env:default:0.0.0.0 MISSING\n', {});
      expect(o['host'], equals(synxString('0.0.0.0')));
    });

    test(':env with no env map reads the process environment', () {
      final r = parseFull('!active\nv:env PATH\n');
      resolve(r, SynxOptions());
      final got = (r.root as SynxObj).map['v'];
      final path = Platform.environment['PATH'] ?? Platform.environment['Path'];
      expect(path, isNotNull);
      expect(got, isA<SynxStr>());
      expect((got as SynxStr).value, path);
      // An explicit empty map means "no variables".
      expect(_resolved('!active\nv:env PATH\n', {})['v'], equals(synxNull()));
    });

    test('env/string matrix matches the Rust reference', () {
      final o = _resolved(
        '!active\nz:env ZIP\nzs[type:string]:env ZIP\nid[type:string]:env SNOW\n'
        'raw:env SNOW\nd[type:string]:env:default:007 MISSING\n'
        'sec[type:string]:secret:env SNOW\nq:env Q\n',
        {'ZIP': '007', 'SNOW': snow, 'Q': '"12"'},
      );
      expect(
        toJson(synxObject(o)),
        '{"d":"007","id":"1550184245153112174","q":"12","raw":1550184245153112174,'
        '"sec":"[SECRET]","z":7,"zs":"007"}',
      );
    });

    test('interpolating a :secret yields the mask, at any depth', () {
      final o = _resolved(
        '!active\ntoken:secret abc123\nmsg hello {token} world\nsection\n  inner use {token} now\n',
      );
      expect(toJson(synxObject(o)).contains('abc123'), isFalse);
      expect(o['msg'], equals(synxString('hello [SECRET] world')));
      final section = (o['section'] as SynxObj).map;
      expect(section['inner'], equals(synxString('use [SECRET] now')));
    });

    test('interpolation keeps non-ASCII literal text', () {
      final o = _resolved('!active\nname Wario\nmsg мир {name} 東京\n');
      expect(o['msg'], equals(synxString('мир Wario 東京')));
    });

    test('a nested-quantifier pattern is skipped and the value left unchanged', () {
      final value = '${'a' * 45}!';
      final sw = Stopwatch()..start();
      final o = _resolved('!active\nv[pattern:^(a+)+\$] $value\nw[pattern:^(a|b)*c\$] abab\n', {});
      expect(sw.elapsedMilliseconds, lessThan(2000));
      expect(o['v'], equals(synxString(value)));
      expect(o['w'],
          equals(synxString("CONSTRAINT_ERR: 'w' does not match pattern /^(a|b)*c\$/")));
    });

    test('constraint messages print numbers like Rust f64 Display', () {
      final o = _resolved('!active\nf[min:0.1] 0.05\nbig[max:70000] 70001\n', {});
      expect(o['f'], equals(synxString("CONSTRAINT_ERR: 'f' value 0.05 is below min 0.1")));
      expect(o['big'],
          equals(synxString("CONSTRAINT_ERR: 'big' value 70001 exceeds max 70000")));
    });

    test('field registry merges declarations across the document', () {
      final o = _resolved(
          '!active\na\n  port[min:10, max:100] 50\nb\n  port[min:60] 70\nc\n  port 55\n', {});
      expect(((o['a'] as SynxObj).map)['port'],
          equals(synxString("CONSTRAINT_ERR: 'port' value 50 is below min 60")));
      expect(((o['b'] as SynxObj).map)['port'], equals(synxInt(70)));
      expect(((o['c'] as SynxObj).map)['port'],
          equals(synxString("CONSTRAINT_ERR: 'port' value 55 is below min 60")));
    });

    test('interpolation resolves dotted paths and leaves loose braces alone', () {
      final o = _resolved(
        '!active\nserver\n  host localhost\n  port 8080\n'
        'url http://{server.host}:{server.port}/api\nloose { server.host } {nope}\n',
      );
      expect(o['url'], equals(synxString('http://localhost:8080/api')));
      expect(o['loose'], equals(synxString('{ server.host } {nope}')));
    });
  });
}
