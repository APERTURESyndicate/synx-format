package com.aperturesyndicate.synx

import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SynxEngineTest {
    @Test
    fun parseProducesJson() {
        val j = SynxEngine.parse("name Wario\nage 30\n")
        assertContains(j, "Wario")
        assertContains(j, "30")
    }

    @Test
    fun parseToolProducesToolJson() {
        val j = SynxEngine.parseTool("!tool\nweb_search\n  query test\n")
        assertContains(j, "web_search")
        assertContains(j, "query")
    }

    @Test
    fun compileRoundTrip() {
        val text = "k v\n"
        val bin = SynxEngine.compile(text, resolved = false)
        assertTrue(SynxEngine.isSynxb(bin))
        val back = SynxEngine.decompile(bin)
        assertContains(back, "k")
        assertContains(back, "v")
    }

    // `synx_is_synxb` returned a one-byte bool read here as an int: "true" for almost anything.
    @Test
    fun isSynxbIsFalseForOtherData() {
        assertFalse(SynxEngine.isSynxb("hello world, definitely not a binary".toByteArray()))
        assertFalse(SynxEngine.isSynxb("SYNX".toByteArray()))
    }

    @Test
    fun parseActiveSeesOnlyTheEnvironmentItIsHanded() {
        val text = "!active\nname:env WHO\nport:env:default:8080 PATH\n"
        val handed = SynxEngine.parseActive(text, mapOf("WHO" to "quote\" and \\ slash"))
        assertContains(handed, "\"name\":\"quote\\\" and \\\\ slash\"")
        assertContains(handed, "\"port\":8080")
        val empty = SynxEngine.parseActive(text, emptyMap())
        assertContains(empty, "\"port\":8080")
    }

    // A `const char*` ends at U+0000: the rest of the document used to be dropped silently.
    @Test
    fun textWithNulIsRefusedNotCut() {
        val text = "name x\nrole guest\u0000\nrole admin\nport 1\n"
        assertFailsWith<IllegalArgumentException> { SynxEngine.parse(text) }
        assertFailsWith<IllegalArgumentException> { SynxEngine.format(text) }
        assertFailsWith<IllegalArgumentException> { SynxEngine.parseActive("!active\nlimit 5\u0000\nlimit[max:3] 9\n", emptyMap()) }
        assertFailsWith<IllegalArgumentException> { SynxEngine.diff(text, "name x\n") }
    }

    // `size_t` is 8 bytes on 64-bit Windows; `NativeLong` there is 4.
    @Test
    fun lengthsCrossAsSizeT() {
        val bin = SynxEngine.compile("k " + "v".repeat(70_000) + "\n")
        assertTrue(SynxEngine.isSynxb(bin))
        assertContains(SynxEngine.decompile(bin), "v".repeat(70_000))
    }

    @Test
    fun invalidJsonStringifyThrows() {
        assertFailsWith<SynxEngineError> {
            SynxEngine.stringify("not json {{{")
        }
    }
}
