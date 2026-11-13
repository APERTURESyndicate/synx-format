package com.aperturesyndicate.synx;

import java.io.IOException;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.StandardCharsets;
import java.nio.file.AccessDeniedException;
import java.nio.file.Files;
import java.nio.file.InvalidPathException;
import java.nio.file.NoSuchFileException;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;
import java.util.Set;
import java.util.concurrent.ConcurrentHashMap;
import java.util.regex.Pattern;
import java.util.regex.PatternSyntaxException;

/**
 * SYNX {@code !active} engine — a port of {@code crates/synx-core/src/engine.rs} (SYNX 3.8.0).
 *
 * <p>Resolution is one pass per object in document order: for each key, first its child object
 * (or each object item of its list), then the key's own markers; after all keys, {@code {…}}
 * interpolation. A marker therefore sees every key above it already resolved, at any depth.
 * Declarations belong to the object that holds the key (metadata paths name list items as
 * {@code list.#0} and escape {@code .} / {@code \} inside a key).
 */
public final class SynxEngine {

    private SynxEngine() {}

    public static final int MAX_RESOLVE_DEPTH = 512;

    private static final int MAX_CALC_EXPR_LEN = 4096;
    private static final int MAX_CALC_RESOLVED_LEN = 64 * 1024;
    private static final long MAX_FILE_SIZE = 10L * 1024 * 1024;
    private static final int DEFAULT_MAX_INCLUDE_DEPTH = 16;
    private static final int MAX_ENGINE_SCRATCH = 4 * 1024 * 1024;
    private static final String CALC_TOO_LONG = "CALC_ERR: resolved expression too long (max 65536 bytes)";

    /** Apply markers and constraints to {@code result} in place. No-op on Static mode. */
    public static void resolve(SynxParseResult result, SynxOptions options) {
        if (result.mode != SynxMode.ACTIVE) return;
        SynxOptions opts = options == null ? new SynxOptions() : options;
        if (opts.includeDepth == 0) {
            IncludeState state = INCLUDE_STATE.get();
            if (state.open.isEmpty()) {
                state.reads = 0;
                state.readBytes = 0;
            }
        }
        new Resolver(result, opts).run();
    }

    /** Most included documents one top-level resolve may read and resolve. */
    private static final int MAX_INCLUDE_READS = 1024;

    /**
     * Most bytes one top-level resolve may read from disk through {@code :include},
     * {@code !include}, {@code !use} and {@code :watch}.
     *
     * <p>safety: every file is capped at {@link #MAX_FILE_SIZE}, but a document may name the
     * same file from a thousand keys; each read is parsed and kept.
     */
    private static final long MAX_RESOLVE_READ_BYTES = 64L * 1024 * 1024;

    /** Files whose resolution is in progress, outermost first, and what has been read so far. */
    private static final class IncludeState {
        final ArrayList<Path> open = new ArrayList<>();
        int reads;
        long readBytes;
    }

    /** A file the document may not read; the message is the text the document gets. */
    private static final class ReadRefused extends Exception {
        private static final long serialVersionUID = 1L;

        ReadRefused(String message) { super(message, null, false, false); }
    }

    /**
     * Reads a file for {@code :include}, {@code !include}, {@code !use} or {@code :watch},
     * counting its size against {@link #MAX_RESOLVE_READ_BYTES}. {@code shown} is the path as
     * the document wrote it.
     *
     * <p>spec: the refusal never carries the operating system's wording or the absolute path,
     * so the same document resolves to the same string on every machine.
     */
    static String readBudgeted(Path p, String shown) throws ReadRefused {
        // A name this platform cannot hold at all is a file that is not there.
        if (p == null) throw new ReadRefused("file not found: " + shown);
        IncludeState state = INCLUDE_STATE.get();
        long size = 0;
        try {
            size = Files.size(p);
        } catch (IOException | RuntimeException ignored) {
            // A file whose size cannot be read fails below, on read.
        }
        if (state.readBytes + size > MAX_RESOLVE_READ_BYTES) {
            throw new ReadRefused("read budget (" + MAX_RESOLVE_READ_BYTES + " bytes) exceeded");
        }
        // A directory reads as "permission denied" on Windows and "is a directory" elsewhere;
        // it is neither.
        boolean isDirectory;
        try {
            isDirectory = Files.isDirectory(p);
        } catch (RuntimeException e) {
            isDirectory = false;
        }
        if (isDirectory) throw new ReadRefused("not a file: " + shown);
        byte[] bytes;
        try {
            bytes = Files.readAllBytes(p);
        } catch (NoSuchFileException e) {
            throw new ReadRefused("file not found: " + shown);
        } catch (AccessDeniedException e) {
            throw new ReadRefused("permission denied: " + shown);
        } catch (IOException | RuntimeException e) {
            // An invalid or over-long name reaches here as a plain FileSystemException.
            throw new ReadRefused((exists(p) ? "cannot read file: " : "file not found: ") + shown);
        }
        String text;
        try {
            text = StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(java.nio.charset.CodingErrorAction.REPORT)
                .onUnmappableCharacter(java.nio.charset.CodingErrorAction.REPORT)
                .decode(java.nio.ByteBuffer.wrap(bytes)).toString();
        } catch (CharacterCodingException e) {
            throw new ReadRefused("file is not valid UTF-8: " + shown);
        }
        state.readBytes += bytes.length;
        return text;
    }

    private static final ThreadLocal<IncludeState> INCLUDE_STATE = ThreadLocal.withInitial(IncludeState::new);

    /**
     * Opens {@code full} for resolution; returns {@code null}, or why it may not be opened:
     * the file is already being resolved (a cycle), or the document has read its budget.
     * A successful call is paired with {@link #includeLeave()}.
     *
     * <p>safety: the depth cap alone let {@code a:include self.synx} / {@code b:include
     * self.synx} resolve 2^16 times — minutes for a three-line file, and a third key made it
     * hang. A cycle now stops at its first repeat, and the budget bounds an acyclic fan-out.
     */
    static String includeEnter(Path full, String shown) {
        IncludeState state = INCLUDE_STATE.get();
        if (state.open.contains(full)) return "circular include: '" + shown + "'";
        if (state.reads >= MAX_INCLUDE_READS) return "include budget (" + MAX_INCLUDE_READS + " files) exceeded";
        state.reads++;
        state.open.add(full);
        return null;
    }

    static void includeLeave() {
        IncludeState state = INCLUDE_STATE.get();
        if (!state.open.isEmpty()) state.open.remove(state.open.size() - 1);
    }

    /** Names a custom marker ({@link SynxOptions#markerFns}) cannot take. */
    static final Set<String> BUILTIN_MARKERS = Set.of(
        "env", "default", "calc", "ref", "alias", "secret", "random", "unique", "geo", "i18n",
        "split", "join", "clamp", "round", "map", "format", "replace", "sort", "sum", "fallback",
        "once", "version", "watch", "prompt", "vision", "audio", "include", "import", "inherit", "spam",
        "template"
    );

    // ─── marker bits ────────────────────────────────────────────────────────

    private static final int MK_ALIAS = 1, MK_CALC = 1 << 1, MK_CLAMP = 1 << 2, MK_DEFAULT = 1 << 3,
        MK_ENV = 1 << 4, MK_FALLBACK = 1 << 5, MK_FORMAT = 1 << 6, MK_GEO = 1 << 7, MK_I18N = 1 << 8,
        MK_IMPORT = 1 << 9, MK_INCLUDE = 1 << 10, MK_JOIN = 1 << 11, MK_MAP = 1 << 12, MK_ONCE = 1 << 13,
        MK_PROMPT = 1 << 14, MK_RANDOM = 1 << 15, MK_REF = 1 << 16, MK_REPLACE = 1 << 17,
        MK_ROUND = 1 << 18, MK_SECRET = 1 << 19, MK_SORT = 1 << 20, MK_SPAM = 1 << 21,
        MK_SPLIT = 1 << 22, MK_SUM = 1 << 23, MK_UNIQUE = 1 << 24, MK_VERSION = 1 << 25,
        MK_WATCH = 1 << 26;

    private static final Map<String, Integer> MARKER_BITS = Map.ofEntries(
        Map.entry("alias", MK_ALIAS), Map.entry("calc", MK_CALC), Map.entry("clamp", MK_CLAMP),
        Map.entry("default", MK_DEFAULT), Map.entry("env", MK_ENV), Map.entry("fallback", MK_FALLBACK),
        Map.entry("format", MK_FORMAT), Map.entry("geo", MK_GEO), Map.entry("i18n", MK_I18N),
        Map.entry("import", MK_IMPORT), Map.entry("include", MK_INCLUDE), Map.entry("join", MK_JOIN),
        Map.entry("map", MK_MAP), Map.entry("once", MK_ONCE), Map.entry("prompt", MK_PROMPT),
        Map.entry("random", MK_RANDOM), Map.entry("ref", MK_REF), Map.entry("replace", MK_REPLACE),
        Map.entry("round", MK_ROUND), Map.entry("secret", MK_SECRET), Map.entry("sort", MK_SORT),
        Map.entry("spam", MK_SPAM), Map.entry("split", MK_SPLIT), Map.entry("sum", MK_SUM),
        Map.entry("unique", MK_UNIQUE), Map.entry("version", MK_VERSION), Map.entry("watch", MK_WATCH));

    /** Classifies a marker chain once, so each marker test is a bit test, not a list scan. */
    static int markerBits(List<String> markers) {
        int bits = 0;
        for (String m : markers) {
            Integer b = MARKER_BITS.get(m);
            if (b != null) bits |= b;
        }
        return bits;
    }

    private static String markerArg(List<String> markers, String name, int off) {
        int idx = markers.indexOf(name);
        return idx >= 0 && idx + off < markers.size() ? markers.get(idx + off) : null;
    }

    // ─── resolver ───────────────────────────────────────────────────────────

    private static final class Resolver {
        final SynxParseResult result;
        final SynxOptions opts;
        final Map<String, Map<String, SynxMeta>> meta;
        final SynxValue rootValue;
        final SynxObject root;
        final Map<String, SynxValue> includes = new LinkedHashMap<>();
        final List<String> rootSpecial = new ArrayList<>();
        final StringBuilder path = new StringBuilder();
        final Random rng;
        /**
         * What the {@code :include} pre-pass resolved for a root key. Kept apart from the
         * {@code {key:alias}} map, so {@code {x:include}} still sees exactly the includes the
         * document declares.
         */
        final Map<String, PreInclude> preIncludes = new HashMap<>();
        /**
         * What the keys under {@code :secret} held before the pre-pass masked them, by object
         * path and key (see {@link #maskSecretLiterals}).
         */
        final Map<String, Map<String, SynxValue>> secretOriginals = new HashMap<>();

        Resolver(SynxParseResult r, SynxOptions o) {
            this.result = r;
            this.opts = o;
            if (r.metadata == null) r.metadata = new LinkedHashMap<>();
            this.meta = r.metadata;
            this.rootValue = r.root;
            this.root = r.root instanceof SynxValue.Obj ob ? ob.map() : null;
            long seed = System.nanoTime() ^ Thread.currentThread().getId();
            if (o.env != null && o.env.containsKey("SYNX_SEED")) {
                Long s = SynxNumbers.parseI64(o.env.get("SYNX_SEED"));
                if (s != null) seed = s;
            }
            this.rng = new Random(seed);
        }

        String basePath() { return opts.basePath == null ? "." : opts.basePath; }

        int maxIncludeDepth() { return opts.maxIncludeDepth == null ? DEFAULT_MAX_INCLUDE_DEPTH : opts.maxIncludeDepth; }

        void run() {
            Map<String, SynxValue> packages = loadPackages(result.uses);
            loadIncludes(result.includes);

            // `:include` / `:import` keys also feed the `{key:alias}` map.
            if (root != null) {
                Map<String, SynxMeta> rootMeta = meta.get("");
                if (rootMeta != null) {
                    for (String key : root.keys()) {
                        SynxMeta m = rootMeta.get(key);
                        if (m == null || !(m.hasMarker("include") || m.hasMarker("import"))) continue;
                        // The included document runs its own pre-pass: without the depth cap a
                        // file that includes itself recursed until the stack ran out.
                        if (opts.includeDepth >= maxIncludeDepth()) continue;
                        if (!(root.get(key) instanceof SynxValue.Str p)) continue;
                        Jail jail = jailPath(basePath(), p.value());
                        if (jail.error != null || checkFileSize(jail.path) != null) continue;
                        if (jail.path != null && includeEnter(jail.path, p.value()) != null) continue;
                        SynxValue included;
                        try {
                            included = parseIncluded(readBudgeted(jail.path, p.value()), jail.path);
                        } catch (ReadRefused e) {
                            continue;
                        } finally {
                            if (jail.path != null) includeLeave();
                        }
                        // The key's `:include` marker takes this result instead of reading and
                        // resolving the file a second time (2^depth for a chain of includes).
                        preIncludes.put(key, new PreInclude(p.value(), included));
                        // safety: the tree is also what `{leaf:key}` reads. Under `:secret`
                        // that scope gets the masked tree, or `x {password:db}` printed what
                        // `db:secret:include` hid.
                        includes.putIfAbsent(key, m.hasMarker("secret")
                            ? maskSecretValue(SynxObject.deepCopy(included)) : included);
                    }
                }
                for (var e : packages.entrySet()) {
                    if (!root.contains(e.getKey())) root.set(e.getKey(), SynxObject.deepCopy(e.getValue()));
                }
            }

            // Registries: the live tree before inheritance, each object's declarations first,
            // then its children in document order.
            Map<String, String> types = new HashMap<>();
            Map<String, SynxConstraints> cons = new HashMap<>();
            path.setLength(0);
            collectRegistries(rootValue, types, cons, 0);

            applyInheritance();
            if (root != null) {
                for (String k : root.keys()) if (k.startsWith("_")) root.remove(k);
            }

            // Every value under `:secret` is masked before anything can read it.
            path.setLength(0);
            maskSecretLiterals(rootValue);

            if (root != null) {
                for (String k : root.keys()) if (isCalcSpecialKey(k)) rootSpecial.add(k);
                path.setLength(0);
                resolveObject(root, 0);
            }

            validateFields(rootValue, cons, types);
            result.uses = new ArrayList<>();
        }

        SynxValue parseIncluded(String text, Path full) {
            SynxParseResult included = SynxParser.parse(text);
            if (included.mode == SynxMode.ACTIVE) {
                SynxOptions child = copyOptions(opts);
                child.includeDepth = opts.includeDepth + 1;
                Path parent = full.getParent();
                child.basePath = parent == null ? "" : parent.toString();
                SynxEngine.resolve(included, child);
            }
            return included.root;
        }

        void pushKey(String key) {
            if (path.length() > 0) path.append('.');
            SynxParser.appendKeySegment(path, key);
        }

        void pushItem(int idx) { path.append(".#").append(idx); }

        // ─── the walk ───────────────────────────────────────────────────────

        void resolveObject(SynxObject obj, int depth) {
            if (depth >= MAX_RESOLVE_DEPTH) {
                for (String k : obj.keys()) obj.set(k, SynxValue.ofString("NESTING_ERR: maximum object nesting depth exceeded"));
                return;
            }
            Map<String, SynxMeta> mm = meta.get(path.toString());
            int baseLen = path.length();
            // What this object's `:secret` keys held before the pre-pass.
            Map<String, SynxValue> originals = secretOriginals.isEmpty() ? null : secretOriginals.remove(path.toString());
            List<String> keys = obj.keys();
            for (String key : keys) {
                SynxValue original = originals == null ? null : originals.remove(key);
                if (original != null) {
                    obj.set(key, original);
                    if (original instanceof SynxValue.Obj || original instanceof SynxValue.Arr) {
                        // Secrets nested in it stay masked until their own turn.
                        pushKey(key);
                        maskSecretSubtree(original);
                        path.setLength(baseLen);
                    }
                }
                SynxValue child = obj.get(key);
                if (child instanceof SynxValue.Obj o) {
                    pushKey(key);
                    resolveObject(o.map(), depth + 1);
                    path.setLength(baseLen);
                } else if (child instanceof SynxValue.Arr a) {
                    pushKey(key);
                    int listLen = path.length();
                    List<SynxValue> items = a.values();
                    for (int idx = 0; idx < items.size(); idx++) {
                        if (items.get(idx) instanceof SynxValue.Obj io) {
                            pushItem(idx);
                            resolveObject(io.map(), depth + 1);
                            path.setLength(listLen);
                        }
                    }
                    path.setLength(baseLen);
                }
                if (mm == null) continue;
                SynxMeta m = mm.get(key);
                if (m != null) applyMarkers(obj, key, m, path.toString());
            }
            // Then `{…}` interpolation on every string value of this object.
            for (String key : obj.keys()) {
                if (obj.get(key) instanceof SynxValue.Str s && s.value().indexOf('{') >= 0) {
                    String out = resolveInterpolation(s.value(), obj);
                    if (!out.equals(s.value())) obj.set(key, SynxValue.ofString(out));
                }
            }
        }

        String lookupEnv(String name) {
            if (opts.env != null) return opts.env.get(name);
            try {
                return System.getenv(name);
            } catch (RuntimeException e) {
                return null;
            }
        }

        // ─── markers, in the reference's fixed order ────────────────────────

        void applyMarkers(SynxObject obj, String key, SynxMeta m, String metaPath) {
            List<String> markers = m.markers;
            int mb = markerBits(markers);

            // :spam:MAX[:WINDOW_SEC] target
            if ((mb & MK_SPAM) != 0) {
                String maxS = markerArg(markers, "spam", 1);
                String winS = markerArg(markers, "spam", 2);
                Long maxCalls = maxS == null ? null : SynxNumbers.parseU64(maxS);
                Long window = winS == null ? null : SynxNumbers.parseU64(winS);
                long max = maxCalls == null ? 0 : maxCalls;
                long win = window == null ? 1 : window;
                if (max == 0) {
                    obj.set(key, SynxValue.ofString("SPAM_ERR: invalid limit, use :spam:MAX[:WINDOW_SEC]"));
                    return;
                }
                SynxValue cur = obj.get(key);
                String target = cur == null ? key : valueToString(cur);
                if (!allowSpamAccess(key + "::" + target, max, win)) {
                    obj.set(key, SynxValue.ofString("SPAM_ERR: '" + target + "' exceeded "
                        + Long.toUnsignedString(max) + " calls per " + Long.toUnsignedString(win) + "s"));
                    return;
                }
                cur = obj.get(key);
                if (cur != null) {
                    String t = valueToString(cur);
                    SynxValue res = deepGet(rootValue, t);
                    if (res == null) res = obj.get(t);
                    if (res != null) obj.set(key, SynxObject.deepCopy(res));
                }
            }

            // :include / :import
            if ((mb & (MK_INCLUDE | MK_IMPORT)) != 0) {
                if (obj.get(key) instanceof SynxValue.Str fp) {
                    int maxDepth = maxIncludeDepth();
                    if (opts.includeDepth >= maxDepth) {
                        obj.set(key, SynxValue.ofString("INCLUDE_ERR: max include depth (" + maxDepth + ") exceeded"));
                        return;
                    }
                    // `:include` with `:secret` masks the leaves of the included tree.
                    boolean isSecret = (mb & MK_SECRET) != 0;
                    if (metaPath.isEmpty()) {
                        PreInclude pre = preIncludes.get(key);
                        if (pre != null && pre.path().equals(fp.value())) {
                            SynxValue copy = SynxObject.deepCopy(pre.root());
                            obj.set(key, isSecret ? maskSecretValue(copy) : copy);
                            return;
                        }
                    }
                    Jail jail = jailPath(basePath(), fp.value());
                    if (jail.error != null) {
                        obj.set(key, SynxValue.ofString("INCLUDE_ERR: " + jail.error));
                        return;
                    }
                    String sizeErr = checkFileSize(jail.path);
                    if (sizeErr != null) {
                        obj.set(key, SynxValue.ofString("INCLUDE_ERR: " + sizeErr));
                        return;
                    }
                    String openErr = jail.path == null ? null : includeEnter(jail.path, fp.value());
                    if (openErr != null) {
                        obj.set(key, SynxValue.ofString("INCLUDE_ERR: " + openErr));
                        return;
                    }
                    try {
                        SynxValue included = parseIncluded(readBudgeted(jail.path, fp.value()), jail.path);
                        obj.set(key, isSecret ? maskSecretValue(included) : included);
                    } catch (ReadRefused e) {
                        obj.set(key, SynxValue.ofString("INCLUDE_ERR: " + e.getMessage()));
                    } finally {
                        if (jail.path != null) includeLeave();
                    }
                }
                return;
            }

            // :env
            if ((mb & MK_ENV) != 0) {
                if (obj.get(key) instanceof SynxValue.Str name) {
                    String envVal = lookupEnv(name.value());
                    boolean forceString = wantsString(m);
                    int di = markers.indexOf("default");
                    if (envVal != null && !envVal.isEmpty()) {
                        obj.set(key, castEngineText(envVal, forceString));
                    } else if (di >= 0 && markers.size() > di + 1) {
                        obj.set(key, castEngineText(String.join(":", markers.subList(di + 1, markers.size())), forceString));
                    } else {
                        obj.set(key, SynxValue.ofNull());
                    }
                }
            }

            // :random
            if ((mb & MK_RANDOM) != 0) {
                if (obj.get(key) instanceof SynxValue.Arr arr) {
                    List<SynxValue> items = arr.values();
                    if (items.isEmpty()) {
                        obj.set(key, SynxValue.ofNull());
                        return;
                    }
                    SynxValue picked;
                    if (!m.args.isEmpty()) {
                        List<Double> weights = new ArrayList<>();
                        for (String a : m.args) {
                            Double w = SynxNumbers.parseF64(a);
                            if (w != null) weights.add(w);
                        }
                        picked = weightedRandom(items, weights);
                    } else {
                        picked = items.get(rng.nextInt(items.size()));
                    }
                    obj.set(key, picked);
                }
            }

            // :ref (feeds the resolved value into later markers; :ref:calc:*2)
            if ((mb & MK_REF) != 0) {
                if (obj.get(key) instanceof SynxValue.Str target) {
                    SynxValue resolved = deepGet(rootValue, target.value());
                    if (resolved == null) resolved = obj.get(target.value());
                    resolved = resolved == null ? SynxValue.ofNull() : SynxObject.deepCopy(resolved);
                    boolean done = false;
                    if ((mb & MK_CALC) != 0) {
                        Double n = valueAsNumber(resolved);
                        String calcExpr = markerArg(markers, "calc", 1);
                        if (n != null && calcExpr != null) {
                            int first = calcExpr.isEmpty() ? ' ' : calcExpr.codePointAt(0);
                            if ("+-*/%".indexOf(first) >= 0) {
                                SynxCalc.Result res = SynxCalc.evaluate(SynxNumbers.formatNumber(n) + " " + calcExpr);
                                obj.set(key, res.ok ? SynxNumbers.numberValue(res.value) : SynxValue.ofString("CALC_ERR: " + res.error));
                                done = true;
                            }
                        }
                    }
                    if (!done) obj.set(key, resolved);
                }
            }

            // :i18n[:COUNT_FIELD]
            if ((mb & MK_I18N) != 0) {
                if (obj.get(key) instanceof SynxValue.Obj tr) {
                    String lang = opts.lang == null ? "en" : opts.lang;
                    SynxValue val = tr.map().get(lang);
                    if (val == null) val = tr.map().get("en");
                    if (val == null && !tr.map().isEmpty()) val = tr.map().entriesList().get(0).getValue();
                    val = val == null ? SynxValue.ofNull() : SynxObject.deepCopy(val);
                    String countField = markerArg(markers, "i18n", 1);
                    if (countField != null && val instanceof SynxValue.Obj forms) {
                        Double count = null;
                        SynxValue local = obj.get(countField);
                        if (local != null) count = valueAsNumber(local);
                        if (count == null) {
                            SynxValue rv = deepGet(rootValue, countField);
                            if (rv != null) count = valueAsNumber(rv);
                        }
                        long n = SynxNumbers.toI64(count == null ? 0.0 : count);
                        String category = pluralCategory(lang, n);
                        SynxValue chosen = forms.map().get(category);
                        if (chosen == null) chosen = forms.map().get("other");
                        if (chosen == null && !forms.map().isEmpty()) chosen = forms.map().entriesList().get(0).getValue();
                        if (chosen == null) chosen = SynxValue.ofNull();
                        if (chosen instanceof SynxValue.Str s) {
                            obj.set(key, SynxValue.ofString(s.value().replace("{count}", Long.toString(n))));
                        } else {
                            obj.set(key, chosen);
                        }
                    } else {
                        obj.set(key, val);
                    }
                }
            }

            // :calc
            if ((mb & MK_CALC) != 0) {
                if (obj.get(key) instanceof SynxValue.Str expr) {
                    long exprBytes = SynxParser.utf8Len(expr.value(), 0, expr.value().length());
                    if (exprBytes > MAX_CALC_EXPR_LEN) {
                        obj.set(key, SynxValue.ofString("CALC_ERR: expression too long (" + exprBytes
                            + " chars, max " + MAX_CALC_EXPR_LEN + ")"));
                        return;
                    }
                    String resolved = substituteCalcVars(expr.value(), obj, key);
                    if (resolved != null) resolved = substituteCalcPaths(resolved);
                    if (resolved == null) {
                        obj.set(key, SynxValue.ofString(CALC_TOO_LONG));
                        return;
                    }
                    SynxCalc.Result res = SynxCalc.evaluate(resolved);
                    obj.set(key, res.ok ? SynxNumbers.numberValue(res.value) : SynxValue.ofString("CALC_ERR: " + res.error));
                }
            }

            // :alias
            if ((mb & MK_ALIAS) != 0) {
                if (obj.get(key) instanceof SynxValue.Str t) {
                    String target = t.value();
                    // A list item's `#<index>` segment is a metadata detail, not part of a path a
                    // document can name.
                    StringBuilder cp = new StringBuilder();
                    for (String seg : SynxParser.metaPathSegments(metaPath)) {
                        if (seg.startsWith("#")) continue;
                        cp.append(seg).append('.');
                    }
                    cp.append(key);
                    String currentPath = cp.toString();
                    if (target.equals(key) || target.equals(currentPath)) {
                        obj.set(key, SynxValue.ofString("ALIAS_ERR: self-referential alias: " + currentPath + " → " + target));
                    } else {
                        SynxValue targetVal = deepGet(rootValue, target);
                        int dot = target.lastIndexOf('.');
                        String targetParent = dot >= 0 ? target.substring(0, dot) : "";
                        String targetKey = dot >= 0 ? target.substring(dot + 1) : target;
                        Map<String, SynxMeta> tm = meta.get(targetParent);
                        boolean targetHasAlias = tm != null && tm.get(targetKey) != null && tm.get(targetKey).hasMarker("alias");
                        boolean isCycle = targetHasAlias && targetVal instanceof SynxValue.Str s
                            && (s.value().equals(key) || s.value().equals(currentPath));
                        if (isCycle) {
                            String a = currentPath, b = target;
                            if (SynxParser.compareCodePoints(b, a) < 0) { String x = a; a = b; b = x; }
                            obj.set(key, SynxValue.ofString("ALIAS_ERR: circular alias detected: " + a + " → " + b));
                        } else {
                            obj.set(key, targetVal == null ? SynxValue.ofNull() : SynxObject.deepCopy(targetVal));
                        }
                    }
                }
            }

            // :unique
            if ((mb & MK_UNIQUE) != 0) {
                if (obj.get(key) instanceof SynxValue.Arr arr) {
                    Set<String> seen = new HashSet<>();
                    List<SynxValue> unique = new ArrayList<>();
                    for (SynxValue item : arr.values()) {
                        if (seen.add(valueToString(item))) unique.add(item);
                    }
                    obj.set(key, SynxValue.ofArray(unique));
                }
            }

            // :geo
            if ((mb & MK_GEO) != 0) {
                if (obj.get(key) instanceof SynxValue.Arr arr) {
                    String region = opts.region == null ? "US" : opts.region;
                    String prefix = region + " ";
                    SynxValue res = null;
                    for (SynxValue item : arr.values()) {
                        if (item instanceof SynxValue.Str s && s.value().startsWith(prefix)) {
                            res = SynxValue.ofString(SynxParser.trim(s.value().substring(prefix.length())));
                            break;
                        }
                    }
                    if (res == null && !arr.values().isEmpty()) {
                        SynxValue first = arr.values().get(0);
                        res = first;
                        if (first instanceof SynxValue.Str s) {
                            int sp = s.value().indexOf(' ');
                            if (sp >= 0) res = SynxValue.ofString(SynxParser.trim(s.value().substring(sp + 1)));
                        }
                    }
                    obj.set(key, res == null ? SynxValue.ofNull() : res);
                }
            }

            // :split[:DELIM]
            if ((mb & MK_SPLIT) != 0) {
                if (obj.get(key) instanceof SynxValue.Str s) {
                    String d = markerArg(markers, "split", 1);
                    String sep = d == null ? "," : delimiterFromKeyword(d);
                    List<SynxValue> items = new ArrayList<>();
                    for (String part : splitLiteral(s.value(), sep)) {
                        String p = SynxParser.trim(part);
                        if (!p.isEmpty()) items.add(castPrimitive(p));
                    }
                    obj.set(key, SynxValue.ofArray(items));
                }
            }

            // :join[:DELIM]
            if ((mb & MK_JOIN) != 0) {
                if (obj.get(key) instanceof SynxValue.Arr arr) {
                    String d = markerArg(markers, "join", 1);
                    String sep = d == null ? "," : delimiterFromKeyword(d);
                    StringBuilder sb = new StringBuilder();
                    List<SynxValue> items = arr.values();
                    for (int i = 0; i < items.size(); i++) {
                        if (i > 0) sb.append(sep);
                        sb.append(valueToString(items.get(i)));
                    }
                    obj.set(key, SynxValue.ofString(sb.toString()));
                }
            }

            // :default (standalone, without :env)
            if ((mb & MK_DEFAULT) != 0 && (mb & MK_ENV) == 0) {
                SynxValue v = obj.get(key);
                boolean empty = v == null || v instanceof SynxValue.Null || (v instanceof SynxValue.Str s && s.value().isEmpty());
                int di = markers.indexOf("default");
                if (empty && markers.size() > di + 1) {
                    obj.set(key, castEngineText(String.join(":", markers.subList(di + 1, markers.size())), wantsString(m)));
                }
            }

            // :clamp:MIN:MAX
            if ((mb & MK_CLAMP) != 0) {
                String minS = markerArg(markers, "clamp", 1);
                String maxS = markerArg(markers, "clamp", 2);
                // A bound must be a finite number: `nan`, `inf` and `1e400` skip the marker.
                Double lo = SynxNumbers.parseF64(minS == null ? "" : minS);
                Double hi = SynxNumbers.parseF64(maxS == null ? "" : maxS);
                if (lo != null && hi != null && Double.isFinite(lo) && Double.isFinite(hi)) {
                    if (lo > hi) {
                        obj.set(key, SynxValue.ofString("CONSTRAINT_ERR: clamp min (" + SynxNumbers.display(lo)
                            + ") > max (" + SynxNumbers.display(hi) + ")"));
                    } else {
                        Double n = valueAsNumber(obj.get(key));
                        if (n != null) {
                            double c = n;
                            if (c < lo) c = lo;
                            if (c > hi) c = hi;
                            obj.set(key, SynxNumbers.numberValue(c));
                        }
                    }
                }
            }

            // :round[:N]
            if ((mb & MK_ROUND) != 0) {
                long decimals = 0;
                String d = markerArg(markers, "round", 1);
                if (d != null) {
                    Long n = SynxNumbers.parseU64(d);
                    if (n != null && Long.compareUnsigned(n, 0xFFFF_FFFFL) <= 0) decimals = n;
                }
                Double n = valueAsNumber(obj.get(key));
                if (n != null) {
                    // A scale past the range of f64 rounds nothing: the number stays.
                    double factor = powi(10, (int) Math.min(decimals, 400));
                    double scaled = n * factor;
                    double rounded = Double.isFinite(scaled) ? roundHalfAway(scaled) / factor : n;
                    // safety: only a whole number that fits is an integer. The cast alone
                    // saturates: `:round` of 1e30 was 9223372036854775807 and of NaN was 0 —
                    // invented values, where `:calc` and `:clamp` keep the float.
                    obj.set(key, decimals == 0 && SynxNumbers.isWhole(rounded) && Math.abs(rounded) < SynxNumbers.I64_MAX_F
                        ? SynxValue.ofInt(SynxNumbers.toI64(rounded)) : SynxValue.ofFloat(rounded));
                }
            }

            // :map[:SOURCE_KEY] — `- lookup result` entries
            if ((mb & MK_MAP) != 0) {
                if (obj.get(key) instanceof SynxValue.Arr arr) {
                    String sourceKey = markerArg(markers, "map", 1);
                    String lookup = "";
                    if (sourceKey != null && !sourceKey.isEmpty()) {
                        SynxValue res = deepGet(rootValue, sourceKey);
                        if (res == null) res = obj.get(sourceKey);
                        if (res != null) lookup = valueToString(res);
                    }
                    SynxValue res = SynxValue.ofNull();
                    for (SynxValue item : arr.values()) {
                        if (!(item instanceof SynxValue.Str s)) continue;
                        int sp = s.value().indexOf(' ');
                        if (sp >= 0 && SynxParser.trim(s.value().substring(0, sp)).equals(lookup)) {
                            res = castPrimitive(SynxParser.trim(s.value().substring(sp + 1)));
                            break;
                        }
                    }
                    obj.set(key, res);
                }
            }

            // :format:PATTERN
            if ((mb & MK_FORMAT) != 0) {
                String pattern = markerArg(markers, "format", 1);
                if (pattern == null) pattern = "%s";
                SynxValue v = obj.get(key);
                if (v != null) obj.set(key, SynxValue.ofString(applyFormatPattern(pattern, v)));
            }

            // :replace:FROM:TO
            if ((mb & MK_REPLACE) != 0) {
                if (obj.get(key) instanceof SynxValue.Str s) {
                    String from = markerArg(markers, "replace", 1);
                    String to = markerArg(markers, "replace", 2);
                    if (from != null && !from.isEmpty()) {
                        obj.set(key, SynxValue.ofString(s.value().replace(from, to == null ? "" : to)));
                    }
                }
            }

            // :sort / :sort:desc — numbers numerically, anything else by code point
            if ((mb & MK_SORT) != 0) {
                if (obj.get(key) instanceof SynxValue.Arr arr) {
                    boolean desc = "desc".equals(markerArg(markers, "sort", 1));
                    List<SynxValue> sorted = new ArrayList<>(arr.values());
                    stableSort(sorted);
                    if (desc) java.util.Collections.reverse(sorted);
                    obj.set(key, SynxValue.ofArray(sorted));
                }
            }

            // :sum
            if ((mb & MK_SUM) != 0) {
                if (obj.get(key) instanceof SynxValue.Arr arr) {
                    double total = 0;
                    boolean allInt = true;
                    for (SynxValue item : arr.values()) {
                        if (item instanceof SynxValue.Int i) {
                            total += (double) i.value();
                        } else if (item instanceof SynxValue.Float f) {
                            total += f.value();
                            if (!SynxNumbers.isWhole(f.value())) allInt = false;
                        } else if (item instanceof SynxValue.Str s) {
                            // `inf` and `nan` are text here as everywhere (see castPrimitive).
                            Double f = SynxNumbers.parseF64(s.value());
                            if (f != null && Double.isFinite(f)) {
                                total += f;
                                if (!SynxNumbers.isWhole(f)) allInt = false;
                            }
                        }
                    }
                    obj.set(key, allInt && SynxNumbers.isWhole(total) && Math.abs(total) < SynxNumbers.I64_MAX_F
                        ? SynxValue.ofInt(SynxNumbers.toI64(total)) : SynxValue.ofFloat(total));
                }
            }

            // :fallback:DEFAULT — when the value (a path) is empty or missing
            if ((mb & MK_FALLBACK) != 0) {
                String defaultVal = markerArg(markers, "fallback", 1);
                if (defaultVal == null) defaultVal = "";
                SynxValue v = obj.get(key);
                boolean useFallback;
                if (v == null || v instanceof SynxValue.Null) {
                    useFallback = true;
                } else if (v instanceof SynxValue.Str s) {
                    if (s.value().isEmpty()) {
                        useFallback = true;
                    } else {
                        Jail jail = jailPath(basePath(), s.value());
                        useFallback = jail.error != null || !exists(jail.path);
                    }
                } else {
                    useFallback = false;
                }
                if (useFallback && !defaultVal.isEmpty()) obj.set(key, SynxValue.ofString(defaultVal));
            }

            // :once[:uuid|timestamp|random]
            if ((mb & MK_ONCE) != 0) {
                String genType = markerArg(markers, "once", 1);
                if (genType == null) genType = "uuid";
                Path lockPath = opts.basePath == null ? Path.of(".synx.lock") : joinPath(opts.basePath, ".synx.lock");
                String locked = lockPath == null ? null : readLockValue(lockPath, key);
                if (locked != null) {
                    obj.set(key, SynxValue.ofString(locked));
                } else {
                    String generated = generateOnce(genType);
                    if (lockPath != null) writeLockValue(lockPath, key, generated);
                    obj.set(key, SynxValue.ofString(generated));
                }
            }

            // :version:OP:REQUIRED
            if ((mb & MK_VERSION) != 0) {
                if (obj.get(key) instanceof SynxValue.Str cur) {
                    String op = markerArg(markers, "version", 1);
                    String required = markerArg(markers, "version", 2);
                    obj.set(key, SynxValue.ofBool(compareVersions(cur.value(), op == null ? ">=" : op,
                        required == null ? "" : required)));
                }
            }

            // :watch[:KEY_PATH]
            if ((mb & MK_WATCH) != 0) {
                if (obj.get(key) instanceof SynxValue.Str fp) {
                    int maxDepth = maxIncludeDepth();
                    if (opts.includeDepth >= maxDepth) {
                        obj.set(key, SynxValue.ofString("WATCH_ERR: max include depth (" + maxDepth + ") exceeded"));
                        return;
                    }
                    Jail jail = jailPath(basePath(), fp.value());
                    if (jail.error != null) {
                        obj.set(key, SynxValue.ofString("WATCH_ERR: " + jail.error));
                        return;
                    }
                    String sizeErr = checkFileSize(jail.path);
                    if (sizeErr != null) {
                        obj.set(key, SynxValue.ofString("WATCH_ERR: " + sizeErr));
                        return;
                    }
                    String keyPath = markerArg(markers, "watch", 1);
                    try {
                        String content = readBudgeted(jail.path, fp.value());
                        if (keyPath != null) {
                            SynxValue v = extractFromFileContent(content, keyPath, fileExtension(jail.path));
                            obj.set(key, v == null ? SynxValue.ofNull() : v);
                        } else {
                            obj.set(key, SynxValue.ofString(SynxParser.trim(content)));
                        }
                    } catch (ReadRefused e) {
                        obj.set(key, SynxValue.ofString("WATCH_ERR: " + e.getMessage()));
                    }
                }
            }

            // :secret
            // Last of the markers that produce the value, so whatever they resolved is what
            // gets masked. It used to sit after `:alias`: `t:secret:once` came out as plain
            // text, `:secret:default:x` and `:secret:watch` masked the argument instead of the
            // result, and `:secret:format` formatted the mask. `:prompt` and custom markers
            // below still see only the mask.
            if ((mb & MK_SECRET) != 0) {
                SynxValue v = obj.get(key);
                if (v != null) obj.set(key, maskSecretValue(v));
            }

            // :prompt[:LABEL]
            if ((mb & MK_PROMPT) != 0) {
                String label = markerArg(markers, "prompt", 1);
                if (label == null) label = key;
                SynxValue v = obj.get(key);
                if (v != null) obj.set(key, SynxValue.ofString(label + " (SYNX):\n```synx\n" + stringifyPrompt(v, 0) + "```"));
            }

            // custom markers (stand-in for the reference's WASM markers)
            if (opts.markerFns != null && !opts.markerFns.isEmpty()) {
                for (String mk : markers) {
                    if (BUILTIN_MARKERS.contains(mk)) continue;
                    SynxMarkerFn fn = opts.markerFns.get(mk);
                    if (fn == null) continue;
                    SynxValue cur = obj.get(key);
                    SynxValue out = fn.apply(key, m.args, cur == null ? SynxValue.ofNull() : cur);
                    obj.set(key, out == null ? SynxValue.ofNull() : out);
                }
            }

            // constraints, always last
            if (m.constraints != null) validateConstraints(obj, key, m.constraints);
        }

        // ─── :calc substitution ─────────────────────────────────────────────

        /**
         * Puts numbers into a {@code :calc} expression: a key of the current object first (it
         * shadows the root), then a root key — one pass over the expression's words. Only when a
         * key a word cannot name holds a number does it fall back to the exact sequential
         * whole-word replacement. {@code null} when the result grows past the cap.
         */
        String substituteCalcVars(String expr, SynxObject local, String selfKey) {
            boolean localIsRoot = root != null && root == local;
            boolean special = false;
            if (root != null) {
                for (String k : rootSpecial) {
                    if (valueAsNumber(root.get(k)) != null) { special = true; break; }
                }
            }
            if (!special && !localIsRoot) {
                for (var e : local) {
                    if (!e.getKey().equals(selfKey) && valueAsNumber(e.getValue()) != null && isCalcSpecialKey(e.getKey())) {
                        special = true;
                        break;
                    }
                }
            }
            if (special) {
                String resolved = expr;
                List<SynxObject> maps = new ArrayList<>();
                maps.add(local);
                if (!localIsRoot && root != null) maps.add(root);
                for (SynxObject mm : maps) {
                    for (var e : mm) {
                        if (mm == local && e.getKey().equals(selfKey)) continue;
                        Double n = valueAsNumber(e.getValue());
                        if (n == null) continue;
                        resolved = replaceWord(resolved, e.getKey(), SynxNumbers.formatNumber(n));
                        if (SynxParser.utf8Len(resolved, 0, resolved.length()) > MAX_CALC_RESOLVED_LEN) return null;
                    }
                }
                return resolved;
            }

            StringBuilder out = new StringBuilder(expr.length() + 16);
            long bytes = 0;
            int i = 0;
            int n = expr.length();
            while (i < n) {
                char c = expr.charAt(i);
                if (isWordChar(c)) {
                    int start = i;
                    while (i < n && isWordChar(expr.charAt(i))) i++;
                    String token = expr.substring(start, i);
                    Double num = null;
                    if (!token.equals(selfKey)) num = valueAsNumber(local.get(token));
                    if (num == null && root != null) num = valueAsNumber(root.get(token));
                    String text = num != null ? SynxNumbers.formatNumber(num) : token;
                    out.append(text);
                    bytes += text.length();
                } else {
                    int cp = expr.codePointAt(i);
                    int w = Character.charCount(cp);
                    out.append(expr, i, i + w);
                    bytes += SynxParser.utf8Len(expr, i, i + w);
                    i += w;
                }
                if (bytes > MAX_CALC_RESOLVED_LEN) return null;
            }
            return out.toString();
        }

        /** Replaces dot-path references ({@code stats.base_hp}) with the number they name. */
        String substituteCalcPaths(String resolved) {
            if (resolved.indexOf('.') < 0) return resolved;
            StringBuilder out = new StringBuilder(resolved.length());
            long bytes = 0;
            int i = 0;
            int n = resolved.length();
            while (i < n) {
                char c = resolved.charAt(i);
                if (isWordChar(c)) {
                    int start = i;
                    boolean hasDot = false;
                    while (i < n && (isWordChar(resolved.charAt(i)) || resolved.charAt(i) == '.')) {
                        if (resolved.charAt(i) == '.') hasDot = true;
                        i++;
                    }
                    String token = resolved.substring(start, i);
                    String text = token;
                    if (hasDot) {
                        Double num = valueAsNumber(deepGet(rootValue, token));
                        if (num != null) text = SynxNumbers.formatNumber(num);
                    }
                    out.append(text);
                    bytes += text.length();
                } else {
                    int w = Character.charCount(resolved.codePointAt(i));
                    out.append(resolved, i, i + w);
                    bytes += SynxParser.utf8Len(resolved, i, i + w);
                    i += w;
                }
                if (bytes > MAX_CALC_RESOLVED_LEN) return null;
            }
            return out.toString();
        }

        // ─── interpolation ──────────────────────────────────────────────────

        String resolveInterpolation(String tpl, SynxObject local) {
            StringBuilder out = new StringBuilder(tpl.length());
            long bytes = 0;
            int i = 0;
            int n = tpl.length();
            while (i < n) {
                if (bytes >= MAX_ENGINE_SCRATCH) break;
                if (tpl.charAt(i) == '{') {
                    int close = tpl.indexOf('}', i + 1);
                    if (close >= 0) {
                        String inner = tpl.substring(i + 1, close);
                        int colon = inner.indexOf(':');
                        String name = colon >= 0 ? inner.substring(0, colon) : inner;
                        if (isRefName(name)) {
                            SynxValue val;
                            if (colon >= 0) {
                                String scope = inner.substring(colon + 1);
                                val = null;
                                if (scope.equals("include")) {
                                    if (includes.size() == 1) val = deepGet(includes.values().iterator().next(), name);
                                } else {
                                    SynxValue inc = includes.get(scope);
                                    if (inc != null) val = deepGet(inc, name);
                                }
                            } else {
                                val = deepGet(rootValue, name);
                                if (val == null) val = local.get(name);
                            }
                            if (val != null) {
                                bytes = appendCapped(out, bytes, valueToString(val), MAX_ENGINE_SCRATCH);
                            } else {
                                out.append('{');
                                bytes++;
                                bytes = appendCapped(out, bytes, inner, MAX_ENGINE_SCRATCH - 1);
                                if (bytes < MAX_ENGINE_SCRATCH) { out.append('}'); bytes++; }
                            }
                            i = close + 1;
                            continue;
                        }
                    }
                }
                int w = Character.charCount(tpl.codePointAt(i));
                if (bytes < MAX_ENGINE_SCRATCH) {
                    out.append(tpl, i, i + w);
                    bytes += SynxParser.utf8Len(tpl, i, i + w);
                }
                i += w;
            }
            return out.toString();
        }

        // ─── inheritance and secrets ────────────────────────────────────────

        /**
         * {@code child:inherit:p1:p2} merges parents left to right, the child overriding all. Each
         * field keeps the declarations of the object its value came from; inherited nested subtrees
         * are copies carrying their own declarations; a list or scalar child is left as written.
         */
        void applyInheritance() {
            Map<String, SynxMeta> rootMeta = meta.get("");
            if (rootMeta == null || root == null) return;
            List<String[]> childAndParents = new ArrayList<>();
            List<List<String>> parentsList = new ArrayList<>();
            for (var e : rootMeta.entrySet()) {
                SynxMeta m = e.getValue();
                int idx = m.markers.indexOf("inherit");
                if (idx < 0) continue;
                List<String> parents = new ArrayList<>(m.markers.subList(idx + 1, m.markers.size()));
                if (parents.isEmpty() && !m.args.isEmpty()) parents = new ArrayList<>(m.args);
                if (!parents.isEmpty()) {
                    childAndParents.add(new String[] { e.getKey() });
                    parentsList.add(parents);
                }
            }
            for (int ix = 0; ix < childAndParents.size(); ix++) {
                String childKey = childAndParents.get(ix)[0];
                if (!(root.get(childKey) instanceof SynxValue.Obj child)) continue;
                SynxObject merged = new SynxObject();
                Map<String, String> source = new HashMap<>(); // field → parent; absent = the child
                for (String pn : parentsList.get(ix)) {
                    if (!(root.get(pn) instanceof SynxValue.Obj parent)) continue;
                    for (var e : parent.map()) {
                        merged.set(e.getKey(), SynxObject.deepCopy(e.getValue()));
                        source.put(e.getKey(), pn);
                    }
                }
                for (var e : child.map()) {
                    merged.set(e.getKey(), e.getValue());
                    source.remove(e.getKey());
                }

                Map<String, SynxMeta> top = new LinkedHashMap<>();
                List<String> nestedPaths = new ArrayList<>();
                List<Map<String, SynxMeta>> nestedMeta = new ArrayList<>();
                String childSeg = SynxParser.keySegment(childKey);
                for (String k : merged.keys()) {
                    String parentName = source.get(k);
                    String from = parentName == null ? childKey : parentName;
                    Map<String, SynxMeta> mm = meta.get(SynxParser.keySegment(from));
                    if (mm != null && mm.get(k) != null) top.put(k, copyMeta(mm.get(k)));
                    if (parentName != null) {
                        String prefix = SynxParser.keySegment(parentName) + "." + SynxParser.keySegment(k);
                        for (var pe : meta.entrySet()) {
                            String p = pe.getKey();
                            if (!p.startsWith(prefix)) continue;
                            String rest = p.substring(prefix.length());
                            if (!rest.isEmpty() && rest.charAt(0) != '.') continue;
                            nestedPaths.add(childSeg + "." + SynxParser.keySegment(k) + rest);
                            nestedMeta.add(copyMetaMap(pe.getValue()));
                        }
                    }
                }
                root.set(childKey, SynxValue.ofObject(merged));
                if (top.isEmpty()) meta.remove(childSeg);
                else meta.put(childSeg, top);
                for (int j = 0; j < nestedPaths.size(); j++) meta.put(nestedPaths.get(j), nestedMeta.get(j));
            }
        }

        /**
         * The {@code :secret} pre-pass: every value under {@code :secret} is masked before any
         * resolution, so no field that names it — <code>{that_key}</code>, {@code :alias},
         * {@code :map} — reads the plain text, wherever in the document it stands. What the
         * key held is kept in {@link #secretOriginals} and comes back for the key's own turn
         * (its subtree and its marker chain); what that produces is masked again.
         *
         * <p>safety: a literal next to {@code :default}, {@code :replace}, {@code :format},
         * {@code :split}, … was left in the clear until its own turn, and the fields of a
         * group under {@code :secret} were never masked here: {@code leak:alias password}
         * above {@code password:secret:default:x hunter2} read {@code hunter2}.
         */
        void maskSecretLiterals(SynxValue v) {
            if (!(v instanceof SynxValue.Obj o)) return;
            SynxObject obj = o.map();
            Map<String, SynxMeta> mm = meta.get(path.toString());
            if (mm != null) {
                for (var e : mm.entrySet()) {
                    if (!e.getValue().hasMarker("secret")) continue;
                    SynxValue val = obj.get(e.getKey());
                    if (val == null || val instanceof SynxValue.Secret || val instanceof SynxValue.Null) continue;
                    obj.set(e.getKey(), maskSecretValue(SynxObject.deepCopy(val)));
                    secretOriginals.computeIfAbsent(path.toString(), k -> new HashMap<>()).put(e.getKey(), val);
                }
            }
            int baseLen = path.length();
            for (String key : obj.keys()) {
                SynxValue child = obj.get(key);
                if (child instanceof SynxValue.Obj || child instanceof SynxValue.Arr) {
                    pushKey(key);
                    maskSecretSubtree(child);
                    path.setLength(baseLen);
                }
            }
        }

        /** {@link #maskSecretLiterals} for the object or list at the current path. */
        void maskSecretSubtree(SynxValue child) {
            if (child instanceof SynxValue.Obj) {
                maskSecretLiterals(child);
            } else if (child instanceof SynxValue.Arr a) {
                int listLen = path.length();
                List<SynxValue> items = a.values();
                for (int idx = 0; idx < items.size(); idx++) {
                    if (items.get(idx) instanceof SynxValue.Obj) {
                        pushItem(idx);
                        maskSecretLiterals(items.get(idx));
                        path.setLength(listLen);
                    }
                }
            }
        }

        // ─── registries and validation ──────────────────────────────────────

        void collectRegistries(SynxValue v, Map<String, String> types, Map<String, SynxConstraints> cons, int depth) {
            if (depth > MAX_RESOLVE_DEPTH || !(v instanceof SynxValue.Obj o)) return;
            Map<String, SynxMeta> mm = meta.get(path.toString());
            if (mm != null) {
                for (var e : mm.entrySet()) {
                    SynxMeta m = e.getValue();
                    if (m.typeHint != null) types.putIfAbsent(e.getKey(), m.typeHint);
                    if (m.constraints != null) {
                        SynxConstraints existing = cons.get(e.getKey());
                        if (existing == null) cons.put(e.getKey(), copyConstraints(m.constraints));
                        else mergeConstraints(existing, m.constraints);
                    }
                }
            }
            int baseLen = path.length();
            for (var e : o.map()) {
                SynxValue child = e.getValue();
                if (child instanceof SynxValue.Obj) {
                    pushKey(e.getKey());
                    collectRegistries(child, types, cons, depth + 1);
                    path.setLength(baseLen);
                } else if (child instanceof SynxValue.Arr a) {
                    pushKey(e.getKey());
                    int listLen = path.length();
                    List<SynxValue> items = a.values();
                    for (int idx = 0; idx < items.size(); idx++) {
                        if (items.get(idx) instanceof SynxValue.Obj) {
                            pushItem(idx);
                            collectRegistries(items.get(idx), types, cons, depth + 1);
                            path.setLength(listLen);
                        }
                    }
                    path.setLength(baseLen);
                }
            }
        }

        void report(String msg) {
            if (opts.strict) System.err.println("synx: " + msg);
        }

        void validateConstraints(SynxObject obj, String key, SynxConstraints c) {
            SynxValue v = obj.get(key);
            if (v == null) {
                if (c.required) {
                    String msg = "CONSTRAINT_ERR: '" + key + "' is required";
                    obj.set(key, SynxValue.ofString(msg));
                    report(msg);
                }
                return;
            }
            String msg = constraintViolation(key, v, c);
            if (msg != null) {
                obj.set(key, SynxValue.ofString(msg));
                report(msg);
            }
        }

        /** Constraints, then type, for every field of the tree; an existing error is not re-measured. */
        void validateFields(SynxValue v, Map<String, SynxConstraints> cons, Map<String, String> types) {
            if (cons.isEmpty() && types.isEmpty()) return;
            if (!(v instanceof SynxValue.Obj o)) return;
            SynxObject obj = o.map();
            for (String key : obj.keys()) {
                SynxValue val = obj.get(key);
                SynxConstraints c = cons.get(key);
                if (c != null && !isFieldError(val)) {
                    String msg = constraintViolation(key, val, c);
                    if (msg != null) {
                        val = SynxValue.ofString(msg);
                        obj.set(key, val);
                        report(msg);
                    }
                }
                String want = types.get(key);
                if (want != null && !valueMatchesType(val, want)) {
                    String msg = "TYPE_ERR: '" + key + "' expected " + want + " but got " + val.typeName();
                    val = SynxValue.ofString(msg);
                    obj.set(key, val);
                    report(msg);
                }
                if (val instanceof SynxValue.Obj) {
                    validateFields(val, cons, types);
                } else if (val instanceof SynxValue.Arr a) {
                    for (SynxValue item : a.values()) {
                        if (item instanceof SynxValue.Obj) validateFields(item, cons, types);
                    }
                }
            }
        }

        // ─── helpers needing resolver state ─────────────────────────────────

        SynxValue weightedRandom(List<SynxValue> items, List<Double> weights) {
            List<Double> w = new ArrayList<>(weights);
            if (w.size() < items.size()) {
                double assigned = 0;
                for (double x : w) assigned += x;
                double perItem = assigned < 100 ? (100 - assigned) / (items.size() - w.size()) : assigned / w.size();
                while (w.size() < items.size()) w.add(perItem);
            }
            double total = 0;
            for (double x : w) total += x;
            if (total <= 0) return items.get(rng.nextInt(items.size()));
            double roll = rng.nextInt(10_000) / 10_000.0;
            double cumulative = 0;
            for (int i = 0; i < items.size(); i++) {
                cumulative += w.get(i) / total;
                if (roll <= cumulative) return items.get(i);
            }
            return items.get(items.size() - 1);
        }

        String generateOnce(String genType) {
            switch (genType) {
                case "timestamp":
                    return Long.toString(System.currentTimeMillis() / 1000);
                case "random":
                    return Long.toString(Long.remainderUnsigned(rng.nextLong(), 0xFFFF_FFFFL));
                default: {
                    long hi = rng.nextLong(), lo = rng.nextLong();
                    long timeHi = ((hi >>> 16) & 0x0FFF) | 0x4000;
                    long clkSeq = ((lo >>> 48) & 0x3FFF) | 0x8000;
                    return String.format("%08x-%04x-%04x-%04x-%012x",
                        (hi >>> 32) & 0xFFFF_FFFFL, (hi >>> 16) & 0xFFFF, timeHi & 0xFFFF, clkSeq & 0xFFFF,
                        lo & 0x0000_FFFF_FFFF_FFFFL);
                }
            }
        }

        Map<String, SynxValue> loadPackages(List<SynxUseDirective> directives) {
            Map<String, SynxValue> out = new LinkedHashMap<>();
            // safety: a package may `!use` another. The depth was counted but never checked,
            // and nothing noticed a package reaching itself: packages using one another
            // overflowed the stack.
            if (directives == null || directives.isEmpty() || opts.includeDepth >= maxIncludeDepth()) return out;
            String pkgBase = opts.packagesPath == null ? "./synx_packages" : opts.packagesPath;
            Path pkgRoot = joinPath(basePath(), pkgBase);
            if (pkgRoot == null) return out;
            for (SynxUseDirective ud : directives) {
                if (!isContainedRelative(ud.pkg())) continue;
                Path pkgDir = joinPath(pkgRoot.toString(), ud.pkg());
                if (pkgDir == null) continue;
                Path entry = readManifestMain(pkgDir);
                if (entry == null) entry = pkgDir.resolve("src").resolve("main.synx");
                if (!Files.isRegularFile(entry) || checkFileSize(entry) != null) continue;
                Path opened;
                try { opened = entry.toRealPath(); } catch (IOException | RuntimeException e) { opened = entry; }
                if (includeEnter(opened, ud.pkg()) != null) continue;
                try {
                    out.put(ud.alias(), parseIncluded(readBudgeted(entry, ud.pkg()), entry));
                } catch (ReadRefused e) {
                    continue;
                } finally {
                    includeLeave();
                }
            }
            return out;
        }

        void loadIncludes(List<SynxIncludeDirective> directives) {
            if (directives == null || opts.includeDepth >= maxIncludeDepth()) return;
            for (SynxIncludeDirective inc : directives) {
                Jail jail = jailPath(basePath(), inc.path());
                if (jail.error != null || checkFileSize(jail.path) != null) continue;
                if (jail.path != null && includeEnter(jail.path, inc.path()) != null) continue;
                try {
                    includes.put(inc.alias(), parseIncluded(readBudgeted(jail.path, inc.path()), jail.path));
                } catch (ReadRefused e) {
                    continue;
                } finally {
                    if (jail.path != null) includeLeave();
                }
            }
        }
    }

    // ─── constraints ────────────────────────────────────────────────────────

    /** The error a value earns against its constraints, or {@code null}; the first failing check wins. */
    static String constraintViolation(String key, SynxValue val, SynxConstraints c) {
        // A key written without a value parses as an empty group; for `[required]` that is
        // the missing value, not a present object.
        boolean isMissing = val instanceof SynxValue.Null
            || (val instanceof SynxValue.Str s && s.value().isEmpty())
            || (val instanceof SynxValue.Obj o && o.map().isEmpty());
        if (c.required && isMissing) {
            return "CONSTRAINT_ERR: '" + key + "' is required";
        }
        // An absent value (null) is checked against `enum` only, read as an empty string.
        if (val instanceof SynxValue.Null) {
            if (c.enumValues != null && !c.enumValues.contains("")) return enumError(key, c);
            return null;
        }
        if (c.typeName != null) {
            boolean ok;
            switch (c.typeName) {
                case "int":    ok = val instanceof SynxValue.Int; break;
                case "float":  ok = val instanceof SynxValue.Float || val instanceof SynxValue.Int; break;
                case "bool":   ok = val instanceof SynxValue.Bool; break;
                // A `:secret` is a masked string: `[type:string]:secret:env` accepts it.
                case "string": ok = val instanceof SynxValue.Str || val instanceof SynxValue.Secret; break;
                default:       ok = true;
            }
            if (!ok) return "CONSTRAINT_ERR: '" + key + "' expected type '" + c.typeName + "'";
        }
        if (c.enumValues != null) {
            String text;
            if (val instanceof SynxValue.Str s) text = s.value();
            else if (val instanceof SynxValue.Int i) text = Long.toString(i.value());
            else if (val instanceof SynxValue.Float f) text = SynxNumbers.display(f.value());
            else if (val instanceof SynxValue.Bool b) text = b.value() ? "true" : "false";
            else text = "";
            if (!c.enumValues.contains(text)) return enumError(key, c);
        }
        // Numbers by value; a string's size is its count of Unicode scalar values.
        Double n = null;
        if (val instanceof SynxValue.Int i) n = (double) i.value();
        else if (val instanceof SynxValue.Float f) n = f.value();
        else if (val instanceof SynxValue.Str s && (c.min != null || c.max != null)) {
            n = (double) s.value().codePointCount(0, s.value().length());
        }
        if (n != null) {
            if (c.min != null && n < c.min) {
                return "CONSTRAINT_ERR: '" + key + "' value " + SynxNumbers.display(n) + " is below min " + SynxNumbers.display(c.min);
            }
            if (c.max != null && n > c.max) {
                return "CONSTRAINT_ERR: '" + key + "' value " + SynxNumbers.display(n) + " exceeds max " + SynxNumbers.display(c.max);
            }
        }
        // Over-long, invalid and backtracking-prone patterns never reject.
        if (c.pattern != null && val instanceof SynxValue.Str s
                && SynxParser.utf8Len(c.pattern, 0, c.pattern.length()) <= 256 && patternRejects(c.pattern, s.value())) {
            return "CONSTRAINT_ERR: '" + key + "' does not match pattern /" + c.pattern + "/";
        }
        return null;
    }

    private static String enumError(String key, SynxConstraints c) {
        return "CONSTRAINT_ERR: '" + key + "' must be one of [" + String.join("|", c.enumValues) + "]";
    }

    private static final int MAX_CACHED_PATTERNS = 256;
    private static final Pattern NO_MATCHER = Pattern.compile("");
    private static final Map<String, Pattern> PATTERNS = new ConcurrentHashMap<>();

    /**
     * Whether {@code s} fails {@code [pattern:…]}. Compiled patterns are cached (bounded). A
     * pattern the reference's {@code regex} crate refuses — nested quantifiers, look-around,
     * back-references, atomic groups, possessive quantifiers — never rejects.
     */
    static boolean patternRejects(String pat, String s) {
        Pattern re = PATTERNS.get(pat);
        if (re == null) {
            if (PATTERNS.size() >= MAX_CACHED_PATTERNS) PATTERNS.clear();
            re = NO_MATCHER;
            if (!hasNestedQuantifier(pat) && !usesBacktrackingOnlySyntax(pat)) {
                try {
                    re = Pattern.compile(pat, Pattern.UNICODE_CHARACTER_CLASS);
                } catch (PatternSyntaxException ignored) {
                    re = NO_MATCHER;
                }
            }
            PATTERNS.put(pat, re);
        }
        return re != NO_MATCHER && !re.matcher(s).find();
    }

    private static boolean usesBacktrackingOnlySyntax(String p) {
        if (p.contains("(?=") || p.contains("(?!") || p.contains("(?<=") || p.contains("(?<!") || p.contains("(?>")
                || p.contains("\\k<")) return true;
        for (int i = 0; i + 1 < p.length(); i++) {
            char c = p.charAt(i);
            if (c == '\\') {
                char d = p.charAt(i + 1);
                if (d >= '1' && d <= '9') return true;
                i++;
                continue;
            }
            if ((c == '*' || c == '+' || c == '?' || c == '}') && p.charAt(i + 1) == '+') return true;
        }
        return false;
    }

    /**
     * Does the pattern repeat something that already repeats — the {@code (a+)+} shape behind
     * exponential backtracking? Mirrors {@code has_nested_quantifier} in {@code engine.rs}.
     */
    static boolean hasNestedQuantifier(String pattern) {
        int[] chars = pattern.codePoints().toArray();
        java.util.ArrayDeque<Integer> opens = new java.util.ArrayDeque<>();
        for (int i = 0; i < chars.length; i++) {
            int ch = chars[i];
            if (ch == '\\') {
                i++;
            } else if (ch == '[') {
                while (i < chars.length && chars[i] != ']') {
                    if (chars[i] == '\\') i++;
                    i++;
                }
            } else if (ch == '(') {
                opens.push(i);
            } else if (ch == ')') {
                if (opens.isEmpty()) continue;
                int start = opens.pop();
                if (i + 1 < chars.length && (chars[i + 1] == '*' || chars[i + 1] == '+' || chars[i + 1] == '{')) {
                    List<Integer> body = new ArrayList<>();
                    for (int j = start + 1; j < i; ) {
                        if (chars[j] == '\\') { j += 2; continue; }
                        body.add(chars[j]);
                        j++;
                    }
                    for (int k = 0; k < body.size(); k++) {
                        int b = body.get(k);
                        if (b == '*' || b == '+' || (b == '{' && k + 1 < body.size() && body.get(k + 1) >= '0' && body.get(k + 1) <= '9')) {
                            return true;
                        }
                    }
                }
            }
        }
        return false;
    }

    static SynxConstraints copyConstraints(SynxConstraints c) {
        SynxConstraints o = new SynxConstraints();
        o.min = c.min; o.max = c.max; o.typeName = c.typeName; o.required = c.required;
        o.readonly = c.readonly; o.pattern = c.pattern; o.enumValues = c.enumValues;
        return o;
    }

    /** required/readonly OR; the larger min, the smaller max; the first type/pattern/enum. */
    static void mergeConstraints(SynxConstraints base, SynxConstraints in) {
        if (in.required) base.required = true;
        if (in.readonly) base.readonly = true;
        if (in.min != null) base.min = base.min == null ? in.min : rustMax(base.min, in.min);
        if (in.max != null) base.max = base.max == null ? in.max : rustMin(base.max, in.max);
        if (base.typeName == null) base.typeName = in.typeName;
        if (base.pattern == null) base.pattern = in.pattern;
        if (base.enumValues == null) base.enumValues = in.enumValues;
    }

    /** f64::max / f64::min: a NaN operand loses. */
    private static double rustMax(double a, double b) {
        if (Double.isNaN(a)) return b;
        if (Double.isNaN(b)) return a;
        return a > b ? a : b;
    }

    private static double rustMin(double a, double b) {
        if (Double.isNaN(a)) return b;
        if (Double.isNaN(b)) return a;
        return a < b ? a : b;
    }

    private static boolean isFieldError(SynxValue v) {
        return v instanceof SynxValue.Str s && (s.value().startsWith("CONSTRAINT_ERR:") || s.value().startsWith("TYPE_ERR:"));
    }

    static boolean valueMatchesType(SynxValue v, String expected) {
        switch (expected) {
            case "int":    return v instanceof SynxValue.Int;
            case "float":  return v instanceof SynxValue.Float || v instanceof SynxValue.Int;
            case "bool":   return v instanceof SynxValue.Bool;
            case "string": return v instanceof SynxValue.Str || v instanceof SynxValue.Secret;
            case "array":  return v instanceof SynxValue.Arr;
            case "object": return v instanceof SynxValue.Obj;
            default:       return true;
        }
    }

    // ─── values ─────────────────────────────────────────────────────────────

    /** Rust {@code value_as_number}: Int and Float only. */
    static Double valueAsNumber(SynxValue v) {
        if (v instanceof SynxValue.Int i) return (double) i.value();
        if (v instanceof SynxValue.Float f) return f.value();
        return null;
    }

    /**
     * A value as text: an int as an integer, a float as Rust {@code Display} (a whole float as an
     * integer, never an exponent), true/false, "null", "" for a group or a list, and
     * {@code [SECRET]} for a secret, which never reaches a string sink as plaintext.
     */
    static String valueToString(SynxValue v) {
        if (v instanceof SynxValue.Str s) return s.value();
        if (v instanceof SynxValue.Int i) return Long.toString(i.value());
        if (v instanceof SynxValue.Float f) return SynxNumbers.formatNumber(f.value());
        if (v instanceof SynxValue.Bool b) return b.value() ? "true" : "false";
        if (v instanceof SynxValue.Null) return "null";
        if (v instanceof SynxValue.Secret) return "[SECRET]";
        return "";
    }

    /** {@code (string)} or {@code [type:string]} keeps env/default text as text. */
    static boolean wantsString(SynxMeta meta) {
        return "string".equals(meta.typeHint) || (meta.constraints != null && "string".equals(meta.constraints.typeName));
    }

    static SynxValue castEngineText(String text, boolean forceString) {
        return forceString ? SynxValue.ofString(text) : castPrimitive(text);
    }

    /** Rust {@code cast_primitive}: quotes, literals, an i64, an f64 (Rust grammar), else the text. */
    static SynxValue castPrimitive(String val) {
        if (val.length() >= 2) {
            char f = val.charAt(0), l = val.charAt(val.length() - 1);
            if ((f == '"' && l == '"') || (f == '\'' && l == '\'')) return SynxValue.ofString(val.substring(1, val.length() - 1));
        }
        switch (val) {
            case "true":  return SynxValue.ofBool(true);
            case "false": return SynxValue.ofBool(false);
            case "null":  return SynxValue.ofNull();
            default: break;
        }
        Long n = SynxNumbers.parseI64(val);
        if (n != null) return SynxValue.ofInt(n);
        // spec: `inf`, `NaN` and `1e400` are text. Read as floats they became `null` in JSON:
        // a host named `inf` lost its value.
        Double d = SynxNumbers.parseF64(val);
        if (d != null && Double.isFinite(d)) return SynxValue.ofFloat(d);
        return SynxValue.ofString(val);
    }

    /** The prefixes of the strings the engine leaves in place of a value it could not produce. */
    private static final String[] RUNTIME_ERROR_PREFIXES = {
        "INCLUDE_ERR:", "WATCH_ERR:", "CALC_ERR:", "SPAM_ERR:", "CONSTRAINT_ERR:",
        "ALIAS_ERR:", "NESTING_ERR:", "LIMIT_ERR:", "TYPE_ERR:", "WASM_ERR:",
    };

    /** Whether {@code s} is an error the engine left in place of a value. */
    static boolean isEngineError(String s) {
        if (s.isEmpty() || s.charAt(0) < 'A' || s.charAt(0) > 'Z') return false;
        for (String prefix : RUNTIME_ERROR_PREFIXES) {
            if (s.startsWith(prefix)) return true;
        }
        return false;
    }

    /**
     * Makes a resolved value a secret. A scalar becomes the secret of its text; a list or a
     * group keeps its shape and every scalar in it is masked. An absent value (null) stays
     * null — wrapping it made the secret the text "null", which {@code [required]} read as a
     * present value — an existing secret is kept, and an engine error stays readable:
     * {@code :secret:watch missing.txt} is a failure to report, not a value to hide.
     */
    static SynxValue maskSecretValue(SynxValue v) {
        if (v == null) return SynxValue.ofNull();
        if (v instanceof SynxValue.Secret || v instanceof SynxValue.Null) return v;
        if (v instanceof SynxValue.Arr a) {
            List<SynxValue> out = new ArrayList<>(a.values().size());
            for (SynxValue item : a.values()) out.add(maskSecretValue(item));
            return SynxValue.ofArray(out);
        }
        if (v instanceof SynxValue.Obj o) {
            SynxObject map = o.map();
            for (String k : map.keys()) map.set(k, maskSecretValue(map.get(k)));
            return v;
        }
        if (v instanceof SynxValue.Str s && isEngineError(s.value())) return v;
        return SynxValue.ofSecret(valueToString(v));
    }

    static SynxValue deepGet(SynxValue root, String path) {
        if (!(root instanceof SynxValue.Obj o)) return null;
        SynxValue direct = o.map().get(path);
        if (direct != null) return direct;
        SynxObject cur = o.map();
        int from = 0;
        while (true) {
            int dot = path.indexOf('.', from);
            String part = dot < 0 ? path.substring(from) : path.substring(from, dot);
            SynxValue v = cur.get(part);
            if (v == null) return null;
            if (dot < 0) return v;
            if (!(v instanceof SynxValue.Obj next)) return null;
            cur = next.map();
            from = dot + 1;
        }
    }

    /** Every code point alphanumeric (Unicode Alphabetic or Numeric), {@code _} or {@code .}; true for "". */
    static boolean isRefName(String s) {
        for (int i = 0; i < s.length(); ) {
            int cp = s.codePointAt(i);
            if (cp != '_' && cp != '.' && !isAlphanumeric(cp)) return false;
            i += Character.charCount(cp);
        }
        return true;
    }

    static boolean isAlphanumeric(int cp) {
        if (Character.isAlphabetic(cp)) return true;
        int t = Character.getType(cp);
        return t == Character.DECIMAL_DIGIT_NUMBER || t == Character.LETTER_NUMBER || t == Character.OTHER_NUMBER;
    }

    static boolean isWordChar(char c) {
        return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '_';
    }

    /** A key whole-word substitution reaches in ways a token lookup cannot. */
    static boolean isCalcSpecialKey(String k) {
        boolean allWord = true, allDigit = true;
        for (int i = 0; i < k.length(); i++) {
            char c = k.charAt(i);
            if (!isWordChar(c)) allWord = false;
            if (c < '0' || c > '9') allDigit = false;
        }
        return !allWord || allDigit || k.equals("NaN") || k.equals("inf");
    }

    /** Whole-word replacement of {@code word} by {@code repl}, capped like the reference. */
    static String replaceWord(String hay, String word, String repl) {
        if (word.length() > hay.length()) return hay;
        StringBuilder out = new StringBuilder(hay.length());
        long bytes = 0;
        int i = 0;
        int wl = word.length();
        while (i <= hay.length() - wl) {
            if (bytes >= MAX_ENGINE_SCRATCH) break;
            if (hay.startsWith(word, i)) {
                boolean beforeOk = i == 0 || !isWordChar(hay.charAt(i - 1));
                boolean afterOk = i + wl >= hay.length() || !isWordChar(hay.charAt(i + wl));
                if (beforeOk && afterOk) {
                    bytes = appendCapped(out, bytes, repl, MAX_ENGINE_SCRATCH);
                    i += wl;
                    continue;
                }
            }
            int w = Character.charCount(hay.codePointAt(i));
            if (bytes < MAX_ENGINE_SCRATCH) {
                out.append(hay, i, i + w);
                bytes += SynxParser.utf8Len(hay, i, i + w);
            }
            i += w;
        }
        if (i < hay.length()) appendCapped(out, bytes, hay.substring(i), MAX_ENGINE_SCRATCH);
        return out.toString();
    }

    /** Appends as much of {@code s} as fits in {@code cap} UTF-8 bytes; returns the new byte count. */
    private static long appendCapped(StringBuilder out, long bytes, String s, long cap) {
        long room = cap - bytes;
        if (room <= 0) return bytes;
        int end = SynxParser.utf8PrefixLen(s, 0, s.length(), room);
        out.append(s, 0, end);
        return bytes + SynxParser.utf8Len(s, 0, end);
    }

    static String delimiterFromKeyword(String k) {
        switch (k) {
            case "space": return " ";
            case "pipe":  return "|";
            case "dash":  return "-";
            case "dot":   return ".";
            case "semi":  return ";";
            case "tab":   return "\t";
            case "slash": return "/";
            default:      return k;
        }
    }

    /** {@code str::split} on a literal separator; an empty separator splits into code points. */
    static List<String> splitLiteral(String s, String sep) {
        List<String> out = new ArrayList<>();
        if (sep.isEmpty()) {
            out.add("");
            for (int i = 0; i < s.length(); ) {
                int w = Character.charCount(s.codePointAt(i));
                out.add(s.substring(i, i + w));
                i += w;
            }
            out.add("");
            return out;
        }
        int from = 0;
        while (true) {
            int at = s.indexOf(sep, from);
            if (at < 0) { out.add(s.substring(from)); return out; }
            out.add(s.substring(from, at));
            from = at + sep.length();
        }
    }

    /**
     * Stable sort: two numbers compare numerically, any other pair by text in code point order.
     * Up to 20 items it is the insertion sort Rust's stable sort uses for short slices, so even
     * a mixed list orders exactly as in the reference.
     */
    static void stableSort(List<SynxValue> items) {
        // A total order: numbers first, by value (NaN after the rest of them), then
        // everything else by its text, by code point. Comparing a number with text as text
        // made `10 < 1a < 9 < 10` a cycle — TimSort answers that with
        // IllegalArgumentException — and the result depended on the algorithm.
        java.util.Comparator<SynxValue> cmp = (a, b) -> {
            Double an = valueAsNumber(a), bn = valueAsNumber(b);
            if (an != null && bn != null) {
                if (an.isNaN() || bn.isNaN()) return Boolean.compare(an.isNaN(), bn.isNaN());
                return an < bn ? -1 : (an > bn ? 1 : 0);
            }
            if (an != null) return -1;
            if (bn != null) return 1;
            return SynxParser.compareCodePoints(valueToString(a), valueToString(b));
        };
        if (items.size() <= 20) {
            for (int i = 1; i < items.size(); i++) {
                for (int j = i; j > 0 && cmp.compare(items.get(j), items.get(j - 1)) < 0; j--) {
                    SynxValue t = items.get(j);
                    items.set(j, items.get(j - 1));
                    items.set(j - 1, t);
                }
            }
        } else {
            items.sort(cmp);
        }
    }

    /** f64::round: half away from zero (and -0.0 for a negative value that rounds to zero). */
    static double roundHalfAway(double x) {
        if (Double.isNaN(x) || Double.isInfinite(x)) return x;
        double a = Math.abs(x);
        if (a >= 4503599627370496.0) return x; // 2^52: already integral
        double t = Math.floor(a);
        return Math.copySign(a - t >= 0.5 ? t + 1 : t, x);
    }

    /** LLVM {@code __powidf2} — what Rust's {@code f64::powi} with a run-time exponent compiles to. */
    static double powi(double a, int b) {
        boolean recip = b < 0;
        double r = 1;
        while (true) {
            if ((b & 1) != 0) r *= a;
            b /= 2;
            if (b == 0) break;
            a *= a;
        }
        return recip ? 1 / r : r;
    }

    // ─── :format ────────────────────────────────────────────────────────────

    static String applyFormatPattern(String pattern, SynxValue v) {
        if (v instanceof SynxValue.Int i) {
            if (pattern.indexOf('d') >= 0 || pattern.indexOf('i') >= 0) return formatIntPattern(pattern, i.value());
            if (pattern.indexOf('f') >= 0 || pattern.indexOf('e') >= 0) return formatFloatPattern(pattern, (double) i.value());
            return Long.toString(i.value());
        }
        if (v instanceof SynxValue.Float f) {
            double d = f.value();
            if (pattern.indexOf('f') >= 0 || pattern.indexOf('e') >= 0) return formatFloatPattern(pattern, d);
            if ((pattern.indexOf('d') >= 0 || pattern.indexOf('i') >= 0) && SynxNumbers.isWhole(d) && Math.abs(d) < 9.0e15) {
                // A whole float takes an integer pattern: `12.0` under `%05d` is `00012`.
                return formatIntPattern(pattern, SynxNumbers.toI64(d));
            }
            return SynxNumbers.formatNumber(d);
        }
        if (v instanceof SynxValue.Str s) return s.value();
        return valueToString(v);
    }

    static String formatIntPattern(String pattern, long n) {
        final int maxWidth = 4096;
        if (pattern.startsWith("%")) {
            String s = pattern.substring(1);
            String inner = null;
            if (s.endsWith("d") || s.endsWith("i")) inner = s.substring(0, s.length() - 1);
            if (inner != null) {
                String text = Long.toString(n);
                if (inner.startsWith("0")) {
                    Long w = SynxNumbers.parseU64(inner.substring(1));
                    if (w != null) return padLeft(text, '0', clampWidth(w, maxWidth));
                }
                Long w = SynxNumbers.parseU64(inner);
                if (w != null) return padLeft(text, ' ', clampWidth(w, maxWidth));
            }
        }
        return Long.toString(n);
    }

    private static int clampWidth(long w, int max) {
        return Long.compareUnsigned(w, max) > 0 ? max : (int) w;
    }

    /** Right-aligns in {@code width} characters; the fill goes before a minus sign too ({@code {:0>w$}}). */
    private static String padLeft(String text, char fill, int width) {
        int n = text.codePointCount(0, text.length());
        if (n >= width) return text;
        return String.valueOf(fill).repeat(width - n) + text;
    }

    static String formatFloatPattern(String pattern, double f) {
        final int maxPrec = 1024;
        if (pattern.startsWith("%")) {
            String s = pattern.substring(1);
            if (s.equals("e")) return f == 0 ? "0e+0" : SynxNumbers.jsExponential(f);
            String inner = null;
            if (s.endsWith("f") || s.endsWith("e")) inner = s.substring(0, s.length() - 1);
            if (inner != null && inner.startsWith(".")) {
                Long p = SynxNumbers.parseU64(inner.substring(1));
                if (p != null) {
                    int prec = clampWidth(p, maxPrec);
                    return s.endsWith("e") ? SynxNumbers.lowerExp(f, prec) : SynxNumbers.fixed(f, prec);
                }
            }
        }
        return SynxNumbers.display(f);
    }

    // ─── :version ───────────────────────────────────────────────────────────

    /** The integer at the start of {@code s} (after whitespace, optional sign), or 0. */
    static long leadingInt(String s) {
        String t = SynxParser.trimStart(s);
        boolean neg = false;
        int i = 0;
        if (!t.isEmpty() && (t.charAt(0) == '-' || t.charAt(0) == '+')) {
            neg = t.charAt(0) == '-';
            i = 1;
        }
        long n = 0;
        for (; i < t.length() && SynxNumbers.isDigit(t.charAt(i)); i++) {
            int d = t.charAt(i) - '0';
            n = n > (Long.MAX_VALUE - d) / 10 ? Long.MAX_VALUE : n * 10 + d;
        }
        return neg ? -n : n;
    }

    static boolean compareVersions(String current, String op, String required) {
        String[] cv = current.split("\\.", -1);
        String[] rv = required.split("\\.", -1);
        int len = Math.max(cv.length, rv.length);
        int ord = 0;
        for (int i = 0; i < len; i++) {
            long a = i < cv.length ? leadingInt(cv[i]) : 0;
            long b = i < rv.length ? leadingInt(rv[i]) : 0;
            if (a != b) { ord = a < b ? -1 : 1; break; }
        }
        switch (op) {
            case ">=": return ord >= 0;
            case "<=": return ord <= 0;
            case ">":  return ord > 0;
            case "<":  return ord < 0;
            case "==": case "=": return ord == 0;
            case "!=": return ord != 0;
            default:   return false;
        }
    }

    // ─── :i18n plural rules ─────────────────────────────────────────────────

    static String pluralCategory(String lang, long n) {
        long abs = n == Long.MIN_VALUE ? Long.MIN_VALUE : Math.abs(n); // unsigned 2^63 for MIN_VALUE
        long n10 = Long.remainderUnsigned(abs, 10), n100 = Long.remainderUnsigned(abs, 100);
        switch (lang) {
            case "ru": case "uk": case "be": case "pl":
                if (n10 == 1 && n100 != 11) return "one";
                if (n10 >= 2 && n10 <= 4 && !(n100 >= 12 && n100 <= 14)) return "few";
                return "many";
            case "cs": case "sk":
                if (abs == 1) return "one";
                if (abs >= 2 && abs <= 4) return "few";
                return "other";
            case "ar":
                if (abs == 0) return "zero";
                if (abs == 1) return "one";
                if (abs == 2) return "two";
                if (n100 >= 3 && n100 <= 10) return "few";
                if (n100 >= 11 && n100 <= 99) return "many";
                return "other";
            case "fr": case "pt":
                return Long.compareUnsigned(abs, 1) <= 0 ? "one" : "other";
            case "ja": case "zh": case "ko": case "vi": case "th":
                return "other";
            default:
                return abs == 1 ? "one" : "other";
        }
    }

    // ─── :prompt ────────────────────────────────────────────────────────────

    static String stringifyPrompt(SynxValue v, int indent) {
        String spaces = " ".repeat(indent);
        StringBuilder sb = new StringBuilder();
        if (v instanceof SynxValue.Obj o) {
            List<String> keys = new ArrayList<>(o.map().keys());
            keys.sort(SynxParser::compareCodePoints);
            for (String key : keys) {
                SynxValue val = o.map().get(key);
                if (val instanceof SynxValue.Obj) {
                    sb.append(spaces).append(key).append('\n').append(stringifyPrompt(val, indent + 2));
                } else if (val instanceof SynxValue.Arr a) {
                    sb.append(spaces).append(key).append('\n');
                    for (SynxValue item : a.values()) sb.append(spaces).append("  - ").append(valueToString(item)).append('\n');
                } else {
                    sb.append(spaces).append(key).append(' ').append(valueToString(val)).append('\n');
                }
            }
        } else if (v instanceof SynxValue.Arr a) {
            for (SynxValue item : a.values()) sb.append(spaces).append("  - ").append(valueToString(item)).append('\n');
        } else {
            sb.append(spaces).append(valueToString(v)).append('\n');
        }
        return sb.toString();
    }

    // ─── :spam ──────────────────────────────────────────────────────────────

    private static final Map<String, List<Long>> SPAM_BUCKETS = new HashMap<>();

    static boolean allowSpamAccess(String bucket, long maxCalls, long windowSec) {
        long now = System.nanoTime();
        long win = Long.compareUnsigned(windowSec, 1) < 0 ? 1 : windowSec;
        long windowNanos = Long.compareUnsigned(win, Long.MAX_VALUE / 1_000_000_000L) > 0 ? Long.MAX_VALUE : win * 1_000_000_000L;
        synchronized (SPAM_BUCKETS) {
            List<Long> calls = SPAM_BUCKETS.computeIfAbsent(bucket, k -> new ArrayList<>());
            calls.removeIf(ts -> now - ts > windowNanos);
            if (Long.compareUnsigned(calls.size(), maxCalls) >= 0) return false;
            calls.add(now);
            return true;
        }
    }

    // ─── :once lock file ────────────────────────────────────────────────────

    static String readLockValue(Path lockPath, String key) {
        String content;
        try { content = readUtf8(lockPath); } catch (IOException e) { return null; }
        for (String line : rustLines(content)) {
            if (line.startsWith(key) && line.startsWith(" ", key.length())) {
                return SynxParser.trimStart(line.substring(key.length()));
            }
        }
        return null;
    }

    static void writeLockValue(Path lockPath, String key, String value) {
        List<String> lines;
        try { lines = new ArrayList<>(rustLines(readUtf8(lockPath))); } catch (IOException e) { lines = new ArrayList<>(); }
        String newLine = key + " " + value;
        boolean found = false;
        for (int i = 0; i < lines.size(); i++) {
            String line = lines.get(i);
            if (line.startsWith(key) && line.startsWith(" ", key.length())) {
                lines.set(i, newLine);
                found = true;
                break;
            }
        }
        if (!found) lines.add(newLine);
        try {
            Files.writeString(lockPath, String.join("\n", lines) + "\n", StandardCharsets.UTF_8);
        } catch (IOException ignored) {
            // The reference ignores a failed write too.
        }
    }

    /** Rust {@code str::lines}: split on LF, drop one CR per line, no empty last line. */
    static List<String> rustLines(String s) {
        List<String> out = new ArrayList<>();
        if (s.isEmpty()) return out;
        if (s.endsWith("\n")) s = s.substring(0, s.length() - 1);
        for (String p : s.split("\n", -1)) out.add(p.endsWith("\r") ? p.substring(0, p.length() - 1) : p);
        return out;
    }

    // ─── files ──────────────────────────────────────────────────────────────

    private record Jail(Path path, String error) {}

    private record PreInclude(String path, SynxValue root) {}

    /** Keeps {@code file} inside {@code base}: the path to read, or the reference's SECURITY error. */
    private static Jail jailPath(String base, String file) {
        if (!file.isEmpty() && (file.charAt(0) == '/' || file.charAt(0) == '\\')) {
            return new Jail(null, "SECURITY: rooted paths are not allowed: '" + file + "'");
        }
        Path fp;
        try {
            fp = Path.of(file);
        } catch (InvalidPathException e) {
            // Not a path this platform can name: it passes the jail and fails on read.
            return new Jail(null, null);
        }
        if (fp.isAbsolute()) return new Jail(null, "SECURITY: absolute paths are not allowed: '" + file + "'");
        // safety: `NUL`, `con.txt` or `COM1` in any directory opens a device on Windows —
        // `:include NUL` read it as an empty document, and a console or a serial port blocks
        // the read. synx-core reaches the disk through a verbatim path, where these are
        // ordinary missing files; the same answer is given here.
        if (IS_WINDOWS && hasDeviceName(fp)) return new Jail(null, "file not found: " + file);
        Path baseCanon;
        try {
            baseCanon = Path.of(base.isEmpty() ? "." : base).toRealPath();
        } catch (IOException | InvalidPathException e) {
            // A base that cannot be resolved refuses instead of jailing nothing.
            return new Jail(null, "SECURITY: base directory not found: '" + (base.isEmpty() ? "." : base) + "'");
        }
        Path full = baseCanon.resolve(fp);
        Path fullCanon;
        try {
            fullCanon = full.toRealPath();
        } catch (IOException e) {
            // Judged by the path as the document wrote it, segment by segment: a base
            // directory named `a..b` is no traversal.
            for (String seg : file.split("[/\\\\]")) {
                if (seg.equals("..")) return new Jail(null, "SECURITY: path traversal detected: '" + file + "'");
            }
            if (!full.startsWith(baseCanon)) return new Jail(null, "SECURITY: path escapes base directory: '" + file + "'");
            return new Jail(full, null);
        }
        if (!fullCanon.startsWith(baseCanon)) return new Jail(null, "SECURITY: path escapes base directory: '" + file + "'");
        return new Jail(fullCanon, null);
    }

    private static final boolean IS_WINDOWS =
        System.getProperty("os.name", "").toLowerCase(java.util.Locale.ROOT).startsWith("windows");

    /** Whether a component of {@code p} is a DOS device name, with or without an extension. */
    static boolean hasDeviceName(Path p) {
        for (Path part : p) {
            String name = part.toString();
            int dot = name.indexOf('.');
            String stem = (dot < 0 ? name : name.substring(0, dot)).stripTrailing().toUpperCase(java.util.Locale.ROOT);
            switch (stem) {
                case "CON": case "PRN": case "AUX": case "NUL": case "CONIN$": case "CONOUT$":
                    return true;
                default:
                    break;
            }
            if (stem.length() == 4 && (stem.startsWith("COM") || stem.startsWith("LPT"))) {
                char d = stem.charAt(3);
                if ((d >= '0' && d <= '9') || d == '\u00b9' || d == '\u00b2' || d == '\u00b3') return true;
            }
        }
        return false;
    }

    /** {@code Path::join}: an absolute second part replaces the first. */
    private static Path joinPath(String a, String b) {
        try {
            Path pb = Path.of(b);
            if (pb.isAbsolute() || a.isEmpty()) return pb;
            return Path.of(a).resolve(pb);
        } catch (InvalidPathException e) {
            return null;
        }
    }

    private static boolean exists(Path p) {
        try { return p != null && Files.exists(p); } catch (RuntimeException e) { return false; }
    }

    private static String checkFileSize(Path p) {
        try {
            long size = Files.size(p);
            if (size > MAX_FILE_SIZE) return "SECURITY: file too large (" + size + " bytes, max " + MAX_FILE_SIZE + ")";
        } catch (IOException | RuntimeException ignored) {
            // A file whose size cannot be read fails later, on read.
        }
        return null;
    }

    /** {@code fs::read_to_string}: the file must be valid UTF-8. */
    static String readUtf8(Path p) throws IOException {
        if (p == null) throw new NoSuchFileException("");
        try {
            return Files.readString(p, StandardCharsets.UTF_8);
        } catch (CharacterCodingException e) {
            throw new IOException("stream did not contain valid UTF-8", e);
        } catch (InvalidPathException e) {
            throw new NoSuchFileException("");
        }
    }

    /** {@code Path::extension}: none for a dot-file without another dot. */
    private static String fileExtension(Path p) {
        Path name = p.getFileName();
        if (name == null) return "";
        String s = name.toString();
        int dot = s.lastIndexOf('.');
        return dot <= 0 ? "" : s.substring(dot + 1);
    }

    private static Path readManifestMain(Path pkgDir) {
        String text;
        try { text = readUtf8(pkgDir.resolve("synx-pkg.synx")); } catch (IOException | RuntimeException e) { return null; }
        for (String line : rustLines(text)) {
            String t = SynxParser.trim(line);
            for (String field : new String[] { "main ", "entry " }) {
                if (t.startsWith(field)) {
                    String p = SynxParser.trim(t.substring(field.length()));
                    if (isContainedRelative(p)) {
                        try { return pkgDir.resolve(p); } catch (InvalidPathException e) { return null; }
                    }
                }
            }
        }
        return null;
    }

    /**
     * A relative path with no root, drive, {@code :} or {@code ..} segment, judged by its text so
     * a package directory that is itself a symlink keeps working. A {@code !use} name or manifest
     * {@code main} that fails it would otherwise read files outside the packages directory.
     */
    static boolean isContainedRelative(String p) {
        if (p.isEmpty() || p.charAt(0) == '/' || p.charAt(0) == '\\' || p.indexOf(':') >= 0) return false;
        try {
            if (Path.of(p).isAbsolute()) return false;
        } catch (InvalidPathException ignored) {
            // Not a path this platform can name; it fails when read.
        }
        for (String seg : p.split("[/\\\\]", -1)) {
            if (seg.equals("..")) return false;
        }
        return true;
    }

    // ─── :watch key paths ───────────────────────────────────────────────────

    static SynxValue extractFromFileContent(String content, String keyPath, String ext) {
        if (ext.equals("json")) {
            SynxValue cur;
            try {
                cur = new JsonReader(content).document();
            } catch (IllegalArgumentException e) {
                return null;
            }
            for (String part : keyPath.split("\\.", -1)) {
                if (!(cur instanceof SynxValue.Obj o)) return null;
                cur = o.map().get(part);
                if (cur == null) return null;
            }
            return cur;
        }
        SynxValue cur = SynxParser.parse(content).root;
        for (String part : keyPath.split("\\.", -1)) {
            if (!(cur instanceof SynxValue.Obj o)) return null;
            cur = o.map().get(part);
            if (cur == null) return null;
        }
        return cur;
    }

    /** A strict RFC 8259 reader with serde_json's value mapping (objects in key order, last duplicate wins). */
    private static final class JsonReader {
        final String s;
        int i;

        JsonReader(String s) { this.s = s; }

        SynxValue document() {
            SynxValue v = value(0);
            ws();
            if (i != s.length()) throw new IllegalArgumentException("trailing characters");
            return v;
        }

        void ws() {
            while (i < s.length()) {
                char c = s.charAt(i);
                if (c == ' ' || c == '\t' || c == '\n' || c == '\r') i++;
                else break;
            }
        }

        SynxValue value(int depth) {
            if (depth > 128) throw new IllegalArgumentException("recursion limit exceeded");
            ws();
            if (i >= s.length()) throw new IllegalArgumentException("EOF");
            char c = s.charAt(i);
            switch (c) {
                case '{': {
                    i++;
                    java.util.TreeMap<String, SynxValue> fields = new java.util.TreeMap<>(SynxParser::compareCodePoints);
                    ws();
                    if (i < s.length() && s.charAt(i) == '}') { i++; return toObject(fields); }
                    while (true) {
                        ws();
                        if (i >= s.length() || s.charAt(i) != '"') throw new IllegalArgumentException("key");
                        String k = string();
                        ws();
                        if (i >= s.length() || s.charAt(i) != ':') throw new IllegalArgumentException(":");
                        i++;
                        fields.put(k, value(depth + 1));
                        ws();
                        if (i < s.length() && s.charAt(i) == ',') { i++; continue; }
                        if (i < s.length() && s.charAt(i) == '}') { i++; return toObject(fields); }
                        throw new IllegalArgumentException("object");
                    }
                }
                case '[': {
                    i++;
                    List<SynxValue> items = new ArrayList<>();
                    ws();
                    if (i < s.length() && s.charAt(i) == ']') { i++; return SynxValue.ofArray(items); }
                    while (true) {
                        items.add(value(depth + 1));
                        ws();
                        if (i < s.length() && s.charAt(i) == ',') { i++; continue; }
                        if (i < s.length() && s.charAt(i) == ']') { i++; return SynxValue.ofArray(items); }
                        throw new IllegalArgumentException("array");
                    }
                }
                case '"':
                    return SynxValue.ofString(string());
                case 't': return literal("true", SynxValue.ofBool(true));
                case 'f': return literal("false", SynxValue.ofBool(false));
                case 'n': return literal("null", SynxValue.ofNull());
                default:
                    return number();
            }
        }

        SynxValue toObject(java.util.TreeMap<String, SynxValue> fields) {
            SynxObject o = new SynxObject();
            for (var e : fields.entrySet()) o.set(e.getKey(), e.getValue());
            return SynxValue.ofObject(o);
        }

        SynxValue literal(String word, SynxValue v) {
            if (!s.startsWith(word, i)) throw new IllegalArgumentException(word);
            i += word.length();
            return v;
        }

        String string() {
            i++; // opening quote
            StringBuilder sb = new StringBuilder();
            while (true) {
                if (i >= s.length()) throw new IllegalArgumentException("EOF in string");
                char c = s.charAt(i++);
                if (c == '"') return sb.toString();
                if (c < 0x20) throw new IllegalArgumentException("control character in string");
                if (c != '\\') { sb.append(c); continue; }
                if (i >= s.length()) throw new IllegalArgumentException("EOF in escape");
                char e = s.charAt(i++);
                switch (e) {
                    case '"': sb.append('"'); break;
                    case '\\': sb.append('\\'); break;
                    case '/': sb.append('/'); break;
                    case 'b': sb.append('\b'); break;
                    case 'f': sb.append('\f'); break;
                    case 'n': sb.append('\n'); break;
                    case 'r': sb.append('\r'); break;
                    case 't': sb.append('\t'); break;
                    case 'u': {
                        char hi = hex4();
                        if (Character.isHighSurrogate(hi)) {
                            if (i + 1 < s.length() && s.charAt(i) == '\\' && s.charAt(i + 1) == 'u') {
                                i += 2;
                                char lo = hex4();
                                if (!Character.isLowSurrogate(lo)) throw new IllegalArgumentException("lone surrogate");
                                sb.append(hi).append(lo);
                            } else {
                                throw new IllegalArgumentException("lone surrogate");
                            }
                        } else if (Character.isLowSurrogate(hi)) {
                            throw new IllegalArgumentException("lone surrogate");
                        } else {
                            sb.append(hi);
                        }
                        break;
                    }
                    default: throw new IllegalArgumentException("escape");
                }
            }
        }

        char hex4() {
            if (i + 4 > s.length()) throw new IllegalArgumentException("hex");
            int v = 0;
            for (int k = 0; k < 4; k++) {
                int d = Character.digit(s.charAt(i + k), 16);
                if (d < 0 || s.charAt(i + k) > 'f') throw new IllegalArgumentException("hex");
                v = v * 16 + d;
            }
            i += 4;
            return (char) v;
        }

        SynxValue number() {
            int start = i;
            if (i < s.length() && s.charAt(i) == '-') i++;
            if (i >= s.length()) throw new IllegalArgumentException("number");
            if (s.charAt(i) == '0') {
                i++;
            } else if (s.charAt(i) >= '1' && s.charAt(i) <= '9') {
                while (i < s.length() && SynxNumbers.isDigit(s.charAt(i))) i++;
            } else {
                throw new IllegalArgumentException("number");
            }
            boolean isFloat = false;
            if (i < s.length() && s.charAt(i) == '.') {
                isFloat = true;
                i++;
                int d = i;
                while (i < s.length() && SynxNumbers.isDigit(s.charAt(i))) i++;
                if (i == d) throw new IllegalArgumentException("number");
            }
            if (i < s.length() && (s.charAt(i) == 'e' || s.charAt(i) == 'E')) {
                isFloat = true;
                i++;
                if (i < s.length() && (s.charAt(i) == '+' || s.charAt(i) == '-')) i++;
                int d = i;
                while (i < s.length() && SynxNumbers.isDigit(s.charAt(i))) i++;
                if (i == d) throw new IllegalArgumentException("number");
            }
            String text = s.substring(start, i);
            if (!isFloat) {
                Long n = SynxNumbers.parseI64(text);
                if (n != null) return SynxValue.ofInt(n);
            }
            return SynxValue.ofFloat(Double.parseDouble(text));
        }
    }

    // ─── option and metadata copies ─────────────────────────────────────────

    static SynxOptions copyOptions(SynxOptions o) {
        SynxOptions c = new SynxOptions();
        c.env = o.env;
        c.region = o.region;
        c.lang = o.lang;
        c.basePath = o.basePath;
        c.maxIncludeDepth = o.maxIncludeDepth;
        c.packagesPath = o.packagesPath;
        c.strict = o.strict;
        c.markerFns = o.markerFns;
        c.includeDepth = o.includeDepth;
        return c;
    }

    static SynxMeta copyMeta(SynxMeta m) {
        SynxMeta c = new SynxMeta();
        c.markers = new ArrayList<>(m.markers);
        c.args = new ArrayList<>(m.args);
        c.typeHint = m.typeHint;
        c.constraints = m.constraints;
        return c;
    }

    static Map<String, SynxMeta> copyMetaMap(Map<String, SynxMeta> mm) {
        Map<String, SynxMeta> c = new LinkedHashMap<>();
        for (var e : mm.entrySet()) c.put(e.getKey(), copyMeta(e.getValue()));
        return c;
    }
}
