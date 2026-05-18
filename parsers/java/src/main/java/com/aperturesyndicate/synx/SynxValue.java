package com.aperturesyndicate.synx;

import java.util.ArrayList;
import java.util.List;

/**
 * SYNX value variants — a sealed hierarchy of records (JDK 17+).
 * Parity with the Rust {@code Value} enum in {@code crates/synx-core/src/value.rs}.
 *
 * <p>Use the factory helpers ({@code ofNull}, {@code ofBool}, …) instead of
 * instantiating records directly so call sites read as well as Rust / Swift / C++ ports.
 */
public sealed interface SynxValue {

    /** Diagnostic type name matching Rust {@code Value::type_name}. */
    String typeName();

    /** Tagged singleton — only one instance is ever created. */
    record Null() implements SynxValue {
        public static final Null INSTANCE = new Null();
        @Override public String typeName() { return "null"; }
    }

    record Bool(boolean value) implements SynxValue {
        @Override public String typeName() { return "bool"; }
    }

    record Int(long value) implements SynxValue {
        @Override public String typeName() { return "int"; }
    }

    record Float(double value) implements SynxValue {
        @Override public String typeName() { return "float"; }
    }

    record Str(String value) implements SynxValue {
        @Override public String typeName() { return "string"; }
    }

    record Arr(List<SynxValue> values) implements SynxValue {
        @Override public String typeName() { return "array"; }
    }

    record Obj(SynxObject map) implements SynxValue {
        @Override public String typeName() { return "object"; }
    }

    /** Redacted in JSON / stringify output as {@code [SECRET]}. */
    record Secret(String value) implements SynxValue {
        @Override public String typeName() { return "secret"; }
    }

    // ─── factories ───────────────────────────────────────────────────────────

    static SynxValue ofNull()              { return Null.INSTANCE; }
    static SynxValue ofBool(boolean b)     { return new Bool(b); }
    static SynxValue ofInt(long n)         { return new Int(n); }
    static SynxValue ofFloat(double f)     { return new Float(f); }
    static SynxValue ofString(String s)    { return new Str(s); }
    static SynxValue ofSecret(String s)    { return new Secret(s); }
    static SynxValue ofArray()             { return new Arr(new ArrayList<>()); }
    static SynxValue ofArray(List<SynxValue> v) { return new Arr(v); }
    static SynxValue ofObject()            { return new Obj(new SynxObject()); }
    static SynxValue ofObject(SynxObject o) { return new Obj(o); }

    // ─── typed accessors ─────────────────────────────────────────────────────

    default boolean isNull()   { return this instanceof Null; }
    default boolean isBool()   { return this instanceof Bool; }
    default boolean isInt()    { return this instanceof Int; }
    default boolean isFloat()  { return this instanceof Float; }
    default boolean isString() { return this instanceof Str; }
    default boolean isArray()  { return this instanceof Arr; }
    default boolean isObject() { return this instanceof Obj; }
    default boolean isSecret() { return this instanceof Secret; }

    /** Returns boxed boolean, or {@code null} when this is not a Bool. */
    default Boolean asBool() {
        return this instanceof Bool b ? b.value() : null;
    }
    default Long asInt() {
        return this instanceof Int i ? i.value() : null;
    }
    default Double asFloat() {
        return this instanceof Float f ? f.value() : null;
    }
    default String asString() {
        return this instanceof Str s ? s.value() : null;
    }
    default String asSecret() {
        return this instanceof Secret s ? s.value() : null;
    }
    default List<SynxValue> asArray() {
        return this instanceof Arr a ? a.values() : null;
    }
    default SynxObject asObject() {
        return this instanceof Obj o ? o.map() : null;
    }

    /** Numeric coercion: int/float → double; bool → 0/1; otherwise null. */
    default Double asDouble() {
        if (this instanceof Int i)    return (double) i.value();
        if (this instanceof Float f)  return f.value();
        if (this instanceof Bool b)   return b.value() ? 1.0 : 0.0;
        if (this instanceof Str s) {
            try { return Double.parseDouble(s.value()); }
            catch (NumberFormatException ignored) { return null; }
        }
        return null;
    }
}
