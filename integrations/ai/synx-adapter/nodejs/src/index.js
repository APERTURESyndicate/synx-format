import { Synx } from '@aperturesyndicate/synx-format';

const ROOT_KEY_LINE = /^([A-Za-z_][\w.-]*)(?:\s|$)/;

/**
 * Top-level keys as SYNX comment lines (dict-shaped data only).
 * @param {Record<string, unknown>} data
 */
export function makeAnchorIndex(data, prefix = '# @anchor') {
  if (data && typeof data === 'object' && !Array.isArray(data)) {
    const keys = Object.keys(data);
    if (keys.length) return keys.map((k) => `${prefix}: ${k}`).join('\n');
    return `${prefix}: (empty object)`;
  }
  return `${prefix}: (use a dict root for a key index)`;
}

/**
 * Insert anchor comments before each plausible root-level key line.
 * @param {string} synxText
 */
export function injectSectionAnchors(synxText, prefix = '# @anchor') {
  const lines = synxText.split(/\n/);
  const out = [];
  for (const line of lines) {
    if (!line.trim()) {
      out.push(line);
      continue;
    }
    if (line[0] === ' ' || line[0] === '\t') {
      out.push(line);
      continue;
    }
    const st = line.trimStart();
    if (st.startsWith('#') || st.startsWith('//') || st.startsWith('!')) {
      out.push(line);
      continue;
    }
    const m = line.match(ROOT_KEY_LINE);
    if (m) out.push(`${prefix}: ${m[1]}`);
    out.push(line);
  }
  return out.join('\n');
}

/** @param {string} tag */
function sanitizeXmlTag(tag) {
  const t = (tag || '').trim() || 'synx_data';
  if (!/^[A-Za-z_][\w.-]*$/.test(t)) return 'synx_data';
  return t;
}

/** CDATA cannot contain `]]>`. */
function cdataEscape(s) {
  return s.split(']]>').join(']]]]><![CDATA[>');
}

/**
 * Wrap arbitrary prompt text (usually SYNX) in `<tag>…</tag>` for Claude-friendly XML boundaries.
 * @param {string} body
 * @param {{ tag?: string; cdata?: boolean }} [opts]
 */
export function wrapSynxInXml(body, opts = {}) {
  const tag = sanitizeXmlTag(opts.tag ?? 'synx_data');
  const cdata = opts.cdata !== false;
  if (cdata) {
    const inner = cdataEscape(body);
    return `<${tag}><![CDATA[\n${inner}\n]]></${tag}>`;
  }
  const esc = body
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
  return `<${tag}>\n${esc}\n</${tag}>`;
}

/**
 * @param {import('@aperturesyndicate/synx-format').SynxObject | string} data
 * @param {{
 *   label?: string;
 *   wrapFence?: boolean;
 *   active?: boolean;
 *   wrapXml?: boolean;
 *   xmlTag?: string;
 *   xmlCdata?: boolean;
 *   anchorIndex?: boolean;
 *   sectionAnchors?: boolean;
 *   anchorPrefix?: string;
 * }} [options]
 */
export function packForLlm(data, options = {}) {
  const label = options.label ?? 'Context';
  const wrapFence = options.wrapFence !== false;
  const active = options.active === true;
  const wrapXml = options.wrapXml === true;
  const xmlTag = options.xmlTag;
  const xmlCdata = options.xmlCdata !== false;
  const anchorIndex = options.anchorIndex === true;
  const sectionAnchors = options.sectionAnchors === true;
  const anchorPrefix = options.anchorPrefix ?? '# @anchor';

  let text = typeof data === 'string' ? data : Synx.stringify(data, active);
  if (sectionAnchors) text = injectSectionAnchors(text, anchorPrefix);
  if (anchorIndex && typeof data === 'object' && data !== null && !Array.isArray(data) && typeof data !== 'string')
    text = `${makeAnchorIndex(/** @type {Record<string, unknown>} */ (data), anchorPrefix)}\n${text}`;
  let body;
  if (!wrapFence) {
    body = text;
  } else {
    const trimmed = text.trim();
    body = `${label} (SYNX):\n\`\`\`synx\n${trimmed}\n\`\`\``;
  }
  if (wrapXml) {
    body = wrapSynxInXml(body, { tag: xmlTag, cdata: xmlCdata });
  }
  return body;
}

/**
 * @param {import('@aperturesyndicate/synx-format').SynxObject} data
 */
export function estimateVsJson(data) {
  const json = JSON.stringify(data);
  const synx = Synx.stringify(data, false);
  const jc = json.length;
  const sc = synx.length;
  return {
    jsonChars: jc,
    synxChars: sc,
    ratio: sc / Math.max(jc, 1),
    savedChars: Math.max(0, jc - sc),
  };
}
