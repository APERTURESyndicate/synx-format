#!/usr/bin/env python3
"""
Update main README.md with LLM compatibility section
Run this after getting new benchmark results
"""

import json
import sys
from pathlib import Path
from format_results import group_results_by_model, create_progress_bar, format_percentage


def update_readme(results_file: str, readme_path: str):
    """Update README with LLM compatibility section"""
    
    # Read benchmark results
    if not Path(results_file).exists():
        print(f"Error: {results_file} not found")
        return False
    
    with open(results_file) as f:
        results = json.load(f)
    
    grouped = group_results_by_model(results)
    
    # Generate content
    content = "## LLM SYNX Format Compatibility\n\n"
    content += "How well different LLM models understand and work with SYNX format:\n\n"
    
    for model_name in sorted(grouped.keys()):
        tests = grouped[model_name]
        
        # Use arrow to mark best model per test type
        arrow_parse = "→ " if "parse" in tests and tests["parse"]["passed"] == tests["parse"]["total"] else "  "
        arrow_gen = "→ " if "generate" in tests and tests["generate"]["passed"] == tests["generate"]["total"] else "  "
        
        content += f"{model_name}\n"
        
        if "parse" in tests:
            p = tests["parse"]
            bar = create_progress_bar(p["passed"], p["total"])
            pct = format_percentage(p["passed"], p["total"])
            content += f"{arrow_parse}SYNX Parsing      {bar}  {pct} ({p['passed']}/{p['total']})\n"
        
        if "generate" in tests:
            g = tests["generate"]
            bar = create_progress_bar(g["passed"], g["total"])
            pct = format_percentage(g["passed"], g["total"])
            content += f"{arrow_gen}SYNX Generation   {bar}  {pct} ({g['passed']}/{g['total']})\n"
        
        content += "\n"
    
    # Read current README
    if not Path(readme_path).exists():
        print(f"Error: {readme_path} not found")
        return False
    
    with open(readme_path, "r") as f:
        readme_text = f.read()
    
    # Find and replace LLM section
    import re
    
    # Pattern to find existing LLM section or insert point
    pattern = r"## LLM SYNX Format Compatibility.*?(?=\n## |\Z)"
    
    if re.search(pattern, readme_text, re.DOTALL):
        # Replace existing section
        readme_text = re.sub(pattern, content.rstrip(), readme_text, flags=re.DOTALL)
    else:
        # Add before the benchmarks section if it exists
        benchmark_pattern = r"(## Performance|## Benchmarks|## Development)"
        if re.search(benchmark_pattern, readme_text):
            readme_text = re.sub(
                benchmark_pattern,
                f"\n{content}\n\\1",
                readme_text
            )
        else:
            # Just append before closing
            readme_text = readme_text.rstrip() + f"\n\n{content}"
    
    # Write back
    with open(readme_path, "w") as f:
        f.write(readme_text)
    
    print(f"✓ Updated {readme_path}")
    return True


if __name__ == "__main__":
    if len(sys.argv) < 2:
        results = "llm_results.json"
    else:
        results = sys.argv[1]
    
    readme = "../README.md"
    
    update_readme(results, readme)
