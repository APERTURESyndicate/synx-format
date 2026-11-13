"""One probe of the Python binding per process.

    python probe.py <directory with synx_native> <probe>

Prints `ok <value>` or `raised <ExceptionType>`; a probe that kills the
interpreter prints nothing.
"""
import os
import sys
import tempfile

sys.path.insert(0, sys.argv[1])
import synx_native as synx  # noqa: E402


def deep(n):
    v = 1
    for _ in range(n):
        v = [v]
    return v


def cyclic():
    v = {"a": 1}
    v["self"] = v
    return v


def not_utf8():
    path = os.path.join(tempfile.mkdtemp(prefix="synx-py-"), "in.synx")
    with open(path, "wb") as f:
        f.write(b"name caf\xe9\nport 1\n")
    return synx.load(path)


PROBES = {
    "deep": lambda: len(synx.stringify(deep(200000))),
    "cycle": lambda: len(synx.stringify(cyclic())),
    "cycle-diff": lambda: synx.diff(cyclic(), {"a": 2}),
    "int64": lambda: synx.parse("max 9223372036854775807\n")["max"],
    "beyond-int64": lambda: synx.stringify({"a": 2**70}),
    "masked": lambda: synx.parse_active("!active\npw:secret masked-value-7f3a\ncopy:alias pw\nmsg text {pw}\n", {}),
    "not-utf8": not_utf8,
    "wrong-env": lambda: synx.parse_active("!active\nk:env:default:none K\n", {"K": 1}),
}

try:
    print("ok", repr(PROBES[sys.argv[2]]()))
except Exception as e:  # the binding's own refusals
    print("raised", type(e).__name__)
