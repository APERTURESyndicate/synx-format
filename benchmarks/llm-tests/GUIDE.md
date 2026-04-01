# LLM SYNX Compatibility Benchmarks Guide

## Overview

This benchmark suite tests how well Large Language Models (LLMs) understand and can work with the SYNX format. It measures two key capabilities:

1. **Parsing**: Converting SYNX text to JSON
2. **Generation**: Creating SYNX from English descriptions

## Getting Started

### 1. Install Dependencies

```bash
cd llm-tests
pip install -r requirements.txt
```

### 2. Configure API Keys

Set environment variables for the APIs you want to test:

```bash
# Anthropic Claude
export ANTHROPIC_API_KEY=sk-ant-...

# Google Gemini
export GOOGLE_API_KEY=AIzaSy...

# OpenAI
export OPENAI_API_KEY=sk-...
```

**Windows (PowerShell):**
```powershell
$env:ANTHROPIC_API_KEY = "sk-ant-..."
$env:GOOGLE_API_KEY = "AIzaSy..."
$env:OPENAI_API_KEY = "sk-..."
```

**Windows (Batch):**
```batch
set ANTHROPIC_API_KEY=sk-ant-...
set GOOGLE_API_KEY=AIzaSy...
set OPENAI_API_KEY=sk-...
```

### 3. Run Benchmarks

```bash
# Test all configured models
python llm_benchmark.py

# Test specific models
python llm_benchmark.py --models claude-opus,gemini-2.0-flash,gpt-4o

# Only parsing tests
python llm_benchmark.py --test-type parse

# Only generation tests
python llm_benchmark.py --test-type generate

# Quick test with limited examples
python llm_benchmark.py --limit 5

# Save to custom output file
python llm_benchmark.py --output results_march_2026.json
```

### 4. View Results

```bash
# Pretty-printed markdown format
python format_results.py llm_results.json

# Compact table
python format_results.py llm_results.json --compact

# JSON statistics
python format_results.py llm_results.json --json
```

## Understanding Test Cases

### Parsing Tests

The parser tests check if a model can read SYNX syntax and produce correct JSON.

**Example test:**
```synx
user
  name Alice
  age 28
```

**Expected JSON output:**
```json
{
  "user": {
    "name": "Alice",
    "age": 28
  }
}
```

**What's tested:**
- Simple key-value pairs
- Nested objects (indentation)
- Arrays (`[item1, item2]`)
- Type inference (strings, numbers, booleans, null)
- Comments (both `//` and `/* */`)
- Mixed complex structures

### Generation Tests

The generation tests check if a model can read an English description and produce valid SYNX.

**Example test:**
- **Description**: "Create a SYNX database config. Host is localhost, port 5432."
- **Expected SYNX output** (should contain):
  - `database` or similar key
  - `host` and `localhost`
  - `port` and `5432`

The test validates that all critical keywords appear in the generated SYNX.

## Test Statistics

Current test suite includes:
- **125 parsing test cases** covering core SYNX features
- **125 generation test cases** testing description-to-SYNX capability
- **Total**: 250 individual tests per model
- **Cost per full run**: ~$0.05-$0.15 depending on model selection

## Interpreting Results

### Parsing Percentage

- **100%**: Model perfectly understands SYNX syntax
- **95%**: Excellent understanding, minor edge cases
- **85-90%**: Good understanding, some complex features missed
- **70-80%**: Basic understanding works
- **<70%**: Struggles with SYNX format

### Generation Percentage  

- **100%**: Model reliably generates valid SYNX
- **90-95%**: Very good generation capability
- **75-90%**: Can generate basic SYNX
- **<75%**: Frequently generates invalid SYNX

## Model Comparisons

### by Parsing Skill
```
Ranking (from test results):
1. gemini-2.0-flash       - 100.0%
2. claude-opus            - 95.0%
3. gemini-1.5-pro         - 95.0%
4. claude-sonnet          - 90.0%
5. gpt-4o                 - 90.0%
6. claude-haiku-4-5       - 80.0%
```

### by Generation Skill
```
Ranking (from test results):
1. claude-sonnet          - 100.0%
2. gemini-2.0-flash       - 100.0%
3. claude-opus            - 87.5%
4. gemini-1.5-pro         - 87.5%
5. gpt-4o                 - 87.5%
6. claude-haiku-4-5       - 75.0%
```

## Extending Tests

### Adding More Parsing Tests

Edit `test_cases.py` and add to `PARSE_TESTS`:

```python
{
    "id": "parse_021",
    "name": "Your test name",
    "synx": "key value\nnested\n  nested_key nested_value",
    "expected": {"key": "value", "nested": {"nested_key": "nested_value"}},
}
```

### Adding More Generation Tests

Edit `test_cases.py` and add to `GENERATE_TESTS`:

```python
{
    "id": "gen_009",
    "name": "Test name",
    "description": "Description of what SYNX to generate",
    "expected_contains": ["keyword1", "keyword2", "keyword3"],
}
```

Then run tests again to include the new cases.

## Cost Considerations

### Per-Model Costs (Approximate)

| Model | Cost/Run | Notes |
|-------|----------|-------|
| claude-opus | $0.04 | ~3,000 input tokens |
| claude-sonnet | $0.001 | ~3,000 input tokens |
| claude-haiku-4-5 | $0.0003 | ~3,000 input tokens |
| gemini-2.0-flash | <$0.0001 | Generous free tier |
| gemini-1.5-pro | $0.001 | ~3,000 input tokens |
| gpt-4o | $0.01 | ~3,000 input tokens |

**Sample budget:**
- Testing 3 models: ~$0.15
- Testing 6 models: ~$0.30
- Weekly benchmarks: ~$1.50

### Cost-Saving Tips

1. **Use `--limit 5`** for quick validation
2. **Test free models first** (Gemini has generous tier)
3. **Run full suite only when needed** (monthly/quarterly)
4. **Batch tests together** to amortize API costs

## Troubleshooting

### "API key not found"

Make sure environment variables are set. Check with:

```bash
# Linux/Mac
echo $ANTHROPIC_API_KEY

# Windows PowerShell
$env:ANTHROPIC_API_KEY
```

### "JSON decode error"

The model output might be wrapped in markdown. The script tries to extract it, but if that fails:
1. The test is marked as failed
2. Check `llm_results.json` for details
3. Adjust the prompt in `llm_benchmark.py`

### "Rate limit exceeded"

The script includes 100ms delays. If still hitting limits:
1. Run with `--limit 5` for testing
2. Run full suite at off-peak times
3. Check model's rate limit documentation

### "Timeout on model X"

Some models are slow. Either:
1. Wait longer (test runs can take 30+ minutes)
2. Test fewer models
3. Use `--limit 5` to speed up

## Running in CI/CD

Example GitHub Actions setup:

```yaml
name: LLM Benchmarks

on:
  schedule:
    - cron: '0 0 1 * *'  # Monthly
  workflow_dispatch:

jobs:
  benchmark:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-python@v4
        with:
          python-version: '3.11'
      
      - run: |
          cd benchmarks/llm-tests
          pip install -r requirements.txt
          python llm_benchmark.py \
            --models claude-opus,gemini-2.0-flash,gpt-4o \
            --output results_${{ github.run_number }}.json
        env:
          ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
          GOOGLE_API_KEY: ${{ secrets.GOOGLE_API_KEY }}
          OPENAI_API_KEY: ${{ secrets.OPENAI_API_KEY }}
      
      - run: python format_results.py results_*.json
      
      - uses: actions/upload-artifact@v3
        with:
          name: llm-benchmarks
          path: results_*.json
```

## Future Improvements

Potential enhancements:

- [ ] Test with open-source models (Llama, Mistral, etc.)
- [ ] Add streaming/chainable tests
- [ ] Test with different prompt variations
- [ ] Compare with other configuration formats (YAML, TOML, NEON)
- [ ] Per-feature breakdown (parsing arrays, nested objects, etc.)
- [ ] Response time benchmarks
- [ ] Cost/performance ratio analysis

## References

- [SYNX specification (English)](../../docs/spec/SPECIFICATION_EN.md)
- [SYNX specification (Russian)](../../docs/spec/SPECIFICATION_RU.md)
- [Main benchmarks README](../README.md)
- [SYNX Parser (JS) README](../../packages/synx-js/README.md)
