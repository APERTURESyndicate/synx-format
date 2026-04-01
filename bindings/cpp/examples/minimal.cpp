#include <synx/synx.hpp>

#include <cstdlib>
#include <iostream>

int main() {
    const auto j = synx::parse("name Demo\nversion 3.6.0\n");
    if (!j) {
        std::cerr << "synx::parse failed\n";
        return EXIT_FAILURE;
    }
    std::cout << *j << '\n';
    return EXIT_SUCCESS;
}
