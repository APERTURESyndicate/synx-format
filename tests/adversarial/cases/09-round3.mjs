/**
 * Round three: classes found after the linear `[pattern]` matcher, the decoder
 * budgets and the declaration writer landed. Each case wants the two
 * implementations to agree unless it says otherwise.
 */
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { agree, all, field, lacks, survive, within } from '../lib/expect.mjs';
import { worker } from '../lib/impls.mjs';
import { SHAPES } from '../lib/shapes.mjs';
import { T, container, stringTable, varint } from '../lib/synxb.mjs';

/** A static container whose root is not an object. */
const rootOnly = (strings, ...value) => container(Buffer.concat([stringTable(strings), Buffer.from(value.flat())]), { flags: 0 });

/** A cost case over a named shape at size `n` (as in 02-cost.mjs). */
function cost(id, zone, shape, n, ms, mb, extra = {}) {
  const make = SHAPES[shape][1];
  return {
    id, zone, title: `${shape}, n=${n}: under ${ms} ms and ${mb} MiB`,
    once: true, timeout: Math.max(30000, ms * 10),
    op: make(1).op,
    input: () => make(n).input,
    req: () => ({ brief: true, ...(make(n).req || {}) }),
    expect: all(survive, within(ms)),
    ...extra,
  };
}

/**
 * Processes of both implementations ask one directory at the same time: for
 * one shared `:once` key every process must be given the same value, and the
 * keys each process owns must read back unchanged.
 */
async function onceAcrossImplementations(impls) {
  const base = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-once-mixed-'));
  try {
    const procs = [];
    for (let i = 0; i < 4; i++) for (const n of impls) procs.push([n, worker(n)]);
    const given = new Map();
    const shared = new Set();
    await Promise.all(procs.map(async ([, wk], w) => {
      for (let r = 0; r < 8; r++) {
        const doc = `!active\nshared_${r}:once\nown_${w}_${r}:once\n`;
        const resp = await wk.call({ op: 'active', text: doc, base }, 20000);
        if (!resp.ok) continue;
        const v = JSON.parse(resp.out.json);
        shared.add(`${r}=${v[`shared_${r}`]}`);
        given.set(`own_${w}_${r}`, v[`own_${w}_${r}`]);
      }
    }));
    procs.forEach(([, wk]) => wk.close());
    const sharedValues = new Map();
    for (const s of shared) { const [r] = s.split('='); sharedValues.set(r, (sharedValues.get(r) || 0) + 1); }
    const split = [...sharedValues.values()].filter((c) => c > 1).length;
    if (split) return `${split} of 8 shared :once keys were given different values to different processes`;
    for (const n of impls) {
      const single = worker(n);
      let changed = 0;
      for (const [k, v] of given) {
        const resp = await single.call({ op: 'active', text: `!active\n${k}:once\n`, base }, 20000);
        if (resp.ok && JSON.parse(resp.out.json)[k] !== v) changed++;
      }
      single.close();
      if (changed) return `${n}: ${changed} of ${given.size} values differ on the next read`;
    }
    return null;
  } finally {
    fs.rmSync(base, { recursive: true, force: true });
  }
}

const A = '!active\n';
const B = '\\';
const cp = (...codes) => String.fromCodePoint(...codes);
const both = (id, zone, title, op, input, extra = {}) => ({ id, zone, title, op, input, expect: agree, ...extra });

export default [
  // [pattern] in TypeScript (pattern.ts) against the `regex` crate the core uses.
  both('B-083.1', 'ts', '[pattern] (?i) set before | covers the next alternative too', 'active',
    `${A}k[pattern:(?i)^yes$|^no$] NO\n`),
  both('B-083.2', 'ts', '[pattern] (?i) matches the whole simple case-folding orbit', 'active',
    `${A}s[pattern:(?i)^s+$] ${cp(0x17f)}\nm[pattern:(?i)^${cp(0xb5)}$] ${cp(0x39c)}\nd[pattern:(?i)^${cp(0x1c5)}$] ${cp(0x1c6)}\no[pattern:(?i)^${cp(0x3a9)}$] ${cp(0x2126)}\n`),
  both('B-083.3', 'ts', '[pattern] (?i) applies to \\p{…} and other class escapes', 'active',
    `${A}k[pattern:(?i)^${B}p{Lu}+$] abc\n`),
  both('B-083.4', 'ts', '[pattern] (?-u) makes \\w and \\b ASCII', 'active',
    `${A}w[pattern:(?-u)^${B}w+$] ${cp(0xe9)}t${cp(0xe9)}\nb[pattern:(?-u)${B}bx] ${cp(0xe9)}x\nk[pattern:(?i-u)^k$] ${cp(0x212a)}\n`),
  both('B-083.5', 'ts', '[pattern] the crate checks what TypeScript skips as unknown syntax', 'active',
    `${A}a[pattern:^[[:digit:]]+$] abc\nb[pattern:(?x)^ ${B}d+ $] abc\nc[pattern:^${B}d{3 }$] abc\nd[pattern:^${B}b{start}x] ax\n` +
    `e[pattern:^[a-z&&[^aeiou]]+$] aei\nf[pattern:^${B}p{greek}+$] abc\ng[pattern:^${B}pl+$] a1\nh[pattern:^a${B} b$] ab\n` +
    `i[pattern:^${B}b?a$] ba\nj[pattern:(?R)^a$] b\n`),
  both('B-083.6', 'ts', '[pattern] TypeScript checks what the crate refuses to compile', 'active',
    `${A}k[pattern:(?)^${B}d+$] abc\nr[pattern:^[${B}r--]$] -\n`),

  // A 253-byte pattern compiles to 36 000 instructions; every position of the
  // value starts a thread that lives 36 000 steps.
  {
    id: 'B-084', zone: 'ts', title: '[pattern] of 36 × .{1000} against a 64 000-character value: under 3 s',
    once: true, timeout: 120000, op: 'active', req: { brief: true },
    input: () => `${A}k[pattern:${'.{1000}'.repeat(36)}${B}d] ${'a'.repeat(64000)}\n`,
    expect: all(survive, within(3000)),
  },

  // The budget caps memory, but every copy still measures the whole value it
  // would copy — after the budget is spent too: time stays quadratic.
  cost('B-030.6', 'core', 'active: :alias of one big group', 96000, 3000, 400, { impls: ['rust'] }),
  cost('B-030.7', 'ts', 'active: :alias of one big group', 12000, 3000, 400, { impls: ['ts'] }),
  cost('B-030.8', 'ts', 'active: :ref of one big group', 12000, 3000, 400, { impls: ['ts'] }),
  cost('B-031.4', 'ts', 'active: :inherit, many children of one wide parent', 16000, 3000, 400, { impls: ['ts'] }),

  {
    id: 'B-065.2', zone: 'both', title: ':once across Rust and TypeScript processes in one directory, one shared key',
    run: onceAcrossImplementations,
  },

  // A .synxb root that is a string: decompile writes it as the whole document.
  ...[
    ['B-086.1', 'a static container whose root is a string does not decompile to an active document', rootOnly(['!active\nleak:env SYNX_ADVERSARIAL_CANARY\n!include ../../secret.synx s'], T.str, [...varint(0)])],
    ['B-086.2', 'a container whose root is a scalar decompiles to text that reads as that root or is refused', rootOnly([], T.t)],
    ['B-086.3', 'a container whose root is a list decompiles to text that reads as that list or is refused', rootOnly(['a', 'b'], T.arr, [...varint(2)], T.str, [...varint(0)], T.str, [...varint(1)])],
  ].map(([id, title, data]) => ({
    id, zone: 'both', title, op: 'decompile_full', input: data,
    expect: (view) => {
      for (const [name, r] of Object.entries(view.answers)) {
        if (!r || r.crash || r.timeout || r.thrown || r.panic !== undefined) return `${name}: ${JSON.stringify(r).slice(0, 160)}`;
        if (!r.ok) continue; // a refusal is a fine answer
        if (r.out.text_active) return `${name}: decompiled to an active document: ${JSON.stringify(r.out.text).slice(0, 120)}`;
        if (r.out.text_includes) return `${name}: decompiled text has an !include`;
        if (r.out.tree !== undefined && r.out.tree !== r.out.text_tree) return `${name}: container ${r.out.tree} reads back as ${r.out.text_tree}`;
      }
      return null;
    },
  })),

  // Inside the decoder budget (the next element is refused by both), but a
  // 4.6 KB container of one-item lists costs TypeScript ~670 MiB.
  {
    id: 'B-038.5', zone: 'ts', title: 'decompile: 1 572 862 one-item lists (4.6 KB, just under the budget) stays under 256 MiB',
    once: true, timeout: 120000, op: 'decompile', req: { brief: true },
    input: () => container(Buffer.concat([stringTable(['k']), Buffer.from([T.obj, 1, 0, T.arr]), varint(1572862), Buffer.alloc(1572862 * 3).fill(Buffer.from([T.arr, 1, T.nil]))])),
    expect: (view) => {
      for (const [name, r] of Object.entries(view.answers)) {
        if (!r || r.crash || r.timeout) return `${name}: ${JSON.stringify(r).slice(0, 120)}`;
        if (r.peak_mb > 256) return `${name}: peak ${r.peak_mb} MiB, limit 256 MiB`;
      }
      return null;
    },
  },

  // `.synx.lock` in the base directory is a link (a cloned repository can ship
  // one) to a file outside it: :once must not append there.
  {
    id: 'B-089', zone: 'both', title: ':once does not write through a .synx.lock that links outside the base directory',
    run: async (impls) => {
      const { spawnSync } = await import('node:child_process');
      for (const kind of ['symlink', 'hardlink']) {
        for (const n of impls) {
          const top = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-locklink-'));
          try {
            const base = path.join(top, 'base');
            const outside = path.join(top, 'outside');
            fs.mkdirSync(base);
            fs.mkdirSync(outside);
            const target = path.join(outside, 'victim.txt');
            fs.writeFileSync(target, 'line one\n');
            const made = spawnSync('cmd', ['/c', 'mklink', ...(kind === 'hardlink' ? ['/H'] : []), path.join(base, '.synx.lock'), target]);
            if (made.status !== 0) continue; // no right to make the link here
            const w = worker(n);
            try { await w.call({ op: 'active', text: `${A}x;touch\${IFS}pwned;#:once\n`, base }, 20000); } finally { w.close(); }
            const after = fs.readFileSync(target, 'utf-8');
            if (after !== 'line one\n') return `${n}, ${kind}: the file outside now reads ${JSON.stringify(after)}`;
          } finally {
            fs.rmSync(top, { recursive: true, force: true });
          }
        }
      }
      return null;
    },
  },

  // `!use` reads <base>/synx_packages/@x/y/src/main.synx; every step of that
  // path may be a link a cloned repository ships.
  {
    id: 'B-090', zone: 'core', title: '!use does not read a package through a link that leaves the base directory', impls: ['rust'],
    run: async () => {
      const { spawnSync } = await import('node:child_process');
      const link = (l, t, dir = true) => spawnSync('cmd', ['/c', 'mklink', ...(dir ? ['/J'] : []), l, t]).status === 0;
      for (const where of ['synx_packages', 'package dir', 'src dir', 'main.synx file']) {
        const top = fs.mkdtempSync(path.join(os.tmpdir(), 'synx-uselink-'));
        try {
          const base = path.join(top, 'base');
          const outside = path.join(top, 'outside');
          const pkg = path.join(base, 'synx_packages', '@x', 'y');
          fs.mkdirSync(path.join(outside, '@x', 'y', 'src'), { recursive: true });
          fs.writeFileSync(path.join(outside, '@x', 'y', 'src', 'main.synx'), 'token outside-value\n');
          fs.mkdirSync(base);
          let made;
          if (where === 'synx_packages') made = link(path.join(base, 'synx_packages'), outside);
          else if (where === 'package dir') { fs.mkdirSync(path.dirname(pkg), { recursive: true }); made = link(pkg, path.join(outside, '@x', 'y')); }
          else if (where === 'src dir') { fs.mkdirSync(pkg, { recursive: true }); made = link(path.join(pkg, 'src'), path.join(outside, '@x', 'y', 'src')); }
          else { fs.mkdirSync(path.join(pkg, 'src'), { recursive: true }); made = link(path.join(pkg, 'src', 'main.synx'), path.join(outside, '@x', 'y', 'src', 'main.synx'), false); }
          if (!made) continue;
          const w = worker('rust');
          let r;
          try { r = await w.call({ op: 'active', text: `${A}!use @x/y as p\nv:alias p.token\n`, base }, 20000); } finally { w.close(); }
          if (JSON.stringify(r).includes('outside-value')) return `rust, ${where} is a link: ${r.out && r.out.json}`;
        } finally {
          fs.rmSync(top, { recursive: true, force: true });
        }
      }
      return null;
    },
  },

  // A group under :secret is masked leaf by leaf; :inherit copied the leaves as they were.
  { id: 'B-017.6', zone: 'both', title: ':inherit from a :secret group copies the leaves unmasked', op: 'active',
    input: `${A}child:inherit base\nbase:secret\n  p hunter2\n`, expect: all(survive, lacks('hunter2')) },
  { id: 'B-017.7', zone: 'both', title: ':inherit from a :secret group declared before the child, read through :alias', op: 'active',
    input: `${A}creds:secret\n  pass hunter2\nc:inherit creds\nleak:alias c.pass\n`, expect: all(survive, lacks('hunter2')) },

  { id: 'B-017.8', zone: 'ts', title: ':secret:include / :include:secret on a list or a number is still masked', op: 'active',
    input: `${A}k:secret:include\n  - hunter2\nn:include:secret 7731\nm:secret:import\n  - hunter2\n`, expect: all(survive, lacks('hunter2'), lacks('7731')) },

  // Lines under a key whose marker wants a list (:join, :random, :unique, :geo)
  // that are not `- ` items: they became keys of the parent.
  both('B-087.1', 'both', 'a group under a :join key stays under it, not in the parent', 'parse', 'k:join\n  a 1\nz 3\n',
    { expect: all(survive, (v) => (Object.values(v.shown).some((s) => /"a":1,"k"/.test(s)) ? `the body left its key: ${Object.values(v.shown)[0]}` : null)) }),
  both('B-087.2', 'both', 'a group under a :join key does not overwrite a key of the parent', 'active', `${A}port 8080\nhosts:join:,\n  port 1\n`,
    { expect: all(survive, (v) => (Object.values(v.shown).some((s) => s.includes('"port":1')) ? `the parent's port was overwritten: ${Object.values(v.shown)[0]}` : null)) }),
  { id: 'B-087.3', zone: 'both', title: 'a group under a :secret:join key is not published unmasked in the parent', op: 'active',
    input: `${A}db\n  creds:secret:join\n    user admin\n    password hunter2\n`, expect: all(survive, lacks('hunter2')) },

  // :spam names its target in the error it leaves; under :secret the target is the secret.
  // The limit is per process, so the second resolve of the same document trips it.
  {
    id: 'B-088', zone: 'both', title: 'SPAM_ERR does not quote a value marked :secret (same document resolved three times)',
    run: async (impls) => {
      for (const n of impls) {
        const w = worker(n);
        try {
          for (let i = 0; i < 3; i++) {
            const r = await w.call({ op: 'active', text: `${A}k:spam:1:60:secret hunter2\n` }, 20000);
            if (JSON.stringify(r).includes('hunter2')) return `${n}, resolve ${i + 1}: ${r.ok ? r.out.json : r.err}`;
          }
        } finally { w.close(); }
      }
      return null;
    },
  },

  // JSON → SYNX → JSON: the same keys and values (law of B-021.4).
  ...[
    ['B-021.5', 'a key the reader refuses (constructor, prototype, __proto__) is renamed, not dropped', '{"constructor":1,"prototype":2,"a":{"__proto__":3}}'],
    ['B-021.6', 'a list item with " # " or " // " keeps its text', '{"l":["a # b","c // d"]}'],
    ['B-085.1', 'a multi-line string keeps a leading and a trailing line break', '{"a":"\\nb","c":"d\\n","e":"\\n"}'],
    ['B-085.2', 'a multi-line string keeps the indentation of its first line', '{"k":"  a\\n b"}'],
    ['B-085.3', 'a multi-line string keeps CR LF', '{"k":"a\\r\\nb"}'],
    ['B-085.4', 'a multi-line string keeps a blank line and a trailing space', '{"k":"a\\n\\nb","m":"a \\nb"}'],
  ].map(([id, title, json]) => ({
    id, zone: 'both', title, op: 'json2synx', input: json,
    expect: all(survive, field('p1', (v, o) => {
      const shape = (t) => (Array.isArray(t) ? '[' + t.map(shape).join(',') + ']'
        : t && typeof t === 'object' ? '{' + Object.values(t).map(shape).sort().join(',') + '}' : JSON.stringify(t));
      return shape(JSON.parse(v)) === shape(JSON.parse(o.p0));
    }, 'as many keys, with the same values'), field('active1', (v) => v === false, 'false')),
  })),
];
