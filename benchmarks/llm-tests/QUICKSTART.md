# LLM SYNX Benchmark System - Quick Start Guide

## What is this?

A complete system to test how well different AI models (Claude, Gemini, GPT) understand the SYNX configuration format.

## Files Created

```
benchmarks/llm-tests/
├── README.md                      # Quick reference
├── LLM_BENCHMARK_GUIDE.md         # Detailed guide (in parent dir)
├── requirements.txt               # Python dependencies
├── benchmark.bat                  # Windows batch runner
├── test_cases.py                  # 250 test cases (125 parsing + 125 generation)
├── llm_benchmark.py              # Main benchmark runner with LLM API clients
├── format_results.py              # Pretty-print results with progress bars
├── update_readme.py               # Auto-update main README
├── example_results.json           # Example output
└── parsers/                       # (empty, for future custom parsers)
```

## Quick Start (5 minutes)

### Step 1: Install Python dependencies
```bash
cd benchmarks/llm-tests
pip install -r requirements.txt
```

### Step 2: Set your API keys
```bash
# Bash/Zsh
export ANTHROPIC_API_KEY=your_key_here
export GOOGLE_API_KEY=your_key_here
export OPENAI_API_KEY=your_key_here

# PowerShell
$env:ANTHROPIC_API_KEY = "your_key_here"
$env:GOOGLE_API_KEY = "your_key_here"
$env:OPENAI_API_KEY = "your_key_here"

# Windows Command Prompt
set ANTHROPIC_API_KEY=your_key_here
set GOOGLE_API_KEY=your_key_here
set OPENAI_API_KEY=your_key_here
```

### Step 3: Run tests
```bash
# Test all available models
python llm_benchmark.py

# Or test specific models
python llm_benchmark.py --models claude-opus,gemini-2.0-flash,gpt-4o

# Quick test with just 5 examples
python llm_benchmark.py --limit 5
```

### Step 4: View results
```bash
# Pretty print with progress bars
python format_results.py llm_results.json

# Save to README
python update_readme.py llm_results.json
```

## Expected Output

```
## LLM SYNX Format Compatibility

How well different LLM models understand SYNX format:

### gemini-2.0-flash

  SYNX Parsing    ████████████████████  100.0% (20/20)
  SYNX Generation ████████████████████  100.0% (8/8)

### claude-opus

  SYNX Parsing    ███████████████████░   95.0% (19/20)
  SYNX Generation █████████████████░░░   87.5% (7/8)

...
```

## What Gets Tested?

### Parsing (20 tests)
- Can the model read SYNX and output valid JSON?
- Tests: simple pairs, nested objects, arrays, types, comments, etc.

### Generation (8 tests)
- Can the model create SYNX from an English description?
- Tests: configs, arrays, nesting, type inference, etc.

## Example Tests

### Parsing Example
```
Input SYNX:
  user
    name Alice
    age 28

Expected JSON:
  {"user": {"name": "Alice", "age": 28}}
```

### Generation Example  
```
Input Description:
  "Create a database config with host localhost and port 5432"

Expected Output (must contain):
  - "database" or "config"
  - "host" and "localhost"
  - "port" and "5432"
```

## Command Reference

### Main Benchmark Runner
```bash
python llm_benchmark.py [OPTIONS]

Options:
  --models MODEL1,MODEL2,...  Which models to test (default: all available)
  --test-type parse|generate|both  Which tests to run (default: both)
  --limit N                   Test only first N examples (for testing)
  --output FILE              Save results to JSON (default: llm_results.json)

Available models:
  - claude-opus
  - claude-sonnet
  - claude-haiku-4-5
  - gemini-2.0-flash
  - gemini-1.5-pro
  - gemini-1.5-flash
  - gpt-4o
  - gpt-4-turbo
  - gpt-4
```

### Results Formatter
```bash
python format_results.py [RESULTS_FILE] [FORMAT]

Format options:
  (default)    # Markdown with progress bars
  --compact    # Table format
  --json       # Statistics JSON
```

### README Updater
```bash
python update_readme.py [RESULTS_FILE]
# Automatically updates ../README.md with results
```

## Cost Estimate

| Scenario | Approx Cost |
|----------|------------|
| Test 1 model | $0.001-0.04 |
| Test 3 models | $0.10-0.20 |
| Test 6 models (all) | $0.30-0.50 |
| Weekly full run (1 model) | $0.01-0.30 |
| Monthly full run (all models) | $0.30-1.50 |

*Free tier available: Gemini 2.0 Flash has generous free tier*

## Common Tasks

### Quick validation (under 1 minute)
```bash
python llm_benchmark.py --limit 3
```

### Test only one model
```bash
python llm_benchmark.py --models claude-opus --output claude_opus_results.json
```

### Compare new version with old
```bash
# Run with current models
python llm_benchmark.py --output new_results.json

# Compare
diff <(python format_results.py old_results.json) <(python format_results.py new_results.json)
```

### Add to main README
```bash
# After running tests:
python update_readme.py llm_results.json

# Check the changes:
head -50 ../README.md | tail -30
```

## Troubleshooting

**"API key not found"**
- Check your environment variables are set
- Restart terminal after setting them

**"JSON decode error"**
- Model output wasn't valid JSON
- Test failed automatically
- Check llm_results.json for details

**"Rate limit exceeded"**
- Tests include delays but APIs are rate-limited
- Wait a bit and retry
- Use --limit for testing

**"Test timeout"**
- Some models are slow
- Full run can take 30-60 minutes
- Use --limit 5 for faster iterations

## Integration with CI/CD

The system is CI/CD friendly:
- No external dependencies except API keys
- JSON output for easy parsing
- Exit codes indicate success/failure
- Can be scheduled (monthly/weekly)

See `LLM_BENCHMARK_GUIDE.md` for GitHub Actions example.

## Next Steps

1. ✓ Install dependencies
2. ✓ Set API keys
3. ✓ Run first test: `python llm_benchmark.py --limit 3`
4. ✓ View results: `python format_results.py llm_results.json`
5. ✓ Run full suite: `python llm_benchmark.py`
6. ✓ Update README: `python update_readme.py llm_results.json`
7. ✓ Commit results to git

## Additional Reading

- `README.md` - Quick reference
- `LLM_BENCHMARK_GUIDE.md` - Complete guide with advanced options
- `test_cases.py` - View/modify test cases
- `llm_benchmark.py` - View/modify LLM prompts
- `format_results.py` - View/modify output formatting

## Support

Issues or questions? Check:
1. API keys are set correctly
2. Models are spelled correctly
3. API accounts have credits
4. Network connection is stable

For feature requests, see: `LLM_BENCHMARK_GUIDE.md`
