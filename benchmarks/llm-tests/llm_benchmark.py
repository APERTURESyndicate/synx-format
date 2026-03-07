#!/usr/bin/env python3
"""
LLM SYNX Format Benchmark
Tests how well different LLM models can parse and generate SYNX format

Usage:
    python llm_benchmark.py --models claude-opus,gemini-2.0-flash,gpt-4o --test-type both
"""

import os
import sys
import json
import argparse
import time
from typing import Optional
from pathlib import Path
from test_cases import get_parse_tests, get_generate_tests

# ─────────────────────────────────────────────────────────────────────────────
# LLM API Clients
# ─────────────────────────────────────────────────────────────────────────────

class LLMClient:
    """Base class for LLM API clients"""
    
    def __init__(self, model_name: str, api_key: Optional[str] = None):
        self.model_name = model_name
        self.api_key = api_key
    
    def parse_synx(self, synx_text: str) -> Optional[dict]:
        """Ask model to parse SYNX text and return JSON"""
        raise NotImplementedError
    
    def generate_synx(self, description: str) -> Optional[str]:
        """Ask model to generate SYNX from description"""
        raise NotImplementedError


class ClaudeClient(LLMClient):
    """Anthropic Claude API client"""
    
    def __init__(self, model_name: str, api_key: Optional[str] = None):
        super().__init__(model_name, api_key or os.getenv("ANTHROPIC_API_KEY"))
        if not self.api_key:
            print(f"⚠ Warning: ANTHROPIC_API_KEY not set - Claude tests will fail")
    
    def parse_synx(self, synx_text: str) -> Optional[dict]:
        """Parse SYNX using Claude"""
        if not self.api_key:
            return None
        
        try:
            import anthropic
            client = anthropic.Anthropic(api_key=self.api_key)
            
            message = client.messages.create(
                model=self.model_name,
                max_tokens=1024,
                messages=[
                    {
                        "role": "user",
                        "content": f"""You are an expert in the SYNX format. Parse the following SYNX text and output ONLY valid JSON.
Do not include any explanation, markdown, or code blocks. Just the JSON.

SYNX input:
{synx_text}

Output the JSON:"""
                    }
                ]
            )
            
            response_text = message.content[0].text.strip()
            # Try to extract JSON if wrapped in markdown
            if response_text.startswith("```"):
                response_text = response_text.split("```")[1].strip()
                if response_text.startswith("json"):
                    response_text = response_text[4:].strip()
            
            return json.loads(response_text)
        except Exception as e:
            print(f"Error parsing SYNX with {self.model_name}: {e}")
            return None
    
    def generate_synx(self, description: str) -> Optional[str]:
        """Generate SYNX using Claude"""
        if not self.api_key:
            return None
        
        try:
            import anthropic
            client = anthropic.Anthropic(api_key=self.api_key)
            
            message = client.messages.create(
                model=self.model_name,
                max_tokens=1024,
                messages=[
                    {
                        "role": "user",
                        "content": f"""You are an expert in the SYNX format. Generate valid SYNX configuration based on the description.
Output ONLY the SYNX code. No explanation, no markdown, no code blocks.

Description:
{description}

SYNX output:"""
                    }
                ]
            )
            
            return message.content[0].text.strip()
        except Exception as e:
            print(f"Error generating SYNX with {self.model_name}: {e}")
            return None


class GeminiClient(LLMClient):
    """Google Gemini API client"""
    
    def __init__(self, model_name: str, api_key: Optional[str] = None):
        super().__init__(model_name, api_key or os.getenv("GOOGLE_API_KEY"))
        if not self.api_key:
            print(f"⚠ Warning: GOOGLE_API_KEY not set - Gemini tests will fail")
    
    def parse_synx(self, synx_text: str) -> Optional[dict]:
        """Parse SYNX using Gemini"""
        if not self.api_key:
            return None
        
        try:
            import google.generativeai as genai
            genai.configure(api_key=self.api_key)
            model = genai.GenerativeModel(self.model_name)
            
            response = model.generate_content(
                f"""You are an expert in the SYNX format. Parse the following SYNX text and output ONLY valid JSON.
Do not include any explanation, markdown, or code blocks. Just the JSON.

SYNX input:
{synx_text}

Output the JSON:"""
            )
            
            response_text = response.text.strip()
            if response_text.startswith("```"):
                response_text = response_text.split("```")[1].strip()
                if response_text.startswith("json"):
                    response_text = response_text[4:].strip()
            
            return json.loads(response_text)
        except Exception as e:
            print(f"Error parsing SYNX with {self.model_name}: {e}")
            return None
    
    def generate_synx(self, description: str) -> Optional[str]:
        """Generate SYNX using Gemini"""
        if not self.api_key:
            return None
        
        try:
            import google.generativeai as genai
            genai.configure(api_key=self.api_key)
            model = genai.GenerativeModel(self.model_name)
            
            response = model.generate_content(
                f"""You are an expert in the SYNX format. Generate valid SYNX configuration based on the description.
Output ONLY the SYNX code. No explanation, no markdown, no code blocks.

Description:
{description}

SYNX output:"""
            )
            
            return response.text.strip()
        except Exception as e:
            print(f"Error generating SYNX with {self.model_name}: {e}")
            return None


class OpenAIClient(LLMClient):
    """OpenAI GPT API client"""
    
    def __init__(self, model_name: str, api_key: Optional[str] = None):
        super().__init__(model_name, api_key or os.getenv("OPENAI_API_KEY"))
        if not self.api_key:
            print(f"⚠ Warning: OPENAI_API_KEY not set - GPT tests will fail")
    
    def parse_synx(self, synx_text: str) -> Optional[dict]:
        """Parse SYNX using GPT"""
        if not self.api_key:
            return None
        
        try:
            from openai import OpenAI
            client = OpenAI(api_key=self.api_key)
            
            response = client.chat.completions.create(
                model=self.model_name,
                messages=[
                    {
                        "role": "user",
                        "content": f"""You are an expert in the SYNX format. Parse the following SYNX text and output ONLY valid JSON.
Do not include any explanation, markdown, or code blocks. Just the JSON.

SYNX input:
{synx_text}

Output the JSON:"""
                    }
                ],
                temperature=0.7,
                max_tokens=1024,
            )
            
            response_text = response.choices[0].message.content.strip()
            if response_text.startswith("```"):
                response_text = response_text.split("```")[1].strip()
                if response_text.startswith("json"):
                    response_text = response_text[4:].strip()
            
            return json.loads(response_text)
        except Exception as e:
            print(f"Error parsing SYNX with {self.model_name}: {e}")
            return None
    
    def generate_synx(self, description: str) -> Optional[str]:
        """Generate SYNX using GPT"""
        if not self.api_key:
            return None
        
        try:
            from openai import OpenAI
            client = OpenAI(api_key=self.api_key)
            
            response = client.chat.completions.create(
                model=self.model_name,
                messages=[
                    {
                        "role": "user",
                        "content": f"""You are an expert in the SYNX format. Generate valid SYNX configuration based on the description.
Output ONLY the SYNX code. No explanation, no markdown, no code blocks.

Description:
{description}

SYNX output:"""
                    }
                ],
                temperature=0.7,
                max_tokens=1024,
            )
            
            return response.choices[0].message.content.strip()
        except Exception as e:
            print(f"Error generating SYNX with {self.model_name}: {e}")
            return None


# ─────────────────────────────────────────────────────────────────────────────
# Test Runner
# ─────────────────────────────────────────────────────────────────────────────

def get_client(model_name: str) -> LLMClient:
    """Factory function to get the right client for a model"""
    if model_name.startswith("claude-"):
        return ClaudeClient(model_name)
    elif model_name.startswith("gemini-"):
        return GeminiClient(model_name)
    elif model_name.startswith("gpt-") or "gpt" in model_name:
        return OpenAIClient(model_name)
    else:
        raise ValueError(f"Unknown model: {model_name}")


def test_parse(client: LLMClient, test_cases: list) -> dict:
    """Run parsing tests"""
    print(f"\n🧪 Testing {client.model_name} - SYNX Parsing ({len(test_cases)} tests)...")
    
    passed = 0
    failed = 0
    skipped = 0
    results = []
    
    for i, test in enumerate(test_cases):
        try:
            result = client.parse_synx(test["synx"])
            
            if result is None:
                skipped += 1
                results.append({"test_id": test["id"], "status": "skipped", "reason": "API error"})
            elif result == test["expected"]:
                passed += 1
                results.append({"test_id": test["id"], "status": "passed"})
            else:
                failed += 1
                results.append({"test_id": test["id"], "status": "failed", "got": result})
            
            print(f"  [{i+1}/{len(test_cases)}] {test['name']}: {'✓' if result == test['expected'] else '✗'}")
            time.sleep(0.1)  # Rate limiting
        except Exception as e:
            failed += 1
            results.append({"test_id": test["id"], "status": "error", "reason": str(e)})
            print(f"  [{i+1}/{len(test_cases)}] {test['name']}: ERROR - {e}")
    
    return {
        "model": client.model_name,
        "test_type": "parse",
        "total": len(test_cases),
        "passed": passed,
        "failed": failed,
        "skipped": skipped,
        "details": results
    }


def test_generate(client: LLMClient, test_cases: list) -> dict:
    """Run generation tests"""
    print(f"\n🧪 Testing {client.model_name} - SYNX Generation ({len(test_cases)} tests)...")
    
    passed = 0
    failed = 0
    skipped = 0
    results = []
    
    for i, test in enumerate(test_cases):
        try:
            result = client.generate_synx(test["description"])
            
            if result is None:
                skipped += 1
                results.append({"test_id": test["id"], "status": "skipped", "reason": "API error"})
            else:
                # Check if all expected keywords are in the result
                result_lower = result.lower()
                all_found = all(keyword.lower() in result_lower for keyword in test["expected_contains"])
                
                if all_found:
                    passed += 1
                    results.append({"test_id": test["id"], "status": "passed"})
                else:
                    failed += 1
                    results.append({"test_id": test["id"], "status": "failed", "got": result})
                
                print(f"  [{i+1}/{len(test_cases)}] {test['name']}: {'✓' if all_found else '✗'}")
            
            time.sleep(0.1)  # Rate limiting
        except Exception as e:
            failed += 1
            results.append({"test_id": test["id"], "status": "error", "reason": str(e)})
            print(f"  [{i+1}/{len(test_cases)}] {test['name']}: ERROR - {e}")
    
    return {
        "model": client.model_name,
        "test_type": "generate",
        "total": len(test_cases),
        "passed": passed,
        "failed": failed,
        "skipped": skipped,
        "details": results
    }


# ─────────────────────────────────────────────────────────────────────────────
# Main
# ─────────────────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="LLM SYNX Format Benchmark")
    parser.add_argument(
        "--models",
        default="claude-opus,claude-sonnet,claude-haiku-4-5,gemini-2.0-flash,gpt-4o",
        help="Comma-separated list of models to test"
    )
    parser.add_argument(
        "--test-type",
        choices=["parse", "generate", "both"],
        default="both",
        help="Type of tests to run"
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=None,
        help="Limit number of tests (for quick testing)"
    )
    parser.add_argument(
        "--output",
        type=str,
        default="llm_results.json",
        help="Output file for results"
    )
    
    args = parser.parse_args()
    
    models = [m.strip() for m in args.models.split(",")]
    parse_tests = get_parse_tests()
    generate_tests = get_generate_tests()
    
    if args.limit:
        parse_tests = parse_tests[:args.limit]
        generate_tests = generate_tests[:args.limit]
    
    all_results = []
    
    for model_name in models:
        print(f"\n{'='*60}")
        print(f"Testing: {model_name}")
        print(f"{'='*60}")
        
        try:
            client = get_client(model_name)
            
            if args.test_type in ["parse", "both"]:
                result = test_parse(client, parse_tests)
                all_results.append(result)
                print(f"✓ Parse: {result['passed']}/{result['total']} passed")
            
            if args.test_type in ["generate", "both"]:
                result = test_generate(client, generate_tests)
                all_results.append(result)
                print(f"✓ Generate: {result['passed']}/{result['total']} passed")
        
        except Exception as e:
            print(f"❌ Error testing {model_name}: {e}")
    
    # Save results
    output_file = args.output
    with open(output_file, "w") as f:
        json.dump(all_results, f, indent=2)
    
    print(f"\n📝 Results saved to: {output_file}")
    
    # Print summary
    print(f"\n{'='*60}")
    print("SUMMARY")
    print(f"{'='*60}")
    
    for result in all_results:
        pct = (result["passed"] / result["total"] * 100) if result["total"] > 0 else 0
        print(f"{result['model']:30} {result['test_type']:10} {result['passed']:3}/{result['total']:3} ({pct:5.1f}%)")


if __name__ == "__main__":
    main()
