import React, { useMemo } from 'react';

type LineKind = 'comment' | 'blank' | 'content';

function lineKind(line: string): LineKind {
  if (!line.trim()) return 'blank';
  const s = line.trimStart();
  if (s.startsWith('#') || s.startsWith('//')) return 'comment';
  return 'content';
}

function depthForLine(line: string, indentStep: number): number {
  const m = line.match(/^(\s*)/);
  const raw = m ? m[1] : '';
  const spaces = raw.replace(/\t/g, '  ').length;
  return Math.floor(spaces / Math.max(1, indentStep));
}

export type SynxVisualizerProps = {
  /** Raw SYNX (or similarly indented key/value text). */
  synx: string;
  /** Spaces per indent level (default 2). */
  indentStep?: number;
};

/**
 * Lightweight indented viewer for SYNX-shaped text (Claude Artifacts-friendly).
 * Uses indentation and comment styling only; does not parse the full SYNX grammar.
 */
export function SynxVisualizer({ synx, indentStep = 2 }: SynxVisualizerProps) {
  const lines = useMemo(() => synx.replace(/\r\n/g, '\n').split('\n'), [synx]);

  return (
    <div
      style={{
        fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace',
        fontSize: 13,
        lineHeight: 1.5,
        padding: '12px 16px',
        background: '#fafafa',
        borderRadius: 8,
        border: '1px solid #e5e7eb',
        maxHeight: 'min(70vh, 640px)',
        overflow: 'auto',
      }}
    >
      {lines.map((line, i) => {
        const kind = lineKind(line);
        const depth = kind === 'content' ? depthForLine(line, indentStep) : 0;
        return (
          <div
            key={i}
            style={{
              paddingLeft: kind === 'content' ? depth * 14 : 0,
              color:
                kind === 'comment'
                  ? '#6b7280'
                  : kind === 'blank'
                    ? 'transparent'
                    : '#111827',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
            }}
          >
            {kind === 'blank' ? '\u00a0' : line}
          </div>
        );
      })}
    </div>
  );
}

export default SynxVisualizer;
