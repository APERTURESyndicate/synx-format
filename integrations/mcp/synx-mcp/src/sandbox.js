/**
 * Filesystem sandbox for synx_read_path / synx_write_path / synx_apply_patch.
 *
 * Set exactly one of:
 *   SYNX_MCP_ROOT          — single allowed root directory (absolute path)
 *   SYNX_MCP_ROOTS         — comma-separated absolute roots (first match wins containment)
 *
 * All requested paths are resolved with path.resolve(); the result must be equal to a root
 * or start with root + path.sep (after normalization).
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export const MAX_FILE_BYTES = 10 * 1024 * 1024;

function normalizeRoot(p) {
  const resolved = path.resolve(p);
  if (!path.isAbsolute(resolved)) {
    throw new Error(`Sandbox root must be absolute: ${p}`);
  }
  const withSep =
    resolved.endsWith(path.sep) || resolved.endsWith('/') || resolved.endsWith('\\')
      ? resolved
      : resolved + path.sep;
  return { resolved, withSep };
}

export function getSandboxRoots() {
  const multi = process.env.SYNX_MCP_ROOTS?.trim();
  const single = process.env.SYNX_MCP_ROOT?.trim();
  const raw = multi || single || '';
  if (!raw) return [];
  return raw
    .split(',')
    .map((s) => s.trim())
    .filter(Boolean)
    .map((r) => normalizeRoot(r));
}

export function requireSandboxRoots() {
  const roots = getSandboxRoots();
  if (!roots.length) {
    throw new Error(
      'Filesystem MCP tools require SYNX_MCP_ROOT (one directory) or SYNX_MCP_ROOTS (comma-separated). Paths must be absolute.',
    );
  }
  return roots;
}

function pathContainedInRoot(resolvedFile, root) {
  const normFile = path.normalize(resolvedFile);
  if (normFile === root.resolved) return true;
  if (process.platform === 'win32') {
    const f = normFile.toLowerCase();
    const prefix = root.withSep.toLowerCase();
    return f.startsWith(prefix);
  }
  return normFile.startsWith(root.withSep);
}

/**
 * @param {string} userPath — absolute or relative; relative is resolved from cwd (discouraged — pass absolute under a root)
 */
export function resolveInSandbox(userPath) {
  const roots = requireSandboxRoots();
  const abs = path.resolve(userPath);
  for (const root of roots) {
    if (pathContainedInRoot(abs, root)) return abs;
  }
  throw new Error(
    `Path escapes sandbox (not under SYNX_MCP_ROOT[S]): ${userPath}\nResolved: ${abs}`,
  );
}

export function readTextSafe(absPath) {
  const st = fs.statSync(absPath);
  if (!st.isFile()) throw new Error(`Not a file: ${absPath}`);
  if (st.size > MAX_FILE_BYTES) {
    throw new Error(`File too large (${st.size} bytes, max ${MAX_FILE_BYTES})`);
  }
  return fs.readFileSync(absPath, 'utf8');
}

export function atomicWriteText(absPath, content) {
  const dir = path.dirname(absPath);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = path.join(
    dir,
    `.synx-mcp-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.tmp`,
  );
  try {
    fs.writeFileSync(tmp, content, 'utf8');
    fs.renameSync(tmp, absPath);
  } finally {
    try {
      fs.unlinkSync(tmp);
    } catch {
      /* ignore */
    }
  }
}

/**
 * @param {{ find: string, replace: string }[]} replacements — each `find` must appear exactly once
 */
export function applyReplacementsOnce(original, replacements) {
  let s = original;
  for (const { find, replace } of replacements) {
    if (!find) throw new Error('apply_patch: empty find');
    const n = s.split(find).length - 1;
    if (n === 0) throw new Error(`apply_patch: find not found (must appear exactly once): ${find.slice(0, 80)}${find.length > 80 ? '…' : ''}`);
    if (n > 1)
      throw new Error(`apply_patch: find is ambiguous (${n} occurrences), refine find string`);
    s = s.replace(find, replace);
  }
  return s;
}
