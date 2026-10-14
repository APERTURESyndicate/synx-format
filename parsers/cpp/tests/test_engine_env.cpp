// 3.7.3 / 3.7.4 engine rules: `[type:string]` keeps env text, 64-bit ids stay
// exact, `:secret` never reaches an interpolated string, non-ASCII template
// text survives. Mirrors the Rust engine tests of the same names.
#include "test_helpers.hpp"
#include "synx/synx.hpp"

#include <string>

using namespace synx;

namespace {

ParseResult resolve_env(const char* text) {
    Options opts;
    opts.env.emplace();
    opts.env->emplace("ZIP", "007");
    opts.env->emplace("SNOW", "1550184245153112174");
    opts.env->emplace("Q", "\"12\"");
    ParseResult r = parse(text);
    resolve(r, opts);
    return r;
}

std::string str_of(const ParseResult& r, const char* key) {
    const Value* v = r.root.get(key);
    const std::string* s = v ? v->as_string() : nullptr;
    return s ? *s : std::string("<not a string>");
}

} // namespace

// ─── 3.7.4: `[type:string]` keeps env text, exact 64-bit ids ───────────────

SYNX_TEST(env_type_string_constraint_keeps_text) {
    ParseResult r = resolve_env(
        "!active\nid[type:string]:env SNOW\nzip[type:string]:env ZIP\n"
        "fallback[type:string]:env:default:0042 MISSING\nplain:env ZIP\n");
    EXPECT_EQ(str_of(r, "id"), std::string("1550184245153112174"));
    EXPECT_EQ(str_of(r, "zip"), std::string("007"));
    EXPECT_EQ(str_of(r, "fallback"), std::string("0042"));
    const Value* plain = r.root.get("plain");
    EXPECT_TRUE(plain && plain->is_int() && *plain->as_int() == 7);
}

SYNX_TEST(env_type_hint_string_keeps_text) {
    ParseResult r = resolve_env("!active\nzip(string):env ZIP\n");
    EXPECT_EQ(str_of(r, "zip"), std::string("007"));
}

SYNX_TEST(env_cast_keeps_64bit_integers_exact) {
    ParseResult r = resolve_env("!active\nraw:env SNOW\nq:env Q\n");
    const Value* raw = r.root.get("raw");
    EXPECT_TRUE(raw && raw->is_int() && *raw->as_int() == 1550184245153112174LL);
    EXPECT_EQ(str_of(r, "q"), std::string("12"));
}

SYNX_TEST(default_standalone_honours_string_constraint) {
    ParseResult r = resolve_env("!active\na[type:string]:default:007 null\nb:default:007 null\n");
    EXPECT_EQ(str_of(r, "a"), std::string("007"));
    const Value* b = r.root.get("b");
    EXPECT_TRUE(b && b->is_int() && *b->as_int() == 7);
}

SYNX_TEST(type_string_constraint_accepts_secret) {
    ParseResult r = resolve_env("!active\ntoken[type:string]:secret:env SNOW\n");
    const Value* t = r.root.get("token");
    EXPECT_TRUE(t && t->is_secret());
    EXPECT_TRUE(t && t->is_secret() && *t->as_secret() == "1550184245153112174");
    EXPECT_EQ(to_json(r.root), std::string("{\"token\":\"[SECRET]\"}"));
}

SYNX_TEST(env_reference_projection) {
    ParseResult r = resolve_env(
        "!active\nz:env ZIP\nzs[type:string]:env ZIP\nid[type:string]:env SNOW\n"
        "raw:env SNOW\nd[type:string]:env:default:007 MISSING\n"
        "sec[type:string]:secret:env SNOW\nq:env Q\n");
    EXPECT_EQ(to_json(r.root), std::string(
        "{\"d\":\"007\",\"id\":\"1550184245153112174\",\"q\":\"12\",\"raw\":1550184245153112174,"
        "\"sec\":\"[SECRET]\",\"z\":7,\"zs\":\"007\"}"));
}

// ─── 3.7.3: interpolation ──────────────────────────────────────────────────

SYNX_TEST(interpolation_never_leaks_secret) {
    ParseResult r = parse(
        "!active\ntoken:secret abc\nmsg hello {token} world\nsection\n  nested use {token} now\n");
    resolve(r, Options{});
    EXPECT_EQ(str_of(r, "msg"), std::string("hello [SECRET] world"));
    const Value* section = r.root.get("section");
    const Value* nested = section ? section->get("nested") : nullptr;
    EXPECT_TRUE(nested && nested->is_string() && *nested->as_string() == "use [SECRET] now");
    EXPECT_TRUE(to_json(r.root).find("abc") == std::string::npos);
}

SYNX_TEST(interpolation_preserves_non_ascii) {
    ParseResult r = parse("!active\nname Привет\nmsg мир {name} 東京\n");
    resolve(r, Options{});
    EXPECT_EQ(str_of(r, "msg"), std::string("мир Привет 東京"));
}

SYNX_TEST(auto_interpolation_nested_path) {
    ParseResult r = parse(
        "!active\nserver\n  host localhost\n  port 8080\nurl http://{server.host}:{server.port}/api\n");
    resolve(r, Options{});
    EXPECT_EQ(str_of(r, "url"), std::string("http://localhost:8080/api"));
}

SYNX_TEST(interpolation_unresolved_stays_literal) {
    ParseResult r = parse("!active\nmsg a {nope} b {\n");
    resolve(r, Options{});
    EXPECT_EQ(str_of(r, "msg"), std::string("a {nope} b {"));
}

SYNX_TEST(interpolated_float_uses_shortest_digits) {
    ParseResult r = parse("!active\nratio(float) 3.14\nmsg r={ratio}\n");
    resolve(r, Options{});
    EXPECT_EQ(str_of(r, "msg"), std::string("r=3.14"));
}
