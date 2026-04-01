using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.RegularExpressions;

namespace SynxLanguageService.Parser
{
    public class SynxNode
    {
        public string Key { get; set; } = "";
        public string RawValue { get; set; } = "";
        public object Value { get; set; }
        public int Line { get; set; }
        public int Column { get; set; }
        public int Indent { get; set; }
        public List<string> Markers { get; set; } = new();
        public Dictionary<string, string> MarkerArgs { get; set; } = new();
        public List<string> Constraints { get; set; } = new();
        public string TypeHint { get; set; } = "";
        public List<SynxNode> Children { get; set; } = new();
        public SynxNode Parent { get; set; }
        public bool IsListItem { get; set; }
    }

    public class ParsedDoc
    {
        public string Mode { get; set; } = "static";
        public int ModeLine { get; set; } = -1;
        public List<SynxNode> Nodes { get; set; } = new();
        public Dictionary<string, SynxNode> KeyMap { get; set; } = new();
        public List<SynxNode> AllNodes { get; set; } = new();
        public string[] Lines { get; set; } = Array.Empty<string>();
    }

    public static class SynxParser
    {
        private static readonly Regex ModeRe = new(@"^!(active|static)\s*$");
        private static readonly Regex ListRe = new(@"^(\s*)-\s+(.*)$");
        private static readonly Regex KeyRe = new(@"^(\s*)([^\s\[:#/!\-(][^\s\[:(]*)(?:\((\w+)\))?(?:\[([^\]]*)\])?((?::[a-zA-Z_]\w*)*)(?:\s+(.*))?$");

        public static ParsedDoc Parse(string text)
        {
            var doc = new ParsedDoc();
            var lines = text.Split(new[] { "\r\n", "\n" }, StringSplitOptions.None);
            doc.Lines = lines;

            var stack = new Stack<(int indent, SynxNode node)>();

            for (int i = 0; i < lines.Length; i++)
            {
                var line = lines[i];
                var trimmed = line.TrimStart();

                if (string.IsNullOrWhiteSpace(trimmed)) continue;
                if (trimmed.StartsWith("#") || trimmed.StartsWith("//")) continue;

                var modeMatch = ModeRe.Match(trimmed);
                if (modeMatch.Success)
                {
                    doc.Mode = modeMatch.Groups[1].Value;
                    doc.ModeLine = i;
                    continue;
                }

                int indent = line.Length - line.TrimStart().Length;

                // Pop stack to find parent
                while (stack.Count > 0 && stack.Peek().indent >= indent)
                    stack.Pop();

                var parent = stack.Count > 0 ? stack.Peek().node : null;

                // List item
                var listMatch = ListRe.Match(line);
                if (listMatch.Success)
                {
                    var itemValue = listMatch.Groups[2].Value.Trim();
                    var listNode = new SynxNode
                    {
                        Key = $"[{(parent?.Children.Count(c => c.IsListItem) ?? 0)}]",
                        RawValue = itemValue,
                        Value = ParseValue(itemValue),
                        Line = i,
                        Column = indent,
                        Indent = indent,
                        Parent = parent,
                        IsListItem = true
                    };
                    doc.AllNodes.Add(listNode);
                    parent?.Children.Add(listNode);
                    continue;
                }

                // Key-value
                var keyMatch = KeyRe.Match(line);
                if (!keyMatch.Success) continue;

                var node = new SynxNode
                {
                    Key = keyMatch.Groups[2].Value,
                    TypeHint = keyMatch.Groups[3].Value,
                    Line = i,
                    Column = indent,
                    Indent = indent,
                    Parent = parent
                };

                // Parse constraints
                if (!string.IsNullOrEmpty(keyMatch.Groups[4].Value))
                {
                    node.Constraints = keyMatch.Groups[4].Value
                        .Split(',')
                        .Select(c => c.Trim())
                        .Where(c => !string.IsNullOrEmpty(c))
                        .ToList();
                }

                // Parse markers
                if (!string.IsNullOrEmpty(keyMatch.Groups[5].Value))
                {
                    var parts = keyMatch.Groups[5].Value.Split(':')
                        .Where(p => !string.IsNullOrEmpty(p)).ToList();

                    foreach (var part in parts)
                        node.Markers.Add(part);
                }

                var rawValue = keyMatch.Groups[6].Value?.Trim() ?? "";
                node.RawValue = rawValue;
                node.Value = string.IsNullOrEmpty(rawValue) ? null : ParseValue(rawValue, node.TypeHint);

                if (parent != null)
                    parent.Children.Add(node);
                else
                    doc.Nodes.Add(node);

                doc.AllNodes.Add(node);
                stack.Push((indent, node));

                // Build key map with dot path
                var path = BuildPath(node);
                doc.KeyMap[path] = node;
            }

            return doc;
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

        private static object ParseValue(string raw, string typeHint = "")
        {
            if (string.IsNullOrEmpty(raw)) return null;

            switch (typeHint?.ToLower())
            {
                case "int": return int.TryParse(raw, out var i) ? i : (object)raw;
                case "float": return double.TryParse(raw, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var f) ? f : (object)raw;
                case "bool": return raw.ToLower() == "true";
                case "string": return raw;
            }

            if (raw == "true" || raw == "false") return raw == "true";
            if (raw == "null") return null;
            if (int.TryParse(raw, out var iv)) return iv;
            if (double.TryParse(raw, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var dv)) return dv;
            return raw;
        }

        public static Dictionary<string, object> ResolveToObject(ParsedDoc doc)
        {
            var result = new Dictionary<string, object>();
            foreach (var node in doc.Nodes)
                result[node.Key] = ResolveNode(node);
            return result;
        }

        private static object ResolveNode(SynxNode node)
        {
            if (node.Children.Count == 0) return node.Value;

            if (node.Children.All(c => c.IsListItem))
                return node.Children.Select(c => c.Value).ToList();

            var obj = new Dictionary<string, object>();
            foreach (var child in node.Children.Where(c => !c.IsListItem))
                obj[child.Key] = ResolveNode(child);
            return obj;
        }

        public static double? SafeCalc(string expr, Dictionary<string, double> vars)
        {
            try
            {
                var tokens = Tokenize(expr, vars);
                int pos = 0;
                var result = ParseExpr(tokens, ref pos);
                return result;
            }
            catch
            {
                return null;
            }
        }

        private static List<(string type, double value)> Tokenize(string expr, Dictionary<string, double> vars)
        {
            var tokens = new List<(string type, double value)>();
            int i = 0;
            while (i < expr.Length)
            {
                if (char.IsWhiteSpace(expr[i])) { i++; continue; }

                if (char.IsDigit(expr[i]) || expr[i] == '.')
                {
                    int start = i;
                    while (i < expr.Length && (char.IsDigit(expr[i]) || expr[i] == '.')) i++;
                    if (double.TryParse(expr.Substring(start, i - start), System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out var n))
                        tokens.Add(("num", n));
                    continue;
                }

                if (char.IsLetter(expr[i]) || expr[i] == '_')
                {
                    int start = i;
                    while (i < expr.Length && (char.IsLetterOrDigit(expr[i]) || expr[i] == '_')) i++;
                    var name = expr.Substring(start, i - start);
                    if (vars.TryGetValue(name, out var v))
                        tokens.Add(("num", v));
                    continue;
                }

                switch (expr[i])
                {
                    case '+': tokens.Add(("op+", 0)); break;
                    case '-': tokens.Add(("op-", 0)); break;
                    case '*': tokens.Add(("op*", 0)); break;
                    case '/': tokens.Add(("op/", 0)); break;
                    case '%': tokens.Add(("op%", 0)); break;
                    case '(': tokens.Add(("(", 0)); break;
                    case ')': tokens.Add((")", 0)); break;
                }
                i++;
            }
            return tokens;
        }

        private static double ParseExpr(List<(string type, double value)> tokens, ref int pos)
        {
            var left = ParseTerm(tokens, ref pos);
            while (pos < tokens.Count && (tokens[pos].type == "op+" || tokens[pos].type == "op-"))
            {
                var op = tokens[pos].type; pos++;
                var right = ParseTerm(tokens, ref pos);
                left = op == "op+" ? left + right : left - right;
            }
            return left;
        }

        private static double ParseTerm(List<(string type, double value)> tokens, ref int pos)
        {
            var left = ParseFactor(tokens, ref pos);
            while (pos < tokens.Count && (tokens[pos].type == "op*" || tokens[pos].type == "op/" || tokens[pos].type == "op%"))
            {
                var op = tokens[pos].type; pos++;
                var right = ParseFactor(tokens, ref pos);
                left = op == "op*" ? left * right : op == "op/" ? (right != 0 ? left / right : 0) : left % right;
            }
            return left;
        }

        private static double ParseFactor(List<(string type, double value)> tokens, ref int pos)
        {
            if (pos >= tokens.Count) return 0;
            if (tokens[pos].type == "(")
            {
                pos++;
                var result = ParseExpr(tokens, ref pos);
                if (pos < tokens.Count && tokens[pos].type == ")") pos++;
                return result;
            }
            if (tokens[pos].type == "op-")
            {
                pos++;
                return -ParseFactor(tokens, ref pos);
            }
            if (tokens[pos].type == "num")
            {
                var v = tokens[pos].value;
                pos++;
                return v;
            }
            pos++;
            return 0;
        }
    }
}
