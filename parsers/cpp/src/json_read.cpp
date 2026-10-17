// JSON text → Value for `:watch` on a `.json` file — serde_json's reading:
// integers that fit i64 are Int (a larger u64 and `-0` are Float), objects
// come out with sorted keys and the last duplicate wins, depth is capped at 128.
#include "engine_internal.hpp"

#include "numfmt.hpp"
#include "unicode.hpp"

#include <algorithm>
#include <cmath>
#include <string>
#include <string_view>

namespace synx {
namespace detail {

namespace {

class JsonReader {
public:
    explicit JsonReader(std::string_view s) : s_(s) {}

    bool document(Value& out) {
        if (!value(out, 0)) return false;
        skip_ws();
        return pos_ == s_.size();
    }

private:
    void skip_ws() {
        while (pos_ < s_.size() && (s_[pos_] == ' ' || s_[pos_] == '\t' || s_[pos_] == '\n' || s_[pos_] == '\r')) ++pos_;
    }

    bool literal(std::string_view word) {
        if (s_.substr(pos_, word.size()) != word) return false;
        pos_ += word.size();
        return true;
    }

    bool value(Value& out, size_t depth) {
        skip_ws();
        if (pos_ >= s_.size()) return false;
        char c = s_[pos_];
        if (c == 'n') { out = Value::make_null(); return literal("null"); }
        if (c == 't') { out = Value::make_bool(true); return literal("true"); }
        if (c == 'f') { out = Value::make_bool(false); return literal("false"); }
        if (c == '"') {
            std::string str;
            if (!string(str)) return false;
            out = Value::make_string(std::move(str));
            return true;
        }
        if (c == '[') {
            if (depth >= 128) return false;
            ++pos_;
            Array arr;
            skip_ws();
            if (pos_ < s_.size() && s_[pos_] == ']') { ++pos_; out = Value::make_array(std::move(arr)); return true; }
            while (true) {
                Value item;
                if (!value(item, depth + 1)) return false;
                arr.push_back(std::move(item));
                skip_ws();
                if (pos_ >= s_.size()) return false;
                if (s_[pos_] == ',') { ++pos_; continue; }
                if (s_[pos_] == ']') { ++pos_; break; }
                return false;
            }
            out = Value::make_array(std::move(arr));
            return true;
        }
        if (c == '{') {
            if (depth >= 128) return false;
            ++pos_;
            Object obj;
            skip_ws();
            if (pos_ < s_.size() && s_[pos_] == '}') { ++pos_; out = Value::make_object(std::move(obj)); return true; }
            while (true) {
                skip_ws();
                std::string k;
                if (pos_ >= s_.size() || s_[pos_] != '"' || !string(k)) return false;
                skip_ws();
                if (pos_ >= s_.size() || s_[pos_] != ':') return false;
                ++pos_;
                Value v;
                if (!value(v, depth + 1)) return false;
                auto it = std::find_if(obj.begin(), obj.end(), [&](const Pair& p) { return p.key == k; });
                if (it != obj.end()) it->value = std::move(v);
                else obj.push_back(Pair{std::move(k), std::move(v)});
                skip_ws();
                if (pos_ >= s_.size()) return false;
                if (s_[pos_] == ',') { ++pos_; continue; }
                if (s_[pos_] == '}') { ++pos_; break; }
                return false;
            }
            // serde_json's default map is a BTreeMap.
            std::sort(obj.begin(), obj.end(), [](const Pair& a, const Pair& b) { return a.key < b.key; });
            out = Value::make_object(std::move(obj));
            return true;
        }
        return number(out);
    }

    bool hex4(uint32_t& out) {
        if (pos_ + 4 > s_.size()) return false;
        out = 0;
        for (size_t k = 0; k < 4; ++k) {
            char h = s_[pos_ + k];
            uint32_t d = (h >= '0' && h <= '9') ? static_cast<uint32_t>(h - '0')
                       : (h >= 'a' && h <= 'f') ? static_cast<uint32_t>(h - 'a' + 10)
                       : (h >= 'A' && h <= 'F') ? static_cast<uint32_t>(h - 'A' + 10) : 16u;
            if (d > 15) return false;
            out = out * 16 + d;
        }
        pos_ += 4;
        return true;
    }

    bool string(std::string& out) {
        ++pos_;  // opening quote
        while (pos_ < s_.size()) {
            unsigned char c = static_cast<unsigned char>(s_[pos_]);
            if (c == '"') { ++pos_; return true; }
            if (c < 0x20) return false;
            if (c != '\\') { out.push_back(static_cast<char>(c)); ++pos_; continue; }
            if (++pos_ >= s_.size()) return false;
            char e = s_[pos_++];
            switch (e) {
                case '"': out.push_back('"'); break;
                case '\\': out.push_back('\\'); break;
                case '/': out.push_back('/'); break;
                case 'b': out.push_back('\b'); break;
                case 'f': out.push_back('\f'); break;
                case 'n': out.push_back('\n'); break;
                case 'r': out.push_back('\r'); break;
                case 't': out.push_back('\t'); break;
                case 'u': {
                    uint32_t cp = 0;
                    if (!hex4(cp)) return false;
                    if (cp >= 0xDC00 && cp <= 0xDFFF) return false;  // lone trailing surrogate
                    if (cp >= 0xD800 && cp <= 0xDBFF) {
                        uint32_t lo = 0;
                        if (s_.substr(pos_, 2) != "\\u") return false;
                        pos_ += 2;
                        if (!hex4(lo) || lo < 0xDC00 || lo > 0xDFFF) return false;
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                    }
                    push_utf8(out, cp);
                    break;
                }
                default: return false;
            }
        }
        return false;
    }

    bool number(Value& out) {
        size_t start = pos_;
        bool neg = false;
        if (pos_ < s_.size() && s_[pos_] == '-') { neg = true; ++pos_; }
        size_t int_start = pos_;
        if (pos_ >= s_.size() || !(s_[pos_] >= '0' && s_[pos_] <= '9')) return false;
        if (s_[pos_] == '0') {
            ++pos_;
            if (pos_ < s_.size() && s_[pos_] >= '0' && s_[pos_] <= '9') return false;
        } else {
            while (pos_ < s_.size() && s_[pos_] >= '0' && s_[pos_] <= '9') ++pos_;
        }
        size_t int_end = pos_;
        bool is_float = false;
        if (pos_ < s_.size() && s_[pos_] == '.') {
            is_float = true;
            ++pos_;
            size_t d = pos_;
            while (pos_ < s_.size() && s_[pos_] >= '0' && s_[pos_] <= '9') ++pos_;
            if (pos_ == d) return false;
        }
        if (pos_ < s_.size() && (s_[pos_] == 'e' || s_[pos_] == 'E')) {
            is_float = true;
            ++pos_;
            if (pos_ < s_.size() && (s_[pos_] == '+' || s_[pos_] == '-')) ++pos_;
            size_t d = pos_;
            while (pos_ < s_.size() && s_[pos_] >= '0' && s_[pos_] <= '9') ++pos_;
            if (pos_ == d) return false;
        }
        std::string_view text = s_.substr(start, pos_ - start);
        if (!is_float) {
            uint64_t mag = 0;
            if (parse_u64(s_.substr(int_start, int_end - int_start), mag)) {
                if (!neg && mag <= static_cast<uint64_t>(INT64_MAX)) { out = Value::make_int(static_cast<int64_t>(mag)); return true; }
                if (neg && mag != 0 && mag <= 9223372036854775808ull) {
                    out = Value::make_int(static_cast<int64_t>(0 - mag));
                    return true;
                }
            }
        }
        double f = 0.0;
        // serde_json refuses a number out of f64 range.
        if (!parse_f64(text, f) || std::isinf(f)) return false;
        out = Value::make_float(f);
        return true;
    }

    std::string_view s_;
    size_t pos_ = 0;
};

} // namespace

std::optional<Value> parse_json(std::string_view text) {
    Value v;
    JsonReader r(text);
    if (!r.document(v)) return std::nullopt;
    return v;
}

} // namespace detail
} // namespace synx
