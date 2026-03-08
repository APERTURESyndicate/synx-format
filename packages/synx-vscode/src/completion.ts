/**
 * SYNX IntelliSense — completion, hover, and signature help.
 */

import * as vscode from 'vscode';
import { parseSynx } from './parser';

// ─── Marker definitions ──────────────────────────────────────────────────────

interface MarkerInfo {
  label: string;
  detail: string;
  docs: string;
  snippet: string;
}

const MARKERS: MarkerInfo[] = [
  { label: 'calc', detail: 'Arithmetic expression', docs: 'Evaluates a math expression. References numeric keys by name or dot-path.\n\n```synx\n!active\nstats\n  base_hp 150\n  multiplier 1.2\n\ntotal_hp:calc stats.base_hp * stats.multiplier\n```\n\nOperators: `+` `-` `*` `/` `%` `(` `)`\n\n**Safe evaluator** — no `eval()`.', snippet: 'calc' },
  { label: 'random', detail: 'Random selection', docs: 'Picks one random item from the list below.\n\n**Equal probability:**\n```synx\npick:random\n  - Alpha\n  - Beta\n```\n\n**Weighted:**\n```synx\nloot:random 70 20 10\n  - common\n  - rare\n  - legendary\n```', snippet: 'random' },
  { label: 'env', detail: 'Environment variable', docs: 'Reads a system environment variable.\n\n```synx\nport:env PORT\nport:env:default:8080 PORT\n```\n\nReturns `null` if not found (unless combined with `:default`).', snippet: 'env' },
  { label: 'alias', detail: 'Reference another key', docs: 'Copies the value of another key — no duplication.\n\n```synx\nadmin_email alex@example.com\ncomplaints_email:alias admin_email\n```', snippet: 'alias' },
  { label: 'ref', detail: 'Reference with chaining', docs: 'References another key and feeds the resolved value through the remaining marker chain.\n\n```synx\nbase_rate 50\nquick_rate:ref:calc:*2 base_rate\n```\n\nSimple reference: `pg_host:ref host` — same as `:alias` but chainable with `:calc`, `:template`, etc.', snippet: 'ref' },
  { label: 'inherit', detail: 'Inherit block fields', docs: 'Copies fields from one or more parent blocks into this block. Child fields override inherited fields.\n\n```synx\n_base_resource\n  hp 100\n\n_base_rare\n  rarity rare\n\nsteel:inherit:_base_resource:_base_rare\n  name Steel\n```\n\nParents merge left-to-right; later parents override earlier ones.', snippet: 'inherit:${1:_parent}:${2:_mixin}' },
  { label: 'i18n', detail: 'Multilingual value', docs: 'Selects localized value by language. Optional count-field enables plural form selection.\n\n```synx\nitems_count 5\nitems_label:i18n:items_count\n  en\n    one {count} item\n    other {count} items\n```\n\nWith `:i18n:COUNT_FIELD`, the correct plural category is selected and `{count}` is replaced.', snippet: 'i18n:${1:count_field}' },
  { label: 'secret', detail: 'Hidden value', docs: 'Readable by your code but hidden in logs, `toString()`, `JSON.stringify()`.\n\nUse `.reveal()` to access the real value.\n\n```synx\napi_key:secret sk-abc123\n```\n\n```javascript\ndata.api_key.reveal() // "sk-abc123"\n```', snippet: 'secret' },
  { label: 'default', detail: 'Fallback value', docs: 'Sets a fallback if the main value is empty or not found.\n\nMost often combined with `:env`.\n\n```synx\nport:env:default:8080 PORT\ntheme:default dark\n```', snippet: 'default' },
  { label: 'unique', detail: 'Deduplicate list', docs: 'Removes duplicate elements from a list.\n\n```synx\ntags:unique\n  - action\n  - rpg\n  - action\n```\n\nResult: `["action", "rpg"]`', snippet: 'unique' },
  { label: 'include', detail: 'Include external file', docs: 'Inserts contents of another `.synx` file.\nPath is relative to the current file.\n\n```synx\ndatabase:include ./db.synx\n```', snippet: 'include' },
  { label: 'import', detail: 'Alias of :include', docs: 'Alias for `:include` (key-level file embedding).\n\n```synx\ndatabase:import ./db.synx\n```\n\nUse `:import` to avoid confusion with top-level directive `!include`.', snippet: 'import' },
  { label: 'geo', detail: 'Region-based selection', docs: 'Selects a value based on the user\'s region.\n\n```synx\ncurrency:geo\n  - US USD\n  - EU EUR\n  - GB GBP\n```\n\nRequires runtime region support.', snippet: 'geo' },
  { label: 'template', detail: 'String interpolation (legacy)', docs: '**Legacy marker** — `{}` interpolation now works automatically on all string values in `!active` mode.\nYou no longer need `:template`.\n\n```synx\nfirst_name John\ngreeting Hello, {first_name}!\n```', snippet: 'template' },
  { label: 'split', detail: 'Split string → array', docs: 'Splits a string by delimiter into an array.\n\nDefault: comma. Keywords: `space`, `pipe`, `dash`, `dot`, `semi`, `tab`.\n\n```synx\ncolors:split red, green, blue\nwords:split:space hello world foo\n```', snippet: 'split' },
  { label: 'join', detail: 'Join array → string', docs: 'Joins list elements into a string with a delimiter.\n\nDefault: comma. Keywords: `space`, `pipe`, `dash`, `slash`.\n\n```synx\npath:join:slash\n  - home\n  - user\n  - documents\n```\n\nResult: `"home/user/documents"`', snippet: 'join' },
  { label: 'clamp', detail: 'Clamp number to range', docs: 'Clamps a numeric value to `[min, max]`.\n\n```synx\n!active\nvolume:clamp:0:100 75\nepsilon:clamp:0.0:1.0 1.5\n```\n\nMin and max are specified in the marker chain.', snippet: 'clamp:${1:0}:${2:100}' },
  { label: 'round', detail: 'Round to N decimal places', docs: 'Rounds a number to the specified number of decimal places. Works standalone or after `:calc`.\n\n```synx\n!active\nprice:round:2 109.5678\nprofit:calc:round:2 revenue * 0.3\n```\n\nWith `:round:0` (default) returns an integer.', snippet: 'round:${1:2}' },
  { label: 'map', detail: 'Map lookup via another key', docs: 'Looks up the value of a source key and maps it to a human-readable label using the list below.\n\n```synx\n!active\nstatus_code 1\nstatus_label:map:status_code\n  - 0 offline\n  - 1 online\n  - 2 away\n```\n\nReturns `null` if no matching entry is found.', snippet: 'map:${1:source_key}' },
  { label: 'format', detail: 'Format number as string', docs: 'Formats a number using a printf-style pattern.\n\nSupported patterns:\n- `%.2f` — fixed decimal places\n- `%05d` — zero-padded integer\n- `%e` — scientific notation\n\n```synx\n!active\nprice:format:%.2f 1234.5\nid:format:%06d 42\n```', snippet: 'format:${1:%.2f}' },
  { label: 'fallback', detail: 'File-path fallback', docs: 'Checks whether the value (a file path) exists on disk. Uses the fallback path if the file is missing or the value is empty.\n\n```synx\n!active\nicon:fallback:./default.png ./custom.png\ntheme_file:fallback:./themes/default.css ./themes/user.css\n```', snippet: 'fallback:${1:./default.txt}' },
  { label: 'once', detail: 'Generate-and-persist value', docs: 'Generates a value **once** on first parse and stores it in a `.synx.lock` file. Subsequent parses return the same value.\n\nGeneration types: `uuid` (default), `random`, `timestamp`\n\n```synx\n!active\nsession_id:once uuid\napp_seed:once random\nbuild_time:once timestamp\n```', snippet: 'once' },
  { label: 'version', detail: 'Semantic version compare', docs: 'Compares the value (a version string) against a required version using an operator. Returns a boolean.\n\nOperators: `>=` `<=` `>` `<` `==` `!=`\n\n```synx\n!active\nruntime:version:>=:18.0 20.11.0\napi_compat:version:==:3.0 3.0.0\n```', snippet: 'version:${1:>=}:${2:1.0.0}' },
  { label: 'watch', detail: 'Read external file at parse time', docs: 'Reads an external file at parse time and uses its content as the value. Optionally extracts a key from JSON or SYNX files.\n\n```synx\n!active\nraw_data:watch ./data.txt\napp_name:watch:name ./package.json\ndb_host:watch:database.host ./config.synx\n```\n\nThe file is re-read every time the SYNX document is parsed (hot-reload when combined with a file watcher).', snippet: 'watch' },
];

const CONSTRAINT_ITEMS: vscode.CompletionItem[] = [
  makeConstraint('required', 'Mark key as required — parser throws if empty/missing'),
  makeConstraint('readonly', 'Mark key as read-only — cannot be changed via API/hot-reload'),
  makeConstraint('min:', 'Minimum value (numbers) or length (strings)'),
  makeConstraint('max:', 'Maximum value (numbers) or length (strings)'),
  makeConstraint('type:', 'Enforce type: int, float, bool, string'),
  makeConstraint('pattern:', 'Regex pattern validation — value must match'),
  makeConstraint('enum:', 'Allowed values (pipe-separated: light|dark|auto)'),
];

function makeConstraint(label: string, detail: string): vscode.CompletionItem {
  const item = new vscode.CompletionItem(label, vscode.CompletionItemKind.Property);
  item.detail = detail;
  return item;
}

// ─── Completion Provider ─────────────────────────────────────────────────────

export function createCompletionProvider(): vscode.Disposable {
  return vscode.languages.registerCompletionItemProvider('synx', {
    provideCompletionItems(doc, position) {
      const line = doc.lineAt(position).text;
      const before = line.substring(0, position.character);

      // Inside [constraints]
      const bracketOpen = before.lastIndexOf('[');
      const bracketClose = before.lastIndexOf(']');
      if (bracketOpen !== -1 && bracketClose < bracketOpen) {
        return CONSTRAINT_ITEMS;
      }

      // After : in key:marker (no space yet)
      if (before.includes(':') && !before.includes(' ')) {
        return MARKERS.map(m => {
          const item = new vscode.CompletionItem(m.label, vscode.CompletionItemKind.Function);
          item.detail = m.detail;
          item.documentation = new vscode.MarkdownString(m.docs);
          item.insertText = new vscode.SnippetString(m.snippet);
          return item;
        });
      }

      // After ( → type casts
      if (before.endsWith('(') || /\(\w*$/.test(before)) {
        return ['int', 'float', 'bool', 'string', 'random', 'random:int', 'random:float', 'random:bool'].map(t => {
          const item = new vscode.CompletionItem(t, vscode.CompletionItemKind.TypeParameter);
          item.detail = t.startsWith('random') ? `Generate random ${t === 'random' ? 'int' : t.split(':')[1]}` : `Cast to ${t}`;
          item.insertText = new vscode.SnippetString(t + ')');
          return item;
        });
      }

      // Start of line → !active, !lock
      if (before.trim() === '!' || before.trim() === '') {
        const items: vscode.CompletionItem[] = [];

        const activeItem = new vscode.CompletionItem('!active', vscode.CompletionItemKind.Keyword);
        activeItem.detail = 'Enable active mode (functions + constraints)';
        activeItem.documentation = new vscode.MarkdownString(
          'First line of the file. Enables `:calc`, `:random`, `:env`, `{}` interpolation, and other markers.\n\n' +
          'Without `!active` — all markers are treated as plain text.'
        );
        if (before.trim() === '!') {
          activeItem.insertText = 'active';
          activeItem.range = new vscode.Range(position.line, position.character, position.line, position.character);
        }
        items.push(activeItem);

        const lockItem = new vscode.CompletionItem('!lock', vscode.CompletionItemKind.Keyword);
        lockItem.detail = 'Lock config (prevent external set/add/remove)';
        lockItem.documentation = new vscode.MarkdownString(
          'Prevents code from modifying config values via `Synx.set()`, `Synx.add()`, `Synx.remove()`.\n\n' +
          'Internal SYNX markers still work normally.\n\n' +
          '```synx\n!active\n!lock\nmax_players 100\n```'
        );
        if (before.trim() === '!') {
          lockItem.insertText = 'lock';
          lockItem.range = new vscode.Range(position.line, position.character, position.line, position.character);
        }
        items.push(lockItem);

        const includeItem = new vscode.CompletionItem('!include', vscode.CompletionItemKind.Keyword);
        includeItem.detail = 'Include external file for {} references';
        includeItem.documentation = new vscode.MarkdownString(
          'Imports another `.synx` file, making its keys available for `{key:alias}` interpolation.\n\n' +
          '```synx\n!active\n!include ./db.synx db\n\nurl http://{host:db}:{port:db}/mydb\n```'
        );
        if (before.trim() === '!') {
          includeItem.insertText = 'include ${1:./file.synx} ${2:alias}';
          includeItem.insertText = new vscode.SnippetString('include ${1:./file.synx} ${2:alias}');
          includeItem.range = new vscode.Range(position.line, position.character, position.line, position.character);
        }
        items.push(includeItem);

        return items;
      }

      // Inside {} (auto-interpolation) → suggest known keys
      if (before.match(/\{[\w.]*$/) || before.includes(':template') || line.match(/:\w*template/)) {
        const parsed = parseSynx(doc.getText());
        const items: vscode.CompletionItem[] = [];
        for (const [keyPath] of parsed.keyMap) {
          const item = new vscode.CompletionItem(`{${keyPath}}`, vscode.CompletionItemKind.Variable);
          item.detail = `Reference to ${keyPath}`;
          item.insertText = `{${keyPath}}`;
          items.push(item);
        }
        return items;
      }

      // Inside :alias → suggest existing keys
      if (before.includes(':alias ') || before.match(/:alias\s*$/)) {
        const parsed = parseSynx(doc.getText());
        const items: vscode.CompletionItem[] = [];
        for (const [keyPath, node] of parsed.keyMap) {
          if (node.markers.length === 0 || !node.markers.includes('alias')) {
            const item = new vscode.CompletionItem(keyPath, vscode.CompletionItemKind.Reference);
            item.detail = `Key at line ${node.line + 1}`;
            items.push(item);
          }
        }
        return items;
      }

      // Inside :calc expression → suggest known keys (including dot-paths)
      if (before.match(/:calc\s+[\w.]*$/)) {
        const parsed = parseSynx(doc.getText());
        const items: vscode.CompletionItem[] = [];
        for (const [keyPath, node] of parsed.keyMap) {
          const item = new vscode.CompletionItem(keyPath, vscode.CompletionItemKind.Variable);
          item.detail = `Key at line ${node.line + 1}`;
          items.push(item);
        }
        return items;
      }

      return undefined;
    },
  }, ':', '(', '[', '!', '{');
}

// ─── Hover Provider ──────────────────────────────────────────────────────────

export function createHoverProvider(): vscode.Disposable {
  return vscode.languages.registerHoverProvider('synx', {
    provideHover(doc, position) {
      const line = doc.lineAt(position).text;

      // Hover on marker :name
      const markerRange = doc.getWordRangeAtPosition(position, /:\w+/);
      if (markerRange) {
        const word = doc.getText(markerRange);
        const markerName = word.substring(1);
        const marker = MARKERS.find(m => m.label === markerName);
        if (marker) {
          const md = new vscode.MarkdownString();
          md.appendMarkdown(`**:${marker.label}** — ${marker.detail}\n\n`);
          md.appendMarkdown(marker.docs);
          return new vscode.Hover(md, markerRange);
        }
      }

      // Hover on constraint [name:value]
      const bracketRange = doc.getWordRangeAtPosition(position, /\[[^\]]+\]/);
      if (bracketRange) {
        const text = doc.getText(bracketRange);
        const md = new vscode.MarkdownString();
        md.appendMarkdown(`**Constraints:** \`${text}\`\n\n`);
        md.appendMarkdown('Constraints validate the value in `!active` mode.\n');
        md.appendMarkdown('Available: `min`, `max`, `type`, `required`, `pattern`, `enum`, `readonly`');
        return new vscode.Hover(md, bracketRange);
      }

      // Hover on !active
      const activeRange = doc.getWordRangeAtPosition(position, /!active/);
      if (activeRange) {
        const md = new vscode.MarkdownString();
        md.appendMarkdown('**!active** — Enables live config mode\n\n');
        md.appendMarkdown('When present on the first line, markers (`:calc`, `:random`, `:env`, etc.) and constraints (`[min:N]`, `[required]`, etc.) become functional.\n\n');
        md.appendMarkdown('Without `!active`, the file is treated as plain static data — markers are literal key names.');
        return new vscode.Hover(md, activeRange);
      }

      // Hover on key → show value type
      const parsed = parseSynx(doc.getText());
      for (const [keyPath, node] of parsed.keyMap) {
        if (node.line === position.line) {
          const keyStart = line.indexOf(node.key);
          const keyEnd = keyStart + node.key.length;
          if (position.character >= keyStart && position.character <= keyEnd) {
            const md = new vscode.MarkdownString();
            md.appendMarkdown(`**${keyPath}**\n\n`);
            const val = node.value;
            const type = val === null ? 'null' : Array.isArray(val) ? 'array' : typeof val;
            md.appendMarkdown(`Type: \`${type}\``);
            if (node.typeHint) md.appendMarkdown(` (cast: \`${node.typeHint}\`)`);
            if (node.markers.length) md.appendMarkdown(`\n\nMarkers: ${node.markers.map(m => '`:' + m + '`').join(', ')}`);
            if (node.constraints) md.appendMarkdown(`\n\nConstraints: \`[${node.constraints}]\``);
            md.appendMarkdown(`\n\nValue: \`${JSON.stringify(val)}\``);
            return new vscode.Hover(md, new vscode.Range(position.line, keyStart, position.line, keyEnd));
          }
        }
      }

      return undefined;
    },
  });
}
