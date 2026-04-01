#!/usr/bin/env python3
"""
Replay corpus files through synx_native — mirrors crates/synx-core/fuzz/fuzz_targets/fuzz_parse.rs:
  valid UTF-8 only, then parse → JSON, stringify, parse_active (resolve), parse_tool (reshape).

Requires: pip install synx-format  OR  pip install -e bindings/python  (from repo root).

Usage:
  python3 synx_fuzz_replay.py file1 [file2 ...]
  python3 synx_fuzz_replay.py --bench file1 [file2 ...]
"""

from __future__ import annotations

import argparse
import sys
import time


def replay_one(s: object, text: str) -> None:
    root = s.parse(text)
    _ = s.parse_to_json(text)
    _ = s.stringify(root)
    _ = s.parse_active(text, None, None)
    _ = s.parse_tool(text)


def main() -> int:
    p = argparse.ArgumentParser(description="SYNX Python binding corpus replay (fuzz_parse parity).")
    p.add_argument("--bench", action="store_true", help="Print per-file timings (ms).")
    p.add_argument("paths", nargs="+", help="Files to read (UTF-8 only; others skipped).")
    args = p.parse_args()

    try:
        import synx_native as s
    except ImportError:
        print(
            "ERROR: synx_native not found. Install: pip install synx-format\n"
            "  or from repo: pip install -e bindings/python",
            file=sys.stderr,
        )
        return 1

    total_ms = 0.0
    ok = 0
    skipped = 0

    for path in args.paths:
        try:
            raw = open(path, "rb").read()
        except OSError as e:
            print(f"MISSING: {path}: {e}", file=sys.stderr)
            return 1

        try:
            text = raw.decode("utf-8", errors="strict")
        except UnicodeDecodeError:
            skipped += 1
            continue

        try:
            t0 = time.perf_counter()
            replay_one(s, text)
            elapsed_ms = (time.perf_counter() - t0) * 1000.0
            if args.bench:
                print(f"{elapsed_ms:8.3f} ms\t{path}")
            total_ms += elapsed_ms
            ok += 1
        except Exception as e:
            print(f"FAIL {path}: {e}", file=sys.stderr)
            return 1

    if args.bench and ok > 0:
        print(
            f"Total: {total_ms:.3f} ms over {ok} file(s); skipped non-UTF8: {skipped}",
            file=sys.stderr,
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
