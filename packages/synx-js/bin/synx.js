#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const Synx = require('../dist/index');

const args = process.argv.slice(2);
const command = args[0];

function usage() {
  console.log(`
SYNX CLI — The Active Data Format (v3.6.0)

DEPRECATED: This Node.js CLI is superseded by the native Rust CLI.
            Install: cargo install synx-cli
            Docs:    https://github.com/APERTURESyndicate/synx-format#cli-rust

Usage:
  synx convert <file.synx> [--format json|yaml|toml|env]  [--strict] [--active]
  synx validate <file.synx>  [--strict]
  synx watch <file.synx> [--format json] [--exec <command>]
  synx schema <file.synx>

Commands:
  convert    Parse .synx and output in another format (default: json)
  validate   Parse with strict mode — exits 1 on any marker error
  watch      Watch file for changes, re-parse and print or exec command
  schema     Extract constraints as JSON Schema

Options:
  --format, -f   Output format: json (default), yaml, toml, env
  --strict       Throw on INCLUDE_ERR / WATCH_ERR / CALC_ERR / CONSTRAINT_ERR
  --active       Force active mode resolution
  --exec, -e     Command to run after each re-parse (watch mode)
  --help, -h     Show this help
`);
  process.exit(0);
}

function getFlag(flag, alias) {
  const idx = args.indexOf(flag);
  const aidx = alias ? args.indexOf(alias) : -1;
  return idx !== -1 || aidx !== -1;
}

function getFlagValue(flag, alias, defaultVal) {
  let idx = args.indexOf(flag);
  if (idx === -1 && alias) idx = args.indexOf(alias);
  if (idx === -1 || idx + 1 >= args.length) return defaultVal;
  return args[idx + 1];
}

if (!command || command === '--help' || command === '-h') usage();

const file = args[1];
if (!file) {
  console.error('Error: no file specified');
  process.exit(1);
}

const absPath = path.resolve(file);
if (!fs.existsSync(absPath)) {
  console.error(`Error: file not found: ${absPath}`);
  process.exit(1);
}

const format = getFlagValue('--format', '-f', 'json');
const strict = getFlag('--strict');
const execCmd = getFlagValue('--exec', '-e', null);

function parseFile(filePath) {
  const text = fs.readFileSync(filePath, 'utf-8');
  return Synx.parse(text, {
    basePath: path.dirname(filePath),
    strict,
  });
}

function formatOutput(obj) {
  switch (format) {
    case 'json': return Synx.toJSON(obj);
    case 'yaml': return Synx.toYAML(obj);
    case 'toml': return Synx.toTOML(obj);
    case 'env':  return Synx.toEnv(obj);
    default:
      console.error(`Unknown format: ${format}. Use json, yaml, toml, or env.`);
      process.exit(1);
  }
}

switch (command) {
  case 'convert': {
    try {
      const obj = parseFile(absPath);
      process.stdout.write(formatOutput(obj));
    } catch (e) {
      console.error(e.message);
      process.exit(1);
    }
    break;
  }

  case 'validate': {
    try {
      const text = fs.readFileSync(absPath, 'utf-8');
      Synx.parse(text, { basePath: path.dirname(absPath), strict: true });
      console.log('OK');
    } catch (e) {
      console.error('FAIL:', e.message);
      process.exit(1);
    }
    break;
  }

  case 'watch': {
    console.error(`[synx] watching ${absPath}`);
    const handle = Synx.watch(absPath, (config, error) => {
      if (error) {
        console.error(`[synx] error: ${error.message}`);
        return;
      }
      if (execCmd) {
        const { execSync } = require('child_process');
        try {
          execSync(execCmd, { stdio: 'inherit' });
        } catch { /* command failed */ }
      } else {
        process.stdout.write(formatOutput(config));
      }
    }, { basePath: path.dirname(absPath), strict });
    process.on('SIGINT', () => { handle.close(); process.exit(0); });
    break;
  }

  case 'schema': {
    const text = fs.readFileSync(absPath, 'utf-8');
    const schema = Synx.schema(text);
    console.log(JSON.stringify(schema, null, 2));
    break;
  }

  default:
    console.error(`Unknown command: ${command}`);
    usage();
}
