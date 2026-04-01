#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import re
from pathlib import Path


ROW_RE = re.compile(r"^(<tr\b[^>]*>)(.*)(</tr>)$", re.DOTALL)
HREF_RE = re.compile(r"href='([^']+)'")
LINE_COV_RE = re.compile(r"Line Coverage</td>")
ZERO_LINE_RE = re.compile(r">\s*0\.00%\s*\(0/\d+\)\s*<")


def _iter_rows(html: str) -> list[str]:
    # The llvm-cov HTML is minified in a single line; splitting like this is robust enough.
    parts = html.split("<tr")
    if len(parts) == 1:
        return []
    rows = []
    prefix = parts[0]
    for p in parts[1:]:
        row = "<tr" + p
        end = row.find("</tr>")
        if end == -1:
            continue
        rows.append(row[: end + len("</tr>")])
    return rows


def prune_index(index_path: Path, remove_zero_line: bool) -> tuple[str, set[str], int, int]:
    html = index_path.read_text(encoding="utf-8", errors="strict")

    rows = _iter_rows(html)
    if not rows:
        return html, set(), 0, 0

    kept: list[str] = []
    removed_hrefs: set[str] = set()
    kept_count = 0
    removed_count = 0

    for row in rows:
        # Keep header row + totals row unconditionally.
        if "Filename</td>" in row or "Totals</pre>" in row:
            kept.append(row)
            continue

        href_m = HREF_RE.search(row)
        href = href_m.group(1) if href_m else None

        is_zero = False
        if remove_zero_line:
            # Treat a row as "0%" if line coverage shows 0.00% (0/N).
            # This matches the "we didn't use it" interpretation for fuzz coverage.
            # The row contains multiple percentages; line coverage is the 3rd coverage column.
            # We don't fully parse HTML; we just check for the 0.00% token with (0/N).
            is_zero = "0.00% (0/" in row and ZERO_LINE_RE.search(row) is not None

        if is_zero:
            removed_count += 1
            if href:
                removed_hrefs.add(href)
        else:
            kept_count += 1
            kept.append(row)

    # Rebuild by replacing the original rows with the kept ones.
    # Find the first <tr ...> and the last </tr> in the file and replace that block.
    first = html.find("<tr")
    last = html.rfind("</tr>")
    if first == -1 or last == -1 or last < first:
        return html, removed_hrefs, kept_count, removed_count

    rebuilt = html[:first] + "".join(kept) + html[last + len("</tr>") :]
    return rebuilt, removed_hrefs, kept_count, removed_count


def main() -> int:
    ap = argparse.ArgumentParser(description="Prune llvm-cov HTML report rows and pages.")
    ap.add_argument("html_dir", type=Path, help="Path to html/ directory (contains index.html)")
    ap.add_argument("--keep-files", action="store_true", help="Only prune index.html (do not delete pages)")
    args = ap.parse_args()

    html_dir: Path = args.html_dir
    index_path = html_dir / "index.html"
    if not index_path.exists():
        raise SystemExit(f"index.html not found under: {html_dir}")

    rebuilt, removed_hrefs, kept_count, removed_count = prune_index(
        index_path=index_path,
        remove_zero_line=True,
    )
    index_path.write_text(rebuilt, encoding="utf-8")

    removed_files = 0
    if not args.keep_files:
        for href in sorted(removed_hrefs):
            # href is relative to html_dir
            target = (html_dir / href).resolve()
            try:
                # Only delete within the report folder for safety.
                target.relative_to(html_dir.resolve())
            except Exception:
                continue
            if target.is_file():
                target.unlink()
                removed_files += 1

    # Best-effort: remove now-empty directories (depth-first).
    if not args.keep_files:
        for root, dirs, files in os.walk(html_dir, topdown=False):
            if not dirs and not files:
                try:
                    Path(root).rmdir()
                except OSError:
                    pass

    print(
        f"Pruned index.html: removed {removed_count} row(s), kept {kept_count} row(s); "
        f"deleted {removed_files} page(s)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

