/**
 * SYNX VS Code Extension — main entry point.
 * Registers all providers, commands, and diagnostics.
 */

import * as vscode from 'vscode';
import { createDiagnostics } from './diagnostics';
import { createCompletionProvider, createHoverProvider } from './completion';
import {
  createDocumentSymbolProvider,
  createDefinitionProvider,
  createReferenceProvider,
} from './navigation';
import { createFormattingProvider } from './formatter';
import { createColorProvider } from './colors';
import { createInlayHintsProvider } from './inlay-hints';
import { activateNestingColors } from './nesting-colors';
import { activateCommentStyling } from './comment-styles';
import {
  registerConvertToJson,
  registerConvertFromJson,
  registerFreeze,
  registerPreview,
} from './commands';

export function activate(ctx: vscode.ExtensionContext): void {
  // ─── Diagnostics ─────────────────────────────────────────────────────────
  ctx.subscriptions.push(createDiagnostics(ctx));

  // ─── Completion + Hover ──────────────────────────────────────────────────
  ctx.subscriptions.push(createCompletionProvider());
  ctx.subscriptions.push(createHoverProvider());

  // ─── Navigation (Symbols, GoTo, References) ─────────────────────────────
  ctx.subscriptions.push(createDocumentSymbolProvider());
  ctx.subscriptions.push(createDefinitionProvider());
  ctx.subscriptions.push(createReferenceProvider());

  // ─── Formatter ───────────────────────────────────────────────────────────
  ctx.subscriptions.push(createFormattingProvider());

  // ─── Color Provider ──────────────────────────────────────────────────────
  ctx.subscriptions.push(createColorProvider());

  // ─── Inlay Hints ─────────────────────────────────────────────────────────
  ctx.subscriptions.push(createInlayHintsProvider());

  // ─── Nesting Colors ────────────────────────────────────────────────────
  activateNestingColors(ctx);

  // ─── Comment Styling ───────────────────────────────────────────────────
  activateCommentStyling(ctx);

  // ─── Commands ────────────────────────────────────────────────────────────
  registerConvertToJson(ctx);
  registerConvertFromJson(ctx);
  registerFreeze(ctx);
  registerPreview(ctx);
}

export function deactivate(): void {}
