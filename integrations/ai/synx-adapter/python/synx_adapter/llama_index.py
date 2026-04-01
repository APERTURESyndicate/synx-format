"""LlamaIndex helpers. Install: ``pip install synx-adapter[llama]``."""

from __future__ import annotations

from typing import Any, Sequence

try:
    import llama_index.core  # noqa: F401
except ImportError as e:  # pragma: no cover
    raise ImportError(
        "synx_adapter.llama_index requires llama-index-core. "
        "pip install synx-adapter[llama]"
    ) from e

from synx_adapter.core import pack_for_llm


def _node_text(node: Any) -> str:
    fn = getattr(node, "get_content", None)
    if callable(fn):
        try:
            return fn() or ""
        except Exception:
            pass
    return str(getattr(node, "text", node) or "")


def pack_nodes_synx(
    nodes: Sequence[Any],
    *,
    label: str = "Retrieval",
    max_chars_per_chunk: int = 12_000,
    wrap_fence: bool = True,
) -> str:
    """Turn retrieved nodes into one fenced SYNX document for a prompt."""
    rows: list[dict[str, Any]] = []
    for i, n in enumerate(nodes):
        t = _node_text(n)
        if len(t) > max_chars_per_chunk:
            t = t[:max_chars_per_chunk] + "…"
        rows.append({"i": i, "text": t})
    return pack_for_llm({"chunks": rows}, label=label, wrap_fence=wrap_fence)
