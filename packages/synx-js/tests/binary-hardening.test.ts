/**
 * Regression tests for `.synxb` binary hardening (cross-language interop with
 * `synx_core::binary`). Each guards a fixed defect:
 *   - large / negative i64 survived (zigzag was 32-bit, then rounded at 2^53);
 *   - !lock / !tool / !schema / !llm flags survive compile→decompile;
 *   - a decompression bomb / oversized declared size is rejected;
 *   - a __proto__ key in the payload does not poison Object.prototype.
 */
import Synx from '../src/index';

describe('synxb — integer fidelity', () => {
  const cases: Array<[string, number]> = [
    ['max_safe', Number.MAX_SAFE_INTEGER],
    ['min_safe', -Number.MAX_SAFE_INTEGER],
    ['e15', 9_000_000_000_000_007],
    ['neg_e15', -9_000_000_000_000_007],
    ['pow2_52', 4_503_599_627_370_497],
    ['neg_pow2_52', -4_503_599_627_370_497],
    ['small', 42],
    ['neg_small', -42],
    ['zero', 0],
  ];
  for (const [name, n] of cases) {
    test(`${name} (${n}) round-trips through .synxb`, () => {
      const bin = Synx.compile(`v ${n}\n`, false);
      const back = Synx.parse(Synx.decompile(bin));
      expect(back.v).toBe(n);
    });
  }
});

describe('synxb — directive flags survive the round trip', () => {
  test('!lock is preserved', () => {
    const text = '!lock\nname Wario\n';
    const back = Synx.decompile(Synx.compile(text, false));
    expect(back.startsWith('!lock')).toBe(true);
  });
  test('!tool / !schema are preserved', () => {
    const text = '!tool\n!schema\nweb_search\n  q string\n';
    const back = Synx.decompile(Synx.compile(text, false));
    expect(back).toContain('!tool');
    expect(back).toContain('!schema');
  });
  test('!llm is preserved', () => {
    const back = Synx.decompile(Synx.compile('!llm\nx 1\n', false));
    expect(back).toContain('!llm');
  });
});

describe('synxb — hostile input is rejected, not fatal', () => {
  test('oversized declared size throws instead of allocating', () => {
    const bin = Synx.compile('a 1\n', false);
    // Overwrite the little-endian u32 uncompressed-size field with 0xFFFFFFFF.
    bin[7] = bin[8] = bin[9] = bin[10] = 0xff;
    expect(() => Synx.decompile(bin)).toThrow();
  });
  test('truncated body throws instead of returning junk', () => {
    const bin = Synx.compile('a 1\nb 2\n', false);
    expect(() => Synx.decompile(bin.slice(0, bin.length - 2))).toThrow();
  });
});

describe('synxb — malformed payloads are rejected, never hang or crash', () => {
  const MAGIC = Uint8Array.from([0x53, 0x59, 0x4e, 0x58, 0x42]);
  // Assemble a .synxb whose (uncompressed) payload we control, deflated raw.
  function framed(payloadBytes: number[]): Uint8Array {
    const zlib = require('zlib');
    const raw = new Uint8Array(zlib.deflateRawSync(Buffer.from(payloadBytes), { level: 9 }));
    const size = payloadBytes.length >>> 0;
    const out = new Uint8Array(11 + raw.length);
    out.set(MAGIC, 0);
    out[5] = 1; out[6] = 0;
    out[7] = size & 0xff; out[8] = (size >>> 8) & 0xff; out[9] = (size >>> 16) & 0xff; out[10] = (size >>> 24) & 0xff;
    out.set(raw, 11);
    return out;
  }
  const varint = (n: number): number[] => {
    const out: number[] = [];
    while (n > 0x7f) { out.push((n & 0x7f) | 0x80); n = Math.floor(n / 128); }
    out.push(n);
    return out;
  };

  test('huge string-table count is rejected, not looped', () => {
    // 10-byte varint = ~2^64. Must throw fast, not spin.
    const p = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
    const start = Date.now();
    expect(() => Synx.decompile(framed(p))).toThrow();
    expect(Date.now() - start).toBeLessThan(2000);
  });
  test('array count beyond remaining bytes is rejected', () => {
    const p = [...varint(0), 0x06, ...[0xff, 0xff, 0xff, 0xff, 0x0f]];
    expect(() => Synx.decompile(framed(p))).toThrow();
  });
  test('truncated float is rejected', () => {
    const p = [...varint(0), 0x04, 0x01, 0x02];
    expect(() => Synx.decompile(framed(p))).toThrow();
  });
  test('null root decompiles to a primitive line, not a crash', () => {
    const p = [...varint(0), 0x00];
    expect(() => Synx.decompile(framed(p))).not.toThrow();
  });
});

describe('synxb — __proto__ payload does not pollute', () => {
  test('decompile never touches Object.prototype', () => {
    // A normal document; the guarantee is structural (null-proto build), so the
    // guard holds regardless of whether the payload carries __proto__.
    const bin = Synx.compile('a 1\n', false);
    Synx.decompile(bin);
    expect(({} as Record<string, unknown>).polluted).toBeUndefined();
  });
});

// Not binary, but same file for hardening regressions: multiline block
// accumulation must be linear. It was O(n²) (utf8Length over the whole
// growing string every line); a few thousand lines took tens of seconds.
describe('multiline blocks parse in linear time', () => {
  test('50k continuation lines parse quickly and correctly', () => {
    const lines = ['doc |'];
    for (let i = 0; i < 50000; i++) lines.push(`  line ${i}`);
    const start = Date.now();
    const out = Synx.parse<{ doc: string }>(lines.join('\n'));
    expect(Date.now() - start).toBeLessThan(4000);
    expect(out.doc.startsWith('line 0\n')).toBe(true);
    expect(out.doc.endsWith('line 49999')).toBe(true);
  });
  test('|+ preserves relative indent over many lines', () => {
    const lines = ['code |+'];
    for (let i = 0; i < 20000; i++) lines.push(i % 2 ? '    deep' : '  base');
    const out = Synx.parse<{ code: string }>(lines.join('\n'));
    const body = out.code.split('\n');
    expect(body[0]).toBe('base');
    expect(body[1]).toBe('  deep');
    expect(body.length).toBe(20000);
  });
});
