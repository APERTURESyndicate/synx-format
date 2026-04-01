"""CLI: read JSON from stdin, emit SYNX (optionally wrapped for LLM)."""

from __future__ import annotations

import argparse
import json
import sys

from synx_adapter.core import estimate_vs_json, pack_for_llm


def main() -> None:
    p = argparse.ArgumentParser(description="Compress JSON context to SYNX for Claude / LLM prompts.")
    p.add_argument("--label", default="Context", help="Label for ```synx fence")
    p.add_argument("--no-fence", action="store_true", help="Output raw SYNX only")
    p.add_argument("--active", action="store_true", help="Prepend !active when absent")
    p.add_argument(
        "--stats",
        action="store_true",
        help="Print JSON size comparison to stderr (minified JSON vs SYNX)",
    )
    p.add_argument(
        "--xml",
        action="store_true",
        help="Wrap output in <synx_data>… (CDATA by default); complements Claude XML habits",
    )
    p.add_argument(
        "--xml-tag",
        default="synx_data",
        help="XML element name for --xml (default: synx_data)",
    )
    p.add_argument(
        "--xml-no-cdata",
        action="store_true",
        help="Escape text instead of CDATA (fragile if payload contains < or &)",
    )
    p.add_argument(
        "--anchor-index",
        action="store_true",
        help="Prepend # @anchor lines listing top-level keys (dict input only)",
    )
    p.add_argument(
        "--section-anchors",
        action="store_true",
        help="Insert # @anchor before each root-level key line in the SYNX body",
    )
    p.add_argument(
        "--anchor-prefix",
        default="# @anchor",
        metavar="TEXT",
        help='Comment prefix for anchors (default: "# @anchor")',
    )
    args = p.parse_args()

    data = json.load(sys.stdin)
    if args.stats:
        s = estimate_vs_json(data)
        print(json.dumps(s, indent=2), file=sys.stderr)

    out = pack_for_llm(
        data,
        label=args.label,
        wrap_fence=not args.no_fence,
        active=args.active,
        wrap_xml=args.xml,
        xml_tag=args.xml_tag,
        xml_cdata=not args.xml_no_cdata,
        anchor_index=args.anchor_index,
        section_anchors=args.section_anchors,
        anchor_prefix=args.anchor_prefix,
    )
    sys.stdout.write(out)
    if not out.endswith("\n"):
        sys.stdout.write("\n")


if __name__ == "__main__":
    main()
