#!/usr/bin/env node
import fs from 'node:fs';
import { packForLlm, estimateVsJson } from './index.js';

function parseArgs(argv) {
  const o = {
    label: 'Context',
    fence: true,
    active: false,
    stats: false,
    xml: false,
    xmlTag: 'synx_data',
    xmlCdata: true,
    anchorIndex: false,
    sectionAnchors: false,
    anchorPrefix: '# @anchor',
  };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--no-fence') o.fence = false;
    else if (a === '--active') o.active = true;
    else if (a === '--stats') o.stats = true;
    else if (a === '--xml') o.xml = true;
    else if (a === '--xml-no-cdata') o.xmlCdata = false;
    else if (a === '--xml-tag' && argv[i + 1]) o.xmlTag = argv[++i];
    else if (a === '--anchor-index') o.anchorIndex = true;
    else if (a === '--section-anchors') o.sectionAnchors = true;
    else if (a === '--anchor-prefix' && argv[i + 1]) o.anchorPrefix = argv[++i];
    else if (a === '--label' && argv[i + 1]) o.label = argv[++i];
    else if (a === '-h' || a === '--help') {
      console.error(
        `Usage: synx-context [--label L] [--no-fence] [--active] [--stats] [--xml] [--xml-tag NAME] [--xml-no-cdata] < input.json`,
      );
      process.exit(0);
    }
  }
  return o;
}

async function main() {
  const args = parseArgs(process.argv);
  const raw = await fs.promises.readFile(0, 'utf8');
  const data = JSON.parse(raw);
  if (args.stats) {
    console.error(JSON.stringify(estimateVsJson(data), null, 2));
  }
  const out = packForLlm(data, {
    label: args.label,
    wrapFence: args.fence,
    active: args.active,
    wrapXml: args.xml,
    xmlTag: args.xmlTag,
    xmlCdata: args.xmlCdata,
    anchorIndex: args.anchorIndex,
    sectionAnchors: args.sectionAnchors,
    anchorPrefix: args.anchorPrefix,
  });
  process.stdout.write(out.endsWith('\n') ? out : `${out}\n`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
