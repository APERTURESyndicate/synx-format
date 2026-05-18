// .synxb compact binary format. Wire-compatible with crates/synx-core 3.6.x.
//
// Raw DEFLATE via `dart:io` ZLibCodec(raw: true, level: 9) matches Rust
// miniz_oxide::deflate::compress_to_vec byte-for-byte.

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'meta.dart';
import 'value.dart';

const List<int> _magic = [0x53, 0x59, 0x4E, 0x58, 0x42]; // "SYNXB"
const int _version = 1;

const int _flagActive = 0x01;
const int _flagLocked = 0x02;
const int _flagHasMeta = 0x04;
const int _flagResolved = 0x08;
const int _flagTool = 0x10;
const int _flagSchema = 0x20;
const int _flagLlm = 0x40;

const int _tagNull = 0x00;
const int _tagFalse = 0x01;
const int _tagTrue = 0x02;
const int _tagInt = 0x03;
const int _tagFloat = 0x04;
const int _tagString = 0x05;
const int _tagArray = 0x06;
const int _tagObject = 0x07;
const int _tagSecret = 0x08;

class SynxBinaryError implements Exception {
  final String message;
  const SynxBinaryError(this.message);
  @override
  String toString() => 'SynxBinaryError: $message';
}

bool isSynxb(List<int> data) {
  if (data.length < 5) return false;
  for (var i = 0; i < 5; i++) {
    if (data[i] != _magic[i]) return false;
  }
  return true;
}

class SynxCompileResult {
  final Uint8List? bytes;
  final String? error;
  const SynxCompileResult.success(this.bytes) : error = null;
  const SynxCompileResult.failure(this.error) : bytes = null;
  bool get ok => bytes != null;
}

class SynxDecompileResult {
  final SynxParseResult? result;
  final String? error;
  const SynxDecompileResult.success(this.result) : error = null;
  const SynxDecompileResult.failure(this.error) : result = null;
  bool get ok => result != null;
}

SynxCompileResult compile(SynxParseResult r, bool resolved) {
  final st = _StringTable();
  _collectStrings(r.root, st);
  final hasMeta = !resolved && r.metadata.isNotEmpty;
  if (hasMeta) {
    _collectMetadataStrings(r.metadata, st);
    _collectIncludeStrings(r.includes, st);
  }

  final payload = BytesBuilder();
  st.encode(payload);
  _encodeValue(payload, r.root, st);
  if (hasMeta) {
    _encodeMetadata(payload, r.metadata, st);
    _encodeIncludes(payload, r.includes, st);
  }
  final payloadBytes = payload.toBytes();
  final payloadLen = payloadBytes.length;

  late Uint8List compressed;
  try {
    compressed = Uint8List.fromList(
        ZLibCodec(raw: true, level: 9).encode(payloadBytes));
  } on Exception catch (e) {
    return SynxCompileResult.failure('deflate failed: $e');
  }

  final out = BytesBuilder();
  out.add(_magic);
  out.addByte(_version);
  var flags = 0;
  if (r.mode == SynxMode.active) flags |= _flagActive;
  if (r.locked) flags |= _flagLocked;
  if (hasMeta) flags |= _flagHasMeta;
  if (resolved) flags |= _flagResolved;
  if (r.tool) flags |= _flagTool;
  if (r.schema) flags |= _flagSchema;
  if (r.llm) flags |= _flagLlm;
  out.addByte(flags);
  out.addByte(payloadLen & 0xFF);
  out.addByte((payloadLen >> 8) & 0xFF);
  out.addByte((payloadLen >> 16) & 0xFF);
  out.addByte((payloadLen >> 24) & 0xFF);
  out.add(compressed);
  return SynxCompileResult.success(out.toBytes());
}

SynxDecompileResult decompile(List<int> data) {
  if (data.length < 11) {
    return const SynxDecompileResult.failure('file too small for .synxb header');
  }
  if (!isSynxb(data)) {
    return const SynxDecompileResult.failure(
        'invalid .synxb magic (expected SYNXB)');
  }
  if (data[5] != _version) {
    return SynxDecompileResult.failure(
        'unsupported .synxb version: ${data[5]}');
  }
  final flags = data[6];
  final uncomp = data[7] |
      (data[8] << 8) |
      (data[9] << 16) |
      (data[10] << 24);

  late Uint8List payload;
  try {
    payload = Uint8List.fromList(
        ZLibCodec(raw: true).decode(data.sublist(11)));
  } on Exception catch (e) {
    return SynxDecompileResult.failure('decompression failed: $e');
  }
  if (payload.length != uncomp) {
    return const SynxDecompileResult.failure(
        'size mismatch in decompressed payload');
  }

  final cur = _Cursor(payload);
  late _StringTableReader reader;
  late SynxValue root;
  try {
    reader = _StringTableReader.decode(cur);
    root = _decodeValue(cur, reader);
  } on SynxBinaryError catch (e) {
    return SynxDecompileResult.failure(e.message);
  }

  final pr = SynxParseResult(root: root);
  pr.mode = (flags & _flagActive) != 0 ? SynxMode.active : SynxMode.static_;
  pr.locked = (flags & _flagLocked) != 0;
  pr.tool = (flags & _flagTool) != 0;
  pr.schema = (flags & _flagSchema) != 0;
  pr.llm = (flags & _flagLlm) != 0;
  if ((flags & _flagHasMeta) != 0) {
    try {
      pr.metadata = _decodeMetadata(cur, reader);
      pr.includes = _decodeIncludes(cur, reader);
    } on SynxBinaryError catch (e) {
      return SynxDecompileResult.failure(e.message);
    }
  }
  return SynxDecompileResult.success(pr);
}

// ─── varint / zigzag ────────────────────────────────────────────────────────

void _varint(BytesBuilder out, int value) {
  var v = value;
  while (true) {
    final b = v & 0x7F;
    v = v >>> 7;
    if (v == 0) {
      out.addByte(b);
      return;
    }
    out.addByte(b | 0x80);
  }
}

int _readVarint(_Cursor cur) {
  var result = 0;
  var shift = 0;
  while (true) {
    if (cur.pos >= cur.data.length) {
      throw const SynxBinaryError('unexpected end of data in varint');
    }
    final b = cur.data[cur.pos++];
    result |= (b & 0x7F) << shift;
    if ((b & 0x80) == 0) return result;
    shift += 7;
    if (shift >= 64) throw const SynxBinaryError('varint overflow');
  }
}

int _zigzagEncode(int n) {
  // For 64-bit Dart ints: (n << 1) ^ (n >> 63).
  final s = (n << 1) ^ (n >> 63);
  return s;
}

int _zigzagDecode(int n) => (n >>> 1) ^ -(n & 1);

void _writeF64LE(BytesBuilder out, double f) {
  final bd = ByteData(8);
  bd.setFloat64(0, f, Endian.little);
  out.add(bd.buffer.asUint8List());
}

double _readF64LE(_Cursor cur) {
  if (cur.pos + 8 > cur.data.length) {
    throw const SynxBinaryError('unexpected end of data in float');
  }
  final bd = ByteData.sublistView(cur.data, cur.pos, cur.pos + 8);
  cur.pos += 8;
  return bd.getFloat64(0, Endian.little);
}

class _Cursor {
  final Uint8List data;
  int pos = 0;
  _Cursor(this.data);
}

// ─── String table ───────────────────────────────────────────────────────────

class _StringTable {
  final List<String> strings = [];
  final Map<String, int> index = {};

  int intern(String s) {
    final existing = index[s];
    if (existing != null) return existing;
    final idx = strings.length;
    strings.add(s);
    index[s] = idx;
    return idx;
  }

  int indexOf(String s) => index[s] ?? 0;

  void encode(BytesBuilder out) {
    _varint(out, strings.length);
    for (final s in strings) {
      final bytes = _encodeUtf8(s);
      _varint(out, bytes.length);
      out.add(bytes);
    }
  }
}

class _StringTableReader {
  final List<String> strings;
  _StringTableReader(this.strings);

  static _StringTableReader decode(_Cursor cur) {
    final count = _readVarint(cur);
    final list = <String>[];
    for (var i = 0; i < count; i++) {
      final len = _readVarint(cur);
      if (cur.pos + len > cur.data.length) {
        throw const SynxBinaryError('unexpected end of data in string table');
      }
      list.add(_decodeUtf8(cur.data.sublist(cur.pos, cur.pos + len)));
      cur.pos += len;
    }
    return _StringTableReader(list);
  }

  String get(int idx) {
    if (idx < 0 || idx >= strings.length) {
      throw const SynxBinaryError('string index out of bounds');
    }
    return strings[idx];
  }
}

// ─── Value encode / decode ──────────────────────────────────────────────────

void _collectStrings(SynxValue v, _StringTable t) {
  switch (v) {
    case SynxStr(value: var s):
      t.intern(s);
      return;
    case SynxSecret(value: var s):
      t.intern(s);
      return;
    case SynxArr(values: var arr):
      for (final item in arr) {
        _collectStrings(item, t);
      }
      return;
    case SynxObj(map: var map):
      for (final e in map.entries) {
        t.intern(e.key);
        _collectStrings(e.value, t);
      }
      return;
    default:
      return;
  }
}

void _collectMetadataStrings(SynxMetadataTree tree, _StringTable t) {
  tree.forEach((path, m) {
    t.intern(path);
    m.forEach((key, meta) {
      t.intern(key);
      for (final mk in meta.markers) {
        t.intern(mk);
      }
      for (final a in meta.args) {
        t.intern(a);
      }
      if (meta.typeHint != null) t.intern(meta.typeHint!);
      final c = meta.constraints;
      if (c != null) {
        if (c.typeName != null) t.intern(c.typeName!);
        if (c.pattern != null) t.intern(c.pattern!);
        if (c.enumValues != null) {
          for (final e in c.enumValues!) {
            t.intern(e);
          }
        }
      }
    });
  });
}

void _collectIncludeStrings(List<SynxIncludeDirective> incs, _StringTable t) {
  for (final inc in incs) {
    t.intern(inc.path);
    t.intern(inc.alias);
  }
}

void _encodeValue(BytesBuilder out, SynxValue v, _StringTable t) {
  switch (v) {
    case SynxNull():
      out.addByte(_tagNull);
      return;
    case SynxBool(value: var b):
      out.addByte(b ? _tagTrue : _tagFalse);
      return;
    case SynxInt(value: var n):
      out.addByte(_tagInt);
      _varint(out, _zigzagEncode(n));
      return;
    case SynxFloat(value: var f):
      out.addByte(_tagFloat);
      _writeF64LE(out, f);
      return;
    case SynxStr(value: var s):
      out.addByte(_tagString);
      _varint(out, t.indexOf(s));
      return;
    case SynxSecret(value: var s):
      out.addByte(_tagSecret);
      _varint(out, t.indexOf(s));
      return;
    case SynxArr(values: var arr):
      out.addByte(_tagArray);
      _varint(out, arr.length);
      for (final item in arr) {
        _encodeValue(out, item, t);
      }
      return;
    case SynxObj(map: var map):
      out.addByte(_tagObject);
      final keys = map.sortedKeys;
      _varint(out, keys.length);
      for (final k in keys) {
        _varint(out, t.indexOf(k));
        _encodeValue(out, map[k] ?? synxNull(), t);
      }
      return;
  }
}

SynxValue _decodeValue(_Cursor cur, _StringTableReader t) {
  if (cur.pos >= cur.data.length) {
    throw const SynxBinaryError('unexpected end of data');
  }
  final tag = cur.data[cur.pos++];
  switch (tag) {
    case _tagNull:
      return synxNull();
    case _tagFalse:
      return synxBool(false);
    case _tagTrue:
      return synxBool(true);
    case _tagInt:
      return synxInt(_zigzagDecode(_readVarint(cur)));
    case _tagFloat:
      return synxFloat(_readF64LE(cur));
    case _tagString:
      return synxString(t.get(_readVarint(cur)));
    case _tagSecret:
      return synxSecret(t.get(_readVarint(cur)));
    case _tagArray:
      final count = _readVarint(cur);
      final arr = <SynxValue>[];
      for (var i = 0; i < count; i++) {
        arr.add(_decodeValue(cur, t));
      }
      return synxArray(arr);
    case _tagObject:
      final count = _readVarint(cur);
      final obj = SynxObject();
      for (var i = 0; i < count; i++) {
        final ki = _readVarint(cur);
        obj.set(t.get(ki), _decodeValue(cur, t));
      }
      return synxObject(obj);
  }
  throw SynxBinaryError('unknown type tag 0x${tag.toRadixString(16)}');
}

// ─── Metadata encode / decode ───────────────────────────────────────────────

void _encodeConstraints(BytesBuilder out, SynxConstraints c, _StringTable t) {
  var bits = 0;
  if (c.min != null) bits |= 0x01;
  if (c.max != null) bits |= 0x02;
  if (c.typeName != null) bits |= 0x04;
  if (c.required) bits |= 0x08;
  if (c.readonly) bits |= 0x10;
  if (c.pattern != null) bits |= 0x20;
  if (c.enumValues != null) bits |= 0x40;
  out.addByte(bits);
  if (c.min != null) _writeF64LE(out, c.min!);
  if (c.max != null) _writeF64LE(out, c.max!);
  if (c.typeName != null) _varint(out, t.indexOf(c.typeName!));
  if (c.pattern != null) _varint(out, t.indexOf(c.pattern!));
  if (c.enumValues != null) {
    _varint(out, c.enumValues!.length);
    for (final v in c.enumValues!) {
      _varint(out, t.indexOf(v));
    }
  }
}

SynxConstraints _decodeConstraints(_Cursor cur, _StringTableReader t) {
  if (cur.pos >= cur.data.length) {
    throw const SynxBinaryError('unexpected end in constraints');
  }
  final bits = cur.data[cur.pos++];
  final c = SynxConstraints();
  if ((bits & 0x01) != 0) c.min = _readF64LE(cur);
  if ((bits & 0x02) != 0) c.max = _readF64LE(cur);
  if ((bits & 0x04) != 0) c.typeName = t.get(_readVarint(cur));
  if ((bits & 0x08) != 0) c.required = true;
  if ((bits & 0x10) != 0) c.readonly = true;
  if ((bits & 0x20) != 0) c.pattern = t.get(_readVarint(cur));
  if ((bits & 0x40) != 0) {
    final count = _readVarint(cur);
    final vals = <String>[];
    for (var i = 0; i < count; i++) {
      vals.add(t.get(_readVarint(cur)));
    }
    c.enumValues = vals;
  }
  return c;
}

void _encodeMetadata(BytesBuilder out, SynxMetadataTree tree, _StringTable t) {
  final outerKeys = tree.keys.toList()..sort();
  _varint(out, outerKeys.length);
  for (final path in outerKeys) {
    _varint(out, t.indexOf(path));
    final m = tree[path]!;
    final innerKeys = m.keys.toList()..sort();
    _varint(out, innerKeys.length);
    for (final fk in innerKeys) {
      final meta = m[fk]!;
      _varint(out, t.indexOf(fk));
      _varint(out, meta.markers.length);
      for (final mk in meta.markers) {
        _varint(out, t.indexOf(mk));
      }
      _varint(out, meta.args.length);
      for (final a in meta.args) {
        _varint(out, t.indexOf(a));
      }
      if (meta.typeHint != null) {
        out.addByte(1);
        _varint(out, t.indexOf(meta.typeHint!));
      } else {
        out.addByte(0);
      }
      if (meta.constraints != null) {
        out.addByte(1);
        _encodeConstraints(out, meta.constraints!, t);
      } else {
        out.addByte(0);
      }
    }
  }
}

SynxMetadataTree _decodeMetadata(_Cursor cur, _StringTableReader t) {
  final outer = _readVarint(cur);
  final tree = <String, SynxMetaMap>{};
  for (var i = 0; i < outer; i++) {
    final path = t.get(_readVarint(cur));
    final inner = _readVarint(cur);
    final m = <String, SynxMeta>{};
    for (var j = 0; j < inner; j++) {
      final fk = t.get(_readVarint(cur));
      final meta = SynxMeta();
      final mc = _readVarint(cur);
      for (var k = 0; k < mc; k++) {
        meta.markers.add(t.get(_readVarint(cur)));
      }
      final ac = _readVarint(cur);
      for (var k = 0; k < ac; k++) {
        meta.args.add(t.get(_readVarint(cur)));
      }
      if (cur.pos >= cur.data.length) {
        throw const SynxBinaryError('unexpected end in meta (type_hint flag)');
      }
      final hasTh = cur.data[cur.pos++];
      if (hasTh != 0) meta.typeHint = t.get(_readVarint(cur));
      if (cur.pos >= cur.data.length) {
        throw const SynxBinaryError('unexpected end in meta (constraints flag)');
      }
      final hasC = cur.data[cur.pos++];
      if (hasC != 0) meta.constraints = _decodeConstraints(cur, t);
      m[fk] = meta;
    }
    tree[path] = m;
  }
  return tree;
}

void _encodeIncludes(
    BytesBuilder out, List<SynxIncludeDirective> incs, _StringTable t) {
  _varint(out, incs.length);
  for (final inc in incs) {
    _varint(out, t.indexOf(inc.path));
    _varint(out, t.indexOf(inc.alias));
  }
}

List<SynxIncludeDirective> _decodeIncludes(
    _Cursor cur, _StringTableReader t) {
  final count = _readVarint(cur);
  final out = <SynxIncludeDirective>[];
  for (var i = 0; i < count; i++) {
    final p = t.get(_readVarint(cur));
    final a = t.get(_readVarint(cur));
    out.add(SynxIncludeDirective(p, a));
  }
  return out;
}

// ─── UTF-8 helpers ──────────────────────────────────────────────────────────

Uint8List _encodeUtf8(String s) => Uint8List.fromList(utf8.encode(s));

String _decodeUtf8(List<int> bytes) =>
    utf8.decode(bytes, allowMalformed: true);
