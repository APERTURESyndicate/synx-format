import 'package:synx/synx.dart';
import 'package:synx/src/engine.dart' show resolve;
import 'package:test/test.dart';

void main() {
  test('env override', () {
    final opts = SynxOptions()..env = {'APP_PORT': '9090'};
    final r = parseFull('!active\nport:env:default:3000 APP_PORT\n');
    resolve(r, opts);
    final o = (r.root as SynxObj).map;
    expect(o['port'], equals(synxString('9090')));
  });

  test('env falls back to default', () {
    final opts = SynxOptions()..env = {};
    final r = parseFull('!active\nport:env:default:3000 NOT_SET\n');
    resolve(r, opts);
    final o = (r.root as SynxObj).map;
    expect(o['port'], equals(synxString('3000')));
  });

  test('calc basic', () {
    final r = parseFull('!active\nprice 100\ntax:calc price * 0.2\n');
    resolve(r, SynxOptions());
    final o = (r.root as SynxObj).map;
    final d = o['tax']?.asDouble;
    expect(d, isNotNull);
    expect(d, closeTo(20, 0.01));
  });

  test('secret redacted in JSON', () {
    final r = parseFull('!active\ntoken:secret abc123\n');
    resolve(r, SynxOptions());
    final j = toJson(r.root);
    expect(j, contains('[SECRET]'));
    expect(j.contains('abc123'), isFalse);
  });

  test('clamp', () {
    final r = parseFull('!active\nx:clamp:0:10 99\n');
    resolve(r, SynxOptions());
    final o = (r.root as SynxObj).map;
    expect(o['x']?.asDouble, equals(10.0));
  });

  test('format padded', () {
    final r = parseFull('!active\nnum:format:%05d 42\n');
    resolve(r, SynxOptions());
    final o = (r.root as SynxObj).map;
    expect(o['num'], equals(synxString('00042')));
  });
}
