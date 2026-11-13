// Constraint and type-hint failures are written into the value, exactly like the
// Rust engine (`CONSTRAINT_ERR: …`, `TYPE_ERR: …`); `(a+)+`-shaped patterns are
// refused instead of run.
#include "test_helpers.hpp"
#include "synx/synx.hpp"

#include <chrono>
#include <string>
#include <string_view>

using namespace synx;

// Internal helper (src/engine_internal.hpp), linked from the library.
namespace synx { namespace detail {
bool has_nested_quantifier(std::string_view pattern);
} }

namespace {

ParseResult resolved(const std::string& text) {
    ParseResult r = parse(text);
    resolve(r, Options{});
    return r;
}

std::string str_at(const Value& obj, const char* key) {
    const Value* v = obj.get(key);
    const std::string* s = v ? v->as_string() : nullptr;
    return s ? *s : std::string("<not a string>");
}

} // namespace

SYNX_TEST(constraints_required_type_enum_messages) {
    ParseResult r = resolved(
        "!active\nmissing[required] null\nblank[required] \"\"\n"
        "age_bad[type:int] forty\nflag[type:bool] yes\n"
        "color_bad[enum:red|green|blue] purple\n");
    EXPECT_EQ(str_at(r.root, "missing"), std::string("CONSTRAINT_ERR: 'missing' is required"));
    EXPECT_EQ(str_at(r.root, "blank"), std::string("CONSTRAINT_ERR: 'blank' is required"));
    EXPECT_EQ(str_at(r.root, "age_bad"), std::string("CONSTRAINT_ERR: 'age_bad' expected type 'int'"));
    EXPECT_EQ(str_at(r.root, "flag"), std::string("CONSTRAINT_ERR: 'flag' expected type 'bool'"));
    EXPECT_EQ(str_at(r.root, "color_bad"),
              std::string("CONSTRAINT_ERR: 'color_bad' must be one of [red|green|blue]"));
}

SYNX_TEST(constraints_float_type_accepts_int) {
    ParseResult r = resolved("!active\nratio[type:float] 3\n");
    const Value* v = r.root.get("ratio");
    EXPECT_TRUE(v && v->is_int() && *v->as_int() == 3);
}

SYNX_TEST(constraints_min_max_numbers_print_like_rust_display) {
    ParseResult r = resolved("!active\nlow[min:1, max:10] 0\nhigh[min:1, max:10] 11\nscore[min:0.5] 0.25\n");
    EXPECT_EQ(str_at(r.root, "low"), std::string("CONSTRAINT_ERR: 'low' value 0 is below min 1"));
    EXPECT_EQ(str_at(r.root, "high"), std::string("CONSTRAINT_ERR: 'high' value 11 exceeds max 10"));
    EXPECT_EQ(str_at(r.root, "score"), std::string("CONSTRAINT_ERR: 'score' value 0.25 is below min 0.5"));
}

SYNX_TEST(constraints_string_length_counts_unicode_scalars_not_bytes) {
    ParseResult r = resolved("!active\ncyr[max:4] мир\ncyr_long[max:4] мирок\nemoji[max:2] 😀😀\n");
    EXPECT_EQ(str_at(r.root, "cyr"), std::string("мир"));
    EXPECT_EQ(str_at(r.root, "emoji"), std::string("😀😀"));
    EXPECT_EQ(str_at(r.root, "cyr_long"), std::string("CONSTRAINT_ERR: 'cyr_long' value 5 exceeds max 4"));
}

SYNX_TEST(constraints_pattern_mismatch_message) {
    ParseResult r = resolved("!active\nok[pattern:^[a-z]+$] abc\nbad[pattern:^[a-z]+$] ABC\n");
    EXPECT_EQ(str_at(r.root, "ok"), std::string("abc"));
    EXPECT_EQ(str_at(r.root, "bad"),
              std::string("CONSTRAINT_ERR: 'bad' does not match pattern /^[a-z]+$/"));
}

SYNX_TEST(constraints_run_after_markers) {
    ParseResult r = resolved("!active\nbase 7\ntotal[max:10]:calc base * 2\nsmall[max:20]:calc base * 2\n");
    EXPECT_EQ(str_at(r.root, "total"), std::string("CONSTRAINT_ERR: 'total' value 14 exceeds max 10"));
    const Value* small = r.root.get("small");
    EXPECT_TRUE(small && small->is_int() && *small->as_int() == 14);
}

SYNX_TEST(constraints_field_registry_applies_by_name_across_the_document) {
    ParseResult r = resolved("!active\na\n  port[type:int, min:1, max:65535] 8080\nb\n  port 70000\nc\n  port abc\n");
    const Value* a = r.root.get("a");
    const Value* ap = a ? a->get("port") : nullptr;
    EXPECT_TRUE(ap && ap->is_int() && *ap->as_int() == 8080);
    EXPECT_EQ(str_at(*r.root.get("b"), "port"),
              std::string("CONSTRAINT_ERR: 'port' value 70000 exceeds max 65535"));
    EXPECT_EQ(str_at(*r.root.get("c"), "port"),
              std::string("CONSTRAINT_ERR: 'port' expected type 'int'"));
}

SYNX_TEST(constraints_registry_merge_takes_the_stricter_bounds) {
    ParseResult r = resolved("!active\nx[min:1, max:100] 5\ny\n  x[min:10, max:50] 7\n");
    EXPECT_EQ(str_at(r.root, "x"), std::string("CONSTRAINT_ERR: 'x' value 5 is below min 10"));
    EXPECT_EQ(str_at(*r.root.get("y"), "x"), std::string("CONSTRAINT_ERR: 'x' value 7 is below min 10"));
}

SYNX_TEST(constraints_type_registry_reports_type_err) {
    ParseResult r = resolved("!active\nhp(int) 100\nmonster\n  hp 50\nboss\n  hp lots\n");
    const Value* hp = r.root.get("hp");
    EXPECT_TRUE(hp && hp->is_int() && *hp->as_int() == 100);
    EXPECT_EQ(str_at(*r.root.get("boss"), "hp"),
              std::string("TYPE_ERR: 'hp' expected int but got string"));
}

SYNX_TEST(constraints_existing_error_strings_are_not_revalidated) {
    ParseResult r = resolved("!active\nage[type:int, max:5] forty\n");
    EXPECT_EQ(str_at(r.root, "age"), std::string("CONSTRAINT_ERR: 'age' expected type 'int'"));
}

// ─── ReDoS guard ───────────────────────────────────────────────────────────

SYNX_TEST(constraints_nested_quantifier_pattern_is_refused) {
    EXPECT_TRUE(detail::has_nested_quantifier("^(a+)+$"));
    EXPECT_TRUE(detail::has_nested_quantifier("(a*){2,}"));
    EXPECT_FALSE(detail::has_nested_quantifier("^[a-z]+$"));
    EXPECT_FALSE(detail::has_nested_quantifier("^(ab)c+$"));
    EXPECT_FALSE(detail::has_nested_quantifier("^([+*])$"));
}

SYNX_TEST(constraints_redos_pattern_is_skipped_quickly_and_value_unchanged) {
    const std::string value = std::string(45, 'a') + "!";
    auto started = std::chrono::steady_clock::now();
    ParseResult r = resolved("!active\nv[pattern:^(a+)+$] " + value + "\n");
    auto elapsed = std::chrono::steady_clock::now() - started;
    EXPECT_TRUE(elapsed < std::chrono::seconds(2));
    EXPECT_EQ(str_at(r.root, "v"), value);
}

// ─── :calc substitutes shortest floats ─────────────────────────────────────

SYNX_TEST(calc_substitution_matches_rust_number_formatting) {
    ParseResult r = resolved("!active\na 0.1\nb:calc a * 3\nc:calc a + 0.2\nd 2.0\ne:calc d * 3\nf:calc a / 3\n");
    EXPECT_EQ(to_json(r.root), std::string(
        "{\"a\":0.1,\"b\":0.30000000000000004,\"c\":0.30000000000000004,\"d\":2.0,"
        "\"e\":6,\"f\":0.03333333333333333}"));
}

SYNX_TEST(whole_floats_interpolate_without_fraction) {
    ParseResult r = resolved("!active\nratio(float) 2.0\nmsg {ratio} x\n");
    EXPECT_EQ(str_at(r.root, "msg"), std::string("2 x"));
}

// ─── patterns: the regex crate's dialect, linear time ──────────────────────

SYNX_TEST(constraints_patterns_the_regex_crate_refuses_are_skipped) {
    // The `regex` crate refuses each of these; synx-core then checks nothing.
    const char* refused[] = {
        "[", "(", ")", "a{2,1}", "\\", "*a", "[z-a]", "x{", "x{,3}", "\\1", "(a", "a)", "[a", "a|*",
        "(?=a)+", "[\\", "\\x4", "{", "(?i)*", "\\Q", "(?-u:.)", "\\p{N", "\\p{Word}",
    };
    for (const char* pat : refused) {
        ParseResult r = resolved(std::string("!active\nv[pattern:") + pat + "] hello\n");
        EXPECT_EQ(str_at(r.root, "v"), std::string("hello"));
    }
}

SYNX_TEST(constraints_patterns_read_as_the_regex_crate_reads_them) {
    // pattern, value, whether it matches: the crate's answers (`regex` 1.12).
    struct Case { const char* pat; const char* value; bool match; };
    const Case cases[] = {
        {"a**", "b", true}, {"^*", "b", true}, {"(?<n>x)", "x", true}, {"a{99999}", "a", false},
        {"[[:nope:]]", "e", true}, {"^[a-z]+$", "hello", true}, {"^\\d{3}-\\d{4}$", "555-1234", true},
        {"(?i)^abc$", "ABC", true}, {"(?i)^abc$", "x", false},
        {"^\\p{L}+$", "\xD0\xBC\xD0\xB8\xD1\x80", true}, {"^\\d+$", "\xD9\xA1\xD9\xA2", true},
        {"\\bfoo\\b", "food", false}, {"\\bfoo\\b", "a foo b", true}, {"(?i)k", "\xE2\x84\xAA", true},
        {"[a-z&&[^aeiou]]+$", "xyz", true}, {"^\\p{Greek}+$", "\xCE\xB1", true}, {"(?i)[^k]", "K", false},
        {"^.$", "\xF0\x9F\x98\x80", true}, {"[\\w--\\d]", "5.", false}, {"(?x) a b # c", "ab", true},
    };
    for (const Case& c : cases) {
        ParseResult r = resolved(std::string("!active\nv[pattern:") + c.pat + "] " + c.value + "\n");
        const std::string want = c.match ? std::string(c.value)
                                         : "CONSTRAINT_ERR: 'v' does not match pattern /" + std::string(c.pat) + "/";
        EXPECT_EQ(str_at(r.root, "v"), want);
    }
}

SYNX_TEST(constraints_patterns_run_in_linear_time) {
    // `std::regex` took seconds per extra character here (B-079).
    const std::string value = std::string(5000, 'a') + "!";
    const auto start = std::chrono::steady_clock::now();
    ParseResult r = resolved("!active\nv[pattern:^(a|aa)+$] " + value + "\nw[pattern:^(.*)(.*)(.*)(.*)(.*)(.*)(.*)x$] "
                             + value + "\n");
    const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - start).count();
    EXPECT_EQ(str_at(r.root, "v"), std::string("CONSTRAINT_ERR: 'v' does not match pattern /^(a|aa)+$/"));
    EXPECT_TRUE(ms < 5000);
}

// ─── :calc failures carry the exact Rust CALC_ERR string ───────────────────

SYNX_TEST(calc_failures_write_calc_err_into_the_value) {
    const std::string dash = "\xE2\x80\x94";
    ParseResult r = resolved(
        "!active\nbase 7\nu:calc nope + 1\nz:calc 1 / 0\nm:calc 5 % 0\n"
        "p1:calc (1 + 2\np2:calc 1 + 2)\nstar:calc * 3\nrp:calc )\nhole:calc 2 +\n"
        "num:calc 1.2.3 + 1\nop:calc 2 * -\nbad:calc 3 3\nuni:calc мир\n");
    const std::string pre = "CALC_ERR: SYNX :calc " + dash + " ";
    EXPECT_EQ(str_at(r.root, "u"), pre + "unexpected character: 'n' in expression");
    EXPECT_EQ(str_at(r.root, "z"), pre + "division by zero");
    EXPECT_EQ(str_at(r.root, "m"), pre + "division by zero");
    EXPECT_EQ(str_at(r.root, "p1"), pre + "missing closing parenthesis");
    EXPECT_EQ(str_at(r.root, "p2"), pre + "unexpected token at position 3");
    // 3.8.0: the token as written, not Rust's `{:?}` of it.
    EXPECT_EQ(str_at(r.root, "star"), pre + "unexpected token: '*'");
    EXPECT_EQ(str_at(r.root, "rp"), pre + "unexpected token: ')'");
    EXPECT_EQ(str_at(r.root, "hole"), pre + "unexpected end of expression");
    EXPECT_EQ(str_at(r.root, "num"), pre + "invalid number: '1.2.3'");
    EXPECT_EQ(str_at(r.root, "op"), pre + "invalid number: '-'");
    EXPECT_EQ(str_at(r.root, "bad"), pre + "unexpected token at position 1");
    // 3.8.0: the whole character, not its first byte read as Latin-1.
    EXPECT_EQ(str_at(r.root, "uni"), pre + "unexpected character: '\xD0\xBC' in expression");
}

SYNX_TEST(calc_too_long_expression) {
    std::string expr;
    for (int k = 0; k < 2100; ++k) expr += "1+";
    expr += "1";
    ParseResult r = resolved("!active\nlong:calc " + expr + "\n");
    EXPECT_EQ(str_at(r.root, "long"), std::string("CALC_ERR: expression too long (4201 chars, max 4096)"));
}

SYNX_TEST(calc_reads_siblings_and_dotted_paths) {
    ParseResult r = resolved("!active\nbase_obj\n  hp 10\n  dmg:calc hp * 2\ndot:calc base_obj.hp + 1\n");
    const Value* dmg = r.root.get("base_obj") ? r.root.get("base_obj")->get("dmg") : nullptr;
    const Value* dot = r.root.get("dot");
    EXPECT_TRUE(dmg && dmg->is_int() && *dmg->as_int() == 20);
    EXPECT_TRUE(dot && dot->is_int() && *dot->as_int() == 11);
}
