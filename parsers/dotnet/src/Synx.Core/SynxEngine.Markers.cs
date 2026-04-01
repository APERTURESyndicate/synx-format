using System.Globalization;
using System.Text;

namespace Synx;

public static partial class SynxEngine
{
    private static void ApplyMarkers(
        Dictionary<string, SynxValue> map,
        string key,
        SynxMeta meta,
        SynxValue rootPtr,
        SynxOptions options,
        string path,
        Dictionary<string, Dictionary<string, SynxMeta>> metadata,
        Dictionary<string, SynxValue> includes)
    {
        var markers = meta.Markers;

        if (markers.Contains("spam"))
        {
            var spamIdx = markers.IndexOf("spam");
            var maxCalls = spamIdx + 1 < markers.Count && int.TryParse(markers[spamIdx + 1], out var maxC) ? maxC : 0;
            var windowSec = spamIdx + 2 < markers.Count && ulong.TryParse(markers[spamIdx + 2], out var ws) ? ws : 1UL;
            if (maxCalls == 0)
            {
                map[key] = new SynxValue.Str("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]");
                return;
            }
            var target = map.TryGetValue(key, out var tv) ? ValueToString(tv) : key;
            var bucket = key + "::" + target;
            if (!AllowSpam(bucket, maxCalls, windowSec))
            {
                map[key] = new SynxValue.Str($"SPAM_ERR: '{target}' exceeded {maxCalls} calls per {windowSec}s");
                return;
            }
            if (map.TryGetValue(key, out var cur))
            {
                var t = ValueToString(cur);
                var resolved = DeepGet(rootPtr, t) ?? (map.TryGetValue(t, out var lm) ? lm : null);
                if (resolved != null)
                    map[key] = CloneValue(resolved);
            }
        }

        if (markers.Contains("include") || markers.Contains("import"))
        {
            if (map.TryGetValue(key, out var incVal) && incVal is SynxValue.Str fp)
            {
                var maxD = options.MaxIncludeDepth ?? DefaultMaxIncludeDepth;
                if (options.IncludeDepth >= maxD)
                {
                    map[key] = new SynxValue.Str($"INCLUDE_ERR: max include depth ({maxD}) exceeded");
                    return;
                }
                try
                {
                    var baseDir = options.BasePath ?? ".";
                    var full = JailPath(baseDir, fp.Value);
                    CheckFileSize(full);
                    var text = File.ReadAllText(full);
                    var parsed = SynxParserCore.Parse(text);
                    if (parsed.Mode == SynxMode.Active)
                    {
                        var child = CloneOptions(options);
                        child.IncludeDepth = options.IncludeDepth + 1;
                        child.BasePath = Path.GetDirectoryName(full) ?? ".";
                        Resolve(parsed, child);
                    }
                    map[key] = CloneValue(parsed.Root);
                }
                catch (Exception e)
                {
                    map[key] = new SynxValue.Str("INCLUDE_ERR: " + e.Message);
                }
            }
            return;
        }

        if (markers.Contains("env"))
        {
            if (map.TryGetValue(key, out var ev) && ev is SynxValue.Str varName)
            {
                string? envVal = null;
                if (options.Env != null && options.Env.TryGetValue(varName.Value, out var evm))
                    envVal = evm;
                else
                    envVal = Environment.GetEnvironmentVariable(varName.Value);
                var forceStr = meta.TypeHint == "string";
                var di = markers.IndexOf("default");
                if (!string.IsNullOrEmpty(envVal))
                {
                    map[key] = forceStr ? new SynxValue.Str(envVal) : CastPrimitive(envVal);
                }
                else if (di >= 0 && markers.Count > di + 1)
                {
                    var fallback = string.Join(":", markers.Skip(di + 1));
                    map[key] = forceStr ? new SynxValue.Str(fallback) : CastPrimitive(fallback);
                }
                else
                    map[key] = new SynxValue.Null();
            }
        }

        if (markers.Contains("random"))
        {
            if (map.TryGetValue(key, out var rv) && rv is SynxValue.Arr arr)
            {
                if (arr.Items.Count == 0)
                {
                    map[key] = new SynxValue.Null();
                    return;
                }
                if (meta.Args.Count > 0)
                {
                    var weights = meta.Args.Select(a => double.TryParse(a, CultureInfo.InvariantCulture, out var w) ? w : 0.0).ToList();
                    map[key] = WeightedPick(arr, weights);
                }
                else
                    map[key] = CloneValue(arr.Items[RandomInt(arr.Items.Count)]);
            }
        }

        if (markers.Contains("ref"))
        {
            if (map.TryGetValue(key, out var refVal) && refVal is SynxValue.Str tgt)
            {
                var resolved = DeepGet(rootPtr, tgt.Value)
                               ?? (map.TryGetValue(tgt.Value, out var local) ? local : null)
                               ?? new SynxValue.Null();
                if (markers.Contains("calc") && ValueAsNumber(resolved) is { } n)
                {
                    var calcIdx = markers.IndexOf("calc");
                    if (calcIdx + 1 < markers.Count)
                    {
                        var calcExpr = markers[calcIdx + 1];
                        if (calcExpr.Length > 0 && "+-*/%".Contains(calcExpr[0]))
                        {
                            var expr = FormatNumber(n) + " " + calcExpr;
                            if (SynxSafeCalc.TryEval(expr, out var result, out var err))
                            {
                                map[key] = result % 1 == 0 && Math.Abs(result) < long.MaxValue
                                    ? new SynxValue.Int((long)result)
                                    : new SynxValue.Float(result);
                            }
                            else
                                map[key] = new SynxValue.Str("CALC_ERR: " + err);
                        }
                        else
                            map[key] = CloneValue(resolved);
                    }
                    else
                        map[key] = CloneValue(resolved);
                }
                else
                    map[key] = CloneValue(resolved);
            }
        }

        if (markers.Contains("i18n"))
        {
            if (map.TryGetValue(key, out var iv) && iv is SynxValue.Obj trans)
            {
                var lang = options.Lang ?? "en";
                SynxValue val = trans.Map.TryGetValue(lang, out var lv) ? lv
                    : trans.Map.TryGetValue("en", out var en) ? en
                    : trans.Map.Values.FirstOrDefault() ?? new SynxValue.Null();

                var i18nIdx = markers.IndexOf("i18n");
                var countField = i18nIdx + 1 < markers.Count ? markers[i18nIdx + 1] : null;

                if (countField != null && val is SynxValue.Obj pluralForms)
                {
                    var countVal = (long)(map.TryGetValue(countField, out var cv) ? ValueAsNumber(cv) ?? 0 : ValueAsNumber(DeepGet(rootPtr, countField) ?? new SynxValue.Null()) ?? 0);
                    var cat = PluralCategory(lang, countVal);
                    var chosen = pluralForms.Map.TryGetValue(cat, out var c1) ? c1
                        : pluralForms.Map.TryGetValue("other", out var c2) ? c2
                        : pluralForms.Map.Values.FirstOrDefault() ?? new SynxValue.Null();
                    if (chosen is SynxValue.Str ps)
                        map[key] = new SynxValue.Str(ps.Value.Replace("{count}", countVal.ToString(CultureInfo.InvariantCulture)));
                    else
                        map[key] = CloneValue(chosen);
                }
                else
                    map[key] = CloneValue(val);
            }
        }

        if (markers.Contains("calc"))
        {
            if (map.TryGetValue(key, out var cv) && cv is SynxValue.Str expr0)
            {
                var expr = expr0.Value;
                if (expr.Length > MaxCalcExprLen)
                {
                    map[key] = new SynxValue.Str(
                        $"CALC_ERR: expression too long ({expr.Length} chars, max {MaxCalcExprLen})");
                    return;
                }
                var resolved = expr;
                if (rootPtr is SynxValue.Obj rootObj)
                {
                    foreach (var (rk, rv) in rootObj.Map)
                    {
                        if (ValueAsNumberString(rv) is { } ns)
                        {
                            resolved = ReplaceWord(resolved, rk, ns);
                            if (resolved.Length > MaxCalcResolvedLen)
                            {
                                map[key] = new SynxValue.Str(
                                    $"CALC_ERR: resolved expression too long (max {MaxCalcResolvedLen} bytes)");
                                return;
                            }
                        }
                    }
                }
                foreach (var (rk, rv) in map)
                {
                    if (rk == key) continue;
                    if (ValueAsNumberString(rv) is { } ns2)
                    {
                        resolved = ReplaceWord(resolved, rk, ns2);
                        if (resolved.Length > MaxCalcResolvedLen)
                        {
                            map[key] = new SynxValue.Str(
                                $"CALC_ERR: resolved expression too long (max {MaxCalcResolvedLen} bytes)");
                            return;
                        }
                    }
                }

                resolved = ResolveCalcDots(resolved, rootPtr);
                if (resolved.Length > MaxCalcResolvedLen)
                {
                    map[key] = new SynxValue.Str(
                        $"CALC_ERR: resolved expression too long (max {MaxCalcResolvedLen} bytes)");
                    return;
                }

                if (SynxSafeCalc.TryEval(resolved, out var cr, out var cerr))
                    map[key] = cr % 1 == 0 && Math.Abs(cr) < long.MaxValue
                        ? new SynxValue.Int((long)cr)
                        : new SynxValue.Float(cr);
                else
                    map[key] = new SynxValue.Str("CALC_ERR: " + (cerr ?? "error"));
            }
        }

        if (markers.Contains("alias"))
        {
            if (map.TryGetValue(key, out var av) && av is SynxValue.Str at)
            {
                var currentPath = path.Length == 0 ? key : path + "." + key;
                if (at.Value == key || at.Value == currentPath)
                    map[key] = new SynxValue.Str($"ALIAS_ERR: self-referential alias: {currentPath} → {at.Value}");
                else
                {
                    var targetVal = DeepGet(rootPtr, at.Value);
                    var dot = at.Value.LastIndexOf('.');
                    var targetParent = dot >= 0 ? at.Value[..dot] : "";
                    var targetKeyName = dot >= 0 ? at.Value[(dot + 1)..] : at.Value;
                    var targetHasAlias = metadata.TryGetValue(targetParent, out var tmm)
                                         && tmm.TryGetValue(targetKeyName, out var tm)
                                         && tm.Markers.Contains("alias");
                    var isCycle = targetHasAlias && targetVal is SynxValue.Str ts &&
                                  (ts.Value == key || ts.Value == currentPath);
                    if (isCycle)
                        map[key] = new SynxValue.Str(
                            $"ALIAS_ERR: circular alias detected: {currentPath} → {at.Value}");
                    else
                        map[key] = CloneValue(targetVal ?? new SynxValue.Null());
                }
            }
        }

        if (markers.Contains("secret"))
        {
            if (map.TryGetValue(key, out var sv))
                map[key] = new SynxValue.Secret(ValueToString(sv));
        }

        if (markers.Contains("unique"))
        {
            if (map.TryGetValue(key, out var uv) && uv is SynxValue.Arr ua)
            {
                var seen = new HashSet<string>(StringComparer.Ordinal);
                var uq = new List<SynxValue>();
                foreach (var item in ua.Items)
                {
                    var s = ValueToString(item);
                    if (seen.Add(s)) uq.Add(CloneValue(item));
                }
                map[key] = new SynxValue.Arr(uq);
            }
        }

        if (markers.Contains("geo"))
        {
            if (map.TryGetValue(key, out var gv) && gv is SynxValue.Arr ga)
            {
                var region = options.Region ?? "US";
                var prefix = region + " ";
                SynxValue? found = null;
                foreach (var item in ga.Items)
                {
                    if (item is SynxValue.Str gs && gs.Value.StartsWith(prefix, StringComparison.Ordinal))
                    {
                        found = new SynxValue.Str(gs.Value[prefix.Length..].Trim());
                        break;
                    }
                }
                if (found != null)
                    map[key] = found;
                else if (ga.Items.Count > 0 && ga.Items[0] is SynxValue.Str f0)
                {
                    var sp = f0.Value.IndexOf(' ');
                    map[key] = sp >= 0 ? new SynxValue.Str(f0.Value[(sp + 1)..].Trim()) : CloneValue(ga.Items[0]);
                }
                else
                    map[key] = new SynxValue.Null();
            }
        }

        if (markers.Contains("split"))
        {
            if (map.TryGetValue(key, out var spv) && spv is SynxValue.Str ss)
            {
                var si = markers.IndexOf("split");
                var sep = si + 1 < markers.Count ? DelimiterFromKeyword(markers[si + 1]) : ",";
                var items = ss.Value.Split(sep, StringSplitOptions.None)
                    .Select(x => x.Trim())
                    .Where(x => x.Length > 0)
                    .Select(CastPrimitive)
                    .ToList();
                map[key] = new SynxValue.Arr(items);
            }
        }

        if (markers.Contains("join"))
        {
            if (map.TryGetValue(key, out var jv) && jv is SynxValue.Arr ja)
            {
                var ji = markers.IndexOf("join");
                var sep = ji + 1 < markers.Count ? DelimiterFromKeyword(markers[ji + 1]) : ",";
                map[key] = new SynxValue.Str(string.Join(sep, ja.Items.Select(ValueToString)));
            }
        }

        if (markers.Contains("default") && !markers.Contains("env"))
        {
            var isEmpty = !map.TryGetValue(key, out var dv) || dv is SynxValue.Null
                          || dv is SynxValue.Str ds && ds.Value.Length == 0;
            if (isEmpty)
            {
                var di = markers.IndexOf("default");
                if (markers.Count > di + 1)
                {
                    var fallback = string.Join(":", markers.Skip(di + 1));
                    map[key] = meta.TypeHint == "string" ? new SynxValue.Str(fallback) : CastPrimitive(fallback);
                }
            }
        }

        if (markers.Contains("clamp"))
        {
            var ci = markers.IndexOf("clamp");
            var minS = ci + 1 < markers.Count ? markers[ci + 1] : "";
            var maxS = ci + 2 < markers.Count ? markers[ci + 2] : "";
            if (double.TryParse(minS, CultureInfo.InvariantCulture, out var lo) &&
                double.TryParse(maxS, CultureInfo.InvariantCulture, out var hi))
            {
                if (lo > hi)
                    map[key] = new SynxValue.Str($"CONSTRAINT_ERR: clamp min ({lo}) > max ({hi})");
                else if (map.TryGetValue(key, out var clv) && ValueAsNumber(clv) is { } cn)
                {
                    var cld = Math.Clamp(cn, lo, hi);
                    map[key] = cld % 1 == 0 && Math.Abs(cld) < long.MaxValue
                        ? new SynxValue.Int((long)cld)
                        : new SynxValue.Float(cld);
                }
            }
        }

        if (markers.Contains("round"))
        {
            var ri = markers.IndexOf("round");
            var dec = ri + 1 < markers.Count && int.TryParse(markers[ri + 1], out var dd) ? dd : 0;
            if (map.TryGetValue(key, out var rv) && ValueAsNumber(rv) is { } rn)
            {
                var factor = Math.Pow(10, dec);
                var rounded = Math.Round(rn * factor) / factor;
                map[key] = dec == 0 ? new SynxValue.Int((long)rounded) : new SynxValue.Float(rounded);
            }
        }

        if (markers.Contains("map"))
        {
            if (map.TryGetValue(key, out var mv) && mv is SynxValue.Arr ma)
            {
                var mi = markers.IndexOf("map");
                var sourceKey = mi + 1 < markers.Count ? markers[mi + 1] : "";
                string lookupVal;
                if (sourceKey.Length > 0)
                    lookupVal = ValueToString(DeepGet(rootPtr, sourceKey) ?? (map.TryGetValue(sourceKey, out var sm) ? sm : new SynxValue.Str("")));
                else
                    lookupVal = "";

                SynxValue? pick = null;
                foreach (var item in ma.Items)
                {
                    if (item is SynxValue.Str line)
                    {
                        var sp = line.Value.IndexOf(' ');
                        if (sp > 0 && line.Value[..sp].Trim() == lookupVal)
                        {
                            pick = CastPrimitive(line.Value[(sp + 1)..].Trim());
                            break;
                        }
                    }
                }
                map[key] = CloneValue(pick ?? new SynxValue.Null());
            }
        }

        if (markers.Contains("format"))
        {
            var fi = markers.IndexOf("format");
            var pat = fi + 1 < markers.Count ? markers[fi + 1] : "%s";
            if (map.TryGetValue(key, out var fv))
                map[key] = new SynxValue.Str(ApplyFormatPattern(pat, fv));
        }

        if (markers.Contains("fallback"))
        {
            var fbi = markers.IndexOf("fallback");
            var defv = fbi + 1 < markers.Count ? markers[fbi + 1] : "";
            var useFb = !map.TryGetValue(key, out var fbv) || fbv is SynxValue.Null
                || fbv is SynxValue.Str fbs && fbs.Value.Length == 0
                || fbv is SynxValue.Str pathStr && pathStr.Value.Length > 0 && !SafeFileExists(options.BasePath ?? ".", pathStr.Value);
            if (useFb && defv.Length > 0)
                map[key] = new SynxValue.Str(defv);
        }

        if (markers.Contains("once"))
        {
            var oi = markers.IndexOf("once");
            var gen = oi + 1 < markers.Count ? markers[oi + 1] : "uuid";
            var lockPath = Path.Combine(options.BasePath ?? ".", ".synx.lock");
            if (ReadLock(lockPath, key) is { } existing)
                map[key] = new SynxValue.Str(existing);
            else
            {
                var generated = gen switch
                {
                    "timestamp" => DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString(CultureInfo.InvariantCulture),
                    "random" => RandomInt(int.MaxValue).ToString(CultureInfo.InvariantCulture),
                    _ => GenerateUuid(),
                };
                WriteLock(lockPath, key, generated);
                map[key] = new SynxValue.Str(generated);
            }
        }

        if (markers.Contains("version"))
        {
            if (map.TryGetValue(key, out var vv) && vv is SynxValue.Str curv)
            {
                var vi = markers.IndexOf("version");
                var op = vi + 1 < markers.Count ? markers[vi + 1] : ">=";
                var req = vi + 2 < markers.Count ? markers[vi + 2] : "";
                map[key] = new SynxValue.Bool(CompareVersions(curv.Value, op, req));
            }
        }

        if (markers.Contains("watch"))
        {
            if (map.TryGetValue(key, out var wv) && wv is SynxValue.Str wp)
            {
                var maxD = options.MaxIncludeDepth ?? DefaultMaxIncludeDepth;
                if (options.IncludeDepth >= maxD)
                {
                    map[key] = new SynxValue.Str($"WATCH_ERR: max include depth ({maxD}) exceeded");
                    return;
                }
                try
                {
                    var full = JailPath(options.BasePath ?? ".", wp.Value);
                    CheckFileSize(full);
                    var wi = markers.IndexOf("watch");
                    var kp = wi + 1 < markers.Count ? markers[wi + 1] : null;
                    var content = File.ReadAllText(full);
                    var ext = Path.GetExtension(full).TrimStart('.');
                    map[key] = kp != null
                        ? CloneValue(ExtractFromContent(content, kp, ext) ?? new SynxValue.Null())
                        : new SynxValue.Str(content.Trim());
                }
                catch (Exception e)
                {
                    map[key] = new SynxValue.Str("WATCH_ERR: " + e.Message);
                }
            }
        }

        if (markers.Contains("prompt"))
        {
            var pi = markers.IndexOf("prompt");
            var label = pi + 1 < markers.Count ? markers[pi + 1] : key;
            if (map.TryGetValue(key, out var pv))
            {
                var synx = StringifyValue(pv, 0);
                map[key] = new SynxValue.Str(label + " (SYNX):\n```synx\n" + synx + "```");
            }
        }

        if (meta.Constraints is { } constr)
            ValidateConstraintsOnMap(map, key, constr);
    }

    private static string ResolveCalcDots(string resolved, SynxValue rootPtr)
    {
        var bytes = Encoding.UTF8.GetBytes(resolved);
        var len = bytes.Length;
        var dotResolved = new StringBuilder(resolved.Length);
        var i = 0;
        while (i < len)
        {
            if (IsWordCharByte(bytes[i]))
            {
                var start = i;
                var hasDot = false;
                while (i < len && (IsWordCharByte(bytes[i]) || bytes[i] == (byte)'.'))
                {
                    if (bytes[i] == (byte)'.') hasDot = true;
                    i++;
                }
                var token = resolved[start..i];
                if (hasDot && token.Contains('.'))
                {
                    if (DeepGet(rootPtr, token) is { } dv && ValueAsNumber(dv) is { } n)
                    {
                        dotResolved.Append(FormatNumber(n));
                        if (dotResolved.Length > MaxCalcResolvedLen)
                            return resolved;
                        continue;
                    }
                }
                dotResolved.Append(token);
            }
            else
            {
                dotResolved.Append((char)bytes[i]);
                i++;
            }
            if (dotResolved.Length > MaxCalcResolvedLen)
                return resolved;
        }
        return dotResolved.ToString();
    }
}
