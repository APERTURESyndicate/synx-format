/**
 * The implementations a request can be sent to, by name:
 *
 *   rust          synx-core of the working tree, release profile
 *   rust-checked  the same with overflow checks and debug assertions
 *   rust-ref      synx-core at the release commit (built by compat.mjs snapshot)
 *   ts            packages/synx-js/src of the working tree, on Node
 *   ts-bun        the same on Bun
 *   ref           a published build of the TypeScript package (see local.json)
 *
 * `local.json` next to this suite (not tracked) holds machine-specific paths:
 *   { "referenceDist": "<…/node_modules/@aperturesyndicate/synx-format/dist>",
 *     "corpus": [ { "name": "…", "root": "<dir>", "match": "<regex on the relative path>" } ] }
 */
import fs from 'node:fs';
import path from 'node:path';
import { HERE, TMP, TS_WORKER, Worker, buildRust, buildTs, once, ensureDir } from './harness.mjs';

export function localConfig() {
  const file = path.join(HERE, 'local.json');
  if (!fs.existsSync(file)) return {};
  return JSON.parse(fs.readFileSync(file, 'utf-8'));
}

function newestMtime(dir) {
  let newest = 0;
  for (const name of fs.readdirSync(dir)) {
    const st = fs.statSync(path.join(dir, name));
    newest = Math.max(newest, st.isDirectory() ? newestMtime(path.join(dir, name)) : st.mtimeMs);
  }
  return newest;
}

let tsDir = null;
function currentTs(quiet) {
  if (tsDir) return tsDir;
  const src = path.join(HERE, '..', '..', 'packages', 'synx-js', 'src');
  const stamp = path.join(ensureDir(TMP), 'ts-current.stamp');
  const out = path.join(TMP, 'ts-current');
  const srcTime = newestMtime(src);
  if (fs.existsSync(stamp) && fs.existsSync(path.join(out, 'index.js')) && Number(fs.readFileSync(stamp, 'utf-8')) >= srcTime) {
    tsDir = out;
    return out;
  }
  tsDir = buildTs({ quiet });
  fs.writeFileSync(stamp, String(srcTime));
  return tsDir;
}

const rustExe = {};
function rust(profile, quiet) {
  if (!rustExe[profile]) rustExe[profile] = buildRust({ profile, quiet });
  return rustExe[profile];
}

/** `[command, args]` that start the named implementation's worker. */
export function command(name, { quiet = true } = {}) {
  switch (name) {
    case 'rust': return [rust('release', quiet), []];
    case 'rust-checked': return [rust('checked', quiet), []];
    case 'rust-ref': {
      // Built by `compat/compat.mjs snapshot` against synx-core at the release commit.
      const exe = path.join(process.env.CARGO_TARGET_DIR || path.join(HERE, '..', '..', 'fuzz', 'target'), 'ref', 'release', 'advharness' + (process.platform === 'win32' ? '.exe' : ''));
      if (!fs.existsSync(exe)) throw new Error('no Rust reference build: run "node tests/adversarial/compat/compat.mjs snapshot" first');
      return [exe, []];
    }
    case 'ts': return [process.execPath, [TS_WORKER, currentTs(quiet)]];
    case 'ts-bun': return ['bun', [TS_WORKER, currentTs(quiet)]];
    case 'ref': {
      const dist = process.env.SYNX_REFERENCE_DIST || localConfig().referenceDist;
      if (!dist || !fs.existsSync(path.join(dist, 'index.js'))) {
        throw new Error('no reference build: set SYNX_REFERENCE_DIST or "referenceDist" in tests/adversarial/local.json');
      }
      return [process.execPath, [TS_WORKER, dist]];
    }
    default: throw new Error(`unknown implementation '${name}'`);
  }
}

export function worker(name, opts) {
  const [cmd, args] = command(name, opts);
  return new Worker(name, cmd, args);
}

export function runOnce(name, req, opts = {}) {
  const [cmd, args] = command(name, opts);
  return once(cmd, args, req, opts);
}
