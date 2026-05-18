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
