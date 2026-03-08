import Synx from '../src/index';

// ─── Static mode tests ───────────────────────────────────

describe('SYNX Parser — Static Mode', () => {
  test('parses simple key-value pairs', () => {
    const data = Synx.parse(`
      name Wario
      age 30
      active true
      score 99.5
      empty null
    `);
    expect(data.name).toBe('Wario');
    expect(data.age).toBe(30);
    expect(data.active).toBe(true);
    expect(data.score).toBe(99.5);
    expect(data.empty).toBe(null);
  });

  test('parses nested objects', () => {
    const data = Synx.parse(`
server
  host 0.0.0.0
  port 8080
  ssl
    enabled true
    `);
    expect(data.server).toBeDefined();
    expect((data.server as any).host).toBe('0.0.0.0');
    expect((data.server as any).port).toBe(8080);
    expect((data.server as any).ssl.enabled).toBe(true);
  });

  test('parses lists', () => {
    const data = Synx.parse(`
inventory
  - Sword
  - Shield
  - Potion
    `);
    expect(data.inventory).toEqual(['Sword', 'Shield', 'Potion']);
  });

  test('parses multiline blocks', () => {
    const data = Synx.parse(`
rules |
  Rule one.
  Rule two.
  Rule three.
    `);
    expect(data.rules).toBe('Rule one.\nRule two.\nRule three.');
  });

  test('ignores # comments', () => {
    const data = Synx.parse(`
# This is a comment
name Wario # inline comment
    `);
    expect(data.name).toBe('Wario');
  });

  test('ignores // comments', () => {
    const data = Synx.parse(`
// This is a comment
name Wario // inline comment
    `);
    expect(data.name).toBe('Wario');
  });

  test('markers are ignored without !active', () => {
    const data = Synx.parse(`
price 100
total:calc price * 2
    `);
    // Without !active, ":calc" is part of the key, or treated as plain text
    // The parser should still parse it but engine won't resolve
    expect(data['total']).toBe('price * 2');
  });
});

// ─── Active mode tests ───────────────────────────────────

describe('SYNX Engine — Active Mode', () => {
  test(':calc computes arithmetic', () => {
    const data = Synx.parse(`
!active
price 100
tax:calc price * 0.2
total:calc price + 20
    `);
    expect(data.tax).toBe(20);
    expect(data.total).toBe(120);
  });

  test(':random picks one item', () => {
    const data = Synx.parse(`
!active
pick:random
  - Alpha
  - Beta
  - Gamma
    `);
    expect(['Alpha', 'Beta', 'Gamma']).toContain(data.pick);
  });

  test(':random with weights works', () => {
    // Run many times to test distribution isn't broken
    for (let i = 0; i < 20; i++) {
      const data = Synx.parse(`
!active
tier:random 90 5 5
  - common
  - rare
  - legendary
      `);
      expect(['common', 'rare', 'legendary']).toContain(data.tier);
    }
  });

  test(':env reads environment variable', () => {
    const data = Synx.parse(`
!active
home:env TEST_SYNX_VAR
    `, { env: { TEST_SYNX_VAR: 'hello_synx' } });
    expect(data.home).toBe('hello_synx');
  });

  test(':env with :default falls back', () => {
    const data = Synx.parse(`
!active
port:env:default:3000 NONEXISTENT_PORT
    `, { env: {} });
    expect(data.port).toBe(3000);
  });

  test(':alias references another key', () => {
    const data = Synx.parse(`
!active
admin admin@test.com
contact:alias admin
    `);
    expect(data.contact).toBe('admin@test.com');
  });

  test(':unique deduplicates list', () => {
    const data = Synx.parse(`
!active
tags:unique
  - action
  - rpg
  - action
  - sim
  - rpg
    `);
    expect(data.tags).toEqual(['action', 'rpg', 'sim']);
  });

  test(':secret hides value in toString/JSON', () => {
    const data = Synx.parse(`
!active
key:secret my_api_key_123
    `);
    expect(String(data.key)).toBe('[SECRET]');
    expect(JSON.stringify(data.key)).toBe('"[SECRET]"');
    // But valueOf reveals it for code usage
    expect((data.key as any).reveal()).toBe('my_api_key_123');
  });

  test(':template substitutes {placeholders} (legacy marker)', () => {
    const data = Synx.parse(`
!active
name Wario
age 30
greeting:template Hello, {name}! You are {age} years old.
    `);
    expect(data.greeting).toBe('Hello, Wario! You are 30 years old.');
  });

  test(':template with nested references (legacy marker)', () => {
    const data = Synx.parse(`
!active
server
  host localhost
  port 8080
url:template http://{server.host}:{server.port}/api
    `);
    expect(data.url).toBe('http://localhost:8080/api');
  });

  test('auto-{} interpolation without :template marker', () => {
    const data = Synx.parse(`
!active
name Wario
age 30
greeting Hello, {name}! You are {age} years old.
    `);
    expect(data.greeting).toBe('Hello, Wario! You are 30 years old.');
  });

  test('auto-{} with nested dot-path references', () => {
    const data = Synx.parse(`
!active
server
  host localhost
  port 8080
url http://{server.host}:{server.port}/api
    `);
    expect(data.url).toBe('http://localhost:8080/api');
  });

  test(':split splits string by delimiter', () => {
    const data = Synx.parse(`
!active
raw:split red, green, blue
    `);
    expect(data.raw).toEqual(['red', 'green', 'blue']);
  });

  test(':split with space delimiter', () => {
    const data = Synx.parse(`
!active
words:split:space hello world foo
    `);
    expect(data.words).toEqual(['hello', 'world', 'foo']);
  });

  test(':split auto-casts numeric values', () => {
    const data = Synx.parse(`
!active
nums:split 1, 2, 3
    `);
    expect(data.nums).toEqual([1, 2, 3]);
  });

  test(':join joins array into string', () => {
    const data = Synx.parse(`
!active
tags:join
  - red
  - green
  - blue
    `);
    expect(data.tags).toBe('red,green,blue');
  });

  test(':join with space delimiter', () => {
    const data = Synx.parse(`
!active
words:join:space
  - hello
  - world
    `);
    expect(data.words).toBe('hello world');
  });

  test('strict=false keeps INCLUDE_ERR as value', () => {
    const data = Synx.parse(`
!active
db:include ./__missing__.synx
    `);
    expect(String(data.db)).toContain('INCLUDE_ERR:');
  });

  test('strict=true throws on INCLUDE_ERR', () => {
    expect(() => {
      Synx.parse(`
!active
db:include ./__missing__.synx
      `, { strict: true });
    }).toThrow(/SYNX strict mode error/);
  });

  test(':ref resolves reference', () => {
    const data = Synx.parse(`
!active
base_rate 50
quick_rate:ref base_rate
    `);
    expect(data.quick_rate).toBe(50);
  });

  test(':ref:calc with shorthand expression', () => {
    const data = Synx.parse(`
!active
base_rate 50
double_rate:ref:calc:*2 base_rate
    `);
    expect(data.double_rate).toBe(100);
  });

  test(':inherit merges parent fields', () => {
    const data = Synx.parse(`
!active
_base_resource
  weight 10
  stackable true
  category misc
steel:inherit:_base_resource
  weight 25
  material metal
    `);
    expect(data.steel).toBeDefined();
    expect((data.steel as any).weight).toBe(25);
    expect((data.steel as any).stackable).toBe(true);
    expect((data.steel as any).category).toBe('misc');
    expect((data.steel as any).material).toBe('metal');
    // Private blocks excluded from output
    expect(data._base_resource).toBeUndefined();
  });

  test(':i18n selects language', () => {
    const data = Synx.parse(`
!active
title:i18n
  en Hello
  ru Привет
  de Hallo
    `, { lang: 'ru' });
    expect(data.title).toBe('Привет');
  });

  test(':i18n falls back to en', () => {
    const data = Synx.parse(`
!active
title:i18n
  en Hello
  ru Привет
    `, { lang: 'fr' });
    expect(data.title).toBe('Hello');
  });
});

// ─── Export format tests ─────────────────────────────────

describe('SYNX Export Formats', () => {
  const obj = Synx.parse(`
name Wario
age 30
server
  host localhost
  port 8080
tags
  - red
  - green
  - blue
  `);

  test('toJSON produces valid JSON', () => {
    const json = Synx.toJSON(obj);
    const parsed = JSON.parse(json);
    expect(parsed.name).toBe('Wario');
    expect(parsed.server.port).toBe(8080);
  });

  test('toYAML produces YAML', () => {
    const yaml = Synx.toYAML(obj);
    expect(yaml).toContain('name: Wario');
    expect(yaml).toContain('port: 8080');
    expect(yaml).toContain('- red');
  });

  test('toTOML produces TOML', () => {
    const toml = Synx.toTOML(obj);
    expect(toml).toContain('name = "Wario"');
    expect(toml).toContain('[server]');
    expect(toml).toContain('port = 8080');
  });

  test('toEnv produces KEY=VALUE', () => {
    const env = Synx.toEnv(obj);
    expect(env).toContain('NAME=Wario');
    expect(env).toContain('AGE=30');
    expect(env).toContain('SERVER_HOST=localhost');
    expect(env).toContain('SERVER_PORT=8080');
  });

  test('toEnv with prefix', () => {
    const env = Synx.toEnv(obj, 'APP');
    expect(env).toContain('APP_NAME=Wario');
    expect(env).toContain('APP_SERVER_PORT=8080');
  });
});

// ─── Schema tests ────────────────────────────────────────

describe('SYNX Schema Export', () => {
  test('extracts constraints as JSON Schema', () => {
    const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
theme[enum:light|dark|auto] dark
    `);
    expect(schema.$schema).toContain('json-schema.org');
    expect(schema.type).toBe('object');
    expect(schema.required).toContain('app_name');
    expect(schema.properties.app_name?.minimum).toBe(3);
    expect(schema.properties.volume?.type).toBe('integer');
    expect(schema.properties.theme?.enum).toEqual(['light', 'dark', 'auto']);
  });
});

// ─── Safe Calc tests ─────────────────────────────────────

describe('Safe Calculator', () => {
  test('basic arithmetic', () => {
    const data = Synx.parse('!active\nresult:calc 2 + 3 * 4');
    expect(data.result).toBe(14); // 2 + 12, not (2+3)*4
  });

  test('parentheses', () => {
    const data = Synx.parse('!active\nresult:calc (2 + 3) * 4');
    expect(data.result).toBe(20);
  });

  test('modulo', () => {
    const data = Synx.parse('!active\nresult:calc 10 % 3');
    expect(data.result).toBe(1);
  });

  test('negative numbers', () => {
    const data = Synx.parse('!active\nresult:calc -5 + 3');
    expect(data.result).toBe(-2);
  });
});
