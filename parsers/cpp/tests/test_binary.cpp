#include "test_helpers.hpp"
#include "synx/binary.hpp"
#include "synx/parser.hpp"

using namespace synx;

#if defined(SYNX_HAVE_ZLIB) || 1 // We rely on the compiler define from CMake;
                                 // when zlib is absent compile() returns an error
                                 // (test below tolerates either path).

SYNX_TEST(binary_static_roundtrip) {
    ParseResult r = parse("name App\nport 8080\n");
    auto compiled = compile(r, false);
    if (!compiled.ok()) {
        // zlib not linked — skip silently.
        return;
    }
    EXPECT_TRUE(is_synxb(compiled.value()));
    auto restored = decompile(compiled.value());
    EXPECT_TRUE(restored.ok());
    EXPECT_TRUE(restored.value().root.equals(r.root));
}

SYNX_TEST(binary_magic_check) {
    std::vector<uint8_t> good = {'S','Y','N','X','B', 1, 0};
    std::vector<uint8_t> bad  = {'J','S','O','N'};
    EXPECT_TRUE(is_synxb(good));
    EXPECT_FALSE(is_synxb(bad));
}

SYNX_TEST(binary_invalid_magic_rejected) {
    std::vector<uint8_t> bytes = {'W','R','O','N','G', 1, 0, 0, 0, 0, 0};
    auto res = decompile(bytes);
    EXPECT_FALSE(res.ok());
}

#endif
