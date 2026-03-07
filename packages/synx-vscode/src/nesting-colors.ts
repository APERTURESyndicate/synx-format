/**
 * SYNX Nesting Colors — applies depth-based foreground colors to keys
 * so each indentation level gets a distinct, readable colour.
 */

import * as vscode from 'vscode';

// ── Colour palette (dark / light) ────────────────────────────────────────────
// Each level defines a key colour and a complementary value colour.
const LEVELS = [
  { dark: '#569CD6', light: '#0451A5' },   // blue
  { dark: '#4EC9B0', light: '#0F7B6C' },   // teal
  { dark: '#DCDCAA', light: '#795E26' },   // yellow / olive
  { dark: '#C586C0', light: '#AF00DB' },   // purple
  { dark: '#CE9178', light: '#A31515' },   // orange / red
  { dark: '#D7BA7D', light: '#986801' },   // gold / brown
];

// ── Decoration types (created once) ──────────────────────────────────────────
const keyDecorations: vscode.TextEditorDecorationType[] = LEVELS.map(c =>
  vscode.window.createTextEditorDecorationType({
    dark:  { color: c.dark },
    light: { color: c.light },
  }),
);

// Simple regex to detect a key token at the start of a (possibly indented) line.
// Matches: optional spaces, then a key that doesn't start with #, /, !, -
const KEY_RE = /^(\s*)([^\s\[:#/!\-(][^\s\[:(]*)/;

export function activateNestingColors(ctx: vscode.ExtensionContext): void {
  const update = (editor: vscode.TextEditor | undefined) => {
    if (!editor || editor.document.languageId !== 'synx') return;
    applyNestingColors(editor);
  };

  update(vscode.window.activeTextEditor);

  ctx.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(update),
    vscode.workspace.onDidChangeTextDocument(e => {
      const editor = vscode.window.activeTextEditor;
      if (editor && e.document === editor.document) update(editor);
    }),
  );

  ctx.subscriptions.push(...keyDecorations);
}

function applyNestingColors(editor: vscode.TextEditor): void {
  const doc = editor.document;
  const buckets: vscode.Range[][] = LEVELS.map(() => []);

  for (let i = 0; i < doc.lineCount; i++) {
    const text = doc.lineAt(i).text;
    const trimmed = text.trim();

    // Skip blanks, comments, mode/lock directives, list items
    if (!trimmed || trimmed.startsWith('#') || trimmed.startsWith('//') ||
        trimmed.startsWith('!') || trimmed.startsWith('- ')) {
      continue;
    }

    const m = text.match(KEY_RE);
    if (!m) continue;

    const indent = m[1].length;
    const key = m[2];
    const level = Math.min(Math.floor(indent / 2), LEVELS.length - 1);

    const start = new vscode.Position(i, indent);
    const end = new vscode.Position(i, indent + key.length);
    buckets[level].push(new vscode.Range(start, end));
  }

  for (let lv = 0; lv < LEVELS.length; lv++) {
    editor.setDecorations(keyDecorations[lv], buckets[lv]);
  }
}
