/**
 * The tools of the SYNX MCP server, as plain functions (no MCP SDK here, so
 * they run under `node --test` without an install).
 */
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  applyReplacementsOnce,
  assertNoMaskWrittenOverSecret,
  atomicWriteText,
  maskSecretLiterals,
  readTextSafe,
  resolveInSandbox,
} from './sandbox.js';

/** How long one `synx` run may take before it is killed. */
const CLI_TIMEOUT_MS = 60_000;

export function synxCommand() {
  return process.env.SYNX_CLI?.trim() || 'synx';
}

/** What the `synx` binary needs to start, on any platform. */
const BASE_ENV_NAMES = ['PATH', 'Path', 'SystemRoot', 'SYSTEMROOT', 'windir', 'TEMP', 'TMP', 'TMPDIR'];

/**
 * The environment of the `synx` child process.
 *
 * safety: `synx parse` and `synx validate` resolve an `!active` document, and
 * the text comes from the model: `x:env ANTHROPIC_API_KEY` returned the
 * server's own key in the tool result. The child sees what it needs to
 * start, plus the names the operator lists in SYNX_MCP_PASS_ENV.
 */
export function childEnv() {
  const passed = (process.env.SYNX_MCP_PASS_ENV ?? '')
    .split(',')
    .map((name) => name.trim())
    .filter(Boolean);
  const env = {};
  for (const name of [...BASE_ENV_NAMES, ...passed]) {
    if (process.env[name] !== undefined) env[name] = process.env[name];
  }
  return env;
}

export function runSynx(args) {
  const cmd = synxCommand();
  const r = spawnSync(cmd, args, {
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    windowsHide: true,
    env: childEnv(),
    timeout: CLI_TIMEOUT_MS,
  });
  const err = r.error ? String(r.error.message) : '';
  return {
    ok: r.status === 0,
    code: r.status ?? -1,
    stdout: (r.stdout || '').trimEnd(),
    stderr: (r.stderr || '').trimEnd(),
    err,
  };
}

function withTempSynx(prefix, text, fn) {
  const dir = mkdtempSync(join(tmpdir(), `synx-mcp-${prefix}-`));
  const file = join(dir, 'file.synx');
  try {
    writeFileSync(file, text, 'utf8');
    return fn(file);
  } finally {
    try {
      rmSync(dir, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
}

function errorMessage(e) {
  return e instanceof Error ? e.message : String(e);
}

function runHint(r, what) {
  return r.err ? `Failed to run "${synxCommand()}". ${what}` : undefined;
}

/** `synx validate` on a file under the sandbox roots. */
export function validatePath({ path: filePath, strict }) {
  let abs;
  try {
    // safety: the path went to the CLI as given — any file on the machine,
    // a UNC share, or an argument starting with `-`.
    abs = resolveInSandbox(filePath);
  } catch (e) {
    return { ok: false, exitCode: -1, stdout: '', stderr: errorMessage(e) };
  }
  const args = ['validate', abs];
  if (strict) args.push('--strict');
  const r = runSynx(args);
  return {
    ok: r.ok,
    exitCode: r.code,
    stdout: r.stdout,
    stderr: r.stderr || r.err,
    hint: runHint(r, 'Set SYNX_CLI to the binary path.'),
  };
}

export function validateText({ text, strict }) {
  const out = withTempSynx('val', text, (file) => {
    const args = ['validate', file];
    if (strict) args.push('--strict');
    return runSynx(args);
  });
  return {
    ok: out.ok,
    exitCode: out.code,
    stdout: out.stdout,
    stderr: out.stderr || out.err,
    hint: runHint(out, 'Set SYNX_CLI or install synx-cli.'),
  };
}

export function parseJson({ text, active }) {
  const out = withTempSynx('parse', text, (file) => {
    const args = ['parse', file];
    if (active) args.push('--active');
    return runSynx(args);
  });
  return {
    ok: out.ok,
    json: out.ok ? out.stdout : null,
    stderr: out.stderr || out.err,
  };
}

export function formatText({ text }) {
  const out = withTempSynx('fmt', text, (file) => runSynx(['format', file]));
  return {
    ok: out.ok,
    formatted: out.ok ? out.stdout : null,
    stderr: out.stderr || out.err,
  };
}

export function readPath({ path: filePath }) {
  try {
    const abs = resolveInSandbox(filePath);
    const { text, masked } = maskSecretLiterals(readTextSafe(abs));
    const result = { ok: true, path: abs, text };
    if (masked > 0) {
      result.maskedSecrets = masked;
      result.note = `${masked} literal :secret value(s) are shown as [SECRET]; edit other lines with synx_apply_patch.`;
    }
    return result;
  } catch (e) {
    return { ok: false, error: errorMessage(e) };
  }
}

function readIfPresent(abs) {
  try {
    return readTextSafe(abs);
  } catch {
    return null;
  }
}

export function writePath({ path: filePath, text }) {
  try {
    const abs = resolveInSandbox(filePath);
    const current = readIfPresent(abs);
    if (current !== null) assertNoMaskWrittenOverSecret(current, text);
    atomicWriteText(abs, text);
    return { ok: true, path: abs, bytes: Buffer.byteLength(text, 'utf8') };
  } catch (e) {
    return { ok: false, error: errorMessage(e) };
  }
}

export function applyPatch({ path: filePath, replacements }) {
  try {
    const abs = resolveInSandbox(filePath);
    const original = readTextSafe(abs);
    const next = applyReplacementsOnce(original, replacements);
    atomicWriteText(abs, next);
    return { ok: true, path: abs, replacementsApplied: replacements.length };
  } catch (e) {
    return { ok: false, error: errorMessage(e) };
  }
}
