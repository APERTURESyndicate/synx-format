// SYNX `!active` engine — resolves markers, includes, packages, interpolation,
// constraints. Mirrors crates/synx-core/src/engine.rs.
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'calc.dart';
import 'constraints.dart';
import 'meta.dart';
import 'num.dart';
import 'options.dart';
import 'parser.dart' as parser;
import 'value.dart';

const int maxResolveDepth = 512;
const int _maxCalcExprLen = 4096;
const int _maxCalcResolvedLen = 64 * 1024;
const int _maxFileSize = 10 * 1024 * 1024;
const int _defaultMaxIncludeDepth = 16;

/// Upper bound for one scratch string built from hostile templates, in UTF-8 bytes.
const int _maxScratch = 4 * 1024 * 1024;

/// Markers the engine handles itself; a custom marker (`SynxOptions.markerFns`)
/// with one of these names is never called.
const Set<String> _builtinMarkers = {
  'env', 'default', 'calc', 'ref', 'alias', 'secret', 'random', 'unique',
  'geo', 'i18n', 'split', 'join', 'clamp', 'round', 'map', 'format',
  'replace', 'sort', 'sum', 'fallback', 'once', 'version', 'watch', 'prompt',
  'vision', 'audio', 'include', 'import', 'inherit', 'spam', 'template',
};

bool isBuiltinMarker(String name) => _builtinMarkers.contains(name);

/// Process-wide `:spam` buckets: key → call times.
final Map<String, List<DateTime>> _spamBuckets = {};

/// Apply markers and constraints to `result` in place. No-op on Static mode.
void resolve(SynxParseResult result, SynxOptions opts) {
  if (result.mode != SynxMode.active) return;
  _Resolver(result, opts).run();
}

class _Resolver {
  final SynxParseResult result;
  final SynxOptions opts;
  final Random rng;
  late final Map<String, SynxValue> includes;

  /// Root `:include` keys the pre-pass already resolved: key → (path, root).
  final Map<String, (String, SynxValue)> preresolved = {};
  final List<String> rootSpecial = [];

  _Resolver(this.result, this.opts) : rng = _seedRng(opts);

  static Random _seedRng(SynxOptions o) {
    final s = o.env?['SYNX_SEED'];
    if (s != null) {
      final n = int.tryParse(s);
      if (n != null) return Random(n);
    }
    return Random();
  }

  SynxParseResult get r => result;
  SynxMetadataTree get metadata => result.metadata;

  void run() {
    final packages = _loadPackages();
    includes = _loadIncludes();

    // `:include` / `:import` root keys double as aliases, so `{leaf:key}` works.
    // The marker of the same key takes this result instead of resolving the file
    // again (each level of an include chain resolved it twice: 2^depth). It is
    // kept apart from the aliases, so `{x:include}` still sees only the real includes.
    final root0 = result.root;
    final rootMeta = metadata[''];
    if (root0 is SynxObj && rootMeta != null && opts.includeDepth < (opts.maxIncludeDepth ?? _defaultMaxIncludeDepth)) {
      for (final e in root0.map.entryView) {
        final meta = rootMeta[e.key];
        if (meta == null) continue;
        if (!meta.markers.contains('include') && !meta.markers.contains('import')) continue;
        final v = e.value;
        if (v is! SynxStr) continue;
        try {
          final full = _jailPath(opts.basePath ?? '.', v.value);
          _checkFileSize(full);
          final included = _readAndResolve(full).root;
          preresolved[e.key] = (v.value, included);
          includes.putIfAbsent(e.key, () => included);
        } catch (_) {
          // Same as Rust: an unreadable include is no alias.
        }
      }
    }

    // Merge packages into root before resolution.
    if (root0 is SynxObj) {
      packages.forEach((alias, v) {
        if (!root0.map.contains(alias)) root0.map[alias] = v;
      });
    }

    // Type and constraint registries — spec: built from the document as it
    // stands, before inheritance: each object's own declarations, then its
    // children in document order.
    final types = <String, String>{};
    final constraints = <String, SynxConstraints>{};
    _collectRegistries(result.root, '', types, constraints, 0);

    _applyInheritance();
    final rootObj = result.root;
    if (rootObj is SynxObj) {
      for (final k in rootObj.map.keys) {
        if (k.startsWith('_')) rootObj.map.remove(k);
      }
    }

    // safety: mask literal `:secret` values before any resolution, so a field
    // that interpolates `{that_key}` can never observe the plaintext.
    _maskSecretLiterals(result.root, '');

    if (rootObj is SynxObj) {
      // Root keys a token lookup cannot find; resolution never adds or removes keys.
      for (final k in rootObj.map.keyView) {
        if (_isCalcSpecialKey(k)) rootSpecial.add(k);
      }
      _resolveValue(rootObj.map, metadata.isNotEmpty ? '' : null, 0);
    }

    _validateFields(result.root, constraints, types);
  }

  /// One pass in document order: each key's subtree, then the key's own
  /// markers, then the next key — so a marker sees every key before it fully
  /// resolved, at any depth. After all keys, the `{…}` interpolation pass.
  /// [path] is null when the document has no metadata at all.
  void _resolveValue(SynxObject map, String? path, int depth) {
    if (depth >= maxResolveDepth) {
      for (final k in map.keys) {
        map[k] = synxString('NESTING_ERR: maximum object nesting depth exceeded');
      }
      return;
    }
    final keys = map.keys;
    final mm = path == null ? null : metadata[path];
    var hasBrace = false;
    for (final key in keys) {
      final child = map[key];
      if (child is SynxObj) {
        _resolveValue(child.map, _childPath(path, key), depth + 1);
      } else if (child is SynxArr) {
        final listPath = _childPath(path, key);
        final items = child.values;
        for (var idx = 0; idx < items.length; idx++) {
          final item = items[idx];
          if (item is SynxObj) {
            _resolveValue(item.map, listPath == null ? null : '$listPath.#$idx', depth + 1);
          }
        }
      }
      final meta = mm?[key];
      if (meta != null) _applyMarkers(map, key, meta, path!);
      if (!hasBrace) {
        final v = map[key];
        if (v is SynxStr && v.value.contains('{')) hasBrace = true;
      }
    }
    if (!hasBrace) return;
    for (final key in keys) {
      final v = map[key];
      if (v is SynxStr && v.value.contains('{')) {
        final res = _resolveInterpolation(v.value, map);
        if (res != v.value) map[key] = synxString(res);
      }
    }
  }

  static String? _childPath(String? path, String key) {
    if (path == null) return null;
    final seg = parser.keySegment(key);
    return path.isEmpty ? seg : '$path.$seg';
  }

  // ─── files ────────────────────────────────────────────────────────────────

  /// Resolve [rel] inside [base] or throw (Rust `jail_path`).
  String _jailPath(String base, String rel) {
    if (rel.isNotEmpty && (rel[0] == '/' || rel[0] == '\\')) {
      throw StateError("SECURITY: rooted paths are not allowed: '$rel'");
    }
    if (_isAbsolute(rel)) {
      throw StateError("SECURITY: absolute paths are not allowed: '$rel'");
    }
    final baseCanon = Directory(base).absolute.path;
    final joined = '$baseCanon${Platform.pathSeparator}$rel';
    if (File(joined).existsSync() || Directory(joined).existsSync()) {
      final full = File(joined).absolute.uri.normalizePath().toFilePath();
      if (!_isSubPath(baseCanon, full)) {
        throw StateError("SECURITY: path escapes base directory: '$rel'");
      }
      return full;
    }
    // The file may not exist yet — at least verify no `..` escapes.
    if (joined.contains('..')) {
      throw StateError("SECURITY: path traversal detected: '$rel'");
    }
    return joined;
  }

  static bool _isAbsolute(String p) =>
      p.length >= 3 && p[1] == ':' && (p[2] == '\\' || p[2] == '/') || p.startsWith('\\\\');

  static bool _isSubPath(String root, String candidate) {
    String norm(String p) => File(p).absolute.uri.normalizePath().toFilePath().replaceAll('\\', '/');
    var r = norm(root);
    while (r.endsWith('/')) {
      r = r.substring(0, r.length - 1);
    }
    var c = norm(candidate);
    if (Platform.isWindows) {
      r = r.toLowerCase();
      c = c.toLowerCase();
    }
    return c == r || c.startsWith('$r/');
  }

  void _checkFileSize(String path) {
    final f = File(path);
    if (f.existsSync()) {
      final len = f.lengthSync();
      if (len > _maxFileSize) {
        throw StateError('SECURITY: file too large ($len bytes, max $_maxFileSize)');
      }
    }
  }

  /// Read a file as text, with Rust's portable not-found / permission messages.
  String _readText(String full, String shown) {
    try {
      return File(full).readAsStringSync();
    } on PathNotFoundException {
      throw StateError('file not found: $shown');
    } on PathAccessException {
      throw StateError('permission denied: $shown');
    } on FileSystemException catch (e) {
      throw StateError(e.message);
    }
  }

  SynxOptions _childOptions(String baseDir) => SynxOptions()
    ..env = opts.env
    ..region = opts.region
    ..lang = opts.lang
    ..basePath = baseDir
    ..maxIncludeDepth = opts.maxIncludeDepth
    ..packagesPath = opts.packagesPath
    ..strict = opts.strict
    ..markerFns = opts.markerFns
    ..includeDepth = opts.includeDepth + 1;

  SynxParseResult _readAndResolve(String full) {
    final sub = parser.parse(File(full).readAsStringSync());
    if (sub.mode == SynxMode.active) resolve(sub, _childOptions(File(full).parent.path));
    return sub;
  }

  Map<String, SynxValue> _loadIncludes() {
    final map = <String, SynxValue>{};
    final maxDepth = opts.maxIncludeDepth ?? _defaultMaxIncludeDepth;
    if (opts.includeDepth >= maxDepth) return map;
    for (final inc in result.includes) {
      try {
        final full = _jailPath(opts.basePath ?? '.', inc.path);
        _checkFileSize(full);
        map[inc.alias] = _readAndResolve(full).root;
      } catch (_) {
        // Same as Rust: skip failed includes.
      }
    }
    return map;
  }

  /// `!use @scope/name` → `<packages>/@scope/name/<manifest main>` or `src/main.synx`.
  Map<String, SynxValue> _loadPackages() {
    final map = <String, SynxValue>{};
    if (result.uses.isEmpty) return map;
    final pkgRoot = '${opts.basePath ?? '.'}/${opts.packagesPath ?? './synx_packages'}';
    for (final use in result.uses) {
      // safety: a package name or manifest entry must stay inside the packages
      // directory (`@x/../../etc` would read any file). A bad name skips the
      // package; a bad manifest path counts as absent (Rust `is_contained_relative`).
      if (!_isSafeRelative(use.pkg)) continue;
      final pkgDir = '$pkgRoot/${use.pkg}';
      final main = _manifestMain(pkgDir);
      final entry = main != null ? '$pkgDir/$main' : '$pkgDir/src/main.synx';
      final f = File(entry);
      if (!f.existsSync()) continue;
      try {
        _checkFileSize(entry);
        final sub = parser.parse(f.readAsStringSync());
        if (sub.mode == SynxMode.active) resolve(sub, _childOptions(f.parent.path));
        map[use.alias] = sub.root;
      } catch (_) {
        // skip failed packages
      }
    }
    return map;
  }

  /// The manifest's first contained `main` / `entry` path, relative to the package directory.
  String? _manifestMain(String pkgDir) {
    final f = File('$pkgDir/synx-pkg.synx');
    if (!f.existsSync()) return null;
    for (final line in f.readAsLinesSync()) {
      final t = trimWs(line);
      for (final prefix in const ['main ', 'entry ']) {
        if (t.startsWith(prefix)) {
          final p = trimWs(t.substring(prefix.length));
          if (_isSafeRelative(p)) return p;
        }
      }
    }
    return null;
  }

  /// A relative path that stays inside its base: no leading `/` or `\`, no `:`
  /// anywhere, no `..` segment. Judged by text only (Rust `is_contained_relative`).
  static bool _isSafeRelative(String p) {
    if (p.isEmpty || p[0] == '/' || p[0] == '\\' || p.contains(':')) return false;
    return !p.split(RegExp(r'[/\\]')).contains('..');
  }

  // ─── :inherit ─────────────────────────────────────────────────────────────

  void _applyInheritance() {
    final rootMeta = metadata[''];
    final root = result.root;
    if (rootMeta == null || root is! SynxObj) return;

    // Two surface syntaxes: `child:inherit:a:b` (parents in the chain) and
    // `child:inherit a` (the parser moves the value into the args).
    final inherits = <(String, List<String>)>[];
    rootMeta.forEach((key, meta) {
      final idx = meta.markers.indexOf('inherit');
      if (idx < 0) return;
      var parents = meta.markers.sublist(idx + 1);
      if (parents.isEmpty && meta.args.isNotEmpty) parents = meta.args.toList();
      if (parents.isNotEmpty) inherits.add((key, parents));
    });

    final map = root.map;
    for (final (childKey, parents) in inherits) {
      // spec: only an object inherits; a list or a scalar child is left as written.
      final childVal = map[childKey];
      if (childVal is! SynxObj) continue;

      // Parents merge left to right, the child overrides all. `source` remembers
      // the object each field came from (null = the child itself).
      final merged = SynxObject();
      final source = <String, String?>{};
      for (final parentName in parents) {
        final pv = map[parentName];
        if (pv is SynxObj) {
          for (final e in pv.map.entryView) {
            merged[e.key] = cloneValue(e.value);
            source[e.key] = parentName;
          }
        }
      }
      for (final e in childVal.map.entryView) {
        merged[e.key] = cloneValue(e.value);
        source[e.key] = null;
      }

      // spec: a field keeps the declarations of the object its value came from;
      // the inherited subtree is a copy, and its nested declarations move with it.
      final top = <String, SynxMeta>{};
      final nested = <(String, SynxMetaMap)>[];
      final childSeg = parser.keySegment(childKey);
      source.forEach((k, src) {
        final from = src ?? childKey;
        final m = metadata[parser.keySegment(from)]?[k];
        if (m != null) top[k] = _cloneMeta(m);
        if (src != null) {
          final prefix = '${parser.keySegment(src)}.${parser.keySegment(k)}';
          metadata.forEach((path, mm) {
            if (!path.startsWith(prefix)) return;
            final rest = path.substring(prefix.length);
            if (rest.isNotEmpty && rest[0] != '.') return;
            nested.add(('$childSeg.${parser.keySegment(k)}$rest', _cloneMetaMap(mm)));
          });
        }
      });
      map[childKey] = SynxObj(merged);
      if (top.isEmpty) {
        metadata.remove(childSeg);
      } else {
        metadata[childSeg] = top;
      }
      for (final (path, mm) in nested) {
        metadata[path] = mm;
      }
    }
  }

  static SynxMeta _cloneMeta(SynxMeta m) => SynxMeta()
    ..markers = m.markers.toList()
    ..args = m.args.toList()
    ..typeHint = m.typeHint
    ..constraints = m.constraints == null ? null : cloneConstraints(m.constraints!);

  static SynxMetaMap _cloneMetaMap(SynxMetaMap mm) =>
      {for (final e in mm.entries) e.key: _cloneMeta(e.value)};

  // ─── registries and validation ────────────────────────────────────────────

  void _collectRegistries(
      SynxValue value, String path, Map<String, String> types, Map<String, SynxConstraints> constraints, int depth) {
    if (depth > maxResolveDepth || value is! SynxObj || metadata.isEmpty) return;
    final mm = metadata[path];
    if (mm != null) {
      mm.forEach((key, meta) {
        final hint = meta.typeHint;
        if (hint != null) types.putIfAbsent(key, () => hint);
        final c = meta.constraints;
        if (c != null) {
          final existing = constraints[key];
          if (existing != null) {
            mergeConstraints(existing, c);
          } else {
            constraints[key] = cloneConstraints(c);
          }
        }
      });
    }
    for (final e in value.map.entryView) {
      final child = e.value;
      if (child is SynxObj) {
        _collectRegistries(child, _childPath(path, e.key)!, types, constraints, depth + 1);
      } else if (child is SynxArr) {
        final listPath = _childPath(path, e.key)!;
        for (var idx = 0; idx < child.values.length; idx++) {
          final item = child.values[idx];
          if (item is SynxObj) _collectRegistries(item, '$listPath.#$idx', types, constraints, depth + 1);
        }
      }
    }
  }

  /// Constraints then type for every field of the tree, in one walk. A value the
  /// per-key pass already turned into an error is not measured again.
  void _validateFields(SynxValue value, Map<String, SynxConstraints> constraints, Map<String, String> types) {
    if (constraints.isEmpty && types.isEmpty) return;
    if (value is! SynxObj) return;
    final o = value.map;
    for (final key in o.keys) {
      var val = o[key]!;
      final c = constraints[key];
      if (c != null &&
          !(val is SynxStr && (val.value.startsWith('CONSTRAINT_ERR:') || val.value.startsWith('TYPE_ERR:')))) {
        final err = constraintViolation(key, val, c);
        if (err != null) {
          val = _fail(o, key, err);
        }
      }
      final expected = types[key];
      if (expected != null && !matchesType(val, expected)) {
        val = _fail(o, key, "TYPE_ERR: '$key' expected $expected but got ${val.typeName}");
      }
      if (val is SynxObj) {
        _validateFields(val, constraints, types);
      } else if (val is SynxArr) {
        for (final item in val.values) {
          if (item is SynxObj) _validateFields(item, constraints, types);
        }
      }
    }
  }

  /// Write a constraint failure into the value (and to stderr with `strict`).
  SynxValue _fail(SynxObject obj, String key, String msg) {
    final v = synxString(msg);
    obj[key] = v;
    if (opts.strict) stderr.writeln('synx: $msg');
    return v;
  }

  void _validateConstraints(SynxObject map, String key, SynxConstraints c) {
    final v = map[key];
    if (v != null) {
      final err = constraintViolation(key, v, c);
      if (err != null) _fail(map, key, err);
    } else if (c.required) {
      _fail(map, key, "CONSTRAINT_ERR: '$key' is required");
    }
  }

  // ─── :secret pre-pass ─────────────────────────────────────────────────────

  static const Set<String> _resolvingBeforeSecret = {
    'env', 'include', 'import', 'watch', 'ref', 'alias', 'calc', 'map', 'i18n', 'default',
    'fallback', 'once', 'split', 'join', 'format', 'replace',
  };

  /// Wrap every literal `:secret` value before resolution, so no later
  /// interpolation can read the plaintext. A secret that itself needs resolving
  /// (`:env:secret`, …) is left for its own marker to mask in place.
  void _maskSecretLiterals(SynxValue value, String path) {
    if (value is! SynxObj || metadata.isEmpty) return;
    final o = value.map;
    final mm = metadata[path];
    if (mm != null) {
      mm.forEach((key, meta) {
        if (!meta.markers.contains('secret') || meta.markers.any(_resolvingBeforeSecret.contains)) return;
        final v = o[key];
        if (v != null && v is! SynxSecret && v is! SynxObj && v is! SynxArr) {
          o[key] = synxSecret(valueToString(v));
        }
      });
    }
    for (final e in o.entryView) {
      final child = e.value;
      if (child is SynxObj) {
        _maskSecretLiterals(child, _childPath(path, e.key)!);
      } else if (child is SynxArr) {
        final listPath = _childPath(path, e.key)!;
        for (var idx = 0; idx < child.values.length; idx++) {
          final item = child.values[idx];
          if (item is SynxObj) _maskSecretLiterals(item, '$listPath.#$idx');
        }
      }
    }
  }

  // ─── interpolation ────────────────────────────────────────────────────────

  /// Resolve `{key}`, `{key.nested}`, `{key:alias}`, `{key:include}` in a string.
  String _resolveInterpolation(String tpl, SynxObject local) {
    final out = StringBuffer();
    var bytes = 0;
    var i = 0;
    while (i < tpl.length) {
      if (bytes >= _maxScratch) break;
      if (tpl.codeUnitAt(i) == 0x7B) {
        final close = tpl.indexOf('}', i + 1);
        if (close > i) {
          final inner = tpl.substring(i + 1, close);
          final colon = inner.indexOf(':');
          String? refName;
          SynxValue? resolved;
          if (colon >= 0) {
            final name = inner.substring(0, colon);
            if (_isRefName(name)) {
              refName = name;
              final scope = inner.substring(colon + 1);
              if (scope == 'include') {
                if (includes.length == 1) resolved = _deepGet(includes.values.first, name);
              } else {
                final inc = includes[scope];
                if (inc != null) resolved = _deepGet(inc, name);
              }
            }
          } else if (_isRefName(inner)) {
            refName = inner;
            resolved = _deepGet(result.root, inner) ?? local[inner];
          }
          if (refName != null) {
            if (resolved != null) {
              bytes = _appendCapped(out, bytes, valueToString(resolved), _maxScratch - bytes);
            } else {
              out.write('{');
              bytes++;
              bytes = _appendCapped(out, bytes, inner, _maxScratch - (bytes + 1));
              if (bytes < _maxScratch) {
                out.write('}');
                bytes++;
              }
            }
            i = close + 1;
            continue;
          }
        }
      }
      // One whole character of literal text.
      final c = tpl.codeUnitAt(i);
      if (c >= 0xD800 && c <= 0xDBFF && i + 1 < tpl.length) {
        out.write(tpl.substring(i, i + 2));
        bytes += 4;
        i += 2;
      } else {
        out.writeCharCode(c);
        bytes += utf8LenUnit(c);
        i++;
      }
    }
    return out.toString();
  }

  static final RegExp _refName = RegExp(r'^[\p{Alphabetic}\p{N}_.]*$', unicode: true);

  /// Rust `char::is_alphanumeric` on whole scalars, or `_` / `.` (empty is valid).
  static bool _isRefName(String s) => _refName.hasMatch(s);

  /// Append at most [room] UTF-8 bytes of [s], cut at a character boundary.
  static int _appendCapped(StringBuffer out, int bytes, String s, int room) {
    if (room <= 0) return bytes;
    var i = 0;
    var used = 0;
    while (i < s.length) {
      final c = s.codeUnitAt(i);
      final pair = c >= 0xD800 && c <= 0xDBFF && i + 1 < s.length;
      final b = pair ? 4 : utf8LenUnit(c);
      if (used + b > room) break;
      used += b;
      i += pair ? 2 : 1;
    }
    out.write(i == s.length ? s : s.substring(0, i));
    return bytes + used;
  }

  // ─── markers ──────────────────────────────────────────────────────────────

  static String? _arg(List<String> markers, String name, int offset) {
    final i = markers.indexOf(name) + offset;
    return i < markers.length ? markers[i] : null;
  }

  void _applyMarkers(SynxObject map, String key, SynxMeta meta, String path) {
    final markers = meta.markers;
    final root = result.root;
    final mb = markers.toSet();
    bool has(String m) => mb.contains(m);

    // ── :spam ── key:spam:MAX_CALLS[:WINDOW_SEC] target
    if (has('spam')) {
      final maxCalls = _parseU(_arg(markers, 'spam', 1), u64Max) ?? 0;
      final windowSec = _parseU(_arg(markers, 'spam', 2), u64Max) ?? 1;
      if (maxCalls == 0) {
        map[key] = synxString('SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]');
        return;
      }
      final cur0 = map[key];
      final target = cur0 != null ? valueToString(cur0) : key;
      if (!_allowSpam('$key::$target', maxCalls, windowSec)) {
        map[key] = synxString("SPAM_ERR: '$target' exceeded $maxCalls calls per ${windowSec}s");
        return;
      }
      final cur = map[key];
      if (cur != null) {
        final t = valueToString(cur);
        final resolved = _deepGet(root, t) ?? map[t];
        if (resolved != null) map[key] = cloneValue(resolved);
      }
    }

    // ── :include / :import ──
    if (has('include') || has('import')) {
      final v = map[key];
      if (v is SynxStr) {
        final maxD = opts.maxIncludeDepth ?? _defaultMaxIncludeDepth;
        if (opts.includeDepth >= maxD) {
          map[key] = synxString('INCLUDE_ERR: max include depth ($maxD) exceeded');
          return;
        }
        final pre = path.isEmpty ? preresolved[key] : null;
        if (pre != null && pre.$1 == v.value) {
          map[key] = pre.$2;
          return;
        }
        try {
          final full = _jailPath(opts.basePath ?? '.', v.value);
          _checkFileSize(full);
          final sub = parser.parse(_readText(full, v.value));
          if (sub.mode == SynxMode.active) resolve(sub, _childOptions(File(full).parent.path));
          map[key] = sub.root;
        } on StateError catch (e) {
          map[key] = synxString('INCLUDE_ERR: ${e.message}');
        }
      }
      return;
    }

    // ── :env ──
    if (has('env')) {
      final v = map[key];
      if (v is SynxStr) {
        // A null map reads the process environment; an explicit (even empty) map
        // is the complete set of variables.
        final envVal = (opts.env ?? Platform.environment)[v.value];
        final forceStr = _wantsString(meta);
        final di = markers.indexOf('default');
        if (envVal != null && envVal.isNotEmpty) {
          map[key] = forceStr ? synxString(envVal) : _castPrimitive(envVal);
        } else if (di >= 0 && markers.length > di + 1) {
          // All parts after `default`, joined back: IPs (0.0.0.0) and URLs survive.
          final fallback = markers.sublist(di + 1).join(':');
          map[key] = forceStr ? synxString(fallback) : _castPrimitive(fallback);
        } else {
          map[key] = synxNull();
        }
      }
    }

    // ── :random ──
    if (has('random')) {
      final v = map[key];
      if (v is SynxArr) {
        if (v.values.isEmpty) {
          map[key] = synxNull();
          return;
        }
        if (meta.args.isNotEmpty) {
          final weights = <double>[];
          for (final a in meta.args) {
            final w = parseF64(a);
            if (w != null) weights.add(w);
          }
          map[key] = _weightedPick(v.values, weights);
        } else {
          map[key] = cloneValue(v.values[rng.nextInt(v.values.length)]);
        }
      }
    }

    // ── :ref ── like :alias, but the value feeds the markers after it (:ref:calc:*2)
    if (has('ref')) {
      final v = map[key];
      if (v is SynxStr) {
        final resolved = _deepGet(root, v.value) ?? map[v.value] ?? synxNull();
        var out = cloneValue(resolved);
        final n = _asNumber(resolved);
        final calcExpr = _arg(markers, 'calc', 1);
        if (has('calc') && n != null && calcExpr != null && calcExpr.isNotEmpty && '+-*/%'.contains(calcExpr[0])) {
          final res = safeCalc('${formatNumber(n)} $calcExpr');
          out = res.ok ? _numberValue(res.value) : synxString('CALC_ERR: ${res.error}');
        }
        map[key] = out;
      }
    }

    // ── :i18n ── key:i18n[:COUNT_FIELD]
    if (has('i18n')) {
      final v = map[key];
      if (v is SynxObj) {
        final lang = opts.lang ?? 'en';
        final trans = v.map;
        final val = trans[lang] ?? trans['en'] ?? (trans.isNotEmpty ? trans.entryView.first.value : synxNull());
        final countField = _arg(markers, 'i18n', 1);
        if (countField != null && val is SynxObj) {
          final local = map[countField];
          var countNum = local == null ? null : _asNumber(local);
          if (countNum == null) {
            final rc = _deepGet(root, countField);
            if (rc != null) countNum = _asNumber(rc);
          }
          final countVal = saturatingI64(countNum ?? 0);
          final forms = val.map;
          final chosen = forms[_pluralCategory(lang, countVal)] ??
              forms['other'] ??
              (forms.isNotEmpty ? forms.entryView.first.value : synxNull());
          map[key] = chosen is SynxStr
              ? synxString(chosen.value.replaceAll('{count}', countVal.toString()))
              : cloneValue(chosen);
        } else {
          map[key] = cloneValue(val);
        }
      }
    }

    // ── :calc ──
    if (has('calc')) {
      final v = map[key];
      if (v is SynxStr) {
        final expr = v.value;
        final exprBytes = utf8Len(expr);
        if (exprBytes > _maxCalcExprLen) {
          map[key] = synxString('CALC_ERR: expression too long ($exprBytes chars, max $_maxCalcExprLen)');
          return;
        }
        final rootMap = root is SynxObj ? root.map : null;
        var resolved = _substituteCalcVars(expr, map, key, rootMap);
        if (resolved != null) resolved = _resolveCalcDots(resolved);
        if (resolved == null) {
          map[key] = synxString('CALC_ERR: resolved expression too long (max $_maxCalcResolvedLen bytes)');
          return;
        }
        final res = safeCalc(resolved);
        map[key] = res.ok ? _numberValue(res.value) : synxString('CALC_ERR: ${res.error}');
      }
    }

    // ── :alias ──
    if (has('alias')) {
      final v = map[key];
      if (v is SynxStr) {
        final target = v.value;
        // A list item's `#<index>` segment is a metadata detail, not part of a
        // path a document can name.
        final sb = StringBuffer();
        for (final seg in parser.metaPathSegments(path)) {
          if (seg.startsWith('#')) continue;
          sb
            ..write(seg)
            ..write('.');
        }
        sb.write(key);
        final currentPath = sb.toString();
        if (target == key || target == currentPath) {
          map[key] = synxString('ALIAS_ERR: self-referential alias: $currentPath → $target');
        } else {
          final targetVal = _deepGet(root, target);
          final dot = target.lastIndexOf('.');
          final targetParent = dot >= 0 ? target.substring(0, dot) : '';
          final targetKeyName = dot >= 0 ? target.substring(dot + 1) : target;
          final targetHasAlias = metadata[targetParent]?[targetKeyName]?.markers.contains('alias') ?? false;
          final isCycle = targetHasAlias &&
              targetVal is SynxStr &&
              (targetVal.value == key || targetVal.value == currentPath);
          if (isCycle) {
            // Order-independent message: participants sorted.
            final a = compareCodePoints(currentPath, target) <= 0 ? currentPath : target;
            final b = identical(a, currentPath) ? target : currentPath;
            map[key] = synxString('ALIAS_ERR: circular alias detected: $a → $b');
          } else {
            map[key] = targetVal != null ? cloneValue(targetVal) : synxNull();
          }
        }
      }
    }

    // ── :secret ── idempotent: the pre-pass may already have wrapped a literal.
    if (has('secret')) {
      final v = map[key];
      if (v != null && v is! SynxSecret) map[key] = synxSecret(valueToString(v));
    }

    // ── :unique ──
    if (has('unique')) {
      final v = map[key];
      if (v is SynxArr) {
        final seen = <String>{};
        final out = <SynxValue>[];
        for (final item in v.values) {
          if (seen.add(valueToString(item))) out.add(cloneValue(item));
        }
        map[key] = synxArray(out);
      }
    }

    // ── :geo ──
    if (has('geo')) {
      final v = map[key];
      if (v is SynxArr) {
        final prefix = '${opts.region ?? 'US'} ';
        SynxValue? out;
        for (final item in v.values) {
          if (item is SynxStr && item.value.startsWith(prefix)) {
            out = synxString(trimWs(item.value.substring(prefix.length)));
            break;
          }
        }
        if (out == null) {
          if (v.values.isEmpty) {
            out = synxNull();
          } else {
            final f0 = v.values[0];
            final sp = f0 is SynxStr ? f0.value.indexOf(' ') : -1;
            out = f0 is SynxStr && sp >= 0 ? synxString(trimWs(f0.value.substring(sp + 1))) : cloneValue(f0);
          }
        }
        map[key] = out;
      }
    }

    // ── :split ──
    if (has('split')) {
      final v = map[key];
      if (v is SynxStr) {
        final sk = _arg(markers, 'split', 1);
        final sep = sk != null ? _delimiter(sk) : ',';
        final items = <SynxValue>[];
        for (final p in _rustSplit(v.value, sep)) {
          final t = trimWs(p);
          if (t.isNotEmpty) items.add(_castPrimitive(t));
        }
        map[key] = synxArray(items);
      }
    }

    // ── :join ──
    if (has('join')) {
      final v = map[key];
      if (v is SynxArr) {
        final jk = _arg(markers, 'join', 1);
        map[key] = synxString(v.values.map(valueToString).join(jk != null ? _delimiter(jk) : ','));
      }
    }

    // ── :default (standalone, without :env) ──
    if (has('default') && !has('env')) {
      final v = map[key];
      final empty = v == null || v is SynxNull || (v is SynxStr && v.value.isEmpty);
      if (empty) {
        final di = markers.indexOf('default');
        if (markers.length > di + 1) {
          final fallback = markers.sublist(di + 1).join(':');
          map[key] = _wantsString(meta) ? synxString(fallback) : _castPrimitive(fallback);
        }
      }
    }

    // ── :clamp ── key:clamp:MIN:MAX value
    if (has('clamp')) {
      final lo = parseF64(_arg(markers, 'clamp', 1) ?? '');
      final hi = parseF64(_arg(markers, 'clamp', 2) ?? '');
      // A bound must be a finite number: `nan`, `inf`, `1e400` skip the marker.
      if (lo != null && hi != null && lo.isFinite && hi.isFinite) {
        if (lo > hi) {
          map[key] = synxString('CONSTRAINT_ERR: clamp min (${rustDisplay(lo)}) > max (${rustDisplay(hi)})');
        } else {
          final v = map[key];
          final n = v == null ? null : _asNumber(v);
          if (n != null) map[key] = _numberValue(n < lo ? lo : (n > hi ? hi : n));
        }
      }
    }

    // ── :round ── key:round:N value (N decimal places, default 0)
    if (has('round')) {
      final dec = _parseU(_arg(markers, 'round', 1), u32Max) ?? 0;
      final v = map[key];
      final n = v == null ? null : _asNumber(v);
      if (n != null) {
        final factor = pow(10, dec.toSigned(32)).toDouble();
        final rounded = _roundHalfAway(n * factor) / factor;
        map[key] = dec == 0 ? synxInt(saturatingI64(rounded)) : synxFloat(rounded);
      }
    }

    // ── :map ── key:map:source_key + `- lookup result` items
    if (has('map')) {
      final v = map[key];
      if (v is SynxArr) {
        final sourceKey = _arg(markers, 'map', 1) ?? '';
        var lookup = '';
        if (sourceKey.isNotEmpty) {
          final src = _deepGet(root, sourceKey) ?? map[sourceKey];
          lookup = src != null ? valueToString(src) : '';
        }
        SynxValue? pick;
        for (final item in v.values) {
          if (item is SynxStr) {
            final sp = item.value.indexOf(' ');
            if (sp >= 0 && trimWs(item.value.substring(0, sp)) == lookup) {
              pick = _castPrimitive(trimWs(item.value.substring(sp + 1)));
              break;
            }
          }
        }
        map[key] = pick ?? synxNull();
      }
    }

    // ── :format ── key:format:PATTERN value (printf-style: %.2f, %d, %05d, %e)
    if (has('format')) {
      final v = map[key];
      if (v != null) map[key] = synxString(_applyFormat(_arg(markers, 'format', 1) ?? '%s', v));
    }

    // ── :replace:FROM:TO ── literal substring replacement; TO defaults to deletion
    if (has('replace')) {
      final v = map[key];
      if (v is SynxStr) {
        final from = _arg(markers, 'replace', 1) ?? '';
        final to = _arg(markers, 'replace', 2) ?? '';
        if (from.isNotEmpty) map[key] = synxString(v.value.replaceAll(from, to));
      }
    }

    // ── :sort / :sort:desc ── two numbers compare numerically, anything else as
    // text by code point (never locale collation); stable.
    if (has('sort')) {
      final v = map[key];
      if (v is SynxArr) {
        final desc = _arg(markers, 'sort', 1) == 'desc';
        final keyed = <(SynxValue, int, String?)>[];
        for (var i = 0; i < v.values.length; i++) {
          final item = cloneValue(v.values[i]);
          keyed.add((item, i, _asNumber(item) == null ? valueToString(item) : null));
        }
        keyed.sort((x, y) {
          final xn = _asNumber(x.$1);
          final yn = _asNumber(y.$1);
          int c;
          if (xn != null && yn != null) {
            c = xn.isNaN || yn.isNaN ? 0 : xn.compareTo(yn);
            if (c != 0 && xn == yn) c = 0; // -0.0 == 0.0
          } else {
            c = compareCodePoints(x.$3 ?? valueToString(x.$1), y.$3 ?? valueToString(y.$1));
          }
          return c != 0 ? c : x.$2 - y.$2;
        });
        var sorted = keyed.map((k) => k.$1).toList();
        if (desc) sorted = sorted.reversed.toList();
        map[key] = synxArray(sorted);
      }
    }

    // ── :sum ── numeric items (and numeric strings); int while every summand is whole
    if (has('sum')) {
      final v = map[key];
      if (v is SynxArr) {
        var total = 0.0;
        var allInt = true;
        for (final item in v.values) {
          if (item is SynxInt) {
            total += item.value.toDouble();
          } else if (item is SynxFloat) {
            total += item.value;
            if (item.value % 1 != 0) allInt = false;
          } else if (item is SynxStr) {
            final f = parseF64(item.value);
            if (f != null) {
              total += f;
              if (f % 1 != 0) allInt = false;
            }
          }
        }
        map[key] = allInt && total.isFinite && total % 1 == 0 && total.abs() < 9223372036854775807.0
            ? synxInt(total.toInt())
            : synxFloat(total);
      }
    }

    // ── :fallback ── key:fallback:DEFAULT value — the default when the path is missing
    if (has('fallback')) {
      final defv = _arg(markers, 'fallback', 1) ?? '';
      final v = map[key];
      final useFb = switch (v) {
        null || SynxNull() => true,
        SynxStr(value: var s) when s.isEmpty => true,
        SynxStr(value: var s) => !_safePathExists(s),
        _ => false,
      };
      if (useFb && defv.isNotEmpty) map[key] = synxString(defv);
    }

    // ── :once ── key:once[:uuid|random|timestamp], persisted in .synx.lock
    if (has('once')) {
      final gen = _arg(markers, 'once', 1) ?? 'uuid';
      final lockPath = '${opts.basePath ?? '.'}/.synx.lock';
      final existing = _readLock(lockPath, key);
      if (existing != null) {
        map[key] = synxString(existing);
      } else {
        final generated = switch (gen) {
          'timestamp' => (DateTime.now().millisecondsSinceEpoch ~/ 1000).toString(),
          'random' => rng.nextInt(4294967295).toString(),
          _ => _uuid(),
        };
        _writeLock(lockPath, key, generated);
        map[key] = synxString(generated);
      }
    }

    // ── :version ── key:version:OP:REQUIRED value → bool
    if (has('version')) {
      final v = map[key];
      if (v is SynxStr) {
        map[key] = synxBool(_compareVersions(v.value, _arg(markers, 'version', 1) ?? '>=', _arg(markers, 'version', 2) ?? ''));
      }
    }

    // ── :watch ── key:watch[:KEY_PATH] ./file
    if (has('watch')) {
      final v = map[key];
      if (v is SynxStr) {
        final maxD = opts.maxIncludeDepth ?? _defaultMaxIncludeDepth;
        if (opts.includeDepth >= maxD) {
          map[key] = synxString('WATCH_ERR: max include depth ($maxD) exceeded');
          return;
        }
        String full;
        try {
          full = _jailPath(opts.basePath ?? '.', v.value);
          _checkFileSize(full);
        } on StateError catch (e) {
          map[key] = synxString('WATCH_ERR: ${e.message}');
          return;
        }
        final kp = _arg(markers, 'watch', 1);
        try {
          final content = _readText(full, v.value);
          final dot = full.lastIndexOf('.');
          final sep = max(full.lastIndexOf('/'), full.lastIndexOf('\\'));
          final ext = dot > sep ? full.substring(dot + 1) : '';
          map[key] = kp != null ? (_extractFromContent(content, kp, ext) ?? synxNull()) : synxString(trimWs(content));
        } on StateError catch (e) {
          map[key] = synxString('WATCH_ERR: ${e.message}');
        }
      }
    }

    // ── :prompt ── key:prompt:LABEL subtree → fenced SYNX text
    if (has('prompt')) {
      final label = _arg(markers, 'prompt', 1) ?? key;
      final v = map[key];
      if (v != null) map[key] = synxString('$label (SYNX):\n```synx\n${_stringifyValue(v, 0)}```');
    }

    // ── custom markers (the analog of synx-core's WASM markers) ──
    if (opts.markerFns.isNotEmpty) {
      for (var i = 0; i < markers.length; i++) {
        final m = markers[i];
        if (isBuiltinMarker(m)) continue;
        final fn = opts.markerFns[m];
        if (fn == null) continue;
        map[key] = fn(key, markers.sublist(i + 1), map[key] ?? synxNull());
        break; // only one custom marker per key
      }
    }

    // ── Constraint validation (always last, after all markers resolved) ──
    final c = meta.constraints;
    if (c != null) _validateConstraints(map, key, c);
  }

  bool _safePathExists(String rel) {
    try {
      final full = _jailPath(opts.basePath ?? '.', rel);
      return File(full).existsSync() || Directory(full).existsSync();
    } catch (_) {
      // A path that escapes the jail counts as missing.
      return false;
    }
  }

  String? _readLock(String lockPath, String key) {
    String content;
    try {
      content = File(lockPath).readAsStringSync();
    } catch (_) {
      return null;
    }
    for (final line in _rustLines(content)) {
      if (line.startsWith(key) && line.length > key.length && line[key.length] == ' ') {
        return trimWsStart(line.substring(key.length));
      }
    }
    return null;
  }

  void _writeLock(String lockPath, String key, String val) {
    String content;
    try {
      content = File(lockPath).readAsStringSync();
    } catch (_) {
      content = '';
    }
    final lines = _rustLines(content);
    final nl = '$key $val';
    var found = false;
    for (var i = 0; i < lines.length; i++) {
      if (lines[i].startsWith(key) && lines[i].length > key.length && lines[i][key.length] == ' ') {
        lines[i] = nl;
        found = true;
        break;
      }
    }
    if (!found) lines.add(nl);
    try {
      File(lockPath).writeAsStringSync('${lines.join('\n')}\n');
    } catch (_) {
      // As Rust: a lock that cannot be written is not an error.
    }
  }

  String _uuid() {
    final b = List<int>.generate(16, (_) => rng.nextInt(256));
    b[6] = (b[6] & 0x0F) | 0x40;
    b[8] = (b[8] & 0x3F) | 0x80;
    final h = b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();
    return '${h.substring(0, 8)}-${h.substring(8, 12)}-${h.substring(12, 16)}-${h.substring(16, 20)}-${h.substring(20)}';
  }

  static bool _allowSpam(String bucket, int maxCalls, int windowSec) {
    final now = DateTime.now();
    final window = Duration(seconds: windowSec < 1 ? 1 : windowSec);
    final calls = _spamBuckets.putIfAbsent(bucket, () => []);
    calls.removeWhere((t) => now.difference(t) > window);
    if (calls.length >= maxCalls) return false;
    calls.add(now);
    return true;
  }

  SynxValue _weightedPick(List<SynxValue> items, List<double> weights) {
    final w = weights.toList();
    if (w.length < items.length) {
      final assigned = w.fold<double>(0, (a, b) => a + b);
      // Room left under 100 is shared equally; past 100, unassigned items get the
      // average assigned weight so they stay visible.
      final perItem = assigned < 100.0 ? (100.0 - assigned) / (items.length - w.length) : assigned / w.length;
      while (w.length < items.length) {
        w.add(perItem);
      }
    }
    final total = w.fold<double>(0, (a, b) => a + b);
    if (total <= 0) return cloneValue(items[rng.nextInt(items.length)]);
    final rv = rng.nextDouble();
    var cum = 0.0;
    for (var i = 0; i < items.length; i++) {
      cum += w[i] / total;
      if (rv <= cum) return cloneValue(items[i]);
    }
    return cloneValue(items.last);
  }

  // ─── :calc ────────────────────────────────────────────────────────────────

  /// Substitute numeric names into a `:calc` expression: a key of the current
  /// object first (it shadows the root), then a root key. Null when the result
  /// grows past the cap. perf: one pass over the words with a lookup each; the
  /// sequential form survives only when a special key with a numeric value is in play.
  String? _substituteCalcVars(String expr, SynxObject local, String selfKey, SynxObject? rootMap) {
    final localIsRoot = identical(local, rootMap);
    var special = false;
    if (rootMap != null) {
      for (final k in rootSpecial) {
        final v = rootMap[k];
        if (v != null && _asNumber(v) != null) {
          special = true;
          break;
        }
      }
    }
    if (!special && !localIsRoot) {
      for (final e in local.entryView) {
        if (e.key != selfKey && _asNumber(e.value) != null && _isCalcSpecialKey(e.key)) {
          special = true;
          break;
        }
      }
    }

    if (special) {
      // Exact sequential whole-word replacement, object before root.
      var resolved = expr;
      final maps = localIsRoot || rootMap == null ? [local] : [local, rootMap];
      for (final m in maps) {
        for (final e in m.entryView) {
          if (identical(m, local) && e.key == selfKey) continue;
          final n = _asNumber(e.value);
          if (n == null) continue;
          // An empty name matches everywhere: the expression only grows past the cap.
          if (e.key.isEmpty) return null;
          resolved = _replaceWord(resolved, e.key, formatNumber(n));
          if (utf8Len(resolved) > _maxCalcResolvedLen) return null;
        }
      }
      return resolved;
    }

    final out = StringBuffer();
    var bytes = 0;
    var i = 0;
    while (i < expr.length) {
      if (_isWordChar(expr.codeUnitAt(i))) {
        final start = i;
        while (i < expr.length && _isWordChar(expr.codeUnitAt(i))) {
          i++;
        }
        final token = expr.substring(start, i);
        double? n;
        if (token != selfKey) {
          final lv = local[token];
          if (lv != null) n = _asNumber(lv);
        }
        if (n == null && rootMap != null) {
          final rv = rootMap[token];
          if (rv != null) n = _asNumber(rv);
        }
        final text = n != null ? formatNumber(n) : token;
        out.write(text);
        bytes += text.length;
      } else {
        final c = expr.codeUnitAt(i);
        if (c >= 0xD800 && c <= 0xDBFF && i + 1 < expr.length) {
          out.write(expr.substring(i, i + 2));
          bytes += 4;
          i += 2;
        } else {
          out.writeCharCode(c);
          bytes += utf8LenUnit(c);
          i++;
        }
      }
      if (bytes > _maxCalcResolvedLen) return null;
    }
    return out.toString();
  }

  /// Substitute dot-path references (`base.hp`, `server.port`) with root values.
  String? _resolveCalcDots(String resolved) {
    final out = StringBuffer();
    var bytes = 0;
    var i = 0;
    final len = resolved.length;
    while (i < len) {
      if (_isWordChar(resolved.codeUnitAt(i))) {
        final start = i;
        var hasDot = false;
        while (i < len && (_isWordChar(resolved.codeUnitAt(i)) || resolved.codeUnitAt(i) == 0x2E)) {
          if (resolved.codeUnitAt(i) == 0x2E) hasDot = true;
          i++;
        }
        final token = resolved.substring(start, i);
        var text = token;
        if (hasDot) {
          final dv = _deepGet(result.root, token);
          final n = dv == null ? null : _asNumber(dv);
          if (n != null) text = formatNumber(n);
        }
        out.write(text);
        bytes += text.length;
      } else {
        final c = resolved.codeUnitAt(i);
        if (c >= 0xD800 && c <= 0xDBFF && i + 1 < len) {
          out.write(resolved.substring(i, i + 2));
          bytes += 4;
          i += 2;
        } else {
          out.writeCharCode(c);
          bytes += utf8LenUnit(c);
          i++;
        }
      }
      if (bytes > _maxCalcResolvedLen) return null;
    }
    return out.toString();
  }
}

// ─── Shared utilities ────────────────────────────────────────────────────────

/// Deep copy of objects and lists; scalars are immutable and shared.
SynxValue cloneValue(SynxValue v) {
  switch (v) {
    case SynxObj(map: var m):
      final copy = SynxObject();
      for (final e in m.entryView) {
        copy[e.key] = cloneValue(e.value);
      }
      return SynxObj(copy);
    case SynxArr(values: var items):
      return SynxArr([for (final x in items) cloneValue(x)]);
    default:
      return v;
  }
}

/// The value at [path] (direct root key first, then a dot path), not copied.
SynxValue? _deepGet(SynxValue root, String path) {
  if (root is SynxObj) {
    final direct = root.map[path];
    if (direct != null) return direct;
  }
  SynxValue current = root;
  var start = 0;
  while (true) {
    final dot = path.indexOf('.', start);
    final part = dot < 0 ? path.substring(start) : path.substring(start, dot);
    if (current is! SynxObj) return null;
    final next = current.map[part];
    if (next == null) return null;
    current = next;
    if (dot < 0) return current;
    start = dot + 1;
  }
}

double? _asNumber(SynxValue v) => switch (v) {
      SynxInt(value: var n) => n.toDouble(),
      SynxFloat(value: var f) => f,
      _ => null,
    };

/// A whole result below 2^63 is an int, anything else a float.
SynxValue _numberValue(double d) =>
    d.isFinite && d % 1 == 0 && d.abs() < 9223372036854775807.0 ? synxInt(d.toInt()) : synxFloat(d);

/// Value as text (interpolation, :join, :calc operands, enum checks, :prompt):
/// synx-core `value_to_string`.
String valueToString(SynxValue v) => switch (v) {
      SynxNull() => 'null',
      SynxBool(value: var b) => b ? 'true' : 'false',
      SynxInt(value: var n) => n.toString(),
      SynxFloat(value: var f) => formatNumber(f),
      SynxStr(value: var s) => s,
      // safety: valueToString feeds interpolation, :join, :map, :prompt — a
      // secret must reach none of those sinks as plaintext.
      SynxSecret() => '[SECRET]',
      SynxArr() || SynxObj() => '',
    };

/// Text from outside the document (`:env`, `:default`, `:split`) as a value:
/// Rust `cast_primitive`.
SynxValue _castPrimitive(String val) {
  if (val.length >= 2) {
    final a = val.codeUnitAt(0);
    final b = val.codeUnitAt(val.length - 1);
    if ((a == 0x22 && b == 0x22) || (a == 0x27 && b == 0x27)) {
      return synxString(val.substring(1, val.length - 1));
    }
  }
  switch (val) {
    case 'true':
      return synxBool(true);
    case 'false':
      return synxBool(false);
    case 'null':
      return synxNull();
  }
  final i = parseI64(val);
  if (i != null) return synxInt(i);
  final d = parseF64(val);
  if (d != null) return synxFloat(d);
  return synxString(val);
}

/// `(string)` or `[type:string]` keeps env/default text as text instead of casting it.
bool _wantsString(SynxMeta meta) => meta.typeHint == 'string' || meta.constraints?.typeName == 'string';

int? _parseU(String? s, BigInt max) => s == null ? null : parseUnsigned(s, max);

double _roundHalfAway(double x) => x.isFinite ? (x < 0 ? -(-x).roundToDouble() : x.roundToDouble()) : x;

bool _isWordChar(int c) =>
    (c >= 0x30 && c <= 0x39) || (c >= 0x41 && c <= 0x5A) || (c >= 0x61 && c <= 0x7A) || c == 0x5F;

/// A key whole-word substitution can reach in ways a token lookup cannot: one
/// with a non-word character (`max-hp`, `a.b`, `цена`), or one a substituted
/// number can itself spell (`2`, `NaN`, `inf`).
bool _isCalcSpecialKey(String k) {
  var allWord = true;
  var allDigit = true;
  for (var i = 0; i < k.length; i++) {
    final c = k.codeUnitAt(i);
    if (!_isWordChar(c)) allWord = false;
    if (c < 0x30 || c > 0x39) allDigit = false;
  }
  return !allWord || allDigit || k == 'NaN' || k == 'inf';
}

/// Replace whole-word occurrences of [word] (word = ASCII alphanumerics and `_`).
String _replaceWord(String hay, String word, String repl) {
  if (word.length > hay.length) return hay;
  final out = StringBuffer();
  var i = 0;
  final last = hay.length - word.length;
  while (i <= last) {
    if (hay.startsWith(word, i)) {
      final beforeOk = i == 0 || !_isWordChar(hay.codeUnitAt(i - 1));
      final afterOk = i + word.length >= hay.length || !_isWordChar(hay.codeUnitAt(i + word.length));
      if (beforeOk && afterOk) {
        out.write(repl);
        i += word.length;
        continue;
      }
    }
    out.writeCharCode(hay.codeUnitAt(i));
    i++;
  }
  if (i < hay.length) out.write(hay.substring(i));
  return out.toString();
}

String _delimiter(String kw) => switch (kw) {
      'space' => ' ',
      'pipe' => '|',
      'dash' => '-',
      'dot' => '.',
      'semi' => ';',
      'tab' => '\t',
      'slash' => '/',
      _ => kw,
    };

/// Rust `str::split`: an empty separator splits around every character.
List<String> _rustSplit(String s, String sep) {
  if (sep.isNotEmpty) return s.split(sep);
  return ['', ...s.runes.map(String.fromCharCode), ''];
}

/// Rust `str::lines`: split at `\n`, drop a trailing `\r`, no final empty line.
List<String> _rustLines(String content) {
  final lines = <String>[];
  var start = 0;
  while (start < content.length) {
    final nl = content.indexOf('\n', start);
    final end = nl < 0 ? content.length : nl;
    var line = content.substring(start, end);
    if (line.endsWith('\r')) line = line.substring(0, line.length - 1);
    lines.add(line);
    if (nl < 0) break;
    start = nl + 1;
  }
  return lines;
}

/// The integer at the start of [s] (after whitespace, optional sign), or 0 —
/// JavaScript's `parseInt(s, 10) || 0`.
int _leadingInt(String s) {
  final t = trimWsStart(s);
  var i = 0;
  var neg = false;
  if (t.isNotEmpty && (t[0] == '-' || t[0] == '+')) {
    neg = t[0] == '-';
    i = 1;
  }
  var n = 0;
  for (; i < t.length; i++) {
    final c = t.codeUnitAt(i);
    if (c < 0x30 || c > 0x39) break;
    final d = c - 0x30;
    n = n > (9223372036854775807 - d) ~/ 10 ? 9223372036854775807 : n * 10 + d;
  }
  return neg ? -n : n;
}

/// Compare two versions: each dot-separated part is its leading integer,
/// positions never shift.
bool _compareVersions(String current, String op, String required) {
  final cv = current.split('.').map(_leadingInt).toList();
  final rv = required.split('.').map(_leadingInt).toList();
  final len = max(cv.length, rv.length);
  var ord = 0;
  for (var i = 0; i < len; i++) {
    final a = i < cv.length ? cv[i] : 0;
    final b = i < rv.length ? rv[i] : 0;
    if (a == b) continue;
    ord = a.compareTo(b);
    break;
  }
  return switch (op) {
    '>=' => ord >= 0,
    '<=' => ord <= 0,
    '>' => ord > 0,
    '<' => ord < 0,
    '==' || '=' => ord == 0,
    '!=' => ord != 0,
    _ => false,
  };
}

String _pluralCategory(String lang, int n) {
  final absN = n.abs();
  final n10 = absN % 10;
  final n100 = absN % 100;
  switch (lang) {
    case 'ru':
    case 'uk':
    case 'be':
    case 'pl':
      if (n10 == 1 && n100 != 11) return 'one';
      if (n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14)) return 'few';
      return 'many';
    case 'cs':
    case 'sk':
      if (absN == 1) return 'one';
      if (absN >= 2 && absN <= 4) return 'few';
      return 'other';
    case 'ar':
      if (absN == 0) return 'zero';
      if (absN == 1) return 'one';
      if (absN == 2) return 'two';
      if (n100 >= 3 && n100 <= 10) return 'few';
      if (n100 >= 11 && n100 <= 99) return 'many';
      return 'other';
    case 'fr':
    case 'pt':
      return absN <= 1 ? 'one' : 'other';
    case 'ja':
    case 'zh':
    case 'ko':
    case 'vi':
    case 'th':
      return 'other';
    default:
      return absN == 1 ? 'one' : 'other';
  }
}

// ─── :format ─────────────────────────────────────────────────────────────────

String _applyFormat(String pattern, SynxValue value) {
  switch (value) {
    case SynxInt(value: var n):
      if (pattern.contains('d') || pattern.contains('i')) return _formatIntPattern(pattern, n);
      if (pattern.contains('f') || pattern.contains('e')) return _formatFloatPattern(pattern, n.toDouble());
      return n.toString();
    case SynxFloat(value: var f):
      if (pattern.contains('f') || pattern.contains('e')) return _formatFloatPattern(pattern, f);
      // spec: a whole float takes an integer pattern — `12.0` under `%05d` is `00012`.
      if ((pattern.contains('d') || pattern.contains('i')) && f.isFinite && f % 1 == 0 && f.abs() < 9.0e15) {
        return _formatIntPattern(pattern, f.toInt());
      }
      return formatNumber(f);
    case SynxStr(value: var s):
      return s;
    default:
      return valueToString(value);
  }
}

String _formatIntPattern(String pattern, int n) {
  const maxWidth = 4096;
  final text = n.toString();
  if (pattern.startsWith('%') && pattern.length > 1) {
    final last = pattern[pattern.length - 1];
    if (last == 'd' || last == 'i') {
      final inner = pattern.substring(1, pattern.length - 1);
      if (inner.isNotEmpty && inner[0] == '0') {
        final w = parseUnsigned(inner.substring(1), u64Max);
        if (w != null) return text.padLeft(min(w, maxWidth), '0');
      }
      final w2 = parseUnsigned(inner, u64Max);
      if (w2 != null) return text.padLeft(min(w2, maxWidth));
    }
  }
  return text;
}

String _formatFloatPattern(String pattern, double f) {
  const maxPrec = 1024;
  if (pattern.startsWith('%')) {
    final s = pattern.substring(1);
    // %e — exponent form in the JavaScript `toExponential()` shape: 1.23456789e+5.
    if (s == 'e') {
      if (f == 0) return '0e+0';
      final raw = lowerExpShortest(f);
      final epos = raw.lastIndexOf('e');
      if (epos >= 0) {
        var mantissa = raw.substring(0, epos);
        var expPart = raw.substring(epos + 1);
        var sign = '+';
        if (expPart.startsWith('-')) {
          sign = '-';
          expPart = expPart.substring(1);
        } else if (expPart.startsWith('+')) {
          expPart = expPart.substring(1);
        }
        if (mantissa.endsWith('.0')) mantissa = mantissa.substring(0, mantissa.length - 2);
        return '${mantissa}e$sign$expPart';
      }
      return raw;
    }
    if (s.isNotEmpty && (s.endsWith('f') || s.endsWith('e'))) {
      final inner = s.substring(0, s.length - 1);
      if (inner.startsWith('.')) {
        final prec = parseUnsigned(inner.substring(1), u64Max);
        if (prec != null) {
          final p = min(prec, maxPrec);
          return s.endsWith('e') ? rustExpFixed(f, p) : rustFixed(f, p);
        }
      }
    }
  }
  return rustDisplay(f);
}

// ─── :watch / :prompt helpers ────────────────────────────────────────────────

/// A value from file content by key path: JSON dot path, or a SYNX document's key path.
SynxValue? _extractFromContent(String content, String keyPath, String ext) {
  final parts = keyPath.split('.');
  if (ext == 'json') {
    Object? cur;
    try {
      cur = jsonDecode(content);
    } on FormatException {
      return null;
    }
    for (final p in parts) {
      if (cur is! Map || !cur.containsKey(p)) return null;
      cur = cur[p];
    }
    return _jsonToSynx(cur);
  }
  SynxValue cur = parser.parse(content).root;
  for (final p in parts) {
    if (cur is! SynxObj) return null;
    final next = cur.map[p];
    if (next == null) return null;
    cur = next;
  }
  return cur;
}

SynxValue _jsonToSynx(Object? v) {
  if (v == null) return synxNull();
  if (v is bool) return synxBool(v);
  if (v is int) return synxInt(v);
  if (v is double) return synxFloat(v);
  if (v is String) return synxString(v);
  if (v is List) return synxArray([for (final x in v) _jsonToSynx(x)]);
  if (v is Map) {
    final o = SynxObject();
    v.forEach((k, x) => o[k.toString()] = _jsonToSynx(x));
    return SynxObj(o);
  }
  return synxNull();
}

/// A value as SYNX text for `:prompt`.
String _stringifyValue(SynxValue value, int indent) {
  final sp = ' ' * indent;
  final sb = StringBuffer();
  switch (value) {
    case SynxObj(map: var m):
      for (final key in sortCodePoints(m.keys)) {
        final val = m[key]!;
        if (val is SynxObj) {
          sb.write('$sp$key\n');
          sb.write(_stringifyValue(val, indent + 2));
        } else if (val is SynxArr) {
          sb.write('$sp$key\n');
          for (final item in val.values) {
            sb.write('$sp  - ${valueToString(item)}\n');
          }
        } else {
          sb.write('$sp$key ${valueToString(val)}\n');
        }
      }
    case SynxArr(values: var items):
      // A list prompts as its items, as a list inside an object does.
      for (final item in items) {
        sb.write('$sp  - ${valueToString(item)}\n');
      }
    default:
      sb.write('$sp${valueToString(value)}\n');
  }
  return sb.toString();
}
