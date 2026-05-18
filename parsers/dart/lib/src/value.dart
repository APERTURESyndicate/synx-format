// SYNX value tree. Mirrors crates/synx-core/src/value.rs.
// Dart 3 sealed class — pattern-matched everywhere downstream.

/// Sealed root of the SYNX value hierarchy. All concrete values extend this
/// class; subclassing outside this library is not permitted.
sealed class SynxValue {
  const SynxValue();

  /// Diagnostic type tag, matches Rust `Value::type_name`.
  String get typeName;
}

class SynxNull extends SynxValue {
  const SynxNull();
  static const instance = SynxNull();
  @override String get typeName => 'null';
}

class SynxBool extends SynxValue {
  final bool value;
  const SynxBool(this.value);
  @override String get typeName => 'bool';
  @override bool operator ==(Object other) => other is SynxBool && value == other.value;
  @override int get hashCode => value.hashCode;
}

class SynxInt extends SynxValue {
  final int value;
  const SynxInt(this.value);
  @override String get typeName => 'int';
  @override bool operator ==(Object other) => other is SynxInt && value == other.value;
  @override int get hashCode => value.hashCode;
}

class SynxFloat extends SynxValue {
  final double value;
  const SynxFloat(this.value);
  @override String get typeName => 'float';
  @override bool operator ==(Object other) => other is SynxFloat && value == other.value;
  @override int get hashCode => value.hashCode;
}

class SynxStr extends SynxValue {
  final String value;
  const SynxStr(this.value);
  @override String get typeName => 'string';
  @override bool operator ==(Object other) => other is SynxStr && value == other.value;
  @override int get hashCode => value.hashCode;
}

class SynxArr extends SynxValue {
  final List<SynxValue> values;
  const SynxArr(this.values);
  @override String get typeName => 'array';
  @override bool operator ==(Object other) {
    if (other is! SynxArr || values.length != other.values.length) return false;
    for (var i = 0; i < values.length; i++) {
      if (values[i] != other.values[i]) return false;
    }
    return true;
  }
  @override int get hashCode => Object.hashAll(values);
}

class SynxObj extends SynxValue {
  final SynxObject map;
  const SynxObj(this.map);
  @override String get typeName => 'object';
  @override bool operator ==(Object other) => other is SynxObj && map == other.map;
  @override int get hashCode => map.hashCode;
}

/// Redacted in JSON / stringify output as `[SECRET]`.
class SynxSecret extends SynxValue {
  final String value;
  const SynxSecret(this.value);
  @override String get typeName => 'secret';
  @override bool operator ==(Object other) => other is SynxSecret && value == other.value;
  @override int get hashCode => value.hashCode;
}

// ─── Factories ──────────────────────────────────────────────────────────────

SynxValue synxNull() => SynxNull.instance;
SynxValue synxBool(bool b) => SynxBool(b);
SynxValue synxInt(int n) => SynxInt(n);
SynxValue synxFloat(double f) => SynxFloat(f);
SynxValue synxString(String s) => SynxStr(s);
SynxValue synxSecret(String s) => SynxSecret(s);
SynxValue synxArray([List<SynxValue>? items]) => SynxArr(items ?? <SynxValue>[]);
SynxValue synxObject([SynxObject? o]) => SynxObj(o ?? SynxObject());

// ─── Convenience accessors ──────────────────────────────────────────────────

extension SynxValueAccessors on SynxValue {
  bool get isNull => this is SynxNull;

  bool? get asBool => this is SynxBool ? (this as SynxBool).value : null;
  int? get asInt => this is SynxInt ? (this as SynxInt).value : null;
  double? get asFloat => this is SynxFloat ? (this as SynxFloat).value : null;
  String? get asString => this is SynxStr ? (this as SynxStr).value : null;
  String? get asSecret => this is SynxSecret ? (this as SynxSecret).value : null;
  List<SynxValue>? get asArray => this is SynxArr ? (this as SynxArr).values : null;
  SynxObject? get asObject => this is SynxObj ? (this as SynxObj).map : null;

  /// Numeric coercion: int/float → double; bool → 0/1; otherwise null.
  double? get asDouble => switch (this) {
        SynxInt(value: var n) => n.toDouble(),
        SynxFloat(value: var f) => f,
        SynxBool(value: var b) => b ? 1.0 : 0.0,
        SynxStr(value: var s) => double.tryParse(s),
        _ => null,
      };
}

// ─── Mode ───────────────────────────────────────────────────────────────────

enum SynxMode { static_, active }

// ─── Object (insertion-ordered map) ─────────────────────────────────────────

/// Insertion-ordered string → SynxValue container. Used wherever a JSON-style
/// "object" is needed. Equality is order-insensitive; iteration follows
/// insertion order.
class SynxObject {
  final List<MapEntry<String, SynxValue>> _entries = [];

  SynxObject();

  /// Number of entries.
  int get length => _entries.length;
  bool get isEmpty => _entries.isEmpty;
  bool get isNotEmpty => _entries.isNotEmpty;

  /// Look up a key. Returns null when absent.
  SynxValue? operator [](String key) {
    for (final e in _entries) {
      if (e.key == key) return e.value;
    }
    return null;
  }

  /// Insert or overwrite a key.
  void operator []=(String key, SynxValue value) => set(key, value);

  void set(String key, SynxValue value) {
    for (var i = 0; i < _entries.length; i++) {
      if (_entries[i].key == key) {
        _entries[i] = MapEntry(key, value);
        return;
      }
    }
    _entries.add(MapEntry(key, value));
  }

  bool remove(String key) {
    for (var i = 0; i < _entries.length; i++) {
      if (_entries[i].key == key) {
        _entries.removeAt(i);
        return true;
      }
    }
    return false;
  }

  bool contains(String key) => this[key] != null;

  List<String> get keys => _entries.map((e) => e.key).toList();
  List<String> get sortedKeys => keys..sort();
  List<MapEntry<String, SynxValue>> get entries => List.unmodifiable(_entries);

  /// Returns a value for `key`, or `fallback` when absent.
  SynxValue getOr(String key, SynxValue fallback) => this[key] ?? fallback;

  @override
  bool operator ==(Object other) {
    if (other is! SynxObject || _entries.length != other._entries.length) return false;
    for (final e in _entries) {
      final v = other[e.key];
      if (v == null && !other.contains(e.key)) return false;
      if (e.value != v) return false;
    }
    return true;
  }

  @override
  int get hashCode {
    var h = 0;
    for (final e in _entries) {
      h ^= Object.hash(e.key, e.value);
    }
    return h;
  }
}
