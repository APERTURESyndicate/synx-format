// SYNX parser — converts raw text into a value tree with metadata.
// Mirrors crates/synx-core/src/parser.rs line-for-line where practical.
#include "synx/parser.hpp"

#include <algorithm>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <optional>
#include <random>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace synx {

namespace {

// ─── small utility helpers ───────────────────────────────────────────────────

inline bool is_space(char c) noexcept { return c == ' ' || c == '\t'; }

inline std::string_view trim_view(std::string_view s) noexcept {
    size_t start = 0;
    while (start < s.size() && (s[start] == ' ' || s[start] == '\t' || s[start] == '\r')) {
        ++start;
    }
    size_t end = s.size();
    while (end > start && (s[end - 1] == ' ' || s[end - 1] == '\t' || s[end - 1] == '\r')) {
        --end;
    }
    return s.substr(start, end - start);
}

inline std::string_view trim_end_view(std::string_view s) noexcept {
    size_t end = s.size();
    while (end > 0 && (s[end - 1] == ' ' || s[end - 1] == '\t' || s[end - 1] == '\r')) {
        --end;
    }
    return s.substr(0, end);
}

inline bool starts_with(std::string_view s, std::string_view prefix) noexcept {
    return s.size() >= prefix.size() && std::memcmp(s.data(), prefix.data(), prefix.size()) == 0;
}

// Same indent-count semantics as Rust `(raw.len() - raw.trim_start().len()) as i32`.
inline int indent_of(std::string_view raw) noexcept {
    size_t pos = 0;
    while (pos < raw.size() && (raw[pos] == ' ' || raw[pos] == '\t')) {
        ++pos;
    }
    return static_cast<int>(pos);
}

// Strip trailing ` //` or ` #` inline comments. Same behaviour as parser.rs `strip_comment`.
std::string strip_comment(std::string_view val) {
    std::string r(val);
    auto p = r.find(" //");
    if (p != std::string::npos) {
        r.resize(p);
    }
    p = r.find(" #");
    if (p != std::string::npos) {
        r.resize(p);
    }
    while (!r.empty() && (r.back() == ' ' || r.back() == '\t' || r.back() == '\r')) {
        r.pop_back();
    }
    return r;
}

// Strict int parser — no leading zeros stripping, no exponent (matches Rust `i64::parse`).
bool parse_int(std::string_view s, int64_t& out) noexcept {
    if (s.empty()) {
        return false;
    }
    size_t i = 0;
    bool neg = false;
    if (s[0] == '-') {
        neg = true;
        i = 1;
        if (i >= s.size()) {
            return false;
        }
    }
    int64_t v = 0;
    for (; i < s.size(); ++i) {
        char c = s[i];
        if (c < '0' || c > '9') {
            return false;
        }
        int d = c - '0';
        // Overflow check (best-effort, no exceptions)
        if (v > (INT64_MAX - d) / 10) {
            return false;
        }
        v = v * 10 + d;
    }
    out = neg ? -v : v;
    return true;
}

// Strict-ish float parser — accepts `[-]?\d+(\.\d+)?` (matches the `all_numeric` Rust path).
bool parse_float(std::string_view s, double& out) noexcept {
    if (s.empty()) {
        return false;
    }
    std::string tmp(s);
    char* end = nullptr;
    double v = std::strtod(tmp.c_str(), &end);
    if (end == nullptr || end == tmp.c_str() || *end != '\0') {
        return false;
    }
    out = v;
    return true;
}

// ─── value casting (matches `cast` and `cast_typed` in parser.rs) ───────────

Value cast_value(std::string_view val);

// Returns `nullopt` when val is neither quoted nor a recognised keyword,
// so the caller can fall through to numeric / string fallback.
std::optional<Value> cast_quoted_or_keyword(std::string_view val) {
    if (val.size() >= 2) {
        char first = val.front();
        char last = val.back();
        if ((first == '"' && last == '"') || (first == '\'' && last == '\'')) {
            return Value::make_string(std::string(val.substr(1, val.size() - 2)));
        }
    }
    if (val == "true") return Value::make_bool(true);
    if (val == "false") return Value::make_bool(false);
    if (val == "null") return Value::make_null();
    return std::nullopt;
}

Value cast_value(std::string_view val) {
    if (auto early = cast_quoted_or_keyword(val); early.has_value()) {
        return std::move(*early);
    }

    size_t len = val.size();
    if (len == 0) {
        return Value::make_string("");
    }
    size_t start = 0;
    if (val[0] == '-') {
        if (len == 1) {
            return Value::make_string(std::string(val));
        }
        start = 1;
    }
    char c0 = val[start];
    if (c0 < '0' || c0 > '9') {
        return Value::make_string(std::string(val));
    }

    bool seen_dot = false;
    bool all_numeric = true;
    size_t dot_pos = std::string_view::npos;
    for (size_t j = start; j < len; ++j) {
        char c = val[j];
        if (c == '.') {
            if (seen_dot) {
                all_numeric = false;
                break;
            }
            seen_dot = true;
            dot_pos = j;
        } else if (c < '0' || c > '9') {
            all_numeric = false;
            break;
        }
    }
    if (!all_numeric) {
        return Value::make_string(std::string(val));
    }
    if (seen_dot) {
        if (dot_pos > start && dot_pos < len - 1) {
            double f = 0.0;
            if (parse_float(val, f)) {
                return Value::make_float(f);
            }
        }
        return Value::make_string(std::string(val));
    }
    int64_t n = 0;
    if (parse_int(val, n)) {
        return Value::make_int(n);
    }
    return Value::make_string(std::string(val));
}

// Process-wide RNG for `(random)` type hints. Seeded once via random_device.
std::mt19937_64& rng_global() noexcept {
    static std::mt19937_64 r{std::random_device{}()};
    return r;
}

Value cast_typed(std::string_view val, std::string_view hint) {
    if (hint == "int") {
        int64_t n = 0;
        return Value::make_int(parse_int(val, n) ? n : 0);
    }
    if (hint == "float") {
        double f = 0.0;
        return Value::make_float(parse_float(val, f) ? f : 0.0);
    }
    if (hint == "bool") {
        std::string_view t = trim_view(val);
        return Value::make_bool(t == "true");
    }
    if (hint == "string") {
        return Value::make_string(std::string(val));
    }
    if (hint == "random" || hint == "random:int") {
        std::uniform_int_distribution<int64_t> dist{0, INT64_MAX};
        return Value::make_int(dist(rng_global()));
    }
    if (hint == "random:float") {
        std::uniform_real_distribution<double> dist{0.0, 1.0};
        return Value::make_float(dist(rng_global()));
    }
    if (hint == "random:bool") {
        std::uniform_int_distribution<int> dist{0, 1};
        return Value::make_bool(dist(rng_global()) != 0);
    }
    return cast_value(val);
}

// ─── constraints parser ─────────────────────────────────────────────────────

Constraints parse_constraints(std::string_view raw) {
    Constraints c;
    size_t i = 0;
    while (i < raw.size()) {
        size_t comma = raw.find(',', i);
        std::string_view part = trim_view(raw.substr(i, (comma == std::string_view::npos ? raw.size() : comma) - i));
        i = (comma == std::string_view::npos ? raw.size() : comma + 1);
        if (part.empty()) {
            continue;
        }
        if (part == "required") {
            c.required = true;
        } else if (part == "readonly") {
            c.readonly = true;
        } else {
            size_t colon = part.find(':');
            if (colon == std::string_view::npos) {
                continue;
            }
            std::string_view k = trim_view(part.substr(0, colon));
            std::string_view v = trim_view(part.substr(colon + 1));
            if (k == "min") {
                double d = 0.0;
                if (parse_float(v, d)) c.min = d;
            } else if (k == "max") {
                double d = 0.0;
                if (parse_float(v, d)) c.max = d;
            } else if (k == "type") {
                c.type_name = std::string(v);
            } else if (k == "pattern") {
                c.pattern = std::string(v);
            } else if (k == "enum") {
                std::vector<std::string> vals;
                size_t p = 0;
                size_t count = 0;
                // `p <= v.size()` matches Rust `str::split('|')`: "" → [""], "a|" → ["a", ""].
                while (p <= v.size() && count < kMaxConstraintEnumParts) {
                    size_t pipe = v.find('|', p);
                    std::string_view seg = v.substr(p, (pipe == std::string_view::npos ? v.size() : pipe) - p);
                    vals.emplace_back(seg);
                    if (pipe == std::string_view::npos) break;
                    p = pipe + 1;
                    ++count;
                }
                c.enum_values = std::move(vals);
            }
        }
    }
    return c;
}

// ─── one key-line decomposition ─────────────────────────────────────────────

struct ParsedLine {
    std::string key;
    std::optional<std::string> type_hint;
    std::string value;
    std::vector<std::string> markers;
    std::vector<std::string> marker_args;
    std::optional<Constraints> constraints;
};

std::optional<ParsedLine> parse_line(std::string_view trimmed) {
    if (trimmed.empty()
        || trimmed[0] == '#'
        || starts_with(trimmed, "//")
        || starts_with(trimmed, "- ")) {
        return std::nullopt;
    }

    char first = trimmed[0];
    if (first == '[' || first == ':' || first == '-'
        || first == '#' || first == '/' || first == '(') {
        return std::nullopt;
    }

    size_t len = trimmed.size();
    size_t pos = 0;
    while (pos < len) {
        char ch = trimmed[pos];
        if (ch == ' ' || ch == '\t' || ch == '[' || ch == ':' || ch == '(') {
            break;
        }
        ++pos;
    }

    ParsedLine out;
    out.key = std::string(trimmed.substr(0, pos));

    // Optional `(type)`
    if (pos < len && trimmed[pos] == '(') {
        size_t start = pos + 1;
        size_t rel = trimmed.substr(start).find(')');
        if (rel != std::string_view::npos) {
            out.type_hint = std::string(trimmed.substr(start, rel));
            pos = start + rel + 1;
        } else {
            pos = start;
        }
    }

    // Optional `[constraints]` — balanced bracket scan
    if (pos < len && trimmed[pos] == '[') {
        size_t cstart = pos + 1;
        size_t depth = 1;
        size_t scan = cstart;
        while (scan < len && depth > 0) {
            char b = trimmed[scan];
            if (b == '[') ++depth;
            else if (b == ']') {
                --depth;
                if (depth == 0) break;
            }
            ++scan;
        }
        if (depth == 0) {
            out.constraints = parse_constraints(trimmed.substr(cstart, scan - cstart));
            pos = scan + 1;
        } else {
            // Unbalanced — fall back to first `]`
            size_t rel = trimmed.substr(cstart).find(']');
            if (rel != std::string_view::npos) {
                out.constraints = parse_constraints(trimmed.substr(cstart, rel));
                pos = cstart + rel + 1;
            } else {
                out.constraints = parse_constraints(trimmed.substr(cstart));
                pos = len;
            }
        }
    }

    // Optional `:markers`
    if (pos < len && trimmed[pos] == ':') {
        size_t mstart = pos + 1;
        size_t mend = mstart;
        while (mend < len && trimmed[mend] != ' ' && trimmed[mend] != '\t') {
            ++mend;
        }
        std::string_view chain = trimmed.substr(mstart, mend - mstart);
        size_t p = 0;
        size_t segs = 0;
        while (p <= chain.size() && segs < kMaxMarkerChainSegments) {
            size_t colon = chain.find(':', p);
            std::string_view seg = chain.substr(p, (colon == std::string_view::npos ? chain.size() : colon) - p);
            out.markers.emplace_back(seg);
            ++segs;
            if (colon == std::string_view::npos) break;
            p = colon + 1;
        }
        pos = mend;
    }

    // Skip whitespace
    while (pos < len && (trimmed[pos] == ' ' || trimmed[pos] == '\t')) {
        ++pos;
    }

    out.value = (pos < len) ? strip_comment(trimmed.substr(pos)) : std::string{};

    // :random — extract weight percentages from value into args
    bool has_random = false;
    bool has_inherit = false;
    for (const auto& m : out.markers) {
        if (m == "random") has_random = true;
        if (m == "inherit") has_inherit = true;
    }
    if (has_random && !out.value.empty()) {
        std::vector<std::string> nums;
        std::string_view v(out.value);
        size_t p = 0;
        while (p < v.size()) {
            while (p < v.size() && (v[p] == ' ' || v[p] == '\t')) ++p;
            size_t start = p;
            while (p < v.size() && v[p] != ' ' && v[p] != '\t') ++p;
            if (p > start) {
                std::string tok(v.substr(start, p - start));
                double d = 0.0;
                if (parse_float(tok, d)) {
                    nums.push_back(std::move(tok));
                }
            }
        }
        if (!nums.empty()) {
            out.marker_args = std::move(nums);
            out.value.clear();
        }
    }
    // :inherit — value names the parent; promote into marker_args and clear value.
    if (has_inherit && !out.value.empty()) {
        std::string_view t = trim_view(out.value);
        out.marker_args = {std::string(t)};
        out.value.clear();
    }

    return out;
}

// ─── stack / tree helpers ────────────────────────────────────────────────────

enum class StackEntryKind : uint8_t { Root, Key, ListItem };

struct StackEntry {
    StackEntryKind kind;
    std::string key;           // for Key and ListItem.list_key
    size_t item_idx = 0;       // for ListItem only
};

// Returns metadata dot-path from current stack.
std::string build_path(const std::vector<std::pair<int, StackEntry>>& stack) {
    std::string out;
    bool first = true;
    for (size_t i = 1; i < stack.size(); ++i) {
        if (stack[i].second.kind == StackEntryKind::Key) {
            if (!first) out.push_back('.');
            out += stack[i].second.key;
            first = false;
        }
    }
    return out;
}

// Walk the stack into the target parent. `parent` always carries kind == Object.
Object* navigate_to_parent(Object* root,
                           const std::vector<std::pair<int, StackEntry>>& stack,
                           size_t target_idx) {
    if (target_idx == 0) return root;
    Object* current = root;
    for (size_t i = 1; i <= target_idx && i < stack.size(); ++i) {
        const StackEntry& e = stack[i].second;
        switch (e.kind) {
            case StackEntryKind::Root:
                return nullptr; // unreachable in well-formed input
            case StackEntryKind::Key: {
                bool found = false;
                for (auto& p : *current) {
                    if (p.key == e.key) {
                        Object* child = p.value.as_object_mut();
                        if (!child) return nullptr;
                        current = child;
                        found = true;
                        break;
                    }
                }
                if (!found) return nullptr;
                break;
            }
            case StackEntryKind::ListItem: {
                bool found = false;
                for (auto& p : *current) {
                    if (p.key == e.key) {
                        Array* arr = p.value.as_array_mut();
                        if (!arr || e.item_idx >= arr->size()) return nullptr;
                        Object* inner = (*arr)[e.item_idx].as_object_mut();
                        if (!inner) return nullptr;
                        current = inner;
                        found = true;
                        break;
                    }
                }
                if (!found) return nullptr;
                break;
            }
        }
    }
    return current;
}

void insert_value(Object* root,
                  const std::vector<std::pair<int, StackEntry>>& stack,
                  size_t parent_idx,
                  const std::string& key,
                  Value v) {
    Object* parent = navigate_to_parent(root, stack, parent_idx);
    if (!parent) return;
    for (auto& p : *parent) {
        if (p.key == key) {
            p.value = std::move(v);
            return;
        }
    }
    parent->push_back(Pair{key, std::move(v)});
}

struct BlockState {
    int indent;
    std::string key;
    std::string content;
    size_t stack_idx;
};

struct ListState {
    int indent;
    std::string key;
    size_t stack_idx;
};

// Length of byte range bounded by line_starts (handling trailing \r\n).
inline std::string_view line_view(std::string_view text,
                                  const std::vector<size_t>& line_starts,
                                  size_t i,
                                  size_t total_bytes) noexcept {
    size_t s = line_starts[i];
    size_t e = (i + 1 < line_starts.size()) ? line_starts[i + 1] - 1 : total_bytes;
    if (e > s && e > 0 && text[e - 1] == '\r') {
        --e;
    }
    return text.substr(s, e - s);
}

} // namespace

// ─── public API ─────────────────────────────────────────────────────────────

std::string_view clamp_synx_text(std::string_view text) noexcept {
    if (text.size() <= kMaxSynxInputBytes) {
        return text;
    }
    // UTF-8 safe truncation: back off until the previous byte is not a continuation byte.
    size_t end = kMaxSynxInputBytes;
    while (end > 0 && (static_cast<unsigned char>(text[end]) & 0xC0) == 0x80) {
        --end;
    }
    return text.substr(0, end);
}

ParseResult parse(std::string_view text) {
    text = clamp_synx_text(text);

    // Bound number of newlines we index.
    {
        size_t max_nl = kMaxLineStarts > 0 ? kMaxLineStarts - 1 : 0;
        size_t seen = 0;
        size_t scan = 0;
        while (scan < text.size()) {
            const void* p = std::memchr(text.data() + scan, '\n', text.size() - scan);
            if (!p) break;
            if (seen >= max_nl) {
                size_t cut = static_cast<size_t>(static_cast<const char*>(p) - text.data());
                text = text.substr(0, cut);
                break;
            }
            ++seen;
            scan = static_cast<size_t>(static_cast<const char*>(p) - text.data()) + 1;
        }
    }

    const size_t total = text.size();
    std::vector<size_t> line_starts;
    line_starts.reserve(64);
    line_starts.push_back(0);
    {
        size_t scan = 0;
        while (scan < total) {
            const void* p = std::memchr(text.data() + scan, '\n', total - scan);
            if (!p) break;
            size_t pos = static_cast<size_t>(static_cast<const char*>(p) - text.data());
            line_starts.push_back(pos + 1);
            scan = pos + 1;
        }
    }
    const size_t line_count = line_starts.size();

    ParseResult result;
    result.root = Value::make_object();
    Object* root = result.root.as_object_mut();

    std::vector<std::pair<int, StackEntry>> stack;
    stack.reserve(16);
    stack.push_back({-1, StackEntry{StackEntryKind::Root, {}, 0}});

    std::optional<BlockState> block;
    std::optional<ListState> list;
    bool in_block_comment = false;

    size_t i = 0;
    while (i < line_count) {
        std::string_view raw = line_view(text, line_starts, i, total);
        std::string_view t = trim_view(raw);

        // Directives
        if (t == "!active") { result.mode = Mode::Active; ++i; continue; }
        if (t == "!lock")   { result.locked = true;     ++i; continue; }
        if (t == "!tool")   { result.tool = true;       ++i; continue; }
        if (t == "!schema") { result.schema = true;     ++i; continue; }
        if (t == "!llm")    { result.llm = true;        ++i; continue; }
        if (starts_with(t, "!include ")) {
            if (result.includes.size() < kMaxIncludeDirectives) {
                std::string_view rest = trim_view(t.substr(9));
                size_t ws = 0;
                while (ws < rest.size() && rest[ws] != ' ' && rest[ws] != '\t') ++ws;
                std::string path(rest.substr(0, ws));
                std::string alias;
                if (ws < rest.size()) {
                    alias = std::string(trim_view(rest.substr(ws)));
                }
                if (alias.empty() && !path.empty()) {
                    size_t slash = path.find_last_of("/\\");
                    std::string base = (slash == std::string::npos) ? path : path.substr(slash + 1);
                    auto strip_suffix = [&](const std::string& suf) -> bool {
                        if (base.size() >= suf.size()
                            && base.compare(base.size() - suf.size(), suf.size(), suf) == 0) {
                            base.resize(base.size() - suf.size());
                            return true;
                        }
                        return false;
                    };
                    if (!strip_suffix(".synx")) {
                        strip_suffix(".SYNX");
                    }
                    alias = std::move(base);
                }
                result.includes.push_back(IncludeDirective{std::move(path), std::move(alias)});
            }
            ++i; continue;
        }
        if (starts_with(t, "!use ")) {
            std::string_view rest = trim_view(t.substr(5));
            if (!rest.empty() && rest[0] == '@') {
                size_t as_pos = rest.find(" as ");
                std::string package, alias;
                if (as_pos == std::string_view::npos) {
                    package = std::string(trim_view(rest));
                } else {
                    package = std::string(trim_view(rest.substr(0, as_pos)));
                    alias = std::string(trim_view(rest.substr(as_pos + 4)));
                }
                if (alias.empty() && !package.empty()) {
                    size_t slash = package.find_last_of('/');
                    alias = (slash == std::string::npos) ? package : package.substr(slash + 1);
                }
                if (!package.empty()) {
                    result.uses.push_back(UseDirective{std::move(package), std::move(alias)});
                }
            }
            ++i; continue;
        }
        if (starts_with(t, "#!mode:")) {
            std::string_view declared = trim_view(t.substr(7));
            result.mode = (declared == "active") ? Mode::Active : Mode::Static;
            ++i; continue;
        }

        if (t == "###") { in_block_comment = !in_block_comment; ++i; continue; }
        if (in_block_comment) { ++i; continue; }

        if (t.empty() || t[0] == '#' || starts_with(t, "//")) { ++i; continue; }

        int indent = indent_of(raw);

        // Continue multiline block
        if (block.has_value()) {
            if (indent > block->indent) {
                if (block->content.size() < kMaxMultilineBlockBytes) {
                    if (!block->content.empty()) {
                        block->content.push_back('\n');
                    }
                    size_t room = kMaxMultilineBlockBytes - block->content.size();
                    size_t n = std::min(t.size(), room);
                    block->content.append(t.data(), n);
                }
                ++i; continue;
            }
            insert_value(root, stack, block->stack_idx, block->key,
                         Value::make_string(std::move(block->content)));
            block.reset();
        }

        // Continue list items
        if (starts_with(t, "- ")) {
            if (list.has_value() && indent > list->indent) {
                // Pop list-item frames at same/deeper indent
                while (stack.size() > 1) {
                    const auto& back = stack.back();
                    if (back.second.kind == StackEntryKind::ListItem && back.first >= indent) {
                        stack.pop_back();
                    } else {
                        break;
                    }
                }

                std::string val_str = strip_comment(trim_view(t.substr(2)));

                // Peek next non-empty line to detect nested object form
                size_t peek = i + 1;
                bool nested = false;
                while (peek < line_count) {
                    std::string_view pl = line_view(text, line_starts, peek, total);
                    std::string_view pt = trim_view(pl);
                    if (pt.empty()) { ++peek; continue; }
                    int pi = indent_of(pl);
                    if (pi > indent && !starts_with(pt, "- ") && pt[0] != '#' && !starts_with(pt, "//")) {
                        nested = true;
                    }
                    break;
                }

                std::string list_key = list->key;
                size_t list_stack_idx = list->stack_idx;

                Object* parent = navigate_to_parent(root, stack, list_stack_idx);
                if (parent) {
                    // Find-or-create the array
                    Value* arr_val = nullptr;
                    for (auto& p : *parent) {
                        if (p.key == list_key) { arr_val = &p.value; break; }
                    }
                    if (!arr_val) {
                        parent->push_back(Pair{list_key, Value::make_array()});
                        arr_val = &parent->back().value;
                    }
                    Array* arr = arr_val->as_array_mut();
                    if (arr && arr->size() < kMaxListItems) {
                        if (nested) {
                            Object item_obj;
                            std::optional<ParsedLine> parsed = parse_line(val_str);
                            if (parsed.has_value()) {
                                Value v = parsed->type_hint.has_value()
                                    ? cast_typed(parsed->value, *parsed->type_hint)
                                    : (parsed->value.empty()
                                        ? Value::make_object()
                                        : cast_value(parsed->value));
                                item_obj.push_back(Pair{parsed->key, std::move(v)});
                            } else {
                                item_obj.push_back(Pair{"_value", cast_value(val_str)});
                            }
                            size_t item_idx = arr->size();
                            arr->push_back(Value::make_object(std::move(item_obj)));
                            if (stack.size() < kMaxParseNestingDepth) {
                                stack.push_back({indent,
                                    StackEntry{StackEntryKind::ListItem, std::move(list_key), item_idx}});
                            }
                        } else {
                            arr->push_back(cast_value(val_str));
                        }
                    }
                }
                ++i; continue;
            }
        } else if (list.has_value() && indent <= list->indent) {
            list.reset();
            while (stack.size() > 1) {
                const auto& back = stack.back();
                if (back.second.kind == StackEntryKind::ListItem && back.first >= indent) {
                    stack.pop_back();
                } else {
                    break;
                }
            }
        }

        // Key line
        std::optional<ParsedLine> parsed = parse_line(t);
        if (parsed.has_value()) {
            ParsedLine& p = *parsed;
            if (p.key == "__proto__" || p.key == "constructor" || p.key == "prototype") {
                ++i; continue;
            }

            // Pop stack to correct parent
            while (stack.size() > 1 && stack.back().first >= indent) {
                stack.pop_back();
            }
            size_t parent_idx = stack.size() - 1;

            if (result.mode == Mode::Active
                && (!p.markers.empty() || p.constraints.has_value() || p.type_hint.has_value())) {
                std::string path = build_path(stack);
                Meta m;
                m.markers = p.markers;
                m.args = p.marker_args;
                m.type_hint = p.type_hint;
                m.constraints = p.constraints;
                result.metadata[path][p.key] = std::move(m);
            }

            bool is_block = (p.value == "|");
            bool is_list_marker = false;
            for (const auto& m : p.markers) {
                if (m == "random" || m == "unique" || m == "geo" || m == "join") {
                    is_list_marker = true;
                    break;
                }
            }

            if (is_block) {
                insert_value(root, stack, parent_idx, p.key, Value::make_string(""));
                block = BlockState{indent, p.key, {}, parent_idx};
            } else if (is_list_marker && p.value.empty()) {
                insert_value(root, stack, parent_idx, p.key, Value::make_array());
                list = ListState{indent, p.key, parent_idx};
            } else if (p.value.empty()) {
                // Peek for upcoming list
                size_t peek = i + 1;
                while (peek < line_count) {
                    std::string_view pl = line_view(text, line_starts, peek, total);
                    std::string_view pt = trim_view(pl);
                    if (!pt.empty()) {
                        if (starts_with(pt, "- ")) {
                            insert_value(root, stack, parent_idx, p.key, Value::make_array());
                            list = ListState{indent, p.key, parent_idx};
                            goto consumed_line; // 1-level break (over inner while loop only)
                        }
                        break;
                    }
                    ++peek;
                }
                insert_value(root, stack, parent_idx, p.key, Value::make_object());
                if (stack.size() < kMaxParseNestingDepth) {
                    stack.push_back({indent, StackEntry{StackEntryKind::Key, p.key, 0}});
                }
            } else {
                Value v = p.type_hint.has_value()
                    ? cast_typed(p.value, *p.type_hint)
                    : cast_value(p.value);
                insert_value(root, stack, parent_idx, p.key, std::move(v));
            }
        }
    consumed_line:
        ++i;
    }

    // Flush pending block
    if (block.has_value()) {
        insert_value(root, stack, block->stack_idx, block->key,
                     Value::make_string(std::move(block->content)));
    }

    return result;
}

Value reshape_tool_output(const Value& root, bool schema) {
    const Object* obj = root.as_object();
    if (!obj) {
        return root;
    }

    if (schema) {
        // Sort by key for deterministic output
        std::vector<const Pair*> entries;
        entries.reserve(obj->size());
        for (const auto& p : *obj) entries.push_back(&p);
        std::sort(entries.begin(), entries.end(),
                  [](const Pair* a, const Pair* b) { return a->key < b->key; });

        Array tools;
        tools.reserve(entries.size());
        for (const Pair* p : entries) {
            Object def;
            def.push_back(Pair{"name", Value::make_string(p->key)});
            def.push_back(Pair{"params", p->value});
            tools.push_back(Value::make_object(std::move(def)));
        }
        Object out;
        out.push_back(Pair{"tools", Value::make_array(std::move(tools))});
        return Value::make_object(std::move(out));
    }

    if (obj->empty()) {
        Object out;
        out.push_back(Pair{"tool", Value::make_null()});
        out.push_back(Pair{"params", Value::make_object()});
        return Value::make_object(std::move(out));
    }

    // First key in sorted order — matches Rust deterministic pick.
    std::vector<const Pair*> entries;
    entries.reserve(obj->size());
    for (const auto& p : *obj) entries.push_back(&p);
    std::sort(entries.begin(), entries.end(),
              [](const Pair* a, const Pair* b) { return a->key < b->key; });

    const Pair* first = entries.front();
    Value params = first->value.is_object() ? first->value : Value::make_object();
    Object out;
    out.push_back(Pair{"tool", Value::make_string(first->key)});
    out.push_back(Pair{"params", std::move(params)});
    return Value::make_object(std::move(out));
}

} // namespace synx
