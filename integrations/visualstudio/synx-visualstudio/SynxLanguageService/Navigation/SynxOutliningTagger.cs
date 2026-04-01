using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.Linq;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Editor;
using Microsoft.VisualStudio.Text.Outlining;
using Microsoft.VisualStudio.Text.Tagging;
using Microsoft.VisualStudio.Utilities;
using SynxLanguageService.Parser;

namespace SynxLanguageService.Navigation
{
    // ─── Outlining (Folding) ─────────────────────────────────────────────────

    [Export(typeof(ITaggerProvider))]
    [ContentType("synx")]
    [TagType(typeof(IOutliningRegionTag))]
    internal class SynxOutliningTaggerProvider : ITaggerProvider
    {
        public ITagger<T> CreateTagger<T>(ITextBuffer buffer) where T : ITag
        {
            return buffer.Properties.GetOrCreateSingletonProperty(() =>
                new SynxOutliningTagger(buffer)) as ITagger<T>;
        }
    }

    internal class SynxOutliningTagger : ITagger<IOutliningRegionTag>
    {
        private readonly ITextBuffer _buffer;

        public SynxOutliningTagger(ITextBuffer buffer)
        {
            _buffer = buffer;
            _buffer.Changed += (s, e) => TagsChanged?.Invoke(this,
                new SnapshotSpanEventArgs(new SnapshotSpan(e.After, 0, e.After.Length)));
        }

        public event EventHandler<SnapshotSpanEventArgs> TagsChanged;

        public IEnumerable<ITagSpan<IOutliningRegionTag>> GetTags(NormalizedSnapshotSpanCollection spans)
        {
            if (spans.Count == 0) yield break;

            var snapshot = spans[0].Snapshot;
            var text = snapshot.GetText();
            var parsed = SynxParser.Parse(text);

            foreach (var node in parsed.AllNodes)
            {
                if (node.IsListItem || node.Children.Count == 0) continue;

                var startLine = snapshot.GetLineFromLineNumber(node.Line);
                int lastChildLine = GetLastChildLine(node);
                if (lastChildLine <= node.Line) continue;

                var endLine = snapshot.GetLineFromLineNumber(Math.Min(lastChildLine, snapshot.LineCount - 1));
                var regionSpan = new SnapshotSpan(startLine.Start, endLine.End);

                yield return new TagSpan<IOutliningRegionTag>(
                    regionSpan,
                    new OutliningRegionTag(
                        isDefaultCollapsed: false,
                        isImplementation: true,
                        collapsedForm: $"{node.Key} ...",
                        collapsedHintForm: GetPreview(node, snapshot)));
            }
        }

        private static int GetLastChildLine(SynxNode node)
        {
            int max = node.Line;
            foreach (var child in node.Children)
            {
                max = Math.Max(max, child.Line);
                if (child.Children.Count > 0)
                    max = Math.Max(max, GetLastChildLine(child));
            }
            return max;
        }

        private static string GetPreview(SynxNode node, ITextSnapshot snapshot)
        {
            var lines = new List<string>();
            int startLine = node.Line;
            int endLine = Math.Min(GetLastChildLine(node), snapshot.LineCount - 1);
            int count = Math.Min(endLine - startLine + 1, 10);

            for (int i = startLine; i < startLine + count; i++)
                lines.Add(snapshot.GetLineFromLineNumber(i).GetText());

            if (endLine - startLine + 1 > 10)
                lines.Add($"  ... ({endLine - startLine + 1 - 10} more lines)");

            return string.Join(Environment.NewLine, lines);
        }
    }
}
