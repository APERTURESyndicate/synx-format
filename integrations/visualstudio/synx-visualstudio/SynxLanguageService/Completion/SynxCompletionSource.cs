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
            (":random",  "random\r\n  - ",  "Random selection from list. Weighted: :random 70 20 10"),
            (":calc",    "calc ",           "Arithmetic expression. Example: total:calc price * 1.2"),
            (":env",     "env ",            "Environment variable. Example: port:env PORT"),
            (":alias",   "alias ",          "Reference to another key. Example: copy:alias original"),
            (":secret",  "secret ",         "Hidden value — shows [SECRET] in logs"),
            (":default", "default:",        "Fallback value if not set"),
            (":unique",  "unique\r\n  - ",  "Deduplicate list items"),
            (":include", "include ",        "Include another .synx file"),
            (":geo",     "geo\r\n  - ",     "Value by geographic region"),
            (":template","template ",       "String interpolation with {key} placeholders"),
            (":split",   "split ",          "Split string into array. Delimiters: comma, space, pipe"),
            (":join",    "join\r\n  - ",    "Join array into string. Delimiters: comma, space, pipe"),
            (":clamp",   "clamp:0:100 ",    "Clamp number to [min, max]. Syntax: key:clamp:MIN:MAX value"),
            (":round",   "round:2 ",        "Round to N decimals. Chainable: key:calc:round:2 expr"),
            (":map",     "map:\r\n  - ",    "Map lookup: key:map:source_key  - 0 offline  - 1 online"),
            (":format",  "format:%.2f ",    "Printf-style format: %.2f, %05d, %e"),
            (":fallback","fallback:./default.txt ", "File-path fallback — use default if file missing"),
            (":once",    "once uuid",       "Generate-and-persist: uuid, random, timestamp. Stored in .synx.lock"),
            (":version", "version:>=:1.0 ", "Version compare: key:version:OP:REQUIRED value → bool"),
            (":watch",   "watch ",          "Read external file at parse time. key:watch:key_path ./file"),
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

        private static readonly string[] TypeCasts = { "(int)", "(float)", "(bool)", "(string)" };

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

            // After ':' — marker completions
            if (textBefore.EndsWith(":") || textBefore.Contains(":"))
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
                foreach (var tc in TypeCasts)
                {
                    var inner = tc.Trim('(', ')');
                    completions.Add(new Microsoft.VisualStudio.Language.Intellisense.Completion(
                        tc, inner + ")", $"Cast value to {inner}", null, null));
                }
            }

            // Template key suggestions
            if (textBefore.Contains("{"))
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
