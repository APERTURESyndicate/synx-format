using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using SynxLanguageService.Parser;

namespace SynxLanguageService.Commands
{
    /// <summary>
    /// Command implementations for SYNX: Convert to JSON, JSON → SYNX, Freeze, Preview.
    /// These are called from the VS Package command handlers.
    /// </summary>
    public static class SynxCommands
    {
        /// <summary>
        /// Convert SYNX text to formatted JSON string.
        /// </summary>
        public static string ConvertToJson(string synxText)
        {
            var doc = SynxParser.Parse(synxText);
            var obj = SynxParser.ResolveToObject(doc);
            return JsonConvert.SerializeObject(obj, Newtonsoft.Json.Formatting.Indented);
        }

        /// <summary>
        /// Convert JSON string to SYNX text.
        /// </summary>
        public static string ConvertFromJson(string jsonText)
        {
            var obj = JsonConvert.DeserializeObject<JObject>(jsonText);
            var sb = new StringBuilder();
            SerializeToSynx(obj, sb, 0);
            return sb.ToString();
        }

        /// <summary>
        /// Freeze: resolve all active markers and produce a static SYNX output.
        /// </summary>
        public static string Freeze(string synxText)
        {
            var doc = SynxParser.Parse(synxText);
            var obj = SynxParser.ResolveToObject(doc);
            var sb = new StringBuilder();
            sb.AppendLine("!static");
            SerializeObjectToSynx(obj, sb, 0);
            return sb.ToString();
        }

        /// <summary>
        /// Get parsed JSON for preview.
        /// </summary>
        public static string Preview(string synxText)
        {
            return ConvertToJson(synxText);
        }

        private static void SerializeToSynx(JToken token, StringBuilder sb, int indent)
        {
            var prefix = new string(' ', indent);

            if (token is JObject obj)
            {
                foreach (var prop in obj.Properties())
                {
                    if (prop.Value is JObject childObj)
                    {
                        sb.AppendLine($"{prefix}{prop.Name}");
                        SerializeToSynx(childObj, sb, indent + 2);
                    }
                    else if (prop.Value is JArray arr)
                    {
                        sb.AppendLine($"{prefix}{prop.Name}");
                        foreach (var item in arr)
                        {
                            if (item is JObject itemObj)
                            {
                                sb.AppendLine($"{prefix}  - {itemObj.Properties().First().Name} {FormatValue(itemObj.Properties().First().Value)}");
                                foreach (var p in itemObj.Properties().Skip(1))
                                {
                                    sb.AppendLine($"{prefix}    {p.Name} {FormatValue(p.Value)}");
                                }
                            }
                            else
                            {
                                sb.AppendLine($"{prefix}  - {FormatValue(item)}");
                            }
                        }
                    }
                    else
                    {
                        sb.AppendLine($"{prefix}{prop.Name} {FormatValue(prop.Value)}");
                    }
                }
            }
        }

        private static void SerializeObjectToSynx(Dictionary<string, object> obj, StringBuilder sb, int indent)
        {
            var prefix = new string(' ', indent);

            foreach (var kvp in obj)
            {
                if (kvp.Value is Dictionary<string, object> childObj)
                {
                    sb.AppendLine($"{prefix}{kvp.Key}");
                    SerializeObjectToSynx(childObj, sb, indent + 2);
                }
                else if (kvp.Value is List<object> list)
                {
                    sb.AppendLine($"{prefix}{kvp.Key}");
                    foreach (var item in list)
                    {
                        if (item is Dictionary<string, object> dictItem)
                        {
                            var first = dictItem.First();
                            sb.AppendLine($"{prefix}  - {first.Key} {FormatObject(first.Value)}");
                            foreach (var p in dictItem.Skip(1))
                                sb.AppendLine($"{prefix}    {p.Key} {FormatObject(p.Value)}");
                        }
                        else
                        {
                            sb.AppendLine($"{prefix}  - {FormatObject(item)}");
                        }
                    }
                }
                else
                {
                    sb.AppendLine($"{prefix}{kvp.Key} {FormatObject(kvp.Value)}");
                }
            }
        }

        private static string FormatValue(JToken token)
        {
            return token.Type switch
            {
                JTokenType.Null => "null",
                JTokenType.Boolean => token.Value<bool>() ? "true" : "false",
                JTokenType.Integer => token.Value<long>().ToString(),
                JTokenType.Float => token.Value<double>().ToString(System.Globalization.CultureInfo.InvariantCulture),
                JTokenType.String => token.Value<string>(),
                _ => token.ToString()
            };
        }

        private static string FormatObject(object val)
        {
            if (val == null) return "null";
            if (val is bool b) return b ? "true" : "false";
            if (val is int i) return i.ToString();
            if (val is double d) return d.ToString(System.Globalization.CultureInfo.InvariantCulture);
            return val.ToString();
        }
    }
}
