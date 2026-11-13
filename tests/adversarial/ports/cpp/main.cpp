// Port worker for the C++ implementation: one request per line on stdin,
//   id \t op \t base64(text) \t base64(KEY=VALUE lines) \t base directory
// and one answer per line on stdout,
//   id \t base64(canonical JSON, or "ERROR: …") \t base64(default string form of the tree)
// The C++ port has no default string form; the third field is empty.
#include <cstdio>
#include <iostream>
#include <string>
#include <unordered_map>
#include <vector>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#endif

#include "synx/synx.hpp"

static const char* const ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

static std::string encode(const std::string& in) {
    std::string out;
    out.reserve((in.size() + 2) / 3 * 4);
    size_t i = 0;
    for (; i + 2 < in.size(); i += 3) {
        unsigned v = (static_cast<unsigned char>(in[i]) << 16) | (static_cast<unsigned char>(in[i + 1]) << 8) | static_cast<unsigned char>(in[i + 2]);
        out += ALPHABET[v >> 18];
        out += ALPHABET[(v >> 12) & 63];
        out += ALPHABET[(v >> 6) & 63];
        out += ALPHABET[v & 63];
    }
    if (i + 1 == in.size()) {
        unsigned v = static_cast<unsigned char>(in[i]) << 16;
        out += ALPHABET[v >> 18];
        out += ALPHABET[(v >> 12) & 63];
        out += "==";
    } else if (i + 2 == in.size()) {
        unsigned v = (static_cast<unsigned char>(in[i]) << 16) | (static_cast<unsigned char>(in[i + 1]) << 8);
        out += ALPHABET[v >> 18];
        out += ALPHABET[(v >> 12) & 63];
        out += ALPHABET[(v >> 6) & 63];
        out += '=';
    }
    return out;
}

static std::string decode(const std::string& in) {
    int table[256];
    for (int& t : table) t = -1;
    for (int i = 0; i < 64; i++) table[static_cast<unsigned char>(ALPHABET[i])] = i;
    std::string out;
    unsigned acc = 0;
    int bits = 0;
    for (unsigned char c : in) {
        if (table[c] < 0) continue;
        acc = (acc << 6) | static_cast<unsigned>(table[c]);
        bits += 6;
        if (bits >= 8) {
            bits -= 8;
            out += static_cast<char>((acc >> bits) & 0xff);
        }
    }
    return out;
}

static std::vector<std::string> split(const std::string& s, char sep) {
    std::vector<std::string> out;
    size_t start = 0;
    for (;;) {
        size_t at = s.find(sep, start);
        if (at == std::string::npos) {
            out.push_back(s.substr(start));
            return out;
        }
        out.push_back(s.substr(start, at - start));
        start = at + 1;
    }
}

int main() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    std::string line;
    while (std::getline(std::cin, line)) {
        if (!line.empty() && line.back() == '\r') line.pop_back();
        std::vector<std::string> f = split(line, '\t');
        if (f.size() < 5) continue;
        std::string text = decode(f[2]);
        synx::Options options;
        std::unordered_map<std::string, std::string> env;
        for (const std::string& pair : split(decode(f[3]), '\n')) {
            size_t eq = pair.find('=');
            if (eq != std::string::npos && eq > 0) env[pair.substr(0, eq)] = pair.substr(eq + 1);
        }
        options.env = env;
        options.base_path = f[4];
        synx::ParseResult result = synx::parse(text);
        if (f[1] == "active" && result.mode == synx::Mode::Active) synx::resolve(result, options);
        std::string json = synx::to_json(result.root);
        std::string answer = f[0] + "\t" + encode(json) + "\t\n";
        std::fwrite(answer.data(), 1, answer.size(), stdout);
        std::fflush(stdout);
    }
    return 0;
}
