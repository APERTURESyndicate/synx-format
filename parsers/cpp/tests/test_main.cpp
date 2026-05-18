#include "test_helpers.hpp"

#include <cstdio>

namespace synx_test {
int failures = 0;
const char* current_test = "";
}

int main() {
    using synx_test::cases;
    int total = static_cast<int>(cases().size());
    std::printf("Running %d SYNX tests...\n", total);
    for (auto& c : cases()) {
        synx_test::current_test = c.name;
        int before = synx_test::failures;
        c.fn();
        if (synx_test::failures == before) {
            std::printf("  ok    %s\n", c.name);
        } else {
            std::printf("  FAIL  %s\n", c.name);
        }
    }
    std::printf("\n%d tests, %d failures.\n", total, synx_test::failures);
    return synx_test::failures == 0 ? 0 : 1;
}
