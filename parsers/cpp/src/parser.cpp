// SYNX parser — converts raw text into a value tree with metadata.
// Port of crates/synx-core/src/parser.rs.
#include "synx/parser.hpp"

#include "internal.hpp"
#include "synx/key_index.hpp"
#include "numfmt.hpp"
#include "paths.hpp"
#include "unicode.hpp"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace synx {

namespace detail {

Value cast_value(std::string_view val) {
    // Quoted strings keep their literal text ("null", "true", "123" stay strings).
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
    const size_t len = val.size();
    if (len == 0) return Value::make_string(std::string());
    size_t start = 0;
    if (val[0] == '-') {
        if (len == 1) return Value::make_string(std::string(val));
        start = 1;
    }
    if (val[start] >= '0' && val[start] <= '9') {
        size_t dot = std::string_view::npos;
        bool all_numeric = true;
        for (size_t j = start; j < len; ++j) {
            char c = val[j];
            if (c == '.') {
                if (dot != std::string_view::npos) { all_numeric = false; break; }
                dot = j;
            } else if (c < '0' || c > '9') {
                all_numeric = false;
                break;
            }
        }
        if (all_numeric) {
            if (dot != std::string_view::npos) {
                double f = 0.0;
                if (dot > start && dot < len - 1 && parse_f64(val, f)) return Value::make_float(f);
            } else {
                int64_t n = 0;
                if (parse_i64(val, n)) return Value::make_int(n);
            }
        }
    }
    return Value::make_string(std::string(val));
}

Value cast_typed(std::string_view val, std::string_view hint) {
    if (hint == "int") {
        int64_t n = 0;
        return Value::make_int(parse_i64(val, n) ? n : 0);
    }
    if (hint == "float") {
        double f = 0.0;
        return Value::make_float(parse_f64(val, f) ? f : 0.0);
    }
    if (hint == "bool") return Value::make_bool(trim_view(val) == "true");
    if (hint == "string") return Value::make_string(std::string(val));
    if (hint == "random" || hint == "random:int") return Value::make_int(random_i64());
    if (hint == "random:float") return Value::make_float(random_f64_01());
    if (hint == "random:bool") return Value::make_bool(random_bool());
    return cast_value(val);
}

Constraints parse_constraints(std::string_view raw) {
    Constraints c;
    size_t i = 0;
    while (i <= raw.size()) {
        size_t comma = raw.find(',', i);
        size_t end = comma == std::string_view::npos ? raw.size() : comma;
        std::string_view part = trim_view(raw.substr(i, end - i));
        i = end + 1;
        if (part.empty()) continue;
        if (part == "required") {
            c.required = true;
        } else if (part == "readonly") {
            c.readonly = true;
        } else {
            size_t colon = part.find(':');
            if (colon == std::string_view::npos) continue;
            std::string_view k = trim_view(part.substr(0, colon));
            std::string_view v = trim_view(part.substr(colon + 1));
            double d = 0.0;
            // A bound is a finite number: `inf` and `nan` bound nothing sensible
            // (`[max:-inf]` rejected every value).
            if (k == "min") {
                c.min = parse_f64(v, d) && std::isfinite(d) ? std::optional<double>(d) : std::nullopt;
            } else if (k == "max") {
                c.max = parse_f64(v, d) && std::isfinite(d) ? std::optional<double>(d) : std::nullopt;
            } else if (k == "type") {
                c.type_name = std::string(v);
            } else if (k == "pattern") {
                c.pattern = std::string(v);
            } else if (k == "enum") {
                std::vector<std::string> vals;
                size_t p = 0;
                while (vals.size() < kMaxConstraintEnumParts) {
                    size_t pipe = v.find('|', p);
                    size_t e = pipe == std::string_view::npos ? v.size() : pipe;
                    vals.emplace_back(v.substr(p, e - p));
                    if (pipe == std::string_view::npos) break;
                    p = pipe + 1;
                }
                c.enum_values = std::move(vals);
            }
        }
    }
    return c;
}

std::string_view strip_comment(std::string_view val) noexcept {
    std::string_view r = val;
    if (r.find(' ') != std::string_view::npos) {
        size_t p = r.find(" //");
        if (p != std::string_view::npos) r = r.substr(0, p);
        p = r.find(" #");
        if (p != std::string_view::npos) r = r.substr(0, p);
    }
    return trim_end_view(r);
}

} // namespace detail

namespace {

using detail::cast_typed;
using detail::cast_value;
using detail::indent_len;
using detail::KeyIndex;
using detail::parse_constraints;
using detail::parse_f64;
using detail::push_item_segment;
using detail::push_key_segment;
using detail::split_whitespace;
using detail::starts_with;
using detail::strip_boundary;
using detail::strip_comment;
using detail::trim_end_view;
using detail::trim_view;
using detail::utf8_len;
using detail::utf8_prefix_len;
using detail::ws_len_at;

/// Keys that would reach a JS consumer's prototype chain instead of its data.
inline bool is_reserved_js_key(std::string_view key) noexcept {
    return key == "__proto__" || key == "constructor" || key == "prototype";
}

/// `digits[.digits]` — a `:random` weight.
bool is_weight(std::string_view s) noexcept {
    auto digits = [](std::string_view t) {
        if (t.empty()) return false;
        for (char ch : t) {
            if (ch < '0' || ch > '9') return false;
        }
        return true;
    };
    size_t dot = s.find('.');
    return dot == std::string_view::npos ? digits(s) : digits(s.substr(0, dot)) && digits(s.substr(dot + 1));
}

// ─── one key line, borrowed from the source text ────────────────────────────

struct ParsedLine {
    std::string_view key;
    std::optional<std::string_view> type_hint;
    std::string_view value;
    std::vector<std::string_view> markers;
    std::vector<std::string> marker_args;
    std::optional<Constraints> constraints;
};

bool parse_line(std::string_view t, ParsedLine& out) {
    if (t.empty() || t[0] == '#' || starts_with(t, "//") || starts_with(t, "- ")) return false;
    char first = t[0];
    if (first == '[' || first == ':' || first == '-' || first == '#' || first == '/' || first == '(') {
        return false;
    }
    const size_t len = t.size();
    size_t pos = 0;
    while (pos < len) {
        char ch = t[pos];
        if (ch == ' ' || ch == '\t' || ch == '[' || ch == ':' || ch == '(') break;
        ++pos;
    }
    out.key = t.substr(0, pos);
    out.type_hint.reset();
    out.markers.clear();
    out.marker_args.clear();
    out.constraints.reset();

    // Optional `(type)`.
    if (pos < len && t[pos] == '(') {
        size_t start = pos + 1;
        size_t close = t.find(')', start);
        if (close != std::string_view::npos) {
            out.type_hint = t.substr(start, close - start);
            pos = close + 1;
        } else {
            pos += 1;
        }
    }

    // Optional `[constraints]` — balanced scan, so `^[A-Z]{2}$` keeps its own `]`.
    if (pos < len && t[pos] == '[') {
        size_t cstart = pos + 1;
        size_t depth = 1;
        size_t scan = cstart;
        while (scan < len && depth > 0) {
            if (t[scan] == '[') {
                ++depth;
            } else if (t[scan] == ']') {
                --depth;
                if (depth == 0) break;
            }
            ++scan;
        }
        if (depth == 0) {
            out.constraints = parse_constraints(t.substr(cstart, scan - cstart));
            pos = scan + 1;
        } else {
            size_t rel = t.find(']', cstart);
            if (rel != std::string_view::npos) {
                out.constraints = parse_constraints(t.substr(cstart, rel - cstart));
                pos = rel + 1;
            } else {
                out.constraints = parse_constraints(t.substr(cstart));
                pos = len;
            }
        }
    }

    // Optional `:markers`.
    if (pos < len && t[pos] == ':') {
        size_t mstart = pos + 1;
        size_t mend = mstart;
        while (mend < len && t[mend] != ' ' && t[mend] != '\t') ++mend;
        std::string_view chain = t.substr(mstart, mend - mstart);
        size_t p = 0;
        while (out.markers.size() < kMaxMarkerChainSegments) {
            size_t colon = chain.find(':', p);
            size_t e = colon == std::string_view::npos ? chain.size() : colon;
            out.markers.push_back(chain.substr(p, e - p));
            if (colon == std::string_view::npos) break;
            p = colon + 1;
        }
        pos = mend;
    }

    while (pos < len && (t[pos] == ' ' || t[pos] == '\t')) ++pos;
    out.value = pos < len ? strip_comment(t.substr(pos)) : std::string_view();

    if (!out.markers.empty() && !out.value.empty()) {
        auto has = [&](std::string_view name) {
            return std::find(out.markers.begin(), out.markers.end(), name) != out.markers.end();
        };
        // `:random` — the value carries the weight percentages.
        if (has("random")) {
            std::vector<std::string> nums;
            // spec: a weight is `digits[.digits]`. Anything a float parser read
            // counted, so `a:random inf`, `-5` or `nan` lost its value.
            split_whitespace(out.value, [&](std::string_view tok) {
                if (is_weight(tok)) nums.emplace_back(tok);
            });
            if (!nums.empty()) {
                out.marker_args = std::move(nums);
                out.value = std::string_view();
            }
        }
        // `:inherit` — a non-empty value names the parent: the line opens a group.
        if (!out.value.empty() && has("inherit")) {
            out.marker_args = {std::string(trim_view(out.value))};
            out.value = std::string_view();
        }
    }
    return true;
}

// ─── stack / tree ────────────────────────────────────────────────────────────

enum class FrameKind : uint8_t { Root, Key, ListItem };

struct Frame {
    int indent;
    FrameKind kind;
    std::string_view key;  // Key: its key; ListItem: the list's key.
    size_t item_idx = 0;   // ListItem only.
    size_t parent = 0;     // ListItem only: stack index of the frame holding the list.
    KeyIndex index;        // over the frame's own object, once it outgrows a scan
};

constexpr size_t kIndexThreshold = 16;

/// Position of `key` in `obj`, or npos.
size_t find_key(Object& obj, KeyIndex& idx, std::string_view key) {
    if (obj.size() < kIndexThreshold) {
        for (size_t i = 0; i < obj.size(); ++i) {
            if (obj[i].key == key) return i;
        }
        return std::string_view::npos;
    }
    auto key_of = [](const Pair& p) { return std::string_view(p.key); };
    idx.sync(obj, key_of);
    size_t at = idx.find(obj, key, key_of);
    return at == KeyIndex::npos ? std::string_view::npos : at;
}

struct BlockState {
    int indent;
    std::string_view key;
    std::string content;
    size_t stack_idx;
    // `|+` keeps each line's indent relative to the first non-empty one.
    bool preserve_indent;
    int base_indent;  // -1 until the first content line locks it.
};

struct ListState {
    int indent;
    std::string_view key;
    size_t stack_idx;
};

class Parser {
public:
    Parser(std::string_view text, ParseResult& result) : text_(text), r_(result) {
        root_ = r_.root.as_object_mut();
        stack_.reserve(16);
        stack_.push_back(Frame{-1, FrameKind::Root, {}, 0, 0, {}});
    }

    void run();

private:
    // The frames from the root to `target` (root excluded), in descending order.
    // A ListItem's parent is the frame that holds its list, which need not be the
    // frame below it: an object opened between a list key and its first item
    // sits in between without being an ancestor.
    size_t frame_chain(size_t target, size_t* chain) const {
        size_t n = 0;
        size_t idx = target;
        while (idx > 0 && n <= kMaxParseNestingDepth) {
            chain[n++] = idx;
            idx = stack_[idx].kind == FrameKind::ListItem ? stack_[idx].parent : idx - 1;
        }
        std::reverse(chain, chain + n);
        return n;
    }

    // The cache holds one parent. Navigating to another one — the root included —
    // replaces it, which is what keeps it sound: pushing onto a list moves that
    // list's items, and the list's parent is navigated first.
    Object* navigate(size_t target) {
        if (cache_idx_ == target) return cache_obj_;
        if (target == 0) {
            cache_idx_ = 0;
            cache_obj_ = root_;
            return root_;
        }
        size_t chain[kMaxParseNestingDepth + 2];
        size_t n = frame_chain(target, chain);
        Object* current = root_;
        for (size_t c = 0; c < n; ++c) {
            Frame& f = stack_[chain[c]];
            // The frame's own index covers its object; the lookup here is in
            // the parent, whose frame is the previous chain entry (or root).
            KeyIndex& parent_index = c == 0 ? stack_[0].index : stack_[chain[c - 1]].index;
            size_t at = find_key(*current, parent_index, f.key);
            if (at == std::string_view::npos) return nullptr;
            Value& child = (*current)[at].value;
            if (f.kind == FrameKind::Key) {
                current = child.as_object_mut();
                if (!current) return nullptr;
            } else {
                Array* arr = child.as_array_mut();
                if (!arr || f.item_idx >= arr->size()) return nullptr;
                current = (*arr)[f.item_idx].as_object_mut();
                if (!current) return nullptr;
            }
        }
        cache_idx_ = target;
        cache_obj_ = current;
        return current;
    }

    void push_frame(Frame f) {
        stack_.push_back(std::move(f));
        invalidate();
    }
    void pop_frame() {
        stack_.pop_back();
        invalidate();
    }
    void invalidate() {
        cache_idx_ = static_cast<size_t>(-1);
        cache_obj_ = nullptr;
        meta_path_valid_ = false;
    }

    void insert_value(size_t parent_idx, std::string_view key, Value v) {
        Object* target = navigate(parent_idx);
        if (!target) return;  // broken path: the line is dropped, never misplaced
        size_t at = find_key(*target, stack_[parent_idx].index, key);
        if (at != std::string_view::npos) {
            (*target)[at].value = std::move(v);
        } else {
            target->push_back(Pair{std::string(key), std::move(v)});
        }
    }

    const std::string& meta_path() {
        if (!meta_path_valid_) {
            meta_path_.clear();
            if (stack_.size() > 1) {
                size_t chain[kMaxParseNestingDepth + 2];
                size_t n = frame_chain(stack_.size() - 1, chain);
                for (size_t c = 0; c < n; ++c) {
                    const Frame& f = stack_[chain[c]];
                    if (!meta_path_.empty()) meta_path_.push_back('.');
                    push_key_segment(meta_path_, f.key);
                    if (f.kind == FrameKind::ListItem) push_item_segment(meta_path_, f.item_idx);
                }
            }
            meta_path_valid_ = true;
        }
        return meta_path_;
    }

    // The first line at or after byte `from` that is content — not blank, not
    // a comment, not inside a `###` block — CR excluded.
    //
    // spec: a comment line is ignored (§5), so it cannot stand between a key
    // and its list or between a `- ` line and the keys of its item. Only blank
    // lines were skipped here: `items` / `# note` / `- a` read as an empty
    // object and lost every item.
    bool next_content_line(size_t from, std::string_view& line) const {
        size_t p = from;
        bool in_block_comment = false;
        while (p <= text_.size()) {
            const void* hit = p < text_.size() ? std::memchr(text_.data() + p, '\n', text_.size() - p) : nullptr;
            size_t nl = hit ? static_cast<size_t>(static_cast<const char*>(hit) - text_.data()) : text_.size();
            size_t end = (nl > p && text_[nl - 1] == '\r') ? nl - 1 : nl;
            std::string_view l = text_.substr(p, end - p);
            std::string_view t = trim_view(l);
            if (t == "###") {
                in_block_comment = !in_block_comment;
            } else if (!in_block_comment && !t.empty() && t[0] != '#' && !starts_with(t, "//")) {
                line = l;
                return true;
            }
            p = nl + 1;
        }
        return false;
    }

    void handle_list_item(std::string_view t, int indent, size_t pos);
    void handle_key_line(ParsedLine& p, int indent, size_t pos);

    std::string_view text_;
    ParseResult& r_;
    Object* root_;
    std::vector<Frame> stack_;
    size_t cache_idx_ = static_cast<size_t>(-1);
    Object* cache_obj_ = nullptr;
    std::string meta_path_;
    bool meta_path_valid_ = false;
    std::optional<BlockState> block_;
    // Open lists, outermost first.
    //
    // spec: a list inside a list item ends at the next item of the outer list
    // (§8.5). One slot held "the" open list, so a nested `tags` replaced its
    // parent: `- name b` after it matched no list and was dropped without a
    // word, and the keys under it landed in the item before.
    std::vector<ListState> lists_;
};

void Parser::handle_list_item(std::string_view t, int indent, size_t pos) {
    // Pop every frame of the previous item — its own and any object nested in it.
    while (stack_.size() > 1 && stack_.back().indent >= indent) pop_frame();

    std::string_view val_str = strip_comment(trim_view(t.substr(2)));

    // A more deeply indented next line of content that is not an item makes
    // this item an object.
    bool nested = false;
    std::string_view peek;
    if (next_content_line(pos, peek)) {
        std::string_view pt = trim_view(peek);
        int pi = static_cast<int>(indent_len(peek));
        if (pi > indent && !starts_with(pt, "- ")) nested = true;
    }

    const std::string_view list_key = lists_.back().key;
    const size_t list_stack_idx = lists_.back().stack_idx;
    Object* parent = navigate(list_stack_idx);
    if (!parent) return;
    size_t at = find_key(*parent, stack_[list_stack_idx].index, list_key);
    if (at == std::string_view::npos) {
        parent->push_back(Pair{std::string(list_key), Value::make_array()});
        at = parent->size() - 1;
    }
    Array* arr = (*parent)[at].value.as_array_mut();
    if (!arr) return;
    if (arr->size() >= kMaxListItems) {
        // The cap dropped an item: say so, like every other cap.
        r_.truncated = true;
        return;
    }
    if (nested) {
        Object item;
        ParsedLine p;
        bool head_declares = false;
        Meta head_meta;
        std::string_view head_key;
        if (parse_line(val_str, p)) {
            Value v = p.type_hint ? cast_typed(p.value, *p.type_hint)
                    : !p.value.empty() ? cast_value(p.value)
                    : Value::make_object();
            if (!is_reserved_js_key(p.key)) {
                item.push_back(Pair{std::string(p.key), std::move(v)});
                if (r_.mode == Mode::Active && (!p.markers.empty() || p.constraints || p.type_hint)) {
                    head_declares = true;
                    head_key = p.key;
                    for (std::string_view mk : p.markers) head_meta.markers.emplace_back(mk);
                    head_meta.args = std::move(p.marker_args);
                    if (p.type_hint) head_meta.type_hint = std::string(*p.type_hint);
                    head_meta.constraints = std::move(p.constraints);
                }
            }
        } else {
            item.push_back(Pair{"_value", cast_value(val_str)});
        }
        size_t item_idx = arr->size();
        arr->push_back(Value::make_object(std::move(item)));
        if (stack_.size() < kMaxParseNestingDepth) {
            push_frame(Frame{indent, FrameKind::ListItem, list_key, item_idx, list_stack_idx, {}});
            // spec: the key on the `- ` line is a key line like the ones under
            // it. Its markers and constraints were read and thrown away:
            // `- token:secret abc` stayed in the clear and `- host:env H` kept
            // the variable's name.
            if (head_declares) r_.metadata[meta_path()].insert_or_assign(std::string(head_key), std::move(head_meta));
        }
    } else {
        arr->push_back(cast_value(val_str));
    }
}

void Parser::handle_key_line(ParsedLine& p, int indent, size_t pos) {
    while (stack_.size() > 1 && stack_.back().indent >= indent) pop_frame();
    const size_t parent_idx = stack_.size() - 1;

    // A key that replaces an object or a list replaces its declarations too.
    if (r_.mode == Mode::Active && !r_.metadata.empty()) {
        Object* parent = navigate(parent_idx);
        bool replaces_tree = false;
        if (parent) {
            size_t at = find_key(*parent, stack_[parent_idx].index, p.key);
            if (at != std::string_view::npos) {
                const Value& old = (*parent)[at].value;
                replaces_tree = old.is_object() || old.is_array();
            }
        }
        if (replaces_tree) {
            std::string prefix = meta_path();
            if (!prefix.empty()) prefix.push_back('.');
            push_key_segment(prefix, p.key);
            r_.metadata.retain([&](const std::string& path, const MetaMap&) {
                return !(path == prefix
                         || (path.size() > prefix.size() && path.compare(0, prefix.size(), prefix) == 0
                             && path[prefix.size()] == '.'));
            });
        }
    }

    if (r_.mode == Mode::Active && (!p.markers.empty() || p.constraints || p.type_hint)) {
        Meta m;
        m.markers.reserve(p.markers.size());
        for (std::string_view mk : p.markers) m.markers.emplace_back(mk);
        m.args = std::move(p.marker_args);
        if (p.type_hint) m.type_hint = std::string(*p.type_hint);
        m.constraints = std::move(p.constraints);
        r_.metadata[meta_path()].insert_or_assign(std::string(p.key), std::move(m));
    }

    const bool is_block = p.value == "|" || p.value == "|+";
    bool is_list_marker = false;
    for (std::string_view m : p.markers) {
        if (m == "random" || m == "unique" || m == "geo" || m == "join") {
            is_list_marker = true;
            break;
        }
    }

    if (is_block) {
        insert_value(parent_idx, p.key, Value::make_string(std::string()));
        block_ = BlockState{indent, p.key, std::string(), parent_idx, p.value == "|+", -1};
    } else if (is_list_marker && p.value.empty()) {
        // The key exists even when the list stays empty.
        insert_value(parent_idx, p.key, Value::make_array());
        lists_.push_back(ListState{indent, p.key, parent_idx});
    } else if (p.value.empty()) {
        std::string_view peek;
        if (next_content_line(pos, peek) && starts_with(trim_view(peek), "- ")) {
            insert_value(parent_idx, p.key, Value::make_array());
            lists_.push_back(ListState{indent, p.key, parent_idx});
            return;
        }
        insert_value(parent_idx, p.key, Value::make_object());
        // Past the cap the object is still inserted; nesting stops growing.
        if (stack_.size() < kMaxParseNestingDepth) {
            push_frame(Frame{indent, FrameKind::Key, p.key, 0, 0, {}});
        }
    } else {
        Value v = p.type_hint ? cast_typed(p.value, *p.type_hint) : cast_value(p.value);
        insert_value(parent_idx, p.key, std::move(v));
    }
}

void Parser::run() {
    bool in_block_comment = false;
    // Indent of a refused key whose block is being dropped, or -1.
    int dropped_below = -1;
    ParsedLine parsed;
    const size_t total = text_.size();
    size_t pos = 0;
    while (pos <= total) {
        const size_t start = pos;
        const void* hit = start < total ? std::memchr(text_.data() + start, '\n', total - start) : nullptr;
        const size_t nl = hit ? static_cast<size_t>(static_cast<const char*>(hit) - text_.data()) : total;
        pos = nl + 1;
        const size_t end = (nl > start && text_[nl - 1] == '\r') ? nl - 1 : nl;
        const std::string_view raw = text_.substr(start, end - start);
        const std::string_view t = trim_view(raw);

        // A blank line is never structural and never body (§8.4.1).
        if (t.empty()) continue;
        const int indent = static_cast<int>(indent_len(raw));

        if (dropped_below >= 0) {
            if (indent > dropped_below) continue;
            dropped_below = -1;
        }

        // Multiline body first: `#`, `//`, `###` and `!…` are content there.
        if (block_) {
            BlockState& blk = *block_;
            if (indent > blk.indent) {
                if (blk.content.size() < kMaxMultilineBlockBytes) {
                    if (!blk.content.empty()) blk.content.push_back('\n');
                    size_t room = kMaxMultilineBlockBytes - std::min(blk.content.size(), kMaxMultilineBlockBytes);
                    if (room > 0) {
                        std::string_view slice;
                        if (blk.preserve_indent) {
                            if (blk.base_indent < 0) blk.base_indent = indent;
                            size_t strip = static_cast<size_t>(std::min(blk.base_indent, indent));
                            std::string_view raw_trim = trim_end_view(raw);
                            size_t cut = strip_boundary(raw_trim, strip);
                            slice = cut < raw_trim.size() ? raw_trim.substr(cut) : std::string_view();
                        } else {
                            slice = t;
                        }
                        size_t n = utf8_prefix_len(slice, room);
                        blk.content.append(slice.data(), n);
                        if (n < slice.size()) r_.truncated = true;
                    }
                } else {
                    r_.truncated = true;
                }
                continue;
            }
            std::string content = std::move(blk.content);
            std::string_view key = blk.key;
            size_t idx = blk.stack_idx;
            block_.reset();
            insert_value(idx, key, Value::make_string(std::move(content)));
        }

        if (t == "###") {
            in_block_comment = !in_block_comment;
            continue;
        }
        if (in_block_comment) continue;

        // Directives (§6).
        if (t[0] == '!' || starts_with(t, "#!mode:")) {
            if (t == "!active") { r_.mode = Mode::Active; continue; }
            if (t == "!lock") { r_.locked = true; continue; }
            if (t == "!tool") { r_.tool = true; continue; }
            if (t == "!schema") { r_.schema = true; continue; }
            if (t == "!llm") { r_.llm = true; continue; }
            if (starts_with(t, "!include ")) {
                if (r_.includes.size() < kMaxIncludeDirectives) {
                    std::string_view rest = trim_view(t.substr(9));
                    size_t split = 0;
                    while (split < rest.size() && ws_len_at(rest, split) == 0) {
                        split += utf8_len(static_cast<unsigned char>(rest[split]));
                    }
                    std::string path(rest.substr(0, split));
                    std::string alias;
                    if (split < rest.size()) {
                        alias = std::string(trim_view(rest.substr(split + ws_len_at(rest, split))));
                    } else {
                        // Alias from the file name: `./db.synx` → `db`.
                        size_t slash = path.find_last_of("/\\");
                        std::string_view name = slash == std::string::npos
                            ? std::string_view(path) : std::string_view(path).substr(slash + 1);
                        auto ends = [&](std::string_view suf) {
                            return name.size() >= suf.size() && name.substr(name.size() - suf.size()) == suf;
                        };
                        if (ends(".synx") || ends(".SYNX")) name.remove_suffix(5);
                        alias = std::string(name);
                    }
                    r_.includes.push_back(IncludeDirective{std::move(path), std::move(alias)});
                }
                continue;
            }
            if (starts_with(t, "!use ")) {
                std::string_view rest = trim_view(t.substr(5));
                if (!rest.empty() && rest[0] == '@') {
                    size_t as = rest.find(" as ");
                    std::string package(trim_view(rest.substr(0, as)));
                    std::string alias;
                    if (as != std::string_view::npos) {
                        alias = std::string(trim_view(rest.substr(as + 4)));
                    } else {
                        size_t slash = package.find_last_of('/');
                        alias = slash == std::string::npos ? package : package.substr(slash + 1);
                    }
                    if (!package.empty()) r_.uses.push_back(UseDirective{std::move(package), std::move(alias)});
                }
                continue;
            }
            if (starts_with(t, "#!mode:")) {
                r_.mode = trim_view(t.substr(7)) == "active" ? Mode::Active : Mode::Static;
                continue;
            }
        }

        if (t[0] == '#' || starts_with(t, "//")) continue;

        if (starts_with(t, "- ")) {
            // The list this item belongs to: the innermost open one whose key
            // sits to the left of the dash. Lists nested deeper than the dash
            // end here — but only when a list does take the line; a dash no
            // list accepts is skipped and closes nothing, as it always was.
            size_t accepting = lists_.size();
            while (accepting > 0 && indent < lists_[accepting - 1].indent) --accepting;
            if (accepting > 0 && indent > lists_[accepting - 1].indent) lists_.resize(accepting);
            if (!lists_.empty() && indent > lists_.back().indent) {
                handle_list_item(t, indent, pos);
                continue;
            }
        } else {
            // A non-item line closes every list it is at or below the indent of.
            bool closed = false;
            while (!lists_.empty() && indent <= lists_.back().indent) {
                lists_.pop_back();
                closed = true;
            }
            if (closed) {
                while (stack_.size() > 1 && stack_.back().kind == FrameKind::ListItem
                       && stack_.back().indent >= indent) {
                    pop_frame();
                }
            }
        }

        if (!parse_line(t, parsed)) continue;
        // `__proto__` / `constructor` / `prototype` never reach a JS consumer.
        //
        // safety: the lines under such a key go with it. Only the key line was
        // skipped, so its block fell to whatever was open above it — `profile`
        // / `__proto__` / `  role admin` gave `profile` the role — and under
        // `prototype |` each line of the text was read as a line of the document.
        if (is_reserved_js_key(parsed.key)) {
            dropped_below = indent;
            continue;
        }
        handle_key_line(parsed, indent, pos);
    }

    if (block_) {
        insert_value(block_->stack_idx, block_->key, Value::make_string(std::move(block_->content)));
        block_.reset();
    }
}

} // namespace

// ─── public API ─────────────────────────────────────────────────────────────

std::string_view clamp_synx_text(std::string_view text) noexcept {
    // A single leading U+FEFF is dropped (SYNX §2.1/§6): it is not whitespace,
    // and left in place it would turn `!active` into a key.
    if (text.size() >= 3 && static_cast<unsigned char>(text[0]) == 0xEF
        && static_cast<unsigned char>(text[1]) == 0xBB && static_cast<unsigned char>(text[2]) == 0xBF) {
        text.remove_prefix(3);
    }
    if (text.size() <= kMaxSynxInputBytes) return text;
    size_t end = kMaxSynxInputBytes;
    while (end > 0 && (static_cast<unsigned char>(text[end]) & 0xC0) == 0x80) --end;
    return text.substr(0, end);
}

ParseResult parse(std::string_view text) {
    // A leading U+FEFF is dropped (§2.1), not lost content: it does not count.
    const bool bom = text.size() >= 3 && static_cast<unsigned char>(text[0]) == 0xEF
                  && static_cast<unsigned char>(text[1]) == 0xBB && static_cast<unsigned char>(text[2]) == 0xBF;
    const size_t original = bom ? text.size() - 3 : text.size();
    text = clamp_synx_text(text);
    // Line cap: keep at most kMaxLineStarts - 1 newlines.
    {
        size_t seen = 0;
        size_t scan = 0;
        while (scan < text.size()) {
            const void* p = std::memchr(text.data() + scan, '\n', text.size() - scan);
            if (!p) break;
            size_t at = static_cast<size_t>(static_cast<const char*>(p) - text.data());
            if (seen >= kMaxLineStarts - 1) {
                text = text.substr(0, at);
                break;
            }
            ++seen;
            scan = at + 1;
        }
    }
    ParseResult result;
    // Either cap having bitten means input was dropped before parsing began.
    result.truncated = text.size() < original;
    Parser(text, result).run();
    return result;
}

Value reshape_tool_output(const Value& root, bool schema) {
    const Object* obj = root.as_object();
    if (!obj) return root;

    std::vector<const Pair*> entries;
    entries.reserve(obj->size());
    for (const auto& p : *obj) entries.push_back(&p);
    std::sort(entries.begin(), entries.end(), [](const Pair* a, const Pair* b) { return a->key < b->key; });

    if (schema) {
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

    Object out;
    if (entries.empty()) {
        out.push_back(Pair{"tool", Value::make_null()});
        out.push_back(Pair{"params", Value::make_object()});
        return Value::make_object(std::move(out));
    }
    const Pair* first = entries.front();
    out.push_back(Pair{"tool", Value::make_string(first->key)});
    out.push_back(Pair{"params", first->value.is_object() ? first->value : Value::make_object()});
    return Value::make_object(std::move(out));
}

} // namespace synx
