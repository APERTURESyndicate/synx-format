package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Objects;

/** Structural diff between two SYNX objects. Mirrors {@code crates/synx-core/src/diff.rs}. */
public final class SynxDiff {

    private SynxDiff() {}

    public record Change(SynxValue from, SynxValue to) {}

    public record ChangedEntry(String key, Change change) {}

    public static final class Result {
        public final SynxObject added;
        public final SynxObject removed;
        public final List<ChangedEntry> changed;
        public final List<String> unchanged;
        public Result(SynxObject added, SynxObject removed,
                       List<ChangedEntry> changed, List<String> unchanged) {
            this.added = added; this.removed = removed;
            this.changed = changed; this.unchanged = unchanged;
        }
    }

    public static Result diff(SynxObject a, SynxObject b) {
        SynxObject added = new SynxObject();
        SynxObject removed = new SynxObject();
        List<ChangedEntry> changed = new ArrayList<>();
        List<String> unchanged = new ArrayList<>();

        for (var e : a) {
            SynxValue bv = b.get(e.getKey());
            if (bv == null) {
                removed.set(e.getKey(), e.getValue());
            } else if (Objects.equals(e.getValue(), bv)) {
                unchanged.add(e.getKey());
            } else {
                changed.add(new ChangedEntry(e.getKey(), new Change(e.getValue(), bv)));
            }
        }
        for (var e : b) {
            if (a.get(e.getKey()) == null) {
                added.set(e.getKey(), e.getValue());
            }
        }
        Collections.sort(unchanged);
        return new Result(added, removed, changed, unchanged);
    }

    public static SynxValue toValue(Result d) {
        SynxObject root = new SynxObject();
        root.set("added",   SynxValue.ofObject(d.added));
        root.set("removed", SynxValue.ofObject(d.removed));

        SynxObject changedObj = new SynxObject();
        for (ChangedEntry e : d.changed) {
            SynxObject inner = new SynxObject();
            inner.set("from", e.change().from());
            inner.set("to",   e.change().to());
            changedObj.set(e.key(), SynxValue.ofObject(inner));
        }
        root.set("changed", SynxValue.ofObject(changedObj));

        List<SynxValue> arr = new ArrayList<>();
        for (String s : d.unchanged) arr.add(SynxValue.ofString(s));
        root.set("unchanged", SynxValue.ofArray(arr));
        return SynxValue.ofObject(root);
    }
}
