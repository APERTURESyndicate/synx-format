// Value tagged-union implementation. Mirror of crates/synx-core/src/value.rs.
#include "synx/value.hpp"

#include <cmath>
#include <new>
#include <utility>

namespace synx {

Value::Value() noexcept : kind_(Kind::Null), i_(0) {}

Value::Value(const Value& other) : kind_(Kind::Null), i_(0) { copy_from(other); }

Value::Value(Value&& other) noexcept : kind_(Kind::Null), i_(0) { move_from(std::move(other)); }

Value::~Value() { destroy(); }

// Both assignments go through a temporary first, so assigning a value from a
// part of itself (`v = v.as_array()[0]`) is safe.
Value& Value::operator=(const Value& other) {
    if (this != &other) {
        Value tmp(other);
        destroy();
        move_from(std::move(tmp));
    }
    return *this;
}

Value& Value::operator=(Value&& other) noexcept {
    if (this != &other) {
        Value tmp(std::move(other));
        destroy();
        move_from(std::move(tmp));
    }
    return *this;
}

void Value::destroy() noexcept {
    switch (kind_) {
        case Kind::String:
        case Kind::Secret: str_.~basic_string(); break;
        case Kind::Array: arr_.~Array(); break;
        case Kind::Object: obj_.~Object(); break;
        default: break;
    }
    kind_ = Kind::Null;
    i_ = 0;
}

void Value::copy_from(const Value& other) {
    switch (other.kind_) {
        case Kind::Null: i_ = 0; break;
        case Kind::Bool: b_ = other.b_; break;
        case Kind::Int: i_ = other.i_; break;
        case Kind::Float: f_ = other.f_; break;
        case Kind::String:
        case Kind::Secret: new (&str_) std::string(other.str_); break;
        case Kind::Array: new (&arr_) Array(other.arr_); break;
        case Kind::Object: new (&obj_) Object(other.obj_); break;
    }
    kind_ = other.kind_;
}

void Value::move_from(Value&& other) noexcept {
    switch (other.kind_) {
        case Kind::Null: i_ = 0; break;
        case Kind::Bool: b_ = other.b_; break;
        case Kind::Int: i_ = other.i_; break;
        case Kind::Float: f_ = other.f_; break;
        case Kind::String:
        case Kind::Secret: new (&str_) std::string(std::move(other.str_)); break;
        case Kind::Array: new (&arr_) Array(std::move(other.arr_)); break;
        case Kind::Object: new (&obj_) Object(std::move(other.obj_)); break;
    }
    kind_ = other.kind_;
    other.destroy();
}

// ─── factories ────────────────────────────────────────────────────────────────
Value Value::make_null() noexcept { return Value{}; }

Value Value::make_bool(bool b) noexcept {
    Value v;
    v.kind_ = Kind::Bool;
    v.b_ = b;
    return v;
}

Value Value::make_int(int64_t i) noexcept {
    Value v;
    v.kind_ = Kind::Int;
    v.i_ = i;
    return v;
}

Value Value::make_float(double f) noexcept {
    Value v;
    v.kind_ = Kind::Float;
    v.f_ = f;
    return v;
}

Value Value::make_string(std::string s) {
    Value v;
    new (&v.str_) std::string(std::move(s));
    v.kind_ = Kind::String;
    return v;
}

Value Value::make_secret(std::string s) {
    Value v;
    new (&v.str_) std::string(std::move(s));
    v.kind_ = Kind::Secret;
    return v;
}

Value Value::make_array(Array a) {
    Value v;
    new (&v.arr_) Array(std::move(a));
    v.kind_ = Kind::Array;
    return v;
}

Value Value::make_object(Object o) {
    Value v;
    new (&v.obj_) Object(std::move(o));
    v.kind_ = Kind::Object;
    return v;
}

// ─── accessors ────────────────────────────────────────────────────────────────
const bool* Value::as_bool() const noexcept {
    return kind_ == Kind::Bool ? &b_ : nullptr;
}
const int64_t* Value::as_int() const noexcept {
    return kind_ == Kind::Int ? &i_ : nullptr;
}
const double* Value::as_float() const noexcept {
    return kind_ == Kind::Float ? &f_ : nullptr;
}
const std::string* Value::as_string() const noexcept {
    return kind_ == Kind::String ? &str_ : nullptr;
}
const std::string* Value::as_secret() const noexcept {
    return kind_ == Kind::Secret ? &str_ : nullptr;
}
const Array* Value::as_array() const noexcept {
    return kind_ == Kind::Array ? &arr_ : nullptr;
}
Array* Value::as_array_mut() noexcept {
    return kind_ == Kind::Array ? &arr_ : nullptr;
}
const Object* Value::as_object() const noexcept {
    return kind_ == Kind::Object ? &obj_ : nullptr;
}
Object* Value::as_object_mut() noexcept {
    return kind_ == Kind::Object ? &obj_ : nullptr;
}

double Value::as_number_f64(double fallback) const noexcept {
    switch (kind_) {
        case Kind::Int:   return static_cast<double>(i_);
        case Kind::Float: return f_;
        default:          return fallback;
    }
}

// ─── object lookup ───────────────────────────────────────────────────────────
const Value* Value::get(std::string_view key) const noexcept {
    if (kind_ != Kind::Object) {
        return nullptr;
    }
    for (const auto& p : obj_) {
        if (p.key == key) {
            return &p.value;
        }
    }
    return nullptr;
}

Value* Value::get_mut(std::string_view key) noexcept {
    if (kind_ != Kind::Object) {
        return nullptr;
    }
    for (auto& p : obj_) {
        if (p.key == key) {
            return &p.value;
        }
    }
    return nullptr;
}

bool Value::contains(std::string_view key) const noexcept {
    return get(key) != nullptr;
}

void Value::set(std::string key, Value v) {
    if (kind_ != Kind::Object) {
        return;
    }
    for (auto& p : obj_) {
        if (p.key == key) {
            p.value = std::move(v);
            return;
        }
    }
    obj_.push_back(Pair{std::move(key), std::move(v)});
}

bool Value::remove(std::string_view key) noexcept {
    if (kind_ != Kind::Object) {
        return false;
    }
    for (auto it = obj_.begin(); it != obj_.end(); ++it) {
        if (it->key == key) {
            obj_.erase(it);
            return true;
        }
    }
    return false;
}

// ─── equality ────────────────────────────────────────────────────────────────
bool Value::equals(const Value& other) const noexcept {
    if (kind_ != other.kind_) {
        return false;
    }
    switch (kind_) {
        case Kind::Null:
            return true;
        case Kind::Bool:
            return b_ == other.b_;
        case Kind::Int:
            return i_ == other.i_;
        case Kind::Float:
            // Bitwise compare for determinism — NaN != NaN, matches Rust f64 PartialEq.
            return f_ == other.f_;
        case Kind::String:
        case Kind::Secret:
            return str_ == other.str_;
        case Kind::Array: {
            if (arr_.size() != other.arr_.size()) {
                return false;
            }
            for (size_t i = 0; i < arr_.size(); ++i) {
                if (!arr_[i].equals(other.arr_[i])) {
                    return false;
                }
            }
            return true;
        }
        case Kind::Object: {
            if (obj_.size() != other.obj_.size()) {
                return false;
            }
            // Order-insensitive comparison (matches Rust HashMap == HashMap).
            for (const auto& a : obj_) {
                bool found = false;
                for (const auto& b : other.obj_) {
                    if (a.key == b.key) {
                        if (!a.value.equals(b.value)) {
                            return false;
                        }
                        found = true;
                        break;
                    }
                }
                if (!found) {
                    return false;
                }
            }
            return true;
        }
    }
    return false;
}

const char* Value::type_name() const noexcept {
    switch (kind_) {
        case Kind::Null:   return "null";
        case Kind::Bool:   return "bool";
        case Kind::Int:    return "int";
        case Kind::Float:  return "float";
        case Kind::String: return "string";
        case Kind::Array:  return "array";
        case Kind::Object: return "object";
        case Kind::Secret: return "secret";
    }
    return "unknown";
}

} // namespace synx
