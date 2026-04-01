package com.aperturesyndicate.synx

/** Failure from the `synx-c` FFI (`NULL` from `synx.h`) or UTF-8 issues. */
public class SynxEngineError public constructor(
    message: String,
    public val kind: Kind,
) : Exception(message) {
    public enum class Kind {
        OperationFailed,
        InvalidUtf8,
    }

    public companion object {
        @JvmField
        public val OperationFailed: SynxEngineError =
            SynxEngineError("synx-c returned no result", Kind.OperationFailed)

        @JvmField
        public val InvalidUtf8: SynxEngineError =
            SynxEngineError("FFI output is not valid UTF-8", Kind.InvalidUtf8)
    }
}
