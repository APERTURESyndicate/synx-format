// SYNX block writer — port of the §14 writer in crates/synx-core/src/synxl.rs
// (write_synx_entry and its helpers). Written so that parsing reads back the
// same data: strings that would cast or be cut as a comment are quoted or
// carried as `|+`, list objects lead with a key that fits the `- ` line.
#include "internal.hpp"
#include "numfmt.hpp"
#include "synx/parser.hpp"
#include "unicode.hpp"

#include <algorithm>
#include <cmath>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace synx {
namespace detail {

namespace {

constexpr size_t kMaxWriteDepth = kMaxParseNestingDepth;

void push_indent(std::string& out, size_t n) { out.append(n, ' '); }

bool is_quote_wrapped(std::string_view s) noexcept {
    return s.size() >= 2
        && ((s.front() == '"' && s.back() == '"') || (s.front() == '\'' && s.back() == '\''));
}

// Does `cast_value(s)` give back the string `s` itself?
bool auto_cast_keeps_string(std::string_view s) {
    if (is_quote_wrapped(s)) return true;
    if (s == "true" || s == "false" || s == "null") return false;
    size_t start = (!s.empty() && s[0] == '-') ? 1 : 0;
    if (s.size() <= start || !(s[start] >= '0' && s[start] <= '9')) return true;
    size_t dot = std::string_view::npos;
    for (size_t j = start; j < s.size(); ++j) {
        char c = s[j];
        if (c == '.') {
            if (dot != std::string_view::npos) return true;
            dot = j;
        } else if (c < '0' || c > '9') {
            return true;
        }
    }
    if (dot == std::string_view::npos) {
        int64_t n = 0;
        return !parse_i64(s, n);
    }
    double f = 0.0;
    return !(dot > start && dot < s.size() - 1 && parse_f64(s, f));
}

// A quote character that does not occur in `s`, or 0 if both do.
char pick_quote(std::string_view s) noexcept {
    if (s.find('"') == std::string_view::npos) return '"';
    if (s.find('\'') == std::string_view::npos) return '\'';
    return 0;
}

bool synx_value_needs_quote(std::string_view s) {
    return s.empty() || s.size() != trim_view(s).size() || s == "|" || s == "|+"
        || is_quote_wrapped(s) || !auto_cast_keeps_string(s);
}

// A float the SYNX cast reads back as the same float (§8.3 only knows
// `-?digits.digits`): Display, plus `.0` on a whole number. Non-finite: nothing.
void write_float(std::string& out, double f) {
    if (!std::isfinite(f)) return;
    std::string s = display_f64(f);
    out.append(s);
    if (s.find('.') == std::string::npos) out.append(".0");
}

// A scalar as one SYNX value token, or nullopt when no token can carry it. A
// value is comment-stripped at ` #` / ` //` before casting, so a value holding
// either cannot be rescued by quoting.
std::optional<std::string> synx_scalar(const Value& value) {
    switch (value.kind()) {
        case Value::Kind::Null: return std::string("null");
        case Value::Kind::Bool: return std::string(*value.as_bool() ? "true" : "false");
        case Value::Kind::Int: return i64_to_string(*value.as_int());
        case Value::Kind::Float: {
            double f = *value.as_float();
            if (!std::isfinite(f)) return std::string("null");
            std::string s;
            write_float(s, f);
            return s;
        }
        // safety: a secret is written as its mask, never as the plaintext.
        case Value::Kind::Secret: return std::string("[SECRET]");
        case Value::Kind::String: {
            const std::string& s = *value.as_string();
            if (s.find('\n') != std::string::npos || s.find(" #") != std::string::npos
                || s.find(" //") != std::string::npos) {
                return std::nullopt;
            }
            if (!synx_value_needs_quote(s)) return s;
            char q = pick_quote(s);
            if (q == 0) return std::nullopt;
            std::string out;
            out.reserve(s.size() + 2);
            out.push_back(q);
            out.append(s);
            out.push_back(q);
            return out;
        }
        case Value::Kind::Object:
        case Value::Kind::Array:
            return std::nullopt;
    }
    return std::nullopt;
}

std::string_view str_of(const Value& v) {
    if (const std::string* s = v.as_string()) return *s;
    if (const std::string* s = v.as_secret()) return *s;
    return {};
}

// May this value share the `- ` line of a list item?
bool dash_line_safe(const Value& v) {
    switch (v.kind()) {
        case Value::Kind::Object: return v.as_object()->empty();
        case Value::Kind::Array: return false;
        case Value::Kind::String: {
            const std::string& s = *v.as_string();
            return s.find('\n') == std::string::npos && synx_scalar(v).has_value() && !s.empty();
        }
        default: return true;
    }
}

std::vector<const Pair*> sorted_entries(const Object& map) {
    std::vector<const Pair*> keys;
    keys.reserve(map.size());
    for (const auto& p : map) keys.push_back(&p);
    std::sort(keys.begin(), keys.end(), [](const Pair* a, const Pair* b) { return a->key < b->key; });
    return keys;
}

// `key |+` plus an indented body (§14.3).
void write_synx_multiline(std::string& out, std::string_view key, std::string_view body, size_t indent) {
    push_indent(out, indent);
    out.append(key);
    out.append(" |+\n");
    size_t start = 0;
    while (true) {
        size_t nl = body.find('\n', start);
        push_indent(out, indent + 2);
        out.append(body.substr(start, (nl == std::string_view::npos ? body.size() : nl) - start));
        out.push_back('\n');
        if (nl == std::string_view::npos) break;
        start = nl + 1;
    }
}

void write_synx_item(std::string& out, const Value& item, size_t indent, size_t depth) {
    if (depth > kMaxWriteDepth) return;
    if (const Object* map = item.as_object()) {
        std::vector<const Pair*> keys = sorted_entries(*map);
        // An empty object in a list has no key for the `- ` line: a bare dash.
        if (keys.empty()) {
            push_indent(out, indent);
            out.append("-\n");
            return;
        }
        // The `- ` line carries a key whose value fits on it; everything deeper
        // belongs to the item. An item is an object only when a deeper line
        // follows, so a one-key item, or one with no head value that fits,
        // writes its head again in full below (the last one wins).
        size_t head = keys.size();
        for (size_t i = 0; i < keys.size(); ++i) {
            if (dash_line_safe(keys[i]->value)) { head = i; break; }
        }
        const bool repeat_head = head == keys.size() || keys.size() == 1;
        if (head == keys.size()) head = 0;
        push_indent(out, indent);
        out.append("- ");
        out.append(keys[head]->key);
        std::optional<std::string> text = synx_scalar(keys[head]->value);
        if (text && !text->empty()) {
            out.push_back(' ');
            out.append(*text);
        }
        out.push_back('\n');
        for (size_t i = 0; i < keys.size(); ++i) {
            if (i == head && !repeat_head) continue;
            write_synx_entry(out, keys[i]->key, keys[i]->value, indent + 2, depth + 1);
        }
        return;
    }
    push_indent(out, indent);
    out.append("- ");
    std::optional<std::string> text = synx_scalar(item);
    if (text) out.append(*text);
    else out.append(str_of(item));
    out.push_back('\n');
}

} // namespace

void write_synx_entry(std::string& out, std::string_view key, const Value& value,
                      size_t indent, size_t depth) {
    if (depth > kMaxWriteDepth) return;
    if (const Object* map = value.as_object()) {
        push_indent(out, indent);
        out.append(key);
        out.push_back('\n');
        for (const Pair* p : sorted_entries(*map)) {
            write_synx_entry(out, p->key, p->value, indent + 2, depth + 1);
        }
        return;
    }
    if (const Array* items = value.as_array()) {
        push_indent(out, indent);
        out.append(key);
        // A bare key reads back as `{}`; `:join` is the SYNX surface of `[]`.
        if (items->empty()) out.append(":join");
        out.push_back('\n');
        for (const Value& item : *items) write_synx_item(out, item, indent + 2, depth + 1);
        return;
    }
    if (const std::string* s = value.as_string(); s && s->find('\n') != std::string::npos) {
        write_synx_multiline(out, key, *s, indent);
        return;
    }
    std::optional<std::string> text = synx_scalar(value);
    if (text) {
        push_indent(out, indent);
        out.append(key);
        out.push_back(' ');
        out.append(*text);
        out.push_back('\n');
    } else {
        // No single token carries it: an indent-preserving block, never
        // comment-stripped nor cast.
        write_synx_multiline(out, key, str_of(value), indent);
    }
}

} // namespace detail
} // namespace synx
