package com.aperturesyndicate.synx;

/** {@code !use @scope/name [as alias]} package directive. */
public record SynxUseDirective(String pkg, String alias) {}
