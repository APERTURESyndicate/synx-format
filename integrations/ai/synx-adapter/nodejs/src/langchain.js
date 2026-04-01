import { HumanMessage, SystemMessage } from '@langchain/core/messages';
import { packForLlm } from './index.js';

/**
 * @param {import('@aperturesyndicate/synx-format').SynxObject | string} data
 * @param {{ label?: string; wrapFence?: boolean; active?: boolean }} [options]
 */
export function synxSystemMessage(data, options = {}) {
  const label = options.label ?? 'Structured context';
  return new SystemMessage({ content: packForLlm(data, { ...options, label }) });
}

/**
 * @param {import('@aperturesyndicate/synx-format').SynxObject | string} data
 * @param {{ label?: string; wrapFence?: boolean; active?: boolean }} [options]
 */
export function synxHumanMessage(data, options = {}) {
  const label = options.label ?? 'Context';
  return new HumanMessage({ content: packForLlm(data, { ...options, label }) });
}
