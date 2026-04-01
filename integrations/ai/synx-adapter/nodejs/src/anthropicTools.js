import { Synx } from '@aperturesyndicate/synx-format';
import { packForLlm } from './index.js';

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
 * Parse model tool input: JSON first, then SYNX.
 * @param {string} text
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
  return Synx.parse(t);
}
