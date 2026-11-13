/**
 * Checks on text that crosses the model boundary. No imports: usable and
 * testable without the SYNX package installed.
 */

/** A regex character class body for the given code points. */
function charClass(codePoints) {
  return codePoints.map((cp) => `\\u{${cp.toString(16)}}`).join('');
}

// LF, CR, NEL, LINE SEPARATOR, PARAGRAPH SEPARATOR
const LINE_BREAK = new RegExp(`[${charClass([0x0a, 0x0d, 0x85, 0x2028, 0x2029])}]`, 'u');
// Everything `\s` matches, plus NEL, ZERO WIDTH SPACE, WORD JOINER and the BOM
const BLANK = new RegExp(`[\\s${charClass([0x85, 0x200b, 0x2060, 0xfeff])}]`, 'gu');

/**
 * Whether any line of `text` would switch a SYNX parser into active mode.
 *
 * safety: deliberately wider than what either engine accepts. The JS parser
 * trims the BOM around `!active`, the Rust one trims NEL, and both read the
 * legacy `#!mode:` pragma; a check written for one of them was bypassed
 * through the other (`!active` + NBSP, `#!mode: active`). Every blank
 * character is dropped from the line before it is compared.
 */
export function requestsActiveMode(text) {
  for (const line of text.split(LINE_BREAK)) {
    const bare = line.replace(BLANK, '');
    if (bare === '!active' || bare.startsWith('#!mode:')) return true;
  }
  return false;
}

/**
 * A markdown fence no run of backticks in `text` can close.
 *
 * safety: the block was always opened and closed with three backticks, so a
 * tool result holding three backticks ended the block and the rest of the
 * data read as prompt text.
 */
export function fenceFor(text) {
  let longest = 0;
  for (const run of text.match(/`+/g) ?? []) {
    if (run.length > longest) longest = run.length;
  }
  return '`'.repeat(Math.max(3, longest + 1));
}

/** `label (SYNX):` followed by `text` in a fenced `synx` block. */
export function fencedSynxBlock(text, label) {
  const trimmed = text.trim();
  const fence = fenceFor(trimmed);
  return `${label} (SYNX):\n${fence}synx\n${trimmed}\n${fence}`;
}
