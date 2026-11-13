'use strict';
/**
 * Worker for the TypeScript implementation. Same line protocol and operation
 * names as the Rust worker (fuzz/src/main.rs), so one driver can ask both the
 * same question.
 *
 *   node ts-worker.cjs <dir with index.js, parser.js, json.js, engine.js, calc.js>
 *
 * The directory is a `tsc` build of packages/synx-js/src, or the `dist` of a
 * published package — that is how a released version answers the same
 * requests as the working tree.
 */
const path = require('path');
const util = require('util');
const zlib = require('zlib');

const dist = path.resolve(process.argv[2] || '.');
const Synx = require(path.join(dist, 'index.js'));
const { parseData } = require(path.join(dist, 'parser.js'));
const { toCanonicalJSONString } = require(path.join(dist, 'json.js'));
const { safeCalc } = require(path.join(dist, 'calc.js'));
const SynxError = Synx.SynxError;

const canon = (v) => toCanonicalJSONString(v, false);

/** SYNXL hard-error conditions under the names synx-core gives them (synxl.rs). */
const HARD = {
  MissingOrMalformedPrologue: 'MissingPrologue', UnsupportedFormatVersion: 'UnsupportedVersion',
  RepeatedPrologueVersionMismatch: 'UnsupportedVersion', RecordWithoutFieldList: 'NoFieldList',
  DuplicateFieldName: 'DuplicateField', MarkerChainInFieldDecl: 'MarkerChain',
  NonDeterministicTypeHint: 'NonDeterministicHint', BlockCombinedWithType: 'BlockWithType', ZeroArityFieldList: 'ZeroArity',
};
const hardOf = (e) => (e && typeof e.condition === 'string' ? { hard: HARD[e.condition] || e.condition, line: e.line } : null);

/** JSON with keys in the order the object holds them (the canonical writer sorts). */
function ordered(v, depth = 0) {
  if (depth > 300) return 'null';
  if (Array.isArray(v)) return '[' + v.map((x) => ordered(x, depth + 1)).join(',') + ']';
  if (v && typeof v === 'object' && v[Symbol.for('synx.secret')] !== true) {
    return '{' + Object.keys(v).filter((k) => k !== '__synx').map((k) => JSON.stringify(k) + ':' + ordered(v[k], depth + 1)).join(',') + '}';
  }
  return canon(v);
}

function bytesOf(req, field) {
  const s = req[field];
  return s ? Buffer.from(s, 'base64') : Buffer.alloc(0);
}

function textOf(req, field) {
  if (field === 'b64' && typeof req.text === 'string') return req.text;
  // A JavaScript string can hold what UTF-8 cannot: `units` is a list of
  // UTF-16 code units, for lone surrogates.
  if (field === 'b64' && Array.isArray(req.units)) return String.fromCharCode(...req.units);
  return new util.TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytesOf(req, field));
}

function optionsOf(req) {
  const o = {};
  // Absent means an empty environment: the host's is read only on `env: null`.
  if (req.env === null) { /* process.env */ } else o.env = req.env && typeof req.env === 'object' ? req.env : {};
  if (req.base) o.basePath = req.base;
  if (req.lang) o.lang = req.lang;
  if (req.region) o.region = req.region;
  if (typeof req.depth === 'number') o.maxIncludeDepth = req.depth;
  if (req.strict) o.strict = true;
  return o;
}

function resolvedRoot(text, opts) {
  const mode = parseData(text).mode;
  return [Synx.parse(text, opts), mode === 'active'];
}

function inflateRaw(data) {
  try { return zlib.inflateRawSync(Buffer.from(data).subarray(11)).toString('latin1'); } catch { return ''; }
}

function collectDisplay(v, out) {
  out.push(String(v), `${v}`, v === null || v === undefined ? '' : (typeof v.toString === 'function' ? v.toString() : ''));
  if (v && typeof v === 'object' && !(v[Symbol.for('synx.secret')] === true)) {
    for (const k of Object.keys(v)) collectDisplay(v[k], out);
  }
}

const ops = {
  ping: () => 'pong',

  parse(req) {
    const r = parseData(textOf(req, 'b64'));
    return {
      json: canon(r.root),
      // `brief` is the cost mode: one rendering, as a caller would make.
      ordered: req.brief ? '' : ordered(r.root),
      keys: Object.keys(r.root).length,
      active: r.mode === 'active',
      locked: !!r.locked,
      tool: !!r.tool,
      schema: !!r.schema,
      llm: !!r.llm,
      includes: (r.includes || []).map((i) => [i.path, i.alias]),
    };
  },

  active(req) {
    const [root, active] = resolvedRoot(textOf(req, 'b64'), optionsOf(req));
    return { json: canon(root), ordered: req.brief ? '' : ordered(root), active };
  },

  tool(req) {
    return { json: canon(Synx.parseTool(textOf(req, 'b64'), optionsOf(req))) };
  },

  format(req) {
    return { text: Synx.format(textOf(req, 'b64')) };
  },

  format_check(req) {
    const text = textOf(req, 'b64');
    const opts = optionsOf(req);
    const f1 = Synx.format(text);
    const f2 = Synx.format(f1);
    const [r0, m0] = resolvedRoot(text, opts);
    const [r1, m1] = resolvedRoot(f1, opts);
    return { f1, idempotent: f1 === f2, f2: f1 === f2 ? null : f2, p0: canon(r0), p1: canon(r1), m0, m1 };
  },

  stringify(req) {
    return { text: Synx.stringify(parseData(textOf(req, 'b64')).root) };
  },

  stringify_check(req) {
    const r = parseData(textOf(req, 'b64'));
    const s = Synx.stringify(r.root);
    const back = parseData(s);
    return { p0: canon(r.root), p1: canon(back.root), text: s, active1: back.mode === 'active' };
  },

  json2synx(req) {
    const v = JSON.parse(textOf(req, 'b64'));
    const s = Synx.stringify(v);
    const back = parseData(s);
    return {
      p0: canon(v), p1: canon(back.root), text: s,
      active1: back.mode === 'active', includes1: (back.includes || []).length,
    };
  },

  compile(req) {
    const data = Synx.compile(textOf(req, 'b64'), !!req.resolved);
    return { b64: Buffer.from(data).toString('base64'), len: data.length };
  },

  decompile(req) {
    return { text: Synx.decompile(bytesOf(req, 'b64')) };
  },

  // The TypeScript package has no typed decoder; the text form is all there is.
  decompile_tree(req) {
    return { json: canon(parseData(Synx.decompile(bytesOf(req, 'b64'))).root) };
  },

  // The TypeScript package decodes a container only to text; what it reads back as.
  decompile_full(req) {
    const text = Synx.decompile(bytesOf(req, 'b64'));
    const back = parseData(text);
    return { text_tree: canon(back.root), text_active: back.mode === 'active', text_includes: (back.includes || []).length, text };
  },

  compile_check(req) {
    const text = textOf(req, 'b64');
    const r0 = parseData(text);
    const data = Synx.compile(text, false);
    const back = Synx.decompile(data);
    const r2 = parseData(back);
    return { p0: canon(r0.root), p2: canon(r2.root), mode_equal: r0.mode === r2.mode, text: back };
  },

  synxl(req) {
    try {
      const doc = Synx.parseSynxl(textOf(req, 'b64'), { validate: !!req.validate });
      return {
        json: Synx.synxlToJSON(doc),
        diagnostics: doc.diagnostics.map((d) => [d.kind, d.record, d.line]),
      };
    } catch (e) {
      const hard = hardOf(e);
      if (hard) return hard;
      throw e;
    }
  },

  synxl_check(req) {
    const text = textOf(req, 'b64');
    let doc;
    try { doc = Synx.parseSynxl(text); } catch (e) {
      const hard = hardOf(e);
      if (hard) return { hard: hard.hard };
      throw e;
    }
    const j0 = Synx.synxlToJSON(doc);
    const written = Synx.writeSynxl(doc);
    let j1;
    try { j1 = Synx.synxlToJSON(written); } catch (e) { j1 = 'HARD:' + (e && e.message); }
    let stream;
    let streamErr = null;
    try { stream = '[' + [...Synx.streamSynxl(text)].map((r) => canon(r.values)).join(',') + ']'; } catch (e) { stream = ''; streamErr = String(e && e.message); }
    return { j0, j1, written, stream, stream_err: streamErr };
  },

  calc(req) {
    try {
      const n = safeCalc(textOf(req, 'b64'));
      return { num: Number.isNaN(n) ? 'NaN' : n === Infinity ? 'Infinity' : n === -Infinity ? '-Infinity' : String(n) };
    } catch (e) {
      if (e instanceof RangeError) throw e;
      return { calc_err: e.message };
    }
  },

  diff(req) {
    const a = parseData(textOf(req, 'b64')).root;
    const b = parseData(textOf(req, 'b64b')).root;
    return { json: canon(Synx.diff(a, b)) };
  },

  schema(req) {
    return { json: JSON.stringify(Synx.schema(textOf(req, 'b64'))) };
  },

  // One `[pattern:…]` against several values, through the package's own
  // linear matcher: does it compile, and which values match.
  regex(req) {
    const P = require(path.join(dist, 'pattern.js'));
    const prog = P.compileLinear(String(req.pattern));
    const values = Array.isArray(req.values) ? req.values.map(String) : [];
    return { compiled: prog !== null, m: prog === null ? [] : values.map((v) => P.runLinear(prog, v)) };
  },

  // The bytes of `b64` as a file on disk, read by the package's own loaders.
  load_file(req) {
    const fs = require('fs');
    const file = path.join(fs.mkdtempSync(path.join(require('os').tmpdir(), 'synx-load-')), req.name || 'in.synx');
    fs.writeFileSync(file, bytesOf(req, 'b64'));
    try {
      if (file.endsWith('.synxl')) return { json: canon(Synx.loadSynxlSync(file).records.map((r) => r.values)) };
      return { json: canon(Synx.loadSync(file, optionsOf(req))) };
    } finally {
      fs.rmSync(path.dirname(file), { recursive: true, force: true });
    }
  },

  // The bytes of `b64` handed to the parser as bytes, not as a string.
  parse_bytes(req) {
    return { json: canon(Synx.parse(new Uint8Array(bytesOf(req, 'b64')), optionsOf(req))) };
  },

  sinks(req) {
    const text = textOf(req, 'b64');
    const needle = req.needle || '';
    const root = Synx.parse(text, optionsOf(req));
    const sinks = [];
    const add = (name, fn) => { try { sinks.push([name, String(fn())]); } catch (e) { sinks.push([name + '!throws', String(e && e.message)]); } };
    add('canonical_json', () => canon(root));
    add('JSON.stringify', () => JSON.stringify(root, (k, v) => (typeof v === 'bigint' ? v.toString() : v)));
    add('toJSON', () => Synx.toJSON(root));
    add('stringify', () => Synx.stringify(root));
    add('toYAML', () => Synx.toYAML(root));
    add('toTOML', () => Synx.toTOML(root));
    add('toEnv', () => Synx.toEnv(root));
    add('inspect', () => util.inspect(root, { depth: null, showHidden: true, getters: true }));
    add('inspect_proxy', () => util.inspect(root, { depth: null, showProxy: true, showHidden: true }));
    add('display', () => { const out = []; collectDisplay(root, out); return out.join('\n'); });
    add('entries', () => {
      const out = [];
      (function walk(v) {
        if (!v || typeof v !== 'object') return;
        for (const k of Reflect.ownKeys(v)) {
          const d = Object.getOwnPropertyDescriptor(v, k);
          out.push(String(k), d && 'value' in d ? util.format('%o', d.value) : '');
          if (d && d.value && typeof d.value === 'object') walk(d.value);
        }
      })(root);
      return out.join('\n');
    });
    add('structuredClone', () => util.inspect(structuredClone(JSON.parse(JSON.stringify(root, (k, v) => (typeof v === 'bigint' ? v.toString() : v)))), { depth: null }));
    add('diff', () => canon(Synx.diff(root, {})));
    add('synxb_resolved', () => inflateRaw(Synx.compile(text, true)));
    add('synxb_default', () => inflateRaw(Synx.compile(text)));
    add('decompile', () => Synx.decompile(Synx.compile(text, true)));
    add('strict_error', () => { try { Synx.parse(text, { ...optionsOf(req), strict: true }); return ''; } catch (e) { return `${e.name}: ${e.message}\n${e.stack}`; } });
    const hits = needle ? sinks.filter(([, s]) => s.includes(needle)).map(([n]) => n) : [];
    return { hits, json: canon(root), thrown: sinks.filter(([n]) => n.endsWith('!throws')).map(([n, s]) => `${n}: ${s}`) };
  },
};

function handle(req) {
  const started = process.hrtime.bigint();
  let resp;
  try {
    const fn = ops[req.op];
    if (!fn) throw new Error(`unknown op '${req.op}'`);
    const out = fn(req);
    // `brief`: only the size of each long string comes back.
    const shorten = (v) => (typeof v === 'string' ? (v.length > 200 ? { len: v.length } : v)
      : Array.isArray(v) ? v.map(shorten)
        : v && typeof v === 'object' ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, shorten(x)])) : v);
    resp = { id: req.id, ok: true, out: req.brief ? shorten(out) : out };
  } catch (e) {
    const typed = e instanceof SynxError;
    resp = {
      id: req.id,
      ok: false,
      err: String(e && e.message),
      name: e && e.name,
      typed,
      // An exception that is not the package's own error type is a defect on
      // any input; the driver decides what a typed one means for the operation.
      thrown: !typed,
      at: typed ? undefined : String(e && e.stack || '').split('\n').slice(1, 4).map((l) => l.trim().replace(/\\/g, '/').replace(/.*\/(?=[^/]+:\d+:\d+\)?$)/, '')).join(' < '),
    };
  }
  resp.ms = Math.round(Number(process.hrtime.bigint() - started) / 1000) / 1000;
  resp.peak_mb = Math.round(process.resourceUsage().maxRSS / 102.4) / 10;
  return resp;
}

if (process.argv[3] === 'once') {
  const req = JSON.parse(require('fs').readFileSync(process.argv[4], 'utf-8'));
  process.stdout.write(JSON.stringify(handle(req)) + '\n');
} else {
  const rl = require('readline').createInterface({ input: process.stdin, crlfDelay: Infinity });
  rl.on('line', (line) => {
    if (!line.trim()) return;
    let resp;
    try { resp = handle(JSON.parse(line)); } catch (e) { resp = { ok: false, err: 'bad request: ' + e.message }; }
    process.stdout.write(JSON.stringify(resp) + '\n');
  });
}
