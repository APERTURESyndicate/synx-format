#include "test_helpers.hpp"
#include "synx/calc.hpp"

using namespace synx;

SYNX_TEST(calc_basic_ops) {
    EXPECT_EQ(safe_calc("2 + 3").value, 5.0);
    EXPECT_EQ(safe_calc("10 - 4").value, 6.0);
    EXPECT_EQ(safe_calc("3 * 7").value, 21.0);
    EXPECT_EQ(safe_calc("20 / 4").value, 5.0);
    EXPECT_EQ(safe_calc("10 % 3").value, 1.0);
}

SYNX_TEST(calc_precedence) {
    EXPECT_EQ(safe_calc("2 + 3 * 4").value, 14.0);
    EXPECT_EQ(safe_calc("(2 + 3) * 4").value, 20.0);
}

SYNX_TEST(calc_negatives) {
    EXPECT_EQ(safe_calc("-5 + 3").value, -2.0);
    EXPECT_EQ(safe_calc("10 * -2").value, -20.0);
}

SYNX_TEST(calc_div_by_zero) {
    CalcResult r = safe_calc("10 / 0");
    EXPECT_FALSE(r.ok);
    EXPECT_FALSE(r.error.empty());
}

SYNX_TEST(calc_empty) {
    EXPECT_EQ(safe_calc("").value, 0.0);
    EXPECT_TRUE(safe_calc("").ok);
}
