/**
 * SYNX Benchmark — Node.js
 * Compares: JSON.parse · js-yaml · synx-js (pure TS) · synx-native (Rust/napi)
 *
 * Usage:
 *   cd benchmarks
 *   npm install
 *   node bench_node.js
 */

'use strict';

const fs   = require('fs');
const path = require('path');

// ── Parsers ─────────────────────────────────────────────────────────────────

const yaml = require('js-yaml');
const { XMLParser } = require('fast-xml-parser');

// Pure-JS TypeScript-compiled SYNX parser (packages/synx-js)
const SynxJS = require(path.resolve(__dirname, '..', 'packages', 'synx-js'));

// Native Rust SYNX parser (bindings/node → synx_core via napi-rs)
const SynxNative = require(path.resolve(__dirname, '..', 'bindings', 'node'));

// ── Input data ───────────────────────────────────────────────────────────────

const jsonText = fs.readFileSync(path.join(__dirname, 'config.json'),  'utf-8');
const yamlText = fs.readFileSync(path.join(__dirname, 'config.yaml'),  'utf-8');
const synxText = fs.readFileSync(path.join(__dirname, 'config.synx'), 'utf-8');

/** JSON → generic nested XML (same data as config.json) for Claude-style prompt comparisons */
function sanitizeXmlTag(name) {
  return String(name).replace(/[^a-zA-Z0-9_.-]/g, '_') || 'node';
}
function xmlEscape(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
function jsonToXml(value, tagName) {
  const tag = sanitizeXmlTag(tagName);
  if (value === null || value === undefined) return `<${tag}/>`;
  if (typeof value !== 'object') {
    return `<${tag}>${xmlEscape(String(value))}</${tag}>`;
  }
  if (Array.isArray(value)) {
    const inner = value.map((x) => jsonToXml(x, 'item')).join('');
    return `<${tag}>${inner}</${tag}>`;
  }
  const inner = Object.entries(value)
    .map(([k, v]) => jsonToXml(v, k))
    .join('');
  return `<${tag}>${inner}</${tag}>`;
}
const xmlText =
  '<?xml version="1.0" encoding="UTF-8"?>\n' +
  jsonToXml(JSON.parse(jsonText), 'config');
const xmlParser = new XMLParser({
  ignoreAttributes: false,
  parseTagValue: true,
});

const ITERATIONS = 50_000;
const WARMUP     = 500;

// ── Helpers ──────────────────────────────────────────────────────────────────

function bench(label, fn) {
    if (global.gc) global.gc();

    for (let i = 0; i < WARMUP; i++) fn();

    const start = process.hrtime.bigint();
    for (let i = 0; i < ITERATIONS; i++) fn();
    const end = process.hrtime.bigint();

    const totalNs  = Number(end - start);
    const totalMs  = totalNs / 1e6;
    const perIter  = totalNs / ITERATIONS;        // ns
    const perIterUs = perIter / 1e3;              // µs

    return { label, totalMs, perIterUs, perIter };
}

function pad(str, n, right = false) {
    str = String(str);
    return right ? str.padStart(n) : str.padEnd(n);
}

// ── Run ──────────────────────────────────────────────────────────────────────

console.log('╔══════════════════════════════════════════════════════════════════╗');
console.log('║         SYNX Benchmark — Node.js  (50 000 iterations)           ║');
console.log('╠══════════════════════════════════════════════════════════════════╣');
console.log(`║  Input sizes:  JSON ${String(jsonText.length).padEnd(5)}b   YAML ${String(yamlText.length).padEnd(5)}b   XML ${String(xmlText.length).padEnd(5)}b   SYNX ${String(synxText.length).padEnd(5)}b   ║`);
console.log('╚══════════════════════════════════════════════════════════════════╝');
console.log();

const results = [
    bench('JSON.parse (built-in)',       () => JSON.parse(jsonText)),
    bench('YAML  (js-yaml)',             () => yaml.load(yamlText)),
    bench('XML   (fast-xml-parser)',     () => xmlParser.parse(xmlText)),
    bench('SYNX  (synx-js / pure JS)',   () => SynxJS.parse(synxText)),
    bench('SYNX  (synx-native / Rust)',  () => SynxNative.parse(synxText)),
    bench('SYNX  (native parseToJson)',  () => SynxNative.parseToJson(synxText)),
];

const fastest   = Math.min(...results.map(r => r.totalMs));
const baselineMs = results[0].totalMs;   // JSON baseline

console.log(
    pad('  Parser',                        36) +
    pad('Time/call',    12, true) +
    pad('Total (ms)',   13, true) +
    pad('vs JSON',      10, true) +
    pad('vs Rust',      10, true)
);
console.log('  ' + '─'.repeat(78));

for (const r of results) {
    const vsJson  = (r.totalMs / baselineMs).toFixed(2) + 'x';
    const nativeRow = results.find((x) => x.label.includes('synx-native / Rust'));
    const vsNative = nativeRow ? (r.totalMs / nativeRow.totalMs).toFixed(2) + 'x' : '—';
    const timeStr = r.perIterUs < 10
        ? r.perIterUs.toFixed(3) + ' µs'
        : r.perIterUs.toFixed(2) + ' µs';

    const marker = r.totalMs === fastest ? ' ←fastest' : '';
    console.log(
        pad('  ' + r.label,                 36) +
        pad(timeStr,       12, true) +
        pad(r.totalMs.toFixed(1) + ' ms', 13, true) +
        pad(vsJson,        10, true) +
        pad(vsNative,      10, true) +
        marker
    );
}

console.log();
console.log('  Note: run with --expose-gc for more stable GC-controlled results.');
console.log('  e.g.  node --expose-gc bench_node.js');
console.log();

// ── Export results as JSON (for README table generation) ─────────────────────

const outFile = path.join(__dirname, 'results_node.json');
fs.writeFileSync(outFile, JSON.stringify({
    platform: process.platform,
    arch:     process.arch,
    node:     process.version,
    iterations: ITERATIONS,
    sizes: { json: jsonText.length, yaml: yamlText.length, xml: xmlText.length, synx: synxText.length },
    results: results.map(r => ({
        label:    r.label,
        totalMs:  +r.totalMs.toFixed(3),
        perIterUs: +r.perIterUs.toFixed(4),
    })),
}, null, 2));
console.log(`  Results saved → ${outFile}`);
