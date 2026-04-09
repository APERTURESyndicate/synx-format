using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.Linq;
using Microsoft.VisualStudio.Language.Intellisense;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Operations;
using Microsoft.VisualStudio.Utilities;
using SynxLanguageService.Parser;

namespace SynxLanguageService.Completion
{
    [Export(typeof(ICompletionSourceProvider))]
    [ContentType("synx")]
    [Name("synxCompletion")]
    internal class SynxCompletionSourceProvider : ICompletionSourceProvider
    {
        [Import]
        internal ITextStructureNavigatorSelectorService NavigatorService { get; set; }

        public ICompletionSource TryCreateCompletionSource(ITextBuffer textBuffer)
        {
            return new SynxCompletionSource(textBuffer);
        }
    }

    internal class SynxCompletionSource : ICompletionSource
    {
        private readonly ITextBuffer _buffer;
        private bool _isDisposed;

        // (name, insertionText, desc)
        private static readonly (string name, string insertion, string desc)[] Markers = new[]
        {
            (":calc",     "calc ",            "Arithmetic expression. Operators: + - * / % (). Example: total:calc price * 1.2"),
            (":random",   "random\r\n  - ",   "Random selection from list. Weighted: :random 70 20 10"),
            (":env",      "env ",             "Environment variable. Example: port:env PORT. Chainable with :default"),
            (":alias",    "alias ",           "Reference to another key. Supports dot-paths. Example: copy:alias server.host"),
            (":ref",      "ref ",             "Reference with chaining. Feeds resolved value to next marker. Example: rate:ref:calc:*2 base_rate"),
            (":inherit",  "inherit:",         "Inherit fields from parent blocks. Example: steel:inherit:_base_resource:_base_rare. Child fields override."),
            (":i18n",     "i18n\r\n  ",       "Multilingual value with optional plural support. Example: label:i18n:count_field → en/other {count} items"),
            (":secret",   "secret ",          "Hidden value — shows [SECRET] in logs/JSON. Use .reveal() to access"),
            (":default",  "default:",         "Fallback value if not set. Chainable: port:env:default:8080 PORT"),
            (":unique",   "unique\r\n  - ",   "Deduplicate list items. Preserves first occurrence order"),
            (":include",  "include ",         "Include another .synx file. Path relative to current file. Max depth: 16"),
            (":import",   "import ",          "Alias for :include (key-level file embedding)"),
            (":geo",      "geo\r\n  - ",      "Value by geographic region. List: - REGION value. Requires runtime region support"),
            (":template", "template ",        "String interpolation (legacy). Modern: just use {key} in !active mode"),
            (":split",    "split ",           "Split string → array. Delimiters: comma, space, pipe, dash, dot, semi, tab"),
            (":join",     "join\r\n  - ",     "Join array → string. Keywords: space, pipe, dash, dot, semi, tab, slash"),
            (":spam",     "spam:3:60 ",       "Rate-limit access. Syntax: :spam:MAX_CALLS:WINDOW_SEC. Default window: 1s"),
            (":clamp",    "clamp:0:100 ",     "Clamp number to [min, max]. Example: volume:clamp:0:100 150 → 100"),
            (":round",    "round:2 ",         "Round to N decimals. Default: 0. Chainable: key:calc:round:2 expr"),
            (":map",      "map:\r\n  - ",     "Map lookup: key:map:source_key — list items: - lookup_value result_text"),
            (":format",   "format:%.2f ",     "Printf-style format: %.2f (float), %05d (zero-padded), %e (scientific)"),
            (":fallback", "fallback:./default.txt ", "File-path fallback — use default path if value file missing or empty"),
            (":once",     "once uuid",        "Generate-and-persist: uuid, random, timestamp. Stored in .synx.lock sidecar"),
            (":version",  "version:>=:1.0 ",  "Version compare: key:version:OP:REQUIRED value → bool. OPs: >= <= > < == !="),
            (":watch",    "watch ",           "Read external file at parse time. Optional key extraction: key:watch:key_path ./file"),
            (":prompt",   "prompt:Label ",    "Format subtree for LLM prompt. Output: Label (SYNX):\\n```synx\\n...\\n```"),
            (":vision",   "vision ",          "Metadata marker — image generation intent. Value passes through unchanged"),
            (":audio",    "audio ",           "Metadata marker — audio/TTS generation intent. Value passes through unchanged"),
        };

        private static readonly (string name, string desc)[] Constraints = new[]
        {
            ("required", "Field must have a value"),
            ("readonly", "Value cannot be changed"),
            ("min:", "Minimum length or numeric value"),
            ("max:", "Maximum length or numeric value"),
            ("type:", "Enforce type: int, float, bool, string"),
            ("pattern:", "Regex validation pattern"),
            ("enum:", "Allowed values separated by |"),
        };

        private static readonly string[] TypeCasts = { "(int)", "(float)", "(bool)", "(string)" }; // kept for reference

        public SynxCompletionSource(ITextBuffer buffer)
        {
            _buffer = buffer;
        }

        public void AugmentCompletionSession(ICompletionSession session, IList<CompletionSet> completionSets)
        {
            var triggerPoint = session.GetTriggerPoint(_buffer.CurrentSnapshot);
            if (!triggerPoint.HasValue) return;

            var snapshot = _buffer.CurrentSnapshot;
            var line = triggerPoint.Value.GetContainingLine();
            var lineText = line.GetText();
            var position = triggerPoint.Value.Position - line.Start.Position;

            var completions = new List<Microsoft.VisualStudio.Language.Intellisense.Completion>();

            // Detect context
            var textBefore = lineText.Substring(0, Math.Min(position, lineText.Length));

            // Directive completions at start of line
            var trimmedBefore = textBefore.TrimStart();
            if (trimmedBefore == "!" || trimmedBefore == "")
            {
                var directives = new (string name, string insertion, string desc)[]
                {
                    ("!active",  "!active",              "Enable active mode — markers and constraints become functional"),
                    ("!lock",    "!lock",                "Lock config — prevent external set/add/remove via API"),
                    ("!static",  "!static",              "Explicit static mode (default if no !active)"),
                    ("!include", "!include ./file.synx", "Include external file for {} references"),
                    ("!tool",    "!tool",                "Mark config as tool-consumable"),
                    ("!schema",  "!schema",              "Schema envelope hint for validation tools"),
                    ("!llm",     "!llm",                 "LLM envelope hint — structured for AI consumption"),
                };

                foreach (var (name, insertion, desc) in directives)
                {
                    completions.Add(new Microsoft.VisualStudio.Language.Intellisense.Completion(
                        name, trimmedBefore == "!" ? insertion.Substring(1) : insertion, desc, null, null));
                }
            }

            // After ':' — marker completions
            if (textBefore.EndsWith(":") || (textBefore.Contains(":") && !textBefore.Contains(" ")))
            {
                foreach (var (name, insertion, desc) in Markers)
                {
                    completions.Add(new Microsoft.VisualStudio.Language.Intellisense.Completion(
                        name, insertion, desc, null, null));
                }
            }

            // Inside [...] — constraint completions
            if (textBefore.Contains("[") && !textBefore.Contains("]"))
            {
                foreach (var (name, desc) in Constraints)
                {
                    completions.Add(new Microsoft.VisualStudio.Language.Intellisense.Completion(
                        name, name, desc, null, null));
                }
            }

            // After '(' — type cast completions
            if (textBefore.EndsWith("("))
            {
                var casts = new[] { "(int)", "(float)", "(bool)", "(string)", "(random)", "(random:int)", "(random:float)", "(random:bool)" };
                foreach (var tc in casts)
                {
                    var inner = tc.Trim('(', ')');
                    completions.Add(new Microsoft.VisualStudio.Language.Intellisense.Completion(
                        tc, inner + ")", tc.StartsWith("(random") ? $"Generate random {(inner == "random" ? "int" : inner.Split(':').Last())}" : $"Cast value to {inner}", null, null));
                }
            }

            // Template key suggestions and alias key suggestions
            if (textBefore.Contains("{") || textBefore.Contains(":alias ") || System.Text.RegularExpressions.Regex.IsMatch(textBefore, @":calc\s+[\w.]*$"))
            {
                var doc = SynxParser.Parse(snapshot.GetText());
                foreach (var key in doc.KeyMap.Keys)
                {
                    completions.Add(new Microsoft.VisualStudio.Language.Intellisense.Completion(
                        key, key, $"Reference to key: {key}", null, null));
                }
            }

            if (completions.Count > 0)
            {
                var trackingSpan = FindTokenSpanAtPosition(triggerPoint.Value, snapshot);
                completionSets.Add(new CompletionSet("synx", "SYNX", trackingSpan, completions, null));
            }
        }

        private ITrackingSpan FindTokenSpanAtPosition(SnapshotPoint point, ITextSnapshot snapshot)
        {
            var line = point.GetContainingLine();
            var lineText = line.GetText();
            int pos = point.Position - line.Start.Position;

            int start = pos;
            while (start > 0 && !char.IsWhiteSpace(lineText[start - 1]) && lineText[start - 1] != ':' && lineText[start - 1] != '[' && lineText[start - 1] != '(')
                start--;

            return snapshot.CreateTrackingSpan(new SnapshotSpan(line.Start + start, point), SpanTrackingMode.EdgeInclusive);
        }

        public void Dispose()
        {
            if (!_isDisposed)
            {
                GC.SuppressFinalize(this);
                _isDisposed = true;
            }
        }
    }
}
