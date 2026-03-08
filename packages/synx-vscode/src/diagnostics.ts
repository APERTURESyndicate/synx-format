/**
 * SYNX Diagnostics — comprehensive real-time validation for .synx files.
 */

import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';
import { parseSynx, ParsedDoc, SynxNode, safeCalc } from './parser';

const KNOWN_MARKERS = new Set([
  'random', 'calc', 'env', 'alias', 'ref', 'inherit', 'i18n', 'secret', 'default',
  'unique', 'include', 'import', 'geo', 'template', 'split', 'join',
  'clamp', 'round', 'map', 'format', 'fallback', 'once', 'version', 'watch',
]);

const KNOWN_CONSTRAINTS = new Set([
  'min', 'max', 'type', 'required', 'readonly', 'pattern', 'enum',
]);

const KNOWN_TYPES = new Set(['int', 'float', 'bool', 'string', 'random', 'random:int', 'random:float', 'random:bool']);

const DELIM_KEYWORDS = new Set(['space', 'pipe', 'dash', 'dot', 'semi', 'tab', 'slash']);

export function createDiagnostics(context: vscode.ExtensionContext): vscode.DiagnosticCollection {
  const collection = vscode.languages.createDiagnosticCollection('synx');
  context.subscriptions.push(collection);

  const validate = (doc: vscode.TextDocument) => {
    if (doc.languageId === 'synx') {
      runValidation(doc, collection);
    }
  };

  if (vscode.window.activeTextEditor?.document.languageId === 'synx') {
    validate(vscode.window.activeTextEditor.document);
  }

  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument(e => validate(e.document)),
    vscode.workspace.onDidOpenTextDocument(validate),
    vscode.workspace.onDidCloseTextDocument(doc => collection.delete(doc.uri)),
  );

  return collection;
}

function runValidation(doc: vscode.TextDocument, collection: vscode.DiagnosticCollection): void {
  const diagnostics: vscode.Diagnostic[] = [];
  const text = doc.getText();
  const lines = text.split(/\r?\n/);
  const parsed = parseSynx(text);

  // ── Tab check ──
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].startsWith('\t') || lines[i].includes('\t')) {
      const col = lines[i].indexOf('\t');
      diagnostics.push(mkDiag(i, col, col + 1, 'Use spaces for indentation, not tabs', vscode.DiagnosticSeverity.Error));
    }
  }

  // ── Key-level checks ──
  const keysByScope = new Map<string, Map<string, number>>();

  for (const node of parsed.allNodes) {
    if (node.isListItem) {
      // Each list item starts a new scope for its children —
      // clear tracked keys at deeper indent levels so the next item's
      // sub-keys are not reported as duplicates.
      for (const [sk] of keysByScope) {
        const colonIdx = sk.indexOf(':');
        if (parseInt(sk.substring(0, colonIdx)) > node.indent) {
          keysByScope.delete(sk);
        }
      }
      continue;
    }
    const raw = lines[node.line];
    const trimmed = raw.trim();
    if (!trimmed) continue;

    // Indentation check
    if (node.indent % 2 !== 0) {
      diagnostics.push(mkDiag(node.line, 0, node.indent, 'Indentation must be a multiple of 2 spaces', vscode.DiagnosticSeverity.Warning));
    }

    // Key starts with forbidden character
    if (/^[-#/!]/.test(node.key)) {
      diagnostics.push(mkDiag(node.line, node.column, node.column + node.key.length,
        `Key cannot start with '${node.key[0]}'`, vscode.DiagnosticSeverity.Error));
    }

    // Duplicate key at same scope
    const scopeKey = `${node.indent}:${getParentPath(node)}`;
    if (!keysByScope.has(scopeKey)) keysByScope.set(scopeKey, new Map());
    const scope = keysByScope.get(scopeKey)!;
    if (scope.has(node.key)) {
      diagnostics.push(mkDiag(node.line, node.column, node.column + node.key.length,
        `Duplicate key "${node.key}" at this level`, vscode.DiagnosticSeverity.Warning));
    }
    scope.set(node.key, node.line);

    // Type hint
    if (node.typeHint && !KNOWN_TYPES.has(node.typeHint)) {
      const pos = raw.indexOf(`(${node.typeHint})`);
      if (pos !== -1) {
        diagnostics.push(mkDiag(node.line, pos, pos + node.typeHint.length + 2,
          `Unknown type cast "(${node.typeHint})". Use: int, float, bool, string, random, random:int, random:float, random:bool`, vscode.DiagnosticSeverity.Error));
      }
    }

    // ── Markers ──
    if (node.markers.length > 0) {
      for (const marker of node.markers) {
        if (DELIM_KEYWORDS.has(marker) || /^\d+/.test(marker)) continue;

        if (!KNOWN_MARKERS.has(marker)) {
          const mpos = raw.indexOf(':' + marker);
          if (mpos !== -1) {
            diagnostics.push(mkDiag(node.line, mpos, mpos + marker.length + 1,
              `Unknown marker ":${marker}"`, vscode.DiagnosticSeverity.Warning));
          }
        }
      }

      // Markers in static mode
      if (parsed.mode === 'static' && node.markers.some(m => KNOWN_MARKERS.has(m))) {
        const colonPos = raw.indexOf(':');
        if (colonPos !== -1) {
          diagnostics.push(mkDiag(node.line, colonPos, colonPos + node.markers.join(':').length + 1,
            'Markers require "!active" on line 1 to be resolved', vscode.DiagnosticSeverity.Information));
        }
      }

      // ── :alias → check reference exists (root or sibling scope) ──
      if (node.markers.includes('alias') && node.rawValue && parsed.mode === 'active') {
        const ref = node.rawValue.trim();
        const parentPath = getParentPath(node);
        const siblingPath = parentPath ? `${parentPath}.${ref}` : ref;
        if (!parsed.keyMap.has(ref) && !parsed.keyMap.has(siblingPath)) {
          const vpos = raw.lastIndexOf(ref);
          if (vpos !== -1) {
            diagnostics.push(mkDiag(node.line, vpos, vpos + ref.length,
              `Key "${ref}" is not defined`, vscode.DiagnosticSeverity.Error));
          }
        }
      }

      // ── :calc → check variable references ──
      if (node.markers.includes('calc') && node.rawValue && parsed.mode === 'active') {
        const expr = node.rawValue;
        const identifiers = expr.match(/[a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)*/g) || [];
        for (const id of identifiers) {
          if (parsed.keyMap.has(id)) {
            continue;
          }

          // Dot paths are always absolute; plain keys can be resolved in parent scope.
          const parentPath = getParentPath(node);
          const fullPath = parentPath && !id.includes('.') ? `${parentPath}.${id}` : id;
          if (!parsed.keyMap.has(fullPath) && !parsed.keyMap.has(id)) {
            const vpos = raw.indexOf(id, raw.indexOf(expr));
            if (vpos !== -1) {
              diagnostics.push(mkDiag(node.line, vpos, vpos + id.length,
                `Undefined key "${id}" in calc expression`, vscode.DiagnosticSeverity.Warning));
            }
          }
        }
      }

      // ── :include / :import → check file exists ──
      const includePath = getFileMarkerArg(node, ['include', 'import']);
      if (includePath && parsed.mode === 'active') {
        const docDir = path.dirname(doc.uri.fsPath);
        const resolved = path.resolve(docDir, includePath);
        if (!fs.existsSync(resolved)) {
          const vpos = raw.lastIndexOf(includePath);
          if (vpos !== -1) {
            diagnostics.push(mkDiag(node.line, vpos, vpos + includePath.length,
              `File "${includePath}" not found`, vscode.DiagnosticSeverity.Error));
          }
        }
      }

      // ── :inherit → check all parent references exist ──
      if (node.markers.includes('inherit') && parsed.mode === 'active') {
        const refs = getInheritRefs(node);
        const parentPath = getParentPath(node);
        for (const ref of refs) {
          const siblingPath = parentPath ? `${parentPath}.${ref}` : ref;
          if (!parsed.keyMap.has(ref) && !parsed.keyMap.has(siblingPath)) {
            const vpos = raw.lastIndexOf(ref);
            if (vpos !== -1) {
              diagnostics.push(mkDiag(node.line, vpos, vpos + ref.length,
                `Key "${ref}" used in :inherit is not defined`, vscode.DiagnosticSeverity.Error));
            }
          }

          if (ref === node.key || ref === node.dotPath) {
            const vpos = raw.lastIndexOf(ref);
            if (vpos !== -1) {
              diagnostics.push(mkDiag(node.line, vpos, vpos + ref.length,
                'Self-inheritance is not allowed', vscode.DiagnosticSeverity.Error));
            }
          }
        }
      }

      // ── :i18n:COUNT_FIELD → validate count field and plural maps ──
      if (node.markers.includes('i18n') && parsed.mode === 'active') {
        const countField = getMarkerSingleArg(node, 'i18n');
        if (countField) {
          const parentPath = getParentPath(node);
          const siblingPath = parentPath ? `${parentPath}.${countField}` : countField;
          if (!parsed.keyMap.has(countField) && !parsed.keyMap.has(siblingPath)) {
            const vpos = raw.lastIndexOf(countField);
            if (vpos !== -1) {
              diagnostics.push(mkDiag(node.line, vpos, vpos + countField.length,
                `Count field "${countField}" used in :i18n is not defined`, vscode.DiagnosticSeverity.Warning));
            }
          }

          for (const langNode of node.children) {
            if (langNode.children.length === 0) continue;
            const hasOther = langNode.children.some(c => c.key === 'other');
            if (!hasOther) {
              diagnostics.push(mkDiag(langNode.line, langNode.column, langNode.column + langNode.key.length,
                `Plural map for "${langNode.key}" should include "other"`, vscode.DiagnosticSeverity.Warning));
            }
          }
        }
      }

      // ── :template → check placeholders ──
      if (node.markers.includes('template') && node.rawValue && parsed.mode === 'active') {
        const placeholders = node.rawValue.match(/\{(\w+(?:\.\w+)*)\}/g) || [];
        for (const ph of placeholders) {
          const ref = ph.slice(1, -1);
          if (!parsed.keyMap.has(ref)) {
            const vpos = raw.indexOf(ph);
            if (vpos !== -1) {
              diagnostics.push(mkDiag(node.line, vpos, vpos + ph.length,
                `Key "${ref}" referenced in template is not defined`, vscode.DiagnosticSeverity.Warning));
            }
          }
        }
      }
    }

    // ── Constraints ──
    if (node.constraints) {
      if (parsed.mode !== 'active') {
        const bpos = raw.indexOf('[');
        if (bpos !== -1) {
          diagnostics.push(mkDiag(node.line, bpos, bpos + node.constraints.length + 2,
            'Constraints require "!active" mode', vscode.DiagnosticSeverity.Information));
        }
      }

      const parts = node.constraints.split(',');
      for (const part of parts) {
        const p = part.trim();
        if (!p) continue;
        const colonIdx = p.indexOf(':');
        const name = colonIdx !== -1 ? p.substring(0, colonIdx).trim() : p;

        if (!KNOWN_CONSTRAINTS.has(name)) {
          const cpos = raw.indexOf(p);
          if (cpos !== -1) {
            diagnostics.push(mkDiag(node.line, cpos, cpos + p.length,
              `Unknown constraint "${name}"`, vscode.DiagnosticSeverity.Warning));
          }
        }

        // Validate min/max value
        if ((name === 'min' || name === 'max') && colonIdx !== -1) {
          const numStr = p.substring(colonIdx + 1).trim();
          if (numStr && isNaN(Number(numStr))) {
            const cpos = raw.indexOf(p);
            if (cpos !== -1) {
              diagnostics.push(mkDiag(node.line, cpos, cpos + p.length,
                `"${name}" constraint requires a number`, vscode.DiagnosticSeverity.Error));
            }
          }
        }

        // Validate enum values
        if (name === 'enum' && colonIdx !== -1 && parsed.mode === 'active') {
          const allowed = p.substring(colonIdx + 1).split('|').map(s => s.trim());
          if (node.rawValue && allowed.length > 0 && !allowed.includes(node.rawValue)) {
            const vpos = raw.lastIndexOf(node.rawValue);
            if (vpos !== -1) {
              diagnostics.push(mkDiag(node.line, vpos, vpos + node.rawValue.length,
                `Value "${node.rawValue}" not in enum [${allowed.join(', ')}]`, vscode.DiagnosticSeverity.Error));
            }
          }
        }
      }
    }
  }

  collection.set(doc.uri, diagnostics);
}

function getParentPath(node: SynxNode): string {
  const lastDot = node.dotPath.lastIndexOf('.');
  return lastDot !== -1 ? node.dotPath.substring(0, lastDot) : '';
}

function mkDiag(line: number, startCol: number, endCol: number, msg: string, severity: vscode.DiagnosticSeverity): vscode.Diagnostic {
  return new vscode.Diagnostic(
    new vscode.Range(line, startCol, line, endCol),
    msg,
    severity,
  );
}

function getMarkerSingleArg(node: SynxNode, marker: string): string {
  const idx = node.markers.indexOf(marker);
  if (idx === -1) return '';
  const inlineArg = node.markers[idx + 1] ?? '';
  if (inlineArg && !KNOWN_MARKERS.has(inlineArg)) return inlineArg;
  const rawArg = node.rawValue?.trim() ?? '';
  return rawArg;
}

function getFileMarkerArg(node: SynxNode, markerNames: string[]): string {
  for (const marker of markerNames) {
    const idx = node.markers.indexOf(marker);
    if (idx === -1) continue;
    const inlineArg = node.markers[idx + 1] ?? '';
    if (inlineArg && !KNOWN_MARKERS.has(inlineArg)) return inlineArg;
    if (node.rawValue?.trim()) return node.rawValue.trim();
  }
  return '';
}

function getInheritRefs(node: SynxNode): string[] {
  const idx = node.markers.indexOf('inherit');
  if (idx === -1) return [];

  const refs: string[] = [];
  for (const token of node.markers.slice(idx + 1)) {
    if (!token || KNOWN_MARKERS.has(token)) break;
    refs.push(token);
  }

  if (refs.length > 0) return refs;

  const raw = node.rawValue?.trim() ?? '';
  if (!raw) return [];
  return raw.split(/\s+/).filter(Boolean);
}
