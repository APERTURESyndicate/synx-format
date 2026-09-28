/** A `:secret` value must serialise as the redaction mask in every JSON path,
 *  matching the Rust writer's `"[SECRET]"`, and must never leak the plaintext. */
import Synx from '../src/index';

describe('secret redaction', () => {
  test('a :secret value projects to "[SECRET]", not {} or plaintext', () => {
    const out = Synx.parse('!active\ntoken:secret abc123\n');
    const json = Synx.toJSON(out, false);
    expect(json).toContain('[SECRET]');
    expect(json).not.toContain('abc123');
  });
  test('a secret does not leak through {interpolation}', () => {
    const out = Synx.parse('!active\ntoken:secret abc123\nmsg hello {token} world\n');
    const json = Synx.toJSON(out, false);
    expect(json).not.toContain('abc123');
    expect((out as Record<string, unknown>).msg).toBe('hello [SECRET] world');
  });
});

describe('secret not leaked through nested interpolation', () => {
  test('a nested field interpolating a root-level secret sees [SECRET]', () => {
    const out = Synx.parse<{ section: { msg: string } }>(
      '!active\ntoken:secret abc123\nsection\n  msg use {token} now\n'
    );
    const json = Synx.toJSON(out, false);
    expect(json).not.toContain('abc123');
    expect(out.section.msg).toBe('use [SECRET] now');
  });
});
