import { Synx } from '@aperturesyndicate/synx-format';
import { packForLlm } from './index.js';
import { requestsActiveMode } from './guards.js';

/**
 * Plain SYNX text for an Anthropic tool_result block.
 * @param {import('@aperturesyndicate/synx-format').SynxObject | string} data
 * @param {{
 *   wrapLikePrompt?: boolean;
 *   active?: boolean;
 *   label?: string;
 *   wrapFence?: boolean;
 *   wrapXml?: boolean;
 *   xmlTag?: string;
 *   xmlCdata?: boolean;
 *   anchorIndex?: boolean;
 *   sectionAnchors?: boolean;
 *   anchorPrefix?: string;
 * }} [opts]
 */
export function toolResultSynxBody(data, opts = {}) {
  if (opts.wrapLikePrompt) {
    return packForLlm(data, {
      label: opts.label ?? 'Tool result',
      wrapFence: opts.wrapFence === true,
      wrapXml: opts.wrapXml === true,
      xmlTag: opts.xmlTag,
      xmlCdata: opts.xmlCdata,
      active: opts.active === true,
      anchorIndex: opts.anchorIndex === true,
      sectionAnchors: opts.sectionAnchors === true,
      anchorPrefix: opts.anchorPrefix,
    });
  }
  const active = opts.active === true;
  return typeof data === 'string' ? data : Synx.stringify(data, active);
}

/**
 * Parse model tool input: JSON first, then SYNX — as static data.
 *
 * safety: this text is written by the model. `Synx.parse` resolves an
 * `!active` document, so `q:env OPENAI_API_KEY` or `q:include ./secrets.synx`
 * pulled the host's environment and files into tool arguments (the Python
 * twin parses statically and never did). Where the installed synx-format has
 * `Synx.parseStatic` it is used; otherwise a document that asks for active
 * mode is refused, and anything else is static by the parser's own rule.
 *
 * @param {string} text
 * @throws {Error} when the text asks for active mode and cannot be parsed statically
 */
export function toolInputFromText(text) {
  const t = text.trim();
  if (!t) return null;
  if (t.startsWith('{') || t.startsWith('[')) {
    try {
      return JSON.parse(t);
    } catch {
      /* fall through */
    }
  }
  if (typeof Synx.parseStatic === 'function') return Synx.parseStatic(t);
  if (requestsActiveMode(t)) {
    throw new Error('SYNX tool input must be static data: `!active` and `#!mode:` are not accepted');
  }
  return Synx.parse(t);
}
