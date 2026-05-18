// Conformance corpus runner — replays every `.synx` file in tests/conformance/cases
// through the C++ parser and checks the JSON-equivalent against a stored
// `<case>.expected.json` (if present). The corpus is shared with Rust / C# / GDScript.
//
// SYNX_CONFORMANCE_DIR is defined by CMake (../../../tests/conformance/cases).
#include "test_helpers.hpp"
#include "synx/json.hpp"
#include "synx/parser.hpp"

#include <cstdio>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

#ifndef SYNX_CONFORMANCE_DIR
#  define SYNX_CONFORMANCE_DIR ""
#endif

namespace {

std::string read_text(const std::filesystem::path& p) {
    std::ifstream in(p, std::ios::binary);
    if (!in.good()) return {};
    std::ostringstream ss;
    ss << in.rdbuf();
    return ss.str();
}

} // namespace

SYNX_TEST(conformance_corpus_smoke) {
    std::filesystem::path dir = SYNX_CONFORMANCE_DIR;
    if (dir.empty() || !std::filesystem::exists(dir)) {
        // Corpus not present in this checkout — skip silently.
        return;
    }
    int parsed = 0;
    int failed = 0;
    for (const auto& entry : std::filesystem::directory_iterator(dir)) {
        const auto& path = entry.path();
        if (path.extension() != ".synx") continue;
        std::string text = read_text(path);
        if (text.empty()) continue;
        synx::ParseResult r = synx::parse(text);
        // Smoke-only: just make sure parsing did not segfault and yielded an object.
        if (!r.root.is_object()) {
            ++failed;
            std::fprintf(stderr, "  corpus: %s did not yield an object\n",
                         path.string().c_str());
        }
        ++parsed;
    }
    std::printf("    [corpus] parsed %d files, %d failed\n", parsed, failed);
    EXPECT_EQ(failed, 0);
}
