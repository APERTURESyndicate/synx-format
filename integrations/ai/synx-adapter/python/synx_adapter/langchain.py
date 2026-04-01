"""LangChain helpers. Install: ``pip install synx-adapter[langchain]``."""

from __future__ import annotations

from typing import Any

try:
    from langchain_core.messages import HumanMessage, SystemMessage
except ImportError as e:  # pragma: no cover
    raise ImportError(
        "synx_adapter.langchain requires langchain-core. "
        "pip install synx-adapter[langchain]"
    ) from e

from synx_adapter.core import pack_for_llm, to_synx_string


def SynxSystemMessage(
    data: Any,
    *,
    label: str = "Structured context",
    wrap_fence: bool = True,
    active: bool = False,
    wrap_xml: bool = False,
    xml_tag: str = "synx_data",
    xml_cdata: bool = True,
) -> SystemMessage:
    return SystemMessage(
        content=pack_for_llm(
            data,
            label=label,
            wrap_fence=wrap_fence,
            active=active,
            wrap_xml=wrap_xml,
            xml_tag=xml_tag,
            xml_cdata=xml_cdata,
        )
    )


def SynxHumanMessage(
    data: Any,
    *,
    label: str = "Context",
    wrap_fence: bool = True,
    active: bool = False,
    wrap_xml: bool = False,
    xml_tag: str = "synx_data",
    xml_cdata: bool = True,
) -> HumanMessage:
    return HumanMessage(
        content=pack_for_llm(
            data,
            label=label,
            wrap_fence=wrap_fence,
            active=active,
            wrap_xml=wrap_xml,
            xml_tag=xml_tag,
            xml_cdata=xml_cdata,
        )
    )


def synx_tool_payload(data: Any, *, active: bool = False) -> str:
    """SYNX only (no markdown fence) — e.g. for a separate tool-result channel."""
    return to_synx_string(data, active=active)
