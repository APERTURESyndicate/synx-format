// What the filesystem tools of the MCP server must refuse.
// Run: node --test   (no install needed — nothing here imports the MCP SDK)
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  applyReplacementsOnce,
  assertNoMaskWrittenOverSecret,
  hasWindowsPathTrap,
  isSecretPath,
  maskSecretLiterals,
  resolveInSandbox,
} from '../src/sandbox.js';
import { applyPatch, childEnv, parseJson, readPath, validatePath, writePath } from '../src/tools.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));

function scratch(name) {
  return fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), `synx-mcp-test-${name}-`)));
}

/** Runs `fn` with the environment changed, then puts it back. */
function withEnv(vars, fn) {
  const saved = {};
  for (const [key, value] of Object.entries(vars)) {
    saved[key] = process.env[key];
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
  try {
    return fn();
  } finally {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
}

/** A directory link: a junction on Windows (no privilege needed), a symlink elsewhere. */
function linkDir(target, link) {
  try {
    fs.symlinkSync(target, link, process.platform === 'win32' ? 'junction' : 'dir');
    return true;
  } catch {
    return false;
  }
}

/** The `synx` binary built from this repository, if there is one. */
function findCli() {
  const exe = process.platform === 'win32' ? 'synx.exe' : 'synx';
  const repo = path.resolve(HERE, '../../../..');
  const candidates = [
    process.env.SYNX_CLI,
    ...['target-sy3', 'target'].flatMap((dir) => ['debug', 'release'].map((p) => path.join(repo, dir, p, exe))),
  ];
  return candidates.find((p) => p && fs.existsSync(p));
}

test('a path outside the root is refused, inside is returned as a real path', () => {
  const dir = scratch('contain');
  const root = path.join(dir, 'root');
  fs.mkdirSync(path.join(root, 'sub'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'outside.synx'), 'k v\n');
  withEnv({ SYNX_MCP_ROOT: root, SYNX_MCP_ROOTS: undefined }, () => {
    assert.equal(resolveInSandbox(path.join(root, 'sub', 'new.synx')), path.join(root, 'sub', 'new.synx'));
    assert.equal(resolveInSandbox(root), root);
    for (const bad of [
      path.join(dir, 'outside.synx'),
      path.join(root, '..', 'outside.synx'),
      path.join(root, 'sub', '..', '..', 'outside.synx'),
      `${root}-sibling${path.sep}x.synx`,
      'a\0b',
    ]) {
      assert.throws(() => resolveInSandbox(bad), /sandbox|NUL/, bad);
    }
  });
  withEnv({ SYNX_MCP_ROOT: undefined, SYNX_MCP_ROOTS: undefined }, () => {
    assert.throws(() => resolveInSandbox(path.join(root, 'x.synx')), /require SYNX_MCP_ROOT/);
  });
});

test('a link inside the root does not lead out of it', (t) => {
  const dir = scratch('link');
  const root = path.join(dir, 'root');
  const outside = path.join(dir, 'outside');
  fs.mkdirSync(root);
  fs.mkdirSync(outside);
  fs.writeFileSync(path.join(outside, 'secret.txt'), 'top-secret');
  if (!linkDir(outside, path.join(root, 'link'))) {
    t.skip('cannot create a directory link here');
    return;
  }
  withEnv({ SYNX_MCP_ROOT: root, SYNX_MCP_ROOTS: undefined }, () => {
    const read = readPath({ path: path.join(root, 'link', 'secret.txt') });
    assert.equal(read.ok, false);
    assert.match(read.error, /escapes sandbox/);

    const written = writePath({ path: path.join(root, 'link', 'planted.txt'), text: 'x' });
    assert.equal(written.ok, false);
    assert.equal(fs.existsSync(path.join(outside, 'planted.txt')), false);

    const deep = writePath({ path: path.join(root, 'link', 'new-dir', 'planted.txt'), text: 'x' });
    assert.equal(deep.ok, false);
    assert.equal(fs.existsSync(path.join(outside, 'new-dir')), false);

    const patched = applyPatch({
      path: path.join(root, 'link', 'secret.txt'),
      replacements: [{ find: 'top', replace: 'no' }],
    });
    assert.equal(patched.ok, false);
    assert.equal(fs.readFileSync(path.join(outside, 'secret.txt'), 'utf8'), 'top-secret');

    const validated = validatePath({ path: path.join(root, 'link', 'secret.txt') });
    assert.equal(validated.ok, false);
    assert.match(validated.stderr, /escapes sandbox/);
  });

  // A root that is itself reached through a link keeps working.
  const alias = path.join(dir, 'alias');
  assert.ok(linkDir(root, alias));
  fs.writeFileSync(path.join(root, 'ok.synx'), 'k v\n');
  withEnv({ SYNX_MCP_ROOT: alias, SYNX_MCP_ROOTS: undefined }, () => {
    assert.equal(readPath({ path: path.join(alias, 'ok.synx') }).text, 'k v\n');
  });
});

test('synx_validate_path stays in the sandbox and never passes an option to the CLI', () => {
  const dir = scratch('validate');
  const root = path.join(dir, 'root');
  fs.mkdirSync(root);
  fs.writeFileSync(path.join(dir, 'outside.synx'), 'k v\n');
  // A CLI that does not exist: reaching it at all would show up as a run failure.
  withEnv({ SYNX_MCP_ROOT: root, SYNX_MCP_ROOTS: undefined, SYNX_CLI: path.join(dir, 'no-such-synx') }, () => {
    for (const bad of [path.join(dir, 'outside.synx'), '\\\\127.0.0.1\\share\\x.synx', '--strict', '-h']) {
      const result = validatePath({ path: bad });
      assert.equal(result.ok, false, bad);
      assert.equal(result.hint, undefined, `${bad}: the CLI was started`);
      assert.match(result.stderr, /sandbox|Refused/, bad);
    }
  });
  withEnv({ SYNX_MCP_ROOT: undefined, SYNX_MCP_ROOTS: undefined, SYNX_CLI: path.join(dir, 'no-such-synx') }, () => {
    const result = validatePath({ path: path.join(dir, 'outside.synx') });
    assert.equal(result.hint, undefined);
    assert.match(result.stderr, /require SYNX_MCP_ROOT/);
  });
});

test('credential files and version-control directories are refused unless allowed', () => {
  for (const secret of [
    '.env', '.env.local', '.env.production', 'config/.env', '.git/config', '.git/hooks/pre-commit',
    'a/.ssh/known_hosts', 'id_rsa', 'deploy/server.key', 'cert.PEM', 'AuthKey_ABC.p8', '.npmrc',
    'credentials.toml',
    // `.env` after any name: these were read while `.env` next to them was refused.
    'prod.env', 'docker.env', 'config/local.env', 'app.env.production', 'PROD.ENV', 'a.env.example.bak',
    // The names `synx pack` refuses (B-091).
    '_netrc', '.envrc', '.pgpass', '.my.cnf', '.git-credentials', '.dockercfg', 'credentials', 'kubeconfig',
    'id_rsa_github',
  ]) {
    assert.equal(isSecretPath(secret), true, secret);
  }
  for (const plain of [
    '.env.example', 'app.synx', 'config/app.synx', 'README.md', 'keys.synx', 'gitlog.txt',
    'env.synx', 'environment.synx', 'prod.env.example', 'app.environment', 'envoy.synx', 'id_rsa.pub',
    'credentials.md',
  ]) {
    assert.equal(isSecretPath(plain), false, plain);
  }

  const root = scratch('secret-files');
  fs.writeFileSync(path.join(root, '.env'), 'TOKEN=abc\n');
  fs.mkdirSync(path.join(root, '.git', 'hooks'), { recursive: true });
  withEnv({ SYNX_MCP_ROOT: root, SYNX_MCP_ROOTS: undefined, SYNX_MCP_ALLOW_SECRET_FILES: undefined }, () => {
    const read = readPath({ path: path.join(root, '.env') });
    assert.equal(read.ok, false);
    assert.doesNotMatch(JSON.stringify(read), /abc/);
    const hook = writePath({ path: path.join(root, '.git', 'hooks', 'pre-commit'), text: '#!/bin/sh\n' });
    assert.equal(hook.ok, false);
    assert.equal(fs.existsSync(path.join(root, '.git', 'hooks', 'pre-commit')), false);
  });
  withEnv({ SYNX_MCP_ROOT: root, SYNX_MCP_ROOTS: undefined, SYNX_MCP_ALLOW_SECRET_FILES: '1' }, () => {
    assert.equal(readPath({ path: path.join(root, '.env') }).text, 'TOKEN=abc\n');
  });
});

test('Windows device names, data streams and rewritten names are refused', () => {
  for (const trap of [
    'C:\\root\\NUL', 'C:\\root\\con.txt', 'C:\\root\\sub\\COM1', 'C:\\root\\a.synx:stream',
    'C:\\root\\.env::$DATA', 'C:\\root\\.env ', 'C:\\root\\dir.\\x.synx', '\\\\?\\C:\\root\\x.synx',
  ]) {
    assert.equal(hasWindowsPathTrap(trap), true, trap);
  }
  for (const fine of ['C:\\root\\app.synx', 'C:\\root\\null\\console.synx', '\\\\server\\share\\x.synx']) {
    assert.equal(hasWindowsPathTrap(fine), false, fine);
  }
  if (process.platform !== 'win32') return;
  const root = scratch('win');
  fs.writeFileSync(path.join(root, '.env'), 'TOKEN=abc\n');
  withEnv({ SYNX_MCP_ROOT: root, SYNX_MCP_ROOTS: undefined }, () => {
    for (const trap of ['NUL', 'a.synx:stream', '.env::$DATA', '.env ', '.env.']) {
      const read = readPath({ path: path.join(root, trap) });
      assert.equal(read.ok, false, trap);
      assert.doesNotMatch(JSON.stringify(read), /abc/, trap);
    }
  });
});

test('apply_patch replaces text literally', () => {
  const patched = (find, replace) => applyReplacementsOnce('password OLD\nrest of file\n', [{ find, replace }]);
  assert.equal(patched('OLD', 'pa$$w0rd'), 'password pa$$w0rd\nrest of file\n');
  assert.equal(patched('OLD', '$&$&'), 'password $&$&\nrest of file\n');
  assert.equal(patched('OLD', "x$'y"), "password x$'y\nrest of file\n");
  assert.equal(patched('OLD', 'x$`y'), 'password x$`y\nrest of file\n');
  assert.throws(() => patched('missing', 'x'), /not found/);
  assert.throws(() => applyReplacementsOnce('a a', [{ find: 'a', replace: 'b' }]), /ambiguous/);
});

test('literal :secret values are masked on read, references are not', () => {
  const source = [
    '!active',
    'port 8080',
    'db_password:secret hunter2',
    'api_key[required]:secret sk-live-123',
    'typed(string):secret   spaced value',
    'from_env:secret:env API_KEY',
    'with_default:secret:env:default:dev JWT',
    'nested',
    '  token:secret abc # trailing',
    'servers',
    '  - pass:secret zzz',
    'pem:secret |',
    '  -----BEGIN KEY-----',
    '  MIIabc',
    'after secret-looking value',
    '###',
    'commented:secret keepme',
    '###',
    'empty:secret',
    '',
  ].join('\n');
  const { text, masked } = maskSecretLiterals(source);
  assert.equal(masked, 6);
  for (const leaked of ['hunter2', 'sk-live-123', 'spaced value', 'abc # trailing', 'zzz', 'BEGIN KEY', 'MIIabc']) {
    assert.equal(text.includes(leaked), false, leaked);
  }
  const lines = text.split('\n');
  assert.equal(lines.length, source.split('\n').length);
  assert.equal(lines[2], 'db_password:secret [SECRET]');
  assert.equal(lines[4], 'typed(string):secret   [SECRET]');
  assert.equal(lines[5], 'from_env:secret:env API_KEY');
  assert.equal(lines[6], 'with_default:secret:env:default:dev JWT');
  assert.equal(lines[8], '  token:secret [SECRET]');
  assert.equal(lines[10], '  - pass:secret [SECRET]');
  assert.equal(lines[12], '  [SECRET]');
  assert.equal(lines[14], 'after secret-looking value');
  assert.equal(lines[16], 'commented:secret keepme');

  // CRLF files keep their line endings.
  assert.equal(maskSecretLiterals('a:secret x\r\nb 1\r\n').text, 'a:secret [SECRET]\r\nb 1\r\n');
});

test('a masked read cannot be written back over the secret', () => {
  const root = scratch('mask');
  const file = path.join(root, 'app.synx');
  fs.writeFileSync(file, '!active\nport 8080\ndb_password:secret hunter2\n');
  withEnv({ SYNX_MCP_ROOT: root, SYNX_MCP_ROOTS: undefined }, () => {
    const read = readPath({ path: file });
    assert.equal(read.maskedSecrets, 1);
    assert.equal(read.text.includes('hunter2'), false);

    const overwrite = writePath({ path: file, text: read.text.replace('8080', '9090') });
    assert.equal(overwrite.ok, false);
    assert.match(overwrite.error, /\[SECRET\] mask/);
    assert.equal(fs.readFileSync(file, 'utf8').includes('hunter2'), true);

    const patched = applyPatch({ path: file, replacements: [{ find: 'port 8080', replace: 'port 9090' }] });
    assert.equal(patched.ok, true);
    assert.equal(fs.readFileSync(file, 'utf8'), '!active\nport 9090\ndb_password:secret hunter2\n');
  });
  // Text that sets the secret itself is an ordinary write.
  assert.doesNotThrow(() => assertNoMaskWrittenOverSecret('a:secret old\n', 'a:secret new\n'));
  assert.doesNotThrow(() => assertNoMaskWrittenOverSecret('a 1\n', 'note [SECRET]\n'));
  assert.throws(() => assertNoMaskWrittenOverSecret('a:secret old\n', 'a:secret [SECRET]\n'));
});

test('the synx child process does not inherit the server environment', (t) => {
  withEnv({ SYNX_MCP_TEST_SECRET: 'sk-test-123', SYNX_MCP_PASS_ENV: undefined }, () => {
    const env = childEnv();
    assert.equal(env.SYNX_MCP_TEST_SECRET, undefined);
    assert.ok(Object.keys(env).every((k) => /^(path|systemroot|windir|temp|tmp|tmpdir)$/i.test(k)), Object.keys(env).join());
  });
  withEnv({ SYNX_MCP_TEST_SECRET: 'sk-test-123', SYNX_MCP_PASS_ENV: ' SYNX_MCP_TEST_SECRET , OTHER ' }, () => {
    assert.equal(childEnv().SYNX_MCP_TEST_SECRET, 'sk-test-123');
  });

  const cli = findCli();
  if (!cli) {
    t.skip('no synx binary built in this repository');
    return;
  }
  assert.match(execFileSync(cli, ['--version'], { encoding: 'utf8' }), /synx/);
  const text = '!active\nleak:env SYNX_MCP_TEST_SECRET\nport:env:default:8080 NOPE_UNSET\n';
  withEnv({ SYNX_CLI: cli, SYNX_MCP_TEST_SECRET: 'sk-test-123', SYNX_MCP_PASS_ENV: undefined }, () => {
    const result = parseJson({ text });
    assert.equal(result.ok, true, result.stderr);
    assert.equal(result.json.includes('sk-test-123'), false, result.json);
    assert.equal(JSON.parse(result.json).port, 8080);
  });
  withEnv({ SYNX_CLI: cli, SYNX_MCP_TEST_SECRET: 'sk-test-123', SYNX_MCP_PASS_ENV: 'SYNX_MCP_TEST_SECRET' }, () => {
    assert.equal(JSON.parse(parseJson({ text }).json).leak, 'sk-test-123');
  });

  // A document handed over as text cannot include files of the machine.
  const outside = path.join(scratch('include'), 'secret.synx');
  fs.writeFileSync(outside, 'token OUTSIDE\n');
  withEnv({ SYNX_CLI: cli }, () => {
    const result = parseJson({ text: `!active\nx:include ${outside}\ny:include ../../${path.basename(outside)}\n` });
    assert.equal(result.json.includes('OUTSIDE'), false, result.json);
  });
});
