#!/usr/bin/env node
/**
 * SYNX MCP server — stdio transport, tools backed by the `synx` Rust CLI.
 * Set SYNX_CLI to full path of the binary if `synx` is not on PATH.
 */
import { spawnSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import {
  applyReplacementsOnce,
  atomicWriteText,
  readTextSafe,
  resolveInSandbox,
} from './sandbox.js';

const AGENT_SNIPPET = `SYNX — quick rules for tools
- Static: \`key value\` per line; indent with 2 spaces for nesting; lists use \`- item\`.
- First-line directives: \`!active\`, \`!lock\`, \`!tool\`, \`!schema\`; \`!include path [alias]\`; block comments \`###\`.
- Active markers use \`:marker\` on keys (e.g. \`:calc\`, \`:env\`, \`:ref\`) — only after \`!active\`.
- Type cast: \`key(int) 42\`; constraints: \`key[required,min:1]\`.
- Block string: value \`|\` then indented lines.
- For JSON interchange, use synx_parse_json. Validate before edits with synx_validate_*.
Full spec: docs/spec/SPECIFICATION_EN.md in the synx-format repo.
`;

function synxCommand() {
  return process.env.SYNX_CLI?.trim() || 'synx';
}

function runSynx(args) {
  const cmd = synxCommand();
  const r = spawnSync(cmd, args, {
    encoding: 'utf8',
    maxBuffer: 32 * 1024 * 1024,
    windowsHide: true,
  });
  const err = r.error ? String(r.error.message) : '';
  return {
    ok: r.status === 0,
    code: r.status ?? -1,
    stdout: (r.stdout || '').trimEnd(),
    stderr: (r.stderr || '').trimEnd(),
    err,
  };
}

function withTempSynx(prefix, text, fn) {
  const dir = mkdtempSync(join(tmpdir(), `synx-mcp-${prefix}-`));
  const file = join(dir, 'file.synx');
  try {
    writeFileSync(file, text, 'utf8');
    return fn(file);
  } finally {
    try {
      rmSync(dir, { recursive: true, force: true });
    } catch {
      /* ignore */
    }
  }
}

const mcp = new McpServer({
  name: 'synx-mcp',
  version: '0.1.0',
});

mcp.registerResource(
  'synx_overview',
  'synx://docs/overview',
  {
    title: 'SYNX overview for agents',
    description: 'Short syntax rules for LLM / agent tools',
    mimeType: 'text/markdown',
  },
  async () => ({
    contents: [
      {
        uri: 'synx://docs/overview',
        mimeType: 'text/markdown',
        text: AGENT_SNIPPET,
      },
    ],
  }),
);

mcp.registerTool(
  'synx_validate_path',
  {
    description:
      'Validate a .synx file on disk using the synx CLI (exit semantics).',
    inputSchema: {
      path: z.string().describe('Absolute or workspace path to .synx'),
      strict: z
        .boolean()
        .optional()
        .describe('Treat resolver warnings (_ERR strings) as errors'),
    },
  },
  async ({ path, strict }) => {
    const args = ['validate', path];
    if (strict) args.push('--strict');
    const r = runSynx(args);
    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify(
            {
              ok: r.ok,
              exitCode: r.code,
              stdout: r.stdout,
              stderr: r.stderr || r.err,
              hint: r.err
                ? `Failed to run "${synxCommand()}". Set SYNX_CLI to the binary path.`
                : undefined,
            },
            null,
            2,
          ),
        },
      ],
    };
  },
);

mcp.registerTool(
  'synx_validate_text',
  {
    description: 'Validate SYNX source text (temp file + synx validate).',
    inputSchema: {
      text: z.string().describe('Full .synx document'),
      strict: z.boolean().optional(),
    },
  },
  async ({ text, strict }) => {
    const out = withTempSynx('val', text, (file) => {
      const args = ['validate', file];
      if (strict) args.push('--strict');
      return runSynx(args);
    });
    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify(
            {
              ok: out.ok,
              exitCode: out.code,
              stdout: out.stdout,
              stderr: out.stderr || out.err,
              hint: out.err
                ? `Failed to run "${synxCommand()}". Set SYNX_CLI or install synx-cli.`
                : undefined,
            },
            null,
            2,
          ),
        },
      ],
    };
  },
);

mcp.registerTool(
  'synx_parse_json',
  {
    description: 'Parse SYNX text to JSON (optional !active resolve).',
    inputSchema: {
      text: z.string(),
      active: z.boolean().optional().describe('Resolve !active before returning JSON'),
    },
  },
  async ({ text, active }) => {
    const out = withTempSynx('parse', text, (file) => {
      const args = ['parse', file];
      if (active) args.push('--active');
      return runSynx(args);
    });
    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify(
            {
              ok: out.ok,
              json: out.ok ? out.stdout : null,
              stderr: out.stderr || out.err,
            },
            null,
            2,
          ),
        },
      ],
    };
  },
);

mcp.registerTool(
  'synx_format_text',
  {
    description: 'Return canonical-formatted SYNX (synx format).',
    inputSchema: {
      text: z.string(),
    },
  },
  async ({ text }) => {
    const out = withTempSynx('fmt', text, (file) => runSynx(['format', file]));
    return {
      content: [
        {
          type: 'text',
          text: JSON.stringify(
            {
              ok: out.ok,
              formatted: out.ok ? out.stdout : null,
              stderr: out.stderr || out.err,
            },
            null,
            2,
          ),
        },
      ],
    };
  },
);

mcp.registerTool(
  'synx_read_path',
  {
    description:
      'Read a UTF-8 text file under SYNX_MCP_ROOT[S] (sandbox). Use for .synx / configs.',
    inputSchema: {
      path: z
        .string()
        .describe('Absolute file path; must lie under SYNX_MCP_ROOT or SYNX_MCP_ROOTS'),
    },
  },
  async ({ path: filePath }) => {
    try {
      const abs = resolveInSandbox(filePath);
      const text = readTextSafe(abs);
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify({ ok: true, path: abs, text }, null, 2),
          },
        ],
      };
    } catch (e) {
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify({
              ok: false,
              error: e instanceof Error ? e.message : String(e),
            }),
          },
        ],
      };
    }
  },
);

mcp.registerTool(
  'synx_write_path',
  {
    description:
      'Atomically write UTF-8 text to a file under SYNX_MCP_ROOT[S]. Creates parent dirs.',
    inputSchema: {
      path: z.string().describe('Absolute file path under sandbox root'),
      text: z.string().describe('Full file contents'),
    },
  },
  async ({ path: filePath, text }) => {
    try {
      const abs = resolveInSandbox(filePath);
      atomicWriteText(abs, text);
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify({ ok: true, path: abs, bytes: Buffer.byteLength(text, 'utf8') }),
          },
        ],
      };
    } catch (e) {
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify({
              ok: false,
              error: e instanceof Error ? e.message : String(e),
            }),
          },
        ],
      };
    }
  },
);

mcp.registerTool(
  'synx_apply_patch',
  {
    description:
      'Sequentially replace unique substrings in a sandbox file (safe, no regex). Each find must occur exactly once.',
    inputSchema: {
      path: z.string(),
      replacements: z
        .array(
          z.object({
            find: z.string(),
            replace: z.string(),
          }),
        )
        .min(1),
    },
  },
  async ({ path: filePath, replacements }) => {
    try {
      const abs = resolveInSandbox(filePath);
      const original = readTextSafe(abs);
      const next = applyReplacementsOnce(original, replacements);
      atomicWriteText(abs, next);
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify(
              {
                ok: true,
                path: abs,
                replacementsApplied: replacements.length,
              },
              null,
              2,
            ),
          },
        ],
      };
    } catch (e) {
      return {
        content: [
          {
            type: 'text',
            text: JSON.stringify({
              ok: false,
              error: e instanceof Error ? e.message : String(e),
            }),
          },
        ],
      };
    }
  },
);

const transport = new StdioServerTransport();
await mcp.connect(transport);
