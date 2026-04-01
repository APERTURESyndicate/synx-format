package com.aperturesyndicate.synx.internal

import com.sun.jna.Library
import com.sun.jna.Native
import com.sun.jna.Pointer
import com.sun.jna.NativeLong
import com.sun.jna.ptr.LongByReference
import java.nio.charset.StandardCharsets

/** JNA mapping of `bindings/c-header/include/synx.h`; loads `synx_c` / `libsynx_c` / `synx_c.dll`. */
internal interface CSynxLibrary : Library {
    fun synx_parse(input: String?): Pointer?

    fun synx_parse_active(input: String?): Pointer?

    fun synx_stringify(jsonInput: String?): Pointer?

    fun synx_format(input: String?): Pointer?

    fun synx_parse_tool(input: String?): Pointer?

    fun synx_free(ptr: Pointer?)

    fun synx_compile(input: String?, resolved: Int, outLen: LongByReference?): Pointer?

    fun synx_free_bytes(ptr: Pointer?, len: NativeLong)

    fun synx_decompile(data: ByteArray?, len: NativeLong): Pointer?

    fun synx_is_synxb(data: ByteArray?, len: NativeLong): Int

    fun synx_diff(inputA: String?, inputB: String?): Pointer?
}

internal object LibSynx {
    val INSTANCE: CSynxLibrary by lazy { load() }

    private fun load(): CSynxLibrary {
        System.getenv("SYNX_LIB_DIR")?.trim()?.takeIf { it.isNotEmpty() }?.let { dir ->
            System.setProperty("jna.library.path", dir)
        }
        val opts = mapOf(
            Library.OPTION_STRING_ENCODING to StandardCharsets.UTF_8.name(),
        )
        return Native.load("synx_c", CSynxLibrary::class.java, opts)
    }
}
