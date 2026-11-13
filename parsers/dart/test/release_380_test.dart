import 'dart:io';

import 'package:synx/synx.dart';
import 'package:test/test.dart';

/// SYNX 3.8.0 parity: every expected value below is synx-core's output for the same input.
String active(String body) => toJson(parseFullActive('!active\n$body').root);
String static_(String text) => toJson(synxObject(parse(text)));

void main() {
  group('active mode', () {
    test('markers resolve in document order at any depth', () {
      expect(
          active('price 100\ntax:calc price * 0.2\ntotal:calc price + tax\nlimit 5\nsvc\n  limit 100\n'
              '  price:calc limit * 2\n  note Total is {total}\n'),
          '{"limit":5,"price":100,"svc":{"limit":100,"note":"Total is 120","price":200},"tax":20,"total":120}');
    });

    test('calc reaches special keys and local shadows root', () {
      expect(
          active('price 100\nцена 7\nmax-hp 10\nhp 3\nleft:calc max-hp - hp\nboth:calc цена * hp\nn.m 4\n'
              'prod:calc n.m * hp\nsvc\n  hp 50\n  x:calc hp * 2\n  y:calc price + hp\n2 5\nz:calc 2 * 2\ninf 1\nq:calc inf + 1\n'),
          '{"2":5,"both":21,"hp":3,"inf":1,"left":7,"max-hp":10,"n.m":4,"price":100,"prod":12,"q":2,'
          '"svc":{"hp":50,"x":250,"y":150},"z":25,"цена":7}');
    });

    test('calc errors read as in synx-core', () {
      expect(
          active('x:calc 1 / 0\nz:calc (2+3)*4\nw:calc 1.2.3 + 1\nv:calc -\ns:calc цена * 2\nn:calc )\nm:calc 5 5\n'
              't:calc (1 + 2\nr:calc -7 % 3\n'),
          '{"m":"CALC_ERR: SYNX :calc — unexpected token at position 1","n":"CALC_ERR: SYNX :calc — unexpected token: \')\'",'
          '"r":-1,"s":"CALC_ERR: SYNX :calc — unexpected character: \'ц\' in expression",'
          '"t":"CALC_ERR: SYNX :calc — missing closing parenthesis","v":"CALC_ERR: SYNX :calc — invalid number: \'-\'",'
          '"w":"CALC_ERR: SYNX :calc — invalid number: \'1.2.3\'","x":"CALC_ERR: SYNX :calc — division by zero","z":20}');
    });

    test('every list item keeps its own declarations', () {
      expect(
          active('servers\n  - name a\n    port:env:default:7001 SYNX_UNSET_Q\n  - name b\n    port 8080\n'
              '  - name c\n    port[type:int, max:9] 77\n  - name d\n    port 5\n'),
          '{"servers":[{"name":"a","port":"CONSTRAINT_ERR: \'port\' value 7001 exceeds max 9"},'
          '{"name":"b","port":"CONSTRAINT_ERR: \'port\' value 8080 exceeds max 9"},'
          '{"name":"c","port":"CONSTRAINT_ERR: \'port\' value 77 exceeds max 9"},{"name":"d","port":5}]}');
      expect(parseFull('!active\nservers\n  - name a\n    port:env X\n').metadata.containsKey('servers.#0'), isTrue);
    });

    test('dotted key and nested key never share declarations', () {
      expect(active('a.b\n  x:env:default:9 SYNX_UNSET_Q\n  y:split a,b\na\n  b\n    x hello\n    y c,d\n'),
          '{"a":{"b":{"x":"hello","y":"c,d"}},"a.b":{"x":9,"y":["a","b"]}}');
      expect(parseFull('!active\na.b\n  x:env Q\n').metadata.containsKey(r'a\.b'), isTrue);
    });

    test('a replaced subtree drops its declarations', () {
      expect(
          active('port\n  port[enum:a|b] x\nport\na\n  x(int) 1\na 5\nx hello\nk1(int) 5\nk1(string) 6\n'
              'b\n  y(int) 1\nc\n  y(string) z\n'),
          '{"a":5,"b":{"y":1},"c":{"y":"TYPE_ERR: \'y\' expected int but got string"},"k1":"6","port":{},"x":"hello"}');
    });

    test('inherit carries declarations, merges parents and leaves lists alone', () {
      expect(
          active('_base\n  port:env:default:5 SYNX_UNSET_Q\n  weight 1\n  sub\n    deep:calc 2 * 3\n'
              'steel:inherit:_base\n  weight 2\nplain:inherit _base\n  port 9000\nlistchild:inherit:_base\n  - a\n'
              'scalar:inherit:_base 5\nmulti:inherit:_base:extra\n  z 1\nextra\n  weight 3\n  q:calc 2 + 2\n'),
          '{"extra":{"q":4,"weight":3},"listchild":["a"],"multi":{"port":5,"q":4,"sub":{"deep":6},"weight":3,"z":1},'
          '"plain":{"port":9000,"sub":{"deep":6},"weight":1},"scalar":{"port":5,"sub":{"deep":6},"weight":1},'
          '"steel":{"port":5,"sub":{"deep":6},"weight":2}}');
    });

    test('null checks only enum, strings count scalars', () {
      expect(
          active('mode[enum:a|b]:env SYNX_UNSET_Q\nport[type:int]:env SYNX_UNSET_Q\nname[min:3]:env SYNX_UNSET_Q\n'
              'e2[enum:a|]:env SYNX_UNSET_Q\nword[min:2, max:3] мира\nword2[max:3] мир\n'),
          '{"e2":null,"mode":"CONSTRAINT_ERR: \'mode\' must be one of [a|b]","name":null,"port":null,'
          '"word":"CONSTRAINT_ERR: \'word\' value 4 exceeds max 3","word2":"мир"}');
    });
  });

  group('values as text', () {
    test('numbers read as plain decimal text', () {
      expect(
          active('a 0.0000001\nh 123456789012345678901234.5\nt v={a} h={h}\nc:calc a * 2\nl:join\n  - 0.00000025\n'
              '  - 2.5\ne[enum:0.0000001] 0.0000001\n'),
          '{"a":1e-7,"c":2e-7,"e":1e-7,"h":1.2345678901234569e23,"l":"0.00000025,2.5","t":"v=0.0000001 h=123456789012345690000000"}');
      expect(active('obj\n  k 1\nnothing null\na See {obj}!\nb Value {nothing}.\n'),
          '{"a":"See !","b":"Value null.","nothing":null,"obj":{"k":1}}');
    });

    test('format follows Rust formatting', () {
      expect(
          active('a:format:%.2f 0.125\nb:format:%.0f 2.5\nd:format:%05d -42\ne:format:%.3e 123456.789\n'
              'f:format:%e 123456.789\ng:format:%+5d 7\nj:format:%.2e 0\nk:format:%.0e 15\nl:format:%05d 12.0\n'
              'm:format:%d 9223372036854775807\nn:format:%.2f -0.0\ns:format:%.2f "3.14159"\nt:format:%x 5\n'),
          '{"a":"0.12","b":"2","d":"00-42","e":"1.235e5","f":"1.23456789e+5","g":"    7","j":"0.00e0","k":"2e1",'
          '"l":"00012","m":"9223372036854775807","n":"-0.00","s":"3.14159","t":"5"}');
    });

    test('sort, sum, replace and version', () {
      expect(
          active('s1:sort\n  - b\n  - a\n  - 10\n  - 9\n  - B\ns2:sort:desc\n  - 3\n  - 1.5\n  - 2\n'
              's3:sort\n  - zz\n  - é\n  - e\n  - Z\nsum1:sum\n  - 1\n  - 2.5\n  - "3"\n  - x\n'
              'r1:replace:o:0 foo boo\nv1:version:>=:1.2.3 1.2.3-beta\nv3:version:<:2 1.x.9\n'),
          '{"r1":"f00 b00","s1":[9,10,"B","a","b"],"s2":[3,2,1.5],"s3":["Z","e","zz","é"],"sum1":6.5,"v1":true,"v3":true}');
    });
  });

  group('parser', () {
    test('indentation is measured in UTF-8 bytes', () {
      // U+00A0 counts 2 bytes, U+3000 counts 3; tab and space 1. `|+` cuts whole characters.
      final nb = String.fromCharCode(0xA0);
      final id = String.fromCharCode(0x3000);
      final src = 'a\n${nb}b 1\n$nb${nb}c 2\n\t d 3\n${id}e 4\nf\n${id}g 5\n  h 6\nlist\n$nb- x\n$nb- y\n'
          'code |+\n$nb${nb}line1\n$nb$nb$nb${nb}line2\n$nb$nb line3\ntxt |\n  ${nb}a  \n$nb \n  b$id\nlast 1$nb\n';
      expect(static_(src),
          '{"a":{"b":1,"c":2,"d":3,"e":4},"code":"line1\\n$nb${nb}line2\\n line3","f":{"g":5,"h":6},"last":1,'
          '"list":["x","y"],"txt":"a\\nb"}');
    });

    test('list items get the right parent', () {
      expect(
          static_('items\n  - name a\n    sub\n      k 1\n  - name b\n    v 2\n    w 3\nafter 1\nx:join\n x_1\n # c\n'
              '  - k v\n   y 2\nlst\n  - a\n  lst 5\n  - b\nobjs\n  - one\n  - two 2\n    three 3\n  - 4\n'),
          '{"after":1,"items":[{"name":"a","sub":{"k":1}},{"name":"b","v":2,"w":3}],"lst":5,'
          '"objs":["one",{"three":3,"two":2},4],"x":[],"x_1":[{"k":"v","y":2}]}');
    });

    test('an item of a re-declared list pops its frames before it is dropped', () {
      // `a` was re-declared as an object, so `- {cfg.v}` is dropped — but the frames at
      // or deeper than its indent are popped first, and `name` lands at the root.
      expect(static_("a\n  - base_hp 's'\n  a\n      x_1 [x]\n  - {cfg.v}\n   name\n"),
          '{"a":{"x_1":"[x]"},"name":{}}');
    });

    test('!use packages stay inside the packages directory', () {
      final dir = Directory.systemTemp.createTempSync('synx-use-').path;
      try {
        Directory('$dir/synx_packages/@a/ok/src').createSync(recursive: true);
        File('$dir/synx_packages/@a/ok/src/main.synx').writeAsStringSync('v 1\n');
        Directory('$dir/secret/src').createSync(recursive: true);
        File('$dir/secret/src/main.synx').writeAsStringSync('leak 1\n');
        // A manifest path that leaves the package counts as absent: src/main.synx is read.
        Directory('$dir/synx_packages/@a/evil/src').createSync(recursive: true);
        File('$dir/synx_packages/@a/evil/synx-pkg.synx').writeAsStringSync('main ../../../secret/src/main.synx\n');
        File('$dir/synx_packages/@a/evil/src/main.synx').writeAsStringSync('own 1\n');
        final opts = SynxOptions()..basePath = dir;
        expect(
            toJson(synxObject(parseActive(
                '!active\n!use @a/ok\n!use @a/../../secret as up\n!use @a/c:x as colon\n!use @a/evil\nx 1\n', opts))),
            '{"evil":{"own":1},"ok":{"v":1},"x":1}');
      } finally {
        Directory(dir).deleteSync(recursive: true);
      }
    });

    test('a large object parses in linear time', () {
      final sb = StringBuffer();
      for (var i = 0; i < 50000; i++) {
        sb.write('k$i $i\n');
      }
      final sw = Stopwatch()..start();
      final o = parse(sb.toString());
      expect(o.length, 50000);
      expect(sw.elapsedMilliseconds, lessThan(5000));
    });
  });

  group('writers', () {
    test('stringify round-trips data', () {
      const src = 'a "007"\nb "true"\nc "x # y"\nd ""\ne:join\nf -0.0\ng 2.0\ncode |+\n  if x\n    y\n'
          'list\n  - item x\n    item y\n  - one 1\n    sub\n      k v\n';
      final text = stringify(synxObject(parse(src)));
      expect(static_(text), static_(src));
      expect(text, contains('e:join\n'));
      expect(text, contains('f -0.0\n'));
      expect(text, contains('code |+\n'));
      expect(stringify(parseFullActive('!active\ns:secret hunter2\n').root), 's [SECRET]\n');
    });

    test('synxb is deterministic and round-trips', () {
      const src = '!active\nb 2\na x\nc\n  - x\n  - x\n  - y\nport[min:1]:env:default:80 P\nbig 9223372036854775807\n';
      final one = compile(src).bytes!;
      expect(compile(src).bytes, one);
      expect(one[6] & 0x04, isNot(0)); // not resolved: metadata section present
      final back = decompileToText(one)!;
      expect(static_(back), static_(src));
      expect(back, startsWith('!active\n\n'));
      expect(back, contains('big 9223372036854775807\n'));
    });

    test('format keeps block bodies and sorts by code point', () {
      const src = 'code |+\n  zeta()\n    alpha()\n  beta\nname x\n';
      final out = format(src);
      expect(out, 'code |+\n  zeta()\n    alpha()\n  beta\n\nname x\n');
      expect(format(out), out);
      expect(format('ab 2\na_b 1\n'), 'a_b 1\nab 2\n');
      expect(static_(format('b 1\na first\nc 2\na second\n')), '{"a":"second","b":1,"c":2}');
    });

    test('format past the depth cap drops the deeper subtree', () {
      // As synx-core: levels past the cap are dropped, the next shallower line goes on.
      final sb = StringBuffer();
      for (var i = 0; i < 140; i++) {
        sb.write('${' ' * i}k$i\n');
      }
      sb.write('tail 1\n  t2 2\n');
      final expected = StringBuffer();
      for (var i = 0; i <= 128; i++) {
        expected.write('${' ' * (2 * i)}k$i\n');
      }
      expected.write('\ntail 1\n  t2 2\n');
      expect(format(sb.toString()), expected.toString());
    });

    test('a self-include stops at the first repeat', () {
      final dir = Directory.systemTemp.createTempSync('synx-self-').path;
      try {
        File('$dir/self.synx').writeAsStringSync('!active\nme:include ./self.synx\nv 1\n');
        final sw = Stopwatch()..start();
        final json = toJson(synxObject(parseActive(File('$dir/self.synx').readAsStringSync(), SynxOptions()..basePath = dir)));
        sw.stop();
        // The text handed to parseActive has no path; the file it includes is
        // open once, and including it again from inside itself is the cycle.
        expect(json, '{"me":{"me":"INCLUDE_ERR: circular include: \'./self.synx\'","v":1},"v":1}');
        expect(sw.elapsedMilliseconds, lessThan(3000));

        // The pre-pass result is no extra include: `{x:include}` still sees exactly one.
        File('$dir/common.synx').writeAsStringSync('site Demo\n');
        expect(
            toJson(synxObject(parseActive(
                '!active\ndb:include ./common.synx\nx {site:include} / {site:db}\n', SynxOptions()..basePath = dir))),
            '{"db":{"site":"Demo"},"x":"Demo / Demo"}');
      } finally {
        Directory(dir).deleteSync(recursive: true);
      }
    });

    test('clamp bounds must be finite numbers', () {
      expect(
          active('a:clamp:nan:5 7\nb:clamp:1:inf 7\nc:clamp:Infinity:5 7\nd:clamp:1e400:5 7\ne:clamp:5abc:9 7\n'
              'f:clamp:-1e3:1E1 70\ng:clamp:1:5 7\n'),
          '{"a":7,"b":7,"c":7,"d":7,"e":7,"f":10,"g":5}');
    });

    test('a leading BOM alone is not truncation', () {
      final bom = String.fromCharCode(0xFEFF);
      expect(parseFull('${bom}name x\n').truncated, isFalse);
      expect(static_('${bom}name x\n'), '{"name":"x"}');
    });

    test('keys sort by code point, not UTF-16 unit', () {
      final pua = String.fromCharCode(0xE000);
      final rep = String.fromCharCode(0xFFFD);
      final emoji = String.fromCharCode(0x1F600);
      final src = '$emoji 2\n$pua 1\na\n  $emoji x\n  $rep y\n';
      expect(static_(src), '{"a":{"$rep":"y","$emoji":"x"},"$pua":1,"$emoji":2}');
      expect(stringify(synxObject(parse(src))), startsWith('a\n  $rep y\n'));
    });

    test('json floats use the ryu layout', () {
      expect(
          static_('a 1.0\nb 0.5\nc 1000000000000000000000.0\nd 0.0000001\ne -0.0\nf 10000000000000000.0\ng 1000000000000000.0\n'),
          '{"a":1.0,"b":0.5,"c":1e21,"d":1e-7,"e":-0.0,"f":1e16,"g":1000000000000000.0}');
    });
  });
}
