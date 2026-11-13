// Port worker for the Dart implementation; protocol as in ports/go/main.go.
// The library is imported by path: pass its `lib/synx.dart` through SYNX_DART_LIB
// when generating this file's import (see ports.mjs, which rewrites the line below).
import 'dart:convert';
import 'dart:io';

import 'SYNX_DART_LIB' as synx;

void main() {
  String? line;
  while ((line = stdin.readLineSync(encoding: utf8)) != null) {
    final f = line!.split('\t');
    if (f.length < 5) continue;
    if (f[1] == 'decompile') {
      // The third field is the .synxb container itself.
      String answer;
      try {
        answer = synx.decompileToText(base64.decode(f[2])) ?? 'ERROR: refused';
      } catch (e) {
        answer = 'ERROR: ${e.runtimeType}: $e';
      }
      stdout.writeln('${f[0]}\t${base64.encode(utf8.encode(answer))}\t');
      continue;
    }
    final text = utf8.decode(base64.decode(f[2]));
    final env = <String, String>{};
    for (final l in utf8.decode(base64.decode(f[3])).split('\n')) {
      final i = l.indexOf('=');
      if (i > 0) env[l.substring(0, i)] = l.substring(i + 1);
    }
    String json;
    var shown = '';
    try {
      final root = f[1] == 'active'
          ? synx.parseActive(text, synx.SynxOptions()
            ..env = env
            ..basePath = f[4])
          : synx.parse(text);
      final value = synx.synxObject(root);
      json = synx.toJson(value);
      shown = '$root $value ${value.toString()}';
    } catch (e) {
      json = 'ERROR: ${e.runtimeType}: $e';
    }
    stdout.writeln('${f[0]}\t${base64.encode(utf8.encode(json))}\t${base64.encode(utf8.encode(shown))}');
  }
}
