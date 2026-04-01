/**
 * SYNX Commands — Convert to JSON, JSON → SYNX, Freeze, Preview.
 */

import * as vscode from 'vscode';
import * as path from 'path';
import { parseSynx, resolveToObject, serializeToSynx, jsonToSynx } from './parser';

// ─── Convert SYNX → JSON ────────────────────────────────────────────────────

export function registerConvertToJson(ctx: vscode.ExtensionContext): void {
  ctx.subscriptions.push(
    vscode.commands.registerCommand('synx.convertToJson', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'synx') {
        vscode.window.showWarningMessage('Open a .synx file first');
        return;
      }

      const doc = parseSynx(editor.document.getText());
      const obj = resolveToObject(doc);
      const json = JSON.stringify(obj, null, 2);

      const synxPath = editor.document.uri.fsPath;
      const jsonPath = synxPath.replace(/\.synx$/, '.json');

      const uri = vscode.Uri.file(jsonPath);
      await vscode.workspace.fs.writeFile(uri, Buffer.from(json, 'utf-8'));
      const jsonDoc = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(jsonDoc, { viewColumn: vscode.ViewColumn.Beside });
      vscode.window.showInformationMessage(`Saved: ${path.basename(jsonPath)}`);
    })
  );
}

// ─── Convert JSON → SYNX ────────────────────────────────────────────────────

export function registerConvertFromJson(ctx: vscode.ExtensionContext): void {
  ctx.subscriptions.push(
    vscode.commands.registerCommand('synx.convertFromJson', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'json') {
        vscode.window.showWarningMessage('Open a .json file first');
        return;
      }

      const text = editor.document.getText();
      let synxText: string;
      try {
        synxText = jsonToSynx(text);
      } catch {
        vscode.window.showErrorMessage('Invalid JSON');
        return;
      }

      const jsonPath = editor.document.uri.fsPath;
      const synxPath = jsonPath.replace(/\.json$/, '.synx');

      const uri = vscode.Uri.file(synxPath);
      await vscode.workspace.fs.writeFile(uri, Buffer.from(synxText, 'utf-8'));
      const synxDoc = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(synxDoc, { viewColumn: vscode.ViewColumn.Beside });
      vscode.window.showInformationMessage(`Saved: ${path.basename(synxPath)}`);
    })
  );
}

// ─── Freeze (resolve all markers to static values) ──────────────────────────

export function registerFreeze(ctx: vscode.ExtensionContext): void {
  ctx.subscriptions.push(
    vscode.commands.registerCommand('synx.freeze', async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'synx') {
        vscode.window.showWarningMessage('Open a .synx file first');
        return;
      }

      const doc = parseSynx(editor.document.getText());
      const obj = resolveToObject(doc);
      const frozen = '!static\n' + serializeToSynx(obj, 0);

      const synxPath = editor.document.uri.fsPath;
      const frozenPath = synxPath.replace(/\.synx$/, '.static.synx');

      const uri = vscode.Uri.file(frozenPath);
      await vscode.workspace.fs.writeFile(uri, Buffer.from(frozen, 'utf-8'));
      const frozenDoc = await vscode.workspace.openTextDocument(uri);
      await vscode.window.showTextDocument(frozenDoc, { viewColumn: vscode.ViewColumn.Beside });
      vscode.window.showInformationMessage(`Frozen: ${path.basename(frozenPath)}`);
    })
  );
}

// ─── Preview (side panel with live JSON output) ──────────────────────────────

const previewPanels = new Map<string, vscode.WebviewPanel>();

export function registerPreview(ctx: vscode.ExtensionContext): void {
  ctx.subscriptions.push(
    vscode.commands.registerCommand('synx.preview', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor || editor.document.languageId !== 'synx') {
        vscode.window.showWarningMessage('Open a .synx file first');
        return;
      }

      const key = editor.document.uri.toString();
      const existing = previewPanels.get(key);
      if (existing) {
        existing.reveal(vscode.ViewColumn.Beside);
        updatePreview(existing, editor.document);
        return;
      }

      const panel = vscode.window.createWebviewPanel(
        'synxPreview',
        `Preview: ${path.basename(editor.document.fileName)}`,
        vscode.ViewColumn.Beside,
        { enableScripts: false }
      );

      previewPanels.set(key, panel);
      panel.onDidDispose(() => previewPanels.delete(key));
      updatePreview(panel, editor.document);

      // Live update on text change
      const disposable = vscode.workspace.onDidChangeTextDocument(e => {
        if (e.document.uri.toString() === key) {
          updatePreview(panel, e.document);
        }
      });
      panel.onDidDispose(() => disposable.dispose());
    })
  );
}

function updatePreview(panel: vscode.WebviewPanel, doc: vscode.TextDocument): void {
  try {
    const parsed = parseSynx(doc.getText());
    const obj = resolveToObject(parsed);
    const json = JSON.stringify(obj, null, 2);
    panel.webview.html = getPreviewHtml(json, parsed.mode);
  } catch (err) {
    panel.webview.html = getPreviewHtml(`Error: ${err}`, 'error');
  }
}

function getPreviewHtml(json: string, mode: string): string {
  const escaped = json
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');

  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <style>
    body {
      font-family: var(--vscode-editor-font-family, 'Consolas', monospace);
      font-size: var(--vscode-editor-font-size, 14px);
      color: var(--vscode-editor-foreground);
      background: var(--vscode-editor-background);
      padding: 16px;
      margin: 0;
    }
    .badge {
      display: inline-block;
      padding: 2px 8px;
      border-radius: 4px;
      font-size: 12px;
      margin-bottom: 12px;
      background: var(--vscode-badge-background);
      color: var(--vscode-badge-foreground);
    }
    pre {
      white-space: pre-wrap;
      word-break: break-word;
      margin: 0;
    }
  </style>
</head>
<body>
  <div class="badge">${mode.toUpperCase()}</div>
  <pre>${escaped}</pre>
</body>
</html>`;
}
