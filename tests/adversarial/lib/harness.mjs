/**
 * Shared plumbing for the adversarial suite: builds the two workers from the
 * working tree, talks to them, and compares their answers.
 *
 * The Rust worker is `fuzz/` (cargo), the TypeScript worker is
 * `lib/ts-worker.cjs` over a `tsc` build of packages/synx-js/src. Both are
 * rebuilt from source on every run — never from a `dist/` or `target/` someone
 * else left behind.
 */
import { spawn, spawnSync } from 'node:child_process';
import { createInterface } from 'node:readline';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const LINE_BREAKS = new RegExp('[' + String.fromCharCode(0x2028, 0x2029, 0x85) + ']', 'g');

export const HERE = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const REPO = path.resolve(HERE, '..', '..');
export const TMP = path.join(HERE, 'tmp');
const EXE = process.platform === 'win32' ? '.exe' : '';

export function ensureDir(dir) {
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * Where every worker runs. A document resolved without a base directory
 * falls back to the process directory — `:once` writes `.synx.lock` there —
 * so the workers never run inside the repository.
 */
let workerCwd = null;
export function scratchCwd() {
  if (!workerCwd) {
    workerCwd = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-adversarial-cwd-'));
    process.on('exit', () => { try { fs.rmSync(workerCwd, { recursive: true, force: true }); } catch { /* scratch only */ } });
  }
  return workerCwd;
}

/** Build the Rust worker; returns the path of the executable. */
export function buildRust({ profile = 'release', features = [], quiet = false } = {}) {
  const manifest = path.join(REPO, 'fuzz', 'Cargo.toml');
  const args = ['build', '--manifest-path', manifest, '--offline'];
  if (profile === 'release') args.push('--release');
  else if (profile !== 'dev') args.push('--profile', profile);
  if (features.length) args.push('--features', features.join(','));
  const r = spawnSync('cargo', args, { stdio: quiet ? 'pipe' : ['ignore', 'inherit', 'inherit'], encoding: 'utf-8' });
  if (r.status !== 0) {
    throw new Error(`cargo build failed (${profile})\n${r.stderr || ''}`);
  }
  const targetDir = process.env.CARGO_TARGET_DIR || path.join(REPO, 'fuzz', 'target');
  const dirName = profile === 'dev' ? 'debug' : profile;
  return path.join(targetDir, dirName, 'advharness' + EXE);
}

/** Compile packages/synx-js/src with the project's own tsconfig; returns the output directory. */
export function buildTs({ quiet = false } = {}) {
  const pkg = path.join(REPO, 'packages', 'synx-js');
  const out = ensureDir(path.join(TMP, 'ts-current'));
  const tsc = path.join(pkg, 'node_modules', 'typescript', 'bin', 'tsc');
  const r = spawnSync(process.execPath, [
    tsc, '-p', path.join(pkg, 'tsconfig.json'), '--outDir', out,
    '--declaration', 'false', '--declarationMap', 'false', '--sourceMap', 'false',
  ], { encoding: 'utf-8' });
  // tsc still emits when there are type errors; only a missing output is fatal.
  if (!fs.existsSync(path.join(out, 'index.js'))) {
    throw new Error(`tsc produced no output\n${r.stdout}\n${r.stderr}`);
  }
  if (r.status !== 0 && !quiet) {
    process.stderr.write(`note: tsc reported type errors, output was still emitted\n${r.stdout.split('\n').slice(0, 8).join('\n')}\n`);
  }
  return out;
}

export const TS_WORKER = path.join(HERE, 'lib', 'ts-worker.cjs');

/** What a Windows process exit code means when it is not an ordinary status. */
export function describeExit(code, signal) {
  if (signal) return `signal ${signal}`;
  const known = {
    3221225725: 'stack overflow (0xC00000FD)',
    3221226505: 'abort / fast-fail (0xC0000409)',
    3221225477: 'access violation (0xC0000005)',
    3221225495: 'out of memory (0xC0000017)',
    134: 'abort (SIGABRT)',
    139: 'segmentation fault',
  };
  const u = code < 0 ? code + 4294967296 : code;
  return known[u] || `exit code ${code}`;
}

/**
 * One persistent worker process. Requests are answered in order; a request
 * that outlives its deadline or kills the process comes back as
 * `{ timeout: true }` or `{ crash: '…' }`, and the next call starts a new process.
 */
export class Worker {
  constructor(name, cmd, args = [], { cwd, env } = {}) {
    this.name = name;
    this.cmd = cmd;
    this.args = args;
    this.opts = { cwd, env };
    this.proc = null;
    this.pending = null;
    this.nextId = 1;
    this.starts = 0;
  }

  start() {
    this.starts++;
    this.stderr = '';
    this.proc = spawn(this.cmd, this.args, {
      cwd: this.opts.cwd || scratchCwd(),
      env: this.opts.env || process.env,
      stdio: ['pipe', 'pipe', 'pipe'],
      windowsHide: true,
    });
    const proc = this.proc;
    proc.stderr.on('data', (d) => { this.stderr = (this.stderr + d).slice(-4000); });
    // Lines end at `\n` only: readline also ends them at U+2028 / U+2029,
    // which both workers write unescaped inside JSON strings, and such an
    // answer was lost and reported as a timeout.
    let parts = [];
    proc.stdout.setEncoding('utf-8');
    proc.stdout.on('data', (chunk) => {
      // perf: a long answer arrives in many chunks; only the new one is searched.
      if (chunk.indexOf('\n') === -1) { parts.push(chunk); return; }
      let buf = parts.join('') + chunk;
      parts = [];
      let nl;
      while ((nl = buf.indexOf('\n')) !== -1) {
        const line = buf.slice(0, nl).replace(/\r$/, '');
        buf = buf.slice(nl + 1);
        if (buf.indexOf('\n') === -1) { if (buf) parts.push(buf); buf = ''; }
        const p = this.pending;
        if (!p) continue;
        let resp;
        try { resp = JSON.parse(line); } catch { continue; }
        this.pending = null;
        clearTimeout(p.timer);
        p.resolve(resp);
      }
    });
    proc.on('exit', (code, signal) => {
      if (this.proc === proc) this.proc = null;
      const p = this.pending;
      if (p && p.proc === proc) {
        this.pending = null;
        clearTimeout(p.timer);
        p.resolve(p.timedOut ? { timeout: true, ms: p.limit } : { crash: describeExit(code, signal), stderr: this.stderr.trim().slice(-600) });
      }
    });
    proc.stdin.on('error', () => {});
  }

  call(req, timeoutMs = 10000) {
    if (!this.proc) this.start();
    const proc = this.proc;
    return new Promise((resolve) => {
      const p = { resolve, proc, limit: timeoutMs, timedOut: false };
      p.timer = setTimeout(() => { p.timedOut = true; proc.kill(); }, timeoutMs);
      this.pending = p;
      // Node's readline ends a line at U+2028, U+2029 and U+0085 too; inside
      // a JSON string they may be escaped, so a request is always one line.
      const line = JSON.stringify({ id: this.nextId++, ...req }).replace(LINE_BREAKS, (c) => '\\u' + c.charCodeAt(0).toString(16).padStart(4, '0'));
      proc.stdin.write(line + '\n');
    });
  }

  close() {
    if (this.proc) {
      const proc = this.proc;
      this.proc = null;
      try { proc.stdin.end(); } catch { /* already gone */ }
      proc.kill();
    }
  }
}

/** A fresh process per request, with its own peak memory: for cost curves. */
export function once(cmd, args, req, { timeoutMs = 120000, cwd } = {}) {
  const dir = ensureDir(path.join(TMP, 'once'));
  const file = path.join(dir, `req-${process.pid}-${Math.random().toString(36).slice(2)}.json`);
  fs.writeFileSync(file, JSON.stringify(req));
  const started = process.hrtime.bigint();
  const r = spawnSync(cmd, [...args, 'once', file], { encoding: 'utf-8', timeout: timeoutMs, cwd: cwd || scratchCwd(), maxBuffer: 1 << 30, windowsHide: true });
  const wallMs = Number(process.hrtime.bigint() - started) / 1e6;
  fs.rmSync(file, { force: true });
  if (r.error && r.error.code === 'ETIMEDOUT') return { timeout: true, ms: timeoutMs };
  if (r.status !== 0 || !r.stdout) return { crash: describeExit(r.status, r.signal), stderr: (r.stderr || '').trim().slice(-600), wallMs };
  try {
    const lines = r.stdout.trim().split('\n');
    return { ...JSON.parse(lines[lines.length - 1]), wallMs };
  } catch {
    return { crash: 'unparsable answer', stderr: r.stdout.slice(0, 300), wallMs };
  }
}

export const b64 = (textOrBytes) => Buffer.from(textOrBytes).toString('base64');

/**
 * Canonical JSON with numbers reduced to their value. The two implementations
 * may write one number differently (`5.0` and `5`, `1e21` and `1e+21`): that
 * difference is tracked on its own and must not drown everything else.
 */
export function normJson(text) {
  if (typeof text !== 'string') return text;
  let out = '';
  let i = 0;
  const n = text.length;
  while (i < n) {
    const c = text[i];
    if (c === '"') {
      let j = i + 1;
      while (j < n && text[j] !== '"') j += text[j] === '\\' ? 2 : 1;
      out += text.slice(i, j + 1);
      i = j + 1;
    } else if (c === '-' || (c >= '0' && c <= '9')) {
      let j = i + 1;
      while (j < n && /[0-9.eE+\-]/.test(text[j])) j++;
      const tok = text.slice(i, j);
      // An integer token is exact and stays as written (it may be an i64
      // beyond 2^53); anything with a point or an exponent is a double and is
      // written the way JavaScript writes that double.
      if (/^-?\d+$/.test(tok)) out += tok.replace(/^-0$/, '0');
      else {
        const v = Number(tok);
        out += Object.is(v, -0) ? '-0' : String(v);
      }
      i = j;
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

/** The comparable part of an answer: value, typed refusal, or failure kind. */
export function verdict(resp, field = 'json') {
  if (!resp) return 'no answer';
  if (resp.timeout) return `TIMEOUT>${resp.ms}ms`;
  if (resp.crash) return `CRASH: ${resp.crash}`;
  if (resp.panic !== undefined) return `PANIC: ${resp.panic}`;
  if (resp.thrown) return `THROWN ${resp.name}: ${resp.err}`;
  if (!resp.ok) return `ERR: ${resp.err}`;
  const out = resp.out;
  if (out && typeof out === 'object' && field in out) return out[field];
  return JSON.stringify(out);
}

export function isFailure(resp) {
  return !resp || resp.timeout || resp.crash || resp.panic !== undefined || resp.thrown;
}

export function scratch(prefix = 'synx-adversarial-') {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}
