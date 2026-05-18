package com.aperturesyndicate.synx;

import java.util.List;
import java.util.Objects;

/**
 * Constraints from {@code [min:3, max:30, required, type:int, pattern:^\d+$, enum:a|b, readonly]}.
 * Mutable for parser convenience; consumers read fields directly.
 */
public final class SynxConstraints {
    public Double min;
    public Double max;
    public String typeName;
    public boolean required;
    public boolean readonly;
    public String pattern;
    public List<String> enumValues;

    public SynxConstraints() {}

    public boolean hasAny() {
        return min != null || max != null || typeName != null
            || required || readonly || pattern != null || enumValues != null;
    }

    @Override
    public boolean equals(Object o) {
        if (this == o) return true;
        if (!(o instanceof SynxConstraints c)) return false;
        return required == c.required
            && readonly == c.readonly
            && Objects.equals(min, c.min)
            && Objects.equals(max, c.max)
            && Objects.equals(typeName, c.typeName)
            && Objects.equals(pattern, c.pattern)
            && Objects.equals(enumValues, c.enumValues);
    }

    @Override
    public int hashCode() {
        return Objects.hash(min, max, typeName, required, readonly, pattern, enumValues);
    }
}
