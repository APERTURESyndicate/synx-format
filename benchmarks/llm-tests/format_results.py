#!/usr/bin/env python3
"""
Format LLM benchmark results as Markdown table with progress bars

Converts JSON results into a pretty README-compatible format
"""

import json
import sys
from typing import List, Dict, Tuple
from pathlib import Path


def create_progress_bar(passed: int, total: int, width: int = 20) -> str:
    """Create a progress bar like: ████████░░░░░░"""
    if total == 0:
        return "░" * width
    
    filled = int((passed / total) * width)
    empty = width - filled
    return "█" * filled + "░" * empty


def format_percentage(passed: int, total: int) -> str:
    """Format percentage"""
    if total == 0:
        return "0.0%"
    return f"{(passed / total * 100):5.1f}%"


def group_results_by_model(results: List[Dict]) -> Dict[str, Dict]:
    """Group results by model name"""
    grouped = {}
    
    for result in results:
        model = result["model"]
        test_type = result["test_type"]
        
        if model not in grouped:
            grouped[model] = {}
        
        grouped[model][test_type] = result
    
    return grouped


def generate_markdown_table(results: List[Dict]) -> str:
    """Generate README-compatible markdown table"""
    grouped = group_results_by_model(results)
    
    output = []
    output.append("## LLM SYNX Format Compatibility\n")
    output.append("How well different LLM models understand SYNX format:\n")
    
    for model_name in sorted(grouped.keys()):
        tests = grouped[model_name]
        output.append(f"\n### {model_name}\n")
        
        # Parse test
        if "parse" in tests:
            parse = tests["parse"]
            bar = create_progress_bar(parse["passed"], parse["total"])
            pct = format_percentage(parse["passed"], parse["total"])
            output.append(
                f"  SYNX Parsing    {bar}  {pct} ({parse['passed']}/{parse['total']})"
            )
        
        # Generate test
        if "generate" in tests:
            gen = tests["generate"]
            bar = create_progress_bar(gen["passed"], gen["total"])
            pct = format_percentage(gen["passed"], gen["total"])
            output.append(
                f"  SYNX Generation {bar}  {pct} ({gen['passed']}/{gen['total']})"
            )
        
        output.append("")
    
    return "\n".join(output)


def generate_compact_table(results: List[Dict]) -> str:
    """Generate compact comparison table"""
    grouped = group_results_by_model(results)
    
    output = []
    output.append("## LLM Compatibility Summary\n")
    
    # Headers
    output.append("| Model | SYNX Parsing | SYNX Generation |")
    output.append("|-------|------|------|")
    
    for model_name in sorted(grouped.keys()):
        tests = grouped[model_name]
        
        parse_str = ""
        if "parse" in tests:
            parse = tests["parse"]
            bar = create_progress_bar(parse["passed"], parse["total"], 15)
            pct = format_percentage(parse["passed"], parse["total"])
            parse_str = f"{bar} {pct}"
        
        gen_str = ""
        if "generate" in tests:
            gen = tests["generate"]
            bar = create_progress_bar(gen["passed"], gen["total"], 15)
            pct = format_percentage(gen["passed"], gen["total"])
            gen_str = f"{bar} {pct}"
        
        output.append(f"| {model_name} | {parse_str} | {gen_str} |")
    
    return "\n".join(output)


def generate_json_stats(results: List[Dict]) -> str:
    """Generate JSON statistics for the results"""
    grouped = group_results_by_model(results)
    
    stats = {
        "timestamp": None,
        "models": {}
    }
    
    for model_name in grouped:
        tests = grouped[model_name]
        model_stats = {}
        
        if "parse" in tests:
            p = tests["parse"]
            model_stats["parse"] = {
                "passed": p["passed"],
                "total": p["total"],
                "percentage": round(p["passed"] / p["total"] * 100, 1) if p["total"] > 0 else 0
            }
        
        if "generate" in tests:
            g = tests["generate"]
            model_stats["generate"] = {
                "passed": g["passed"],
                "total": g["total"],
                "percentage": round(g["passed"] / g["total"] * 100, 1) if g["total"] > 0 else 0
            }
        
        stats["models"][model_name] = model_stats
    
    return json.dumps(stats, indent=2)


def main():
    if len(sys.argv) < 2:
        print("Usage: python format_results.py <results_file.json> [--compact] [--json]")
        sys.exit(1)
    
    results_file = sys.argv[1]
    compact = "--compact" in sys.argv
    json_output = "--json" in sys.argv
    
    if not Path(results_file).exists():
        print(f"Error: {results_file} not found")
        sys.exit(1)
    
    with open(results_file) as f:
        results = json.load(f)
    
    if json_output:
        print(generate_json_stats(results))
    elif compact:
        print(generate_compact_table(results))
    else:
        print(generate_markdown_table(results))


if __name__ == "__main__":
    main()
