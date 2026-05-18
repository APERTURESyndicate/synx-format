// SYNX `!active` engine — resolves markers, includes, packages, interpolation,
// constraints. Mirrors crates/synx-core/src/engine.rs.
import 'dart:io';
import 'dart:math';

import 'calc.dart';
import 'meta.dart';
import 'options.dart';
import 'parser.dart' as parser;
import 'value.dart';

const int maxResolveDepth = 512;

const Set<String> _builtinMarkers = {
  'env', 'default', 'calc', 'ref', 'alias', 'secret', 'random', 'unique',
  'geo', 'i18n', 'split', 'join', 'clamp', 'round', 'map', 'format',
  'replace', 'sort', 'sum', 'fallback', 'once', 'version', 'watch', 'prompt',
  'vision', 'audio', 'include', 'import', 'inherit', 'spam',
};

bool isBuiltinMarker(String name) => _builtinMarkers.contains(name);

/// Process-wide spam bucket (at most one resolution per (process, key)).
final Set<String> _spamBuckets = {};

/// Apply markers and constraints to `result` in place. No-op on Static mode.
void resolve(SynxParseResult result, SynxOptions opts) {
  if (result.mode != SynxMode.active) return;
  if (result.root is! SynxObj) return;
  _Resolver(result, opts).run();
}

class _Resolver {
  final SynxParseResult result;
  final SynxOptions opts;
  final Map<String, SynxObject> namespaces = {};
  final Random rng;
  bool _onceLoaded = false;
  final Set<String> _onceKeys = {};
  final Set<String> _onceNewKeys = {};

  _Resolver(this.result, this.opts) : rng = _seedRng(opts);

  static Random _seedRng(SynxOptions o) {
    final s = o.env?['SYNX_SEED'];
    if (s != null) {
      final n = int.tryParse(s);
      if (n != null) return Random(n);
    }
    return Random();
  }

  SynxObject get root => (result.root as SynxObj).map;

  void run() {
    _loadPackages();
    _loadIncludes();
    _applyInheritPass();
    _stripUnderscoreKeys();
    _walk('', 0);
    _validateAll();
    _flushOnce();
  }

  void _loadPackages() {
    if (result.uses.isEmpty) return;
    final base = opts.packagesPath ?? './synx_packages';
    for (final use in result.uses) {
      if (use.pkg.contains('..')) continue;
      final file = File('$base/${use.pkg}/synx.synx');
      String text;
      try {
        text = file.readAsStringSync();
      } catch (_) {
        continue;
      }
      final sub = parser.parse(text);
      if (sub.root is! SynxObj) continue;
      final m = (sub.root as SynxObj).map;
      namespaces[use.alias] = m;
      root.set(use.alias, synxObject(m));
    }
  }

  void _loadIncludes() {
    if (result.includes.isEmpty) return;
    final max = opts.maxIncludeDepth ?? 16;
    if (opts.includeDepth >= max) return;
    final base = opts.basePath ?? '.';
    for (final inc in result.includes) {
      final safe = _jailPath(base, inc.path);
      if (safe == null) continue;
      String text;
      try {
        text = File(safe).readAsStringSync();
      } catch (_) {
        continue;
      }
      final sub = parser.parse(text);
      if (sub.mode == SynxMode.active) {
        final subOpts = SynxOptions()
          ..env = opts.env
          ..region = opts.region
          ..lang = opts.lang
          ..basePath = File(safe).parent.path
          ..maxIncludeDepth = opts.maxIncludeDepth
          ..packagesPath = opts.packagesPath
          ..strict = opts.strict
          ..markerFns = opts.markerFns
          ..includeDepth = opts.includeDepth + 1;
        resolve(sub, subOpts);
      }
      if (sub.root is! SynxObj) continue;
      final m = (sub.root as SynxObj).map;
      namespaces[inc.alias] = m;
      root.set(inc.alias, synxObject(m));
    }
  }

  void _applyInheritPass() {
    result.metadata.forEach((path, fields) {
      fields.forEach((key, meta) {
        if (!meta.hasMarker('inherit') || meta.args.isEmpty) return;
        _inheritMerge(path, key, meta.args);
      });
    });
  }

  void _inheritMerge(String path, String key, List<String> parentNames) {
    final parent = _getObjectAt(path);
    if (parent == null) return;
    final childVal = parent[key];
    if (childVal is! SynxObj) return;
    final target = childVal.map;
    for (final name in parentNames) {
      final pv = parent[name];
      if (pv is SynxObj) {
        _mergeMissing(target, pv.map);
      }
    }
  }

  void _mergeMissing(SynxObject dst, SynxObject src) {
    for (final e in src.entries) {
      final existing = dst[e.key];
      if (existing != null) {
        if (existing is SynxObj && e.value is SynxObj) {
          _mergeMissing(existing.map, (e.value as SynxObj).map);
        }
        continue;
      }
      dst.set(e.key, e.value);
    }
  }

  void _stripUnderscoreKeys() {
    final r = root;
    for (final k in r.keys) {
      if (k.isNotEmpty && k[0] == '_') r.remove(k);
    }
  }

  void _walk(String path, int depth) {
    if (depth > maxResolveDepth) return;
    final fields = result.metadata[path];
    if (fields != null) {
      final keys = fields.keys.toList();
      for (final k in keys) {
        _applyMarkers(fields[k]!, k, path);
      }
    }
    final container = _getObjectAt(path);
    if (container == null) return;
    for (final p in container.entries) {
      if (p.value is SynxObj) {
        final sub = path.isEmpty ? p.key : '$path.${p.key}';
        _walk(sub, depth + 1);
      }
    }
  }

  void _applyMarkers(SynxMeta meta, String key, String path) {
    final parent = _getObjectAt(path);
    if (parent == null) return;
    var value = parent[key] ?? synxNull();

    for (final marker in meta.markers) {
      switch (marker) {
        case 'env':
          value = _applyEnv(value, meta);
          break;
        case 'default':
          value = _applyDefault(value, meta);
          break;
        case 'calc':
          value = _applyCalc(value);
          break;
        case 'ref':
        case 'alias':
          value = _applyRef(value);
          break;
        case 'secret':
          value = _applySecret(value);
          break;
        case 'random':
          value = _applyRandom(value, meta);
          break;
        case 'unique':
          value = _applyUnique(value);
          break;
        case 'geo':
          value = _applyGeo(value, meta);
          break;
        case 'i18n':
          value = _applyI18n(value, meta);
          break;
        case 'split':
          value = _applySplit(value, meta);
          break;
        case 'join':
          value = _applyJoin(value, meta);
          break;
        case 'clamp':
          value = _applyClamp(value, meta);
          break;
        case 'round':
          value = _applyRound(value, meta);
          break;
        case 'map':
          value = _applyMap(value, meta);
          break;
        case 'format':
          value = _applyFormat(value, meta);
          break;
        case 'replace':
          value = _applyReplace(value, meta);
          break;
        case 'sort':
          value = _applySort(value, meta);
          break;
        case 'sum':
          value = _applySum(value);
          break;
        case 'fallback':
          value = _applyFallback(value, meta);
          break;
        case 'once':
          value = _applyOnce(value, path, key);
          break;
        case 'version':
          value = _applyVersion(value);
          break;
        case 'watch':
          value = _applyWatch(value);
          break;
        case 'prompt':
          value = _applyPrompt(value);
          break;
        case 'vision':
        case 'audio':
          break;
        case 'spam':
          value = _applySpam(value, key);
          break;
        case 'inherit':
        case 'include':
        case 'import':
          break;
        default:
          if (!isBuiltinMarker(marker)) {
            final fn = opts.markerFns[marker];
            if (fn != null) value = fn(key, meta.args, value);
          }
      }
    }

    if (meta.typeHint != null) {
      value = _coerceTypeHint(value, meta.typeHint!);
    }
    parent.set(key, value);
  }

  void _validateAll() {
    result.metadata.forEach((path, fields) {
      final container = _getObjectAt(path);
      if (container == null) return;
      fields.forEach((fk, meta) {
        final c = meta.constraints;
        if (c == null) return;
        final fv = container[fk];
        if (fv == null) {
          if (c.required && opts.strict) {
            stderr.writeln("synx: required '$path.$fk' missing");
          }
          return;
        }
        if (c.typeName != null) {
          final match = (c.typeName == 'int' && fv is SynxInt) ||
              (c.typeName == 'float' && fv is SynxFloat) ||
              (c.typeName == 'bool' && fv is SynxBool) ||
              (c.typeName == 'string' && fv is SynxStr) ||
              (c.typeName == 'array' && fv is SynxArr) ||
              (c.typeName == 'object' && fv is SynxObj);
          if (!match && opts.strict) {
            stderr.writeln(
                "synx: type mismatch '$path.$fk' want ${c.typeName}, got ${fv.typeName}");
          }
        }
        final dv = fv.asDouble;
        if (dv != null) {
          if (c.min != null && dv < c.min! && opts.strict) {
            stderr.writeln("synx: '$path.$fk' below min");
          }
          if (c.max != null && dv > c.max! && opts.strict) {
            stderr.writeln("synx: '$path.$fk' above max");
          }
        }
        if (c.enumValues != null && fv is SynxStr) {
          if (!c.enumValues!.contains(fv.value) && opts.strict) {
            stderr.writeln("synx: '$path.$fk' '${fv.value}' not in enum");
          }
        }
        if (c.pattern != null && fv is SynxStr) {
          if (!_regexMatches(fv.value, c.pattern!) && opts.strict) {
            stderr.writeln("synx: '$path.$fk' fails pattern '${c.pattern}'");
          }
        }
      });
    });
  }

  SynxValue _applyOnce(SynxValue v, String path, String key) {
    if (!_onceLoaded) {
      _onceLoaded = true;
      final base = opts.basePath ?? '.';
      try {
        final text = File('$base/.synx.lock').readAsStringSync();
        for (final line in text.split('\n')) {
          final s = line.trim();
          if (s.isNotEmpty) _onceKeys.add(s);
        }
      } catch (_) {}
    }
    final lockKey = path.isEmpty ? key : '$path.$key';
    if (_onceKeys.contains(lockKey)) return synxNull();
    _onceNewKeys.add(lockKey);
    return v;
  }

  void _flushOnce() {
    if (_onceNewKeys.isEmpty) return;
    final base = opts.basePath ?? '.';
    final all = {..._onceKeys, ..._onceNewKeys}.toList()..sort();
    try {
      File('$base/.synx.lock').writeAsStringSync('${all.join('\n')}\n');
    } catch (_) {}
  }

  // ─── path helpers ────────────────────────────────────────────────────────

  SynxObject? _getObjectAt(String path) {
    if (path.isEmpty) return root;
    var current = root;
    for (final seg in path.split('.')) {
      final v = current[seg];
      if (v is! SynxObj) return null;
      current = v.map;
    }
    return current;
  }

  SynxValue? _lookup(String path) {
    final dot = path.indexOf('.');
    if (dot >= 0) {
      final ns = path.substring(0, dot);
      final rest = path.substring(dot + 1);
      final nsRoot = namespaces[ns];
      if (nsRoot != null) {
        final v = _deepGet(rest, nsRoot);
        if (v != null) return v;
      }
    }
    return _deepGet(path, root);
  }

  SynxValue? _deepGet(String path, SynxObject from) {
    if (path.isEmpty) return null;
    var current = from;
    final parts = path.split('.');
    for (var i = 0; i < parts.length; i++) {
      final v = current[parts[i]];
      if (v == null) return null;
      if (i == parts.length - 1) return v;
      if (v is! SynxObj) return null;
      current = v.map;
    }
    return null;
  }

  String _interpolate(String s) {
    if (!s.contains('{')) return s;
    final out = StringBuffer();
    var i = 0;
    while (i < s.length) {
      final c = s[i];
      if (c == '{') {
        final end = s.indexOf('}', i + 1);
        if (end < 0) {
          out.write('{');
          i++;
          continue;
        }
        final inner = s.substring(i + 1, end).trim();
        final v = _lookup(inner);
        if (v != null) {
          out.write(valueToString(v));
        } else {
          out.write('{');
          out.write(inner);
          out.write('}');
        }
        i = end + 1;
        continue;
      }
      out.write(c);
      i++;
    }
    return out.toString();
  }

  String? _jailPath(String base, String rel) {
    if (rel.isEmpty) return null;
    if (rel[0] == '/' || rel[0] == '\\') return null;
    if (rel.length >= 2 && rel[1] == ':') return null;
    if (rel.startsWith('res://') || rel.startsWith('user://')) return null;
    final normalized = rel.replaceAll('\\', '/');
    for (final seg in normalized.split('/')) {
      if (seg == '..' || seg == '...') return null;
    }
    return '$base/$normalized';
  }

  SynxValue _coerceTypeHint(SynxValue v, String hint) {
    switch (hint) {
      case 'int':
        if (v is SynxInt) return v;
        final d = v.asDouble;
        if (d != null) return synxInt(d.toInt());
        return v;
      case 'float':
        if (v is SynxFloat) return v;
        final d = v.asDouble;
        if (d != null) return synxFloat(d);
        return v;
      case 'string':
        if (v is SynxStr) return v;
        return synxString(valueToString(v));
      case 'bool':
        if (v is SynxBool) return v;
        final s = valueToString(v);
        return synxBool(s == 'true' || s == '1');
    }
    return v;
  }

  // ─── markers ─────────────────────────────────────────────────────────────

  SynxValue _applyEnv(SynxValue v, SynxMeta meta) {
    if (opts.env == null) return v;
    final varName = valueToString(v);
    var fallback = '';
    final idx = meta.markerIndex('env');
    if (idx >= 0 &&
        idx + 1 < meta.markers.length &&
        meta.markers[idx + 1] == 'default' &&
        idx + 2 < meta.markers.length) {
      fallback = meta.markers[idx + 2];
    }
    final val = opts.env![varName];
    if (val != null) return synxString(val);
    if (fallback.isNotEmpty) return synxString(fallback);
    return synxNull();
  }

  SynxValue _applyDefault(SynxValue v, SynxMeta meta) {
    if (meta.hasMarker('env')) return v;
    final empty = v is SynxNull || (v is SynxStr && v.value.isEmpty);
    if (!empty) return v;
    final idx = meta.markerIndex('default');
    if (idx >= 0 && idx + 1 < meta.markers.length) {
      return synxString(meta.markers[idx + 1]);
    }
    return v;
  }

  SynxValue _applyCalc(SynxValue v) {
    var expr = valueToString(v);
    if (expr.isEmpty) return v;
    expr = _interpolate(expr);

    final pairs = <MapEntry<String, double>>[];
    for (final p in root.entries) {
      final d = p.value.asDouble;
      if (d != null) pairs.add(MapEntry(p.key, d));
    }
    pairs.sort((a, b) => b.key.length.compareTo(a.key.length));
    for (final p in pairs) {
      expr = _replaceWord(expr, p.key, _floatPrecision(p.value));
    }

    final r = safeCalc(expr);
    if (!r.ok) return v;
    final d = r.value;
    if (d.floor().toDouble() == d &&
        d.abs() < 9.2233720368547758e18) {
      return synxInt(d.toInt());
    }
    return synxFloat(d);
  }

  SynxValue _applyRef(SynxValue v) {
    final path = valueToString(v);
    if (path.isEmpty) return v;
    final target = _lookup(path);
    return target ?? v;
  }

  SynxValue _applySecret(SynxValue v) {
    if (v is SynxSecret) return v;
    if (v is SynxNull) return v;
    if (v is SynxStr) return synxSecret(v.value);
    return synxSecret(valueToString(v));
  }

  SynxValue _applyRandom(SynxValue v, SynxMeta meta) {
    final opts = <String>[];
    if (v is SynxArr) {
      for (final item in v.values) {
        opts.add(valueToString(item));
      }
    } else if (v is SynxStr) {
      for (final p in v.value.split(',')) {
        opts.add(p.trim());
      }
    }
    if (opts.isEmpty) return v;
    final weights = <double>[];
    for (final a in meta.args) {
      weights.add(double.tryParse(a) ?? 1.0);
    }
    while (weights.length < opts.length) {
      weights.add(1.0);
    }
    final total = weights.fold<double>(0, (a, b) => a + b);
    if (total <= 0) return v;
    final pick = rng.nextDouble() * total;
    var acc = 0.0;
    for (var i = 0; i < opts.length; i++) {
      acc += weights[i];
      if (pick <= acc) return synxString(opts[i]);
    }
    return synxString(opts.last);
  }

  SynxValue _applyUnique(SynxValue v) {
    if (v is! SynxArr) return v;
    final out = <SynxValue>[];
    for (final item in v.values) {
      if (!out.any((seen) => seen == item)) out.add(item);
    }
    return synxArray(out);
  }

  SynxValue _applyGeo(SynxValue v, SynxMeta meta) {
    final region = opts.region;
    if (region == null || region.isEmpty) return v;
    for (final a in meta.args) {
      final colon = a.indexOf(':');
      if (colon < 0) continue;
      if (a.substring(0, colon) == region) {
        return synxString(a.substring(colon + 1));
      }
    }
    return v;
  }

  SynxValue _applyI18n(SynxValue v, SynxMeta meta) {
    final lang = opts.lang ?? 'en';
    final lang2 = lang.length >= 2 ? lang.substring(0, 2) : lang;
    final n = v.asDouble;
    final category = n != null ? _pluralCategory(lang, n) : 'other';
    final keys = [
      '$lang.$category',
      '$lang2.$category',
      lang,
      lang2,
      'other',
    ];
    for (final key in keys) {
      for (final a in meta.args) {
        final colon = a.indexOf(':');
        if (colon < 0) continue;
        if (a.substring(0, colon) == key) {
          return synxString(a.substring(colon + 1));
        }
      }
    }
    return v;
  }

  SynxValue _applySplit(SynxValue v, SynxMeta meta) {
    if (v is! SynxStr) return v;
    var sep = ',';
    final idx = meta.markerIndex('split');
    if (idx >= 0 && idx + 1 < meta.markers.length) sep = meta.markers[idx + 1];
    final out = <SynxValue>[];
    if (sep.isEmpty) {
      for (final c in v.value.split('')) {
        out.add(synxString(c));
      }
    } else {
      for (final p in v.value.split(sep)) {
        out.add(synxString(p.trim()));
      }
    }
    return synxArray(out);
  }

  SynxValue _applyJoin(SynxValue v, SynxMeta meta) {
    if (v is! SynxArr) return v;
    var sep = ',';
    final idx = meta.markerIndex('join');
    if (idx >= 0 && idx + 1 < meta.markers.length) sep = meta.markers[idx + 1];
    return synxString(v.values.map(valueToString).join(sep));
  }

  SynxValue _applyClamp(SynxValue v, SynxMeta meta) {
    var d = v.asDouble;
    if (d == null) return v;
    var lo = double.negativeInfinity;
    var hi = double.infinity;
    final idx = meta.markerIndex('clamp');
    if (idx >= 0 && idx + 2 < meta.markers.length) {
      lo = double.tryParse(meta.markers[idx + 1]) ?? lo;
      hi = double.tryParse(meta.markers[idx + 2]) ?? hi;
    }
    if (d < lo) d = lo;
    if (d > hi) d = hi;
    if (v is SynxInt) return synxInt(d.toInt());
    return synxFloat(d);
  }

  SynxValue _applyRound(SynxValue v, SynxMeta meta) {
    final d = v.asDouble;
    if (d == null) return v;
    var digits = 0;
    final idx = meta.markerIndex('round');
    if (idx >= 0 && idx + 1 < meta.markers.length) {
      digits = int.tryParse(meta.markers[idx + 1]) ?? 0;
    }
    final factor = pow(10, digits).toDouble();
    final r = (d * factor).roundToDouble() / factor;
    if (digits == 0) return synxInt(r.toInt());
    return synxFloat(r);
  }

  SynxValue _applyMap(SynxValue v, SynxMeta meta) {
    final key = valueToString(v);
    for (final a in meta.args) {
      final colon = a.indexOf(':');
      if (colon < 0) continue;
      if (a.substring(0, colon) == key) {
        return synxString(a.substring(colon + 1));
      }
    }
    return v;
  }

  SynxValue _applyFormat(SynxValue v, SynxMeta meta) {
    final idx = meta.markerIndex('format');
    if (idx < 0 || idx + 1 >= meta.markers.length) return v;
    final pattern = _interpolate(meta.markers[idx + 1]);
    final n = v.asDouble ?? 0;
    final sIn = valueToString(v);
    return synxString(_applyPrintf(pattern, n, sIn));
  }

  SynxValue _applyReplace(SynxValue v, SynxMeta meta) {
    if (v is! SynxStr) return v;
    final idx = meta.markerIndex('replace');
    if (idx < 0 || idx + 2 >= meta.markers.length) return v;
    final from = meta.markers[idx + 1];
    final to = meta.markers[idx + 2];
    if (from.isEmpty) return v;
    return synxString(v.value.replaceAll(from, to));
  }

  SynxValue _applySort(SynxValue v, SynxMeta meta) {
    if (v is! SynxArr) return v;
    var desc = false;
    final idx = meta.markerIndex('sort');
    if (idx >= 0 && idx + 1 < meta.markers.length) {
      desc = meta.markers[idx + 1] == 'desc';
    }
    final out = List<SynxValue>.from(v.values);
    out.sort((a, b) {
      final da = a.asDouble, db = b.asDouble;
      if (da != null && db != null) {
        return desc ? db.compareTo(da) : da.compareTo(db);
      }
      final sa = valueToString(a);
      final sb = valueToString(b);
      return desc ? sb.compareTo(sa) : sa.compareTo(sb);
    });
    return synxArray(out);
  }

  SynxValue _applySum(SynxValue v) {
    if (v is! SynxArr) return v;
    var total = 0.0;
    var anyFloat = false;
    for (final item in v.values) {
      final d = item.asDouble;
      if (d != null) {
        total += d;
        if (item is SynxFloat) anyFloat = true;
      }
    }
    return anyFloat ? synxFloat(total) : synxInt(total.toInt());
  }

  SynxValue _applyFallback(SynxValue v, SynxMeta meta) {
    final empty = v is SynxNull || (v is SynxStr && v.value.isEmpty);
    if (!empty) return v;
    final idx = meta.markerIndex('fallback');
    if (idx >= 0 && idx + 1 < meta.markers.length) {
      return synxString(meta.markers[idx + 1]);
    }
    return v;
  }

  SynxValue _applyVersion(SynxValue v) {
    if (v is SynxStr) return v;
    return synxString(valueToString(v));
  }

  SynxValue _applyWatch(SynxValue v) {
    if (v is! SynxStr) return v;
    final base = opts.basePath ?? '.';
    final safe = _jailPath(base, v.value);
    if (safe == null) return v;
    try {
      return synxString(File(safe).readAsStringSync());
    } catch (_) {
      return v;
    }
  }

  SynxValue _applyPrompt(SynxValue v) {
    if (v is! SynxStr) return v;
    return synxString(_interpolate(v.value));
  }

  SynxValue _applySpam(SynxValue v, String key) {
    if (_spamBuckets.contains(key)) return synxNull();
    _spamBuckets.add(key);
    return v;
  }
}

// ─── Shared utilities ───────────────────────────────────────────────────────

String valueToString(SynxValue v) => switch (v) {
      SynxNull() => 'null',
      SynxBool(value: var b) => b ? 'true' : 'false',
      SynxInt(value: var n) => n.toString(),
      SynxFloat(value: var f) => () {
          if (f.isNaN || f.isInfinite) return 'null';
          var s = f.toString();
          if (!s.contains('.') && !s.contains('e') && !s.contains('E')) {
            s = '$s.0';
          }
          return s;
        }(),
      SynxStr(value: var s) => s,
      SynxSecret(value: var s) => s,
      SynxArr(values: var arr) =>
        '[${arr.map(valueToString).join(', ')}]',
      SynxObj() => '[Object]',
    };

String _floatPrecision(double d) {
  var s = d.toString();
  if (!s.contains('.') && !s.contains('e') && !s.contains('E')) s = '$s.0';
  return s;
}

bool _regexMatches(String value, String pattern) {
  try {
    return RegExp(pattern).hasMatch(value);
  } on FormatException {
    return true;
  }
}

String _replaceWord(String s, String word, String repl) {
  if (word.isEmpty) return s;
  final out = StringBuffer();
  var i = 0;
  while (i < s.length) {
    final idx = s.indexOf(word, i);
    if (idx < 0) {
      out.write(s.substring(i));
      break;
    }
    out.write(s.substring(i, idx));
    final leftOK = idx == 0 || !_isWordChar(s[idx - 1]);
    final after = idx + word.length;
    final rightOK = after == s.length || !_isWordChar(s[after]);
    if (leftOK && rightOK) {
      out.write(repl);
      i = after;
    } else {
      out.write(s[idx]);
      i = idx + 1;
    }
  }
  return out.toString();
}

bool _isWordChar(String c) {
  if (c.isEmpty) return false;
  final cc = c.codeUnitAt(0);
  return (cc >= 0x30 && cc <= 0x39) ||
      (cc >= 0x41 && cc <= 0x5A) ||
      (cc >= 0x61 && cc <= 0x7A) ||
      cc == 0x5F;
}

String _applyPrintf(String pattern, double number, String sIn) {
  final out = StringBuffer();
  var i = 0;
  while (i < pattern.length) {
    final c = pattern[i];
    if (c != '%') {
      out.write(c);
      i++;
      continue;
    }
    if (i + 1 < pattern.length && pattern[i + 1] == '%') {
      out.write('%');
      i += 2;
      continue;
    }
    var end = i + 1;
    while (end < pattern.length) {
      final k = pattern[end];
      if (k == 'd' || k == 'i' || k == 'f' || k == 'e' || k == 'g' || k == 's') break;
      end++;
    }
    if (end >= pattern.length) {
      out.write(pattern.substring(i));
      break;
    }
    final spec = pattern.substring(i, end + 1);
    final kind = pattern[end];
    out.write(_formatOne(spec, kind, number, sIn));
    i = end + 1;
  }
  return out.toString();
}

String _formatOne(String spec, String kind, double number, String sIn) {
  // Parse `%[flags][width][.precision]<kind>` — flags supported: `0` and `-`.
  // Conversion done manually because Dart has no printf — only `toString*` /
  // `padLeft` / etc.
  var s = spec.substring(1, spec.length - 1); // strip leading '%' and kind
  var leftAlign = false;
  var zeroPad = false;
  while (s.isNotEmpty && (s[0] == '-' || s[0] == '0' || s[0] == '+' || s[0] == ' ')) {
    if (s[0] == '-') leftAlign = true;
    if (s[0] == '0') zeroPad = true;
    s = s.substring(1);
  }
  int width = 0;
  int dotPos = s.indexOf('.');
  if (dotPos < 0) {
    if (s.isNotEmpty) width = int.tryParse(s) ?? 0;
  } else {
    width = dotPos > 0 ? (int.tryParse(s.substring(0, dotPos)) ?? 0) : 0;
  }
  int precision = -1;
  if (dotPos >= 0) {
    precision = int.tryParse(s.substring(dotPos + 1)) ?? -1;
  }

  String body;
  switch (kind) {
    case 'd':
    case 'i':
      body = number.toInt().toString();
      break;
    case 'f':
      body = number.toStringAsFixed(precision >= 0 ? precision : 6);
      break;
    case 'e':
      body = number.toStringAsExponential(precision >= 0 ? precision : 6);
      break;
    case 'g':
      body = precision >= 0
          ? number.toStringAsPrecision(precision == 0 ? 1 : precision)
          : number.toString();
      break;
    case 's':
      body = sIn;
      if (precision >= 0 && body.length > precision) {
        body = body.substring(0, precision);
      }
      break;
    default:
      return spec;
  }

  if (body.length >= width) return body;
  final pad = width - body.length;
  if (leftAlign) {
    return body + (' ' * pad);
  }
  if (zeroPad && (kind == 'd' || kind == 'i' || kind == 'f' || kind == 'e' || kind == 'g')) {
    // Place zeros after a leading sign if any.
    if (body.startsWith('-')) {
      return '-${'0' * pad}${body.substring(1)}';
    }
    return ('0' * pad) + body;
  }
  return (' ' * pad) + body;
}

String _pluralCategory(String lang, double n) {
  final two = lang.length >= 2 ? lang.substring(0, 2) : lang;
  final intN = n.abs().floor();
  final mod10 = intN % 10;
  final mod100 = intN % 100;
  final intLike = n == n.floorToDouble();

  switch (two) {
    case 'ru':
    case 'uk':
    case 'be':
      if (intLike && mod10 == 1 && mod100 != 11) return 'one';
      if (intLike && mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14)) return 'few';
      if (intLike &&
          (mod10 == 0 ||
              (mod10 >= 5 && mod10 <= 9) ||
              (mod100 >= 11 && mod100 <= 14))) return 'many';
      return 'other';
    case 'pl':
      if (intLike && n == 1) return 'one';
      if (intLike && mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14)) return 'few';
      if (intLike &&
          n != 1 &&
          (mod10 == 0 ||
              mod10 == 1 ||
              (mod10 >= 5 && mod10 <= 9) ||
              (mod100 >= 12 && mod100 <= 14))) return 'many';
      return 'other';
    case 'cs':
    case 'sk':
      if (intLike && n == 1) return 'one';
      if (intLike && intN >= 2 && intN <= 4) return 'few';
      if (!intLike) return 'many';
      return 'other';
    case 'ar':
      if (n == 0) return 'zero';
      if (n == 1) return 'one';
      if (n == 2) return 'two';
      if (intLike && mod100 >= 3 && mod100 <= 10) return 'few';
      if (intLike && mod100 >= 11) return 'many';
      return 'other';
    case 'fr':
    case 'pt':
      if (n >= 0 && n < 2) return 'one';
      return 'other';
    case 'ja':
    case 'zh':
    case 'ko':
    case 'vi':
    case 'th':
      return 'other';
  }
  return n == 1 ? 'one' : 'other';
}
