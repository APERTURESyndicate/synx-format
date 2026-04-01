from __future__ import annotations

import json
import re
from typing import Any

try:
    import synx_native as _synx
except ImportError as e:  # pragma: no cover
    raise ImportError(
        "synx-adapter needs `synx-format` (imports `synx_native`). "
        "pip install synx-format"
    ) from e


def to_synx_string(data: Any, *, active: bool = False) -> str:
    """Serialize dict/list/scalars to SYNX; pass str through.

    If ``active`` is True, prepends ``!active`` when not already present.
    """
    if isinstance(data, str):
        text = data
    else:
        text = _synx.stringify(data)

    if active and not text.lstrip().startswith("!active"):
        return "!active\n" + text
    return text


_ROOT_KEY_LINE = re.compile(r"^([A-Za-z_][\w.-]*)(?:\s|$)")


def make_anchor_index(data: Any, *, prefix: str = "# @anchor") -> str:
    """Compact list of top-level keys as SYNX comment lines (long-context landmarks).

    When ``data`` is not a non-empty dict, returns a single comment line explaining why
    no index was generated (still valid SYNX comments).
    """
    if isinstance(data, dict) and data:
        return "\n".join(f"{prefix}: {k}" for k in data.keys())
    if isinstance(data, dict):
        return f"{prefix}: (empty object)"
    return f"{prefix}: (use a dict root for a key index; got {type(data).__name__})"


def inject_section_anchors(synx_text: str, *, prefix: str = "# @anchor") -> str:
    """Insert ``prefix: <rootKey>`` immediately before each plausible root-level key line.

    Heuristic: non-indented lines that look like ``key`` or ``key value`` (not comments / directives).
    Helps models re-locate sections in very long SYNX blobs (mitigate lost-in-the-middle).
    """
    out_lines: list[str] = []
    for line in synx_text.splitlines():
        if not line.strip():
            out_lines.append(line)
            continue
        if line[0] in " \t":
            out_lines.append(line)
            continue
        st = line.lstrip()
        if st.startswith("#") or st.startswith("//") or st.startswith("!"):
            out_lines.append(line)
            continue
        m = _ROOT_KEY_LINE.match(line)
        if m:
            out_lines.append(f"{prefix}: {m.group(1)}")
        out_lines.append(line)
    return "\n".join(out_lines)


def _sanitize_xml_tag(tag: str) -> str:
    t = (tag or "").strip() or "synx_data"
    if not all(c.isalnum() or c in ("_", "-", ".") for c in t) or not (t[0].isalpha() or t[0] == "_"):
        return "synx_data"
    return t


def _cdata_escape(s: str) -> str:
    """CDATA cannot contain `]]>` — split into concatenated CDATA sections."""
    return s.replace("]]>", "]]]]><![CDATA[>")


def wrap_synx_in_xml(body: str, *, tag: str = "synx_data", cdata: bool = True) -> str:
    """Wrap arbitrary text (usually SYNX) in an XML element for Claude-friendly boundaries.

    Complements XML-heavy prompts: the *outer* structure is XML; the *payload* stays SYNX.
    """
    safe = _sanitize_xml_tag(tag)
    if cdata:
        inner = _cdata_escape(body)
        return f"<{safe}><![CDATA[\n{inner}\n]]></{safe}>"
    esc = (
        body.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
    )
    return f"<{safe}>\n{esc}\n</{safe}>"


def pack_for_llm(
    data: Any,
    *,
    label: str = "Context",
    wrap_fence: bool = True,
    active: bool = False,
    wrap_xml: bool = False,
    xml_tag: str = "synx_data",
    xml_cdata: bool = True,
    anchor_index: bool = False,
    section_anchors: bool = False,
    anchor_prefix: str = "# @anchor",
) -> str:
    """Encode data as SYNX; optional anchor comments for long prompts; fence (markdown); optional XML wrap."""
    text = to_synx_string(data, active=active)
    if section_anchors:
        text = inject_section_anchors(text, prefix=anchor_prefix)
    if anchor_index and not isinstance(data, str) and isinstance(data, dict):
        text = make_anchor_index(data, prefix=anchor_prefix) + "\n" + text
    if wrap_fence:
        body = _synx.to_prompt_block(text, label)
    else:
        body = text
    if wrap_xml:
        body = wrap_synx_in_xml(body, tag=xml_tag, cdata=xml_cdata)
    return body


def estimate_vs_json(data: Any) -> dict[str, int | float]:
    """Compare UTF-8 character counts: minified JSON vs SYNX (static stringify)."""
    if isinstance(data, str):
        raise TypeError("estimate_vs_json expects a dict/list/value, not a raw string")

    json_min = json.dumps(data, separators=(",", ":"), ensure_ascii=False)
    synx = _synx.stringify(data)
    jc, sc = len(json_min), len(synx)
    return {
        "json_chars": jc,
        "synx_chars": sc,
        "ratio": round(sc / max(jc, 1), 4),
        "saved_chars": max(0, jc - sc),
    }
