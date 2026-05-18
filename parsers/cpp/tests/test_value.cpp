#include "test_helpers.hpp"
#include "synx/value.hpp"

using namespace synx;

SYNX_TEST(value_factories) {
    EXPECT_TRUE(Value::make_null().is_null());
    EXPECT_TRUE(Value::make_bool(true).is_bool());
    EXPECT_TRUE(Value::make_int(42).is_int());
    EXPECT_TRUE(Value::make_float(3.14).is_float());
    EXPECT_TRUE(Value::make_string("x").is_string());
    EXPECT_TRUE(Value::make_secret("s").is_secret());
    EXPECT_TRUE(Value::make_array().is_array());
    EXPECT_TRUE(Value::make_object().is_object());
}

SYNX_TEST(value_accessors) {
    Value v = Value::make_int(7);
    EXPECT_TRUE(v.as_int() != nullptr);
    EXPECT_EQ(*v.as_int(), 7);
    EXPECT_TRUE(v.as_string() == nullptr);
}

SYNX_TEST(value_object_get_set) {
    Value v = Value::make_object();
    v.set("a", Value::make_int(1));
    v.set("b", Value::make_string("two"));
    EXPECT_TRUE(v.get("a") != nullptr);
    EXPECT_EQ(*v.get("a")->as_int(), 1);
    EXPECT_TRUE(v.contains("b"));
    EXPECT_TRUE(v.remove("a"));
    EXPECT_FALSE(v.contains("a"));
}

SYNX_TEST(value_equality) {
    Value a = Value::make_object();
    a.set("k", Value::make_int(1));
    Value b = Value::make_object();
    b.set("k", Value::make_int(1));
    EXPECT_TRUE(a.equals(b));

    b.set("k", Value::make_int(2));
    EXPECT_FALSE(a.equals(b));
}
