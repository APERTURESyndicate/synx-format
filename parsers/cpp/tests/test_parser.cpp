#include "test_helpers.hpp"
#include "synx/parser.hpp"

using namespace synx;

SYNX_TEST(parse_simple_kv) {
    ParseResult r = parse("name Wario\nage 30\nactive true\nscore 99.5\nempty null");
    const Object* obj = r.root.as_object();
    EXPECT_TRUE(obj != nullptr);
    const Value* age = r.root.get("age");
    EXPECT_TRUE(age && age->is_int());
    EXPECT_EQ(*age->as_int(), 30);
    EXPECT_EQ(r.mode, Mode::Static);

    const Value* active = r.root.get("active");
    EXPECT_TRUE(active && active->is_bool() && *active->as_bool());

    const Value* score = r.root.get("score");
    EXPECT_TRUE(score && score->is_float());

    const Value* empty = r.root.get("empty");
    EXPECT_TRUE(empty && empty->is_null());
}

SYNX_TEST(parse_nested_objects) {
    ParseResult r = parse("server\n  host 0.0.0.0\n  port 8080\n  ssl\n    enabled true");
    const Value* server = r.root.get("server");
    EXPECT_TRUE(server && server->is_object());
    const Value* port = server->get("port");
    EXPECT_TRUE(port && port->is_int());
    EXPECT_EQ(*port->as_int(), 8080);
}

SYNX_TEST(parse_lists) {
    ParseResult r = parse("inventory\n  - Sword\n  - Shield\n  - Potion");
    const Value* inv = r.root.get("inventory");
    EXPECT_TRUE(inv && inv->is_array());
    EXPECT_EQ(inv->as_array()->size(), static_cast<size_t>(3));
}

SYNX_TEST(parse_multiline_block) {
    ParseResult r = parse("rules |\n  Rule one.\n  Rule two.\n  Rule three.");
    const Value* rules = r.root.get("rules");
    EXPECT_TRUE(rules && rules->is_string());
    EXPECT_TRUE(rules->as_string()->find('\n') != std::string::npos);
}

SYNX_TEST(parse_comments) {
    ParseResult r = parse("# comment\nname Wario # inline\nage 30 // inline");
    const Value* name = r.root.get("name");
    EXPECT_TRUE(name && *name->as_string() == "Wario");
}

SYNX_TEST(parse_active_mode) {
    ParseResult r = parse("!active\nprice 100\ntax:calc price * 0.2");
    EXPECT_EQ(r.mode, Mode::Active);
    auto it = r.metadata.find("");
    EXPECT_TRUE(it != r.metadata.end());
    EXPECT_TRUE(it->second.count("tax") > 0);
}

SYNX_TEST(parse_prototype_pollution_rejected) {
    ParseResult r = parse("__proto__ evil\nconstructor evil\nprototype evil\nname safe\n");
    EXPECT_FALSE(r.root.contains("__proto__"));
    EXPECT_FALSE(r.root.contains("constructor"));
    EXPECT_FALSE(r.root.contains("prototype"));
    EXPECT_TRUE(r.root.contains("name"));
}

SYNX_TEST(parse_type_hint_string) {
    ParseResult r = parse("zip(string) 90210");
    const Value* zip = r.root.get("zip");
    EXPECT_TRUE(zip && zip->is_string());
    EXPECT_EQ(*zip->as_string(), std::string("90210"));
}

SYNX_TEST(parse_constraints) {
    ParseResult r = parse("!active\nname[min:3, max:30, required] Wario");
    auto it = r.metadata.find("");
    EXPECT_TRUE(it != r.metadata.end());
    auto kit = it->second.find("name");
    EXPECT_TRUE(kit != it->second.end());
    EXPECT_TRUE(kit->second.constraints.has_value());
    EXPECT_TRUE(kit->second.constraints->required);
}

SYNX_TEST(parse_tool_directive) {
    ParseResult r = parse("!tool\nweb_search\n  query test\n  lang ru\n");
    EXPECT_TRUE(r.tool);
    EXPECT_FALSE(r.schema);

    Value shaped = reshape_tool_output(r.root, false);
    EXPECT_TRUE(shaped.is_object());
    const Value* tool = shaped.get("tool");
    EXPECT_TRUE(tool && tool->is_string());
    EXPECT_EQ(*tool->as_string(), std::string("web_search"));
}
