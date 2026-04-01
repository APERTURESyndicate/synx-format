using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using Microsoft.VisualStudio.Shell;
using Microsoft.VisualStudio.Shell.TableControl;
using Microsoft.VisualStudio.Shell.TableManager;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Adornments;
using Microsoft.VisualStudio.Text.Editor;
using Microsoft.VisualStudio.Text.Tagging;
using Microsoft.VisualStudio.Utilities;
using SynxLanguageService.Parser;

namespace SynxLanguageService.Diagnostics
{
    // ─── Error Tag ───────────────────────────────────────────────────────────

    [Export(typeof(ITaggerProvider))]
    [ContentType("synx")]
    [TagType(typeof(IErrorTag))]
    internal class SynxErrorTaggerProvider : ITaggerProvider
    {
        public ITagger<T> CreateTagger<T>(ITextBuffer buffer) where T : ITag
        {
            return buffer.Properties.GetOrCreateSingletonProperty(() =>
                new SynxErrorTagger(buffer)) as ITagger<T>;
        }
    }

    internal class SynxErrorTagger : ITagger<IErrorTag>
    {
        private readonly ITextBuffer _buffer;
        private static readonly HashSet<string> KnownMarkers = new(StringComparer.OrdinalIgnoreCase)
        {
            "random", "calc", "env", "alias", "secret", "default",
            "unique", "include", "geo", "template", "split", "join"
        };
        private static readonly HashSet<string> KnownConstraints = new(StringComparer.OrdinalIgnoreCase)
        {
            "min", "max", "type", "required", "readonly", "pattern", "enum"
        };
        private static readonly HashSet<string> KnownTypes = new(StringComparer.OrdinalIgnoreCase)
        {
            "int", "float", "bool", "string"
        };

        public SynxErrorTagger(ITextBuffer buffer)
        {
            _buffer = buffer;
            _buffer.Changed += (s, e) => TagsChanged?.Invoke(this,
                new SnapshotSpanEventArgs(new SnapshotSpan(e.After, 0, e.After.Length)));
        }

        public event EventHandler<SnapshotSpanEventArgs> TagsChanged;

        public IEnumerable<ITagSpan<IErrorTag>> GetTags(NormalizedSnapshotSpanCollection spans)
        {
            if (spans.Count == 0) yield break;

            var snapshot = spans[0].Snapshot;
            var text = snapshot.GetText();
            var parsed = SynxParser.Parse(text);
            var lines = text.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None);

            for (int i = 0; i < lines.Length; i++)
            {
                var line = lines[i];
                if (string.IsNullOrWhiteSpace(line)) continue;

                var snapshotLine = snapshot.GetLineFromLineNumber(i);

                // Tab check
                if (line.Contains('\t'))
                {
                    int col = line.IndexOf('\t');
                    yield return MakeTag(snapshot, snapshotLine.Start.Position + col, 1,
                        PredefinedErrorTypeNames.SyntaxError, "Use spaces, not tabs");
                }

                // Indentation check
                int indent = line.Length - line.TrimStart().Length;
                if (indent > 0 && indent % 2 != 0 && !line.TrimStart().StartsWith("#") && !line.TrimStart().StartsWith("//"))
                {
                    yield return MakeTag(snapshot, snapshotLine.Start.Position, indent,
                        PredefinedErrorTypeNames.Warning, "Indentation should be a multiple of 2 spaces");
                }
            }

            // Node-level checks
            var keysByScope = new Dictionary<string, HashSet<string>>();
            foreach (var node in parsed.AllNodes)
            {
                if (node.IsListItem) continue;
                var snapshotLine = snapshot.GetLineFromLineNumber(node.Line);

                // Invalid key start
                if (node.Key.Length > 0 && "-#/!".Contains(node.Key[0]))
                {
                    yield return MakeTag(snapshot, snapshotLine.Start.Position + node.Column, node.Key.Length,
                        PredefinedErrorTypeNames.SyntaxError, $"Key cannot start with '{node.Key[0]}'");
                }

                // Duplicate keys
                string scope = node.Parent != null ? $"{node.Parent.Line}:{node.Indent}" : $"root:{node.Indent}";
                if (!keysByScope.ContainsKey(scope)) keysByScope[scope] = new HashSet<string>();
                if (!keysByScope[scope].Add(node.Key))
                {
                    yield return MakeTag(snapshot, snapshotLine.Start.Position + node.Column, node.Key.Length,
                        PredefinedErrorTypeNames.Warning, $"Duplicate key: '{node.Key}'");
                }

                // Unknown type cast
                if (!string.IsNullOrEmpty(node.TypeHint) && !KnownTypes.Contains(node.TypeHint))
                {
                    int typePos = lines[node.Line].IndexOf($"({node.TypeHint})");
                    if (typePos >= 0)
                        yield return MakeTag(snapshot, snapshotLine.Start.Position + typePos, node.TypeHint.Length + 2,
                            PredefinedErrorTypeNames.SyntaxError, $"Unknown type cast: ({node.TypeHint}). Use: int, float, bool, string");
                }

                // Markers checks
                foreach (var marker in node.Markers)
                {
                    if (!KnownMarkers.Contains(marker))
                    {
                        int mPos = lines[node.Line].IndexOf($":{marker}");
                        if (mPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + mPos, marker.Length + 1,
                                PredefinedErrorTypeNames.Warning, $"Unknown marker: :{marker}");
                    }

                    // Markers without !active
                    if (parsed.Mode != "active")
                    {
                        int mPos = lines[node.Line].IndexOf($":{marker}");
                        if (mPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + mPos, marker.Length + 1,
                                PredefinedErrorTypeNames.HintedSuggestion, "Markers only work with !active on the first line");
                    }
                }

                // :alias broken ref
                if (node.Markers.Contains("alias") && !string.IsNullOrEmpty(node.RawValue))
                {
                    if (!parsed.KeyMap.ContainsKey(node.RawValue.Trim()))
                    {
                        int vPos = lines[node.Line].LastIndexOf(node.RawValue.Trim());
                        if (vPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, node.RawValue.Trim().Length,
                                PredefinedErrorTypeNames.SyntaxError, $"Alias target not found: '{node.RawValue.Trim()}'");
                    }
                }

                // :calc unknown vars
                if (node.Markers.Contains("calc") && !string.IsNullOrEmpty(node.RawValue))
                {
                    var varRe = new Regex(@"\b[a-zA-Z_]\w*\b");
                    foreach (Match m in varRe.Matches(node.RawValue))
                    {
                        if (!parsed.KeyMap.ContainsKey(m.Value))
                        {
                            int vPos = lines[node.Line].LastIndexOf(m.Value);
                            if (vPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, m.Value.Length,
                                    PredefinedErrorTypeNames.Warning, $"Undefined variable in calc: '{m.Value}'");
                        }
                    }
                }

                // :template missing keys
                if (node.Markers.Contains("template") && !string.IsNullOrEmpty(node.RawValue))
                {
                    var phRe = new Regex(@"\{([^}]+)\}");
                    foreach (Match m in phRe.Matches(node.RawValue))
                    {
                        var key = m.Groups[1].Value;
                        if (!parsed.KeyMap.ContainsKey(key))
                        {
                            int phPos = lines[node.Line].IndexOf(m.Value);
                            if (phPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + phPos, m.Value.Length,
                                    PredefinedErrorTypeNames.Warning, $"Template placeholder not found: {{{key}}}");
                        }
                    }
                }

                // Constraint validation
                foreach (var constraint in node.Constraints)
                {
                    var cName = constraint.Split(':')[0].Trim();
                    if (!KnownConstraints.Contains(cName))
                    {
                        int cPos = lines[node.Line].IndexOf(constraint);
                        if (cPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + cPos, constraint.Length,
                                PredefinedErrorTypeNames.Warning, $"Unknown constraint: {cName}");
                    }

                    // enum validation
                    if (cName == "enum" && constraint.Contains(":"))
                    {
                        var allowed = constraint.Substring(constraint.IndexOf(':') + 1).Split('|');
                        if (node.Value is string sv && !allowed.Contains(sv))
                        {
                            int vPos = lines[node.Line].LastIndexOf(sv);
                            if (vPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, sv.Length,
                                    PredefinedErrorTypeNames.SyntaxError, $"Value '{sv}' not in allowed: {string.Join(", ", allowed)}");
                        }
                    }
                }
            }
        }

        private static ITagSpan<IErrorTag> MakeTag(ITextSnapshot snapshot, int start, int length, string errorType, string message)
        {
            if (start + length > snapshot.Length) length = snapshot.Length - start;
            if (start < 0 || length <= 0) return null;
            var span = new SnapshotSpan(snapshot, start, length);
            return new TagSpan<IErrorTag>(span, new ErrorTag(errorType, message));
        }
    }
}
