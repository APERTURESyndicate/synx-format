/**
 * Hand-built `.synxb` containers: the header and the deflate wrapping are
 * correct, the payload is whatever the case wants it to be.
 *
 * Layout (crates/synx-core/src/binary.rs): "SYNXB", version 1, flags,
 * uncompressed size u32 LE, then raw deflate of
 *   string table: varint count, then varint length + UTF-8 bytes each
 *   value tree:   tag 0 null · 1 false · 2 true · 3 int (zigzag varint) ·
 *                 4 float (8 bytes LE) · 5 string (varint index) ·
 *                 6 array (varint count, values) ·
 *                 7 object (varint count, varint key index + value) ·
 *                 8 secret (varint index)
 */
import zlib from 'node:zlib';

/** LEB128 of a non-negative integer (number or bigint, up to 2^64 − 1). */
export function varint(n) {
  let v = BigInt(n);
  const out = [];
  do {
    let b = Number(v & 0x7fn);
    v >>= 7n;
    if (v > 0n) b |= 0x80;
    out.push(b);
  } while (v > 0n);
  return Buffer.from(out);
}

export const U64_MAX = (1n << 64n) - 1n;

export function stringTable(strings) {
  return Buffer.concat([varint(strings.length), ...strings.flatMap((s) => {
    const b = Buffer.isBuffer(s) ? s : Buffer.from(s, 'utf-8');
    return [varint(b.length), b];
  })]);
}

export const T = { nil: 0, f: 1, t: 2, int: 3, float: 4, str: 5, arr: 6, obj: 7, secret: 8 };

/** Wrap a payload. `declared` overrides the size written in the header. */
export function container(payload, { flags = 0, declared, version = 1, level = 9 } = {}) {
  const body = zlib.deflateRawSync(payload, { level });
  const head = Buffer.alloc(11);
  head.write('SYNXB', 0, 'latin1');
  head[5] = version;
  head[6] = flags;
  head.writeUInt32LE((declared ?? payload.length) >>> 0, 7);
  return Buffer.concat([head, body]);
}

/** A payload that is `tag` followed by `count` as its element count and then `fill`. */
export function counted(tag, count, fill = Buffer.alloc(0), strings = []) {
  return Buffer.concat([stringTable(strings), Buffer.from([tag]), varint(count), fill]);
}
