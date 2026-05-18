package com.aperturesyndicate.synx;

import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Random;
import java.util.Set;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

/**
 * SYNX {@code !active} engine. Resolves markers, includes, packages,
 * interpolation, constraints. Mirrors {@code crates/synx-core/src/engine.rs}.
 */
public final class SynxEngine {

    private SynxEngine() {}

    public static final int MAX_RESOLVE_DEPTH = 512;

    /** Apply markers and constraints to {@code result} in place. No-op on Static mode. */
    public static void resolve(SynxParseResult result, SynxOptions options) {
        if (result.mode != SynxMode.ACTIVE) return;
        if (!(result.root instanceof SynxValue.Obj)) return;
        new Resolver(result, options).run();
    }

    // ─── builtin marker name set (custom markers cannot shadow these) ──────
    static final Set<String> BUILTIN_MARKERS = Set.of(
        "env","default","calc","ref","alias","secret","random","unique","geo","i18n",
        "split","join","clamp","round","map","format","replace","sort","sum","fallback",
        "once","version","watch","prompt","vision","audio","include","import","inherit","spam"
    );

    // Process-wide rate-limit bucket for `:spam`.
    private static final Set<String> SPAM_BUCKETS = Collections.synchronizedSet(new HashSet<>());

    private static final class Resolver {
        final SynxParseResult result;
        final SynxOptions options;
        final SynxObject root;
        final Map<String, SynxObject> namespaces = new HashMap<>();
        final Random rng;
        boolean onceLoaded;
        final Set<String> onceKeys = new HashSet<>();
        final Set<String> onceNewKeys = new LinkedHashSet<>();

        Resolver(SynxParseResult r, SynxOptions o) {
            this.result = r;
            this.options = o;
            this.root = ((SynxValue.Obj) r.root).map();
            long seed = System.nanoTime() ^ Thread.currentThread().getId();
            if (o.env != null && o.env.containsKey("SYNX_SEED")) {
                try { seed = Long.parseLong(o.env.get("SYNX_SEED")); }
                catch (NumberFormatException ignored) {}
            }
            this.rng = new Random(seed);
        }

        void run() {
            loadPackages();
            loadIncludes();
            applyInheritPass();
            stripUnderscoreKeys();
            walk("", 0);
            validateAll();
            flushOnce();
        }

        // ─── top-level passes ─────────────────────────────────────────────
        void loadPackages() {
            if (result.uses.isEmpty()) return;
            String base = options.packagesPath != null ? options.packagesPath : "./synx_packages";
            for (var use : result.uses) {
                if (use.pkg().contains("..")) continue;
                Path p = Paths.get(base, use.pkg(), "synx.synx");
                String text = readText(p);
                if (text == null) continue;
                SynxParseResult sub = SynxParser.parse(text);
                if (!(sub.root instanceof SynxValue.Obj o)) continue;
                namespaces.put(use.alias(), o.map());
                root.set(use.alias(), SynxValue.ofObject(o.map()));
            }
        }

        void loadIncludes() {
            if (result.includes.isEmpty()) return;
            int maxDepth = options.maxIncludeDepth != null ? options.maxIncludeDepth : 16;
            if (options.includeDepth >= maxDepth) return;
            String base = options.basePath != null ? options.basePath : ".";
            for (var inc : result.includes) {
                String safe = jailPath(base, inc.path());
                if (safe == null) continue;
                String text = readText(Paths.get(safe));
                if (text == null) continue;
                SynxParseResult sub = SynxParser.parse(text);
                if (sub.mode == SynxMode.ACTIVE) {
                    SynxOptions subOpts = new SynxOptions();
                    subOpts.env = options.env;
                    subOpts.region = options.region;
                    subOpts.lang = options.lang;
                    subOpts.basePath = Paths.get(safe).toAbsolutePath().getParent().toString();
                    subOpts.maxIncludeDepth = options.maxIncludeDepth;
                    subOpts.packagesPath = options.packagesPath;
                    subOpts.strict = options.strict;
                    subOpts.markerFns = options.markerFns;
                    subOpts.includeDepth = options.includeDepth + 1;
                    SynxEngine.resolve(sub, subOpts);
                }
                if (!(sub.root instanceof SynxValue.Obj o)) continue;
                namespaces.put(inc.alias(), o.map());
                root.set(inc.alias(), SynxValue.ofObject(o.map()));
            }
        }

        void applyInheritPass() {
            for (var pathEntry : result.metadata.entrySet()) {
                String path = pathEntry.getKey();
                for (var fieldEntry : pathEntry.getValue().entrySet()) {
                    SynxMeta meta = fieldEntry.getValue();
                    if (!meta.hasMarker("inherit") || meta.args.isEmpty()) continue;
                    inheritMerge(path, fieldEntry.getKey(), meta.args);
                }
            }
        }

        void inheritMerge(String path, String key, List<String> parentNames) {
            SynxObject parent = getObjectAt(path);
            if (parent == null) return;
            SynxValue childVal = parent.get(key);
            if (!(childVal instanceof SynxValue.Obj childObj)) return;
            SynxObject target = childObj.map();
            for (String parentName : parentNames) {
                SynxValue pv = parent.get(parentName);
                if (pv instanceof SynxValue.Obj po) {
                    mergeMissing(target, po.map());
                }
            }
        }

        void mergeMissing(SynxObject dst, SynxObject src) {
            for (var e : src) {
                SynxValue existing = dst.get(e.getKey());
                if (existing != null) {
                    if (existing instanceof SynxValue.Obj eo && e.getValue() instanceof SynxValue.Obj so) {
                        mergeMissing(eo.map(), so.map());
                    }
                } else {
                    dst.set(e.getKey(), e.getValue());
                }
            }
        }

        void stripUnderscoreKeys() {
            List<String> keys = new ArrayList<>(root.keys());
            for (String k : keys) {
                if (!k.isEmpty() && k.charAt(0) == '_') root.remove(k);
            }
        }

        void walk(String path, int depth) {
            if (depth > MAX_RESOLVE_DEPTH) return;
            Map<String, SynxMeta> fields = result.metadata.get(path);
            if (fields != null) {
                // Snapshot because we may mutate the underlying parent during iteration.
                for (var e : new ArrayList<>(fields.entrySet())) {
                    applyMarkers(e.getValue(), e.getKey(), path);
                }
            }
            SynxObject container = getObjectAt(path);
            if (container == null) return;
            for (var e : new ArrayList<>(container.entriesList())) {
                if (e.getValue() instanceof SynxValue.Obj) {
                    walk(path.isEmpty() ? e.getKey() : path + "." + e.getKey(), depth + 1);
                }
            }
        }

        void applyMarkers(SynxMeta meta, String key, String path) {
            SynxObject parent = getObjectAt(path);
            if (parent == null) return;
            SynxValue value = parent.get(key);
            if (value == null) value = SynxValue.ofNull();

            for (String marker : meta.markers) {
                switch (marker) {
                    case "env":      value = applyEnv(value, meta);                break;
                    case "default":  value = applyDefault(value, meta);            break;
                    case "calc":     value = applyCalc(value);                     break;
                    case "ref":
                    case "alias":    value = applyRef(value);                      break;
                    case "secret":   value = applySecret(value);                   break;
                    case "random":   value = applyRandom(value, meta);             break;
                    case "unique":   value = applyUnique(value);                   break;
                    case "geo":      value = applyGeo(value, meta);                break;
                    case "i18n":     value = applyI18n(value, meta);               break;
                    case "split":    value = applySplit(value, meta);              break;
                    case "join":     value = applyJoin(value, meta);               break;
                    case "clamp":    value = applyClamp(value, meta);              break;
                    case "round":    value = applyRound(value, meta);              break;
                    case "map":      value = applyMap(value, meta);                break;
                    case "format":   value = applyFormat(value, meta);             break;
                    case "replace":  value = applyReplace(value, meta);            break;
                    case "sort":     value = applySort(value, meta);               break;
                    case "sum":      value = applySum(value);                      break;
                    case "fallback": value = applyFallback(value, meta);           break;
                    case "once":     value = applyOnce(value, path, key);          break;
                    case "version":  value = applyVersion(value);                  break;
                    case "watch":    value = applyWatch(value);                    break;
                    case "prompt":   value = applyPrompt(value);                   break;
                    case "vision":
                    case "audio":    break; // passthrough envelopes
                    case "spam":     value = applySpam(value, key);                break;
                    case "inherit":
                    case "include":
                    case "import":   break; // handled elsewhere
                    default:
                        if (!BUILTIN_MARKERS.contains(marker)) {
                            SynxMarkerFn fn = options.markerFns.get(marker);
                            if (fn != null) value = fn.apply(key, meta.args, value);
                        }
                }
            }

            if (meta.typeHint != null) {
                value = coerceTypeHint(value, meta.typeHint);
            }
            parent.set(key, value);
        }

        void validateAll() {
            for (var pathEntry : result.metadata.entrySet()) {
                String path = pathEntry.getKey();
                SynxObject container = getObjectAt(path);
                if (container == null) continue;
                for (var fieldEntry : pathEntry.getValue().entrySet()) {
                    String fk = fieldEntry.getKey();
                    SynxMeta meta = fieldEntry.getValue();
                    if (meta.constraints == null) continue;
                    SynxConstraints c = meta.constraints;
                    SynxValue fv = container.get(fk);
                    if (fv == null) {
                        if (c.required && options.strict) {
                            System.err.println("synx: required '" + path + "." + fk + "' missing");
                        }
                        continue;
                    }
                    if (c.typeName != null) {
                        boolean ok = (c.typeName.equals("int")    && fv instanceof SynxValue.Int)
                                  || (c.typeName.equals("float")  && fv instanceof SynxValue.Float)
                                  || (c.typeName.equals("bool")   && fv instanceof SynxValue.Bool)
                                  || (c.typeName.equals("string") && fv instanceof SynxValue.Str)
                                  || (c.typeName.equals("array")  && fv instanceof SynxValue.Arr)
                                  || (c.typeName.equals("object") && fv instanceof SynxValue.Obj);
                        if (!ok && options.strict) {
                            System.err.println("synx: type mismatch '" + path + "." + fk + "'"
                                + " want " + c.typeName + ", got " + fv.typeName());
                        }
                    }
                    Double dv = fv.asDouble();
                    if (dv != null) {
                        if (c.min != null && dv < c.min && options.strict) {
                            System.err.println("synx: '" + path + "." + fk + "' below min");
                        }
                        if (c.max != null && dv > c.max && options.strict) {
                            System.err.println("synx: '" + path + "." + fk + "' above max");
                        }
                    }
                    if (c.enumValues != null && fv instanceof SynxValue.Str s
                            && !c.enumValues.contains(s.value()) && options.strict) {
                        System.err.println("synx: '" + path + "." + fk + "' '" + s.value() + "' not in enum");
                    }
                    if (c.pattern != null && fv instanceof SynxValue.Str s) {
                        if (!regexMatches(s.value(), c.pattern) && options.strict) {
                            System.err.println("synx: '" + path + "." + fk + "' fails pattern '" + c.pattern + "'");
                        }
                    }
                }
            }
        }

        SynxValue applyOnce(SynxValue v, String path, String key) {
            if (!onceLoaded) {
                onceLoaded = true;
                String base = options.basePath != null ? options.basePath : ".";
                Path lock = Paths.get(base, ".synx.lock");
                String text = readText(lock);
                if (text != null) {
                    for (String line : text.split("\n", -1)) {
                        String s = line.strip();
                        if (!s.isEmpty()) onceKeys.add(s);
                    }
                }
            }
            String lockKey = path.isEmpty() ? key : path + "." + key;
            if (onceKeys.contains(lockKey)) return SynxValue.ofNull();
            onceNewKeys.add(lockKey);
            return v;
        }

        void flushOnce() {
            if (onceNewKeys.isEmpty()) return;
            String base = options.basePath != null ? options.basePath : ".";
            Path lock = Paths.get(base, ".synx.lock");
            Set<String> all = new LinkedHashSet<>(onceKeys);
            all.addAll(onceNewKeys);
            List<String> sorted = new ArrayList<>(all);
            Collections.sort(sorted);
            try {
                Files.writeString(lock, String.join("\n", sorted) + "\n");
            } catch (IOException ignored) {}
        }

        // ─── path helpers ─────────────────────────────────────────────────
        SynxObject getObjectAt(String path) {
            if (path.isEmpty()) return root;
            SynxObject current = root;
            for (String seg : path.split("\\.")) {
                SynxValue v = current.get(seg);
                if (!(v instanceof SynxValue.Obj o)) return null;
                current = o.map();
            }
            return current;
        }

        SynxValue lookup(String path) {
            int dot = path.indexOf('.');
            if (dot >= 0) {
                String ns = path.substring(0, dot);
                SynxObject nsRoot = namespaces.get(ns);
                if (nsRoot != null) {
                    SynxValue v = deepGet(path.substring(dot + 1), nsRoot);
                    if (v != null) return v;
                }
            }
            return deepGet(path, root);
        }

        SynxValue deepGet(String path, SynxObject from) {
            if (path.isEmpty()) return null;
            SynxObject current = from;
            String[] parts = path.split("\\.");
            for (int i = 0; i < parts.length; i++) {
                SynxValue v = current.get(parts[i]);
                if (v == null) return null;
                if (i == parts.length - 1) return v;
                if (!(v instanceof SynxValue.Obj o)) return null;
                current = o.map();
            }
            return null;
        }

        String interpolate(String s) {
            if (s.indexOf('{') < 0) return s;
            StringBuilder out = new StringBuilder(s.length());
            int i = 0;
            while (i < s.length()) {
                char c = s.charAt(i);
                if (c == '{') {
                    int end = s.indexOf('}', i + 1);
                    if (end < 0) { out.append('{'); i++; continue; }
                    String inner = s.substring(i + 1, end).strip();
                    SynxValue v = lookup(inner);
                    if (v != null) out.append(valueToString(v));
                    else { out.append('{').append(inner).append('}'); }
                    i = end + 1;
                } else {
                    out.append(c); i++;
                }
            }
            return out.toString();
        }

        String jailPath(String base, String rel) {
            if (rel == null || rel.isEmpty()) return null;
            if (rel.charAt(0) == '/' || rel.charAt(0) == '\\') return null;
            if (rel.length() >= 2 && rel.charAt(1) == ':') return null; // Windows drive
            if (rel.startsWith("res://") || rel.startsWith("user://")) return null;
            String normalized = rel.replace('\\', '/');
            for (String seg : normalized.split("/", -1)) {
                if (seg.equals("..") || seg.equals("...")) return null;
            }
            return Paths.get(base, normalized).toString();
        }

        SynxValue coerceTypeHint(SynxValue v, String hint) {
            switch (hint) {
                case "int":
                    if (v instanceof SynxValue.Int) return v;
                    Double d = v.asDouble();
                    if (d != null) return SynxValue.ofInt(d.longValue());
                    return v;
                case "float":
                    if (v instanceof SynxValue.Float) return v;
                    Double d2 = v.asDouble();
                    if (d2 != null) return SynxValue.ofFloat(d2);
                    return v;
                case "string":
                    if (v instanceof SynxValue.Str) return v;
                    return SynxValue.ofString(valueToString(v));
                case "bool":
                    if (v instanceof SynxValue.Bool) return v;
                    String s = valueToString(v);
                    return SynxValue.ofBool(s.equals("true") || s.equals("1"));
                default: return v;
            }
        }

        // ─── markers ──────────────────────────────────────────────────────
        SynxValue applyEnv(SynxValue v, SynxMeta meta) {
            if (options.env == null) return v;
            String varName = valueToString(v);
            String fallback = "";
            int idx = meta.markerIndex("env");
            if (idx >= 0 && idx + 1 < meta.markers.size() && meta.markers.get(idx + 1).equals("default")
                && idx + 2 < meta.markers.size()) {
                fallback = meta.markers.get(idx + 2);
            }
            String val = options.env.get(varName);
            if (val != null) return SynxValue.ofString(val);
            if (!fallback.isEmpty()) return SynxValue.ofString(fallback);
            return SynxValue.ofNull();
        }

        SynxValue applyDefault(SynxValue v, SynxMeta meta) {
            if (meta.hasMarker("env")) return v;
            boolean empty = v.isNull() || (v instanceof SynxValue.Str s && s.value().isEmpty());
            if (!empty) return v;
            int idx = meta.markerIndex("default");
            if (idx >= 0 && idx + 1 < meta.markers.size()) {
                return SynxValue.ofString(meta.markers.get(idx + 1));
            }
            return v;
        }

        SynxValue applyCalc(SynxValue v) {
            String expr = valueToString(v);
            if (expr.isEmpty()) return v;
            expr = interpolate(expr);

            List<Map.Entry<String, Double>> nums = new ArrayList<>();
            for (var e : root) {
                Double d = e.getValue().asDouble();
                if (d != null) nums.add(Map.entry(e.getKey(), d));
            }
            nums.sort((a, b) -> Integer.compare(b.getKey().length(), a.getKey().length()));
            for (var e : nums) {
                expr = replaceWord(expr, e.getKey(), String.format(Locale.ROOT, "%.17g", e.getValue()));
            }

            SynxCalc.Result r = SynxCalc.evaluate(expr);
            if (!r.ok) return v;
            double d = r.value;
            if (d == Math.floor(d) && !Double.isInfinite(d) && Math.abs(d) < 9.2233720368547758e18) {
                return SynxValue.ofInt((long) d);
            }
            return SynxValue.ofFloat(d);
        }

        SynxValue applyRef(SynxValue v) {
            String path = valueToString(v);
            if (path.isEmpty()) return v;
            SynxValue target = lookup(path);
            return target != null ? target : v;
        }

        SynxValue applySecret(SynxValue v) {
            if (v instanceof SynxValue.Secret) return v;
            if (v.isNull()) return v;
            if (v instanceof SynxValue.Str s) return SynxValue.ofSecret(s.value());
            return SynxValue.ofSecret(valueToString(v));
        }

        SynxValue applyRandom(SynxValue v, SynxMeta meta) {
            List<String> opts = new ArrayList<>();
            if (v instanceof SynxValue.Arr a) {
                for (SynxValue x : a.values()) opts.add(valueToString(x));
            } else if (v instanceof SynxValue.Str s) {
                for (String p : s.value().split(",")) opts.add(p.strip());
            }
            if (opts.isEmpty()) return v;
            List<Double> weights = new ArrayList<>();
            for (String a : meta.args) {
                try { weights.add(Double.parseDouble(a)); }
                catch (NumberFormatException e) { weights.add(1.0); }
            }
            while (weights.size() < opts.size()) weights.add(1.0);
            double total = 0; for (double w : weights) total += w;
            if (total <= 0) return v;
            double pick = rng.nextDouble() * total;
            double acc = 0;
            for (int i = 0; i < opts.size(); i++) {
                acc += weights.get(i);
                if (pick <= acc) return SynxValue.ofString(opts.get(i));
            }
            return SynxValue.ofString(opts.get(opts.size() - 1));
        }

        SynxValue applyUnique(SynxValue v) {
            if (!(v instanceof SynxValue.Arr a)) return v;
            List<SynxValue> out = new ArrayList<>();
            for (SynxValue item : a.values()) {
                boolean dup = false;
                for (SynxValue seen : out) if (seen.equals(item)) { dup = true; break; }
                if (!dup) out.add(item);
            }
            return SynxValue.ofArray(out);
        }

        SynxValue applyGeo(SynxValue v, SynxMeta meta) {
            if (options.region == null || options.region.isEmpty()) return v;
            for (String a : meta.args) {
                int colon = a.indexOf(':');
                if (colon < 0) continue;
                if (a.substring(0, colon).equals(options.region)) {
                    return SynxValue.ofString(a.substring(colon + 1));
                }
            }
            return v;
        }

        SynxValue applyI18n(SynxValue v, SynxMeta meta) {
            String lang = options.lang != null ? options.lang : "en";
            String lang2 = lang.length() >= 2 ? lang.substring(0, 2) : lang;
            Double n = v.asDouble();
            String category = (n != null) ? pluralCategory(lang, n) : "other";
            String[] keys = { lang + "." + category, lang2 + "." + category, lang, lang2, "other" };
            for (String key : keys) {
                for (String a : meta.args) {
                    int colon = a.indexOf(':');
                    if (colon < 0) continue;
                    if (a.substring(0, colon).equals(key)) {
                        return SynxValue.ofString(a.substring(colon + 1));
                    }
                }
            }
            return v;
        }

        SynxValue applySplit(SynxValue v, SynxMeta meta) {
            if (!(v instanceof SynxValue.Str s)) return v;
            String sep = ",";
            int idx = meta.markerIndex("split");
            if (idx >= 0 && idx + 1 < meta.markers.size()) sep = meta.markers.get(idx + 1);
            List<SynxValue> out = new ArrayList<>();
            if (sep.isEmpty()) {
                for (int i = 0; i < s.value().length(); i++) out.add(SynxValue.ofString(String.valueOf(s.value().charAt(i))));
            } else {
                for (String part : s.value().split(Pattern.quote(sep), -1)) {
                    out.add(SynxValue.ofString(part.strip()));
                }
            }
            return SynxValue.ofArray(out);
        }

        SynxValue applyJoin(SynxValue v, SynxMeta meta) {
            if (!(v instanceof SynxValue.Arr a)) return v;
            String sep = ",";
            int idx = meta.markerIndex("join");
            if (idx >= 0 && idx + 1 < meta.markers.size()) sep = meta.markers.get(idx + 1);
            List<String> parts = new ArrayList<>();
            for (SynxValue x : a.values()) parts.add(valueToString(x));
            return SynxValue.ofString(String.join(sep, parts));
        }

        SynxValue applyClamp(SynxValue v, SynxMeta meta) {
            Double d = v.asDouble();
            if (d == null) return v;
            double lo = Double.NEGATIVE_INFINITY, hi = Double.POSITIVE_INFINITY;
            int idx = meta.markerIndex("clamp");
            if (idx >= 0 && idx + 2 < meta.markers.size()) {
                try { lo = Double.parseDouble(meta.markers.get(idx + 1)); } catch (Exception ignored) {}
                try { hi = Double.parseDouble(meta.markers.get(idx + 2)); } catch (Exception ignored) {}
            }
            if (d < lo) d = lo;
            if (d > hi) d = hi;
            if (v instanceof SynxValue.Int) return SynxValue.ofInt(d.longValue());
            return SynxValue.ofFloat(d);
        }

        SynxValue applyRound(SynxValue v, SynxMeta meta) {
            Double d = v.asDouble();
            if (d == null) return v;
            int digits = 0;
            int idx = meta.markerIndex("round");
            if (idx >= 0 && idx + 1 < meta.markers.size()) {
                try { digits = Integer.parseInt(meta.markers.get(idx + 1)); } catch (Exception ignored) {}
            }
            double factor = Math.pow(10, digits);
            double r = Math.round(d * factor) / factor;
            if (digits == 0) return SynxValue.ofInt((long) r);
            return SynxValue.ofFloat(r);
        }

        SynxValue applyMap(SynxValue v, SynxMeta meta) {
            String key = valueToString(v);
            for (String a : meta.args) {
                int colon = a.indexOf(':');
                if (colon < 0) continue;
                if (a.substring(0, colon).equals(key)) {
                    return SynxValue.ofString(a.substring(colon + 1));
                }
            }
            return v;
        }

        SynxValue applyFormat(SynxValue v, SynxMeta meta) {
            int idx = meta.markerIndex("format");
            if (idx < 0 || idx + 1 >= meta.markers.size()) return v;
            String pattern = interpolate(meta.markers.get(idx + 1));
            Double n = v.asDouble();
            String sIn = valueToString(v);
            return SynxValue.ofString(applyPrintf(pattern, n != null ? n : 0, sIn));
        }

        SynxValue applyReplace(SynxValue v, SynxMeta meta) {
            if (!(v instanceof SynxValue.Str s)) return v;
            int idx = meta.markerIndex("replace");
            if (idx < 0 || idx + 2 >= meta.markers.size()) return v;
            String from = meta.markers.get(idx + 1);
            String to = meta.markers.get(idx + 2);
            if (from.isEmpty()) return v;
            return SynxValue.ofString(s.value().replace(from, to));
        }

        SynxValue applySort(SynxValue v, SynxMeta meta) {
            if (!(v instanceof SynxValue.Arr a)) return v;
            boolean desc = false;
            int idx = meta.markerIndex("sort");
            if (idx >= 0 && idx + 1 < meta.markers.size()) {
                desc = meta.markers.get(idx + 1).equals("desc");
            }
            List<SynxValue> arr = new ArrayList<>(a.values());
            final boolean descending = desc;
            arr.sort((x, y) -> {
                Double dx = x.asDouble(), dy = y.asDouble();
                if (dx != null && dy != null) {
                    return descending ? Double.compare(dy, dx) : Double.compare(dx, dy);
                }
                String sx = valueToString(x), sy = valueToString(y);
                return descending ? sy.compareTo(sx) : sx.compareTo(sy);
            });
            return SynxValue.ofArray(arr);
        }

        SynxValue applySum(SynxValue v) {
            if (!(v instanceof SynxValue.Arr a)) return v;
            double total = 0;
            boolean anyFloat = false;
            for (SynxValue item : a.values()) {
                Double d = item.asDouble();
                if (d != null) {
                    total += d;
                    if (item instanceof SynxValue.Float) anyFloat = true;
                }
            }
            return anyFloat ? SynxValue.ofFloat(total) : SynxValue.ofInt((long) total);
        }

        SynxValue applyFallback(SynxValue v, SynxMeta meta) {
            boolean empty = v.isNull() || (v instanceof SynxValue.Str s && s.value().isEmpty());
            if (!empty) return v;
            int idx = meta.markerIndex("fallback");
            if (idx >= 0 && idx + 1 < meta.markers.size()) {
                return SynxValue.ofString(meta.markers.get(idx + 1));
            }
            return v;
        }

        SynxValue applyVersion(SynxValue v) {
            if (v instanceof SynxValue.Str) return v;
            return SynxValue.ofString(valueToString(v));
        }

        SynxValue applyWatch(SynxValue v) {
            if (!(v instanceof SynxValue.Str s)) return v;
            String base = options.basePath != null ? options.basePath : ".";
            String safe = jailPath(base, s.value());
            if (safe == null) return v;
            String text = readText(Paths.get(safe));
            return text != null ? SynxValue.ofString(text) : v;
        }

        SynxValue applyPrompt(SynxValue v) {
            if (!(v instanceof SynxValue.Str s)) return v;
            return SynxValue.ofString(interpolate(s.value()));
        }

        SynxValue applySpam(SynxValue v, String key) {
            if (SPAM_BUCKETS.contains(key)) return SynxValue.ofNull();
            SPAM_BUCKETS.add(key);
            return v;
        }
    }

    // ─── shared utilities ───────────────────────────────────────────────────
    static String valueToString(SynxValue v) {
        if (v instanceof SynxValue.Null) return "null";
        if (v instanceof SynxValue.Bool b) return b.value() ? "true" : "false";
        if (v instanceof SynxValue.Int i) return Long.toString(i.value());
        if (v instanceof SynxValue.Float f) {
            double d = f.value();
            if (Double.isNaN(d) || Double.isInfinite(d)) return "null";
            String s = String.format(Locale.ROOT, "%.17g", d);
            if (s.indexOf('.') < 0 && s.indexOf('e') < 0 && s.indexOf('E') < 0) s += ".0";
            return s;
        }
        if (v instanceof SynxValue.Str s) return s.value();
        if (v instanceof SynxValue.Secret s) return s.value();
        if (v instanceof SynxValue.Arr a) {
            List<String> parts = new ArrayList<>();
            for (SynxValue x : a.values()) parts.add(valueToString(x));
            return "[" + String.join(", ", parts) + "]";
        }
        return "[Object]";
    }

    static String readText(Path p) {
        try { return Files.readString(p); }
        catch (IOException e) { return null; }
    }

    static boolean regexMatches(String value, String pattern) {
        try {
            Pattern p = Pattern.compile(pattern);
            return p.matcher(value).find();
        } catch (PatternSyntaxException e) {
            return true; // Invalid pattern — do not reject.
        }
    }

    static String replaceWord(String s, String word, String repl) {
        if (word.isEmpty()) return s;
        StringBuilder out = new StringBuilder(s.length() + 16);
        int i = 0;
        while (i < s.length()) {
            int found = s.indexOf(word, i);
            if (found < 0) { out.append(s, i, s.length()); break; }
            out.append(s, i, found);
            boolean leftOK = (found == 0) || !isWordChar(s.charAt(found - 1));
            int after = found + word.length();
            boolean rightOK = (after == s.length()) || !isWordChar(s.charAt(after));
            if (leftOK && rightOK) {
                out.append(repl);
                i = after;
            } else {
                out.append(s.charAt(found));
                i = found + 1;
            }
        }
        return out.toString();
    }

    private static boolean isWordChar(char c) {
        return Character.isLetterOrDigit(c) || c == '_';
    }

    static String applyPrintf(String pattern, double number, String sIn) {
        StringBuilder out = new StringBuilder(pattern.length() + 16);
        int i = 0;
        while (i < pattern.length()) {
            char c = pattern.charAt(i);
            if (c != '%') { out.append(c); i++; continue; }
            if (i + 1 < pattern.length() && pattern.charAt(i + 1) == '%') {
                out.append('%'); i += 2; continue;
            }
            int end = i + 1;
            while (end < pattern.length()) {
                char k = pattern.charAt(end);
                if (k == 'd' || k == 'i' || k == 'f' || k == 'e' || k == 'g' || k == 's') break;
                end++;
            }
            if (end >= pattern.length()) { out.append(pattern.substring(i)); break; }
            String spec = pattern.substring(i, end + 1);
            char kind = pattern.charAt(end);
            try {
                if (kind == 'd' || kind == 'i') {
                    out.append(String.format(Locale.ROOT, spec, (long) number));
                } else if (kind == 'f' || kind == 'e' || kind == 'g') {
                    out.append(String.format(Locale.ROOT, spec, number));
                } else if (kind == 's') {
                    out.append(String.format(Locale.ROOT, spec, sIn));
                }
            } catch (Exception ignored) {
                // Malformed pattern — skip silently to match Rust behaviour.
            }
            i = end + 1;
        }
        return out.toString();
    }

    static String pluralCategory(String lang, double n) {
        String two = lang.length() >= 2 ? lang.substring(0, 2) : lang;
        long intN = (long) Math.floor(Math.abs(n));
        long mod10 = intN % 10;
        long mod100 = intN % 100;
        boolean intLike = n == Math.floor(n);

        switch (two) {
            case "ru": case "uk": case "be":
                if (intLike && mod10 == 1 && mod100 != 11) return "one";
                if (intLike && mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14)) return "few";
                if (intLike && (mod10 == 0 || (mod10 >= 5 && mod10 <= 9)
                                || (mod100 >= 11 && mod100 <= 14))) return "many";
                return "other";
            case "pl":
                if (intLike && n == 1) return "one";
                if (intLike && mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14)) return "few";
                if (intLike && n != 1 && (mod10 == 0 || mod10 == 1
                                          || (mod10 >= 5 && mod10 <= 9)
                                          || (mod100 >= 12 && mod100 <= 14))) return "many";
                return "other";
            case "cs": case "sk":
                if (intLike && n == 1) return "one";
                if (intLike && intN >= 2 && intN <= 4) return "few";
                if (!intLike) return "many";
                return "other";
            case "ar":
                if (n == 0) return "zero";
                if (n == 1) return "one";
                if (n == 2) return "two";
                if (intLike && mod100 >= 3 && mod100 <= 10) return "few";
                if (intLike && mod100 >= 11) return "many";
                return "other";
            case "fr": case "pt":
                if (n >= 0 && n < 2) return "one";
                return "other";
            case "ja": case "zh": case "ko": case "vi": case "th":
                return "other";
            default:
                return n == 1 ? "one" : "other";
        }
    }
}
