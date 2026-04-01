/**
 * SYNX Inlay Hints — show computed :calc results inline.
 */

import * as vscode from 'vscode';
import { parseSynx, safeCalc, SynxNode } from './parser';

export function createInlayHintsProvider(): vscode.Disposable {
  return vscode.languages.registerInlayHintsProvider('synx', {
    provideInlayHints(doc, range): vscode.InlayHint[] {
      const hints: vscode.InlayHint[] = [];
      const parsed = parseSynx(doc.getText());

      if (parsed.mode !== 'active') return hints;

      // Build a variable map from all numeric keys
      const vars: Record<string, number> = {};
      for (const [key, node] of parsed.keyMap) {
        if (typeof node.value === 'number') {
          vars[key] = node.value;
          // Also add short name (last segment)
          const short = key.split('.').pop();
          if (short && !(short in vars)) {
            vars[short] = node.value;
          }
        }
      }

      for (const node of parsed.allNodes) {
        if (node.line < range.start.line || node.line > range.end.line) continue;
        if (!node.markers.includes('calc') || !node.rawValue) continue;

        const result = safeCalc(node.rawValue.trim(), vars);
        if (result !== null && result !== undefined && !isNaN(result)) {
          const line = doc.lineAt(node.line).text;
          const pos = new vscode.Position(node.line, line.length);
          const hint = new vscode.InlayHint(pos, ` = ${formatNumber(result!)}`, vscode.InlayHintKind.Type);
          hint.paddingLeft = true;
          hint.tooltip = `Computed from: ${node.rawValue.trim()}`;
          hints.push(hint);
        }
      }

      return hints;
    },
  });
}

function formatNumber(n: number): string {
  if (Number.isInteger(n)) return String(n);
  return n.toFixed(4).replace(/\.?0+$/, '');
}
