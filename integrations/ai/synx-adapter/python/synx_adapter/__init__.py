"""SYNX-Adapter: structured context → SYNX text for LLM prompts."""

from __future__ import annotations

from synx_adapter.core import (
    estimate_vs_json,
    inject_section_anchors,
    make_anchor_index,
    pack_for_llm,
    to_synx_string,
    wrap_synx_in_xml,
)

__all__ = [
    "pack_for_llm",
    "to_synx_string",
    "estimate_vs_json",
    "wrap_synx_in_xml",
    "make_anchor_index",
    "inject_section_anchors",
]
