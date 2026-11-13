#!/usr/bin/env node
/**
 * SYNX MCP server — stdio transport, tools backed by the `synx` Rust CLI.
 * Set SYNX_CLI to full path of the binary if `synx` is not on PATH.
 */
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { z } from 'zod';
import {
  applyPatch,
  formatText,
  parseJson,
  readPath,
  validatePath,
  validateText,
  writePath,
} from './tools.js';

const AGENT_SNIPPET = `SYNX — quick rules for tools
- Static: \`key value\` per line; indent with 2 spaces for nesting; lists use \`- item\`.
- First-line directives: \`!active\`, \`!lock\`, \`!tool\`, \`!schema\`; \`!include path [alias]\`; block comments \`###\`.
- Active markers use \`:marker\` on keys (e.g. \`:calc\`, \`:env\`, \`:ref\`) — only after \`!active\`.
- Type cast: \`key(int) 42\`; constraints: \`key[required,min:1]\`.
- Block string: value \`|\` then indented lines.
- For JSON interchange, use synx_parse_json. Validate before edits with synx_validate_*.
Full spec: docs/spec/SPECIFICATION_EN.md in the synx-format repo.
`;

/** A tool result holding one JSON document. */
function reply(result, isPretty = true) {
  return {
    content: [
      {
        type: 'text',
        text: isPretty ? JSON.stringify(result, null, 2) : JSON.stringify(result),
      },
    ],
  };
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
      'Validate a .synx file under SYNX_MCP_ROOT[S] using the synx CLI (exit semantics).',
    inputSchema: {
      path: z.string().describe('Absolute file path; must lie under SYNX_MCP_ROOT or SYNX_MCP_ROOTS'),
      strict: z
        .boolean()
        .optional()
        .describe('Treat resolver warnings (_ERR strings) as errors'),
    },
  },
  async (input) => reply(validatePath(input)),
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
  async (input) => reply(validateText(input)),
);

mcp.registerTool(
  'synx_parse_json',
  {
    description:
      'Parse SYNX text to JSON. An !active document is resolved; :env sees only the variables listed in SYNX_MCP_PASS_ENV.',
    inputSchema: {
      text: z.string(),
      active: z.boolean().optional().describe('Kept for older synx CLI versions, which resolve only with it'),
    },
  },
  async (input) => reply(parseJson(input)),
);

mcp.registerTool(
  'synx_format_text',
  {
    description: 'Return canonical-formatted SYNX (synx format).',
    inputSchema: {
      text: z.string(),
    },
  },
  async (input) => reply(formatText(input)),
);

mcp.registerTool(
  'synx_read_path',
  {
    description:
      'Read a UTF-8 text file under SYNX_MCP_ROOT[S] (sandbox). Use for .synx / configs. Literal :secret values come back as [SECRET].',
    inputSchema: {
      path: z
        .string()
        .describe('Absolute file path; must lie under SYNX_MCP_ROOT or SYNX_MCP_ROOTS'),
    },
  },
  async (input) => {
    const result = readPath(input);
    return reply(result, result.ok);
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
  async (input) => reply(writePath(input), false),
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
  async (input) => {
    const result = applyPatch(input);
    return reply(result, result.ok);
  },
);

const transport = new StdioServerTransport();
await mcp.connect(transport);
