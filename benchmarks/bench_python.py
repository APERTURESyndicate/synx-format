"""
SYNX Benchmark — Python
Compares: json.loads · PyYAML · synx_native (Rust/PyO3)

Usage:
    cd benchmarks
    pip install pyyaml
    python bench_python.py
"""

import json
import os
import re
import sys
import time
import platform
import xml.etree.ElementTree as ET

# ── Parsers ──────────────────────────────────────────────────────────────────

try:
    import yaml
except ImportError:
    print("ERROR: pyyaml not installed.  Run: pip install pyyaml")
    sys.exit(1)

# Native Rust SYNX parser (bindings/python → synx_core via PyO3)
_native_path = os.path.join(os.path.dirname(__file__), '..', 'bindings', 'python', 'target', 'release')
sys.path.insert(0, os.path.abspath(_native_path))
try:
    import synx_native as _SynxNative
except ImportError as e:
    print(f"ERROR: synx_native not found in {_native_path}")
    print(f"  {e}")
    print("  Build: cd bindings/python && cargo build --release")
    print("  Then copy synx_native.dll → synx_native.pyd in the release folder.")
    sys.exit(1)

# ── Input data ───────────────────────────────────────────────────────────────

_bench_dir = os.path.dirname(os.path.abspath(__file__))

with open(os.path.join(_bench_dir, 'config.json'),  'r', encoding='utf-8') as f:
    JSON_TEXT = f.read()

with open(os.path.join(_bench_dir, 'config.yaml'),  'r', encoding='utf-8') as f:
    YAML_TEXT = f.read()

with open(os.path.join(_bench_dir, 'config.synx'), 'r', encoding='utf-8') as f:
    SYNX_TEXT = f.read()


def _safe_xml_tag(name: str) -> str:
    s = re.sub(r'[^a-zA-Z0-9_.-]', '_', str(name))
    return s if s else 'node'


def _json_to_xml_element(obj, name: str) -> ET.Element:
    tag = _safe_xml_tag(name)
    if obj is None:
        return ET.Element(tag)
    if isinstance(obj, (str, int, float)):
        el = ET.Element(tag)
        el.text = str(obj)
        return el
    if isinstance(obj, bool):
        el = ET.Element(tag)
        el.text = 'true' if obj else 'false'
        return el
    if isinstance(obj, list):
        root = ET.Element(tag)
        for x in obj:
            root.append(_json_to_xml_element(x, 'item'))
        return root
    root = ET.Element(tag)
    for k, v in obj.items():
        root.append(_json_to_xml_element(v, k))
    return root


_json_obj = json.loads(JSON_TEXT)
XML_TEXT = '<?xml version="1.0" encoding="UTF-8"?>\n' + ET.tostring(
    _json_to_xml_element(_json_obj, 'config'), encoding='unicode'
)

ITERATIONS = 10_000
WARMUP     = 100

# ── Helpers ──────────────────────────────────────────────────────────────────

def bench(label: str, fn) -> dict:
    for _ in range(WARMUP):
        fn()

    start = time.perf_counter_ns()
    for _ in range(ITERATIONS):
        fn()
    elapsed_ns = time.perf_counter_ns() - start

    total_ms  = elapsed_ns / 1_000_000
    per_us    = elapsed_ns / ITERATIONS / 1_000

    return {"label": label, "total_ms": total_ms, "per_us": per_us}

# ── Run ───────────────────────────────────────────────────────────────────────

print("╔══════════════════════════════════════════════════════════════════╗")
print(f"║         SYNX Benchmark — Python  ({ITERATIONS:,} iterations)           ║")
print("╠══════════════════════════════════════════════════════════════════╣")
print(f"║  Input sizes:  JSON {len(JSON_TEXT):<5}b   YAML {len(YAML_TEXT):<5}b   XML {len(XML_TEXT):<5}b   SYNX {len(SYNX_TEXT):<5}b   ║")
print("╚══════════════════════════════════════════════════════════════════╝")
print()

results = [
    bench("json.loads (built-in)",       lambda: json.loads(JSON_TEXT)),
    bench("yaml.safe_load (PyYAML)",     lambda: yaml.safe_load(YAML_TEXT)),
    bench("xml.etree (parse)",           lambda: ET.fromstring(XML_TEXT)),
    bench("synx_native.parse (Rust)",    lambda: _SynxNative.parse(SYNX_TEXT)),
    bench("synx_native.parseToJson",     lambda: _SynxNative.parse_to_json(SYNX_TEXT)),
]

baseline_ms = results[0]["total_ms"]
fastest_ms  = min(r["total_ms"] for r in results)

print(f"  {'Parser':<36} {'Time/call':>12}  {'Total (ms)':>12}  {'vs json':>8}")
print("  " + "─" * 74)

for r in results:
    vs_json = f"{r['total_ms'] / baseline_ms:.2f}x"
    time_str = (
        f"{r['per_us']:.3f} µs" if r['per_us'] < 10 else f"{r['per_us']:.2f} µs"
    )
    marker = " ←fastest" if abs(r["total_ms"] - fastest_ms) < 0.001 else ""
    print(
        f"  {r['label']:<36} {time_str:>12}  {r['total_ms']:>10.1f} ms  {vs_json:>8}{marker}"
    )

print()
print(f"  Python {platform.python_version()} · {platform.system()} {platform.machine()}")
print()

# ── Export results as JSON ────────────────────────────────────────────────────

import json as _json

out_path = os.path.join(_bench_dir, "results_python.json")
with open(out_path, "w", encoding="utf-8") as f:
    _json.dump({
        "platform": platform.system(),
        "arch":     platform.machine(),
        "python":   platform.python_version(),
        "iterations": ITERATIONS,
        "sizes": {
            "json": len(JSON_TEXT),
            "yaml": len(YAML_TEXT),
            "xml": len(XML_TEXT),
            "synx": len(SYNX_TEXT),
        },
        "results": [
            {
                "label":    r["label"],
                "total_ms": round(r["total_ms"], 3),
                "per_us":   round(r["per_us"], 4),
            }
            for r in results
        ],
    }, f, indent=2)

print(f"  Results saved → {out_path}")
