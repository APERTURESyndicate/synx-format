#!/usr/bin/env node
/**
 * Backtracking cost of the highlighting grammars: every regular expression of
 * a TextMate grammar is run over lines built to make a backtracking engine
 * work (long runs of spaces, of one letter, of brackets, of colons…), at two
 * lengths. An expression whose time more than triples when the line doubles
 * is reported with the curve.
 *
 *   node tests/adversarial/grammar.mjs                       the VS Code grammar of this repository
 *   node tests/adversarial/grammar.mjs path/to/x.tmLanguage.json --n 2000
 *
 * Measured in V8, which backtracks like Oniguruma does; an expression V8
 * cannot compile (Oniguruma-only syntax) is listed and skipped.
 *
 * Exit code 1 when an expression is super-linear.
 */
import fs from 'node:fs';
import path from 'node:path';
import { REPO } from './lib/harness.mjs';

const argv = process.argv.slice(2);
const file = argv.find((a) => !a.startsWith('--') && a !== argv[argv.indexOf('--n') + 1]) || path.join(REPO, 'integrations', 'vscode', 'synx-vscode', 'syntaxes', 'synx.tmLanguage.json');
const N = Number(argv.includes('--n') ? argv[argv.indexOf('--n') + 1] : 1500);
const grammar = JSON.parse(fs.readFileSync(file, 'utf-8'));

const found = [];
(function walk(node, where) {
  if (!node || typeof node !== 'object') return;
  if (Array.isArray(node)) { node.forEach((n, i) => walk(n, `${where}[${i}]`)); return; }
  for (const key of ['match', 'begin', 'end', 'while']) if (typeof node[key] === 'string') found.push([`${where}.${key}`, node[key]]);
  for (const [k, v] of Object.entries(node)) if (typeof v === 'object') walk(v, where ? `${where}.${k}` : k);
})(grammar, '');

const lines = (n) => ({
  'x, spaces, y': 'x' + ' '.repeat(n) + 'y',
  'spaces': ' '.repeat(n),
  'one letter': 'a'.repeat(n),
  'letter-space pairs': 'a '.repeat(n),
  'colons': 'k' + ':'.repeat(n),
  'markers': 'k' + ':a'.repeat(n),
  'open brackets': 'k' + '['.repeat(n),
  'open parens': 'k' + '('.repeat(n),
  'quotes': 'k ' + '"'.repeat(n),
  'braces': 'k ' + '{a}'.repeat(n),
  'dashes': '- '.repeat(n),
  'tabs then x': '\t'.repeat(n) + 'x',
  'key then spaces': 'key' + ' '.repeat(n),
  'digits': 'k ' + '1'.repeat(n),
  'dots': 'k ' + '1.'.repeat(n),
});

const time = (re, text) => {
  const t = process.hrtime.bigint();
  re.lastIndex = 0;
  for (let from = 0, guard = 0; guard < 50; guard++) {
    re.lastIndex = from;
    const m = re.exec(text);
    if (!m) break;
    from = m.index + Math.max(1, m[0].length);
    if (from >= text.length) break;
  }
  return Number(process.hrtime.bigint() - t) / 1e6;
};

let bad = 0;
const skipped = [];
for (const [where, source] of found) {
  let re;
  try { re = new RegExp(source, 'g'); } catch { skipped.push(where); continue; }
  for (const [shape, text] of Object.entries(lines(N))) {
    const a = time(re, text);
    if (a < 20) continue;
    const b = time(re, lines(N * 2)[shape]);
    const ratio = b / a;
    if (ratio > 3) {
      bad++;
      console.log(`!! ${where}\n     /${source}/\n     line of ${shape}: ${N} → ${a.toFixed(0)} ms, ${N * 2} → ${b.toFixed(0)} ms (×${ratio.toFixed(1)})`);
    }
  }
}
console.log(`\n${found.length} expressions in ${path.basename(file)}, ${bad} super-linear${skipped.length ? `, ${skipped.length} not compilable in V8: ${skipped.slice(0, 5).join(', ')}` : ''}`);
process.exit(bad ? 1 : 0);
