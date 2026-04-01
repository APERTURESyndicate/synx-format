/**
 * SYNX Comment Styling — decorates markdown-like formatting inside comments.
 *
 * Supports:  *italic*  **bold**  ***bold-italic***  `code`
 * Works in single-line (#, //) and block (###) comments.
 */

import * as vscode from 'vscode';

// ── Decoration types ──

const italicDeco = vscode.window.createTextEditorDecorationType({
  fontStyle: 'italic',
  dark:  { color: '#6A9955' },   // green (dimmer than default comment)
  light: { color: '#008000' },
});

const boldDeco = vscode.window.createTextEditorDecorationType({
  fontWeight: 'bold',
  dark:  { color: '#C586C0' },   // purple
  light: { color: '#AF00DB' },
});

const boldItalicDeco = vscode.window.createTextEditorDecorationType({
  fontWeight: 'bold',
  fontStyle: 'italic',
  dark:  { color: '#D7BA7D' },   // gold
  light: { color: '#986801' },
});

const codeDeco = vscode.window.createTextEditorDecorationType({
  dark:  { color: '#CE9178', backgroundColor: '#ffffff10' },   // orange string-like
  light: { color: '#A31515', backgroundColor: '#00000008' },
});

const doubleQuoteDeco = vscode.window.createTextEditorDecorationType({
  dark:  { color: '#CE9178' },   // orange (like strings)
  light: { color: '#A31515' },
});

const singleQuoteDeco = vscode.window.createTextEditorDecorationType({
  dark:  { color: '#9CDCFE' },   // light blue
  light: { color: '#0070C1' },
});

// ── Patterns ──

const BOLD_ITALIC_RE = /\*\*\*([^*]+)\*\*\*/g;
const BOLD_RE        = /\*\*([^*]+)\*\*/g;
const ITALIC_RE      = /(?<!\*)\*([^*]+)\*(?!\*)/g;
const CODE_RE        = /`([^`]+)`/g;
const DOUBLE_QUOTE_RE = /"([^"]*)"/g;
const SINGLE_QUOTE_RE = /'([^']*)'/g;

export function activateCommentStyling(ctx: vscode.ExtensionContext): void {
  const update = (editor?: vscode.TextEditor) => {
    if (!editor || editor.document.languageId !== 'synx') return;
    applyCommentStyles(editor);
  };

  update(vscode.window.activeTextEditor);

  ctx.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(update),
    vscode.workspace.onDidChangeTextDocument(e => {
      const editor = vscode.window.activeTextEditor;
      if (editor && e.document === editor.document) update(editor);
    }),
    italicDeco, boldDeco, boldItalicDeco, codeDeco, doubleQuoteDeco, singleQuoteDeco,
  );
}

function applyCommentStyles(editor: vscode.TextEditor): void {
  const doc = editor.document;
  const italics: vscode.Range[] = [];
  const bolds: vscode.Range[] = [];
  const boldItalics: vscode.Range[] = [];
  const codes: vscode.Range[] = [];
  const doubleQuotes: vscode.Range[] = [];
  const singleQuotes: vscode.Range[] = [];

  let inBlock = false;

  for (let i = 0; i < doc.lineCount; i++) {
    const text = doc.lineAt(i).text;
    const trimmed = text.trimStart();

    // Block comment fence
    if (trimmed.trimEnd() === '###') {
      inBlock = !inBlock;
      continue;
    }

    // Only process comment lines
    let commentText: string;
    let offset: number;

    if (inBlock) {
      commentText = text;
      offset = 0;
    } else if (trimmed.startsWith('#') && !trimmed.startsWith('#!')) {
      offset = text.indexOf('#');
      commentText = text.substring(offset);
    } else if (trimmed.startsWith('//')) {
      offset = text.indexOf('//');
      commentText = text.substring(offset);
    } else {
      continue;
    }

    // Collect formatted spans (bold-italic first to avoid partial matches)
    collectMatches(commentText, BOLD_ITALIC_RE, offset, i, boldItalics);
    collectMatches(commentText, BOLD_RE, offset, i, bolds);
    collectMatches(commentText, ITALIC_RE, offset, i, italics);
    collectMatches(commentText, CODE_RE, offset, i, codes);
    collectMatches(commentText, DOUBLE_QUOTE_RE, offset, i, doubleQuotes);
    collectMatches(commentText, SINGLE_QUOTE_RE, offset, i, singleQuotes);
  }

  editor.setDecorations(boldItalicDeco, boldItalics);
  editor.setDecorations(boldDeco, bolds);
  editor.setDecorations(italicDeco, italics);
  editor.setDecorations(codeDeco, codes);
  editor.setDecorations(doubleQuoteDeco, doubleQuotes);
  editor.setDecorations(singleQuoteDeco, singleQuotes);
}

function collectMatches(
  text: string,
  re: RegExp,
  lineOffset: number,
  line: number,
  out: vscode.Range[],
): void {
  re.lastIndex = 0;
  let m;
  while ((m = re.exec(text)) !== null) {
    // m[1] is the inner content; highlight the whole match including delimiters
    const start = lineOffset + m.index;
    const end = start + m[0].length;
    out.push(new vscode.Range(line, start, line, end));
  }
}
