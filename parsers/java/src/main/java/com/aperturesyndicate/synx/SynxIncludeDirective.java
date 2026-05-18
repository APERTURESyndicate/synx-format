package com.aperturesyndicate.synx;

/** {@code !include <path> [alias]} directive parsed from a file. */
public record SynxIncludeDirective(String path, String alias) {}
