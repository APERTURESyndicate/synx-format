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
            "random", "calc", "env", "alias", "ref", "inherit", "i18n",
            "secret", "default", "unique", "include", "import", "geo",
            "template", "split", "join", "clamp", "round", "map", "format",
            "fallback", "once", "version", "watch", "spam", "prompt",
            "vision", "audio"
        };
        private static readonly HashSet<string> KnownConstraints = new(StringComparer.OrdinalIgnoreCase)
        {
            "min", "max", "type", "required", "readonly", "pattern", "enum"
        };
        private static readonly HashSet<string> KnownTypes = new(StringComparer.OrdinalIgnoreCase)
        {
            "int", "float", "bool", "string", "random", "random:int", "random:float", "random:bool"
        };
        private static readonly HashSet<string> DelimKeywords = new(StringComparer.OrdinalIgnoreCase)
        {
            "space", "pipe", "dash", "dot", "semi", "tab", "slash"
        };
        private static readonly HashSet<string> ArgMarkers = new(StringComparer.OrdinalIgnoreCase)
        {
            "inherit", "default", "map", "round", "clamp", "format",
            "version", "fallback", "watch", "i18n", "include", "import",
            "spam", "prompt"
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
                var argIndexes = CollectMarkerArgIndexes(node.Markers);
                for (int mi = 0; mi < node.Markers.Count; mi++)
                {
                    var marker = node.Markers[mi];
                    if (argIndexes.Contains(mi)) continue;
                    if (DelimKeywords.Contains(marker) || Regex.IsMatch(marker, @"^\d")) continue;

                    if (!KnownMarkers.Contains(marker))
                    {
                        int mPos = lines[node.Line].IndexOf($":{marker}");
                        if (mPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + mPos, marker.Length + 1,
                                PredefinedErrorTypeNames.Warning, $"Unknown marker: :{marker}");
                    }
                }

                // Markers without !active
                if (node.Markers.Count > 0 && parsed.Mode != "active")
                {
                    foreach (var marker in node.Markers)
                    {
                        if (argIndexes.Contains(node.Markers.IndexOf(marker))) continue;
                        if (!KnownMarkers.Contains(marker)) continue;
                        int mPos = lines[node.Line].IndexOf($":{marker}");
                        if (mPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + mPos, marker.Length + 1,
                                PredefinedErrorTypeNames.HintedSuggestion, "Markers require \"!active\" on line 1 to be resolved");
                        break; // Only warn once per line
                    }
                }

                // :alias broken ref + self-alias + circular alias detection
                if (node.Markers.Contains("alias") && !string.IsNullOrEmpty(node.RawValue) && parsed.Mode == "active")
                {
                    var aliasRef = node.RawValue.Trim();
                    if (!parsed.KeyMap.ContainsKey(aliasRef))
                    {
                        int vPos = lines[node.Line].LastIndexOf(aliasRef);
                        if (vPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, aliasRef.Length,
                                PredefinedErrorTypeNames.SyntaxError, $"Alias target not found: '{aliasRef}'");
                    }

                    // Self-reference check
                    if (aliasRef == node.Key)
                    {
                        int vPos = lines[node.Line].LastIndexOf(aliasRef);
                        if (vPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, aliasRef.Length,
                                PredefinedErrorTypeNames.Warning, $"Self-referential alias: '{node.Key}' aliases itself");
                    }
                    else
                    {
                        // Circular alias detection (1-hop)
                        if (parsed.KeyMap.TryGetValue(aliasRef, out var targetNode))
                        {
                            if (targetNode.Markers.Contains("alias") && targetNode.RawValue?.Trim() == node.Key)
                            {
                                int vPos = lines[node.Line].LastIndexOf(aliasRef);
                                if (vPos >= 0)
                                    yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, aliasRef.Length,
                                        PredefinedErrorTypeNames.Warning, $"Circular alias: '{node.Key}' → '{aliasRef}' forms a cycle");
                            }
                        }
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
                if (node.Markers.Contains("template") && !string.IsNullOrEmpty(node.RawValue) && parsed.Mode == "active")
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

                // :inherit → check all parent references exist
                if (node.Markers.Contains("inherit") && parsed.Mode == "active")
                {
                    var inheritRefs = GetInheritRefs(node);
                    foreach (var iRef in inheritRefs)
                    {
                        if (!parsed.KeyMap.ContainsKey(iRef))
                        {
                            int vPos = lines[node.Line].LastIndexOf(iRef);
                            if (vPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, iRef.Length,
                                    PredefinedErrorTypeNames.SyntaxError, $"Key '{iRef}' used in :inherit is not defined");
                        }
                        if (iRef == node.Key || iRef == BuildPath(node))
                        {
                            int vPos = lines[node.Line].LastIndexOf(iRef);
                            if (vPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, iRef.Length,
                                    PredefinedErrorTypeNames.SyntaxError, "Self-inheritance is not allowed");
                        }
                    }
                }

                // :i18n:COUNT_FIELD → validate count field and plural maps
                if (node.Markers.Contains("i18n") && parsed.Mode == "active")
                {
                    var countField = GetMarkerSingleArg(node, "i18n");
                    if (!string.IsNullOrEmpty(countField))
                    {
                        if (!parsed.KeyMap.ContainsKey(countField))
                        {
                            int vPos = lines[node.Line].LastIndexOf(countField);
                            if (vPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + vPos, countField.Length,
                                    PredefinedErrorTypeNames.Warning, $"Count field '{countField}' used in :i18n is not defined");
                        }

                        foreach (var langNode in node.Children)
                        {
                            if (langNode.Children.Count == 0) continue;
                            bool hasOther = langNode.Children.Any(c => c.Key == "other");
                            if (!hasOther)
                            {
                                var langLine = snapshot.GetLineFromLineNumber(langNode.Line);
                                yield return MakeTag(snapshot, langLine.Start.Position + langNode.Column, langNode.Key.Length,
                                    PredefinedErrorTypeNames.Warning, $"Plural map for '{langNode.Key}' should include 'other'");
                            }
                        }
                    }
                }

                // :spam:MAX[:WINDOW] → validate rate-limit arguments
                if (node.Markers.Contains("spam") && parsed.Mode == "active")
                {
                    var spamIdx = node.Markers.IndexOf("spam");
                    var limitRaw = spamIdx + 1 < node.Markers.Count ? node.Markers[spamIdx + 1] : null;
                    var windowRaw = spamIdx + 2 < node.Markers.Count ? node.Markers[spamIdx + 2] : null;

                    if (string.IsNullOrEmpty(limitRaw) || !double.TryParse(limitRaw, out var limit) || limit <= 0)
                    {
                        int mPos = lines[node.Line].IndexOf(":spam");
                        if (mPos >= 0)
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + mPos, lines[node.Line].Length - mPos,
                                PredefinedErrorTypeNames.Warning, "Invalid :spam syntax. Use :spam:MAX_CALLS[:WINDOW_SEC]");
                    }

                    if (!string.IsNullOrEmpty(windowRaw))
                    {
                        if (!double.TryParse(windowRaw, out var window) || window <= 0)
                        {
                            int wPos = lines[node.Line].IndexOf(windowRaw);
                            if (wPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + wPos, windowRaw.Length,
                                    PredefinedErrorTypeNames.Warning, "WINDOW_SEC in :spam must be a positive number");
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

                    // Constraints in static mode
                    if (parsed.Mode != "active")
                    {
                        int bPos = lines[node.Line].IndexOf("[");
                        if (bPos >= 0)
                        {
                            yield return MakeTag(snapshot, snapshotLine.Start.Position + bPos, constraint.Length + 2,
                                PredefinedErrorTypeNames.HintedSuggestion, "Constraints require \"!active\" mode");
                            break; // Only warn once per node
                        }
                    }

                    // min/max must be numeric
                    if ((cName == "min" || cName == "max") && constraint.Contains(":"))
                    {
                        var numStr = constraint.Substring(constraint.IndexOf(':') + 1).Trim();
                        if (!string.IsNullOrEmpty(numStr) && !double.TryParse(numStr, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out _))
                        {
                            int cPos = lines[node.Line].IndexOf(constraint);
                            if (cPos >= 0)
                                yield return MakeTag(snapshot, snapshotLine.Start.Position + cPos, constraint.Length,
                                    PredefinedErrorTypeNames.SyntaxError, $"\"{cName}\" constraint requires a number");
                        }
                    }

                    // enum validation
                    if (cName == "enum" && constraint.Contains(":") && parsed.Mode == "active")
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

        private static HashSet<int> CollectMarkerArgIndexes(List<string> markers)
        {
            var result = new HashSet<int>();
            for (int i = 0; i < markers.Count; i++)
            {
                var marker = markers[i];
                if (!ArgMarkers.Contains(marker)) continue;

                if (marker.Equals("inherit", StringComparison.OrdinalIgnoreCase))
                {
                    for (int j = i + 1; j < markers.Count; j++)
                    {
                        if (KnownMarkers.Contains(markers[j])) break;
                        result.Add(j);
                    }
                    continue;
                }

                if (i + 1 < markers.Count && !KnownMarkers.Contains(markers[i + 1]))
                    result.Add(i + 1);
            }
            return result;
        }

        private static List<string> GetInheritRefs(SynxNode node)
        {
            var idx = node.Markers.IndexOf("inherit");
            if (idx == -1) return new List<string>();

            var refs = new List<string>();
            for (int i = idx + 1; i < node.Markers.Count; i++)
            {
                if (KnownMarkers.Contains(node.Markers[i])) break;
                refs.Add(node.Markers[i]);
            }

            if (refs.Count > 0) return refs;

            var raw = node.RawValue?.Trim() ?? "";
            if (string.IsNullOrEmpty(raw)) return new List<string>();
            return raw.Split(new[] { ' ' }, StringSplitOptions.RemoveEmptyEntries).ToList();
        }

        private static string GetMarkerSingleArg(SynxNode node, string marker)
        {
            var idx = node.Markers.IndexOf(marker);
            if (idx == -1) return "";
            if (idx + 1 < node.Markers.Count)
            {
                var inlineArg = node.Markers[idx + 1];
                if (!KnownMarkers.Contains(inlineArg))
                    return inlineArg;
            }
            return node.RawValue?.Trim() ?? "";
        }

        private static string BuildPath(SynxNode node)
        {
            var parts = new List<string>();
            var current = node;
            while (current != null && !current.IsListItem)
            {
                parts.Insert(0, current.Key);
                current = current.Parent;
            }
            return string.Join(".", parts);
        }
    }
}
