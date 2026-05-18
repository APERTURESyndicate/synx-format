package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.List;
import java.util.Objects;

/** Metadata attached to one key (markers, args, optional type hint, optional constraints). */
public final class SynxMeta {
    public List<String> markers = new ArrayList<>();
    /** One arg per marker in the chain — same length as {@link #markers}. */
    public List<String> args = new ArrayList<>();
    public String typeHint;          // nullable
    public SynxConstraints constraints; // nullable

    public SynxMeta() {}

    public boolean hasMarker(String name) {
        for (var m : markers) if (m.equals(name)) return true;
        return false;
    }

    /** Index of marker {@code name} in the chain, or {@code -1}. */
    public int markerIndex(String name) {
        for (int i = 0; i < markers.size(); i++) {
            if (markers.get(i).equals(name)) return i;
        }
        return -1;
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (!(o instanceof SynxMeta m)) return false;
        return Objects.equals(markers, m.markers)
            && Objects.equals(args, m.args)
            && Objects.equals(typeHint, m.typeHint)
            && Objects.equals(constraints, m.constraints);
    }

    @Override
    public int hashCode() {
        return Objects.hash(markers, args, typeHint, constraints);
    }
}
