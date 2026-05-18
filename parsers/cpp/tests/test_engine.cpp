#include "test_helpers.hpp"
#include "synx/synx.hpp"

using namespace synx;

SYNX_TEST(engine_env_default) {
    Options opts;
    opts.env.emplace();
    opts.env->emplace("APP_PORT", "9090");
    ParseResult r = parse("!active\nport:env:default:3000 APP_PORT\n");
    resolve(r, opts);
    const Value* port = r.root.get("port");
    EXPECT_TRUE(port && port->is_string());
    EXPECT_EQ(*port->as_string(), std::string("9090"));
}

SYNX_TEST(engine_env_falls_back_to_default) {
    Options opts;
    opts.env.emplace();
    ParseResult r = parse("!active\nport:env:default:3000 NOT_SET\n");
    resolve(r, opts);
    const Value* port = r.root.get("port");
    EXPECT_TRUE(port && port->is_string());
    EXPECT_EQ(*port->as_string(), std::string("3000"));
}

SYNX_TEST(engine_calc_basic) {
    Options opts;
    ParseResult r = parse("!active\nprice 100\ntax:calc price * 0.2\n");
    resolve(r, opts);
    const Value* tax = r.root.get("tax");
    EXPECT_TRUE(tax != nullptr);
    double d = 0.0;
    if (tax->is_int()) d = static_cast<double>(*tax->as_int());
    else if (tax->is_float()) d = *tax->as_float();
    EXPECT_TRUE(d > 19.9 && d < 20.1);
}

SYNX_TEST(engine_secret_redacted_in_json) {
    Options opts;
    ParseResult r = parse("!active\ntoken:secret abc123\n");
    resolve(r, opts);
    std::string json = to_json(r.root);
    EXPECT_TRUE(json.find("[SECRET]") != std::string::npos);
    EXPECT_TRUE(json.find("abc123") == std::string::npos);
}

SYNX_TEST(engine_sum_int_array) {
    Options opts;
    // Build an active result by hand so :sum can be applied to the array.
    ParseResult r;
    Object obj;
    Array arr;
    arr.push_back(Value::make_int(1));
    arr.push_back(Value::make_int(2));
    arr.push_back(Value::make_int(3));
    obj.push_back(Pair{"v", Value::make_array(std::move(arr))});
    r.root = Value::make_object(std::move(obj));
    r.mode = Mode::Active;
    Meta m;
    m.markers = {"sum"};
    r.metadata[""]["v"] = m;
    resolve(r, opts);
    const Value* v = r.root.get("v");
    EXPECT_TRUE(v && v->is_int());
    EXPECT_EQ(*v->as_int(), 6);
}

SYNX_TEST(engine_format_int_pattern) {
    Options opts;
    ParseResult r = parse("!active\nnum 5\npadded:format:%05d:ref num\n");
    // The simpler shape: format applied to its own numeric value.
    r = parse("!active\nnum:format:%05d 42\n");
    resolve(r, opts);
    const Value* v = r.root.get("num");
    EXPECT_TRUE(v && v->is_string());
    EXPECT_EQ(*v->as_string(), std::string("00042"));
}

SYNX_TEST(engine_clamp) {
    Options opts;
    ParseResult r = parse("!active\nx:clamp:0:10 99\n");
    resolve(r, opts);
    const Value* x = r.root.get("x");
    EXPECT_TRUE(x != nullptr);
    double d = 0.0;
    if (x->is_int()) d = static_cast<double>(*x->as_int());
    else if (x->is_float()) d = *x->as_float();
    EXPECT_EQ(d, 10.0);
}
