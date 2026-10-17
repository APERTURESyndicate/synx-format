#include "test_helpers.hpp"
#include "synx/binary.hpp"
#include "synx/parser.hpp"
#include "internal.hpp"

using namespace synx;

SYNX_TEST(binary_static_roundtrip) {
    ParseResult r = parse("name App\nport 8080\n");
    // The payload layer runs in every build; DEFLATE needs zlib.
    std::vector<uint8_t> payload = detail::encode_synxb_payload(r, false);
    auto plain = detail::decode_synxb_payload(detail::synxb_flags(r, false), payload.data(), payload.size());
    EXPECT_TRUE(plain.ok() && plain.value().root.equals(r.root));
    auto compiled = compile(r, false);
#if !defined(SYNX_HAVE_ZLIB)
    EXPECT_FALSE(compiled.ok());
    return;
#endif
    EXPECT_TRUE(compiled.ok());
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
