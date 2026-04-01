package com.aperturesyndicate.synx

import com.aperturesyndicate.synx.internal.LibSynx
import com.sun.jna.NativeLong
import com.sun.jna.Pointer
import com.sun.jna.ptr.LongByReference
import java.nio.charset.StandardCharsets

/**
 * Kotlin/JVM wrapper over **`synx-c`** — same engine as Rust **`synx-core` 3.6.x**
 * (`!active`, `!tool`, `.synxb`, canonical JSON). Uses **JNA** to load the native library.
 */
public object SynxEngine {
    private val lib get() = LibSynx.INSTANCE

    private fun takeString(ptr: Pointer?): String {
        if (ptr == null) throw SynxEngineError.OperationFailed
        try {
            val s = ptr.getString(0, StandardCharsets.UTF_8.name())
            return s ?: throw SynxEngineError.InvalidUtf8
        } finally {
            lib.synx_free(ptr)
        }
    }

    /** Static parse → canonical JSON string. */
    @JvmStatic
    public fun parse(text: String): String = takeString(lib.synx_parse(text))

    /** Parse with `!active` resolution → JSON string. */
    @JvmStatic
    public fun parseActive(text: String): String = takeString(lib.synx_parse_active(text))

    /** JSON (UTF-8) → SYNX text. */
    @JvmStatic
    public fun stringify(json: String): String = takeString(lib.synx_stringify(json))

    /** Canonical SYNX reformat. */
    @JvmStatic
    public fun format(text: String): String = takeString(lib.synx_format(text))

    /** `!tool` reshape → JSON string. */
    @JvmStatic
    public fun parseTool(text: String): String = takeString(lib.synx_parse_tool(text))

    /** Structural diff → JSON string. */
    @JvmStatic
    public fun diff(a: String, b: String): String = takeString(lib.synx_diff(a, b))

    /** SYNX → `.synxb` bytes. */
    @JvmStatic
    public fun compile(text: String, resolved: Boolean = false): ByteArray {
        val outLen = LongByReference()
        val p = lib.synx_compile(text, if (resolved) 1 else 0, outLen)
            ?: throw SynxEngineError.OperationFailed
        val n = outLen.value.toInt()
        return try {
            p.getByteArray(0, n)
        } finally {
            lib.synx_free_bytes(p, NativeLong(n.toLong()))
        }
    }

    /** `.synxb` → SYNX text. */
    @JvmStatic
    public fun decompile(data: ByteArray): String {
        val p = lib.synx_decompile(data, NativeLong(data.size.toLong()))
        return takeString(p)
    }

    /** True if [data] begins with `.synxb` magic. */
    @JvmStatic
    public fun isSynxb(data: ByteArray): Boolean =
        lib.synx_is_synxb(data, NativeLong(data.size.toLong())) != 0
}
