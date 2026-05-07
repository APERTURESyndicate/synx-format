// Lightweight smoke tests for synx-js post-fix verification.
// Run: node smoke.js
const { Synx } = require('./dist/index.js');

let pass = 0, fail = 0;
const test = (name, fn) => {
  try { fn(); console.log('OK  ' + name); pass++; }
  catch (e) { console.log('ERR ' + name + ': ' + e.message); fail++; }
};
const eq = (a, b, msg) => {
  const sa = JSON.stringify(a), sb = JSON.stringify(b);
  if (sa !== sb) throw new Error((msg || '') + ' expected ' + sb + ' got ' + sa);
};

test('basic key-value', () => {
  const r = Synx.parse("name Alice\nage 30\nactive true");
  eq(r.name, "Alice"); eq(r.age, 30); eq(r.active, true);
});
test('quoted strings unwrap', () => {
  const r = Synx.parse('a "hello world"\nb ' + "'null'" + '\nc "true"');
  eq(r.a, "hello world"); eq(r.b, "null"); eq(r.c, "true");
});
test('nested objects', () => {
  const r = Synx.parse("server\n  host 0.0.0.0\n  port 8080");
  eq(r.server.host, "0.0.0.0"); eq(r.server.port, 8080);
});
test('multiline block', () => {
  const r = Synx.parse("text |\n  line1\n  line2");
  eq(r.text, "line1\nline2");
});
test('env marker', () => {
  process.env.SYNX_TEST_VAR = "abc";
  const r = Synx.parse("!active\nx:env SYNX_TEST_VAR");
  eq(r.x, "abc");
});
test('env default casts true', () => {
  delete process.env.SYNX_NOPE;
  const r = Synx.parse("!active\nx:env:default:true SYNX_NOPE");
  eq(r.x, true);
});
test('calc dot-path', () => {
  const r = Synx.parse("!active\nbase\n  hp 100\nboss:calc base.hp * 5");
  eq(r.boss, 500);
});
test('alias', () => {
  const r = Synx.parse("!active\nadmin foo\nsupport:alias admin");
  eq(r.support, "foo");
});
test('secret toJSON', () => {
  const r = Synx.parse("!active\nkey:secret sk-1234");
  eq(JSON.parse(JSON.stringify(r)).key, "[SECRET]");
});
test('constraints pattern with [A-Z]', () => {
  const r = Synx.parse("!active\ncode[pattern:^[A-Z]{2}$] US\nbad[pattern:^[A-Z]{2}$] usa");
  eq(r.code, "US");
  if (typeof r.bad !== 'string' || !r.bad.startsWith("CONSTRAINT_ERR:")) {
    throw new Error("expected CONSTRAINT_ERR for bad, got " + r.bad);
  }
});
test('parseTool call mode', () => {
  const r = Synx.parseTool("!tool\nweb_search\n  query foo");
  eq(r.tool, "web_search");
  eq(r.params, { query: "foo" });
});
test('parseTool schema mode', () => {
  const r = Synx.parseTool("!tool\n!schema\nweb_search\n  query string\ncalc\n  expr string");
  eq(r.tools.length, 2);
  eq(r.tools[0].name, "calc");
  eq(r.tools[1].name, "web_search");
});
test('toJSON sorted keys', () => {
  const r = Synx.parse("z 3\na 1\nm 2");
  const j = Synx.toJSON(r, false);
  eq(j, '{"a":1,"m":2,"z":3}');
});
test('.synxb round-trip', () => {
  const text = "name Alice\nage 30\nactive true";
  const buf = Synx.compile(text);
  const restored = Synx.decompile(buf);
  if (!restored.includes("Alice")) throw new Error("missing Alice");
  if (!restored.includes("30")) throw new Error("missing 30");
});
test('prototype pollution blocked', () => {
  const r = Synx.parse("__proto__ injected\nfoo bar");
  if (r.__proto__ === "injected") throw new Error("prototype pollution!");
  eq(r.foo, "bar");
});
test('inherit with children', () => {
  const r = Synx.parse("!active\nbase\n  port 80\n  host base.x\nprod:inherit base\n  host prod.x");
  eq(r.prod.port, 80);
  eq(r.prod.host, "prod.x");
});

// 3.6.2 markers
test(':replace literal substring', () => {
  const r = Synx.parse("!active\nx:replace:l:L Hello there");
  eq(r.x, "HeLLo there");
});
test(':replace deletion (empty TO)', () => {
  const r = Synx.parse("!active\nx:replace:e: Hello");
  eq(r.x, "Hllo");
});
test(':sort ascending numeric', () => {
  const r = Synx.parse("!active\nxs:sort\n  - 5\n  - 1\n  - 3");
  eq(r.xs, [1, 3, 5]);
});
test(':sort:desc reverses', () => {
  const r = Synx.parse("!active\nxs:sort:desc\n  - 1\n  - 3\n  - 2");
  eq(r.xs, [3, 2, 1]);
});
test(':sort lexicographic for strings', () => {
  const r = Synx.parse("!active\nxs:sort\n  - banana\n  - apple\n  - cherry");
  eq(r.xs, ["apple", "banana", "cherry"]);
});
test(':sum integer array stays Int', () => {
  const r = Synx.parse("!active\ntotal:sum\n  - 1\n  - 2\n  - 3");
  eq(r.total, 6);
  if (!Number.isInteger(r.total)) throw new Error('expected integer');
});
test(':sum mixes float into Float', () => {
  const r = Synx.parse("!active\ntotal:sum\n  - 1.5\n  - 2.5");
  eq(r.total, 4);
});
test(':sum skips non-numeric', () => {
  const r = Synx.parse('!active\ntotal:sum\n  - 1\n  - "two"\n  - 3');
  eq(r.total, 4);
});

console.log('---');
console.log(pass + ' passed, ' + fail + ' failed');
process.exit(fail ? 1 : 0);
