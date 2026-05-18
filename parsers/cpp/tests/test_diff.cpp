#include "test_helpers.hpp"
#include "synx/diff.hpp"

using namespace synx;

namespace {
Object obj_of(std::initializer_list<std::pair<const char*, Value>> entries) {
    Object o;
    for (const auto& e : entries) o.push_back(Pair{e.first, e.second});
    return o;
}
} // namespace

SYNX_TEST(diff_identical) {
    Object a = obj_of({{"x", Value::make_int(1)}, {"y", Value::make_int(2)}});
    Object b = a;
    DiffResult d = diff(a, b);
    EXPECT_TRUE(d.added.empty());
    EXPECT_TRUE(d.removed.empty());
    EXPECT_TRUE(d.changed.empty());
    EXPECT_EQ(d.unchanged.size(), static_cast<size_t>(2));
}

SYNX_TEST(diff_added_removed) {
    Object a = obj_of({{"x", Value::make_int(1)}});
    Object b = obj_of({{"y", Value::make_int(2)}});
    DiffResult d = diff(a, b);
    EXPECT_EQ(d.added.size(), static_cast<size_t>(1));
    EXPECT_EQ(d.removed.size(), static_cast<size_t>(1));
}

SYNX_TEST(diff_changed) {
    Object a = obj_of({{"name", Value::make_string("Alice")}});
    Object b = obj_of({{"name", Value::make_string("Bob")}});
    DiffResult d = diff(a, b);
    EXPECT_EQ(d.changed.size(), static_cast<size_t>(1));
}
