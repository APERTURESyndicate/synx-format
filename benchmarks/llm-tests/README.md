# LLM SYNX Format Compatibility Tests

Comprehensive testing of how well different LLM models understand and work with the SYNX format.

## Quick Start

### 1. Install dependencies

```bash
pip install anthropic google-generativeai openai
```

### 2. Set API keys

```bash
# Anthropic Claude
export ANTHROPIC_API_KEY=your_key_here

# Google Gemini
export GOOGLE_API_KEY=your_key_here

# OpenAI GPT
export OPENAI_API_KEY=your_key_here
```

### 3. Run tests

```bash
# Test all models with both parsing and generation
python llm_benchmark.py --models claude-opus,claude-sonnet,claude-haiku-4-5,gemini-2.0-flash,gpt-4o

# Test only parsing
python llm_benchmark.py --test-type parse

# Quick test with only 5 test cases
python llm_benchmark.py --limit 5

# Save results to custom file
python llm_benchmark.py --output my_results.json
```

### 4. Format results

```bash
# HTML/Markdown format with progress bars
python format_results.py llm_results.json

# Compact table
python format_results.py llm_results.json --compact

# JSON statistics
python format_results.py llm_results.json --json
```

## Test Suites

### Parsing Tests (125 tests)
Tests whether models can correctly parse SYNX format and output valid JSON.

**Types covered:**
- Simple key-value pairs
- Nested objects (multi-level)
- Arrays (simple and complex)
- Numeric types (integers, floats, negative)
- Boolean values and null
- Comments and whitespace handling
- Mixed nested structures

### Generation Tests (125 tests)
Tests whether models can generate valid SYNX from English descriptions.

**Types covered:**
- User configurations
- Server/database configs
- Arrays and nested structures
- Type inference (strings, numbers, booleans)
- Complex multi-level configs

## Models Tested

### Anthropic Claude
- `claude-opus` - Most capable
- `claude-sonnet` - Balanced
- `claude-haiku-4-5` - Fast & lightweight

### Google Gemini
- `gemini-2.0-flash` - Latest fast model
- `gemini-1.5-pro` - Most capable
- `gemini-1.5-flash` - Fast variant

### OpenAI GPT
- `gpt-4o` - Latest multimodal
- `gpt-4-turbo` - Previous best
- `gpt-4` - Base GPT-4

## Results Format

Example output:

```
claude-opus
  SYNX Parsing    ███████████████████░  95.0% (19/20)
  SYNX Generation ████████████████░░░░  90.0% (18/20)

gemini-2.0-flash
  SYNX Parsing    ████████████████████  100.0% (20/20)
  SYNX Generation ███████████████████░  95.0% (19/20)

gpt-4o
  SYNX Parsing    ██████████████████░░  90.0% (18/20)
  SYNX Generation █████████████████░░░  85.0% (17/20)
```

## Extending Tests

### Add parsing test

Edit `test_cases.py` and add to `PARSE_TESTS`:

```python
{
    "id": "parse_021",
    "name": "Your test name",
    "synx": "your: synx\ntext: here",
    "expected": {"your": "synx", "text": "here"},
}
```

### Add generation test

Edit `test_cases.py` and add to `GENERATE_TESTS`:

```python
{
    "id": "gen_009",
    "name": "Your test name",
    "description": "Your English description of what SYNX to generate",
    "expected_contains": ["keyword1", "keyword2"],
}
```

## Notes

- **Rate limiting**: Tests include 100ms delays between API calls to avoid rate limits
- **Cost**: Initial run with all models and all tests will use API credits
- **Time**: Full run takes 20-30 minutes depending on model availability
- **Errors**: If an API key is missing, tests for that provider will be skipped
- **Speed**: You can limit tests with `--limit N` for faster iterations

## Interpreting Results

- **Parsing %**: How many SYNX examples the model correctly parsed to JSON
- **Generation %**: How many English descriptions the model correctly converted to valid SYNX

A model needs to understand:
- Key-value syntax
- Indentation-based nesting
- Type inference
- Comments and special values
- Array syntax
