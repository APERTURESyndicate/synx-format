/**
 * SYNXL: both readers must report the same records, the same diagnostics and
 * the same hard error (condition and line). Found by `fuzz.mjs --checks synxl`.
 */
import { normJson } from '../lib/harness.mjs';
import { survive } from '../lib/expect.mjs';

const P = '!synxl 1\n';

/** Records, diagnostics and hard error of every implementation are identical. */
const sameReading = (view) => {
  const failed = survive(view);
  if (failed) return failed;
  const seen = Object.entries(view.answers).map(([name, r]) => {
    const o = r.out || {};
    return [name, r.ok ? (o.hard ? `hard ${o.hard} at line ${o.line}` : `${normJson(o.json)} diagnostics ${JSON.stringify(o.diagnostics)}`) : `ERR ${r.err}`];
  });
  const other = seen.find(([, v]) => v !== seen[0][1]);
  return other ? `${seen[0][0]}: ${seen[0][1].slice(0, 200)}\n      ${other[0]}: ${other[1].slice(0, 200)}` : null;
};

const synxl = (id, zone, title, input) => ({ id, zone, title, op: 'synxl', input, req: { validate: true }, expect: sameReading });

export default [
  synxl('B-060.1', 'both', 'missing prologue after a blank line: same line number', '\n#'),
  synxl('B-060.2', 'both', 'missing prologue after an open block comment: same line number', '###\n1'),
  synxl('B-061.1', 'ts', '[max] applies to an integer beyond 2^53', `${P}!fields e[max:2]\n9007199254740993\n`),
  synxl('B-061.2', 'ts', '[min] applies to an integer below -2^53', `${P}!fields o[min:1]\n-9372036854775808\n`),
  synxl('B-061.3', 'ts', '[type:int] accepts an integer beyond 2^53', `${P}!fields v[type:int]\n9223372036854775807\n`),
  synxl('B-062', 'both', 'an indented ### closes a block comment in both readers', `${P}###\n ###\n!fields a\nx\n`),
  synxl('B-063.1', 'both', 'empty () hint with [block]: one hard error', `${P}!fields x()[block]\n`),
  synxl('B-063.2', 'both', 'empty () hint with type:float', `${P}!fields a()[type:float]\nabc\n`),
  synxl('B-063.3', 'both', 'empty () hint with type:string', `${P}!fields a()[type:string]\n0\n`),
  synxl('B-063.4', 'both', '(float) hint with type:int', `${P}!fields a(float)[type:int]\n3\n`),
  synxl('B-064', 'both', '!fields with a lone colon: one hard error', `${P}!fields :\n`),
  synxl('B-064.2', 'both', '!fields with a name, a space and a lone colon: one hard error', `${P}!fields a :\n`),
];
