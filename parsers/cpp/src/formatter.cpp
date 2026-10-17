// Canonical SYNX text reformatter. Port of `fmt_canonical` in synx-core/src/lib.rs.
#include "synx/formatter.hpp"

#include "synx/parser.hpp"
#include "unicode.hpp"

#include <algorithm>
#include <string>
#include <string_view>
#include <vector>

namespace synx {

namespace {

using detail::ends_with;
using detail::starts_with;
using detail::trim_view;

struct FmtNode {
    std::string header;
    std::vector<FmtNode> children;
    std::vector<std::string> list_items;
    bool is_multiline = false;
};

// Rust `str::lines`: split at `\n`, drop one trailing `\r`, no final empty line.
std::vector<std::string_view> split_lines(std::string_view text) {
    std::vector<std::string_view> out;
    size_t start = 0;
    while (start < text.size()) {
        size_t nl = text.find('\n', start);
        size_t end = nl == std::string_view::npos ? text.size() : nl;
        std::string_view line = text.substr(start, end - start);
        if (!line.empty() && line.back() == '\r') line.remove_suffix(1);
        out.push_back(line);
        start = end + 1;
    }
    return out;
}

size_t fmt_parse(const std::vector<std::string_view>& lines, size_t start, size_t base, size_t depth,
                 std::vector<FmtNode>& nodes) {
    if (depth > kMaxFmtParseDepth) {
        // safety: the deeper subtree is dropped, but its lines are consumed —
        // returning `start` left the caller on the same line forever.
        size_t i = start;
        while (i < lines.size() && (trim_view(lines[i]).empty() || detail::indent_len(lines[i]) >= base)) ++i;
        return i;
    }
    size_t i = start;
    while (i < lines.size()) {
        std::string_view raw = lines[i];
        std::string_view t = trim_view(raw);
        if (t.empty()) { ++i; continue; }
        size_t ind = detail::indent_len(raw);
        if (ind < base) break;
        if (ind > base) { ++i; continue; }
        if (starts_with(t, "- ") || t[0] == '#' || starts_with(t, "//")) { ++i; continue; }
        // `|+` opens a block too: its lines are text and keep their indent
        // relative to the first; they are never parsed as keys nor sorted.
        const bool preserve = ends_with(t, " |+") || t == "|+";
        FmtNode node;
        node.header = std::string(t);
        node.is_multiline = preserve || ends_with(t, " |") || t == "|";
        size_t block_base = static_cast<size_t>(-1);
        ++i;
        while (i < lines.size()) {
            std::string_view cr = lines[i];
            std::string_view ct = trim_view(cr);
            if (ct.empty()) { ++i; continue; }
            size_t ci = detail::indent_len(cr);
            if (ci <= base) break;
            if (preserve) {
                if (block_base == static_cast<size_t>(-1)) block_base = ci;
                std::string_view body = detail::trim_end_view(cr);
                size_t cut = detail::strip_boundary(body, std::min(block_base, ci));
                node.list_items.emplace_back(body.substr(cut));
                ++i;
            } else if (node.is_multiline || starts_with(ct, "- ")) {
                node.list_items.emplace_back(ct);
                ++i;
            } else if (ct[0] == '#' || starts_with(ct, "//")) {
                ++i;
            } else {
                i = fmt_parse(lines, i, ci, depth + 1, node.children);
            }
        }
        nodes.push_back(std::move(node));
    }
    return i;
}

// The key of a header, lowercased: up to the first whitespace, `[`, `:` or `(`.
std::string sort_key(const std::string& header) {
    size_t end = 0;
    while (end < header.size()) {
        unsigned char c = static_cast<unsigned char>(header[end]);
        if (c == '[' || c == ':' || c == '(') break;
        if (detail::ws_len_at(header, end) != 0) break;
        end += detail::utf8_len(c);
    }
    return detail::to_lowercase(std::string_view(header).substr(0, end));
}

void fmt_sort(std::vector<FmtNode>& nodes) {
    // Stable, by code point of the lowercased key; each key is computed once.
    std::vector<std::pair<std::string, size_t>> keyed;
    keyed.reserve(nodes.size());
    for (size_t i = 0; i < nodes.size(); ++i) keyed.emplace_back(sort_key(nodes[i].header), i);
    std::stable_sort(keyed.begin(), keyed.end(),
                     [](const auto& a, const auto& b) { return a.first < b.first; });
    std::vector<FmtNode> sorted;
    sorted.reserve(nodes.size());
    for (const auto& k : keyed) sorted.push_back(std::move(nodes[k.second]));
    nodes = std::move(sorted);
    for (auto& n : nodes) fmt_sort(n.children);
}

void fmt_emit(const std::vector<FmtNode>& nodes, size_t indent, std::string& out) {
    for (const auto& n : nodes) {
        out.append(indent, ' ');
        out.append(n.header);
        out.push_back('\n');
        if (!n.children.empty()) fmt_emit(n.children, indent + 2, out);
        for (const auto& li : n.list_items) {
            out.append(indent + 2, ' ');
            out.append(li);
            out.push_back('\n');
        }
        if (indent == 0 && (!n.children.empty() || !n.list_items.empty())) out.push_back('\n');
    }
}

} // namespace

std::string format(std::string_view text) {
    std::string_view clamped = clamp_synx_text(text);
    std::vector<std::string_view> lines = split_lines(clamped);

    std::vector<std::string_view> directives;
    size_t body_start = 0;
    for (size_t i = 0; i < lines.size(); ++i) {
        std::string_view t = trim_view(lines[i]);
        if (t == "!active" || t == "!lock" || t == "!tool" || t == "!schema" || t == "!llm"
            || t == "#!mode:active") {
            directives.push_back(t);
            body_start = i + 1;
        } else if (t.empty() || t[0] == '#' || starts_with(t, "//")) {
            body_start = i + 1;
        } else {
            break;
        }
    }

    std::vector<FmtNode> nodes;
    fmt_parse(lines, body_start, 0, 0, nodes);
    fmt_sort(nodes);

    std::string out;
    out.reserve(std::max<size_t>(std::min(clamped.size(), kMaxSynxInputBytes), 64));
    if (!directives.empty()) {
        for (size_t i = 0; i < directives.size(); ++i) {
            if (i > 0) out.push_back('\n');
            out.append(directives[i]);
        }
        out.append("\n\n");
    }
    fmt_emit(nodes, 0, out);
    // Trailing blank lines go; the text ends in exactly one newline.
    out.resize(detail::trim_end_view(out).size());
    out.push_back('\n');
    return out;
}

} // namespace synx
