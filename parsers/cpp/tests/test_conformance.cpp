// Conformance corpus runner — replays every `NNN-*.synx` case in tests/conformance/cases
// through the C++ parser and compares the canonical JSON projection byte for byte
// against the sibling `<case>.expected.json`. The corpus is shared with Rust / C# / GDScript.
//
// SYNX_CONFORMANCE_DIR is defined by CMake (../../../tests/conformance/cases).
#include "test_helpers.hpp"
#include "synx/json.hpp"
#include "synx/parser.hpp"
#include "synx/synx.hpp"

#include <algorithm>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <sstream>
#include <string>
#include <string_view>
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

// Surrounding whitespace is not part of the contract: old checkouts carry CRLF
// line endings, new ones LF, and 012 has no trailing newline at all.
std::string trim_ws(std::string_view s) {
    size_t a = 0;
    size_t b = s.size();
    while (a < b && (s[a] == ' ' || s[a] == '\t' || s[a] == '\r' || s[a] == '\n')) ++a;
    while (b > a && (s[b - 1] == ' ' || s[b - 1] == '\t' || s[b - 1] == '\r' || s[b - 1] == '\n')) --b;
    return std::string(s.substr(a, b - a));
}

// `!tool` documents go through the tool-envelope API; everything else is a plain parse.
bool starts_with_tool(std::string_view input) {
    size_t a = 0;
    while (a < input.size() && (input[a] == ' ' || input[a] == '\t' || input[a] == '\r' || input[a] == '\n')) ++a;
    return input.substr(a).substr(0, 5) == "!tool";
}

} // namespace

SYNX_TEST(conformance_corpus_matches_expected_json) {
    std::filesystem::path dir = SYNX_CONFORMANCE_DIR;
    if (dir.empty() || !std::filesystem::exists(dir)) {
        // Corpus not present in this checkout — skip silently.
        return;
    }
    std::vector<std::filesystem::path> inputs;
    for (const auto& entry : std::filesystem::directory_iterator(dir)) {
        if (entry.path().extension() == ".synx") inputs.push_back(entry.path());
    }
    std::sort(inputs.begin(), inputs.end());

    int checked = 0;
    int mismatched = 0;
    for (const auto& path : inputs) {
        std::filesystem::path expected_path = path;
        expected_path.replace_extension(".expected.json");
        if (!std::filesystem::exists(expected_path)) continue;

        // Raw bytes: stripping a BOM is the parser's job, not the runner's.
        const std::string input = read_text(path);
        const std::string expected = trim_ws(read_text(expected_path));

        std::string got;
        if (starts_with_tool(input)) {
            got = synx::to_json(synx::Value::make_object(synx::Synx::parse_tool(input)));
        } else {
            // Active documents (also after a BOM) are resolved with default options.
            synx::ParseResult r = synx::parse(input);
            if (r.mode == synx::Mode::Active) synx::resolve(r, synx::Options{});
            got = synx::to_json(r.root);
        }

        ++checked;
        if (got != expected) {
            ++mismatched;
            std::fprintf(stderr, "  corpus mismatch: %s\n    got:      %s\n    expected: %s\n",
                         path.filename().string().c_str(), got.c_str(), expected.c_str());
        }
    }
    std::printf("    [corpus] checked %d cases, %d mismatched\n", checked, mismatched);
    EXPECT_TRUE(checked > 0);
    EXPECT_EQ(mismatched, 0);
}
