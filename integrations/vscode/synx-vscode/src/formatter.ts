/**
 * SYNX Formatter — normalizes indentation to 2 spaces, trims trailing whitespace.
 */

import * as vscode from 'vscode';

export function createFormattingProvider(): vscode.Disposable {
  return vscode.languages.registerDocumentFormattingEditProvider('synx', {
    provideDocumentFormattingEdits(doc): vscode.TextEdit[] {
      const edits: vscode.TextEdit[] = [];

      for (let i = 0; i < doc.lineCount; i++) {
        const line = doc.lineAt(i);
        const text = line.text;

        // Skip empty lines
        if (text.trim().length === 0) {
          if (text.length > 0) {
            edits.push(vscode.TextEdit.replace(line.range, ''));
          }
          continue;
        }

        let formatted = text;

        // Replace tabs with 2 spaces
        if (formatted.includes('\t')) {
          const match = formatted.match(/^(\s*)/);
          if (match) {
            const leading = match[1].replace(/\t/g, '  ');
            formatted = leading + formatted.trimStart();
          }
        }

        // Normalize indentation: detect current indent, ensure it's a multiple of 2
        const leadingSpaces = formatted.match(/^( *)/)?.[1].length ?? 0;
        if (leadingSpaces % 2 !== 0) {
          const corrected = Math.round(leadingSpaces / 2) * 2;
          formatted = ' '.repeat(corrected) + formatted.trimStart();
        }

        // Trim trailing whitespace
        formatted = formatted.trimEnd();

        // Normalize key-value separator spacing: `key  :  value` → `key: value`
        // Only for lines that have a key (not list items, not comments)
        if (!formatted.trimStart().startsWith('#') && !formatted.trimStart().startsWith('-')) {
          const trimmed = formatted.trimStart();
          const indent = formatted.length - trimmed.length;
          // Match key followed by optional markers/constraints, then colon separator
          const kvMatch = trimmed.match(/^(\S+(?:\s*\([^)]*\))?(?:\s*\[[^\]]*\])?)\s*:\s+/);
          if (kvMatch) {
            const keyPart = kvMatch[1];
            const rest = trimmed.substring(kvMatch[0].length);
            formatted = ' '.repeat(indent) + keyPart + ': ' + rest;
          }
        }

        if (formatted !== text) {
          edits.push(vscode.TextEdit.replace(line.range, formatted));
        }
      }

      return edits;
    },
  });
}
