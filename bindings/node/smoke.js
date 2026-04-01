'use strict';

const assert = require('assert');
const synx = require('./index.js');

const parsed = synx.parse('name John\nage 25\n');
assert.strictEqual(parsed.name, 'John');
assert.strictEqual(parsed.age, 25);

const active = synx.parseActive('!active\nname John\n', { env: {}, basePath: '.' });
assert.strictEqual(active.name, 'John');

const synxText = synx.stringify({ name: 'John', age: 25, tags: ['x', 'y'] });
assert.ok(synxText.includes('name John'));
assert.ok(synxText.includes('age 25'));

const formatted = synx.format('b 2\na 1\n');
assert.ok(formatted.includes('a 1'));
assert.ok(formatted.includes('b 2'));

console.log('Node binding smoke tests passed.');
