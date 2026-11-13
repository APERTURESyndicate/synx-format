#!/usr/bin/env node
/**
 * Ask the implementations one question and print what each one says.
 *
 *   node tests/adversarial/probe.mjs <op> <file>            [options]
 *   node tests/adversarial/probe.mjs <op> --text 'a 1\nb 2' [options]
 *
 * Operations: parse, active, tool, format, format_check, stringify,
 * stringify_check, json2synx, compile, decompile, decompile_tree,
 * compile_check, synxl, synxl_check, calc, diff, schema, sinks, sig, pubkey.
 *
 * Options:
 *   --impl a,b       implementations (default: rust,ts); see lib/impls.mjs
 *   --text S         input as text; \n \t \r \\ \0 \uXXXX \xHH escapes are read
 *   --hex H          input as hex bytes
 *   --second FILE    second input (diff, sig)
 *   --env K=V        an environment variable for `:env` (repeatable); the
 *                    host environment is never passed
 *   --base DIR       base directory for file markers
 *   --lang L, --region R, --depth N, --needle S, --resolved, --validate, --strict
 *   --timeout MS     per request (default 20000)
 *   --once           a fresh process per request, with peak memory
 *   --raw            print whole answers as JSON
 *
 * Exit code 1 when an implementation crashes, hangs, panics or throws an
 * untyped exception, or when the answers differ.
 */
import fs from 'node:fs';
import { b64, normJson, verdict, isFailure } from './lib/harness.mjs';
import { worker, runOnce } from './lib/impls.mjs';

const argv = process.argv.slice(2);
if (argv.length < 2 || argv.includes('--help')) {
  process.stdout.write(fs.readFileSync(new URL(import.meta.url), 'utf-8').split('*/')[0].replace(/^#!.*\n\/\*\*\n?/, '').replace(/^ \* ?/gm, ''));
  process.exit(argv.includes('--help') ? 0 : 2);
}

function unescape(s) {
  return s.replace(/\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|.)/g, (_, e) => {
    if (e[0] === 'u' && e[1] === '{') return String.fromCodePoint(parseInt(e.slice(2, -1), 16));
    if (e[0] === 'u') return String.fromCharCode(parseInt(e.slice(1), 16));
    if (e[0] === 'x') return String.fromCharCode(parseInt(e.slice(1), 16));
    return { n: '\n', t: '\t', r: '\r', 0: '\0' }[e] ?? e;
  });
}

const op = argv[0];
const req = { op };
let impls = ['rust', 'ts'];
let timeout = 20000;
let useOnce = false;
let raw = false;
let field = null;
for (let i = 1; i < argv.length; i++) {
  const a = argv[i];
  const next = () => argv[++i];
  if (a === '--impl') impls = next().split(',');
  else if (a === '--text') req.b64 = b64(unescape(next()));
  else if (a === '--hex') req.b64 = Buffer.from(next().replace(/\s+/g, ''), 'hex').toString('base64');
  else if (a === '--second') req.b64b = fs.readFileSync(next()).toString('base64');
  else if (a === '--second-text') req.b64b = b64(unescape(next()));
  else if (a === '--env') { const [k, ...v] = next().split('='); (req.env ??= {})[k] = v.join('='); }
  else if (a === '--base') req.base = next();
  else if (a === '--lang') req.lang = next();
  else if (a === '--region') req.region = next();
  else if (a === '--depth') req.depth = Number(next());
  else if (a === '--needle') req.needle = next();
  else if (a === '--resolved') req.resolved = true;
  else if (a === '--validate') req.validate = true;
  else if (a === '--strict') req.strict = true;
  else if (a === '--timeout') timeout = Number(next());
  else if (a === '--once') useOnce = true;
  else if (a === '--raw') raw = true;
  else if (a === '--field') field = next();
  else if (!a.startsWith('--') && req.b64 === undefined) req.b64 = fs.readFileSync(a).toString('base64');
  else { console.error(`unknown argument ${a}`); process.exit(2); }
}

const FIELD = field || { format: 'text', stringify: 'text', decompile: 'text', compile: 'b64', calc: 'num' }[op] || 'json';
const answers = [];
for (const name of impls) {
  let resp;
  if (useOnce) resp = runOnce(name, req, { timeoutMs: timeout });
  else {
    const w = worker(name);
    resp = await w.call(req, timeout);
    w.close();
  }
  answers.push([name, resp]);
}

let failed = false;
const shown = [];
for (const [name, resp] of answers) {
  if (isFailure(resp)) failed = true;
  const v = verdict(resp, FIELD);
  shown.push(typeof v === 'string' ? normJson(v) : JSON.stringify(v));
  const cost = resp && resp.ms !== undefined ? `  [${resp.ms} ms${useOnce ? `, peak ${resp.peak_mb} MiB` : ''}]` : '';
  const text = raw ? JSON.stringify(resp, null, 1) : typeof v === 'string' ? v : JSON.stringify(v);
  console.log(`${name.padEnd(13)}${cost}\n  ${text.length > 4000 ? text.slice(0, 4000) + `… (${text.length} chars)` : text}`);
}
const differ = new Set(shown).size > 1;
if (answers.length > 1) console.log(differ ? 'DIFFERENT' : 'same');
process.exit(failed || differ ? 1 : 0);
