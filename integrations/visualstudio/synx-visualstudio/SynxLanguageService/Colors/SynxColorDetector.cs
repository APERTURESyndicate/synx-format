using System;
using System.Collections.Generic;
using System.ComponentModel.Composition;
using System.Text.RegularExpressions;
using System.Windows.Media;
using Microsoft.VisualStudio.Text;
using Microsoft.VisualStudio.Text.Editor;
using Microsoft.VisualStudio.Text.Tagging;
using Microsoft.VisualStudio.Utilities;

namespace SynxLanguageService.Colors
{
    /// <summary>
    /// Detects #hex color values in SYNX files and provides color adornments.
    /// In Visual Studio, color adornments are handled via editor adornments or ITagger with custom tags.
    /// This provides the detection logic that can be used by adornment layers.
    /// </summary>
    public static class SynxColorDetector
    {
        private static readonly Regex HexColorRe = new(@"#([0-9a-fA-F]{3,8})\b", RegexOptions.Compiled);

        public static IEnumerable<(int start, int length, Color color)> DetectColors(string line, int lineOffset)
        {
            foreach (Match match in HexColorRe.Matches(line))
            {
                // Skip comment lines
                if (line.TrimStart().StartsWith("#")) yield break;

                var hex = match.Groups[1].Value;
                var color = ParseHexColor(hex);
                if (color.HasValue)
                {
                    yield return (lineOffset + match.Index, match.Length, color.Value);
                }
            }
        }

        public static Color? ParseHexColor(string hex)
        {
            byte r, g, b, a = 255;

            try
            {
                switch (hex.Length)
                {
                    case 3: // #RGB
                        r = (byte)(Convert.ToByte(hex.Substring(0, 1), 16) * 17);
                        g = (byte)(Convert.ToByte(hex.Substring(1, 1), 16) * 17);
                        b = (byte)(Convert.ToByte(hex.Substring(2, 1), 16) * 17);
                        break;
                    case 4: // #RGBA
                        r = (byte)(Convert.ToByte(hex.Substring(0, 1), 16) * 17);
                        g = (byte)(Convert.ToByte(hex.Substring(1, 1), 16) * 17);
                        b = (byte)(Convert.ToByte(hex.Substring(2, 1), 16) * 17);
                        a = (byte)(Convert.ToByte(hex.Substring(3, 1), 16) * 17);
                        break;
                    case 6: // #RRGGBB
                        r = Convert.ToByte(hex.Substring(0, 2), 16);
                        g = Convert.ToByte(hex.Substring(2, 2), 16);
                        b = Convert.ToByte(hex.Substring(4, 2), 16);
                        break;
                    case 8: // #RRGGBBAA
                        r = Convert.ToByte(hex.Substring(0, 2), 16);
                        g = Convert.ToByte(hex.Substring(2, 2), 16);
                        b = Convert.ToByte(hex.Substring(4, 2), 16);
                        a = Convert.ToByte(hex.Substring(6, 2), 16);
                        break;
                    default:
                        return null;
                }
            }
            catch
            {
                return null;
            }

            return Color.FromArgb(a, r, g, b);
        }
    }
}
