/**
 * Secrets must not reach any string a host can print; file markers must stay
 * inside the base directory and answer the same way in both implementations.
 *
 * `sinks` resolves the document and renders it every way the library offers
 * (JSON, Display/toString, debug output, serde, stringify, YAML/TOML/env,
 * `.synxb`, diff, strict-mode error) and reports which renderings contain the
 * needle.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { agree, all, lacks, survive } from '../lib/expect.mjs';

const A = '!active\n';

/** No rendering may contain the secret. `allow` lists renderings exempt by design. */
const noSink = (allow = []) => (view) => {
  const failed = survive(view);
  if (failed) return failed;
  for (const [name, r] of Object.entries(view.answers)) {
    const hits = ((r.out && r.out.hits) || []).filter((h) => !allow.includes(h));
    if (hits.length) return `${name}: plaintext in ${hits.join(', ')}`;
  }
  return null;
};

const secret = (id, zone, title, input, extra = {}) => ({
  id, zone, title, op: 'sinks', input,
  req: { needle: 'hunter2', env: { PW: 'hunter2' }, ...(extra.req || {}) },
  // A resolved `.synxb` stores the value by design; everything else is a leak.
  expect: noSink(['synxb_resolved', 'synxb_default']),
  ...extra,
  ...(extra.req ? { req: { needle: 'hunter2', env: { PW: 'hunter2' }, ...extra.req } } : {}),
});

/** A base directory with a file inside, a file outside, and links from inside to outside. */
function jailSetup() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-jail-'));
  const base = path.join(root, 'base');
  const outside = path.join(root, 'outside');
  fs.mkdirSync(path.join(base, 'sub'), { recursive: true });
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(base, 'inside.synx'), 'ok INSIDE\n');
  fs.writeFileSync(path.join(base, 'data.json'), JSON.stringify({ a: { b: 1 }, list: [{ name: 'item0' }, { name: 'item1' }] }));
  fs.writeFileSync(path.join(outside, 'secret.synx'), 'token OUTSIDE_SECRET_123\n');
  if (process.platform === 'win32') {
    spawnSync('cmd', ['/c', 'mklink', '/J', path.join(base, 'link'), outside]);
    spawnSync('cmd', ['/c', 'mklink', path.join(base, 'sym.synx'), path.join(outside, 'secret.synx')]);
  } else {
    try { fs.symlinkSync(outside, path.join(base, 'link')); fs.symlinkSync(path.join(outside, 'secret.synx'), path.join(base, 'sym.synx')); } catch { /* no links here */ }
  }
  return { root, base, outside, abs: path.join(outside, 'secret.synx') };
}
function jailCleanup(ctx) {
  try { fs.rmdirSync(path.join(ctx.base, 'link')); } catch { try { fs.unlinkSync(path.join(ctx.base, 'link')); } catch { /* not there */ } }
  fs.rmSync(ctx.root, { recursive: true, force: true });
}
const jail = (id, title, target, marker = 'include') => ({
  id, zone: 'both', title: `${marker} ${title} stays inside the base directory`,
  op: 'active', setup: jailSetup, cleanup: jailCleanup,
  input: (ctx) => (marker === '!include'
    ? `!include ${target(ctx)} al\n${A}x {token:al}\n`
    : `${A}x:${marker} ${target(ctx)}\n`),
  req: (ctx) => ({ base: ctx.base }),
  expect: all(survive, lacks('OUTSIDE_SECRET')),
});

export default [
  // ── secrets ──
  { id: 'B-024', zone: 'ts', title: ':secret on the first key of a list item is masked', op: 'active', input: `${A}users\n  - password:secret hunter2\n    name bob\n`, expect: all(agree, lacks('hunter2')) },
  secret('B-027.1', 'core', 'literal :secret in Display, serde and JSON-schema conversion', `${A}password:secret hunter2\n`),
  secret('B-027.2', 'core', ':secret:env in Display, serde and JSON-schema conversion', `${A}db\n  password:secret:env PW\n`),
  secret('B-027.3', 'core', 'inherited :secret', `${A}base\n  password:secret hunter2\nprod:inherit:base\n  x 1\n`),
  { id: 'B-025', zone: 'ts', title: ':secret:once is a secret in both', op: 'active', input: `${A}password:secret:once\n`, setup: () => fs.mkdtempSync(path.join(os.tmpdir(), 'synx-once-')), cleanup: (d) => fs.rmSync(d, { recursive: true, force: true }), req: (d) => ({ base: d }), expect: agree },
  { id: 'B-026.1', zone: 'both', title: ':secret:split — one answer', op: 'active', input: `${A}password:secret:split:, hunter2,x\n`, expect: agree },
  { id: 'B-026.2', zone: 'both', title: ':secret on a list', op: 'active', input: `${A}keys:secret\n  - hunter2\n  - other\n`, expect: agree },
  { id: 'B-026.3', zone: 'both', title: ':secret:default on a key without a value', op: 'active', input: `${A}password:secret:default:hunter2\n`, expect: agree },
  { id: 'B-026.4', zone: 'both', title: ':secret on a group', op: 'active', input: `${A}creds:secret\n  user bob\n  pass hunter2\n`, expect: agree },

  // ── path jail ──
  jail('B-039.1', 'of ../outside', () => '../outside/secret.synx'),
  jail('B-039.2', 'with backslashes', () => '..\\outside\\secret.synx'),
  jail('B-039.3', 'through sub/../..', () => 'sub/../../outside/secret.synx'),
  jail('B-039.4', 'of an absolute path', (c) => c.abs),
  jail('B-039.5', 'of a drive-relative path', (c) => c.abs.slice(0, 2) + 'outside\\secret.synx'),
  jail('B-039.6', 'of a \\\\?\\ path', (c) => '\\\\?\\' + c.abs),
  jail('B-039.7', 'of a UNC path', (c) => '\\\\localhost\\C$' + c.abs.slice(2)),
  jail('B-039.8', 'through a junction', () => 'link/secret.synx'),
  jail('B-039.9', 'through a junction, other case', () => 'LINK/SECRET.SYNX'),
  jail('B-039.10', 'of a file symlink', () => 'sym.synx'),
  jail('B-039.11', 'through a junction (watch)', () => 'link/secret.synx', 'watch'),
  jail('B-039.12', 'of ../outside (watch)', () => '../outside/secret.synx', 'watch'),
  jail('B-039.13', 'through a junction (!include)', () => 'link/secret.synx', '!include'),
  jail('B-039.14', 'of ../outside (!include)', () => '../outside/secret.synx', '!include'),
  jail('B-039.15', 'through link/../outside', () => 'link/../outside/secret.synx'),

  // The refusal is part of the result: both implementations must word it alike.
  { id: 'B-040.1', zone: 'both', title: 'drive-relative path: same refusal', op: 'active', setup: jailSetup, cleanup: jailCleanup, input: () => `${A}x:include C:outside\\secret.synx\n`, req: (c) => ({ base: c.base }), expect: agree },
  { id: 'B-040.2', zone: 'both', title: 'name the file system cannot hold: same refusal', op: 'active', setup: jailSetup, cleanup: jailCleanup, input: () => `${A}x:include ${'a'.repeat(300)}.synx\ny:watch http://127.0.0.1:9/x.synx\n`, req: (c) => ({ base: c.base }), expect: agree },
  { id: 'B-041', zone: 'both', title: ':watch key path into a JSON array', op: 'active', setup: jailSetup, cleanup: jailCleanup, input: () => `${A}a:watch:list.0.name data.json\nb:watch:list.length data.json\nc:watch:a.b data.json\n`, req: (c) => ({ base: c.base }), expect: agree },
];
