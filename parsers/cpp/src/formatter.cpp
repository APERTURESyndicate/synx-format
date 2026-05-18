// Canonical SYNX text reformatter. Mirrors fmt_canonical in synx-core/src/lib.rs.
#include "synx/formatter.hpp"
#include "synx/parser.hpp"

#include <algorithm>
#include <cctype>
#include <string>
#include <string_view>
#include <vector>

namespace synx {

namespace {

inline std::string_view trim_view(std::string_view s) noexcept {
    size_t start = 0;
    while (start < s.size() && (s[start] == ' ' || s[start] == '\t' || s[start] == '\r')) ++start;
    size_t end = s.size();
    while (end > start && (s[end - 1] == ' ' || s[end - 1] == '\t' || s[end - 1] == '\r')) --end;
    return s.substr(start, end - start);
}

inline bool starts_with(std::string_view s, std::string_view prefix) noexcept {
    return s.size() >= prefix.size()
        && std::char_traits<char>::compare(s.data(), prefix.data(), prefix.size()) == 0;
}

inline size_t indent_of(std::string_view line) noexcept {
    size_t i = 0;
    while (i < line.size() && (line[i] == ' ' || line[i] == '\t')) ++i;
    return i;
}

struct FmtNode {
    std::string header;
    std::vector<FmtNode> children;
    std::vector<std::string> list_items;
    bool is_multiline = false;
};

void split_lines(std::string_view text, std::vector<std::string_view>& out) {
    size_t start = 0;
    for (size_t i = 0; i <= text.size(); ++i) {
        if (i == text.size() || text[i] == '\n') {
            std::string_view line = text.substr(start, i - start);
            // Strip trailing \r for CRLF.
            if (!line.empty() && line.back() == '\r') {
                line.remove_suffix(1);
            }
            out.push_back(line);
            start = i + 1;
        }
    }
}

void fmt_parse(const std::vector<std::string_view>& lines,
               size_t start,
               size_t base,
               size_t depth,
               std::vector<FmtNode>& nodes,
               size_t& next_idx) {
    next_idx = start;
    if (depth > kMaxFmtParseDepth) {
        return;
    }
    size_t i = start;
    while (i < lines.size()) {
        std::string_view raw = lines[i];
        std::string_view t = trim_view(raw);
        if (t.empty()) { ++i; continue; }
        size_t ind = indent_of(raw);
        if (ind < base) break;
        if (ind > base) { ++i; continue; }
        if (starts_with(t, "- ") || t[0] == '#' || starts_with(t, "//")) { ++i; continue; }

        FmtNode node;
        node.header = std::string(t);
        node.is_multiline = (t == "|") || (t.size() >= 2 && t[t.size() - 2] == ' ' && t.back() == '|');
        ++i;

        while (i < lines.size()) {
            std::string_view cr = lines[i];
            std::string_view ct = trim_view(cr);
            if (ct.empty()) { ++i; continue; }
            size_t ci = indent_of(cr);
            if (ci <= base) break;
            if (node.is_multiline || starts_with(ct, "- ")) {
                node.list_items.emplace_back(ct);
                ++i;
            } else if (ct[0] == '#' || starts_with(ct, "//")) {
                ++i;
            } else {
                std::vector<FmtNode> subs;
                size_t ni = i;
                fmt_parse(lines, i, ci, depth + 1, subs, ni);
                for (auto& s : subs) node.children.push_back(std::move(s));
                i = ni;
            }
        }
        nodes.push_back(std::move(node));
    }
    next_idx = i;
}

std::string fmt_sort_key(const std::string& header) {
    size_t end = 0;
    while (end < header.size()) {
        char c = header[end];
        if (c == ' ' || c == '\t' || c == '[' || c == ':' || c == '(') break;
        ++end;
    }
    std::string out = header.substr(0, end);
    for (char& c : out) {
        c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    }
    return out;
}

void fmt_sort(std::vector<FmtNode>& nodes) {
    std::stable_sort(nodes.begin(), nodes.end(),
        [](const FmtNode& a, const FmtNode& b) {
            return fmt_sort_key(a.header) < fmt_sort_key(b.header);
        });
    for (auto& n : nodes) fmt_sort(n.children);
}

void fmt_emit(const std::vector<FmtNode>& nodes, size_t indent_lvl, std::string& out) {
    std::string sp(indent_lvl, ' ');
    std::string item_sp(indent_lvl + 2, ' ');
    for (const auto& n : nodes) {
        out.append(sp);
        out.append(n.header);
        out.push_back('\n');
        if (!n.children.empty()) {
            fmt_emit(n.children, indent_lvl + 2, out);
        }
        for (const auto& li : n.list_items) {
            out.append(item_sp);
            out.append(li);
            out.push_back('\n');
        }
        if (indent_lvl == 0 && (!n.children.empty() || !n.list_items.empty())) {
            out.push_back('\n');
        }
    }
}

} // namespace

std::string format(std::string_view text) {
    std::string_view clamped = clamp_synx_text(text);

    std::vector<std::string_view> lines;
    lines.reserve(64);
    split_lines(clamped, lines);

    std::vector<std::string_view> directives;
    size_t body_start = 0;
    for (size_t i = 0; i < lines.size(); ++i) {
        std::string_view t = trim_view(lines[i]);
        if (t == "!active" || t == "!lock" || t == "!tool"
            || t == "!schema" || t == "!llm" || t == "#!mode:active") {
            directives.push_back(t);
            body_start = i + 1;
        } else if (t.empty() || t[0] == '#' || starts_with(t, "//")) {
            body_start = i + 1;
        } else {
            break;
        }
    }

    std::vector<FmtNode> nodes;
    size_t next = body_start;
    fmt_parse(lines, body_start, 0, 0, nodes, next);
    fmt_sort(nodes);

    std::string out;
    out.reserve(std::min(clamped.size(), kMaxSynxInputBytes) + 64);

    if (!directives.empty()) {
        for (size_t i = 0; i < directives.size(); ++i) {
            if (i > 0) out.push_back('\n');
            out.append(directives[i]);
        }
        out.append("\n\n");
    }
    fmt_emit(nodes, 0, out);

    // Trim trailing blank lines, ensure single newline at end.
    while (!out.empty() && (out.back() == '\n' || out.back() == ' ' || out.back() == '\t')) {
        out.pop_back();
    }
    out.push_back('\n');
    return out;
}

} // namespace synx
