/**
 * SYNX Navigation — DocumentSymbol, Go to Definition, Find References, Breadcrumbs.
 */

import * as vscode from 'vscode';
import { parseSynx, ParsedDoc, SynxNode } from './parser';

// ─── Document Symbol Provider (Outline + Breadcrumbs) ────────────────────────

export function createDocumentSymbolProvider(): vscode.Disposable {
  return vscode.languages.registerDocumentSymbolProvider('synx', {
    provideDocumentSymbols(doc): vscode.DocumentSymbol[] {
      const parsed = parseSynx(doc.getText());
      return buildSymbols(parsed.nodes, doc);
    },
  });
}

function buildSymbols(nodes: SynxNode[], doc: vscode.TextDocument): vscode.DocumentSymbol[] {
  const symbols: vscode.DocumentSymbol[] = [];

  for (const node of nodes) {
    if (node.isListItem) continue;

    const kind = getSymbolKind(node);
    const detail = getSymbolDetail(node);
    const range = new vscode.Range(node.line, node.column, node.line, doc.lineAt(node.line).text.length);
    const selRange = new vscode.Range(node.line, node.column, node.line, node.column + node.key.length);

    const sym = new vscode.DocumentSymbol(node.key, detail, kind, range, selRange);

    // Add children recursively
    if (node.children.length > 0) {
      const childSymbols = buildSymbols(node.children, doc);
      // Also add list items as children
      for (const child of node.children) {
        if (child.isListItem) {
          const itemRange = new vscode.Range(child.line, child.column, child.line, doc.lineAt(child.line).text.length);
          const itemSel = new vscode.Range(child.line, child.column, child.line, child.column + String(child.rawValue).length);
          const label = child.key.startsWith('[') ? String(child.rawValue || child.value) : child.key;
          childSymbols.push(new vscode.DocumentSymbol(
            label, '', vscode.SymbolKind.EnumMember, itemRange, itemSel
          ));
        }
      }
      sym.children = childSymbols;
    }

    symbols.push(sym);
  }

  return symbols;
}

function getSymbolKind(node: SynxNode): vscode.SymbolKind {
  if (Array.isArray(node.value)) return vscode.SymbolKind.Array;
  if (typeof node.value === 'object' && node.value !== null) return vscode.SymbolKind.Object;
  if (typeof node.value === 'number') return vscode.SymbolKind.Number;
  if (typeof node.value === 'boolean') return vscode.SymbolKind.Boolean;
  if (node.value === null) return vscode.SymbolKind.Null;
  if (node.markers.includes('secret')) return vscode.SymbolKind.Key;
  if (node.markers.length > 0) return vscode.SymbolKind.Function;
  return vscode.SymbolKind.Property;
}

function getSymbolDetail(node: SynxNode): string {
  if (node.markers.length > 0) return ':' + node.markers.join(':');
  if (node.rawValue) {
    const v = String(node.rawValue);
    return v.length > 40 ? v.substring(0, 37) + '...' : v;
  }
  const type = node.value === null ? 'null' : Array.isArray(node.value) ? 'array' : typeof node.value;
  return type;
}

// ─── Definition Provider (Go to Definition) ──────────────────────────────────

export function createDefinitionProvider(): vscode.Disposable {
  return vscode.languages.registerDefinitionProvider('synx', {
    provideDefinition(doc, position): vscode.Location | undefined {
      const line = doc.lineAt(position).text;
      const parsed = parseSynx(doc.getText());

      // Find the node at current line
      const currentNode = parsed.allNodes.find(n => n.line === position.line && !n.isListItem);

      // :alias <target> → go to target definition
      if (currentNode?.markers.includes('alias') && currentNode.rawValue) {
        const ref = currentNode.rawValue.trim();
        const target = parsed.keyMap.get(ref);
        if (target) {
          return new vscode.Location(doc.uri,
            new vscode.Position(target.line, target.column));
        }
      }

      // :template {key.path} → go to referenced key
      const wordRange = doc.getWordRangeAtPosition(position, /\{(\w+(?:\.\w+)*)\}/);
      if (wordRange) {
        const text = doc.getText(wordRange);
        const ref = text.slice(1, -1);
        const target = parsed.keyMap.get(ref);
        if (target) {
          return new vscode.Location(doc.uri,
            new vscode.Position(target.line, target.column));
        }
      }

      // :calc <expression with key names> → go to referenced key
      if (currentNode?.markers.includes('calc') && currentNode.rawValue) {
        const wordR = doc.getWordRangeAtPosition(position, /[a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)*/);
        if (wordR) {
          const word = doc.getText(wordR);
          const parentPath = currentNode.dotPath.includes('.')
            ? currentNode.dotPath.substring(0, currentNode.dotPath.lastIndexOf('.'))
            : '';
          const target = parsed.keyMap.get(word) ?? (parentPath ? parsed.keyMap.get(`${parentPath}.${word}`) : undefined);
          if (target) {
            return new vscode.Location(doc.uri,
              new vscode.Position(target.line, target.column));
          }
        }
      }

      // :inherit[:parent...] → go to parent definition under cursor
      if (currentNode?.markers.includes('inherit')) {
        const wordR = doc.getWordRangeAtPosition(position, /[a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)*/);
        if (wordR) {
          const word = doc.getText(wordR);
          const parentPath = currentNode.dotPath.includes('.')
            ? currentNode.dotPath.substring(0, currentNode.dotPath.lastIndexOf('.'))
            : '';
          const target = parsed.keyMap.get(word) ?? (parentPath ? parsed.keyMap.get(`${parentPath}.${word}`) : undefined);
          if (target) {
            return new vscode.Location(doc.uri,
              new vscode.Position(target.line, target.column));
          }
        }
      }

      // :include / :import <path> → go to file
      if ((currentNode?.markers.includes('include') || currentNode?.markers.includes('import')) && currentNode.rawValue) {
        const filePath = currentNode.rawValue.trim();
        const vpos = line.lastIndexOf(filePath);
        if (vpos !== -1 && position.character >= vpos && position.character <= vpos + filePath.length) {
          const resolved = vscode.Uri.joinPath(
            vscode.Uri.file(doc.uri.fsPath).with({ path: doc.uri.fsPath.replace(/[^/\\]+$/, '') }),
            filePath
          );
          return new vscode.Location(resolved, new vscode.Position(0, 0));
        }
      }

      return undefined;
    },
  });
}

// ─── Reference Provider (Find References) ────────────────────────────────────

export function createReferenceProvider(): vscode.Disposable {
  return vscode.languages.registerReferenceProvider('synx', {
    provideReferences(doc, position): vscode.Location[] {
      const parsed = parseSynx(doc.getText());
      const locations: vscode.Location[] = [];

      // Find which key is under cursor
      const currentNode = parsed.allNodes.find(n =>
        n.line === position.line && !n.isListItem &&
        position.character >= n.column && position.character <= n.column + n.key.length
      );
      if (!currentNode) return locations;

      // Find the dotPath of this key
      let targetPath = '';
      for (const [path, node] of parsed.keyMap) {
        if (node === currentNode) {
          targetPath = path;
          break;
        }
      }
      if (!targetPath) return locations;

      // Search all nodes for references to this key
      for (const node of parsed.allNodes) {
        // :alias references
        if (node.markers.includes('alias') && node.rawValue?.trim() === targetPath) {
          locations.push(new vscode.Location(doc.uri, new vscode.Position(node.line, node.column)));
        }

        // :template {key} references
        if (node.markers.includes('template') && node.rawValue) {
          const re = new RegExp(`\\{${targetPath.replace('.', '\\.')}\\}`, 'g');
          let m;
          while ((m = re.exec(node.rawValue)) !== null) {
            const line = doc.lineAt(node.line).text;
            const idx = line.indexOf(m[0]);
            if (idx !== -1) {
              locations.push(new vscode.Location(doc.uri, new vscode.Position(node.line, idx)));
            }
          }
        }

        // :calc references
        if (node.markers.includes('calc') && node.rawValue) {
          const simpleKey = targetPath.split('.').pop() || targetPath;
          const re = new RegExp(`\\b${simpleKey}\\b`, 'g');
          let m;
          while ((m = re.exec(node.rawValue)) !== null) {
            const line = doc.lineAt(node.line).text;
            const calcStart = line.indexOf(node.rawValue);
            if (calcStart !== -1) {
              locations.push(new vscode.Location(doc.uri, new vscode.Position(node.line, calcStart + m.index)));
            }
          }
        }
      }

      // Include the definition itself
      locations.push(new vscode.Location(doc.uri, new vscode.Position(currentNode.line, currentNode.column)));

      return locations;
    },
  });
}
