"""Checks on text that crosses the model boundary.

No imports beyond the standard library: usable and testable without the native
SYNX module installed.
"""

from __future__ import annotations

import re

_BACKTICK_RUN = re.compile(r"`+")


def fence_for(text: str) -> str:
    """A markdown fence no run of backticks in ``text`` can close.

    safety: the block was always opened and closed with three backticks, so a
    tool result holding three backticks ended the block and the rest of the
    data read as prompt text.
    """
    longest = max((len(run) for run in _BACKTICK_RUN.findall(text)), default=0)
    return "`" * max(3, longest + 1)


def fenced_synx_block(text: str, label: str) -> str:
    """``label (SYNX):`` followed by ``text`` in a fenced ``synx`` block."""
    trimmed = text.strip()
    fence = fence_for(trimmed)
    return f"{label} (SYNX):\n{fence}synx\n{trimmed}\n{fence}"
