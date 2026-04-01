package com.aperturesyndicate.synx

import kotlin.test.Test
import kotlin.test.assertContains
import kotlin.test.assertFailsWith
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

    @Test
    fun invalidJsonStringifyThrows() {
        assertFailsWith<SynxEngineError> {
            SynxEngine.stringify("not json {{{")
        }
    }
}
