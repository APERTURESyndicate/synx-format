// `[pattern:…]` engine: regex-syntax 0.8 parsing rules, a Thompson NFA, and a
// Pike VM over code points. See pattern.hpp.
#include "pattern.hpp"

#include "unicode.hpp"

#include <algorithm>
#include <cstring>
#include <string>
#include <unordered_map>

namespace synx {
namespace detail {

namespace {

struct NamedSet {
    const char* name;
    uint32_t offset;
    uint32_t count;
};

struct NamePair {
    const char* key;
    const char* value;
};

#include "pattern_tables.inc"

using Ranges = Pattern::Ranges;

constexpr uint32_t kMaxScalar = 0x10FFFF;
/// Most instructions a compiled pattern may hold; past it the pattern counts as
/// one synx-core does not compile (its own limit is in bytes of program).
constexpr size_t kMaxProgram = 100000;

enum Op : uint8_t { kChar, kSplit, kJmp, kAssert, kMatch };
enum AssertKind : uint32_t {
    kStartText, kEndText, kStartLine, kEndLine, kStartLineCrlf, kEndLineCrlf,
    kWord, kNotWord, kWordStart, kWordEnd, kWordStartHalf, kWordEndHalf,
};
/// Added to a word assertion that reads ASCII word characters only (`(?-u)`).
constexpr uint32_t kAsciiOffset = 100;

/// Set when the pattern is one the `regex` crate refuses. The port builds
/// without exceptions (UE5), so parsing stops on this flag instead.
thread_local bool t_refused = false;

void refuse() { t_refused = true; }

// ─── sets of code points ────────────────────────────────────────────────────

void canon(Ranges& r) {
    std::sort(r.begin(), r.end());
    Ranges out;
    for (const auto& x : r) {
        if (!out.empty() && x.first <= out.back().second + 1) {
            out.back().second = std::max(out.back().second, x.second);
        } else {
            out.push_back(x);
        }
    }
    r.swap(out);
}

Ranges unite(Ranges a, const Ranges& b) {
    a.insert(a.end(), b.begin(), b.end());
    canon(a);
    return a;
}

/// The complement among Unicode scalar values (no surrogates), as the crate's.
Ranges negate(const Ranges& r) {
    Ranges out;
    uint32_t next = 0;
    for (const auto& x : r) {
        if (x.first > next) out.emplace_back(next, x.first - 1);
        next = x.second + 1;
    }
    if (next <= kMaxScalar) out.emplace_back(next, kMaxScalar);
    Ranges cut;
    for (const auto& x : out) {
        if (x.second < 0xD800 || x.first > 0xDFFF) {
            cut.push_back(x);
        } else {
            if (x.first < 0xD800) cut.emplace_back(x.first, 0xD7FF);
            if (x.second > 0xDFFF) cut.emplace_back(0xE000, x.second);
        }
    }
    return cut;
}

Ranges intersect(const Ranges& a, const Ranges& b) {
    Ranges out;
    size_t i = 0, j = 0;
    while (i < a.size() && j < b.size()) {
        uint32_t lo = std::max(a[i].first, b[j].first);
        uint32_t hi = std::min(a[i].second, b[j].second);
        if (lo <= hi) out.emplace_back(lo, hi);
        if (a[i].second < b[j].second) ++i;
        else ++j;
    }
    return out;
}

Ranges minus(const Ranges& a, const Ranges& b) { return intersect(a, negate(b)); }

Ranges symdiff(const Ranges& a, const Ranges& b) { return minus(unite(a, b), intersect(a, b)); }

bool has(const Ranges& r, uint32_t c) {
    size_t lo = 0, hi = r.size();
    while (lo < hi) {
        size_t mid = (lo + hi) / 2;
        if (c < r[mid].first) hi = mid;
        else if (c > r[mid].second) lo = mid + 1;
        else return true;
    }
    return false;
}

/// Simple case folding of a set, as `ClassUnicode::case_fold_simple`; without
/// Unicode, ASCII letters only.
Ranges fold(Ranges r, bool unicode) {
    Ranges add;
    if (!unicode) {
        for (const auto& x : r) {
            uint32_t l = std::max<uint32_t>(x.first, 'A'), h = std::min<uint32_t>(x.second, 'Z');
            if (l <= h) add.emplace_back(l + 32, h + 32);
            l = std::max<uint32_t>(x.first, 'a');
            h = std::min<uint32_t>(x.second, 'z');
            if (l <= h) add.emplace_back(l - 32, h - 32);
        }
    } else {
        constexpr size_t kEntries = sizeof(kFold) / sizeof(kFold[0]) / 3;
        for (const auto& x : r) {
            // First entry with a key >= x.first.
            size_t lo = 0, hi = kEntries;
            while (lo < hi) {
                size_t mid = (lo + hi) / 2;
                if (kFold[mid * 3] < x.first) lo = mid + 1;
                else hi = mid;
            }
            for (size_t e = lo; e < kEntries && kFold[e * 3] <= x.second; ++e) {
                const uint32_t off = kFold[e * 3 + 1], n = kFold[e * 3 + 2];
                for (uint32_t k = 0; k < n; ++k) add.emplace_back(kFoldTargets[off + k], kFoldTargets[off + k]);
            }
        }
    }
    return unite(std::move(r), add);
}

bool table(std::string_view name, Ranges& out) {
    for (const auto& s : kSets) {
        if (name == s.name) {
            out.clear();
            out.reserve(s.count);
            for (uint32_t k = 0; k < s.count; ++k) {
                out.emplace_back(kSetRanges[(s.offset + k) * 2], kSetRanges[(s.offset + k) * 2 + 1]);
            }
            return true;
        }
    }
    return false;
}

Ranges table_or_refuse(std::string_view name) {
    Ranges r;
    if (!table(name, r)) refuse();
    return r;
}

Ranges gc_union(std::initializer_list<const char*> leaves) {
    Ranges r;
    for (const char* l : leaves) r = unite(std::move(r), table_or_refuse(std::string("gc:") + l));
    return r;
}

Ranges unassigned() {
    return negate(gc_union({"Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter", "Modifier_Letter",
                            "Other_Letter", "Nonspacing_Mark", "Spacing_Mark", "Enclosing_Mark", "Decimal_Number",
                            "Letter_Number", "Other_Number", "Connector_Punctuation", "Dash_Punctuation",
                            "Open_Punctuation", "Close_Punctuation", "Initial_Punctuation", "Final_Punctuation",
                            "Other_Punctuation", "Math_Symbol", "Currency_Symbol", "Modifier_Symbol", "Other_Symbol",
                            "Space_Separator", "Line_Separator", "Paragraph_Separator", "Control", "Format",
                            "Private_Use"}));
}

/// A General_Category by its canonical name (the crate's `gencat`).
Ranges gencat(std::string_view canon_name) {
    if (canon_name == "Any") return Ranges{{0, 0xD7FF}, {0xE000, kMaxScalar}};
    if (canon_name == "ASCII") return Ranges{{0, 0x7F}};
    if (canon_name == "Assigned") return negate(unassigned());
    if (canon_name == "Unassigned") return unassigned();
    if (canon_name == "Letter") {
        return gc_union({"Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter", "Modifier_Letter", "Other_Letter"});
    }
    if (canon_name == "Cased_Letter") return gc_union({"Uppercase_Letter", "Lowercase_Letter", "Titlecase_Letter"});
    if (canon_name == "Mark") return gc_union({"Nonspacing_Mark", "Spacing_Mark", "Enclosing_Mark"});
    if (canon_name == "Number") return gc_union({"Decimal_Number", "Letter_Number", "Other_Number"});
    if (canon_name == "Punctuation") {
        return gc_union({"Connector_Punctuation", "Dash_Punctuation", "Open_Punctuation", "Close_Punctuation",
                         "Initial_Punctuation", "Final_Punctuation", "Other_Punctuation"});
    }
    if (canon_name == "Symbol") return gc_union({"Math_Symbol", "Currency_Symbol", "Modifier_Symbol", "Other_Symbol"});
    if (canon_name == "Separator") return gc_union({"Space_Separator", "Line_Separator", "Paragraph_Separator"});
    if (canon_name == "Other") return unite(gc_union({"Control", "Format", "Private_Use"}), unassigned());
    // A leaf, or Surrogate, which has no scalar values and is refused.
    return table_or_refuse(std::string("gc:") + std::string(canon_name));
}

const char* lookup(const NamePair* table_begin, size_t n, std::string_view key) {
    for (size_t i = 0; i < n; ++i) {
        if (key == table_begin[i].key) return table_begin[i].value;
    }
    return nullptr;
}

#define SYNX_LOOKUP(T, key) lookup(T, sizeof(T) / sizeof(T[0]), key)

/// UAX44-LM3: no `is` prefix, no space, `_` or `-`, ASCII lower case.
std::string normalize(std::string_view name) {
    std::string_view s = name;
    const bool is_prefix = s.size() >= 2 && (s[0] == 'i' || s[0] == 'I') && (s[1] == 's' || s[1] == 'S');
    if (is_prefix) s.remove_prefix(2);
    std::string out;
    for (char ch : s) {
        unsigned char b = static_cast<unsigned char>(ch);
        if (b == ' ' || b == '_' || b == '-' || b > 0x7F) continue;
        out.push_back(b >= 'A' && b <= 'Z' ? static_cast<char>(b + 32) : static_cast<char>(b));
    }
    // `isc` is an alias of the Other category, not `c` with the prefix dropped.
    if (is_prefix && out == "c") out = "isc";
    return out;
}

/// `\pL`, `\p{Greek}`, `\p{Alphabetic}` (regex-syntax `canonical_binary`).
Ranges binary_query(std::string_view name) {
    const std::string norm = normalize(name);
    if (norm != "cf" && norm != "sc" && norm != "lc") {
        if (const char* prop = SYNX_LOOKUP(kPropertyNames, norm)) {
            if (std::strcmp(prop, "Decimal_Number") == 0) return gencat("Decimal_Number");
            return table_or_refuse(std::string("bool:") + prop);
        }
    }
    if (norm == "any") return gencat("Any");
    if (norm == "assigned") return gencat("Assigned");
    if (norm == "ascii") return gencat("ASCII");
    if (const char* gc = SYNX_LOOKUP(kGeneralCategories, norm)) return gencat(gc);
    if (const char* sc = SYNX_LOOKUP(kScripts, norm)) return table_or_refuse(std::string("sc:") + sc);
    refuse();
    return {};
}

/// `\p{name=value}`.
Ranges value_query(std::string_view name, std::string_view value) {
    const char* prop = SYNX_LOOKUP(kPropertyNames, normalize(name));
    const std::string v = normalize(value);
    if (!prop) {
        refuse();
        return {};
    }
    if (std::strcmp(prop, "General_Category") == 0) {
        if (v == "any") return gencat("Any");
        if (v == "assigned") return gencat("Assigned");
        if (v == "ascii") return gencat("ASCII");
        if (const char* gc = SYNX_LOOKUP(kGeneralCategories, v)) return gencat(gc);
        refuse();
        return {};
    }
    if (std::strcmp(prop, "Script") == 0) {
        if (const char* sc = SYNX_LOOKUP(kScripts, v)) return table_or_refuse(std::string("sc:") + sc);
    }
    // Script_Extensions, Age and the segmentation properties carry no table here.
    refuse();
    return {};
}

bool is_space_cp(uint32_t ch) {
    return ch == 0x20 || (ch >= 0x09 && ch <= 0x0D) || ch == 0x85 || ch == 0xA0 || ch == 0x1680
        || (ch >= 0x2000 && ch <= 0x200A) || ch == 0x2028 || ch == 0x2029 || ch == 0x202F || ch == 0x205F
        || ch == 0x3000;
}

bool is_ascii_letter(uint32_t c) { return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z'); }

bool is_ascii_word(uint32_t c) { return (c >= '0' && c <= '9') || is_ascii_letter(c) || c == '_'; }

const Ranges& word_set() {
    static const Ranges w = table_or_refuse("perl_word");
    return w;
}

// ─── syntax tree ────────────────────────────────────────────────────────────

struct Node {
    enum Kind { Empty, Char, Assert, Concat, Alt, Repeat, FlagsOnly } kind = Empty;
    Ranges set;
    uint32_t assert_kind = 0;
    std::vector<Node> items;
    uint32_t min = 0;
    uint32_t max = 0;
    bool unbounded = false;
};

Node make(Node::Kind k) {
    Node n;
    n.kind = k;
    return n;
}

struct Flags {
    bool i = false, m = false, s = false, u = true, x = false, r = false;
};

// ─── parser (regex-syntax 0.8 rules) ────────────────────────────────────────

class Parser {
public:
    explicit Parser(std::vector<uint32_t> p) : p_(std::move(p)) {}

    Node parse() {
        Node n = alternation();
        if (!eof()) { fail(); return {}; }  // an unopened `)`
        return n;
    }

private:
    static void fail() { refuse(); }
    static bool bad() { return t_refused; }
    bool eof() const { return t_refused || pos_ >= p_.size(); }
    uint32_t c() const { return pos_ < p_.size() ? p_[pos_] : 0; }

    void skip_space() {
        if (!flags_.x) return;
        while (!eof()) {
            if (is_space_cp(c())) {
                ++pos_;
            } else if (c() == '#') {
                while (!eof() && c() != '\n') ++pos_;
            } else {
                break;
            }
        }
    }

    Node alternation() {
        std::vector<Node> branches;
        branches.push_back(concat());
        while (!eof() && c() == '|') {
            ++pos_;
            branches.push_back(concat());
        }
        if (branches.size() == 1) return std::move(branches.front());
        Node n = make(Node::Alt);
        n.items = std::move(branches);
        return n;
    }

    Node concat() {
        std::vector<Node> items;
        while (true) {
            skip_space();
            if (eof()) break;
            const uint32_t ch = c();
            if (ch == '|' || ch == ')') break;
            if (ch == '*' || ch == '+' || ch == '?') {
                ++pos_;
                repeat_last(items, ch == '+' ? 1 : 0, ch == '?' ? 1 : 0, ch != '?');
                continue;
            }
            if (ch == '{') {
                counted(items);
                continue;
            }
            items.push_back(atom());
        }
        std::vector<Node> real;
        for (auto& n : items) {
            if (n.kind != Node::FlagsOnly) real.push_back(std::move(n));
        }
        if (real.empty()) return make(Node::Empty);
        if (real.size() == 1) return std::move(real.front());
        Node n = make(Node::Concat);
        n.items = std::move(real);
        return n;
    }

    void wrap_last(std::vector<Node>& items, uint32_t min, uint32_t max, bool unbounded) {
        Node r = make(Node::Repeat);
        r.items.push_back(std::move(items.back()));
        items.pop_back();
        r.min = min;
        r.max = max;
        r.unbounded = unbounded;
        items.push_back(std::move(r));
        skip_space();
        if (!eof() && c() == '?') ++pos_;  // lazy: the same set of matches
    }

    void repeat_last(std::vector<Node>& items, uint32_t min, uint32_t max, bool unbounded) {
        // Repetition operator missing expression.
        if (items.empty() || items.back().kind == Node::FlagsOnly) { fail(); return; }
        wrap_last(items, min, max, unbounded);
    }

    void counted(std::vector<Node>& items) {
        if (items.empty() || items.back().kind == Node::FlagsOnly) { fail(); return; }
        ++pos_;  // {
        auto space = [&] {
            while (!eof() && is_space_cp(c())) ++pos_;
        };
        auto decimal = [&]() -> uint32_t {
            space();
            const size_t start = pos_;
            uint64_t v = 0;
            while (!eof() && c() >= '0' && c() <= '9') {
                v = v * 10 + (c() - '0');
                if (v > 0xFFFFFFFFull) { fail(); return 0u; }
                ++pos_;
            }
            if (pos_ == start) { fail(); return 0u; }
            space();
            return static_cast<uint32_t>(v);
        };
        const uint32_t min = decimal();
        if (bad()) return;
        uint32_t max = min;
        bool unbounded = false;
        if (!eof() && c() == ',') {
            ++pos_;
            space();
            if (!eof() && c() == '}') unbounded = true;
            else max = decimal();
        if (bad()) return;
        }
        if (eof() || c() != '}') { fail(); return; }
        ++pos_;
        if (!unbounded && min > max) { fail(); return; }
        wrap_last(items, min, max, unbounded);
    }

    Node char_node(Ranges r) {
        Node n = make(Node::Char);
        n.set = std::move(r);
        return n;
    }

    Node assert_node(uint32_t k) {
        Node n = make(Node::Assert);
        n.assert_kind = k;
        return n;
    }

    Ranges literal(uint32_t ch) {
        Ranges r{{ch, ch}};
        return flags_.i ? fold(std::move(r), flags_.u) : r;
    }

    Node atom() {
        const uint32_t ch = c();
        switch (ch) {
            case '(':
                return group();
            case '[':
                ++pos_;
                return char_node(bracket());
            case '^':
                ++pos_;
                return assert_node(flags_.m ? (flags_.r ? kStartLineCrlf : kStartLine) : kStartText);
            case '$':
                ++pos_;
                return assert_node(flags_.m ? (flags_.r ? kEndLineCrlf : kEndLine) : kEndText);
            case '.': {
                ++pos_;
                if (!flags_.u) { fail(); return {}; }  // could match invalid UTF-8
                if (flags_.s) return char_node(gencat("Any"));
                Ranges nl{{'\n', '\n'}};
                if (flags_.r) nl.emplace_back('\r', '\r');
                canon(nl);
                return char_node(negate(nl));
            }
            case '\\':
                ++pos_;
                return escape();
            default:
                ++pos_;
                return char_node(literal(ch));
        }
    }

    Node group() {
        ++pos_;  // (
        const Flags saved = flags_;
        if (!eof() && c() == '?') {
            ++pos_;
            if (eof()) { fail(); return {}; }
            if (c() == 'P' || c() == '<') {
                // (?P<name>…) / (?<name>…)
                if (c() == 'P') {
                    ++pos_;
                    if (eof() || c() != '<') { fail(); return {}; }
                }
                ++pos_;
                const size_t start = pos_;
                while (!eof() && c() != '>') ++pos_;
                if (eof()) { fail(); return {}; }
                std::vector<uint32_t> name(p_.begin() + static_cast<std::ptrdiff_t>(start),
                                           p_.begin() + static_cast<std::ptrdiff_t>(pos_));
                ++pos_;
                if (!valid_group_name(name)) { fail(); return {}; }
                for (const auto& seen : names_) {
                    if (seen == name) { fail(); return {}; }
                }
                names_.push_back(std::move(name));
                return group_body(saved);
            }
            if (c() == ':') {
                ++pos_;
                return group_body(saved);
            }
            // Flags: `(?flags)` or `(?flags:…)`.
            Flags next = flags_;
            bool negate_flags = false, any = false, dangling = false;
            std::vector<uint32_t> seen;
            while (true) {
                if (eof()) { fail(); return {}; }
                const uint32_t f = c();
                if (f == ')' || f == ':') break;
                ++pos_;
                if (f == '-') {
                    if (negate_flags) { fail(); return {}; }
                    negate_flags = true;
                    dangling = true;
                    continue;
                }
                if (std::find(seen.begin(), seen.end(), f) != seen.end()) { fail(); return {}; }
                seen.push_back(f);
                const bool on = !negate_flags;
                switch (f) {
                    case 'i': next.i = on; break;
                    case 'm': next.m = on; break;
                    case 's': next.s = on; break;
                    case 'U': break;  // swaps greed; the same set of matches
                    case 'u': next.u = on; break;
                    case 'x': next.x = on; break;
                    case 'R': next.r = on; break;
                    default: { fail(); return {}; }  // lookaround, unknown flag
                }
                any = true;
                dangling = false;
            }
            if (!any || dangling) { fail(); return {}; }
            if (c() == ')') {
                ++pos_;
                flags_ = next;  // to the end of the enclosing group
                return make(Node::FlagsOnly);
            }
            ++pos_;  // :
            flags_ = next;
            return group_body(saved);
        }
        return group_body(saved);
    }

    Node group_body(const Flags& saved) {
        Node inner = alternation();
        if (eof() || c() != ')') { fail(); return {}; }
        ++pos_;
        flags_ = saved;
        Node n = make(Node::Concat);
        n.items.push_back(std::move(inner));
        return n;
    }

    /// regex-syntax `is_capture_char`: `_` or a letter first, then `_`, `.`,
    /// `[`, `]`, letters and digits.
    static bool valid_group_name(const std::vector<uint32_t>& name) {
        if (name.empty()) return false;
        bool first = true;
        for (uint32_t ch : name) {
            bool ok;
            if (ch == '_') ok = true;
            else if (first) ok = is_alphabetic_cp(ch);
            else ok = ch == '.' || ch == '[' || ch == ']' || is_alphabetic_cp(ch) || is_number_cp(ch);
            if (!ok) return false;
            first = false;
        }
        return true;
    }

    static bool is_alphabetic_cp(uint32_t ch) {
        static const Ranges a = table_or_refuse("bool:Alphabetic");
        return has(a, ch);
    }

    static bool is_number_cp(uint32_t ch) {
        static const Ranges n = gencat("Number");
        return has(n, ch);
    }

    uint32_t word(uint32_t kind) const { return flags_.u ? kind : kind + kAsciiOffset; }

    Node escape() {
        if (eof()) { fail(); return {}; }
        const uint32_t e = c();
        switch (e) {
            case 'b': {
                ++pos_;
                // `\b{start}` and the like; `\b{2}` is a repeated boundary.
                if (pos_ + 1 < p_.size() && c() == '{' && (is_ascii_letter(p_[pos_ + 1]) || p_[pos_ + 1] == '-')) {
                    ++pos_;
                    const size_t start = pos_;
                    while (!eof() && c() != '}') ++pos_;
                    if (eof()) { fail(); return {}; }
                    std::string name;
                    for (size_t k = start; k < pos_; ++k) name.push_back(static_cast<char>(p_[k] & 0x7F));
                    ++pos_;
                    if (name == "start") return assert_node(word(kWordStart));
                    if (name == "end") return assert_node(word(kWordEnd));
                    if (name == "start-half") return assert_node(word(kWordStartHalf));
                    if (name == "end-half") return assert_node(word(kWordEndHalf));
                    { fail(); return {}; }
                }
                return assert_node(word(kWord));
            }
            case 'B': ++pos_; return assert_node(word(kNotWord));
            case 'A': ++pos_; return assert_node(kStartText);
            case 'z': ++pos_; return assert_node(kEndText);
            case '<': ++pos_; return assert_node(word(kWordStart));
            case '>': ++pos_; return assert_node(word(kWordEnd));
            default: break;
        }
        bool is_literal = false;
        uint32_t cp = 0;
        Ranges r = escape_set(is_literal, cp);
        return char_node(std::move(r));
    }

    /// An escape that stands for code points, at `\` + 1; `is_literal` and
    /// `cp` are set when it stands for one character.
    Ranges escape_set(bool& is_literal, uint32_t& cp) {
        const uint32_t e = c();
        ++pos_;
        is_literal = false;
        switch (e) {
            case 'd': case 'D': return perl(0, e == 'D');
            case 's': case 'S': return perl(1, e == 'S');
            case 'w': case 'W': return perl(2, e == 'W');
            case 'p': case 'P': return unicode_class(e == 'P');
            default: break;
        }
        cp = literal_escape(e);
        is_literal = true;
        return literal(cp);
    }

    uint32_t literal_escape(uint32_t e) {
        switch (e) {
            case 'n': return '\n';
            case 't': return '\t';
            case 'r': return '\r';
            case 'f': return 0x0C;
            case 'v': return 0x0B;
            case 'a': return 0x07;
            case 'x': return hex_escape(2);
            case 'u': return hex_escape(4);
            case 'U': return hex_escape(8);
            default: break;
        }
        // Any ASCII character that is not a letter or a digit may be escaped.
        if (e < 0x80 && !(e >= '0' && e <= '9') && !is_ascii_letter(e)) return e;
        { fail(); return {}; }
    }

    static int hex_digit(uint32_t ch) {
        if (ch >= '0' && ch <= '9') return static_cast<int>(ch - '0');
        if (ch >= 'a' && ch <= 'f') return static_cast<int>(ch - 'a' + 10);
        if (ch >= 'A' && ch <= 'F') return static_cast<int>(ch - 'A' + 10);
        return -1;
    }

    uint32_t hex_escape(int digits) {
        uint32_t v = 0;
        if (!eof() && c() == '{') {
            ++pos_;
            const size_t start = pos_;
            while (!eof() && c() != '}') {
                int d = hex_digit(c());
                if (d < 0) { fail(); return {}; }
                v = v * 16 + static_cast<uint32_t>(d);
                if (v > kMaxScalar) { fail(); return {}; }
                ++pos_;
            }
            if (eof() || pos_ == start) { fail(); return {}; }
            ++pos_;
        } else {
            for (int k = 0; k < digits; ++k) {
                if (eof()) { fail(); return {}; }
                int d = hex_digit(c());
                if (d < 0) { fail(); return {}; }
                v = v * 16 + static_cast<uint32_t>(d);
                ++pos_;
            }
        }
        if (v > kMaxScalar || (v >= 0xD800 && v <= 0xDFFF)) { fail(); return {}; }
        // Without Unicode a byte past ASCII is not a character.
        if (!flags_.u && v > 0x7F) { fail(); return {}; }
        return v;
    }

    /// `\d` (0), `\s` (1), `\w` (2), negated when `neg`.
    Ranges perl(int kind, bool neg) {
        Ranges r;
        if (flags_.u) {
            r = kind == 0 ? gencat("Decimal_Number") : kind == 1 ? table_or_refuse("bool:White_Space") : word_set();
        } else {
            if (kind == 0) r = {{'0', '9'}};
            else if (kind == 1) r = {{0x09, 0x0D}, {' ', ' '}};
            else r = {{'0', '9'}, {'A', 'Z'}, {'_', '_'}, {'a', 'z'}};
        }
        if (neg) {
            // A negated class without Unicode could match invalid UTF-8.
            if (!flags_.u) { fail(); return {}; }
            return negate(r);
        }
        return r;
    }

    /// `\p…` / `\P…`, at the character after the letter.
    Ranges unicode_class(bool negated) {
        if (!flags_.u || eof()) { fail(); return {}; }
        bool neg = negated;
        Ranges r;
        if (c() == '{') {
            ++pos_;
            const size_t start = pos_;
            while (!eof() && c() != '}') ++pos_;
            if (eof()) { fail(); return {}; }
            std::string body;
            for (size_t k = start; k < pos_; ++k) append_utf8(body, p_[k]);
            ++pos_;
            size_t ne = body.find("!=");
            size_t colon = body.find(':');
            size_t eq = body.find('=');
            if (ne != std::string::npos) {
                neg = !neg;
                r = value_query(std::string_view(body).substr(0, ne), std::string_view(body).substr(ne + 2));
            } else if (colon != std::string::npos) {
                r = value_query(std::string_view(body).substr(0, colon), std::string_view(body).substr(colon + 1));
            } else if (eq != std::string::npos) {
                r = value_query(std::string_view(body).substr(0, eq), std::string_view(body).substr(eq + 1));
            } else {
                r = binary_query(body);
            }
        } else {
            if (c() == '\\') { fail(); return {}; }
            std::string one;
            append_utf8(one, c());
            ++pos_;
            r = binary_query(one);
        }
        if (flags_.i) r = fold(std::move(r), true);
        return neg ? negate(r) : r;
    }

    static void append_utf8(std::string& out, uint32_t cp) {
        if (cp < 0x80) {
            out.push_back(static_cast<char>(cp));
        } else if (cp < 0x800) {
            out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else if (cp < 0x10000) {
            out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else {
            out.push_back(static_cast<char>(0xF0 | (cp >> 18)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        }
    }

    // ─── bracketed classes ──────────────────────────────────────────────────

    void class_space() {
        if (!flags_.x) return;
        while (!eof() && is_space_cp(c())) ++pos_;
    }

    bool at_op() const {
        if (pos_ + 1 >= p_.size()) return false;
        const uint32_t a = p_[pos_], b = p_[pos_ + 1];
        return (a == '&' && b == '&') || (a == '-' && b == '-') || (a == '~' && b == '~');
    }

    /// `[…]`, after the `[`. Items are collected as written; the class is
    /// case-folded once, at its end, then negated (regex-syntax order).
    Ranges bracket() {
        bool negated = false;
        if (!eof() && c() == '^') {
            ++pos_;
            negated = true;
        }
        Ranges set = class_ops();
        if (eof() || c() != ']') { fail(); return {}; }
        ++pos_;
        if (flags_.i) set = fold(std::move(set), flags_.u);
        if (negated) {
            if (!flags_.u) { fail(); return {}; }  // could match invalid UTF-8
            return negate(set);
        }
        return set;
    }

    /// Unions joined by `&&`, `--` and `~~`, left to right; both sides of an
    /// operator are case-folded before it applies.
    Ranges class_ops() {
        Ranges acc = class_union(true);
        while (true) {
            if (bad()) return {};
            class_space();
            if (!at_op()) return acc;
            const uint32_t a = c();
            pos_ += 2;
            Ranges rhs = class_union(false);
            if (flags_.i) {
                acc = fold(std::move(acc), flags_.u);
                rhs = fold(std::move(rhs), flags_.u);
            }
            if (a == '&') acc = intersect(acc, rhs);
            else if (a == '-') acc = minus(acc, rhs);
            else acc = symdiff(acc, rhs);
        }
    }

    /// One literal item for a range end, as written (unfolded).
    bool class_literal(uint32_t& out) {
        if (c() == '\\') {
            ++pos_;
            if (eof()) { fail(); return {}; }
            const uint32_t e = c();
            if (e == 'd' || e == 'D' || e == 's' || e == 'S' || e == 'w' || e == 'W' || e == 'p' || e == 'P') {
                return false;
            }
            if (e == 'b' || e == 'B' || e == 'A' || e == 'z' || e == '<' || e == '>') { fail(); return {}; }  // assertions
            ++pos_;
            out = literal_escape(e);
            return true;
        }
        out = c();
        ++pos_;
        return true;
    }

    Ranges class_union(bool first) {
        Ranges parts;
        bool leading = first;
        while (true) {
            class_space();
            if (eof()) { fail(); return {}; }
            if (c() == ']' && !leading) break;
            // A `-` that opens the class is itself; any other operator may
            // stand first, with an empty left side.
            if (leading && c() == '-') {
                ++pos_;
                parts.emplace_back('-', '-');
                leading = false;
                continue;
            }
            if (at_op()) break;
            leading = false;
            if (c() == '[') {
                if (ascii_class(parts)) continue;
                ++pos_;
                parts = unite(std::move(parts), bracket());
                continue;
            }
            const size_t before = pos_;
            uint32_t lo = 0;
            if (!class_literal(lo)) {
                // `\d`, `\p{…}`: a class, which cannot start a range.
                pos_ = before + 1;
                bool is_literal = false;
                uint32_t cp = 0;
                Ranges set = class_escape_set(is_literal, cp);
                class_space();
                if (!eof() && c() == '-' && pos_ + 1 < p_.size() && p_[pos_ + 1] != ']' && p_[pos_ + 1] != '-') { fail(); return {}; }
                parts = unite(std::move(parts), set);
                continue;
            }
            class_space();
            if (!eof() && c() == '-' && pos_ + 1 < p_.size() && p_[pos_ + 1] != ']' && p_[pos_ + 1] != '-') {
                ++pos_;
                class_space();
                if (eof() || c() == '[') { fail(); return {}; }
                uint32_t hi = 0;
                if (!class_literal(hi)) { fail(); return {}; }  // a class escape cannot end a range
                if (lo > hi) { fail(); return {}; }
                parts.emplace_back(lo, hi);
            } else {
                parts.emplace_back(lo, lo);
            }
        }
        canon(parts);
        return parts;
    }

    /// A class escape inside brackets, unfolded (the bracket folds it).
    Ranges class_escape_set(bool& is_literal, uint32_t& cp) {
        const uint32_t e = c();
        ++pos_;
        is_literal = false;
        cp = 0;
        switch (e) {
            case 'd': case 'D': return perl(0, e == 'D');
            case 's': case 'S': return perl(1, e == 'S');
            case 'w': case 'W': return perl(2, e == 'W');
            default: return unicode_class(e == 'P');
        }
    }

    /// `[:name:]` / `[:^name:]` at a `[`, added to `parts`; false (nothing
    /// read) when the text there is not one — then it is a nested class.
    bool ascii_class(Ranges& parts) {
        if (pos_ + 1 >= p_.size() || p_[pos_ + 1] != ':') return false;
        size_t k = pos_ + 2;
        bool neg = false;
        if (k < p_.size() && p_[k] == '^') {
            neg = true;
            ++k;
        }
        const size_t start = k;
        while (k < p_.size() && p_[k] != ':') ++k;
        if (k + 1 >= p_.size() || p_[k + 1] != ']') return false;
        std::string name;
        for (size_t q = start; q < k; ++q) {
            if (p_[q] > 0x7F) return false;
            name.push_back(static_cast<char>(p_[q]));
        }
        Ranges r;
        if (name == "alnum") r = {{'0', '9'}, {'A', 'Z'}, {'a', 'z'}};
        else if (name == "alpha") r = {{'A', 'Z'}, {'a', 'z'}};
        else if (name == "ascii") r = {{0, 0x7F}};
        else if (name == "blank") r = {{'\t', '\t'}, {' ', ' '}};
        else if (name == "cntrl") r = {{0, 0x1F}, {0x7F, 0x7F}};
        else if (name == "digit") r = {{'0', '9'}};
        else if (name == "graph") r = {{'!', '~'}};
        else if (name == "lower") r = {{'a', 'z'}};
        else if (name == "print") r = {{' ', '~'}};
        else if (name == "punct") r = {{'!', '/'}, {':', '@'}, {'[', '`'}, {'{', '~'}};
        else if (name == "space") r = {{'\t', '\r'}, {' ', ' '}};
        else if (name == "upper") r = {{'A', 'Z'}};
        else if (name == "word") r = {{'0', '9'}, {'A', 'Z'}, {'_', '_'}, {'a', 'z'}};
        else if (name == "xdigit") r = {{'0', '9'}, {'A', 'F'}, {'a', 'f'}};
        else return false;
        pos_ = k + 2;
        if (flags_.i) r = fold(std::move(r), flags_.u);
        if (neg) {
            if (!flags_.u) { fail(); return {}; }
            r = negate(r);
        }
        parts = unite(std::move(parts), r);
        return true;
    }

    std::vector<uint32_t> p_;
    size_t pos_ = 0;
    Flags flags_;
    std::vector<std::vector<uint32_t>> names_;
};

bool starts_anchored(const Node& n) {
    switch (n.kind) {
        case Node::Assert: return n.assert_kind == kStartText;
        case Node::Concat: return !n.items.empty() && starts_anchored(n.items.front());
        case Node::Alt:
            for (const auto& b : n.items) {
                if (!starts_anchored(b)) return false;
            }
            return true;
        case Node::Repeat: return n.min > 0 && starts_anchored(n.items.front());
        default: return false;
    }
}

} // namespace

// ─── compiler ───────────────────────────────────────────────────────────────

struct PatternBuilder {
    Pattern& out;
    /// One set per character node, however many times a repetition emits it.
    std::unordered_map<const Node*, uint32_t> set_ids;

    uint32_t emit(uint8_t op, uint32_t a = 0, uint32_t b = 0) {
        if (out.prog_.size() >= kMaxProgram) {
            refuse();
            return 0;
        }
        out.prog_.push_back(Pattern::Inst{op, a, b});
        return static_cast<uint32_t>(out.prog_.size() - 1);
    }

    uint32_t here() const { return static_cast<uint32_t>(out.prog_.size()); }

    void node(const Node& n) {
        if (t_refused) return;
        switch (n.kind) {
            case Node::Empty:
            case Node::FlagsOnly:
                return;
            case Node::Char: {
                auto found = set_ids.find(&n);
                if (found == set_ids.end()) {
                    out.sets_.push_back(n.set);
                    found = set_ids.emplace(&n, static_cast<uint32_t>(out.sets_.size() - 1)).first;
                }
                emit(kChar, found->second);
                return;
            }
            case Node::Assert:
                emit(kAssert, n.assert_kind);
                return;
            case Node::Concat:
                for (const auto& i : n.items) node(i);
                return;
            case Node::Alt: {
                std::vector<uint32_t> jumps;
                for (size_t k = 0; k < n.items.size(); ++k) {
                    if (k + 1 < n.items.size()) {
                        const uint32_t split = emit(kSplit);
                        out.prog_[split].a = here();
                        node(n.items[k]);
                        jumps.push_back(emit(kJmp));
                        out.prog_[split].b = here();
                    } else {
                        node(n.items[k]);
                    }
                }
                for (uint32_t j : jumps) out.prog_[j].a = here();
                return;
            }
            case Node::Repeat: {
                const Node& inner = n.items.front();
                for (uint32_t k = 0; k < n.min; ++k) node(inner);
                if (n.unbounded) {
                    const uint32_t split = emit(kSplit);
                    out.prog_[split].a = here();
                    node(inner);
                    emit(kJmp, split);
                    out.prog_[split].b = here();
                } else {
                    std::vector<uint32_t> exits;
                    for (uint32_t k = n.min; k < n.max; ++k) {
                        const uint32_t split = emit(kSplit);
                        out.prog_[split].a = here();
                        exits.push_back(split);
                        node(inner);
                    }
                    for (uint32_t s : exits) out.prog_[s].b = here();
                }
                return;
            }
        }
    }
};

std::unique_ptr<Pattern> Pattern::compile(std::string_view source) {
    std::vector<uint32_t> cps;
    for (size_t i = 0; i < source.size();) {
        size_t len = 0;
        cps.push_back(decode_utf8(source, i, len));
        i += len == 0 ? 1 : len;
    }
    t_refused = false;
    Node root = Parser(std::move(cps)).parse();
    if (t_refused) return nullptr;
    auto p = std::unique_ptr<Pattern>(new Pattern());
    PatternBuilder b{*p};
    b.node(root);
    b.emit(kMatch);
    if (t_refused) return nullptr;
    p->anchored_start_ = starts_anchored(root);
    return p;
}

namespace {

class SparseSet {
public:
    explicit SparseSet(size_t n) : dense_(n), sparse_(n) {}
    bool contains(uint32_t v) const { return sparse_[v] < size_ && dense_[sparse_[v]] == v; }
    void add(uint32_t v) {
        sparse_[v] = static_cast<uint32_t>(size_);
        dense_[size_++] = v;
    }
    void clear() { size_ = 0; }
    size_t size() const { return size_; }
    uint32_t at(size_t i) const { return dense_[i]; }

private:
    std::vector<uint32_t> dense_;
    std::vector<uint32_t> sparse_;
    size_t size_ = 0;
};

bool holds(uint32_t kind, const std::vector<uint32_t>& cps, size_t pos) {
    const bool has_prev = pos > 0, has_next = pos < cps.size();
    const uint32_t prev = has_prev ? cps[pos - 1] : 0, next = has_next ? cps[pos] : 0;
    switch (kind) {
        case kStartText: return pos == 0;
        case kEndText: return pos == cps.size();
        case kStartLine: return pos == 0 || prev == '\n';
        case kEndLine: return pos == cps.size() || next == '\n';
        case kStartLineCrlf: return pos == 0 || prev == '\n' || (prev == '\r' && !(has_next && next == '\n'));
        case kEndLineCrlf: return pos == cps.size() || next == '\r' || (next == '\n' && !(has_prev && prev == '\r'));
        default: break;
    }
    const bool ascii = kind >= kAsciiOffset;
    const uint32_t k = ascii ? kind - kAsciiOffset : kind;
    const bool wp = has_prev && (ascii ? is_ascii_word(prev) : has(word_set(), prev));
    const bool wn = has_next && (ascii ? is_ascii_word(next) : has(word_set(), next));
    switch (k) {
        case kWord: return wp != wn;
        case kNotWord: return wp == wn;
        case kWordStart: return !wp && wn;
        case kWordEnd: return wp && !wn;
        case kWordStartHalf: return !wp;
        case kWordEndHalf: return !wn;
        default: return false;
    }
}

} // namespace

bool Pattern::is_match(std::string_view text) const {
    std::vector<uint32_t> cps;
    cps.reserve(text.size());
    for (size_t i = 0; i < text.size();) {
        size_t len = 0;
        cps.push_back(decode_utf8(text, i, len));
        i += len == 0 ? 1 : len;
    }
    const size_t n = prog_.size();
    SparseSet clist(n), nlist(n);
    std::vector<uint32_t> stack;
    // Add `start` and everything reachable from it without reading a code
    // point; true when that reaches the end of the program.
    auto close = [&](SparseSet& set, uint32_t start, size_t pos) {
        stack.push_back(start);
        while (!stack.empty()) {
            const uint32_t pc = stack.back();
            stack.pop_back();
            if (set.contains(pc)) continue;
            set.add(pc);
            const Inst& in = prog_[pc];
            switch (in.op) {
                case kMatch:
                    stack.clear();
                    return true;
                case kJmp:
                    stack.push_back(in.a);
                    break;
                case kSplit:
                    stack.push_back(in.b);
                    stack.push_back(in.a);
                    break;
                case kAssert:
                    if (holds(in.a, cps, pos)) stack.push_back(pc + 1);
                    break;
                default:
                    break;
            }
        }
        return false;
    };
    for (size_t pos = 0; pos <= cps.size(); ++pos) {
        if (pos == 0 || !anchored_start_) {
            if (close(clist, 0, pos)) return true;
        }
        if (clist.size() == 0) {
            if (anchored_start_) return false;
            continue;
        }
        if (pos == cps.size()) break;
        const uint32_t c = cps[pos];
        nlist.clear();
        for (size_t k = 0; k < clist.size(); ++k) {
            const uint32_t pc = clist.at(k);
            if (prog_[pc].op == kChar && has(sets_[prog_[pc].a], c)) {
                if (close(nlist, pc + 1, pos + 1)) return true;
            }
        }
        std::swap(clist, nlist);
    }
    return false;
}

} // namespace detail
} // namespace synx
