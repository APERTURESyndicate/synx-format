using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.Linq;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Editor;
using Microsoft.VisualStudio.Text.Tagging;
using Microsoft.VisualStudio.Utilities;
using SynxLanguageService.Parser;

namespace SynxLanguageService.InlayHints
{
    /// <summary>
    /// Provides inlay hints for :calc expressions showing computed values.
    /// Uses ITagger with a custom InlayHintTag to display "= 500" after calc lines.
    /// </summary>
    public static class SynxInlayHintProvider
    {
        public static IEnumerable<(int line, string hint, string tooltip)> GetCalcHints(string text)
        {
            var parsed = SynxParser.Parse(text);
            if (parsed.Mode != "active") yield break;

            // Build variable map
            var vars = new Dictionary<string, double>();
            foreach (var kvp in parsed.KeyMap)
            {
                if (kvp.Value.Value is int iv)
                {
                    vars[kvp.Key] = iv;
                    var shortName = kvp.Key.Split('.').Last();
                    if (!vars.ContainsKey(shortName)) vars[shortName] = iv;
                }
                else if (kvp.Value.Value is double dv)
                {
                    vars[kvp.Key] = dv;
                    var shortName = kvp.Key.Split('.').Last();
                    if (!vars.ContainsKey(shortName)) vars[shortName] = dv;
                }
            }

            foreach (var node in parsed.AllNodes)
            {
                if (!node.Markers.Contains("calc") || string.IsNullOrEmpty(node.RawValue))
                    continue;

                var result = SynxParser.SafeCalc(node.RawValue.Trim(), vars);
                if (result.HasValue && !double.IsNaN(result.Value))
                {
                    var formatted = result.Value == Math.Floor(result.Value)
                        ? ((long)result.Value).ToString()
                        : result.Value.ToString("0.####");

                    yield return (node.Line, $" = {formatted}", $"Computed from: {node.RawValue.Trim()}");
                }
            }
        }
    }
}
