/**
 * Filesystem sandbox for synx_validate_path / synx_read_path / synx_write_path / synx_apply_patch.
 *
 * Set exactly one of:
 *   SYNX_MCP_ROOT          — single allowed root directory (absolute path)
 *   SYNX_MCP_ROOTS         — comma-separated absolute roots (first match wins containment)
 *
 * A requested path is resolved with path.resolve() and then to its real path
 * (links followed); the real path must be equal to a root's real path or lie
 * under it. Credential files and version-control directories are refused
 * unless SYNX_MCP_ALLOW_SECRET_FILES=1.
 */
import fs from 'node:fs';
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
  if (process.platform === 'win32') {
    const f = normFile.toLowerCase();
    if (f === root.resolved.toLowerCase()) return true;
    return f.startsWith(root.withSep.toLowerCase());
  }
  if (normFile === root.resolved) return true;
  return normFile.startsWith(root.withSep);
}

/**
 * Real path of `abs`: links followed for the part that exists, the rest
 * (a file about to be created, its missing parent directories) appended as
 * written.
 */
function realOfExistingPrefix(abs) {
  let probe = abs;
  const rest = [];
  for (;;) {
    try {
      return path.join(fs.realpathSync.native(probe), ...rest);
    } catch {
      const up = path.dirname(probe);
      if (up === probe) return abs;
      rest.unshift(path.basename(probe));
      probe = up;
    }
  }
}

const SECRET_DIRS = new Set(['.git', '.svn', '.hg', '.ssh', '.aws', '.gnupg']);
const SECRET_NAMES = new Set([
  'id_rsa',
  'id_dsa',
  'id_ecdsa',
  'id_ed25519',
  '.npmrc',
  '.pypirc',
  '.netrc',
  '.htpasswd',
  'credentials.toml',
  'signing-key.hex',
  // The list `synx pack` refuses too (crates/synx-cli/src/pkg_guard.rs).
  '_netrc',
  '.envrc',
  '.pgpass',
  '.my.cnf',
  '.git-credentials',
  '.dockercfg',
  'credentials',
  'kubeconfig',
]);
/** Private keys under a suffix: `id_rsa_github`, `id_ed25519.old` (not `.pub`). */
const SECRET_KEY_PREFIXES = ['id_rsa', 'id_dsa', 'id_ecdsa', 'id_ed25519'];
const SECRET_EXTENSIONS = ['.pem', '.key', '.p12', '.pfx', '.p8', '.jks', '.keystore', '.kdbx'];
const ENV_TEMPLATES = new Set(['example', 'sample', 'template', 'dist']);

/**
 * Whether a path names a credential file or lies in a directory that holds
 * them. Judged on every component, so `root/.git/config` and
 * `root/.ssh/known_hosts` count.
 */
export function isSecretPath(filePath) {
  const parts = filePath.split(/[\\/]+/).filter(Boolean);
  const name = (parts[parts.length - 1] ?? '').toLowerCase();
  if (parts.some((part) => SECRET_DIRS.has(part.toLowerCase()))) return true;
  // safety: `.env` as a part of the name after a dot — `.env`, `.env.local`,
  // `prod.env`, `docker.env.production`. Only names starting with `.env` were
  // caught, so `prod.env` next to it was read. A template (`.env.example`)
  // holds no values; `env.synx` is an ordinary document.
  const env = /\.env(?:\.(.*))?$/.exec(name);
  if (env) return !ENV_TEMPLATES.has(env[1] ?? '');
  if (SECRET_NAMES.has(name)) return true;
  if (SECRET_KEY_PREFIXES.some((p) => name.startsWith(p)) && !name.endsWith('.pub')) return true;
  return SECRET_EXTENSIONS.some((ext) => name.endsWith(ext));
}

function allowsSecretFiles() {
  return process.env.SYNX_MCP_ALLOW_SECRET_FILES?.trim() === '1';
}

const WINDOWS_DEVICE = /^(con|prn|aux|nul|conin\$|conout\$|com[0-9¹²³]|lpt[0-9¹²³])(\..*)?$/i;

/**
 * Whether Windows would open something other than the file the text names:
 * a device (`NUL`, `con.txt`), an alternate data stream (`a.synx:x`,
 * `.env::$DATA`) or a name it rewrites (trailing dot or space).
 */
export function hasWindowsPathTrap(filePath) {
  const parts = filePath.split(/[\\/]+/).filter(Boolean);
  return parts.some((part, i) => {
    const isDrive = i === 0 && /^[a-z]:$/i.test(part);
    if (isDrive || part === '.' || part === '..' || part === '?') return false;
    return part.includes(':') || /[. ]$/.test(part) || WINDOWS_DEVICE.test(part);
  });
}

/**
 * The real path of `userPath`, which must lie under a sandbox root.
 *
 * safety: containment was judged on the text of the path. A symlink or a
 * junction inside the root leads anywhere, and both reads and writes went
 * through it. The path returned is the real one, so the caller does not pass
 * through the link again.
 *
 * @param {string} userPath — absolute or relative; relative is resolved from cwd (discouraged — pass absolute under a root)
 */
export function resolveInSandbox(userPath) {
  const roots = requireSandboxRoots();
  if (typeof userPath !== 'string' || userPath.includes('\0')) {
    throw new Error('Path must be a string without NUL bytes');
  }
  const abs = path.resolve(userPath);
  // safety: judged on the text first — resolving a link means asking the
  // disk, and for `\\host\share\x` that is a connection to `host`.
  if (!roots.some((root) => pathContainedInRoot(abs, root))) {
    throw new Error(`Path escapes sandbox (not under SYNX_MCP_ROOT[S]): ${userPath}\nResolved: ${abs}`);
  }
  if (process.platform === 'win32' && hasWindowsPathTrap(abs)) {
    throw new Error(`Refused: ${userPath} names a device, a data stream, or ends in a dot or a space`);
  }
  const real = realOfExistingPrefix(abs);
  for (const root of roots) {
    const realRoot = normalizeRoot(realOfExistingPrefix(root.resolved));
    if (!pathContainedInRoot(real, realRoot)) continue;
    // safety: a model reading `.env` or `.git/config`, or writing a git hook,
    // is outside what a SYNX tool is for; the operator opts in explicitly.
    if (!allowsSecretFiles() && isSecretPath(path.relative(realRoot.resolved, real))) {
      throw new Error(
        `Refused: ${userPath} looks like a credential file or a version-control directory (set SYNX_MCP_ALLOW_SECRET_FILES=1 to allow)`,
      );
    }
    return real;
  }
  throw new Error(
    `Path escapes sandbox (not under SYNX_MCP_ROOT[S]): ${userPath}\nResolved: ${real}`,
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
  if (Buffer.byteLength(content, 'utf8') > MAX_FILE_BYTES) {
    throw new Error(`Content too large (max ${MAX_FILE_BYTES} bytes)`);
  }
  const dir = path.dirname(absPath);
  fs.mkdirSync(dir, { recursive: true });
  const tmp = path.join(
    dir,
    `.synx-mcp-${process.pid}-${Date.now()}-${Math.random().toString(16).slice(2)}.tmp`,
  );
  try {
    // `wx`: the temporary is created, never opened through something already there.
    fs.writeFileSync(tmp, content, { encoding: 'utf8', flag: 'wx' });
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
    // safety: a string replacement still expands `$$`, `$&`, `` $` `` and `$'`;
    // `pa$$w0rd` was written as `pa$w0rd`. A function result is taken as is.
    s = s.replace(find, () => replace);
  }
  return s;
}

/** Markers whose key holds a reference (a variable name, a path), not the secret itself. */
const SECRET_SOURCE_MARKERS = new Set([
  'env', 'include', 'import', 'watch', 'ref', 'alias', 'calc', 'map', 'i18n', 'default', 'fallback',
  'once', 'split', 'join', 'format', 'replace', 'random', 'template', 'inherit', 'prompt',
]);

export const SECRET_MASK = '[SECRET]';

/**
 * A key line as `{ indent, head, gap, value }`; the head ends at the first
 * whitespace outside `[...]` and `(...)`. Null for a line that is not a key.
 */
function splitKeyLine(line) {
  const indent = line.length - line.trimStart().length;
  let body = line.slice(indent);
  let lead = '';
  if (body.startsWith('- ')) {
    lead = '- ';
    body = body.slice(2);
  }
  if (!body || body.startsWith('#') || body.startsWith('//') || body.startsWith('!')) return null;
  let depth = 0;
  let end = body.length;
  for (let i = 0; i < body.length; i++) {
    const c = body[i];
    if (c === '[' || c === '(') depth++;
    else if ((c === ']' || c === ')') && depth > 0) depth--;
    else if ((c === ' ' || c === '\t') && depth === 0) {
      end = i;
      break;
    }
  }
  const head = body.slice(0, end);
  const rest = body.slice(end);
  const gap = rest.slice(0, rest.length - rest.trimStart().length);
  return { indent, lead, head, gap, value: rest.slice(gap.length) };
}

function isLiteralSecretHead(head) {
  // Drop `[constraints]` and `(type)`: a colon inside them is not a marker.
  const bare = head.replace(/\[[^\]]*\]/g, '').replace(/\([^)]*\)/g, '');
  const markers = bare.split(':').slice(1);
  return markers.includes('secret') && !markers.some((m) => SECRET_SOURCE_MARKERS.has(m));
}

/**
 * Replaces the value of every literal `:secret` key with `[SECRET]`.
 * Line count and every other character stay as they are.
 *
 * why: a `.synx` file can hold `db_password:secret hunter2`; the format keeps
 * that out of every output it produces, and a model reading the file is one
 * more output.
 *
 * @returns {{ text: string, masked: number }}
 */
export function maskSecretLiterals(text) {
  const lines = text.split('\n');
  let masked = 0;
  let blockIndent = -1;
  let isInComment = false;
  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const cr = raw.endsWith('\r') ? '\r' : '';
    const line = cr ? raw.slice(0, -1) : raw;
    const trimmed = line.trim();
    if (trimmed === '###') {
      isInComment = !isInComment;
      blockIndent = -1;
      continue;
    }
    if (isInComment) continue;
    const indent = line.length - line.trimStart().length;
    if (blockIndent >= 0) {
      if (!trimmed) continue;
      if (indent > blockIndent) {
        lines[i] = `${line.slice(0, indent)}${SECRET_MASK}${cr}`;
        continue;
      }
      blockIndent = -1;
    }
    const parts = splitKeyLine(line);
    if (!parts || !isLiteralSecretHead(parts.head)) continue;
    if (parts.value === '|' || parts.value === '|+') {
      blockIndent = parts.indent;
      masked++;
      continue;
    }
    if (!parts.value || parts.value === SECRET_MASK) continue;
    lines[i] = `${line.slice(0, parts.indent)}${parts.lead}${parts.head}${parts.gap}${SECRET_MASK}${cr}`;
    masked++;
  }
  return { text: lines.join('\n'), masked };
}

/**
 * Throws if writing `next` over `current` would replace a literal secret with
 * its mask — a model that read the masked text and wrote it back.
 */
export function assertNoMaskWrittenOverSecret(current, next) {
  if (!next.includes(SECRET_MASK)) return;
  const maskedCurrent = maskSecretLiterals(current);
  if (maskedCurrent.masked === 0) return;
  const before = current.split('\n');
  const shown = maskedCurrent.text.split('\n');
  const wanted = new Set(next.split('\n').map((l) => l.replace(/\r$/, '')));
  for (let i = 0; i < before.length; i++) {
    const real = before[i].replace(/\r$/, '');
    const mask = shown[i].replace(/\r$/, '');
    if (real !== mask && wanted.has(mask) && !wanted.has(real)) {
      throw new Error(
        'Refused: the new text holds the [SECRET] mask where the file holds a secret value. ' +
          'Use synx_apply_patch for the lines you mean to change.',
      );
    }
  }
}
