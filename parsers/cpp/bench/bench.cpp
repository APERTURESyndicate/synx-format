// Micro-benchmark: best-of-N wall time per operation and document.
//
//   synx_bench [--runs N] file.synx...
//
// Prints one line per document and operation: parse (with resolution for
// `!active` documents), JSON projection, stringify and format.
#include "synx/synx.hpp"

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>

namespace {

std::string read_file(const char* path) {
    std::ifstream in(path, std::ios::binary);
    std::ostringstream ss;
    ss << in.rdbuf();
    return ss.str();
}

template <typename F>
double best_of(int runs, F&& fn) {
    double best = 1e300;
    for (int i = 0; i < runs; ++i) {
        auto t0 = std::chrono::steady_clock::now();
        fn();
        auto t1 = std::chrono::steady_clock::now();
        best = std::min(best, std::chrono::duration<double, std::micro>(t1 - t0).count());
    }
    return best;
}

void report(const char* doc, const char* op, double us) {
    if (us >= 1000.0) std::printf("%-22s %-10s %10.2f ms\n", doc, op, us / 1000.0);
    else std::printf("%-22s %-10s %10.1f us\n", doc, op, us);
}

volatile size_t g_sink = 0;

} // namespace

int main(int argc, char** argv) {
    int runs = 0;
    std::vector<const char*> files;
    for (int i = 1; i < argc; ++i) {
        if (std::strcmp(argv[i], "--runs") == 0 && i + 1 < argc) runs = std::atoi(argv[++i]);
        else files.push_back(argv[i]);
    }
    for (const char* path : files) {
        const std::string text = read_file(path);
        const char* name = std::strrchr(path, '/');
        name = name ? name + 1 : path;
        // Enough repetitions for a stable minimum, fewer for big inputs.
        int n = runs > 0 ? runs : (text.size() > 1000000 ? 7 : text.size() > 100000 ? 30 : 300);

        synx::ParseResult parsed;
        report(name, "parse", best_of(n, [&] {
            parsed = synx::parse(text);
            if (parsed.mode == synx::Mode::Active) synx::resolve(parsed, synx::Options{});
            g_sink += parsed.root.is_object() ? 1 : 0;
        }));
        report(name, "json", best_of(n, [&] { g_sink += synx::to_json(parsed.root).size(); }));
        report(name, "stringify", best_of(n, [&] { g_sink += synx::stringify(parsed.root).size(); }));
        report(name, "format", best_of(n, [&] { g_sink += synx::format(text).size(); }));
    }
    return 0;
}
