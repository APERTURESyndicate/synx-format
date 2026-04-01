/**
 * SYNX Color Provider — inline color swatches for #hex values.
 */

import * as vscode from 'vscode';

const HEX_RE = /#([0-9a-fA-F]{3,8})\b/g;

export function createColorProvider(): vscode.Disposable {
  return vscode.languages.registerColorProvider('synx', {
    provideDocumentColors(doc): vscode.ColorInformation[] {
      const colors: vscode.ColorInformation[] = [];

      for (let i = 0; i < doc.lineCount; i++) {
        const line = doc.lineAt(i).text;

        // Skip comments
        if (line.trimStart().startsWith('#')) continue;

        HEX_RE.lastIndex = 0;
        let match;
        while ((match = HEX_RE.exec(line)) !== null) {
          const hex = match[1];
          const color = hexToColor(hex);
          if (color) {
            const range = new vscode.Range(i, match.index, i, match.index + match[0].length);
            colors.push(new vscode.ColorInformation(range, color));
          }
        }
      }

      return colors;
    },

    provideColorPresentations(color): vscode.ColorPresentation[] {
      const r = Math.round(color.red * 255);
      const g = Math.round(color.green * 255);
      const b = Math.round(color.blue * 255);
      const a = color.alpha;

      if (a < 1) {
        const aHex = Math.round(a * 255).toString(16).padStart(2, '0');
        const hex = `#${toHex(r)}${toHex(g)}${toHex(b)}${aHex}`;
        return [new vscode.ColorPresentation(hex)];
      }

      const hex = `#${toHex(r)}${toHex(g)}${toHex(b)}`;
      return [new vscode.ColorPresentation(hex)];
    },
  });
}

function toHex(n: number): string {
  return n.toString(16).padStart(2, '0');
}

function hexToColor(hex: string): vscode.Color | undefined {
  let r: number, g: number, b: number, a = 1;

  if (hex.length === 3) {
    // #RGB → #RRGGBB
    r = parseInt(hex[0] + hex[0], 16) / 255;
    g = parseInt(hex[1] + hex[1], 16) / 255;
    b = parseInt(hex[2] + hex[2], 16) / 255;
  } else if (hex.length === 4) {
    // #RGBA
    r = parseInt(hex[0] + hex[0], 16) / 255;
    g = parseInt(hex[1] + hex[1], 16) / 255;
    b = parseInt(hex[2] + hex[2], 16) / 255;
    a = parseInt(hex[3] + hex[3], 16) / 255;
  } else if (hex.length === 6) {
    // #RRGGBB
    r = parseInt(hex.substring(0, 2), 16) / 255;
    g = parseInt(hex.substring(2, 4), 16) / 255;
    b = parseInt(hex.substring(4, 6), 16) / 255;
  } else if (hex.length === 8) {
    // #RRGGBBAA
    r = parseInt(hex.substring(0, 2), 16) / 255;
    g = parseInt(hex.substring(2, 4), 16) / 255;
    b = parseInt(hex.substring(4, 6), 16) / 255;
    a = parseInt(hex.substring(6, 8), 16) / 255;
  } else {
    return undefined;
  }

  if (isNaN(r) || isNaN(g) || isNaN(b) || isNaN(a)) return undefined;
  return new vscode.Color(r, g, b, a);
}
