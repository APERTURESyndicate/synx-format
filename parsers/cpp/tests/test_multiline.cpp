// SYNX 3.7 §8.4.1 — the `|+` indent-preserving multiline opener, plus the
// 3.7.2–3.7.4 parser rules that sit around block bodies.
#include "test_helpers.hpp"
#include "synx/json.hpp"
#include "synx/parser.hpp"

#include <string>

using namespace synx;

namespace {

std::string str_of(const char* src, const char* key) {
    ParseResult r = parse(src);
    const Value* v = r.root.get(key);
    const std::string* s = v ? v->as_string() : nullptr;
    return s ? *s : std::string("<not a string>");
}

} // namespace

SYNX_TEST(multiline_preserve_indent_relative_to_first_line) {
    // The worked example from the normative text.
    EXPECT_EQ(str_of("prompt |+\n  Outline:\n    - step one\n    - step two\n      sub-step\n  End.\n", "prompt"),
              std::string("Outline:\n  - step one\n  - step two\n    sub-step\nEnd."));
}

SYNX_TEST(multiline_plain_pipe_still_trims_every_line) {
    EXPECT_EQ(str_of("prompt |\n  Outline:\n    - step one\n  End.\n", "prompt"),
              std::string("Outline:\n- step one\nEnd."));
}

SYNX_TEST(multiline_base_indent_locked_on_first_non_empty_line) {
    EXPECT_EQ(str_of("k |+\n    deep\n  shallow\n", "k"), std::string("deep\nshallow"));
}

SYNX_TEST(multiline_trailing_whitespace_stripped_per_line) {
    EXPECT_EQ(str_of("k |+\n  a   \n  b\t\n", "k"), std::string("a\nb"));
}

SYNX_TEST(multiline_block_ends_at_opener_indent) {
    ParseResult r = parse("k |+\n  body\nother value\n");
    const Value* k = r.root.get("k");
    const Value* other = r.root.get("other");
    EXPECT_TRUE(k && k->is_string() && *k->as_string() == "body");
    EXPECT_TRUE(other && other->is_string() && *other->as_string() == "value");
}

SYNX_TEST(multiline_non_ascii_survives_the_slice) {
    EXPECT_EQ(str_of("k |+\n  Привет\n    мир\n", "k"), std::string("Привет\n  мир"));
}

SYNX_TEST(multiline_unicode_whitespace_indent_never_splits_a_character) {
    // Indent is measured in UTF-8 bytes over Unicode whitespace: the NBSP and the
    // ideographic space are multi-byte indent characters.
    EXPECT_EQ(str_of("k |+\n  first\n  \xC2\xA0 second\n  \xE3\x80\x80third\n", "k"),
              std::string("first\n\xC2\xA0 second\n\xE3\x80\x80third"));
}

// ─── body lines are raw text ───────────────────────────────────────────────

SYNX_TEST(multiline_comment_markers_in_body_are_content) {
    EXPECT_EQ(str_of("k |+\n  # hash\n  // slashes\n  ###\n  end\nafter ok\n", "k"),
              std::string("# hash\n// slashes\n###\nend"));
    EXPECT_EQ(str_of("k |+\n  # hash\n  ###\nafter ok\n", "after"), std::string("ok"));
}

SYNX_TEST(multiline_directives_in_body_are_content) {
    ParseResult r = parse("k |\n  !active\n  !include other.synx\ntail end\n");
    EXPECT_TRUE(r.mode == Mode::Static);
    EXPECT_TRUE(r.includes.empty());
    const Value* k = r.root.get("k");
    EXPECT_TRUE(k && k->is_string() && *k->as_string() == "!active\n!include other.synx");
    const Value* tail = r.root.get("tail");
    EXPECT_TRUE(tail && tail->is_string() && *tail->as_string() == "end");
}

// ─── BOM and type hint spacing ─────────────────────────────────────────────

SYNX_TEST(bom_is_dropped_and_active_still_switches_mode) {
    ParseResult r = parse("\xEF\xBB\xBF!active\nname Wario\n");
    EXPECT_TRUE(r.mode == Mode::Active);
    EXPECT_EQ(str_of("\xEF\xBB\xBF" "name Wario\n", "name"), std::string("Wario"));
    EXPECT_TRUE(r.root.get("\xEF\xBB\xBF!active") == nullptr);
}

SYNX_TEST(type_hint_only_applies_directly_after_the_key) {
    ParseResult r = parse("a (int) 5\nb(int) 5\n");
    const Value* a = r.root.get("a");
    const Value* b = r.root.get("b");
    EXPECT_TRUE(a && a->is_string() && *a->as_string() == "(int) 5");
    EXPECT_TRUE(b && b->is_int() && *b->as_int() == 5);
}

// ─── canonical JSON floats (ryu layout) ────────────────────────────────────

SYNX_TEST(json_floats_use_shortest_round_trip_digits) {
    EXPECT_EQ(to_json(Value::make_float(3.14)), std::string("3.14"));
    EXPECT_EQ(to_json(Value::make_float(1.0)), std::string("1.0"));
    EXPECT_EQ(to_json(Value::make_float(-0.5)), std::string("-0.5"));
    EXPECT_EQ(to_json(Value::make_float(100.0)), std::string("100.0"));
    EXPECT_EQ(to_json(Value::make_float(0.00001)), std::string("0.00001"));
    EXPECT_EQ(to_json(Value::make_float(1e-7)), std::string("1e-7"));
    EXPECT_EQ(to_json(Value::make_float(1e16)), std::string("1e16"));
    EXPECT_EQ(to_json(Value::make_float(1.5e20)), std::string("1.5e20"));
    EXPECT_EQ(to_json(Value::make_float(123456789012345.0)), std::string("123456789012345.0"));
}
