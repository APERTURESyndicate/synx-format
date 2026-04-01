# SYNX for JS/TS — @aperturesyndicate/synx-format

The official JavaScript & TypeScript parser for the SYNX format.

## Install

```bash
npm install @aperturesyndicate/synx-format
```

## Usage

```javascript
const Synx = require('@aperturesyndicate/synx-format');

// Load from file
const data = Synx.loadSync('config.synx');
console.log(data.server.port); // 8080

// Parse from string
const data2 = Synx.parse('name Wario\nage 30');
console.log(data2.name); // "Wario"
```

### TypeScript

```typescript
import Synx from '@aperturesyndicate/synx-format';

interface Config {
  server: { port: number; host: string };
  app_name: string;
}

const data = Synx.loadSync<Config>('config.synx');
console.log(data.server.port); // typed as number
```

## API

| Method | Description |
|---|---|
| `Synx.parse<T>(text, options?)` | Parse a .synx string → object |
| `Synx.loadSync<T>(path, options?)` | Load & parse file (sync) |
| `Synx.load<T>(path, options?)` | Load & parse file (async, returns Promise) |
| `Synx.stringify(obj, active?)` | Serialize object → .synx string |

### Options

```typescript
{
  basePath?: string;                 // For :include resolution
  env?: Record<string, string>;      // Override env vars
  region?: string;                   // For :geo ("RU", "US", etc.)
  strict?: boolean;                  // Throw on INCLUDE_ERR/WATCH_ERR/CALC_ERR/CONSTRAINT_ERR
}
```

`strict: true` enables fail-fast behavior for production: marker runtime errors throw instead of silently remaining in the output object as error strings.

## CLI

Install globally:

```bash
npm install -g @aperturesyndicate/synx-format
```

### Commands

```bash
# Convert to JSON/YAML/TOML/.env
synx convert config.synx --format json
synx convert config.synx --format yaml > values.yaml
synx convert config.synx --format toml
synx convert config.synx --format env > .env

# Validate (strict mode, for CI/CD)
synx validate config.synx --strict

# Watch for changes
synx watch config.synx --format json
synx watch config.synx --exec "nginx -s reload"

# Extract JSON Schema from constraints
synx schema config.synx
```

## Export Formats

Convert parsed SYNX to other formats programmatically:

```typescript
const config = Synx.loadSync('config.synx');

Synx.toJSON(config);           // JSON (pretty)
Synx.toJSON(config, false);    // JSON (compact)
Synx.toYAML(config);           // YAML
Synx.toTOML(config);           // TOML
Synx.toEnv(config);            // KEY=VALUE
Synx.toEnv(config, 'PREFIX');  // PREFIX_KEY=VALUE
```

## File Watcher

```typescript
const handle = Synx.watch('config.synx', (config, error) => {
  if (error) return console.error(error);
  console.log('Config reloaded:', config);
}, { strict: true });

handle.close(); // stop watching
```

## Schema Export

Extract constraints as JSON Schema:

```typescript
const schema = Synx.schema(`
!active
app_name[required, min:3, max:30] TotalWario
volume[min:0, max:100, type:int] 75
`);
// { "$schema": "...", "properties": { "app_name": { ... } }, "required": ["app_name"] }
```

## Other Languages

- **Python** — [synx-format](https://pypi.org/project/synx-format/) on PyPI
- **Rust** — [synx](https://crates.io/crates/synx) on crates.io
  ```bash
  cargo add synx
  ```
  ```rust
  use synx::Synx;
  
  let data = Synx::load("config.synx")?;
  println!("{}", &data["server"]["port"]);
  ```

## License

MIT — © APERTURESyndicate
