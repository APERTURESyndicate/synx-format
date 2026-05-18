#include "test_helpers.hpp"
#include "synx/stringify.hpp"
#include "synx/formatter.hpp"
#include "synx/parser.hpp"

using namespace synx;

SYNX_TEST(stringify_roundtrip_basic) {
    const char* text = "active true\nage 30\nname Wario\n";
    ParseResult r = parse(text);
    std::string out = stringify(r.root);
    EXPECT_TRUE(out.find("name Wario") != std::string::npos);
    EXPECT_TRUE(out.find("age 30") != std::string::npos);
    EXPECT_TRUE(out.find("active true") != std::string::npos);
}

SYNX_TEST(stringify_multiline_uses_pipe) {
    Value v = Value::make_object();
    v.set("rules", Value::make_string("a\nb\nc"));
    std::string out = stringify(v);
    EXPECT_TRUE(out.find("rules |") != std::string::npos);
}

SYNX_TEST(format_sorts_keys) {
    std::string out = format("b 2\na 1\nc 3\n");
    // The first non-empty line should be `a 1` (sorted).
    size_t a = out.find("a 1");
    size_t b = out.find("b 2");
    EXPECT_TRUE(a != std::string::npos && b != std::string::npos);
    EXPECT_TRUE(a < b);
}

SYNX_TEST(format_preserves_directive) {
    std::string out = format("!active\nname X\n");
    EXPECT_TRUE(out.find("!active") == 0);
}
