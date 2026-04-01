"""Anthropic Messages API helpers: tool results and tool arguments as SYNX (or JSON).

Large tabular tool payloads are often shorter in SYNX than minified JSON; returning SYNX
from ``tool_result`` text blocks can reduce input size on the next model turn.
"""

from __future__ import annotations

import json
from typing import Any

try:
    import synx_native as _synx
except ImportError as e:  # pragma: no cover
    raise ImportError(
        "synx-adapter needs `synx-format` (imports `synx_native`). "
        "pip install synx-format"
    ) from e

from synx_adapter.core import pack_for_llm, to_synx_string


def tool_result_synx_body(
    data: Any,
    *,
    wrap_like_prompt: bool = False,
    active: bool = False,
    anchor_index: bool = False,
    section_anchors: bool = False,
    anchor_prefix: str = "# @anchor",
    wrap_fence: bool = False,
    wrap_xml: bool = False,
    xml_tag: str = "synx_data",
    xml_cdata: bool = True,
    label: str = "Tool result",
) -> str:
    """Plain text for a ``tool_result`` content block: SYNX serialization of ``data``.

    If ``wrap_like_prompt`` is True, uses :func:`pack_for_llm` (fences/XML/anchors) instead of raw SYNX.
    """
    if wrap_like_prompt:
        return pack_for_llm(
            data,
            label=label,
            wrap_fence=wrap_fence,
            active=active,
            wrap_xml=wrap_xml,
            xml_tag=xml_tag,
            xml_cdata=xml_cdata,
            anchor_index=anchor_index,
            section_anchors=section_anchors,
            anchor_prefix=anchor_prefix,
        )
    return to_synx_string(data, active=active)


def tool_input_from_text(text: str) -> Any:
    """Parse model-supplied tool arguments: try JSON first, then SYNX via ``synx_native.parse``."""
    t = text.strip()
    if not t:
        return None
    if t.startswith("{") or t.startswith("["):
        try:
            return json.loads(t)
        except json.JSONDecodeError:
            pass
    return _synx.parse(t)
