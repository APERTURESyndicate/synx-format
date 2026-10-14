package com.aperturesyndicate.synx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/**
 * SYNX 3.7 section 8.4.1 - the {@code |+} indent-preserving multiline opener.
 *
 * <p>Mirrors the Rust, .NET and Dart suites.
 */
class IndentPreservingMultilineTest {

    private static String value(String text, String key) {
        return Synx.parse(text).get(key).asString();
    }

    @Test
    void plus_preserves_indent_relative_to_the_first_continuation_line() {
        // The worked example from the normative text.
        String text = "prompt |+\n"
                    + "  Outline:\n"
                    + "    - step one\n"
                    + "    - step two\n"
                    + "      sub-step\n"
                    + "  End.\n";
        assertEquals("Outline:\n  - step one\n  - step two\n    sub-step\nEnd.", value(text, "prompt"));
    }

    @Test
    void plain_pipe_still_trims_every_continuation_line() {
        String text = "prompt |\n"
                    + "  Outline:\n"
                    + "    - step one\n"
                    + "  End.\n";
        assertEquals("Outline:\n- step one\nEnd.", value(text, "prompt"));
    }

    @Test
    void base_indent_is_locked_on_the_first_non_empty_continuation_line() {
        // The shallower line that follows loses all of its indentation; no padding is invented.
        String text = "k |+\n"
                    + "    deep\n"
                    + "  shallow\n";
        assertEquals("deep\nshallow", value(text, "k"));
    }

    @Test
    void trailing_whitespace_is_stripped_per_line() {
        String text = "k |+\n"
                    + "  a   \n"
                    + "  b\t\n";
        assertEquals("a\nb", value(text, "k"));
    }

    @Test
    void block_ends_at_the_opener_indent() {
        String text = "k |+\n"
                    + "  body\n"
                    + "other value\n";
        SynxObject o = Synx.parse(text);
        assertEquals("body", o.get("k").asString());
        assertEquals("value", o.get("other").asString());
    }

    @Test
    void non_ascii_survives_the_slice() {
        String text = "k |+\n"
                    + "  Привет\n"
                    + "    мир\n";
        assertEquals("Привет\n  мир", value(text, "k"));
    }

    @Test
    void astral_characters_are_not_split() {
        String text = "k |+\n"
                    + "  😀 smile\n"
                    + "    😀😀\n";
        assertEquals("😀 smile\n  😀😀", value(text, "k"));
    }

    @Test
    void unicode_whitespace_counts_as_indent() {
        // NBSP (U+00A0) and ideographic space (U+3000) indent a body line like ordinary spaces.
        String text = "k |+\n"
                    + "  first\n"
                    + "    second\n"
                    + "  　third\n";
        assertEquals("first\n  second\n　third", value(text, "k"));
    }

    @Test
    void comments_and_directives_inside_a_body_are_raw_text() {
        String text = "code |+\n"
                    + "  # python comment\n"
                    + "  x = 1 // trailing\n"
                    + "  ### markdown heading\n"
                    + "  !active\n"
                    + "  ###\n"
                    + "after ok\n";
        SynxParseResult r = SynxParser.parse(text);
        SynxObject o = ((SynxValue.Obj) r.root).map();
        assertEquals("# python comment\nx = 1 // trailing\n### markdown heading\n!active\n###", o.get("code").asString());
        assertEquals("ok", o.get("after").asString());
        assertEquals(SynxMode.STATIC, r.mode);
    }

    @Test
    void type_hint_applies_only_when_attached_to_the_key() {
        SynxObject o = Synx.parse("spaced (int) 5\nattached(int) 5\n");
        assertEquals(SynxValue.ofString("(int) 5"), o.get("spaced"));
        assertEquals(SynxValue.ofInt(5), o.get("attached"));
    }
}
