package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.Iterator;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Objects;

/**
 * Insertion-ordered key/value container used everywhere a JSON-style "object"
 * is needed. Equality is order-insensitive (matches Rust {@code HashMap == HashMap}
 * semantics); iteration follows insertion order so stringify and canonical
 * reformat stay author-controlled.
 */
public final class SynxObject implements Iterable<Map.Entry<String, SynxValue>> {

    private final LinkedHashMap<String, SynxValue> entries = new LinkedHashMap<>();

    public SynxObject() {}

    public int size()       { return entries.size(); }
    public boolean isEmpty() { return entries.isEmpty(); }

    public boolean contains(String key) { return entries.containsKey(key); }

    /** @return value or {@code null} when absent */
    public SynxValue get(String key) { return entries.get(key); }

    public void set(String key, SynxValue value) {
        entries.put(key, value);
    }

    /** @return {@code true} when the key was present */
    public boolean remove(String key) {
        return entries.remove(key) != null;
    }

    /** Insertion-order keys (live view, do not mutate the returned list). */
    public List<String> keys() { return new ArrayList<>(entries.keySet()); }

    public List<Map.Entry<String, SynxValue>> entriesList() {
        return new ArrayList<>(entries.entrySet());
    }

    @Override
    public Iterator<Map.Entry<String, SynxValue>> iterator() {
        return entries.entrySet().iterator();
    }

    private static final java.util.Comparator<Map.Entry<String, SynxValue>> BY_KEY_UTF16 =
        (a, b) -> a.getKey().compareTo(b.getKey());

    /**
     * The entries in key order (code point order) — what every canonical writer iterates.
     * Built straight into an array instead of copying the entry set.
     */
    @SuppressWarnings("unchecked")
    List<Map.Entry<String, SynxValue>> sortedEntries() {
        Map.Entry<String, SynxValue>[] arr = new Map.Entry[entries.size()];
        int i = 0;
        boolean surrogates = false;
        for (Map.Entry<String, SynxValue> e : entries.entrySet()) {
            String k = e.getKey();
            if (!surrogates) {
                for (int j = 0; j < k.length(); j++) {
                    if (Character.isSurrogate(k.charAt(j))) { surrogates = true; break; }
                }
            }
            arr[i++] = e;
        }
        // Without surrogates UTF-16 order is code point order, and String.compareTo is fast.
        if (arr.length > 1) java.util.Arrays.sort(arr, surrogates ? SynxParser.BY_KEY : BY_KEY_UTF16);
        return java.util.Arrays.asList(arr);
    }

    /** A deep copy: nested objects and arrays are copied too. */
    public SynxObject copy() {
        SynxObject out = new SynxObject();
        for (var e : entries.entrySet()) out.entries.put(e.getKey(), deepCopy(e.getValue()));
        return out;
    }

    /** Deep-copies objects and arrays; scalars are immutable and returned as they are. */
    public static SynxValue deepCopy(SynxValue v) {
        if (v instanceof SynxValue.Obj o) return SynxValue.ofObject(o.map().copy());
        if (v instanceof SynxValue.Arr a) {
            List<SynxValue> out = new ArrayList<>(a.values().size());
            for (SynxValue item : a.values()) out.add(deepCopy(item));
            return SynxValue.ofArray(out);
        }
        return v;
    }

    @Override
    public boolean equals(Object other) {
        if (this == other) return true;
        if (!(other instanceof SynxObject o)) return false;
        if (entries.size() != o.entries.size()) return false;
        for (var e : entries.entrySet()) {
            SynxValue rhs = o.entries.get(e.getKey());
            if (!Objects.equals(e.getValue(), rhs)) return false;
        }
        return true;
    }

    @Override
    public int hashCode() {
        // Order-insensitive hash so equal objects (different insertion order) hash equally.
        int h = 0;
        for (var e : entries.entrySet()) {
            h += Objects.hash(e.getKey(), e.getValue());
        }
        return h;
    }

    @Override
    public String toString() { return entries.toString(); }
}
