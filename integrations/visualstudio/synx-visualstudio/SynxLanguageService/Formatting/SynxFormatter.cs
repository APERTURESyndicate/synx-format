using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.Linq;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Editor;
using Microsoft.VisualStudio.Utilities;
using SynxLanguageService.Parser;

namespace SynxLanguageService.Formatting
{
    /// <summary>
    /// Formats a SYNX document: normalizes indentation to 2-space, trims trailing whitespace, fixes tabs.
    /// </summary>
    public static class SynxFormatter
    {
        public static string Format(string text)
        {
            var lines = text.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None);
            var result = new List<string>();

            foreach (var line in lines)
            {
                if (string.IsNullOrWhiteSpace(line))
                {
                    result.Add("");
                    continue;
                }

                var formatted = line;

                // Replace tabs with 2 spaces
                if (formatted.Contains('\t'))
                {
                    int leadingEnd = 0;
                    while (leadingEnd < formatted.Length && char.IsWhiteSpace(formatted[leadingEnd]))
                        leadingEnd++;
                    var leading = formatted.Substring(0, leadingEnd).Replace("\t", "  ");
                    formatted = leading + formatted.Substring(leadingEnd);
                }

                // Normalize indentation to multiple of 2
                int spaces = 0;
                while (spaces < formatted.Length && formatted[spaces] == ' ')
                    spaces++;

                if (spaces % 2 != 0)
                {
                    int corrected = (int)Math.Round((double)spaces / 2) * 2;
                    formatted = new string(' ', corrected) + formatted.TrimStart();
                }

                // Trim trailing whitespace
                formatted = formatted.TrimEnd();

                result.Add(formatted);
            }

            return string.Join(Environment.NewLine, result);
        }
    }
}
