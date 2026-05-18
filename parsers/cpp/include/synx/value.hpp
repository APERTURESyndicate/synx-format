// SYNX Value — tagged-union, no exceptions, no RTTI.
// Parity with crates/synx-core/src/value.rs.
#pragma once

#include <cstdint>
#include <string>
#include <string_view>
#include <vector>

namespace synx {

class Value;

struct Pair;

/// Insertion-ordered key/value list — chosen over std::map for incomplete-type safety
/// and cache friendliness on small config objects. O(n) lookup is acceptable for
/// typical configs (<100 keys).
using Object = std::vector<Pair>;

/// Sequence of values.
using Array = std::vector<Value>;

class Value {
public:
    enum class Kind : uint8_t {
        Null,
        Bool,
        Int,
        Float,
        String,
        Array,
        Object,
        /// Redacted in JSON / stringify output as `[SECRET]`.
        Secret,
    };

    Value() noexcept;
    Value(const Value& other);
    Value(Value&& other) noexcept;
    ~Value();
    Value& operator=(const Value& other);
    Value& operator=(Value&& other) noexcept;

    // ─── factories ────────────────────────────────────────────────────────────
    static Value make_null() noexcept;
    static Value make_bool(bool b) noexcept;
    static Value make_int(int64_t i) noexcept;
    static Value make_float(double f) noexcept;
    static Value make_string(std::string s);
    static Value make_secret(std::string s);
    static Value make_array(Array a = {});
    static Value make_object(Object o = {});

    Kind kind() const noexcept { return kind_; }

    // ─── type checks ─────────────────────────────────────────────────────────
    bool is_null() const noexcept { return kind_ == Kind::Null; }
    bool is_bool() const noexcept { return kind_ == Kind::Bool; }
    bool is_int() const noexcept { return kind_ == Kind::Int; }
    bool is_float() const noexcept { return kind_ == Kind::Float; }
    bool is_number() const noexcept { return kind_ == Kind::Int || kind_ == Kind::Float; }
    bool is_string() const noexcept { return kind_ == Kind::String; }
    bool is_array() const noexcept { return kind_ == Kind::Array; }
    bool is_object() const noexcept { return kind_ == Kind::Object; }
    bool is_secret() const noexcept { return kind_ == Kind::Secret; }

    // ─── accessors (return nullptr on type mismatch) ─────────────────────────
    const bool* as_bool() const noexcept;
    const int64_t* as_int() const noexcept;
    const double* as_float() const noexcept;
    const std::string* as_string() const noexcept;
    const std::string* as_secret() const noexcept;
    const Array* as_array() const noexcept;
    Array* as_array_mut() noexcept;
    const Object* as_object() const noexcept;
    Object* as_object_mut() noexcept;

    // ─── numeric coercion ────────────────────────────────────────────────────
    /// Returns the value as f64. Int converted lossily; otherwise NaN.
    double as_number_f64(double fallback = 0.0) const noexcept;

    // ─── object lookup helpers ──────────────────────────────────────────────
    /// Returns pointer to value for `key` if `kind() == Object` and key is present.
    const Value* get(std::string_view key) const noexcept;
    Value* get_mut(std::string_view key) noexcept;
    bool contains(std::string_view key) const noexcept;
    /// Insert or overwrite. Only effective when kind() == Object.
    void set(std::string key, Value v);
    /// Remove. Returns true if the key existed.
    bool remove(std::string_view key) noexcept;

    // ─── equality ────────────────────────────────────────────────────────────
    bool equals(const Value& other) const noexcept;
    friend bool operator==(const Value& a, const Value& b) noexcept { return a.equals(b); }
    friend bool operator!=(const Value& a, const Value& b) noexcept { return !a.equals(b); }

    /// Diagnostic name, e.g. "string", "int", "object". For error messages.
    const char* type_name() const noexcept;

private:
    void clear() noexcept;
    void copy_from(const Value& other);
    void move_from(Value&& other) noexcept;

    Kind kind_;
    union {
        bool b_;
        int64_t i_;
        double f_;
    };
    // Non-trivial members live outside the union and are conditionally valid.
    std::string str_;   // String / Secret
    Array arr_;         // Array
    Object obj_;        // Object — forward-declared `Pair`, vector<incomplete> OK in C++17
};

/// Object entry (key/value). Defined after Value so vector<Pair> can be used by Value.
struct Pair {
    std::string key;
    Value value;
};

/// File-level mode.
enum class Mode : uint8_t {
    Static,
    Active,
};

} // namespace synx
