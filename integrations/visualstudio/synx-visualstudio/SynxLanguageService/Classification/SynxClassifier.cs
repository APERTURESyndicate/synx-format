using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.Linq;
using System.Text.RegularExpressions;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Classification;
using Microsoft.VisualStudio.Text.Tagging;
using Microsoft.VisualStudio.Utilities;

namespace SynxLanguageService.Classification
{
    // ─── Content Type ────────────────────────────────────────────────────────

    internal static class SynxContentTypeDefinition
    {
        [Export]
        [Name("synx")]
        [BaseDefinition("code")]
        internal static ContentTypeDefinition SynxContentType = null;

        [Export]
        [FileExtension(".synx")]
        [ContentType("synx")]
        internal static FileExtensionToContentTypeDefinition SynxFileExtension = null;
    }

    // ─── Classification Types ────────────────────────────────────────────────

    internal static class SynxClassificationTypes
    {
        [Export]
        [Name("synx.key")]
        [BaseDefinition("identifier")]
        internal static ClassificationTypeDefinition KeyType = null;

        [Export]
        [Name("synx.value.string")]
        [BaseDefinition("string")]
        internal static ClassificationTypeDefinition StringValueType = null;

        [Export]
        [Name("synx.value.number")]
        [BaseDefinition("number")]
        internal static ClassificationTypeDefinition NumberValueType = null;

        [Export]
        [Name("synx.value.boolean")]
        [BaseDefinition("keyword")]
        internal static ClassificationTypeDefinition BooleanValueType = null;

        [Export]
        [Name("synx.value.null")]
        [BaseDefinition("keyword")]
        internal static ClassificationTypeDefinition NullValueType = null;

        [Export]
        [Name("synx.comment")]
        [BaseDefinition("comment")]
        internal static ClassificationTypeDefinition CommentType = null;

        [Export]
        [Name("synx.marker")]
        [BaseDefinition("keyword")]
        internal static ClassificationTypeDefinition MarkerType = null;

        [Export]
        [Name("synx.constraint")]
        [BaseDefinition("type")]
        internal static ClassificationTypeDefinition ConstraintType = null;

        [Export]
        [Name("synx.typecast")]
        [BaseDefinition("type")]
        internal static ClassificationTypeDefinition TypeCastType = null;

        [Export]
        [Name("synx.mode")]
        [BaseDefinition("keyword")]
        internal static ClassificationTypeDefinition ModeType = null;

        [Export]
        [Name("synx.listoperator")]
        [BaseDefinition("operator")]
        internal static ClassificationTypeDefinition ListOperatorType = null;

        [Export]
        [Name("synx.color")]
        [BaseDefinition("literal")]
        internal static ClassificationTypeDefinition ColorType = null;

        [Export]
        [Name("synx.placeholder")]
        [BaseDefinition("string")]
        internal static ClassificationTypeDefinition PlaceholderType = null;

        [Export]
        [Name("synx.section")]
        [BaseDefinition("type")]
        internal static ClassificationTypeDefinition SectionType = null;
    }

    // ─── Classifier Provider ─────────────────────────────────────────────────

    [Export(typeof(IClassifierProvider))]
    [ContentType("synx")]
    internal class SynxClassifierProvider : IClassifierProvider
    {
        [Import]
        internal IClassificationTypeRegistryService ClassificationRegistry = null;

        public IClassifier GetClassifier(ITextBuffer buffer)
        {
            return buffer.Properties.GetOrCreateSingletonProperty(() =>
                new SynxClassifier(buffer, ClassificationRegistry));
        }
    }

    // ─── Classifier ──────────────────────────────────────────────────────────

    internal class SynxClassifier : IClassifier
    {
        private readonly ITextBuffer _buffer;
        private readonly IClassificationTypeRegistryService _registry;

        private static readonly Regex ModeRe = new(@"^!(active|static)\s*$");
        private static readonly Regex CommentHashRe = new(@"^(\s*)#(.*)$");
        private static readonly Regex CommentSlashRe = new(@"^(\s*)//(.*)$");
        private static readonly Regex ListRe = new(@"^(\s*)(-)(\s+.*)$");
        private static readonly Regex KeyMarkerRe = new(@"^(\s*)([^\s\[:#/!\-(][^\s\[:(]*)(\(\w+\))?(\[[^\]]*\])?((?::[a-zA-Z_]\w*)+)\s+(.*)$");
        private static readonly Regex KeyTypeRe = new(@"^(\s*)([^\s\[:#/!\-(][^\s\[:(]*)(\(\w+\))(\[[^\]]*\])?\s+(.*)$");
        private static readonly Regex KeySimpleRe = new(@"^(\s*)([^\s\[:#/!\-(][^\s\[:(]*)(\[[^\]]*\])?\s+(.*)$");
        private static readonly Regex ParentKeyRe = new(@"^(\s*)([^\s\[:#/!\-(][^\s\[:(]*)\s*$");
        private static readonly Regex HexColorRe = new(@"#[0-9a-fA-F]{3,8}\b");
        private static readonly Regex PlaceholderRe = new(@"\{[^}]+\}");

        internal SynxClassifier(ITextBuffer buffer, IClassificationTypeRegistryService registry)
        {
            _buffer = buffer;
            _registry = registry;
        }

        public event EventHandler<ClassificationChangedEventArgs> ClassificationChanged;

        public IList<ClassificationSpan> GetClassificationSpans(SnapshotSpan span)
        {
            var result = new List<ClassificationSpan>();
            var snapshot = span.Snapshot;

            var startLine = snapshot.GetLineFromPosition(span.Start.Position).LineNumber;
            var endLine = snapshot.GetLineFromPosition(span.End.Position).LineNumber;

            for (int i = startLine; i <= endLine; i++)
            {
                var line = snapshot.GetLineFromLineNumber(i);
                var text = line.GetText();
                var trimmed = text.TrimStart();

                if (string.IsNullOrWhiteSpace(trimmed)) continue;

                // Mode declaration
                if (ModeRe.IsMatch(trimmed))
                {
                    result.Add(MakeSpan(snapshot, line.Start.Position, line.Length, "synx.mode"));
                    continue;
                }

                // Comments
                var commentHash = CommentHashRe.Match(text);
                if (commentHash.Success)
                {
                    int commentStart = commentHash.Groups[1].Length;
                    result.Add(MakeSpan(snapshot, line.Start.Position + commentStart, text.Length - commentStart, "synx.comment"));
                    continue;
                }
                var commentSlash = CommentSlashRe.Match(text);
                if (commentSlash.Success)
                {
                    int commentStart = commentSlash.Groups[1].Length;
                    result.Add(MakeSpan(snapshot, line.Start.Position + commentStart, text.Length - commentStart, "synx.comment"));
                    continue;
                }

                // List item
                var listMatch = ListRe.Match(text);
                if (listMatch.Success)
                {
                    int dashPos = listMatch.Groups[1].Length;
                    result.Add(MakeSpan(snapshot, line.Start.Position + dashPos, 1, "synx.listoperator"));

                    var valueText = listMatch.Groups[3].Value.Trim();
                    int valueStart = text.IndexOf(valueText, dashPos + 1);
                    if (valueStart >= 0)
                        ClassifyValue(result, snapshot, line.Start.Position + valueStart, valueText);
                    continue;
                }

                // Key with markers
                var keyMarker = KeyMarkerRe.Match(text);
                if (keyMarker.Success)
                {
                    ClassifyKeyLine(result, snapshot, line, keyMarker, hasMarkers: true, hasType: !string.IsNullOrEmpty(keyMarker.Groups[3].Value));
                    continue;
                }

                // Key with type
                var keyType = KeyTypeRe.Match(text);
                if (keyType.Success)
                {
                    int offset = keyType.Groups[1].Length;
                    result.Add(MakeSpan(snapshot, line.Start.Position + offset, keyType.Groups[2].Length, "synx.key"));
                    offset += keyType.Groups[2].Length;
                    result.Add(MakeSpan(snapshot, line.Start.Position + offset, keyType.Groups[3].Length, "synx.typecast"));
                    offset += keyType.Groups[3].Length;
                    if (!string.IsNullOrEmpty(keyType.Groups[4].Value))
                    {
                        result.Add(MakeSpan(snapshot, line.Start.Position + offset, keyType.Groups[4].Length, "synx.constraint"));
                        offset += keyType.Groups[4].Length;
                    }
                    var val5 = keyType.Groups[5].Value.Trim();
                    int valPos = text.LastIndexOf(val5);
                    if (valPos >= 0) ClassifyValue(result, snapshot, line.Start.Position + valPos, val5);
                    continue;
                }

                // Simple key-value
                var keySimple = KeySimpleRe.Match(text);
                if (keySimple.Success)
                {
                    int offset = keySimple.Groups[1].Length;
                    result.Add(MakeSpan(snapshot, line.Start.Position + offset, keySimple.Groups[2].Length, "synx.key"));
                    offset += keySimple.Groups[2].Length;
                    if (!string.IsNullOrEmpty(keySimple.Groups[3].Value))
                    {
                        result.Add(MakeSpan(snapshot, line.Start.Position + offset, keySimple.Groups[3].Length, "synx.constraint"));
                        offset += keySimple.Groups[3].Length;
                    }
                    var val4 = keySimple.Groups[4].Value.Trim();
                    int valPos = text.LastIndexOf(val4);
                    if (valPos >= 0) ClassifyValue(result, snapshot, line.Start.Position + valPos, val4);
                    continue;
                }

                // Parent key (no value)
                var parentKey = ParentKeyRe.Match(text);
                if (parentKey.Success)
                {
                    int offset = parentKey.Groups[1].Length;
                    result.Add(MakeSpan(snapshot, line.Start.Position + offset, parentKey.Groups[2].Length, "synx.section"));
                    continue;
                }
            }

            return result;
        }

        private void ClassifyKeyLine(List<ClassificationSpan> result, ITextSnapshot snapshot,
            ITextSnapshotLine line, Match match, bool hasMarkers, bool hasType)
        {
            var text = line.GetText();
            int offset = match.Groups[1].Length;

            // Key name
            result.Add(MakeSpan(snapshot, line.Start.Position + offset, match.Groups[2].Length, "synx.key"));
            offset += match.Groups[2].Length;

            // Type cast
            if (!string.IsNullOrEmpty(match.Groups[3].Value))
            {
                result.Add(MakeSpan(snapshot, line.Start.Position + offset, match.Groups[3].Length, "synx.typecast"));
                offset += match.Groups[3].Length;
            }

            // Constraints
            if (!string.IsNullOrEmpty(match.Groups[4].Value))
            {
                result.Add(MakeSpan(snapshot, line.Start.Position + offset, match.Groups[4].Length, "synx.constraint"));
                offset += match.Groups[4].Length;
            }

            // Markers
            if (hasMarkers && !string.IsNullOrEmpty(match.Groups[5].Value))
            {
                int markerStart = text.IndexOf(match.Groups[5].Value, offset);
                if (markerStart >= 0)
                    result.Add(MakeSpan(snapshot, line.Start.Position + markerStart, match.Groups[5].Length, "synx.marker"));
            }

            // Value
            if (!string.IsNullOrEmpty(match.Groups[6].Value))
            {
                var val = match.Groups[6].Value.Trim();
                int valPos = text.LastIndexOf(val);
                if (valPos >= 0) ClassifyValue(result, snapshot, line.Start.Position + valPos, val);
            }
        }

        private void ClassifyValue(List<ClassificationSpan> result, ITextSnapshot snapshot, int start, string value)
        {
            if (value == "true" || value == "false")
            {
                result.Add(MakeSpan(snapshot, start, value.Length, "synx.value.boolean"));
                return;
            }
            if (value == "null")
            {
                result.Add(MakeSpan(snapshot, start, value.Length, "synx.value.null"));
                return;
            }
            if (Regex.IsMatch(value, @"^-?\d+(\.\d+)?$"))
            {
                result.Add(MakeSpan(snapshot, start, value.Length, "synx.value.number"));
                return;
            }

            // Check for hex colors
            var hexMatch = HexColorRe.Match(value);
            if (hexMatch.Success)
            {
                result.Add(MakeSpan(snapshot, start + hexMatch.Index, hexMatch.Length, "synx.color"));
                return;
            }

            // Check for placeholders
            var placeholders = PlaceholderRe.Matches(value);
            if (placeholders.Count > 0)
            {
                int lastEnd = 0;
                foreach (Match ph in placeholders)
                {
                    if (ph.Index > lastEnd)
                        result.Add(MakeSpan(snapshot, start + lastEnd, ph.Index - lastEnd, "synx.value.string"));
                    result.Add(MakeSpan(snapshot, start + ph.Index, ph.Length, "synx.placeholder"));
                    lastEnd = ph.Index + ph.Length;
                }
                if (lastEnd < value.Length)
                    result.Add(MakeSpan(snapshot, start + lastEnd, value.Length - lastEnd, "synx.value.string"));
                return;
            }

            result.Add(MakeSpan(snapshot, start, value.Length, "synx.value.string"));
        }

        private ClassificationSpan MakeSpan(ITextSnapshot snapshot, int start, int length, string type)
        {
            if (start + length > snapshot.Length) length = snapshot.Length - start;
            if (start < 0 || length <= 0) return null;
            var span = new SnapshotSpan(snapshot, start, length);
            var classificationType = _registry.GetClassificationType(type);
            return new ClassificationSpan(span, classificationType);
        }
    }
}
