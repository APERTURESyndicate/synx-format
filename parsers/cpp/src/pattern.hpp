// `[pattern:…]`: the syntax of the `regex` crate synx-core checks patterns
// with, matched in time linear in the value.
#pragma once

#include <cstdint>
#include <memory>
#include <string_view>
#include <utility>
#include <vector>

namespace synx {
namespace detail {

/// A compiled `[pattern]`.
///
/// safety: `std::regex` backtracks — `^(a|aa)+$` against sixty `a` and a `!`
/// did not answer in 20 s — and reads the ECMAScript dialect: `(?i)` and
/// `\p{L}` are errors there, `{` a literal. A pattern synx-core does not
/// compile is not checked there, so a dialect gap was a value accepted by one
/// implementation and refused by the other. This is a Thompson NFA run as a
/// Pike VM over code points, with the crate's Unicode tables and simple case
/// folding: every step costs at most the size of the program.
class Pattern {
public:
    /// The pattern compiled, or null when the `regex` crate would refuse it.
    static std::unique_ptr<Pattern> compile(std::string_view source);

    /// Whether the pattern matches anywhere in `text` (Rust `Regex::is_match`).
    bool is_match(std::string_view text) const;

    using Ranges = std::vector<std::pair<uint32_t, uint32_t>>;

    struct Inst {
        uint8_t op;
        uint32_t a;
        uint32_t b;
    };

private:
    std::vector<Inst> prog_;
    std::vector<Ranges> sets_;
    bool anchored_start_ = false;
    friend struct PatternBuilder;
};

} // namespace detail
} // namespace synx
