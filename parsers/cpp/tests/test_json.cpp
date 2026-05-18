#include "test_helpers.hpp"
#include "synx/json.hpp"
#include "synx/parser.hpp"

using namespace synx;

SYNX_TEST(json_primitives) {
    EXPECT_EQ(to_json(Value::make_null()), std::string("null"));
    EXPECT_EQ(to_json(Value::make_bool(true)), std::string("true"));
    EXPECT_EQ(to_json(Value::make_int(42)), std::string("42"));
    EXPECT_EQ(to_json(Value::make_string("hi")), std::string("\"hi\""));
}

SYNX_TEST(json_secret_redacted) {
    EXPECT_EQ(to_json(Value::make_secret("s3cr3t")), std::string("\"[SECRET]\""));
}

SYNX_TEST(json_object_sorted_keys) {
    Value v = Value::make_object();
    v.set("b", Value::make_int(2));
    v.set("a", Value::make_int(1));
    std::string j = to_json(v);
    // Keys are alphabetically sorted in canonical output.
    size_t pa = j.find("\"a\"");
    size_t pb = j.find("\"b\"");
    EXPECT_TRUE(pa != std::string::npos);
    EXPECT_TRUE(pb != std::string::npos);
    EXPECT_TRUE(pa < pb);
}

SYNX_TEST(json_escapes) {
    Value v = Value::make_string("line\nbreak\ttab\"quote\\back");
    std::string j = to_json(v);
    EXPECT_TRUE(j.find("\\n") != std::string::npos);
    EXPECT_TRUE(j.find("\\t") != std::string::npos);
    EXPECT_TRUE(j.find("\\\"") != std::string::npos);
    EXPECT_TRUE(j.find("\\\\") != std::string::npos);
}

SYNX_TEST(json_from_parse) {
    ParseResult r = parse("name X\nage 30\nactive true\n");
    std::string j = to_json(r.root);
    EXPECT_TRUE(j.find("\"name\":\"X\"") != std::string::npos);
    EXPECT_TRUE(j.find("\"age\":30") != std::string::npos);
    EXPECT_TRUE(j.find("\"active\":true") != std::string::npos);
}
