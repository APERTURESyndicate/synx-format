package com.aperturesyndicate.synx

import com.aperturesyndicate.synx.internal.LibSynx
import com.aperturesyndicate.synx.internal.SizeT
import com.sun.jna.Memory
import com.sun.jna.Native
import com.sun.jna.Pointer
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

    /**
     * Text crosses into C as a NUL-terminated `const char*`: everything after a U+0000
     * would be dropped silently (a document cut short, a `format` that loses the end of
     * the file). Such text is refused instead.
     */
    private fun whole(text: String, what: String = "text"): String {
        require('\u0000' !in text) { "$what contains U+0000, which cannot be passed to synx-c" }
        return text
    }

    /** Static parse → canonical JSON string. */
    @JvmStatic
    public fun parse(text: String): String = takeString(lib.synx_parse(whole(text)))

    /**
     * Parse with `!active` resolution → JSON string.
     *
     * `:env` reads the environment of this process and files resolve in the current
     * directory. For a document you did not write, use the overload that takes [env].
     */
    @JvmStatic
    public fun parseActive(text: String): String = takeString(lib.synx_parse_active(whole(text)))

    /**
     * Parse with `!active` resolution → JSON string, with the environment and the base
     * directory chosen by the caller.
     *
     * @param env the only variables `:env` can see; an empty map gives the document none,
     *   `null` gives it the environment of this process.
     * @param basePath directory that `:include`, `:watch`, `!use` and the `:once` lock file
     *   resolve in and cannot leave; `null` is the current directory.
     */
    @JvmStatic
    @JvmOverloads
    public fun parseActive(text: String, env: Map<String, String>?, basePath: String? = null): String =
        takeString(lib.synx_parse_active_ex(whole(text), env?.let(::envJson), basePath?.let { whole(it, "basePath") }))

    /** JSON (UTF-8) → SYNX text. */
    @JvmStatic
    public fun stringify(json: String): String = takeString(lib.synx_stringify(whole(json, "json")))

    /** Canonical SYNX reformat. */
    @JvmStatic
    public fun format(text: String): String = takeString(lib.synx_format(whole(text)))

    /** `!tool` reshape → JSON string. */
    @JvmStatic
    public fun parseTool(text: String): String = takeString(lib.synx_parse_tool(whole(text)))

    /** `!tool` reshape → JSON string; [env] and [basePath] as in [parseActive]. */
    @JvmStatic
    @JvmOverloads
    public fun parseTool(text: String, env: Map<String, String>?, basePath: String? = null): String =
        takeString(lib.synx_parse_tool_ex(whole(text), env?.let(::envJson), basePath?.let { whole(it, "basePath") }))

    /** [env] as the JSON object of strings `synx_parse_active_ex` takes. */
    private fun envJson(env: Map<String, String>): String {
        val out = StringBuilder("{")
        var isFirst = true
        for ((name, value) in env) {
            if (!isFirst) out.append(',')
            isFirst = false
            appendJsonString(out, name)
            out.append(':')
            appendJsonString(out, value)
        }
        return out.append('}').toString()
    }

    private fun appendJsonString(out: StringBuilder, text: String) {
        out.append('"')
        for (c in text) {
            when {
                c == '"' -> out.append("\\\"")
                c == '\\' -> out.append("\\\\")
                c < ' ' -> out.append(String.format("\\u%04x", c.code))
                else -> out.append(c)
            }
        }
        out.append('"')
    }

    /** Structural diff → JSON string. */
    @JvmStatic
    public fun diff(a: String, b: String): String = takeString(lib.synx_diff(whole(a), whole(b)))

    /** SYNX → `.synxb` bytes. */
    @JvmStatic
    public fun compile(text: String, resolved: Boolean = false): ByteArray {
        val outLen = Memory(Native.SIZE_T_SIZE.toLong())
        val p = lib.synx_compile(whole(text), if (resolved) 1 else 0, outLen)
            ?: throw SynxEngineError.OperationFailed
        val n = if (Native.SIZE_T_SIZE == 8) outLen.getLong(0) else outLen.getInt(0).toLong() and 0xffffffffL
        return try {
            p.getByteArray(0, Math.toIntExact(n))
        } finally {
            lib.synx_free_bytes(p, SizeT(n))
        }
    }

    /** `.synxb` → SYNX text. */
    @JvmStatic
    public fun decompile(data: ByteArray): String {
        val p = lib.synx_decompile(data, SizeT(data.size.toLong()))
        return takeString(p)
    }

    /** True if [data] begins with `.synxb` magic. */
    @JvmStatic
    public fun isSynxb(data: ByteArray): Boolean =
        lib.synx_is_synxb(data, SizeT(data.size.toLong())) != 0
}
