// Tiny zero-dependency test framework. Each TEST(name) is auto-registered;
// `synx_run_all_tests()` runs them and reports failures.
#pragma once

#include <cstdio>
#include <cstdlib>
#include <functional>
#include <string>
#include <utility>
#include <vector>

namespace synx_test {

struct Case {
    const char* name;
    std::function<void()> fn;
};

inline std::vector<Case>& cases() {
    static std::vector<Case> v;
    return v;
}

struct Register {
    Register(const char* name, std::function<void()> fn) {
        cases().push_back({name, std::move(fn)});
    }
};

extern int failures;
extern const char* current_test;

inline void fail(const char* expr, const char* file, int line) {
    ++failures;
    std::fprintf(stderr, "FAIL [%s] %s:%d  %s\n",
                 current_test, file, line, expr);
}

} // namespace synx_test

#define SYNX_TEST(name)                                                     \
    static void synx_test_##name();                                         \
    static synx_test::Register synx_test_reg_##name(#name, synx_test_##name); \
    static void synx_test_##name()

#define EXPECT_TRUE(cond)                                                   \
    do { if (!(cond)) synx_test::fail(#cond, __FILE__, __LINE__); } while (0)

#define EXPECT_FALSE(cond) EXPECT_TRUE(!(cond))

#define EXPECT_EQ(a, b)                                                     \
    do { if (!((a) == (b))) synx_test::fail(#a " == " #b, __FILE__, __LINE__); } while (0)

#define EXPECT_NEQ(a, b)                                                    \
    do { if ((a) == (b)) synx_test::fail(#a " != " #b, __FILE__, __LINE__); } while (0)
