using System.Globalization;
using System.Text;

namespace Synx;

public static partial class SynxEngine
{
    /// <summary>Bits for the built-in markers <see cref="ApplyMarkers"/> dispatches on.</summary>
    [Flags]
    private enum Mk
    {
        None = 0,
        Alias = 1 << 0, Calc = 1 << 1, Clamp = 1 << 2, Default = 1 << 3, Env = 1 << 4,
        Fallback = 1 << 5, Format = 1 << 6, Geo = 1 << 7, I18n = 1 << 8, Import = 1 << 9,
        Include = 1 << 10, Join = 1 << 11, Map = 1 << 12, Once = 1 << 13, Prompt = 1 << 14,
        Random = 1 << 15, Ref = 1 << 16, Replace = 1 << 17, Round = 1 << 18, Secret = 1 << 19,
        Sort = 1 << 20, Spam = 1 << 21, Split = 1 << 22, Sum = 1 << 23, Unique = 1 << 24,
        Version = 1 << 25, Watch = 1 << 26,
    }

    /// <summary>Classify a marker chain once (perf: one pass instead of a scan per marker).</summary>
    private static Mk MarkerBits(List<string> markers)
    {
        var bits = Mk.None;
        foreach (var m in markers)
        {
            bits |= m switch
            {
                "alias" => Mk.Alias, "calc" => Mk.Calc, "clamp" => Mk.Clamp, "default" => Mk.Default,
                "env" => Mk.Env, "fallback" => Mk.Fallback, "format" => Mk.Format, "geo" => Mk.Geo,
                "i18n" => Mk.I18n, "import" => Mk.Import, "include" => Mk.Include, "join" => Mk.Join,
                "map" => Mk.Map, "once" => Mk.Once, "prompt" => Mk.Prompt, "random" => Mk.Random,
                "ref" => Mk.Ref, "replace" => Mk.Replace, "round" => Mk.Round, "secret" => Mk.Secret,
                "sort" => Mk.Sort, "spam" => Mk.Spam, "split" => Mk.Split, "sum" => Mk.Sum,
                "unique" => Mk.Unique, "version" => Mk.Version, "watch" => Mk.Watch,
                _ => Mk.None,
            };
        }
        return bits;
    }

    private static string? Arg(List<string> markers, string name, int offset)
    {
        var i = markers.IndexOf(name) + offset;
        return i < markers.Count ? markers[i] : null;
    }

    private static void ApplyMarkers(Ctx ctx, Dictionary<string, SynxValue> map, string key, SynxMeta meta, string path)
    {
        var markers = meta.Markers;
        var options = ctx.Options;
        var root = ctx.Root;
        var mb = MarkerBits(markers);

        // ── :spam ── key:spam:MAX_CALLS[:WINDOW_SEC] target
        if ((mb & Mk.Spam) != 0)
        {
            var maxCalls = Arg(markers, "spam", 1) is { } ms && SynxNum.TryParseU64(ms, ulong.MaxValue, out var mc) ? mc : 0;
            var windowSec = Arg(markers, "spam", 2) is { } ws && SynxNum.TryParseU64(ws, ulong.MaxValue, out var w) ? w : 1;
            if (maxCalls == 0)
            {
                map[key] = new SynxValue.Str("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]");
                return;
            }
            var target = map.TryGetValue(key, out var tv) ? ValueToString(tv) : key;
            if (!AllowSpam(key + "::" + target, maxCalls, windowSec))
            {
                map[key] = new SynxValue.Str($"SPAM_ERR: '{target}' exceeded {maxCalls} calls per {windowSec}s");
                return;
            }
            if (map.TryGetValue(key, out var cur))
            {
                var t = ValueToString(cur);
                var resolved = DeepGet(root, t) ?? (map.TryGetValue(t, out var lm) ? lm : null);
                if (resolved != null)
                    map[key] = CloneValue(resolved);
            }
        }

        // ── :include / :import ──
        if ((mb & (Mk.Include | Mk.Import)) != 0)
        {
            if (map.TryGetValue(key, out var incVal) && incVal is SynxValue.Str fp)
            {
                var maxD = options.MaxIncludeDepth ?? DefaultMaxIncludeDepth;
                if (options.IncludeDepth >= maxD)
                {
                    map[key] = new SynxValue.Str($"INCLUDE_ERR: max include depth ({maxD}) exceeded");
                    return;
                }
                // `:include` with `:secret` masks the leaves of the included tree.
                var isSecret = (mb & Mk.Secret) != 0;
                if (path.Length == 0 && ctx.Preresolved.TryGetValue(key, out var pre) && pre.Path == fp.Value)
                {
                    map[key] = isSecret ? MaskSecretValue(CloneValue(pre.Root)) : pre.Root;
                    return;
                }
                try
                {
                    var full = JailPath(options.BasePath ?? ".", fp.Value);
                    CheckFileSize(full);
                    var includedRoot = ReadAndResolve(full, fp.Value, options).Root;
                    map[key] = isSecret ? MaskSecretValue(includedRoot) : includedRoot;
                }
                catch (Exception e)
                {
                    map[key] = new SynxValue.Str("INCLUDE_ERR: " + e.Message);
                }
            }
            return;
        }

        // ── :env ──
        if ((mb & Mk.Env) != 0)
        {
            if (map.TryGetValue(key, out var ev) && ev is SynxValue.Str varName)
            {
                // Options.Env, when given, is the whole environment.
                string? envVal;
                if (options.Env != null)
                    envVal = options.Env.TryGetValue(varName.Value, out var evm) ? evm : null;
                else
                    envVal = Environment.GetEnvironmentVariable(varName.Value);
                var forceStr = WantsString(meta);
                var di = markers.IndexOf("default");
                if (!string.IsNullOrEmpty(envVal))
                    map[key] = forceStr ? new SynxValue.Str(envVal) : CastPrimitive(envVal);
                else if (di >= 0 && markers.Count > di + 1)
                {
                    // All parts after `default`, joined back: IPs (0.0.0.0) and URLs survive.
                    var fallback = string.Join(":", markers.Skip(di + 1));
                    map[key] = forceStr ? new SynxValue.Str(fallback) : CastPrimitive(fallback);
                }
                else
                    map[key] = new SynxValue.Null();
            }
        }

        // ── :random ──
        if ((mb & Mk.Random) != 0)
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
                    var weights = new List<double>();
                    foreach (var a in meta.Args)
                        if (SynxNum.TryParseF64(a, out var wt)) weights.Add(wt);
                    map[key] = WeightedPick(arr.Items, weights);
                }
                else
                    map[key] = CloneValue(arr.Items[RandomInt(arr.Items.Count)]);
            }
        }

        // ── :ref ── like :alias, but the value feeds the markers after it (:ref:calc:*2)
        if ((mb & Mk.Ref) != 0)
        {
            if (map.TryGetValue(key, out var refVal) && refVal is SynxValue.Str tgt)
            {
                var resolved = DeepGet(root, tgt.Value)
                               ?? (map.TryGetValue(tgt.Value, out var local) ? local : null)
                               ?? new SynxValue.Null();
                SynxValue result = CloneValue(resolved);
                if ((mb & Mk.Calc) != 0 && ValueAsNumber(resolved) is { } n
                    && Arg(markers, "calc", 1) is { Length: > 0 } calcExpr && "+-*/%".Contains(calcExpr[0]))
                {
                    var expr = SynxNum.FormatNumber(n) + " " + calcExpr;
                    result = SynxSafeCalc.TryEval(expr, out var r, out var err)
                        ? NumberValue(r)
                        : new SynxValue.Str("CALC_ERR: " + err);
                }
                map[key] = result;
            }
        }

        // ── :i18n ── key:i18n[:COUNT_FIELD]
        if ((mb & Mk.I18n) != 0)
        {
            if (map.TryGetValue(key, out var iv) && iv is SynxValue.Obj trans)
            {
                var lang = options.Lang ?? "en";
                var val = trans.Map.TryGetValue(lang, out var lv) ? lv
                    : trans.Map.TryGetValue("en", out var en) ? en
                    : trans.Map.Count > 0 ? trans.Map.Values.First() : new SynxValue.Null();

                if (Arg(markers, "i18n", 1) is { } countField && val is SynxValue.Obj pluralForms)
                {
                    double? countNum = map.TryGetValue(countField, out var cv) ? ValueAsNumber(cv) : null;
                    if (countNum == null && DeepGet(root, countField) is { } rc) countNum = ValueAsNumber(rc);
                    var countVal = SynxNum.SaturatingI64(countNum ?? 0);
                    var cat = PluralCategory(lang, countVal);
                    var chosen = pluralForms.Map.TryGetValue(cat, out var c1) ? c1
                        : pluralForms.Map.TryGetValue("other", out var c2) ? c2
                        : pluralForms.Map.Count > 0 ? pluralForms.Map.Values.First() : new SynxValue.Null();
                    map[key] = chosen is SynxValue.Str ps
                        ? new SynxValue.Str(ps.Value.Replace("{count}", countVal.ToString(CultureInfo.InvariantCulture)))
                        : CloneValue(chosen);
                }
                else
                    map[key] = CloneValue(val);
            }
        }

        // ── :calc ──
        if ((mb & Mk.Calc) != 0)
        {
            if (map.TryGetValue(key, out var cv) && cv is SynxValue.Str exprVal)
            {
                var expr = exprVal.Value;
                var exprBytes = SynxNum.Utf8Len(expr);
                if (exprBytes > MaxCalcExprLen)
                {
                    map[key] = new SynxValue.Str($"CALC_ERR: expression too long ({exprBytes} chars, max {MaxCalcExprLen})");
                    return;
                }
                var rootMap = (root as SynxValue.Obj)?.Map;
                var resolved = SubstituteCalcVars(expr, map, key, rootMap, ctx.RootSpecial);
                if (resolved != null)
                    resolved = ResolveCalcDots(resolved, root);
                if (resolved == null)
                {
                    map[key] = new SynxValue.Str($"CALC_ERR: resolved expression too long (max {MaxCalcResolvedLen} bytes)");
                    return;
                }
                map[key] = SynxSafeCalc.TryEval(resolved, out var cr, out var cerr)
                    ? NumberValue(cr)
                    : new SynxValue.Str("CALC_ERR: " + cerr);
            }
        }

        // ── :alias ──
        if ((mb & Mk.Alias) != 0)
        {
            if (map.TryGetValue(key, out var av) && av is SynxValue.Str at)
            {
                var target = at.Value;
                // A list item's `#<index>` segment is a metadata detail, not part of a path
                // a document can name.
                var sb = new StringBuilder(path.Length + key.Length + 1);
                foreach (var seg in SynxParserCore.MetaPathSegments(path))
                {
                    if (seg.StartsWith('#')) continue;
                    sb.Append(seg).Append('.');
                }
                sb.Append(key);
                var currentPath = sb.ToString();
                if (target == key || target == currentPath)
                    map[key] = new SynxValue.Str($"ALIAS_ERR: self-referential alias: {currentPath} → {target}");
                else
                {
                    var targetVal = DeepGet(root, target);
                    var dot = target.LastIndexOf('.');
                    var targetParent = dot >= 0 ? target[..dot] : "";
                    var targetKeyName = dot >= 0 ? target[(dot + 1)..] : target;
                    var targetHasAlias = ctx.Metadata.TryGetValue(targetParent, out var tmm)
                                         && tmm.TryGetValue(targetKeyName, out var tm)
                                         && tm.Markers.Contains("alias");
                    var isCycle = targetHasAlias && targetVal is SynxValue.Str ts &&
                                  (ts.Value == key || ts.Value == currentPath);
                    if (isCycle)
                    {
                        // Order-independent message: participants sorted.
                        var (a, b) = CompareCodePoints(currentPath, target) <= 0 ? (currentPath, target) : (target, currentPath);
                        map[key] = new SynxValue.Str($"ALIAS_ERR: circular alias detected: {a} → {b}");
                    }
                    else
                        map[key] = targetVal != null ? CloneValue(targetVal) : new SynxValue.Null();
                }
            }
        }

        // ── :unique ──
        if ((mb & Mk.Unique) != 0)
        {
            if (map.TryGetValue(key, out var uv) && uv is SynxValue.Arr ua)
            {
                var seen = new HashSet<string>(StringComparer.Ordinal);
                var uq = new List<SynxValue>();
                foreach (var item in ua.Items)
                {
                    if (seen.Add(ValueToString(item))) uq.Add(CloneValue(item));
                }
                map[key] = new SynxValue.Arr(uq);
            }
        }

        // ── :geo ──
        if ((mb & Mk.Geo) != 0)
        {
            if (map.TryGetValue(key, out var gv) && gv is SynxValue.Arr ga)
            {
                var prefix = (options.Region ?? "US") + " ";
                SynxValue? result = null;
                foreach (var item in ga.Items)
                {
                    if (item is SynxValue.Str gs && gs.Value.StartsWith(prefix, StringComparison.Ordinal))
                    {
                        result = new SynxValue.Str(gs.Value[prefix.Length..].Trim());
                        break;
                    }
                }
                if (result == null)
                {
                    if (ga.Items.Count == 0)
                        result = new SynxValue.Null();
                    else if (ga.Items[0] is SynxValue.Str f0 && f0.Value.IndexOf(' ') is var sp and >= 0)
                        result = new SynxValue.Str(f0.Value[(sp + 1)..].Trim());
                    else
                        result = CloneValue(ga.Items[0]);
                }
                map[key] = result;
            }
        }

        // ── :split ──
        if ((mb & Mk.Split) != 0)
        {
            if (map.TryGetValue(key, out var spv) && spv is SynxValue.Str ss)
            {
                var sep = Arg(markers, "split", 1) is { } sk ? DelimiterFromKeyword(sk) : ",";
                var items = new List<SynxValue>();
                foreach (var p in RustSplit(ss.Value, sep))
                {
                    var t = p.Trim();
                    if (t.Length > 0) items.Add(CastPrimitive(t));
                }
                map[key] = new SynxValue.Arr(items);
            }
        }

        // ── :join ──
        if ((mb & Mk.Join) != 0)
        {
            if (map.TryGetValue(key, out var jv) && jv is SynxValue.Arr ja)
            {
                var sep = Arg(markers, "join", 1) is { } jk ? DelimiterFromKeyword(jk) : ",";
                map[key] = new SynxValue.Str(string.Join(sep, ja.Items.Select(ValueToString)));
            }
        }

        // ── :default (standalone, without :env) ──
        if ((mb & Mk.Default) != 0 && (mb & Mk.Env) == 0)
        {
            var isEmpty = !map.TryGetValue(key, out var dv) || dv is SynxValue.Null
                          || (dv is SynxValue.Str ds && ds.Value.Length == 0);
            if (isEmpty)
            {
                var di = markers.IndexOf("default");
                if (markers.Count > di + 1)
                {
                    var fallback = string.Join(":", markers.Skip(di + 1));
                    map[key] = WantsString(meta) ? new SynxValue.Str(fallback) : CastPrimitive(fallback);
                }
            }
        }

        // ── :clamp ── key:clamp:MIN:MAX value. A bound must be a finite number: `nan`,
        // `inf`, `1e400` skip the marker (Rust's f64::clamp panicked on a NaN bound).
        if ((mb & Mk.Clamp) != 0)
        {
            if (SynxNum.TryParseF64(Arg(markers, "clamp", 1) ?? "", out var lo) && double.IsFinite(lo)
                && SynxNum.TryParseF64(Arg(markers, "clamp", 2) ?? "", out var hi) && double.IsFinite(hi))
            {
                if (lo > hi)
                    map[key] = new SynxValue.Str($"CONSTRAINT_ERR: clamp min ({SynxNum.Display(lo)}) > max ({SynxNum.Display(hi)})");
                else if (map.TryGetValue(key, out var clv) && ValueAsNumber(clv) is { } cn)
                    map[key] = NumberValue(cn < lo ? lo : cn > hi ? hi : cn);
            }
        }

        // ── :round ── key:round:N value (N decimal places, default 0)
        if ((mb & Mk.Round) != 0)
        {
            var dec = Arg(markers, "round", 1) is { } rs && SynxNum.TryParseU64(rs, uint.MaxValue, out var d) ? (uint)d : 0u;
            if (map.TryGetValue(key, out var rv) && ValueAsNumber(rv) is { } rn)
            {
                // A scale past the range of f64 rounds nothing: the number stays.
                var factor = Math.Pow(10, (int)Math.Min(dec, 400u));
                var scaled = rn * factor;
                var rounded = double.IsFinite(scaled) ? Math.Round(scaled, MidpointRounding.AwayFromZero) / factor : rn;
                // safety: only a whole number that fits is an integer. The saturating cast
                // made `:round` of 1e30 9223372036854775807 and of NaN 0 — invented values,
                // where `:calc` and `:clamp` keep the float.
                map[key] = dec == 0 && rounded % 1 == 0 && Math.Abs(rounded) < long.MaxValue
                    ? new SynxValue.Int((long)rounded)
                    : new SynxValue.Float(rounded);
            }
        }

        // ── :map ── key:map:source_key + `- lookup result` items
        if ((mb & Mk.Map) != 0)
        {
            if (map.TryGetValue(key, out var mv) && mv is SynxValue.Arr ma)
            {
                var sourceKey = Arg(markers, "map", 1) ?? "";
                var lookupVal = "";
                if (sourceKey.Length > 0)
                {
                    var src = DeepGet(root, sourceKey) ?? (map.TryGetValue(sourceKey, out var sm) ? sm : null);
                    lookupVal = src != null ? ValueToString(src) : "";
                }
                SynxValue? pick = null;
                foreach (var item in ma.Items)
                {
                    if (item is SynxValue.Str line && line.Value.IndexOf(' ') is var sp and >= 0
                        && line.Value[..sp].Trim() == lookupVal)
                    {
                        pick = CastPrimitive(line.Value[(sp + 1)..].Trim());
                        break;
                    }
                }
                map[key] = pick ?? new SynxValue.Null();
            }
        }

        // ── :format ── key:format:PATTERN value (printf-style: %.2f, %d, %05d, %e)
        if ((mb & Mk.Format) != 0)
        {
            var pat = Arg(markers, "format", 1) ?? "%s";
            if (map.TryGetValue(key, out var fv))
                map[key] = new SynxValue.Str(ApplyFormatPattern(pat, fv));
        }

        // ── :replace:FROM:TO ── literal substring replacement; TO defaults to deletion
        if ((mb & Mk.Replace) != 0)
        {
            if (map.TryGetValue(key, out var rpv) && rpv is SynxValue.Str rps)
            {
                var from = Arg(markers, "replace", 1) ?? "";
                var to = Arg(markers, "replace", 2) ?? "";
                if (from.Length > 0)
                    map[key] = new SynxValue.Str(rps.Value.Replace(from, to, StringComparison.Ordinal));
            }
        }

        // ── :sort / :sort:desc ── a total order: numbers first, by value (NaN after the
        // rest of them), then everything else by its text, by code point (never locale
        // collation); stable. Comparing a number with text as text made `10 < 1a < 9 < 10`
        // a cycle, and the result depended on the sort algorithm.
        if ((mb & Mk.Sort) != 0)
        {
            if (map.TryGetValue(key, out var sov) && sov is SynxValue.Arr soa)
            {
                var desc = Arg(markers, "sort", 1) == "desc";
                var sorted = new List<SynxValue>(soa.Items.Count);
                foreach (var item in soa.Items) sorted.Add(CloneValue(item));
                var keyed = sorted.Select((v, i) => (v, i, text: ValueAsNumber(v) == null ? ValueToString(v) : null)).ToArray();
                Array.Sort(keyed, (x, y) =>
                {
                    int c;
                    var xn = ValueAsNumber(x.v);
                    var yn = ValueAsNumber(y.v);
                    if (xn is { } a && yn is { } b)
                        c = double.IsNaN(a) || double.IsNaN(b) ? double.IsNaN(a).CompareTo(double.IsNaN(b)) : a.CompareTo(b);
                    else if (xn != null)
                        c = -1;
                    else if (yn != null)
                        c = 1;
                    else
                        c = CompareCodePoints(x.text!, y.text!);
                    return c != 0 ? c : x.i.CompareTo(y.i);
                });
                sorted = keyed.Select(k => k.v).ToList();
                if (desc) sorted.Reverse();
                map[key] = new SynxValue.Arr(sorted);
            }
        }

        // ── :sum ── numeric items (and numeric strings); int while every summand is whole
        if ((mb & Mk.Sum) != 0)
        {
            if (map.TryGetValue(key, out var suv) && suv is SynxValue.Arr sua)
            {
                double total = 0;
                var allInt = true;
                foreach (var v in sua.Items)
                {
                    switch (v)
                    {
                        case SynxValue.Int n:
                            total += n.Value;
                            break;
                        case SynxValue.Float f:
                            total += f.Value;
                            if (f.Value % 1 != 0) allInt = false;
                            break;
                        // `inf` and `nan` are text here as everywhere (see CastPrimitive).
                        case SynxValue.Str s when SynxNum.TryParseF64(s.Value, out var sf) && double.IsFinite(sf):
                            total += sf;
                            if (sf % 1 != 0) allInt = false;
                            break;
                    }
                }
                map[key] = allInt && total % 1 == 0 && Math.Abs(total) < long.MaxValue
                    ? new SynxValue.Int((long)total)
                    : new SynxValue.Float(total);
            }
        }

        // ── :fallback ── key:fallback:DEFAULT value — the default when the path is missing
        if ((mb & Mk.Fallback) != 0)
        {
            var defv = Arg(markers, "fallback", 1) ?? "";
            var useFb = !map.TryGetValue(key, out var fbv) || fbv switch
            {
                SynxValue.Null => true,
                SynxValue.Str { Value.Length: 0 } => true,
                SynxValue.Str fs => !SafePathExists(options.BasePath ?? ".", fs.Value),
                _ => false,
            };
            if (useFb && defv.Length > 0)
                map[key] = new SynxValue.Str(defv);
        }

        // ── :once ── key:once[:uuid|random|timestamp], persisted in .synx.lock
        if ((mb & Mk.Once) != 0)
        {
            var gen = Arg(markers, "once", 1) ?? "uuid";
            var lockPath = Path.Combine(options.BasePath ?? ".", ".synx.lock");
            if (ReadLock(lockPath, key) is { } existing)
                map[key] = new SynxValue.Str(existing);
            else
            {
                var generated = gen switch
                {
                    "timestamp" => DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString(CultureInfo.InvariantCulture),
                    "random" => (NextU64() % uint.MaxValue).ToString(CultureInfo.InvariantCulture),
                    _ => GenerateUuid(),
                };
                WriteLock(lockPath, key, generated);
                map[key] = new SynxValue.Str(generated);
            }
        }

        // ── :version ── key:version:OP:REQUIRED value → bool
        if ((mb & Mk.Version) != 0)
        {
            if (map.TryGetValue(key, out var vv) && vv is SynxValue.Str curv)
            {
                var op = Arg(markers, "version", 1) ?? ">=";
                var req = Arg(markers, "version", 2) ?? "";
                map[key] = new SynxValue.Bool(CompareVersions(curv.Value, op, req));
            }
        }

        // ── :watch ── key:watch[:KEY_PATH] ./file
        if ((mb & Mk.Watch) != 0)
        {
            if (map.TryGetValue(key, out var wv) && wv is SynxValue.Str wp)
            {
                var maxD = options.MaxIncludeDepth ?? DefaultMaxIncludeDepth;
                if (options.IncludeDepth >= maxD)
                {
                    map[key] = new SynxValue.Str($"WATCH_ERR: max include depth ({maxD}) exceeded");
                    return;
                }
                string full;
                try
                {
                    full = JailPath(options.BasePath ?? ".", wp.Value);
                    CheckFileSize(full);
                }
                catch (Exception e)
                {
                    map[key] = new SynxValue.Str("WATCH_ERR: " + e.Message);
                    return;
                }
                var kp = Arg(markers, "watch", 1);
                try
                {
                    var content = ReadText(full, wp.Value);
                    var ext = Path.GetExtension(full).TrimStart('.');
                    map[key] = kp != null
                        ? ExtractFromContent(content, kp, ext) ?? new SynxValue.Null()
                        : new SynxValue.Str(content.Trim());
                }
                catch (Exception e)
                {
                    map[key] = new SynxValue.Str("WATCH_ERR: " + e.Message);
                }
            }
        }

        // ── :secret ──
        // Last of the markers that produce the value, so whatever they resolved is what
        // gets masked. It used to sit after `:alias`: `t:secret:once` came out as plain
        // text, `:secret:default:x` and `:secret:watch` masked the argument instead of the
        // result, and `:secret:format` formatted the mask. `:prompt` and package markers
        // below still see only the mask.
        if ((mb & Mk.Secret) != 0)
        {
            if (map.TryGetValue(key, out var sv))
                map[key] = MaskSecretValue(sv);
        }

        // ── :prompt ── key:prompt:LABEL subtree → fenced SYNX text
        if ((mb & Mk.Prompt) != 0)
        {
            var label = Arg(markers, "prompt", 1) ?? key;
            if (map.TryGetValue(key, out var pv))
                map[key] = new SynxValue.Str(label + " (SYNX):\n```synx\n" + StringifyValue(pv, 0) + "```");
        }

        // ── WASM custom markers ──
        if (options.WasmRuntime is { } wasmRt)
        {
            foreach (var marker in markers)
            {
                if (wasmRt.IsBuiltinMarker(marker)) continue;
                if (!wasmRt.HasMarker(marker)) continue;

                var markerIdx = markers.IndexOf(marker);
                var args = markers.Skip(markerIdx + 1).ToList();
                var currentValue = map.TryGetValue(key, out var cv) ? cv : new SynxValue.Null();
                try
                {
                    map[key] = wasmRt.ApplyMarker(marker, currentValue, args);
                }
                catch (Exception e)
                {
                    map[key] = new SynxValue.Str($"WASM_ERR: {e.Message}");
                }
                break; // Only apply one WASM marker per key
            }
        }

        // ── Constraint validation (always last, after all markers resolved) ──
        if (meta.Constraints is { } constr)
            ValidateConstraints(map, key, constr);
    }

    private static bool SafePathExists(string baseDir, string rel)
    {
        try
        {
            var full = JailPath(baseDir, rel);
            return File.Exists(full) || Directory.Exists(full);
        }
        catch
        {
            // A path that escapes the jail counts as missing.
            return false;
        }
    }

    /// <summary>Rust <c>str::split</c>: an empty separator splits around every character.</summary>
    private static IEnumerable<string> RustSplit(string s, string sep)
    {
        if (sep.Length > 0)
            return s.Split(sep, StringSplitOptions.None);
        var parts = new List<string> { "" };
        foreach (var r in s.EnumerateRunes()) parts.Add(r.ToString());
        parts.Add("");
        return parts;
    }

    /// <summary>Ordinal comparison by Unicode code point (UTF-16 order differs above the surrogates).</summary>
    internal static int CompareCodePoints(string a, string b)
    {
        var n = Math.Min(a.Length, b.Length);
        for (var i = 0; i < n; i++)
        {
            var x = a[i];
            var y = b[i];
            if (x == y) continue;
            if (char.IsSurrogate(x) != char.IsSurrogate(y))
                return char.IsSurrogate(x) ? 1 : -1;
            return x.CompareTo(y);
        }
        return a.Length.CompareTo(b.Length);
    }
}
